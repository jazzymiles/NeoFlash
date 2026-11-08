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

#[derive(Clone, Debug, Default)]
pub(crate) struct FunctionSearchState {
    pub(crate) open: bool,
    pub(crate) just_opened: bool,
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
                    if state.just_opened {
                        ui.memory_mut(|memory| memory.request_focus(search.id.with("edit")));
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
                        .max_height(392.0)
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
                    });
                });
            });

        let results = results_for_context(&state.query, has_document);
        let visible = results.len().min(MAX_VISIBLE_RESULTS);
        let mut close = false;
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
            if visible > 0 {
                state.active = (state.active + 1) % visible;
            }
        } else if ctx
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp))
        {
            if visible > 0 {
                state.active = (state.active + visible - 1) % visible;
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
        if activated.is_some() || close {
            state.open = false;
        }
        store_state(ctx, state);

        if let Some(command) = activated {
            crate::commands::queue_command(ctx, command);
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
