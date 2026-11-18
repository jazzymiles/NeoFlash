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
            (anchor.bottom() + space::s1()).clamp(text_clip.top(), text_clip.bottom()),
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
                        // R-033 quick-fix menu body: one long field plus one short field wide.
                        ui.set_width(
                            neoflash_ui::metrics::field_l() + neoflash_ui::metrics::field_s(),
                        );
                        nf::menu::menu_style(ui.style_mut());
                        nf::menu_section(
                            ui,
                            &format!("{} fixes for {}", menu.fixes.len(), menu.code),
                        );
                        // Up to eight fixes; the list never shrinks below its rows while it is open.
                        let rows = menu.fixes.len().clamp(1, 8) as f32;
                        egui::ScrollArea::vertical()
                            .max_height(rows * neoflash_ui::metrics::row())
                            .auto_shrink([false, false])
                            .themed()
                            .show(ui, |ui| {
                                for (index, fix) in menu.fixes.iter().enumerate() {
                                    let selected = menu.selected == index;
                                    let row = ui.add(
                                        nf::MenuItem::new(&fix.title)
                                            .highlighted(selected)
                                            .shortcut(if selected { "↵" } else { "" }),
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
                                    .shortcut(if selected { "↵" } else { "" }),
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
                        "Quick fix".into()
                    } else {
                        format!("{} fixes", fixes.len())
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
                    nf::tooltip(result.response.clone(), message.trim_end_matches('.'), None);
                    if let Some(action_resp) = result.action.clone() {
                        let action_tip = if fixes.len() == 1 {
                            "Quick fix"
                        } else {
                            "Show quick fixes"
                        };
                        nf::tooltip(action_resp, action_tip, Some("⌘."));
                    }
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
                origin + egui::vec2(right.max(left + space::s1()), row.pos.y + row.height()),
            ));
            if range.is_empty() {
                break;
            }
        }
        offset += row.char_count_including_newline();
    }
    rects
}

#[derive(Clone)]
struct StoredRuntimeDiagnostic {
    pipeline_instance_id: u64,
    script_location: crate::panels::actions_sidebar::ScriptLocation,
    fingerprint: u64,
    diagnostic: Diagnostic,
}

static RUNTIME_SCRIPT_DIAGNOSTICS: std::sync::Mutex<Vec<StoredRuntimeDiagnostic>> =
    std::sync::Mutex::new(Vec::new());

const MAX_STORED_RUNTIME_DIAGNOSTICS: usize = 128;

pub(crate) fn clear_stored_runtime_diagnostics_for_pipeline(pipeline_instance_id: u64) {
    if let Ok(mut guard) = RUNTIME_SCRIPT_DIAGNOSTICS.lock() {
        guard.retain(|entry| entry.pipeline_instance_id != pipeline_instance_id);
    }
}

pub(crate) fn script_for_project_location(
    project: &neoflash_core::Project,
    location: &crate::panels::actions_sidebar::ScriptLocation,
) -> Option<neoflash_core::Script> {
    use crate::panels::actions_sidebar::ScriptLocation;

    match location {
        ScriptLocation::Source(id) => {
            project
                .sources
                .iter()
                .find(|u| &u.id == id)
                .map(|u| neoflash_core::Script {
                    language: u.language,
                    source: u.source.clone(),
                })
        }
        ScriptLocation::Frame {
            symbol,
            layer,
            frame,
        } => {
            let timeline = &project.symbol(symbol)?.timeline;
            let fs = layer
                .as_deref()
                .and_then(|id| {
                    timeline
                        .scripts
                        .iter()
                        .find(|fs| fs.frame == *frame && fs.layer.as_deref() == Some(id))
                })
                .or_else(|| {
                    timeline
                        .scripts
                        .iter()
                        .find(|fs| fs.frame == *frame && fs.layer.is_none())
                })
                .or_else(|| timeline.scripts.iter().find(|fs| fs.frame == *frame))?;
            Some(fs.script.clone())
        }
        ScriptLocation::Object {
            symbol,
            frame,
            element,
            event,
        } => project
            .symbol(symbol)?
            .timeline
            .layers
            .iter()
            .filter_map(|layer| layer.key_at(*frame))
            .flat_map(|key| &key.elements)
            .find(|el| &el.id == element)?
            .scripts
            .iter()
            .find(|os| os.event == *event)
            .map(|os| os.script.clone()),
    }
}

pub(crate) fn output_location_to_script_location(
    project: &neoflash_core::Project,
    loc: &crate::output_pipeline::OutputLocation,
) -> Option<crate::panels::actions_sidebar::ScriptLocation> {
    use crate::panels::actions_sidebar::ScriptLocation;

    if let Some(id) = loc.source_id.as_deref()
        && project.sources.iter().any(|u| u.id == id)
    {
        return Some(ScriptLocation::Source(id.to_owned()));
    }
    if let Some(path) = loc.file_path.as_deref() {
        let path_str = path.to_string_lossy();
        if let Some(unit) = project.sources.iter().find(|u| {
            u.path == path_str.as_ref()
                || std::path::Path::new(&u.path) == path
                || path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .is_some_and(|name| u.path == name)
        }) {
            return Some(ScriptLocation::Source(unit.id.clone()));
        }
    }

    let resolved_symbol = loc
        .symbol_id
        .as_deref()
        .filter(|id| project.symbol(id).is_some())
        .map(str::to_owned)
        .or_else(|| {
            let name = loc.symbol_name.as_deref()?;
            project
                .symbols
                .iter()
                .find(|s| s.name == name)
                .map(|s| s.id.clone())
        });

    if let (Some(element), Some(event)) = (loc.element_id.clone(), loc.object_event) {
        let symbol = resolved_symbol
            .clone()
            .unwrap_or_else(|| project.root.clone());
        if project.symbol(&symbol).is_some() {
            return Some(ScriptLocation::Object {
                symbol,
                frame: loc.frame_index.unwrap_or(0),
                element,
                event,
            });
        }
    }

    let symbol = resolved_symbol?;
    let sym = project.symbol(&symbol)?;
    let max_frame = sym.timeline.frame_count.saturating_sub(1);
    let frame = loc.frame_index.unwrap_or(0).min(max_frame);
    let layer_from_index = loc
        .layer_index
        .and_then(|idx| sym.timeline.layers.get(idx))
        .map(|l| l.id.clone());
    let matched_layer = if let Some(lid) = layer_from_index {
        if sym
            .timeline
            .scripts
            .iter()
            .any(|fs| fs.frame == frame && fs.layer.as_deref() == Some(lid.as_str()))
        {
            Some(lid)
        } else if sym
            .timeline
            .scripts
            .iter()
            .any(|fs| fs.frame == frame && fs.layer.is_none())
        {
            None
        } else {
            Some(lid)
        }
    } else {
        sym.timeline
            .scripts
            .iter()
            .find(|fs| fs.frame == frame)
            .and_then(|fs| fs.layer.clone())
    };
    Some(ScriptLocation::Frame {
        symbol,
        layer: matched_layer,
        frame,
    })
}

fn build_line_diagnostic(
    language: neoflash_core::ScriptLanguage,
    source: &str,
    line_1based: usize,
    col_1based: Option<usize>,
    source_path: Option<String>,
    message: String,
) -> Diagnostic {
    use neoflash_script::{DiagnosticDetails, DiagnosticSeverity};

    let mut line_starts = vec![0usize];
    for (idx, b) in source.bytes().enumerate() {
        if b == b'\n' {
            line_starts.push(idx + 1);
        }
    }
    let total_lines = line_starts.len().max(1);
    let clamped_line = line_1based.clamp(1, total_lines);
    let line_start_byte = line_starts[clamped_line - 1];
    let raw_line_end = if clamped_line < line_starts.len() {
        line_starts[clamped_line].saturating_sub(1)
    } else {
        source.len()
    };
    let line_slice = source.get(line_start_byte..raw_line_end).unwrap_or("");
    let line_slice = line_slice.strip_suffix('\r').unwrap_or(line_slice);
    let leading_ws_bytes = line_slice.len() - line_slice.trim_start().len();
    let trimmed_end_bytes = line_slice.trim_end().len();

    let (span_start, span_end, start_col, end_col) = if trimmed_end_bytes > leading_ws_bytes {
        let first_non_ws_col = line_slice[..leading_ws_bytes].chars().count() + 1;
        let line_end_col = line_slice[..trimmed_end_bytes].chars().count() + 1;
        let req_col = col_1based.unwrap_or(first_non_ws_col).max(first_non_ws_col);
        let chosen_col = if req_col < line_end_col {
            req_col
        } else {
            first_non_ws_col
        };
        let start_byte_in_line = line_slice
            .char_indices()
            .nth(chosen_col - 1)
            .map_or(leading_ws_bytes, |(b, _)| b);
        (
            line_start_byte + start_byte_in_line,
            line_start_byte + trimmed_end_bytes,
            chosen_col,
            line_end_col,
        )
    } else {
        (line_start_byte, line_start_byte, 1, 1)
    };

    Diagnostic {
        language,
        line: clamped_line,
        column: start_col,
        message,
        details: Box::new(DiagnosticDetails {
            code: Some("RUNTIME".into()),
            severity: DiagnosticSeverity::Error,
            span: (span_start, span_end),
            end_line: clamped_line,
            end_column: end_col,
            path: source_path,
            fixes: Vec::new(),
        }),
    }
}

impl crate::app::Editor {
    /// Ensure `self.code_feedback` is up-to-date for the currently active `Actions` script and
    /// overlay any recorded runtime error diagnostics matching its current source fingerprint.
    pub(crate) fn sync_actions_diagnostics_after_navigation(&mut self) {
        use crate::panels::actions_sidebar::ScriptLocation;

        self.actions_ui.edited = None;
        let Some(raw_loc) = self.effective_actions_location() else {
            return;
        };
        let active_loc = self.canonical_script_location(&raw_loc).unwrap_or(raw_loc);
        let Some(script) = script_for_project_location(&self.project, &active_loc) else {
            return;
        };
        if self.current_feedback(&script).is_none() {
            match &active_loc {
                ScriptLocation::Source(_) => self.check_project_sources(false),
                ScriptLocation::Frame { .. } | ScriptLocation::Object { .. } => {
                    self.check_action_source(&script, false);
                }
            }
        }
        let fp = self.code_fingerprint(&script);
        let editor_id = self.actions_editor_id();
        let instance_id = self.output.instance_id();
        let matching_runtime: Vec<Diagnostic> = RUNTIME_SCRIPT_DIAGNOSTICS
            .lock()
            .ok()
            .map(|guard| {
                guard
                    .iter()
                    .filter(|entry| {
                        entry.pipeline_instance_id == instance_id
                            && entry.fingerprint == fp
                            && self
                                .canonical_script_location(&entry.script_location)
                                .as_ref()
                                .unwrap_or(&entry.script_location)
                                == &active_loc
                    })
                    .map(|entry| entry.diagnostic.clone())
                    .collect()
            })
            .unwrap_or_default();

        if matching_runtime.is_empty() {
            return;
        }
        let fb = self
            .code_feedback
            .get_or_insert_with(|| super::CodeFeedback {
                target: editor_id,
                fingerprint: fp,
                text: String::new(),
                success: false,
                diagnostics: Vec::new(),
            });
        if fb.target != editor_id || fb.fingerprint != fp {
            fb.target = editor_id;
            fb.fingerprint = fp;
            fb.diagnostics.clear();
        }
        fb.success = false;
        for diag in matching_runtime {
            let already = fb.diagnostics.iter().any(|existing| {
                existing.line == diag.line
                    && existing.code == diag.code
                    && existing.message == diag.message
            });
            if !already {
                fb.diagnostics.push(diag);
            }
        }
        fb.text = fb
            .diagnostics
            .iter()
            .map(|d| format!("{}: {}", super::diagnostic_severity(d.severity).label(), d))
            .collect::<Vec<_>>()
            .join("\n");
    }

    /// Record a runtime error diagnostic from `Test Movie` / `Test Scene` so `Actions` immediately
    /// highlights the failing line (`Line <L>`) and preserves that highlight when clicking the
    /// `Output` row to navigate to the script.
    pub(crate) fn record_runtime_diagnostic_for_location(
        &mut self,
        loc: &crate::output_pipeline::OutputLocation,
        reason: &str,
    ) {
        use crate::panels::actions_sidebar::ScriptLocation;

        let Some(raw_script_loc) = output_location_to_script_location(&self.project, loc) else {
            return;
        };
        let script_loc = self
            .canonical_script_location(&raw_script_loc)
            .unwrap_or(raw_script_loc);
        let Some(script) = script_for_project_location(&self.project, &script_loc) else {
            return;
        };
        let source_path = match &script_loc {
            ScriptLocation::Source(id) => self
                .project
                .sources
                .iter()
                .find(|u| &u.id == id)
                .map(|u| u.path.clone()),
            _ => None,
        };
        let diag = build_line_diagnostic(
            script.language,
            &script.source,
            loc.line.unwrap_or(1),
            loc.column,
            source_path,
            reason.trim().to_owned(),
        );
        let fp = self.code_fingerprint(&script);
        let instance_id = self.output.instance_id();
        if let Ok(mut guard) = RUNTIME_SCRIPT_DIAGNOSTICS.lock() {
            let dup = guard.iter().any(|entry| {
                entry.pipeline_instance_id == instance_id
                    && entry.fingerprint == fp
                    && entry.script_location == script_loc
                    && entry.diagnostic.line == diag.line
                    && entry.diagnostic.message == diag.message
            });
            if !dup {
                if guard.len() >= MAX_STORED_RUNTIME_DIAGNOSTICS {
                    guard.remove(0);
                }
                guard.push(StoredRuntimeDiagnostic {
                    pipeline_instance_id: instance_id,
                    script_location: script_loc.clone(),
                    fingerprint: fp,
                    diagnostic: diag,
                });
            }
        }
        if let Some(cur) = self
            .effective_actions_location()
            .map(|l| self.canonical_script_location(&l).unwrap_or(l))
            && cur == script_loc
        {
            self.sync_actions_diagnostics_after_navigation();
        }
    }

    /// Clear stored runtime error overlays for this document pipeline (called on `Output` Clear
    /// and when starting a fresh `Test Movie` / `Test Scene` run).
    pub(crate) fn clear_runtime_diagnostics_in_actions(&mut self) {
        clear_stored_runtime_diagnostics_for_pipeline(self.output.instance_id());
        if let Some(fb) = &mut self.code_feedback
            && fb
                .diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("RUNTIME"))
        {
            fb.diagnostics
                .retain(|d| d.code.as_deref() != Some("RUNTIME"));
            fb.success = !fb
                .diagnostics
                .iter()
                .any(|d| d.severity == neoflash_script::DiagnosticSeverity::Error);
            fb.text = if fb.diagnostics.is_empty() {
                "Code accepted for current language support.".to_owned()
            } else {
                fb.diagnostics
                    .iter()
                    .map(|d| format!("{}: {}", super::diagnostic_severity(d.severity).label(), d))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
        }
    }

    /// Ensure `Actions` immediately highlights a compile error when `Test Movie` / `Test Scene` /
    /// `Publish` fails pre-flight, even if the 0.6 s debounced `live_check` had not run yet.
    pub(crate) fn highlight_compiler_error_in_actions(
        &mut self,
        first_error_loc: Option<&crate::output_pipeline::OutputLocation>,
    ) {
        let active_has_error = self
            .effective_actions_location()
            .and_then(|loc| {
                let canon = self.canonical_script_location(&loc).unwrap_or(loc);
                self.compiler_errors.items.iter().find(|item| {
                    item.is_error
                        && item.script.as_ref().is_some_and(|s| {
                            self.canonical_script_location(s).as_ref().unwrap_or(s) == &canon
                        })
                })
            })
            .is_some();

        if !active_has_error
            && let Some(loc) = first_error_loc
            && let Some(target_script_loc) = output_location_to_script_location(&self.project, loc)
        {
            self.open_script_location(target_script_loc);
        }
        self.sync_actions_diagnostics_after_navigation();
    }

    #[cfg(test)]
    pub(crate) fn active_actions_diagnostic_lines(&self) -> Vec<usize> {
        self.code_feedback
            .as_ref()
            .map(|fb| fb.diagnostics.iter().map(|d| d.line).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "sources_diagnostics_tests.rs"]
mod tests;
