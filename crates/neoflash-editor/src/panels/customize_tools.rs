//! Draft-only customization of the global tool rail (R-038).
use super::dialog_helpers::{DialogShell, fixed_dialog_size, group_caption, note};
use super::tools::{ToolSpec, tool_catalogue};
use crate::theme::ScrollTheme as _;
use crate::{
    app::Editor,
    tool_preferences::{ToolPreferences, default_slots},
};
use eframe::egui;
use neoflash_ui::{
    components as nf,
    icons::i20,
    metrics,
    tokens::{TextStyle, space},
};

#[derive(Clone, Debug)]
pub(crate) struct CustomizeToolsDraft {
    pub slots: Vec<Vec<String>>,
    pub selected_slot: usize,
    pub available: Option<String>,
    pub current: Option<usize>,
    pub query: String,
    drag_tool: Option<usize>,
    drag_slot: Option<usize>,
}
impl CustomizeToolsDraft {
    pub(crate) fn new(prefs: &ToolPreferences) -> Self {
        let mut slots = prefs.slots.clone();
        // Empty slots can be selected to separate a tool from a former flyout.
        slots.resize(crate::tool_preferences::TOOL_SLOT_COUNT, vec![]);
        let current = (!slots[0].is_empty()).then_some(0);
        Self {
            slots,
            selected_slot: 0,
            available: None,
            current,
            query: String::new(),
            drag_tool: None,
            drag_slot: None,
        }
    }
    pub(crate) fn add(&mut self) {
        if let Some(name) = self.available.take() {
            if self.slots.iter().flatten().any(|tool| tool == &name) {
                return;
            }
            self.slots[self.selected_slot].push(name);
            self.current = Some(self.slots[self.selected_slot].len() - 1);
        }
    }
    pub(crate) fn remove(&mut self) {
        if let Some(index) = self
            .current
            .take()
            .filter(|i| *i < self.slots[self.selected_slot].len())
        {
            self.available = Some(self.slots[self.selected_slot].remove(index));
        }
    }
    pub(crate) fn apply(self, prefs: &mut ToolPreferences) {
        prefs.slots = self.slots;
        prefs.sanitize();
    }
}
fn draft_id() -> egui::Id {
    egui::Id::new("nf2-customize-tools-draft")
}
fn row(ui: &mut egui::Ui, spec: ToolSpec, selected: bool, width: f32) -> egui::Response {
    // ToolFlyoutItem closes menus on click; this local closable scope keeps the modal open.
    ui.scope_builder(egui::UiBuilder::new().closable(), |ui| {
        ui.add(
            nf::ToolFlyoutItem::new(spec.icon.nf2(true), &format!("{} tool", spec.name))
                .shortcut(spec.key)
                .active(selected)
                .width(width),
        )
    })
    .inner
}
// v3 `D1.customizeTools`: rail · available tools (fills) · Add / Remove · current slot (fills).
// Every column gets its size from the fixed dialog, never from its content.
fn list_frame(ctx: &egui::Context, rail: bool) -> egui::Frame {
    let p = neoflash_ui::palette(ctx);
    nf::Surface::PanelFlush
        .frame(ctx)
        .fill(if rail { p.bg_app } else { p.bg_field })
        .stroke(egui::Stroke::new(1.0_f32, p.border_subtle))
        .corner_radius(if rail {
            neoflash_ui::tokens::radius::lg()
        } else {
            neoflash_ui::tokens::radius::md()
        } as u8)
        .inner_margin(egui::Margin::same(space::s1() as i8))
}
/// Space a [`list_frame`] takes around its content on each axis (margin and border, both sides).
fn list_chrome() -> f32 {
    2.0 * (space::s1() + 1.0)
}
/// A [`list_frame`] of exactly `size` with a scroll area that fills it.
fn list_box(
    ui: &mut egui::Ui,
    id_salt: &'static str,
    rail: bool,
    size: egui::Vec2,
    add: impl FnOnce(&mut egui::Ui, f32),
) {
    let inner = (size - egui::Vec2::splat(list_chrome())).max(egui::Vec2::ZERO);
    list_frame(ui.ctx(), rail).show(ui, |ui| {
        ui.set_width(inner.x);
        ui.set_height(inner.y);
        egui::ScrollArea::vertical()
            .id_salt(id_salt)
            .max_height(inner.y)
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| add(ui, inner.x));
    });
}
fn rail_column(
    ui: &mut egui::Ui,
    draft: &mut CustomizeToolsDraft,
    catalogue: &[ToolSpec],
    size: egui::Vec2,
) {
    ui.set_width(size.x);
    group_caption(ui, "Rail");
    let list = egui::vec2(
        size.x,
        size.y - metrics::section_header() - metrics::row_gap(),
    );
    list_box(ui, "customize-rail", true, list, |ui, _| {
        ui.spacing_mut().item_spacing.y = 2.;
        for index in 0..draft.slots.len() {
            let first = draft.slots[index]
                .first()
                .and_then(|name| catalogue.iter().find(|s| s.name == name));
            let response = ui
                .add(
                    nf::ToolButton::new(
                        first.map_or(i20::PLUS, |s| s.icon.tool_glyph()),
                        &format!("Slot {}", index + 1),
                    )
                    .selected(index == draft.selected_slot)
                    .flyout(draft.slots[index].len() > 1),
                )
                .interact(egui::Sense::click_and_drag());
            if response.clicked() {
                draft.selected_slot = index;
                draft.current = (!draft.slots[index].is_empty()).then_some(0);
            }
            if response.drag_started() {
                draft.drag_slot = Some(index);
            }
            if response.contains_pointer()
                && ui.input(|i| i.pointer.any_released())
                && let Some(from) = draft.drag_slot.take()
            {
                let slot = draft.slots.remove(from);
                draft.slots.insert(index, slot);
                draft.selected_slot = index;
            }
        }
    });
}
fn available_column(
    ui: &mut egui::Ui,
    draft: &mut CustomizeToolsDraft,
    catalogue: &[ToolSpec],
    size: egui::Vec2,
) {
    ui.set_width(size.x);
    let mut available: Vec<_> = catalogue
        .iter()
        .filter(|s| !draft.slots.iter().flatten().any(|name| name == s.name))
        .copied()
        .collect();
    available.sort_by_key(|spec| spec.name);
    group_caption(ui, &format!("Available tools · {}", available.len()));
    ui.add(nf::Search::new(&mut draft.query, "Find tool").width(size.x));
    let list = egui::vec2(
        size.x,
        size.y - metrics::section_header() - metrics::control() - 2.0 * metrics::row_gap(),
    );
    list_box(ui, "available-tools", false, list, |ui, width| {
        ui.spacing_mut().item_spacing.y = 0.;
        for spec in available
            .iter()
            .filter(|s| s.name.to_lowercase().contains(&draft.query.to_lowercase()))
        {
            if row(
                ui,
                *spec,
                draft.available.as_deref() == Some(spec.name),
                width,
            )
            .clicked()
            {
                draft.available = Some(spec.name.into());
            }
        }
    });
}
/// Height reserved for the three-line hint under the current-slot list.
fn slot_hint_height() -> f32 {
    3.0 * TextStyle::Small.spec().line_height
}
fn current_column(
    ui: &mut egui::Ui,
    draft: &mut CustomizeToolsDraft,
    catalogue: &[ToolSpec],
    size: egui::Vec2,
) {
    ui.set_width(size.x);
    group_caption(
        ui,
        &format!("Slot {} · current selection", draft.selected_slot + 1),
    );
    let list = egui::vec2(
        size.x,
        size.y - metrics::section_header() - slot_hint_height() - 2.0 * metrics::row_gap(),
    );
    list_box(ui, "current-tools", false, list, |ui, width| {
        ui.spacing_mut().item_spacing.y = 0.;
        let current = draft.slots[draft.selected_slot].clone();
        for (index, name) in current.iter().enumerate() {
            let Some(spec) = catalogue.iter().find(|s| s.name == name) else {
                continue;
            };
            let response = row(ui, *spec, draft.current == Some(index), width)
                .interact(egui::Sense::click_and_drag());
            if response.clicked() {
                draft.current = Some(index);
            }
            if response.drag_started() {
                draft.drag_tool = Some(index);
            }
            if response.contains_pointer()
                && ui.input(|i| i.pointer.any_released())
                && let Some(from) = draft.drag_tool.take()
            {
                let slot = &mut draft.slots[draft.selected_slot];
                let tool = slot.remove(from);
                slot.insert(index, tool);
                draft.current = Some(index);
            }
        }
    });
    note(
        ui,
        "Tools in one slot form a flyout group. Drag to reorder; the first tool is the default shown on the rail.",
    );
}
impl Editor {
    pub(crate) fn open_customize_tools(&self, ctx: &egui::Context) {
        ctx.data_mut(|d| {
            d.insert_temp(draft_id(), CustomizeToolsDraft::new(&self.workspace.tools))
        });
    }
    pub(crate) fn customize_tools_open(&self, ctx: &egui::Context) -> bool {
        ctx.data(|d| d.get_temp::<CustomizeToolsDraft>(draft_id()).is_some())
    }
    pub(crate) fn customize_tools_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = ctx.data(|d| d.get_temp::<CustomizeToolsDraft>(draft_id())) else {
            return;
        };
        let catalogue = tool_catalogue();
        let mut cancel = false;
        let mut apply = false;
        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);
        // v3: one size per density; the lists fill it and scroll.
        let size = fixed_dialog_size(ctx, metrics::dialog_l(), metrics::dialog_xl_height());
        let response = egui::Modal::new(egui::Id::new("nf2-customize-tools"))
            .frame(nf::Surface::Dialog.frame(ctx))
            .backdrop_color(neoflash_ui::palette(ctx).overlay)
            .show(ctx, |ui| {
                let shell = DialogShell::single(ui, "Customize tools", size);
                cancel |= shell.close;
                shell.page(ui, "customize-tools-page", |ui, area| {
                    // Four columns of one height: the page minus its padding.
                    let height = area.fill_height(ui, "customize-tools", 0.0, metrics::control());
                    let gap = space::s4();
                    let rail_w = metrics::tools_rail();
                    let actions_w = metrics::field_s();
                    let list_w =
                        ((ui.available_width() - rail_w - actions_w - 3.0 * gap) / 2.0).floor();
                    ui.spacing_mut().item_spacing.x = gap;
                    ui.horizontal_top(|ui| {
                        ui.set_height(height);
                        ui.vertical(|ui| {
                            rail_column(ui, &mut draft, &catalogue, egui::vec2(rail_w, height))
                        });
                        ui.vertical(|ui| {
                            available_column(ui, &mut draft, &catalogue, egui::vec2(list_w, height))
                        });
                        ui.vertical(|ui| {
                            ui.set_width(actions_w);
                            ui.add_space(
                                ((height - 2.0 * metrics::control() - metrics::row_gap()) / 2.0)
                                    .max(0.0),
                            );
                            if ui
                                .add_enabled(
                                    draft.available.is_some(),
                                    nf::Button::new("Add ›").width(actions_w),
                                )
                                .clicked()
                            {
                                draft.add();
                            }
                            if ui
                                .add_enabled(
                                    draft.current.is_some(),
                                    nf::Button::new("‹ Remove").width(actions_w),
                                )
                                .clicked()
                            {
                                draft.remove();
                            }
                        });
                        ui.vertical(|ui| {
                            current_column(ui, &mut draft, &catalogue, egui::vec2(list_w, height))
                        });
                    });
                });
                let mut restore = false;
                shell.footer(
                    ui,
                    |ui| {
                        restore = ui.add(nf::Button::ghost("Restore default")).clicked();
                    },
                    |ui| {
                        apply = ui.add(nf::Button::primary("OK")).clicked();
                        cancel |= ui.add(nf::Button::new("Cancel")).clicked();
                    },
                );
                if restore {
                    draft = CustomizeToolsDraft::new(&ToolPreferences {
                        slots: default_slots(),
                        ..Default::default()
                    });
                }
            });
        if !apply && !cancel {
            match kb.resolve(ctx, &response, true) {
                super::dialog_helpers::DialogKeyboardOutcome::Cancel => cancel = true,
                super::dialog_helpers::DialogKeyboardOutcome::Confirm => apply = true,
                super::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
        }
        if apply {
            draft.apply(&mut self.workspace.tools);
            ctx.data_mut(|d| {
                for index in 0..crate::tool_preferences::TOOL_NAMES.len() {
                    d.remove::<usize>(egui::Id::new(("nf2-tool-group", index)));
                }
                d.remove::<CustomizeToolsDraft>(draft_id());
            });
        } else if cancel {
            ctx.data_mut(|d| d.remove::<CustomizeToolsDraft>(draft_id()));
        } else {
            if ctx.input(|i| i.pointer.any_released()) {
                draft.drag_tool = None;
                draft.drag_slot = None;
            }
            ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
        }
    }
}
