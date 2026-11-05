//! Draft-only customization of the global tool rail (R-038).
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
    tokens::{TextStyle, space},
    typography::rich,
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
// The three containers below transcribe the approved R-038 frame geometry, using NF2 tokens.
fn list_frame(ctx: &egui::Context, rail: bool) -> egui::Frame {
    let p = neoflash_ui::palette(ctx);
    nf::Surface::PanelFlush
        .frame(ctx)
        .fill(if rail { p.bg_panel } else { p.bg_field })
        .stroke(egui::Stroke::new(
            1.0_f32,
            if rail {
                p.border_subtle
            } else {
                p.border_default
            },
        ))
        .corner_radius(if rail {
            neoflash_ui::tokens::radius::LG
        } else {
            neoflash_ui::tokens::radius::MD
        } as u8)
        .inner_margin(egui::Margin::same(space::S1 as i8))
}
fn caption(ui: &mut egui::Ui, label: impl Into<String>) {
    ui.label(
        rich(label.into(), TextStyle::Overline).color(neoflash_ui::palette(ui.ctx()).text_tertiary),
    );
}
fn rail_column(ui: &mut egui::Ui, draft: &mut CustomizeToolsDraft, catalogue: &[ToolSpec]) {
    ui.set_width(42.);
    caption(ui, "RAIL");
    list_frame(ui.ctx(), true).show(ui, |ui| {
        ui.set_width(32.);
        egui::ScrollArea::vertical()
            .id_salt("customize-rail")
            .max_height(474.)
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.;
                for index in 0..draft.slots.len() {
                    let first = draft.slots[index]
                        .first()
                        .and_then(|name| catalogue.iter().find(|s| s.name == name));
                    let response = ui
                        .add(
                            nf::ToolButton::new(
                                first.map_or(i20::PLUS, |s| s.icon.nf2(true)),
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
    });
}
fn available_column(ui: &mut egui::Ui, draft: &mut CustomizeToolsDraft, catalogue: &[ToolSpec]) {
    ui.set_width(268.);
    let mut available: Vec<_> = catalogue
        .iter()
        .filter(|s| !draft.slots.iter().flatten().any(|name| name == s.name))
        .copied()
        .collect();
    available.sort_by_key(|spec| spec.name);
    caption(ui, format!("AVAILABLE TOOLS · {}", available.len()));
    ui.add(nf::Search::new(&mut draft.query, "Find tool").width(268.));
    list_frame(ui.ctx(), false).show(ui, |ui| {
        ui.set_width(258.);
        egui::ScrollArea::vertical()
            .id_salt("available-tools")
            .max_height(442.)
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.;
                for spec in available
                    .iter()
                    .filter(|s| s.name.to_lowercase().contains(&draft.query.to_lowercase()))
                {
                    if row(
                        ui,
                        *spec,
                        draft.available.as_deref() == Some(spec.name),
                        258.,
                    )
                    .clicked()
                    {
                        draft.available = Some(spec.name.into());
                    }
                }
            });
    });
}
fn current_column(ui: &mut egui::Ui, draft: &mut CustomizeToolsDraft, catalogue: &[ToolSpec]) {
    ui.set_width(268.);
    caption(
        ui,
        format!("SLOT {} · CURRENT SELECTION", draft.selected_slot + 1),
    );
    list_frame(ui.ctx(), false).show(ui, |ui| {
        ui.set_width(258.);
        egui::ScrollArea::vertical()
            .id_salt("current-tools")
            .max_height(442.)
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.;
                let current = draft.slots[draft.selected_slot].clone();
                for (index, name) in current.iter().enumerate() {
                    let Some(spec) = catalogue.iter().find(|s| s.name == name) else {
                        continue;
                    };
                    let response = row(ui, *spec, draft.current == Some(index), 258.)
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
    });
    ui.label(rich("Tools in one slot form a flyout group. Drag to reorder; the first tool is the default shown on the rail.",TextStyle::Small).color(neoflash_ui::palette(ui.ctx()).text_tertiary));
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
        let response = egui::Modal::new(egui::Id::new("nf2-customize-tools"))
            .frame(nf::Surface::Dialog.frame(ctx))
            .backdrop_color(neoflash_ui::palette(ctx).overlay)
            .show(ctx, |ui| {
                ui.set_width(778.);
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                cancel |= nf::DialogHeader {
                    title: "Customize tools panel",
                }
                .show(ui);
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: space::S5 as i8,
                        right: space::S5 as i8,
                        top: space::S3 as i8,
                        bottom: space::S3 as i8,
                    })
                    .show(ui, |ui| {
                        ui.set_min_height(532.);
                        ui.spacing_mut().item_spacing = egui::vec2(space::S4, space::S2);
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| rail_column(ui, &mut draft, &catalogue));
                            ui.vertical(|ui| available_column(ui, &mut draft, &catalogue));
                            ui.vertical(|ui| {
                                ui.set_width(104.);
                                ui.add_space((532. - 72.) / 2.);
                                if ui
                                    .add_enabled(
                                        draft.available.is_some(),
                                        nf::Button::new("Add ›").width(104.),
                                    )
                                    .clicked()
                                {
                                    draft.add();
                                }
                                if ui
                                    .add_enabled(
                                        draft.current.is_some(),
                                        nf::Button::new("‹ Remove").width(104.),
                                    )
                                    .clicked()
                                {
                                    draft.remove();
                                }
                            });
                            ui.vertical(|ui| current_column(ui, &mut draft, &catalogue));
                        });
                    });
                let mut restore = false;
                nf::surface::dialog_footer(
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
        if apply {
            draft.apply(&mut self.workspace.tools);
            ctx.data_mut(|d| {
                for index in 0..crate::tool_preferences::TOOL_NAMES.len() {
                    d.remove::<usize>(egui::Id::new(("nf2-tool-group", index)));
                }
                d.remove::<CustomizeToolsDraft>(draft_id());
            });
        } else if cancel || response.should_close() {
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
