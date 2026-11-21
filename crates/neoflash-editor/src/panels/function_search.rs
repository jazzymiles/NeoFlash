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
/// The result list shows up to this many `metrics::row()` rows before it scrolls.
const RESULTS_VISIBLE_ROWS: f32 = 16.0;

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

#[cfg(test)]
pub(crate) fn is_open(ctx: &egui::Context) -> bool {
    function_search_state(ctx).open
}

#[cfg(test)]
pub(crate) fn results_for_context(
    query: &str,
    has_document: bool,
    has_ready_ai: bool,
) -> Vec<CommandId> {
    function_search_results(query)
        .into_iter()
        .filter(|command| {
            if matches!(command, CommandId::EditSendToAi) && !has_ready_ai {
                return false;
            }
            if matches!(command, CommandId::HelpSendBetaFeedback)
                && !crate::panels::beta_feedback::is_beta_build()
            {
                return false;
            }
            if matches!(command, CommandId::HelpSearchFunctions) {
                return false;
            }
            has_document
                || matches!(
                    command,
                    CommandId::FileNew
                        | CommandId::FileOpen
                        | CommandId::FileImportLegacyFlash
                        | CommandId::EditPreferences
                        | CommandId::EditKeyboardShortcuts
                )
                || matches!(
                    command.category(),
                    crate::commands::CommandCategory::Window
                        | crate::commands::CommandCategory::Help
                )
        })
        .collect()
}

pub(crate) fn results_for_editor(
    editor: &Editor,
    ctx: Option<&egui::Context>,
    query: &str,
) -> Vec<CommandId> {
    let has_doc = editor.has_document();
    let has_ready_ai = editor.has_ready_ai_connection();
    function_search_results(query)
        .into_iter()
        .filter(|&command| {
            if matches!(command, CommandId::HelpSearchFunctions) {
                return false;
            }
            if matches!(command, CommandId::EditSendToAi) && !has_ready_ai {
                return false;
            }
            if has_doc {
                if matches!(command, CommandId::HelpSendBetaFeedback) {
                    crate::panels::beta_feedback::is_beta_build()
                } else {
                    true
                }
            } else {
                editor.command_menu_state(ctx, command).enabled
            }
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
        let min_x = content.left() + space::s2();
        // Popover width: `width/dialog-m`, or the window minus a dialog padding on both sides.
        let width =
            (content.width() - space::s5() * 2.0).clamp(1.0, neoflash_ui::metrics::dialog_m());
        let max_x = (content.right() - width - space::s2()).max(min_x);
        let x = state
            .trigger_rect
            .map(|rect| rect.left())
            .unwrap_or(content.left() + space::s5())
            .clamp(min_x, max_x);
        let pos = Pos2::new(
            x,
            content.top() + neoflash_ui::metrics::top_bar() + space::s1(),
        );
        let mut activated = None;
        let mut open_manual = false;
        let mut run_selected = false;
        let mut search_edit_id = None;
        let mut results_id = None;
        let mut result_rows = Vec::new();
        let mut tab_stops = Vec::new();
        // egui already interpreted Tab and the arrows at the start of this pass.
        // The popover owns navigation until dismissal, including reverse Tab.
        ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));

        let area = egui::Area::new(egui::Id::new("nf2-function-search-area"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .constrain_to(content)
            .show(ctx, |ui| {
                c::popover_frame(ctx).show(ui, |ui| {
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let edit_id = ui.next_auto_id().with("edit");
                    if state.just_opened {
                        ui.memory_mut(|memory| memory.request_focus(edit_id));
                    }
                    if ui.memory(|memory| memory.has_focus(edit_id)) {
                        run_selected = ui.input_mut(|input| {
                            input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                        });
                    }
                    let search = ui.add(
                        c::Search::new(&mut state.query, "Search menu items and functions")
                            .width(width),
                    );
                    search_edit_id = Some(edit_id);
                    tab_stops.push(edit_id);
                    if !state.query.is_empty() {
                        let clear_id = search.id.with("clear");
                        tab_stops.push(clear_id);
                        ctx.accesskit_node_builder(clear_id, |node| {
                            node.set_role(egui::accesskit::Role::Button);
                            node.set_label("Clear search");
                        });
                        if let Some(clear) = ctx.read_response(clear_id)
                            && neoflash_ui::theme::keyboard_focus(ui, &clear)
                        {
                            neoflash_ui::theme::focus_ring(
                                ui,
                                clear.rect,
                                neoflash_ui::tokens::radius::sm(),
                            );
                        }
                    }
                    if search.changed() {
                        state.active = 0;
                    }
                    ui.add_space(space::s1());

                    let results = results_for_editor(self, Some(ctx), &state.query);
                    let visible = results.len().min(MAX_VISIBLE_RESULTS);
                    if visible == 0 {
                        state.active = 0;
                    } else if state.active >= visible {
                        state.active = visible - 1;
                    }

                    c::menu_section(ui, "Menu Items");
                    egui::ScrollArea::vertical()
                        .id_salt("nf2-function-search-results")
                        .max_height(RESULTS_VISIBLE_ROWS * neoflash_ui::metrics::row())
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            results_id = Some(ui.unique_id());
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 0.0;
                            if visible == 0 {
                                ui.add_space(space::s2());
                                ui.label(
                                    rich("No matching functions", TextStyle::Body)
                                        .color(neoflash_ui::palette(ctx).text_secondary),
                                );
                                ui.add_space(space::s2());
                            }
                            for (index, command) in results.into_iter().take(visible).enumerate() {
                                let label = command.menu_path().replace(" ▸ ", " > ");
                                let shortcut = self.sc(command);
                                let row = ui.add(
                                    c::MenuItem::new(&label)
                                        .shortcut(&shortcut)
                                        .highlighted(index == state.active),
                                );
                                result_rows.push(row.id);
                                if index == state.active {
                                    row.scroll_to_me(Some(egui::Align::Center));
                                }
                                if row.hovered()
                                    && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO)
                                {
                                    state.active = index;
                                }
                                if row.clicked() {
                                    activated = Some(command);
                                }
                            }
                        });

                    c::menu_separator(ui);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::s1();
                        c::kbd(ui, "↑↓", false);
                        ui.label(
                            rich("move", TextStyle::Small)
                                .color(neoflash_ui::palette(ctx).text_secondary),
                        );
                        ui.add_space(space::s2());
                        c::kbd(ui, "↵", false);
                        ui.label(
                            rich("run", TextStyle::Small)
                                .color(neoflash_ui::palette(ctx).text_secondary),
                        );
                        ui.add_space(space::s2());
                        c::kbd(ui, "Esc", false);
                        ui.label(
                            rich("close", TextStyle::Small)
                                .color(neoflash_ui::palette(ctx).text_secondary),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let help = ui.add(c::Button::ghost("Help & Reference…"));
                            tab_stops.push(help.id);
                            open_manual = help.clicked();
                        });
                    });
                });
            });

        let results = results_for_editor(self, Some(ctx), &state.query);
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
        } else if run_selected {
            activated = results.get(state.active).copied();
        } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            close = true;
        }
        // egui's buttons read Enter/Space without consuming them. In particular,
        // opening the manual must not also send the activation key to that dialog.
        ctx.input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
            input.consume_key(egui::Modifiers::NONE, egui::Key::Space);
        });

        if !state.just_opened
            && ctx.memory(|memory| memory.areas().visible_last_frame(&area.response.layer_id))
            && area.response.clicked_elsewhere()
        {
            close = true;
        }
        state.just_opened = false;
        if activated.is_some() || open_manual || close {
            state.open = false;
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused()
                    && (tab_stops.contains(&id) || result_rows.contains(&id))
                {
                    memory.surrender_focus(id);
                }
            });
        } else if !tab_stops.is_empty() {
            let tab_steps = ctx.input_mut(|input| {
                let back = input.count_and_consume_key(egui::Modifiers::SHIFT, egui::Key::Tab);
                let forward = input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::Tab);
                forward as isize - back as isize
            });
            let moved = ctx.memory_mut(|memory| {
                memory.set_modal_layer(area.response.layer_id);
                let current = tab_stops.iter().position(|id| memory.has_focus(*id));
                let next = match current {
                    Some(index) => {
                        (index as isize + tab_steps).rem_euclid(tab_stops.len() as isize) as usize
                    }
                    None => 0,
                };
                if current != Some(next) {
                    memory.request_focus(tab_stops[next]);
                }
                current != Some(next)
            });
            if moved {
                ctx.request_repaint();
            }
        }
        // Keep the native TextEdit role, value and caret intact. Its active
        // descendant identifies the same result that Enter would execute while
        // keyboard focus remains in the search field.
        if let Some(results_id) = results_id {
            ctx.accesskit_node_builder(results_id, |node| {
                node.set_role(egui::accesskit::Role::ListBox);
            });
        }
        for (index, row_id) in result_rows.iter().enumerate() {
            ctx.accesskit_node_builder(*row_id, |node| {
                node.set_role(egui::accesskit::Role::ListBoxOption);
                node.set_selected(state.open && index == state.active);
            });
        }
        if let Some(edit_id) = search_edit_id {
            ctx.accesskit_node_builder(edit_id, |node| {
                node.clear_controls();
                node.clear_active_descendant();
                if state.open {
                    if let Some(results_id) = results_id {
                        node.push_controlled(results_id.value().into());
                    }
                    if let Some(active_id) = result_rows.get(state.active) {
                        node.set_active_descendant(active_id.value().into());
                    }
                }
            });
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
        let _ = ctx.run(input(Vec::new()), |ctx| editor.function_search_popover(ctx));
        let focus = ctx
            .memory(|memory| memory.focused())
            .expect("search keeps focus");
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
        let active_label = results_for_context(&state.query, true, false)[state.active]
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
            + space::s1()
            + neoflash_ui::metrics::control()
            + neoflash_ui::metrics::menu_section()
            + RESULTS_VISIBLE_ROWS * neoflash_ui::metrics::row()
            + 2.0 * space::s5();
        assert!(
            active.y1 <= f64::from(visible_results_bottom),
            "active keyboard result must remain inside the results viewport"
        );
        let assert_search_relation = |output: &egui::FullOutput, expected: Option<&str>| {
            let update = output.platform_output.accesskit_update.as_ref().unwrap();
            let search_id = egui::accesskit::NodeId::from(focus.value());
            let search = &update
                .nodes
                .iter()
                .find(|(id, _)| *id == search_id)
                .unwrap()
                .1;
            assert_eq!(search.role(), egui::accesskit::Role::TextInput);
            let list = search
                .controls()
                .first()
                .expect("search controls its result list");
            assert_eq!(
                update
                    .nodes
                    .iter()
                    .find(|(id, _)| id == list)
                    .unwrap()
                    .1
                    .role(),
                egui::accesskit::Role::ListBox
            );
            let selected: Vec<_> = update
                .nodes
                .iter()
                .filter(|(_, node)| {
                    node.role() == egui::accesskit::Role::ListBoxOption
                        && node.is_selected() == Some(true)
                })
                .collect();
            if let Some(expected) = expected {
                assert_eq!(selected.len(), 1);
                assert_eq!(selected[0].1.label(), Some(expected));
                assert_eq!(search.active_descendant(), Some(selected[0].0));
            } else {
                assert!(selected.is_empty());
                assert!(search.active_descendant().is_none());
            }
            assert_eq!(update.focus, search_id);
            assert_eq!(ctx.memory(|memory| memory.focused()), Some(focus));
        };
        assert_search_relation(&output, Some(&active_label));

        let output = ctx.run(input(vec![egui::Event::Text("gradient".into())]), |ctx| {
            editor.function_search_popover(ctx)
        });
        let state = function_search_state(&ctx);
        assert_eq!(state.query, "gradient");
        let filtered_label = results_for_editor(&editor, Some(&ctx), &state.query)[state.active]
            .menu_path()
            .replace(" ▸ ", " > ");
        assert_search_relation(&output, Some(&filtered_label));
        let search = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|(id, _)| *id == focus.value().into())
            .unwrap()
            .1;
        assert_eq!(
            search.value(),
            Some("gradient"),
            "native text value is preserved"
        );

        let output = ctx.run(
            input(vec![egui::Event::Text("zzzzzzzzzzzz".into())]),
            |ctx| editor.function_search_popover(ctx),
        );
        assert_search_relation(&output, None);
        let output = ctx.run(
            input(vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]),
            |ctx| editor.function_search_popover(ctx),
        );
        assert!(!is_function_search_open(&ctx));
        let search = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|(id, _)| *id == focus.value().into())
            .unwrap()
            .1;
        assert!(search.active_descendant().is_none());
        assert!(search.controls().is_empty());
        let output = ctx.run(input(Vec::new()), |ctx| editor.function_search_popover(ctx));
        assert!(
            output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .all(|(id, node)| *id != focus.value().into()
                    && node.role() != egui::accesskit::Role::ListBoxOption)
        );
    }

    #[test]
    fn after_effects_aep_export_is_searchable_and_exports_without_document_mutation() {
        let hits = function_search_results("after effects");
        assert_eq!(hits.first(), Some(&CommandId::FileExportAfterEffects));
        let aep_hits = function_search_results("aep");
        assert!(aep_hits.contains(&CommandId::FileExportAfterEffects));

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        let before_json = serde_json::to_string(&editor.project).unwrap();
        let dir = std::env::temp_dir().join(format!("nf-editor-aep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let builder = dir.join("demo_scene.jsx");

        assert!(editor.export_aep_dialog(None, Some(builder.clone())));
        assert!(builder.exists());
        assert!(!builder.with_extension("aep").exists());
        assert_eq!(
            serde_json::to_string(&editor.project).unwrap(),
            before_json,
            "exporting AEP must never mutate the authored document"
        );
        assert!(editor.status.contains("After Effects exported:"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
