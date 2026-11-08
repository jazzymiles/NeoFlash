//! Immediate Help command search: a focused NF2 popover that filters and executes registered
//! menu commands without detouring through Preferences ▸ Shortcuts.

use crate::{app::Editor, commands::CommandId};
use eframe::egui::{self, Pos2, Rect};
use neoflash_ui::{
    components as c,
    tokens::{TextStyle, space},
    typography::rich,
};

const MAX_VISIBLE_RESULTS: usize = 24;
const RESULTS_MAX_HEIGHT: f32 = 392.0;

#[derive(Clone, Debug, Default)]
pub(crate) struct FunctionSearchState {
    pub(crate) open: bool,
    pub(crate) just_opened: bool,
    /// Native menu dismissal can reclaim focus in the opening frame. Retry for a few
    /// frames so the first typed character always lands in the Search field.
    pub(crate) focus_attempts: u8,
    pub(crate) query: String,
    pub(crate) active: usize,
    pub(crate) trigger_rect: Option<Rect>,
}

fn state_id() -> egui::Id {
    egui::Id::new("nf2-function-search")
}

pub(crate) fn function_search_state(ctx: &egui::Context) -> FunctionSearchState {
    ctx.data(|data| data.get_temp::<FunctionSearchState>(state_id()))
        .unwrap_or_default()
}

fn store_state(ctx: &egui::Context, state: FunctionSearchState) {
    ctx.data_mut(|data| data.insert_temp(state_id(), state));
}

pub(crate) fn is_function_search_open(ctx: &egui::Context) -> bool {
    function_search_state(ctx).open
}

fn normalized(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|ch| ch.is_alphanumeric())
        .collect()
}

/// Lower is better. Direct path/name matches precede fuzzy subsequences and keyword-only hits.
fn match_score(command: CommandId, query: &str) -> Option<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Some(10_000);
    }
    let path = command.menu_path().to_lowercase();
    let row = command.row_title().to_lowercase();
    if row.starts_with(&query) {
        return Some(0);
    }
    if path.split(" ▸ ").any(|segment| segment.starts_with(&query)) {
        return Some(100);
    }
    if let Some(index) = row.find(&query) {
        return Some(200 + index);
    }
    if let Some(index) = path.find(&query) {
        return Some(400 + index);
    }

    let needle = normalized(&query);
    let candidate = normalized(&format!(
        "{} {}",
        command.menu_path(),
        command.search_keywords()
    ));
    if needle.is_empty() {
        return Some(10_000);
    }
    let mut search_from = 0usize;
    let mut gap = 0usize;
    for ch in needle.chars() {
        let tail = &candidate[search_from..];
        let offset = tail.find(ch)?;
        gap += offset;
        search_from += offset + ch.len_utf8();
    }
    Some(1_000 + gap)
}

pub(crate) fn function_search_results(query: &str) -> Vec<CommandId> {
    let mut results: Vec<_> = CommandId::ALL
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(order, command)| {
            match_score(command, query).map(|score| (score, order, command))
        })
        .collect();
    results.sort_by_key(|(score, order, _)| (*score, *order));
    results.into_iter().map(|(_, _, command)| command).collect()
}

fn results_for_context(query: &str, has_document: bool) -> Vec<CommandId> {
    function_search_results(query)
        .into_iter()
        .filter(|command| {
            has_document
                || matches!(
                    command,
                    CommandId::FileNew | CommandId::FileOpen | CommandId::EditPreferences
                )
        })
        .collect()
}

impl Editor {
    pub(crate) fn open_function_search(&mut self, ctx: &egui::Context) {
        self.open_function_search_at(ctx, None);
    }

    pub(crate) fn open_function_search_at(
        &mut self,
        ctx: &egui::Context,
        trigger_rect: Option<Rect>,
    ) {
        self.close_symbols_popover(ctx);
        store_state(
            ctx,
            FunctionSearchState {
                open: true,
                just_opened: true,
                focus_attempts: 3,
                query: String::new(),
                active: 0,
                trigger_rect,
            },
        );
        ctx.request_repaint();
    }

    pub(crate) fn close_function_search(&self, ctx: &egui::Context) {
        let mut state = function_search_state(ctx);
        state.open = false;
        state.just_opened = false;
        state.focus_attempts = 0;
        store_state(ctx, state);
    }

    pub(crate) fn function_search_popover(&mut self, ctx: &egui::Context) {
        let mut state = function_search_state(ctx);
        if !state.open {
            return;
        }

        let content = ctx.content_rect();
        let min_x = content.left() + space::S2;
        let max_x = (content.right() - 280.0).max(min_x);
        let x = state
            .trigger_rect
            .map(|rect| rect.left())
            .unwrap_or(content.left() + space::S5)
            .clamp(min_x, max_x);
        let pos = Pos2::new(
            x,
            content.top() + neoflash_ui::metrics::top_bar() + space::S1,
        );
        let width = (content.width() - space::S5 * 2.0).clamp(1.0, 560.0);
        let has_document = self.has_document();
        let mut activated = None;
        let mut open_manual = false;

        let area = egui::Area::new(egui::Id::new("nf2-function-search-area"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .constrain_to(content)
            .show(ctx, |ui| {
                c::popover_frame(ctx).show(ui, |ui| {
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let search = ui.add(
                        c::Search::new(&mut state.query, "Search menu items and functions")
                            .width(width),
                    );
                    let edit_id = search.id.with("edit");
                    if ui.memory(|memory| memory.has_focus(edit_id)) {
                        state.focus_attempts = 0;
                    } else if state.focus_attempts > 0 {
                        ui.memory_mut(|memory| memory.request_focus(edit_id));
                        state.focus_attempts -= 1;
                        ctx.request_repaint();
                    }
                    if search.changed() {
                        state.active = 0;
                    }
                    ui.add_space(space::S1);

                    let results = results_for_context(&state.query, has_document);
                    let visible = results.len().min(MAX_VISIBLE_RESULTS);
                    if visible == 0 {
                        state.active = 0;
                    } else if state.active >= visible {
                        state.active = visible - 1;
                    }

                    c::menu_section(ui, "Menu Items");
                    egui::ScrollArea::vertical()
                        .id_salt("nf2-function-search-results")
                        .max_height(RESULTS_MAX_HEIGHT)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 0.0;
                            if visible == 0 {
                                ui.add_space(space::S2);
                                ui.label(
                                    rich("No matching functions", TextStyle::Body)
                                        .color(neoflash_ui::palette(ctx).text_secondary),
                                );
                                ui.add_space(space::S2);
                            }
                            for (index, command) in results.into_iter().take(visible).enumerate() {
                                let label = command.menu_path().replace(" ▸ ", " > ");
                                let shortcut = self.sc(command);
                                let row = ui.add(
                                    c::MenuItem::new(&label)
                                        .shortcut(&shortcut)
                                        .highlighted(index == state.active),
                                );
                                if index == state.active {
                                    row.scroll_to_me(Some(egui::Align::Center));
                                }
                                if row.hovered() {
                                    state.active = index;
                                }
                                if row.clicked() {
                                    activated = Some(command);
                                }
                            }
                        });

                    c::menu_separator(ui);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S1;
                        c::kbd(ui, "↑↓", false);
                        ui.label(
                            rich("move", TextStyle::Small)
                                .color(neoflash_ui::palette(ctx).text_secondary),
                        );
                        ui.add_space(space::S2);
                        c::kbd(ui, "↵", false);
                        ui.label(
                            rich("run", TextStyle::Small)
                                .color(neoflash_ui::palette(ctx).text_secondary),
                        );
                        ui.add_space(space::S2);
                        c::kbd(ui, "Esc", false);
                        ui.label(
                            rich("close", TextStyle::Small)
                                .color(neoflash_ui::palette(ctx).text_secondary),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(c::Button::ghost("Help & Reference…")).clicked() {
                                open_manual = true;
                            }
                        });
                    });
                });
            });

        let results = results_for_context(&state.query, has_document);
        let visible = results.len().min(MAX_VISIBLE_RESULTS);
        let mut close = false;
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
            if visible > 0 {
                state.active = (state.active + 1) % visible;
                ctx.request_repaint();
            }
        } else if ctx
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp))
        {
            if visible > 0 {
                state.active = (state.active + visible - 1) % visible;
                ctx.request_repaint();
            }
        } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            activated = results.get(state.active).copied();
        } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            close = true;
        }

        if !state.just_opened
            && ctx.memory(|memory| memory.areas().visible_last_frame(&area.response.layer_id))
            && area.response.clicked_elsewhere()
        {
            close = true;
        }
        state.just_opened = false;
        if activated.is_some() || open_manual || close {
            state.open = false;
        }
        store_state(ctx, state);

        if open_manual {
            crate::panels::help_manual::open_help_manual(ctx);
            ctx.request_repaint();
        } else if let Some(command) = activated {
            crate::commands::queue_command(ctx, command);
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::Project;

    #[test]
    fn fuzzy_search_ranks_menu_paths_and_keywords_without_preferences() {
        let direct = function_search_results("break apart");
        assert_eq!(direct.first(), Some(&CommandId::ModifyBreakApart));

        let fuzzy = function_search_results("brkaprt");
        assert_eq!(fuzzy.first(), Some(&CommandId::ModifyBreakApart));

        let keyframes = function_search_results("keyframe");
        assert!(keyframes.contains(&CommandId::InsertKeyframe));
        assert!(keyframes.contains(&CommandId::ModifyTimelineConvertToKeyframes));
    }

    #[test]
    fn opening_search_focuses_the_field_and_accepts_the_first_followup_text() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        editor.open_function_search(&ctx);
        assert_eq!(function_search_state(&ctx).focus_attempts, 3);

        let input = |events| egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 900.0))),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input(vec![]), |ctx| editor.function_search_popover(ctx));
        assert!(ctx.memory(|memory| memory.focused().is_some()));
        let _ = ctx.run(input(vec![egui::Event::Text("gradient".into())]), |ctx| {
            editor.function_search_popover(ctx)
        });
        assert_eq!(function_search_state(&ctx).query, "gradient");
        assert_eq!(function_search_state(&ctx).focus_attempts, 0);
    }

    #[test]
    fn keyboard_navigation_scrolls_the_active_result_into_view() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        editor.open_function_search(&ctx);
        let input = |events| egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 900.0))),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input(Vec::new()), |ctx| editor.function_search_popover(ctx));
        for _ in 0..20 {
            let _ = ctx.run(
                input(vec![egui::Event::Key {
                    key: egui::Key::ArrowDown,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }]),
                |ctx| editor.function_search_popover(ctx),
            );
        }
        let output = ctx.run(input(Vec::new()), |ctx| editor.function_search_popover(ctx));
        let state = function_search_state(&ctx);
        assert_eq!(state.active, 20);
        let active_label = results_for_context(&state.query, true)[state.active]
            .menu_path()
            .replace(" ▸ ", " > ");
        let nodes = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("command search accessibility tree")
            .nodes;
        let bounds = |label: &str| {
            nodes.iter().find_map(|(_, node)| {
                (node.label() == Some(label))
                    .then(|| node.bounds())
                    .flatten()
            })
        };
        let active = bounds(&active_label).expect("active command row");
        let visible_results_bottom = neoflash_ui::metrics::top_bar()
            + space::S1
            + neoflash_ui::metrics::control()
            + neoflash_ui::metrics::menu_section()
            + RESULTS_MAX_HEIGHT
            + 2.0 * space::S5;
        assert!(
            active.y1 <= f64::from(visible_results_bottom),
            "active keyboard result must remain inside the results viewport"
        );
    }
}
