//! Transient R-033 diagnostic interaction for one Actions editor and checked source snapshot.
use super::{diagnostic_severity, jump_to};
use crate::theme::ScrollTheme as _;
use eframe::egui::{self, text::CCursor};
use neoflash_script::{Diagnostic, QuickFix};
use neoflash_ui::{components as nf, tokens::space};

#[derive(Clone, Default)]
pub(super) struct DiagnosticUi {
    fingerprint: Option<u64>,
    last_frame: u64,
    peek: Option<usize>,
    pinned: bool,
    caret: usize,
    menu: Option<FixMenu>,
    popup_rect: Option<egui::Rect>,
    scroll: bool,
    dismissed: bool,
    pointer: Option<egui::Pos2>,
}

#[derive(Clone)]
struct FixMenu {
    fixes: Vec<QuickFix>,
    code: String,
    selected: usize,
    scroll: bool,
}

pub(super) enum Action {
    Fix(QuickFix),
    CompilerErrors,
    NoFix,
}

pub(super) fn state_id(id: egui::Id) -> egui::Id {
    id.with("diagnostic-ui")
}

pub(super) fn char_range(source: &str, diagnostic: &Diagnostic) -> std::ops::Range<usize> {
    let (start, end) = diagnostic.span;
    if end > start && source.is_char_boundary(start) && source.is_char_boundary(end) {
        source[..start].chars().count()..source[..end].chars().count()
    } else {
        let start = nf::code::char_index(source, diagnostic.line, diagnostic.column);
        let end = nf::code::char_index(
            source,
            diagnostic.end_line.max(diagnostic.line),
            diagnostic.end_column,
        );
        start..end.max(start)
    }
}

fn safe_fixes<'a>(
    source: &str,
    diagnostics: impl Iterator<Item = &'a Diagnostic>,
) -> Vec<QuickFix> {
    let mut fixes = Vec::new();
    for diagnostic in diagnostics {
        for fix in &diagnostic.fixes {
            // The compiler supplies only safe transformations. Reject invalid/no-op edits too.
            if !fixes.contains(fix)
                && crate::code_tools::apply_quick_fix_edit(source, fix).is_some()
            {
                fixes.push(fix.clone());
            }
        }
    }
    fixes
}

// egui's consume_key deliberately ignores extra Shift/Alt modifiers. F8 has distinct approved routes.
fn consume_navigation(ctx: &egui::Context, modifiers: egui::Modifiers, key: egui::Key) -> bool {
    ctx.input_mut(|input| {
        let mut consumed = false;
        input.events.retain(|event| {
            let take = matches!(event, egui::Event::Key { key: k, modifiers: m, pressed: true, .. } if *k == key && *m == modifiers);
            consumed |= take;
            !take
        });
        consumed
    })
}

impl DiagnosticUi {
    pub(super) fn load(ctx: &egui::Context, id: egui::Id, fingerprint: Option<u64>) -> Self {
        let mut state: Self = ctx.data(|d| d.get_temp(state_id(id))).unwrap_or_default();
        let frame = ctx.cumulative_frame_nr();
        if fingerprint.is_none() || state.fingerprint != fingerprint || frame > state.last_frame + 1
        {
            state = Self::default();
        }
        state.fingerprint = fingerprint;
        state.last_frame = frame;
        if state.pointer != ctx.pointer_hover_pos() {
            state.dismissed = false;
        }
        state
    }

    pub(super) fn store(self, ctx: &egui::Context, id: egui::Id) {
        ctx.data_mut(|d| d.insert_temp(state_id(id), self));
    }

    pub(super) fn is_open(&self) -> bool {
        self.menu.is_some() || self.peek.is_some()
    }

    pub(super) fn close(&mut self) {
        self.menu = None;
        self.peek = None;
        self.popup_rect = None;
        self.pinned = false;
        self.dismissed = true;
    }

    fn offer(&mut self, source: &str, diagnostics: &[&Diagnostic]) -> Option<Action> {
        self.dismissed = false;
        let fixes = safe_fixes(source, diagnostics.iter().copied());
        match fixes.as_slice() {
            [] => Some(Action::NoFix),
            [fix] => {
                let fix = fix.clone();
                self.close();
                Some(Action::Fix(fix))
            }
            _ => {
                let mut codes = Vec::new();
                for diagnostic in diagnostics {
                    if let Some(code) = &diagnostic.code
                        && !codes.contains(code)
                    {
                        codes.push(code.clone());
                    }
                }
                self.menu = Some(FixMenu {
                    fixes,
                    code: if codes.is_empty() {
                        "diagnostic".into()
                    } else {
                        codes.join(", ")
                    },
                    selected: 0,
                    scroll: true,
                });
                self.peek = None;
                self.popup_rect = None;
                None
            }
        }
    }

    /// Consume popup/navigation keys before TextEdit sees them. Feedback is already current.
    pub(super) fn before_editor(
        &mut self,
        ctx: &egui::Context,
        id: egui::Id,
        source: &str,
        diagnostics: &[Diagnostic],
    ) -> Option<Action> {
        let focused = ctx.memory(|m| m.has_focus(id));
        let caret = egui::text_edit::TextEditState::load(ctx, id)
            .and_then(|s| s.cursor.char_range())
            .map_or(0, |r| r.primary.index);
        let over_popup = ctx
            .pointer_hover_pos()
            .is_some_and(|p| self.popup_rect.is_some_and(|r| r.contains(p)));
        if self.is_open()
            && (caret != self.caret
                || (!focused && !over_popup && (self.pinned || self.menu.is_some())))
        {
            self.close();
        }
        self.caret = caret;
        if !focused {
            return None;
        }
        if self.is_open()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.close();
            return None;
        }
        let previous = consume_navigation(ctx, egui::Modifiers::SHIFT, egui::Key::F8);
        let next = !previous && consume_navigation(ctx, egui::Modifiers::NONE, egui::Key::F8);
        if (previous || next) && !diagnostics.is_empty() {
            let mut order: Vec<_> = (0..diagnostics.len()).collect();
            order.sort_by_key(|&index| char_range(source, &diagnostics[index]).start);
            let current = self
                .peek
                .and_then(|index| order.iter().position(|&i| i == index));
            let selected = if let Some(current) = current {
                order[if previous {
                    (current + order.len() - 1) % order.len()
                } else {
                    (current + 1) % order.len()
                }]
            } else if previous {
                *order
                    .iter()
                    .rev()
                    .find(|&&i| char_range(source, &diagnostics[i]).start < caret)
                    .unwrap_or(order.last().unwrap())
            } else {
                *order
                    .iter()
                    .find(|&&i| char_range(source, &diagnostics[i]).start >= caret)
                    .unwrap_or(&order[0])
            };
            let diagnostic = &diagnostics[selected];
            jump_to(ctx, id, source, diagnostic.line, diagnostic.column);
            self.caret = nf::code::char_index(source, diagnostic.line, diagnostic.column);
            self.menu = None;
            self.peek = Some(selected);
            self.pinned = true;
            self.scroll = true;
            self.dismissed = false;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Period)) {
            let (line, _) = nf::code::line_col(source, self.caret);
            return self.offer(
                source,
                &diagnostics
                    .iter()
                    .filter(|d| d.line <= line && line <= d.end_line.max(d.line))
                    .collect::<Vec<_>>(),
            );
        }
        if let Some(menu) = &mut self.menu {
            let count = menu.fixes.len() + 1; // Compiler errors is a selectable menu item too.
            ctx.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    menu.selected = (menu.selected + 1) % count;
                    menu.scroll = true;
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    menu.selected = (menu.selected + count - 1) % count;
                    menu.scroll = true;
                }
            });
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                let action = menu
                    .fixes
                    .get(menu.selected)
                    .cloned()
                    .map_or(Action::CompilerErrors, Action::Fix);
                self.close();
                return Some(action);
            }
        }
        None
    }

    pub(super) fn paint(
        &mut self,
        ui: &mut egui::Ui,
        id: egui::Id,
        source: &str,
        diagnostics: &[Diagnostic],
        output: &egui::text_edit::TextEditOutput,
        text_clip: egui::Rect,
    ) -> Option<Action> {
        if output.response.changed() {
            self.close();
            return None;
        }
        let ctx = ui.ctx().clone();
        let pointer = ctx.pointer_hover_pos();
        self.pointer = pointer;
        let painter = ui.painter().with_clip_rect(text_clip);
        let mut hit = None;
        let mut anchor_rect = None;
        for (index, diagnostic) in diagnostics.iter().enumerate() {
            for rect in range_rects(source, diagnostic, &output.galley, output.galley_pos) {
                if !rect.intersects(text_clip) {
                    continue;
                }
                nf::paint_range_underline(
                    &painter,
                    rect.x_range(),
                    rect.bottom(),
                    diagnostic_severity(diagnostic.severity),
                );
                let rect = rect.intersect(text_clip);
                if pointer.is_some_and(|p| rect.contains(p)) && output.response.contains_pointer() {
                    hit.get_or_insert(index);
                }
                if self.peek == Some(index) && anchor_rect.is_none() {
                    anchor_rect = Some(rect);
                }
            }
        }
        if !self.dismissed
            && self.menu.is_none()
            && !self.pinned
            && let Some(index) = hit
        {
            self.peek = Some(index);
            anchor_rect = range_rects(
                source,
                &diagnostics[index],
                &output.galley,
                output.galley_pos,
            )
            .into_iter()
            .find(|r| r.intersects(text_clip))
            .map(|r| r.intersect(text_clip));
            self.caret = output
                .state
                .cursor
                .char_range()
                .map_or(0, |r| r.primary.index);
        }
        let caret_rect = output
            .galley
            .pos_from_cursor(CCursor::new(self.caret))
            .translate(output.galley_pos.to_vec2());
        if self.scroll {
            ui.scroll_to_rect(caret_rect, None);
            self.scroll = false;
        }
        let anchor = anchor_rect.unwrap_or(caret_rect);
        let popup_hovered =
            pointer.is_some_and(|p| self.popup_rect.is_some_and(|r| r.union(anchor).contains(p)));
        if ctx.input(|i| i.pointer.any_pressed()) && !popup_hovered {
            self.close();
        }
        if self.peek.is_some() && !self.pinned && hit.is_none() && !popup_hovered {
            self.close();
        }
        if !self.is_open() {
            return None;
        }
        let pos = egui::pos2(
            anchor.left().clamp(text_clip.left(), text_clip.right()),
            (anchor.bottom() + space::S1).clamp(text_clip.top(), text_clip.bottom()),
        );
        let mut action = None;
        let mut requested = None;
        let area = egui::Area::new(id.with("diagnostic-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .constrain(true)
            .show(&ctx, |ui| {
                if let Some(menu) = &mut self.menu {
                    nf::popover_frame(&ctx).show(ui, |ui| {
                        ui.set_width(330.0); // Approved R-033 menu body (shared gallery composition).
                        nf::menu::menu_style(ui.style_mut());
                        nf::menu_section(
                            ui,
                            &format!("{} fixes for {}", menu.fixes.len(), menu.code),
                        );
                        egui::ScrollArea::vertical()
                            .max_height(8. * neoflash_ui::metrics::row())
                            .themed()
                            .show(ui, |ui| {
                                for (index, fix) in menu.fixes.iter().enumerate() {
                                    let selected = menu.selected == index;
                                    let row = ui.add(
                                        nf::MenuItem::new(&fix.title)
                                            .highlighted(selected)
                                            .shortcut(if selected { "↩" } else { "" }),
                                    );
                                    if selected && menu.scroll {
                                        row.scroll_to_me(None);
                                    }
                                    if row.hovered()
                                        && ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO)
                                    {
                                        menu.selected = index;
                                    }
                                    if row.clicked() {
                                        action = Some(Action::Fix(fix.clone()));
                                    }
                                }
                            });
                        menu.scroll = false;
                        nf::menu_separator(ui);
                        let selected = menu.selected == menu.fixes.len();
                        if ui
                            .add(
                                nf::MenuItem::new("Show in Compiler errors")
                                    .highlighted(selected)
                                    .shortcut(if selected { "↩" } else { "" }),
                            )
                            .clicked()
                        {
                            action = Some(Action::CompilerErrors);
                        }
                    });
                } else if let Some(index) = self.peek
                    && let Some(diagnostic) = diagnostics.get(index)
                {
                    let fixes = safe_fixes(source, std::iter::once(diagnostic));
                    let label = if fixes.len() == 1 {
                        "Quick fix ⌘.".into()
                    } else {
                        format!("{} fixes ⌘.", fixes.len())
                    };
                    let message = diagnostic.code.as_ref().map_or_else(
                        || diagnostic.message.clone(),
                        |code| format!("[{code}] {}", diagnostic.message),
                    );
                    let mut peek =
                        nf::ErrorPeek::new(diagnostic_severity(diagnostic.severity), &message)
                            .max_width(text_clip.width());
                    if !fixes.is_empty() {
                        peek = peek.action(&label);
                    }
                    let result = peek.show(ui);
                    // The shared compact component truncates long text; retain its full explanation on hover.
                    result.response.clone().on_hover_text(&message);
                    if result.action_clicked() {
                        requested = Some(index);
                    }
                }
            });
        self.popup_rect = Some(area.response.rect);
        if let Some(index) = requested {
            ctx.memory_mut(|m| m.request_focus(id));
            action = self.offer(source, &[&diagnostics[index]]);
        }
        if action.is_some() {
            self.close();
        }
        action
    }
}

/// Use shaped rows, rather than font-width estimates, for tabs, Unicode, soft wraps and scrolling.
pub(super) fn range_rects(
    source: &str,
    diagnostic: &Diagnostic,
    galley: &egui::Galley,
    origin: egui::Pos2,
) -> Vec<egui::Rect> {
    let range = char_range(source, diagnostic);
    let mut offset = 0;
    let mut rects = Vec::new();
    for row in &galley.rows {
        let end = offset + row.char_count_excluding_newline();
        let start = range.start.max(offset);
        let stop = range.end.min(end);
        if start < stop
            || (range.is_empty() && offset <= start && start <= end)
            || (start == end && row.ends_with_newline && range.start <= end && range.end > end)
        {
            let left = row.pos.x + row.x_offset(start - offset);
            let right = row.pos.x + row.x_offset(stop.max(start) - offset);
            rects.push(egui::Rect::from_min_max(
                origin + egui::vec2(left, row.pos.y),
                origin + egui::vec2(right.max(left + space::S1), row.pos.y + row.height()),
            ));
            if range.is_empty() {
                break;
            }
        }
        offset += row.char_count_including_newline();
    }
    rects
}

#[cfg(test)]
#[path = "sources_diagnostics_tests.rs"]
mod tests;
