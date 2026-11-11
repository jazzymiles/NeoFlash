#![allow(unused_imports)]
use super::*;
use crate::theme::{ScrollTheme as _, ThemeMode};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use neoflash_ui::components as c;
use neoflash_ui::components::timeline::{
    self as tl, GridKind, LayerColumns, LayerKind, LayerRow, LayersHeaderColumns, ModeCell,
};
use neoflash_ui::icons::{self, Icon, i16};
use neoflash_ui::tokens::{Palette, TextStyle, radius, space};
use neoflash_ui::typography::rich;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use super::hierarchy::resolve_icon_by_name;

pub(super) fn render_live_canvas_column(
    ui: &mut Ui,
    state: &mut UiLayoutStudioState,
    width: f32,
    height: f32,
) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.set_width(width);
    state.sync_active_to_artboard();

    // Canvas Header Controls: Outlines, %/px Badges, Solo Active, Zoom, Auto-Arrange, Open All Core Panels
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        ui.label(rich("MULTI-PANEL ARTBOARD CANVAS", TextStyle::Overline).color(nf.text_secondary));
        ui.add(c::Checkbox::new(
            &mut state.show_wireframe_outlines,
            "Outlines",
        ));
        ui.add(c::Checkbox::new(
            &mut state.show_dimension_badges,
            "% / px Badges",
        ));
        ui.add(c::Checkbox::new(
            &mut state.solo_active_artboard,
            "Solo Active Panel",
        ));

        c::divider(ui, true);

        // Zoom controls (- / 100% / +)
        if ui.add(c::Button::new("−")).clicked() {
            state.canvas_zoom = (state.canvas_zoom - 0.1).clamp(0.35, 2.5);
        }
        let zoom_lbl = format!("{:.0}%", state.canvas_zoom * 100.0);
        if ui.add(c::Button::new(&zoom_lbl)).clicked() {
            state.canvas_zoom = 1.0;
            state.canvas_pan = [0.0, 0.0];
        }
        if ui.add(c::Button::new("+")).clicked() {
            state.canvas_zoom = (state.canvas_zoom + 0.1).clamp(0.35, 2.5);
        }

        if ui.add(c::Button::new("Open All 6 Panels")).clicked() {
            state.open_all_core_artboards();
        }
        if ui.add(c::Button::new("Auto-Arrange")).clicked() {
            state.auto_arrange_artboards();
        }

        let mut density = state.systems.density.active_profile;
        if c::Select::options(
            ui,
            "studio-canvas-density",
            &mut density,
            &[
                (StudioDensityProfile::Comfortable, "Comfortable (32pt)"),
                (StudioDensityProfile::Compact, "Compact (28pt)"),
                (StudioDensityProfile::ClassicFlash, "Classic Flash (24pt)"),
            ],
            144.0,
        ) {
            state.systems.density.active_profile = density;
        }
    });

    let canvas_h = (height - 40.0).max(240.0);
    let theme = state.preview_theme;
    let artboard_shadow = theme.nf2().palette().overlay;
    let stage_bg = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BgApp);
    let border_col = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BorderSubtle);

    egui::Frame::new()
        .fill(stage_bg)
        .corner_radius(egui::CornerRadius::same(radius::LG as u8))
        .stroke(Stroke::new(1.0_f32, border_col))
        .inner_margin(egui::Margin::same(space::S2 as i8))
        .show(ui, |ui| {
            ui.set_min_size(egui::vec2(
                width - space::S2 * 2.0,
                canvas_h - space::S2 * 2.0,
            ));
            egui::ScrollArea::both()
                .id_salt("studio-live-canvas-scroll")
                .max_height(canvas_h - space::S2 * 2.0)
                .auto_shrink([false, false])
                .themed()
                .show(ui, |ui| {
                    let zoom = state.canvas_zoom.clamp(0.35, 2.5);
                    let indices: Vec<usize> = if state.solo_active_artboard {
                        vec![state.active_artboard_idx.min(state.artboards.len().saturating_sub(1))]
                    } else {
                        state
                            .artboards
                            .iter()
                            .enumerate()
                            .filter(|(_, a)| a.visible)
                            .map(|(i, _)| i)
                            .collect()
                    };

                    // Compute required 2D board dimensions so no panel is ever clipped
                    let mut max_x = (width - space::S3 * 2.0).max(1380.0);
                    let mut max_y = (canvas_h - space::S3 * 2.0).max(960.0);
                    for &idx in &indices {
                        if let Some(ab) = state.artboards.get(idx) {
                            let px = if state.solo_active_artboard {
                                24.0
                            } else {
                                ab.canvas_pos[0] + state.canvas_pan[0]
                            };
                            let py = if state.solo_active_artboard {
                                28.0
                            } else {
                                ab.canvas_pos[1] + state.canvas_pan[1]
                            };
                            max_x = max_x.max(px + ab.canvas_size[0] * zoom + 48.0);
                            max_y = max_y.max(py + ab.canvas_size[1] * zoom + 64.0);
                        }
                    }

                    let board_origin = ui.cursor().min;
                    let (board_rect, board_resp) = ui.allocate_exact_size(
                        egui::vec2(max_x, max_y),
                        Sense::click_and_drag(),
                    );

                    // Subtle pasteboard dot-grid on the multi-panel artboard canvas
                    let grid_step = (24.0 * zoom).clamp(14.0, 48.0);
                    let dot_col = border_col.gamma_multiply(0.45);
                    let mut gx = board_rect.left() + 12.0;
                    while gx < board_rect.right() - 4.0 {
                        let mut gy = board_rect.top() + 12.0;
                        while gy < board_rect.bottom() - 4.0 {
                            ui.painter()
                                .circle_filled(egui::pos2(gx, gy), 1.0, dot_col);
                            gy += grid_step;
                        }
                        gx += grid_step;
                    }

                    // Middle-mouse or Space+drag pans the entire board
                    if board_resp.dragged_by(egui::PointerButton::Middle)
                        || (board_resp.dragged() && ui.input(|i| i.key_down(egui::Key::Space)))
                    {
                        let d = board_resp.drag_delta();
                        state.canvas_pan[0] = (state.canvas_pan[0] + d.x).max(-600.0);
                        state.canvas_pan[1] = (state.canvas_pan[1] + d.y).max(-600.0);
                    }

                    let mut pending_select_artboard: Option<usize> = None;
                    let mut pending_dup_artboard: Option<usize> = None;
                    let mut pending_remove_artboard: Option<usize> = None;
                    let mut pending_svg_export_artboard: Option<usize> = None;

                    for &idx in &indices {
                        let Some(ab_snapshot) = state.artboards.get(idx).cloned() else {
                            continue;
                        };
                        let is_active = idx == state.active_artboard_idx;
                        let pos_x = if state.solo_active_artboard {
                            24.0
                        } else {
                            (ab_snapshot.canvas_pos[0] + state.canvas_pan[0]).max(8.0)
                        };
                        let pos_y = if state.solo_active_artboard {
                            32.0
                        } else {
                            (ab_snapshot.canvas_pos[1] + state.canvas_pan[1]).max(28.0)
                        };

                        let ab_w = (ab_snapshot.canvas_size[0] * zoom).max(76.0);
                        let ab_h = (ab_snapshot.canvas_size[1] * zoom).max(80.0);
                        let ab_rect = Rect::from_min_size(
                            board_origin + egui::vec2(pos_x, pos_y),
                            egui::vec2(ab_w, ab_h),
                        );

                        // ── 1. Artboard Header Bar (draggable title + [W×H px] + [SVG] [Dup] [✕]) ──
                        let hdr_h = 22.0_f32;
                        let hdr_rect = Rect::from_min_size(
                            egui::pos2(ab_rect.left(), ab_rect.top() - hdr_h - 2.0),
                            egui::vec2(ab_rect.width().max(150.0), hdr_h),
                        );
                        let hdr_bg = if is_active {
                            nf.accent_default.gamma_multiply(0.24)
                        } else {
                            nf.bg_raised
                        };
                        let hdr_border = if is_active {
                            nf.accent_default
                        } else {
                            nf.border_subtle
                        };
                        ui.painter().rect_filled(hdr_rect, 4.0, hdr_bg);
                        ui.painter().rect_stroke(
                            hdr_rect,
                            4.0,
                            Stroke::new(1.0_f32, hdr_border),
                            StrokeKind::Inside,
                        );

                        // Active indicator dot + title + size badge
                        let dot_c = if is_active {
                            nf.accent_default
                        } else {
                            nf.text_tertiary
                        };
                        ui.painter().circle_filled(
                            egui::pos2(hdr_rect.left() + 8.0, hdr_rect.center().y),
                            3.5,
                            dot_c,
                        );
                        let title_txt = if let Some(ab_theme) = ab_snapshot.theme_override {
                            format!(
                                "{}  [{}]  ({:.0}×{:.0}px)",
                                ab_snapshot.document.name,
                                ab_theme.label(),
                                ab_snapshot.canvas_size[0],
                                ab_snapshot.canvas_size[1]
                            )
                        } else {
                            format!(
                                "{}  ({:.0}×{:.0}px)",
                                ab_snapshot.document.name,
                                ab_snapshot.canvas_size[0],
                                ab_snapshot.canvas_size[1]
                            )
                        };
                        ui.painter().text(
                            egui::pos2(hdr_rect.left() + 16.0, hdr_rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            title_txt,
                            neoflash_ui::typography::font(TextStyle::MonoSmall),
                            if is_active {
                                nf.text_primary
                            } else {
                                nf.text_secondary
                            },
                        );

                        // Header action pills on the right: [SVG] [Dup] [✕]
                        let close_r = Rect::from_min_size(
                            egui::pos2(hdr_rect.right() - 20.0, hdr_rect.top() + 2.0),
                            egui::vec2(18.0, hdr_h - 4.0),
                        );
                        let dup_r = Rect::from_min_size(
                            egui::pos2(close_r.left() - 30.0, hdr_rect.top() + 2.0),
                            egui::vec2(28.0, hdr_h - 4.0),
                        );
                        let svg_r = Rect::from_min_size(
                            egui::pos2(dup_r.left() - 30.0, hdr_rect.top() + 2.0),
                            egui::vec2(28.0, hdr_h - 4.0),
                        );
                        if hdr_rect.width() >= 190.0 {
                            for (pill_r, pill_txt) in [
                                (svg_r, "SVG"),
                                (dup_r, "Dup"),
                                (close_r, "×"),
                            ] {
                                ui.painter()
                                    .rect_filled(pill_r, 3.0, nf.bg_panel.gamma_multiply(0.85));
                                ui.painter().text(
                                    pill_r.center(),
                                    egui::Align2::CENTER_CENTER,
                                    pill_txt,
                                    egui::FontId::monospace(8.5),
                                    nf.text_secondary,
                                );
                            }
                        }

                        let hdr_drag_rect = Rect::from_min_max(
                            hdr_rect.min,
                            egui::pos2(
                                if hdr_rect.width() >= 190.0 {
                                    svg_r.left() - 2.0
                                } else {
                                    hdr_rect.right()
                                },
                                hdr_rect.bottom(),
                            ),
                        );
                        let hdr_resp = ui.interact(
                            hdr_drag_rect,
                            ui.id().with(("studio-ab-hdr", ab_snapshot.id)),
                            Sense::click_and_drag(),
                        );
                        if hdr_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                        if hdr_resp.clicked() || hdr_resp.drag_started() {
                            pending_select_artboard = Some(idx);
                        }
                        if hdr_resp.dragged() && !state.solo_active_artboard {
                            let d = hdr_resp.drag_delta();
                            if let Some(ab_mut) = state.artboards.get_mut(idx) {
                                ab_mut.canvas_pos[0] = (ab_mut.canvas_pos[0] + d.x).max(8.0);
                                ab_mut.canvas_pos[1] = (ab_mut.canvas_pos[1] + d.y).max(28.0);
                            }
                        }

                        if hdr_rect.width() >= 190.0 {
                            if ui
                                .interact(
                                    svg_r,
                                    ui.id().with(("studio-ab-svg", ab_snapshot.id)),
                                    Sense::click(),
                                )
                                .clicked()
                            {
                                pending_svg_export_artboard = Some(idx);
                            }
                            if ui
                                .interact(
                                    dup_r,
                                    ui.id().with(("studio-ab-dup", ab_snapshot.id)),
                                    Sense::click(),
                                )
                                .clicked()
                            {
                                pending_dup_artboard = Some(idx);
                            }
                            if ui
                                .interact(
                                    close_r,
                                    ui.id().with(("studio-ab-close", ab_snapshot.id)),
                                    Sense::click(),
                                )
                                .clicked()
                            {
                                pending_remove_artboard = Some(idx);
                            }
                        }

                        // ── 2. Artboard Surface Shadow & Live Unclipped Layout Rendering ──
                        ui.painter().rect_filled(
                            ab_rect.translate(egui::vec2(2.0, 3.0)),
                            6.0,
                            artboard_shadow,
                        );

                        let doc_for_layout = if is_active {
                            &state.document
                        } else {
                            &ab_snapshot.document
                        };
                        let mut computed = compute_studio_layout(
                            doc_for_layout,
                            &state.systems,
                            ab_rect.min,
                            ab_rect.size(),
                        );

                        let ab_theme = ab_snapshot.theme_override.unwrap_or(theme);
                        let saved_ctx_theme = ThemeMode::current(ui.ctx());
                        if ab_theme != saved_ctx_theme {
                            ab_theme.apply(ui.ctx());
                        }
                        ui.push_id(("studio-artboard", ab_snapshot.id), |ui| {
                            if ab_snapshot.preset.is_full_shell() {
                                paint_full_workspace_shell_canvas(
                                    ui,
                                    state,
                                    ab_rect,
                                    &mut computed,
                                    ab_theme,
                                );
                            } else if ab_snapshot.preset.is_timeline_surface() {
                                paint_timeline_studio_canvas(
                                    ui,
                                    state,
                                    ab_rect,
                                    &mut computed,
                                    ab_theme,
                                );
                            } else if is_active {
                                paint_node_wysiwyg(
                                    ui,
                                    &mut state.document.root,
                                    &state.systems,
                                    &computed,
                                    ab_theme,
                                );
                            } else if let Some(ab_mut) = state.artboards.get_mut(idx) {
                                paint_node_wysiwyg(
                                    ui,
                                    &mut ab_mut.document.root,
                                    &state.systems,
                                    &computed,
                                    ab_theme,
                                );
                            }
                        });
                        if ab_theme != saved_ctx_theme {
                            saved_ctx_theme.apply(ui.ctx());
                        }

                        // Active artboard highlight frame + pointer hit-testing across all artboards
                        if is_active {
                            ui.painter().rect_stroke(
                                ab_rect,
                                6.0,
                                Stroke::new(1.5_f32, nf.accent_default.gamma_multiply(0.85)),
                                StrokeKind::Outside,
                            );
                            paint_canvas_overlays(ui, state, &computed, ab_theme);
                        }

                        // Pointer click inside `ab_rect` selects this artboard and hit-tests its nodes
                        if let Some(pos) = ui.input(|i| i.pointer.hover_pos())
                            && ab_rect.contains(pos)
                            && is_active
                        {
                            state.hovered_node = computed.hit_test(pos);
                        }
                        if let Some(pos) = ui.input(|i| i.pointer.interact_pos())
                            && ab_rect.contains(pos)
                            && (board_resp.clicked() || ui.input(|i| i.pointer.primary_clicked()))
                        {
                            if !is_active {
                                pending_select_artboard = Some(idx);
                            }
                            if let Some(hit_id) = computed.hit_test(pos) {
                                let multi =
                                    ui.input(|i| i.modifiers.shift || i.modifiers.command);
                                if is_active {
                                    if multi {
                                        state.toggle_multi_select(hit_id);
                                    } else {
                                        state.select_single(hit_id);
                                    }
                                    if let Some(node) = state.document.find_node(hit_id) {
                                        state.status_message = format!(
                                            "Selected '{}' (#{hit_id}) on '{}' — edit % / px or Systems in right inspector.",
                                            node.label, state.document.name
                                        );
                                    }
                                }
                            }
                        }

                        // ── 3. Interactive Bottom-Right & Right-Edge Resize Grips (live % vs px test!) ──
                        let grip_rect = Rect::from_center_size(
                            ab_rect.right_bottom(),
                            egui::vec2(14.0, 14.0),
                        );
                        ui.painter().rect_filled(
                            grip_rect,
                            3.0,
                            if is_active {
                                nf.accent_default
                            } else {
                                nf.border_strong
                            },
                        );
                        let grip_resp = ui.interact(
                            grip_rect,
                            ui.id().with(("studio-ab-resize", ab_snapshot.id)),
                            Sense::drag(),
                        );
                        if grip_resp.hovered() || grip_resp.dragged() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                        }
                        if grip_resp.dragged() {
                            let d = grip_resp.drag_delta() / zoom;
                            if let Some(ab_mut) = state.artboards.get_mut(idx) {
                                ab_mut.canvas_size[0] =
                                    (ab_mut.canvas_size[0] + d.x).clamp(76.0, 2400.0);
                                ab_mut.canvas_size[1] =
                                    (ab_mut.canvas_size[1] + d.y).clamp(80.0, 1800.0);
                                ab_mut.document.root.box_constraints.width =
                                    UiSizeUnit::Percent(100.0);
                                if is_active {
                                    state.document.root.box_constraints.width =
                                        UiSizeUnit::Percent(100.0);
                                }
                                state.status_message = format!(
                                    "Resized '{}' artboard to {:.0}×{:.0}px (testing live % vs px layout).",
                                    ab_mut.document.name,
                                    ab_mut.canvas_size[0],
                                    ab_mut.canvas_size[1]
                                );
                            }
                        }
                    }

                    if let Some(idx) = pending_select_artboard {
                        state.select_artboard(idx);
                    }
                    if let Some(idx) = pending_svg_export_artboard {
                        state.select_artboard(idx);
                        let svg = state.export_active_svg();
                        ui.ctx().copy_text(svg.clone());
                        state.svg_buffer = svg;
                        let _ = state.save_svg_to_file(Path::new(DEFAULT_STUDIO_SVG_EXPORT_PATH));
                        state.inspector_tab = StudioInspectorTab::CodeExport;
                        state.status_message = format!(
                            "Exported '{}' SVG to {DEFAULT_STUDIO_SVG_EXPORT_PATH} & clipboard.",
                            state.document.name
                        );
                    }
                    if let Some(idx) = pending_dup_artboard {
                        state.duplicate_artboard(idx);
                    }
                    if let Some(idx) = pending_remove_artboard {
                        state.remove_artboard(idx);
                    }
                });
        });
}

#[derive(Clone, Copy, Debug)]
struct TimelineHitIds {
    layers_col: UiNodeId,
    ruler_playhead: UiNodeId,
    classic_tween: UiNodeId,
    shape_tween: UiNodeId,
    motion_tween: UiNodeId,
    broken_tween: UiNodeId,
    keyframes_badges: UiNodeId,
    audio_waveform: UiNodeId,
}

fn register_hit_box(
    computed: &mut ComputedStudioLayout,
    systems: &UiStudioSystems,
    id: UiNodeId,
    rect: Rect,
) {
    let typo = TypographyBinding::System(StudioTextStyle::Small).resolve(systems);
    computed.boxes.insert(
        id,
        ComputedNodeBox {
            id,
            rect,
            content_rect: rect,
            resolved_width: rect.width(),
            resolved_height: rect.height(),
            padding: [0.0; 4],
            gap: Vec2::ZERO,
            typography: typo,
        },
    );
    if let Some(pos) = computed.order.iter().position(|&x| x == id) {
        computed.order.remove(pos);
    }
    computed.order.push(id);
}

fn paint_full_workspace_shell_canvas(
    ui: &mut Ui,
    state: &mut UiLayoutStudioState,
    shell_rect: Rect,
    computed: &mut ComputedStudioLayout,
    theme: ThemeMode,
) {
    computed.root_rect = shell_rect;
    computed.order.clear();
    register_hit_box(computed, &state.systems, 1, shell_rect);

    let nf = theme.nf2().palette();
    let bg_app = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BgApp);
    let bg_panel = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BgPanel);
    let bg_raised = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BgRaised);
    let border_sub = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BorderSubtle);
    let border_def = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BorderDefault);
    let text_pri = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::TextPrimary);
    let text_sec = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::TextSecondary);
    let accent = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::AccentDefault);

    let rects = state
        .systems
        .workspace_shell
        .resolve_shell_rects(shell_rect);
    ui.painter().rect_filled(shell_rect, 6.0, bg_app);
    ui.painter().rect_stroke(
        shell_rect,
        6.0,
        Stroke::new(1.0_f32, border_def),
        StrokeKind::Inside,
    );

    // 1. Top Menu Bar + Document Tabs + Edit Breadcrumb Bar (Node 2)
    let top_bars_rect = rects.menubar.union(rects.doc_tabs).union(rects.edit_bar);
    register_hit_box(computed, &state.systems, 2, top_bars_rect);

    ui.painter().rect_filled(rects.menubar, 0.0, bg_raised);
    let menus = [
        "File", "Edit", "View", "Insert", "Modify", "Text", "Commands", "Control", "Window", "Help",
    ];
    let mut mx = rects.menubar.left() + 10.0;
    for m in menus {
        ui.painter().text(
            egui::pos2(mx, rects.menubar.center().y),
            egui::Align2::LEFT_CENTER,
            m,
            neoflash_ui::typography::font(TextStyle::Small),
            text_pri,
        );
        mx += (m.len() as f32) * 6.4 + 12.0;
    }
    ui.painter().text(
        egui::pos2(rects.menubar.right() - 10.0, rects.menubar.center().y),
        egui::Align2::RIGHT_CENTER,
        "Animator · HTML5 / WASM",
        neoflash_ui::typography::font(TextStyle::MonoSmall),
        text_sec,
    );

    // Document Tabs Bar
    ui.painter().rect_filled(rects.doc_tabs, 0.0, bg_panel);
    let tab1_rect = Rect::from_min_size(
        egui::pos2(rects.doc_tabs.left() + 6.0, rects.doc_tabs.top() + 3.0),
        egui::vec2(108.0, rects.doc_tabs.height() - 3.0),
    );
    let tab2_rect = Rect::from_min_size(
        egui::pos2(tab1_rect.right() + 4.0, rects.doc_tabs.top() + 3.0),
        egui::vec2(114.0, rects.doc_tabs.height() - 3.0),
    );
    ui.painter().rect_filled(tab1_rect, 4.0, bg_raised);
    ui.painter().hline(
        tab1_rect.x_range(),
        tab1_rect.top() + 1.0,
        Stroke::new(2.0_f32, accent),
    );
    ui.painter().text(
        tab1_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Scene 1 *",
        neoflash_ui::typography::font(TextStyle::SmallStrong),
        text_pri,
    );
    ui.painter()
        .rect_filled(tab2_rect, 4.0, bg_panel.gamma_multiply(0.8));
    ui.painter().text(
        tab2_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Character_Rig",
        neoflash_ui::typography::font(TextStyle::Small),
        text_sec,
    );

    // Edit Breadcrumb Bar
    ui.painter().rect_filled(rects.edit_bar, 0.0, bg_raised);
    ui.painter().hline(
        rects.edit_bar.x_range(),
        rects.edit_bar.bottom() - 0.5,
        Stroke::new(1.0_f32, border_sub),
    );
    ui.painter().text(
        egui::pos2(rects.edit_bar.left() + 12.0, rects.edit_bar.center().y),
        egui::Align2::LEFT_CENTER,
        "Scene 1   ▸   Hero_MC (Movie Clip)",
        neoflash_ui::typography::font(TextStyle::Small),
        text_pri,
    );
    ui.painter().text(
        egui::pos2(rects.edit_bar.right() - 12.0, rects.edit_bar.center().y),
        egui::Align2::RIGHT_CENTER,
        "Zoom: 100%  |  Fit Stage",
        neoflash_ui::typography::font(TextStyle::MonoSmall),
        text_sec,
    );

    // 2. Left Tools Dock Rail (Node 3)
    register_hit_box(computed, &state.systems, 3, rects.left_tools);
    ui.painter().rect_filled(rects.left_tools, 4.0, bg_panel);
    ui.painter().rect_stroke(
        rects.left_tools,
        4.0,
        Stroke::new(1.0_f32, border_sub),
        StrokeKind::Inside,
    );
    let tool_icons = [
        (i16::SELECT, true),
        (i16::SUBSELECT, false),
        (i16::TRANSFORM, false),
        (i16::PEN, false),
        (i16::TEXT, false),
        (i16::LINE, false),
        (i16::RECT, false),
        (i16::OVAL, false),
        (i16::PENCIL, false),
        (i16::BRUSH, false),
        (i16::BUCKET, false),
        (i16::EYEDROPPER, false),
    ];
    let cols = state.systems.tool_rail.columns.clamp(1, 2);
    let btn_side = state.systems.tool_rail.button_side.clamp(
        20.0,
        (rects.left_tools.width() / cols as f32 - 4.0).max(20.0),
    );
    let start_x = rects.left_tools.left()
        + ((rects.left_tools.width() - cols as f32 * (btn_side + 2.0)).max(4.0)) * 0.5;
    let mut ty = rects.left_tools.top() + 6.0;
    for chunk in tool_icons.chunks(cols) {
        for (c_idx, &(ico, active)) in chunk.iter().enumerate() {
            let bx = start_x + c_idx as f32 * (btn_side + 2.0);
            let b_rect = Rect::from_min_size(egui::pos2(bx, ty), egui::vec2(btn_side, btn_side));
            if b_rect.bottom() < rects.left_tools.bottom() - 40.0 {
                if active {
                    ui.painter()
                        .rect_filled(b_rect, 4.0, accent.gamma_multiply(0.28));
                    ui.painter().rect_stroke(
                        b_rect,
                        4.0,
                        Stroke::new(1.0_f32, accent),
                        StrokeKind::Inside,
                    );
                }
                icons::paint(
                    ui.painter(),
                    ui.ctx(),
                    Rect::from_center_size(b_rect.center(), egui::vec2(14.0, 14.0)),
                    ico,
                    if active { text_pri } else { text_sec },
                );
            }
        }
        ty += btn_side + 2.0;
    }
    // Stroke & Fill wells at bottom of Tools rail
    let well_y = (rects.left_tools.bottom() - 28.0).max(rects.left_tools.top() + 8.0);
    let sw_rect = Rect::from_center_size(
        egui::pos2(rects.left_tools.center().x - 6.0, well_y),
        egui::vec2(14.0, 14.0),
    );
    let fw_rect = Rect::from_center_size(
        egui::pos2(rects.left_tools.center().x + 6.0, well_y + 6.0),
        egui::vec2(14.0, 14.0),
    );
    ui.painter()
        .rect_filled(sw_rect, 2.0, Color32::from_rgb(108, 147, 255));
    ui.painter()
        .rect_filled(fw_rect, 2.0, Color32::from_rgb(255, 134, 82));

    // 3. Center Stage Viewport with Rulers, Cyan Guides & Selected Vector Instance (Node 4)
    register_hit_box(computed, &state.systems, 4, rects.center_stage);
    paint_stage_viewport_mock(ui, state, rects.center_stage, theme);

    // 4. Right Multi-Panel Inspector Dock (Node 5)
    register_hit_box(computed, &state.systems, 5, rects.right_inspector);
    paint_right_inspector_dock_mock(ui, state, rects.right_inspector, theme);

    // 5. Bottom Timeline Dock (Node 6 + Sub-Nodes 7..14)
    register_hit_box(computed, &state.systems, 6, rects.bottom_timeline);
    let hit_ids = TimelineHitIds {
        layers_col: 7,
        ruler_playhead: 8,
        classic_tween: 9,
        shape_tween: 10,
        motion_tween: 11,
        broken_tween: 12,
        keyframes_badges: 13,
        audio_waveform: 14,
    };
    let _ = nf;
    paint_interactive_timeline_surface(ui, state, rects.bottom_timeline, computed, hit_ids, theme);
}

fn paint_stage_viewport_mock(
    ui: &mut Ui,
    state: &UiLayoutStudioState,
    rect: Rect,
    theme: ThemeMode,
) {
    let palette = theme.nf2().palette();
    let pasteboard = palette.bg_canvas;
    let stage = palette.bg_stage;
    let ruler_bg = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BgRaised);
    let border_sub = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BorderSubtle);
    let text_sec = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::TextSecondary);
    let accent = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::AccentDefault);

    ui.painter().rect_filled(rect, 4.0, pasteboard);
    ui.painter().rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, border_sub),
        StrokeKind::Inside,
    );

    let r_thick = if state.systems.stage_overlay.show_rulers {
        state
            .systems
            .stage_overlay
            .stage_ruler_thickness_px
            .clamp(12.0, 32.0)
    } else {
        0.0
    };

    if r_thick > 0.0 {
        let top_ruler = Rect::from_min_size(rect.min, egui::vec2(rect.width(), r_thick));
        let left_ruler = Rect::from_min_size(rect.min, egui::vec2(r_thick, rect.height()));
        ui.painter().rect_filled(top_ruler, 0.0, ruler_bg);
        ui.painter().rect_filled(left_ruler, 0.0, ruler_bg);
        ui.painter().hline(
            top_ruler.x_range(),
            top_ruler.bottom(),
            Stroke::new(1.0_f32, border_sub),
        );
        ui.painter().vline(
            left_ruler.right(),
            left_ruler.y_range(),
            Stroke::new(1.0_f32, border_sub),
        );

        let mut tx = top_ruler.left() + r_thick + 20.0;
        let mut val = 0_i32;
        while tx < top_ruler.right() - 12.0 {
            ui.painter().vline(
                tx,
                (top_ruler.bottom() - 6.0)..=top_ruler.bottom(),
                Stroke::new(1.0_f32, border_sub),
            );
            ui.painter().text(
                egui::pos2(tx + 2.0, top_ruler.center().y),
                egui::Align2::LEFT_CENTER,
                format!("{val}"),
                egui::FontId::monospace(8.5),
                text_sec,
            );
            tx += 50.0;
            val += 100;
        }
        let mut ty = left_ruler.top() + r_thick + 20.0;
        while ty < left_ruler.bottom() - 10.0 {
            ui.painter().hline(
                (left_ruler.right() - 6.0)..=left_ruler.right(),
                ty,
                Stroke::new(1.0_f32, border_sub),
            );
            ty += 50.0;
        }
    }

    let inner = Rect::from_min_max(rect.min + egui::vec2(r_thick, r_thick), rect.max);
    let stage_w = (inner.width() * 0.72).clamp(140.0, 460.0);
    let stage_h = (stage_w
        * (state.systems.stage_overlay.stage_aspect_h
            / state.systems.stage_overlay.stage_aspect_w.max(1.0)))
    .clamp(90.0, (inner.height() - 24.0).max(90.0));
    let stage_rect = Rect::from_center_size(inner.center(), egui::vec2(stage_w, stage_h));

    // Stage paper shadow + surface
    ui.painter().rect_filled(
        stage_rect.translate(egui::vec2(3.0, 4.0)),
        2.0,
        palette.overlay,
    );
    ui.painter().rect_filled(stage_rect, 2.0, stage);

    // Cyan guides
    if state.systems.stage_overlay.show_guides {
        let cyan = palette.guide;
        let gw = state
            .systems
            .stage_overlay
            .guide_stroke_width_px
            .clamp(0.5, 3.0);
        let gx = stage_rect.left() + stage_rect.width() * 0.38;
        let gy = stage_rect.top() + stage_rect.height() * 0.54;
        ui.painter()
            .vline(gx, inner.y_range(), Stroke::new(gw, cyan));
        ui.painter()
            .hline(inner.x_range(), gy, Stroke::new(gw, cyan));
    }

    // Motion guide path & vector character instance with 8 selection handles
    let hero_rect = Rect::from_center_size(
        stage_rect.center(),
        egui::vec2(stage_w * 0.34, stage_h * 0.42),
    );
    ui.painter().circle_filled(
        hero_rect.center(),
        hero_rect.height().min(hero_rect.width()) * 0.38,
        Color32::from_rgb(255, 126, 76),
    );
    ui.painter().rect_stroke(
        hero_rect,
        0.0,
        Stroke::new(1.25_f32, accent),
        StrokeKind::Outside,
    );
    let hs = state
        .systems
        .stage_overlay
        .selection_handle_size_px
        .clamp(3.0, 12.0);
    for pt in [
        hero_rect.left_top(),
        egui::pos2(hero_rect.center().x, hero_rect.top()),
        hero_rect.right_top(),
        egui::pos2(hero_rect.right(), hero_rect.center().y),
        hero_rect.right_bottom(),
        egui::pos2(hero_rect.center().x, hero_rect.bottom()),
        hero_rect.left_bottom(),
        egui::pos2(hero_rect.left(), hero_rect.center().y),
    ] {
        let h_rect = Rect::from_center_size(pt, egui::vec2(hs, hs));
        ui.painter().rect_filled(h_rect, 1.0, Color32::WHITE);
        ui.painter().rect_stroke(
            h_rect,
            1.0,
            Stroke::new(1.0_f32, accent),
            StrokeKind::Outside,
        );
    }
    // Transformation pivot circle
    ui.painter()
        .circle_filled(hero_rect.center(), 3.5, Color32::WHITE);
    ui.painter().circle_stroke(
        hero_rect.center(),
        3.5,
        Stroke::new(1.2_f32, Color32::BLACK),
    );
}

fn paint_right_inspector_dock_mock(
    ui: &mut Ui,
    state: &UiLayoutStudioState,
    rect: Rect,
    theme: ThemeMode,
) {
    let bg_panel = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BgPanel);
    let bg_raised = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BgRaised);
    let bg_field = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BgField);
    let border_sub = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BorderSubtle);
    let text_pri = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::TextPrimary);
    let text_sec = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::TextSecondary);
    let accent = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::AccentDefault);

    ui.painter().rect_filled(rect, 4.0, bg_panel);
    ui.painter().rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, border_sub),
        StrokeKind::Inside,
    );

    let split_y = rect.top() + rect.height() * 0.54;
    let top_pnl = Rect::from_min_max(rect.min, egui::pos2(rect.right(), split_y - 2.0));
    let bot_pnl = Rect::from_min_max(egui::pos2(rect.left(), split_y + 2.0), rect.max);

    // Top Panel: Properties / Filters
    let hdr_h = state.systems.panel_header.height.clamp(22.0, 36.0);
    let top_hdr = Rect::from_min_size(top_pnl.min, egui::vec2(top_pnl.width(), hdr_h));
    ui.painter().rect_filled(top_hdr, 4.0, bg_raised);
    ui.painter().text(
        egui::pos2(top_hdr.left() + 10.0, top_hdr.center().y),
        egui::Align2::LEFT_CENTER,
        "Properties   |   Filters",
        neoflash_ui::typography::font(TextStyle::SmallStrong),
        text_pri,
    );

    let ctrl_h = state
        .systems
        .density
        .active()
        .control_height
        .clamp(20.0, 32.0);
    let gap = state
        .systems
        .two_column_form
        .gap
        .resolve(&state.systems)
        .clamp(2.0, 16.0);
    let pad = 8.0_f32;
    let mut y = top_hdr.bottom() + 6.0;

    // Instance name field mock
    let inst_r = Rect::from_min_size(
        egui::pos2(top_pnl.left() + pad, y),
        egui::vec2((top_pnl.width() - pad * 2.0).max(40.0), ctrl_h),
    );
    if inst_r.bottom() < top_pnl.bottom() {
        ui.painter().rect_filled(inst_r, 4.0, bg_field);
        ui.painter().text(
            egui::pos2(inst_r.left() + 8.0, inst_r.center().y),
            egui::Align2::LEFT_CENTER,
            "Name   hero_mc",
            neoflash_ui::typography::font(TextStyle::MonoSmall),
            text_pri,
        );
        y += ctrl_h + 6.0;
    }

    // 2-Column X/Y and W/H grid mock
    let col_w = ((top_pnl.width() - pad * 2.0 - gap) * 0.5).max(32.0);
    for pair in [
        ("X  140.0 px", "Y   96.0 px"),
        ("W  240.0 px", "H  180.0 px"),
    ] {
        let r1 = Rect::from_min_size(
            egui::pos2(top_pnl.left() + pad, y),
            egui::vec2(col_w, ctrl_h),
        );
        let r2 = Rect::from_min_size(egui::pos2(r1.right() + gap, y), egui::vec2(col_w, ctrl_h));
        if r1.bottom() < top_pnl.bottom() - 4.0 {
            ui.painter().rect_filled(r1, 4.0, bg_field);
            ui.painter().rect_filled(r2, 4.0, bg_field);
            ui.painter().text(
                egui::pos2(r1.left() + 6.0, r1.center().y),
                egui::Align2::LEFT_CENTER,
                pair.0,
                neoflash_ui::typography::font(TextStyle::MonoSmall),
                text_pri,
            );
            ui.painter().text(
                egui::pos2(r2.left() + 6.0, r2.center().y),
                egui::Align2::LEFT_CENTER,
                pair.1,
                neoflash_ui::typography::font(TextStyle::MonoSmall),
                text_pri,
            );
            y += ctrl_h + gap;
        }
    }

    // Bottom Panel: Library / Color & Swatches
    ui.painter()
        .hline(rect.x_range(), split_y, Stroke::new(1.0_f32, border_sub));
    let bot_hdr = Rect::from_min_size(bot_pnl.min, egui::vec2(bot_pnl.width(), hdr_h));
    ui.painter().rect_filled(bot_hdr, 0.0, bg_raised);
    ui.painter().text(
        egui::pos2(bot_hdr.left() + 10.0, bot_hdr.center().y),
        egui::Align2::LEFT_CENTER,
        "Library   |   Color & Swatches",
        neoflash_ui::typography::font(TextStyle::SmallStrong),
        text_pri,
    );

    let mut ly = bot_hdr.bottom() + 4.0;
    let row_h = state.systems.density.active().row_height.clamp(18.0, 28.0);
    for (idx, (sym_name, sym_kind)) in [
        ("Hero_MC", "Movie Clip"),
        ("Spotlight_Mask", "Graphic"),
        ("CTA_Button", "Button"),
        ("Soundtrack.mp3", "Audio"),
    ]
    .into_iter()
    .enumerate()
    {
        let r = Rect::from_min_size(
            egui::pos2(bot_pnl.left() + 4.0, ly),
            egui::vec2(bot_pnl.width() - 8.0, row_h),
        );
        if r.bottom() <= bot_pnl.bottom() - 2.0 {
            if idx == 0 {
                ui.painter()
                    .rect_filled(r, 3.0, accent.gamma_multiply(0.22));
            }
            ui.painter().text(
                egui::pos2(r.left() + 8.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                sym_name,
                neoflash_ui::typography::font(TextStyle::Small),
                text_pri,
            );
            ui.painter().text(
                egui::pos2(r.right() - 8.0, r.center().y),
                egui::Align2::RIGHT_CENTER,
                sym_kind,
                neoflash_ui::typography::font(TextStyle::Caption),
                text_sec,
            );
            ly += row_h + 1.0;
        }
    }
}

fn paint_timeline_studio_canvas(
    ui: &mut Ui,
    state: &mut UiLayoutStudioState,
    tl_rect: Rect,
    computed: &mut ComputedStudioLayout,
    theme: ThemeMode,
) {
    let palette = theme.nf2().palette();
    computed.root_rect = tl_rect;
    computed.order.clear();
    register_hit_box(computed, &state.systems, 1, tl_rect);

    let bg_panel = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BgPanel);
    let bg_raised = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BgRaised);
    let border_def = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BorderDefault);
    let border_sub = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BorderSubtle);
    let text_pri = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::TextPrimary);
    let text_sec = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::TextSecondary);

    ui.painter().rect_filled(tl_rect, 6.0, bg_panel);
    ui.painter().rect_stroke(
        tl_rect,
        6.0,
        Stroke::new(1.0_f32, border_def),
        StrokeKind::Inside,
    );

    // Top Panel Header + Toolbar (Node 2) — real NF2 FloatingPanelHeader + Transport & Layer Action Icons
    let hdr_h = state.systems.panel_header.height.clamp(24.0, 44.0);
    let bar_h = 34.0_f32;
    let top_bar_rect = Rect::from_min_size(tl_rect.min, egui::vec2(tl_rect.width(), hdr_h + bar_h));
    register_hit_box(computed, &state.systems, 2, top_bar_rect);

    let hdr_only_rect = Rect::from_min_size(tl_rect.min, egui::vec2(tl_rect.width(), hdr_h));
    {
        let mut hdr_ui = ui.new_child(egui::UiBuilder::new().max_rect(hdr_only_rect));
        let _ = c::FloatingPanelHeader {
            tabs: &["Timeline", "Motion Editor"],
            active: 0,
            collapsed: false,
        }
        .show(&mut hdr_ui);
    }

    let action_bar_rect = Rect::from_min_size(
        egui::pos2(tl_rect.left(), tl_rect.top() + hdr_h),
        egui::vec2(tl_rect.width(), bar_h),
    );
    ui.painter().rect_filled(action_bar_rect, 0.0, bg_raised);
    ui.painter().hline(
        action_bar_rect.x_range(),
        action_bar_rect.bottom() - 0.5,
        Stroke::new(1.0_f32, border_sub),
    );
    let mut act_x = action_bar_rect.left() + 8.0;
    let act_cy = action_bar_rect.center().y;
    for &(ico, active) in &[
        (i16::LAYERPLUS, false),
        (i16::FOLDERPLUS, false),
        (i16::TRASH, false),
        (i16::CHEVLEFT, false),
        (i16::PLAY, true),
        (i16::CHEVRIGHT, false),
        (i16::LOOP, false),
        (i16::ONION, true),
        (i16::FRAME, false),
        (i16::LAYERS, false),
        (i16::PLUS, false),
    ] {
        let b_rect =
            Rect::from_center_size(egui::pos2(act_x + 12.0, act_cy), egui::vec2(24.0, 24.0));
        if b_rect.right() < action_bar_rect.right() - 260.0 {
            if active {
                ui.painter()
                    .rect_filled(b_rect, 4.0, palette.accent_default.gamma_multiply(0.24));
            }
            icons::paint(
                ui.painter(),
                ui.ctx(),
                Rect::from_center_size(b_rect.center(), egui::vec2(16.0, 16.0)),
                ico,
                if active { text_pri } else { text_sec },
            );
        }
        act_x += 26.0;
    }
    ui.painter().text(
        egui::pos2(action_bar_rect.right() - 12.0, act_cy),
        egui::Align2::RIGHT_CENTER,
        format!(
            "Frame: {}   |   24.0 fps   |   Cell: {:.1}×{:.1}px   |   Arrow Head: {:.1}px",
            state.systems.timeline_geometry.playhead_frame,
            state.systems.timeline_geometry.frame_cell_width_px,
            state.systems.timeline_geometry.layer_row_height_px,
            state.systems.timeline_geometry.tween_arrow_head_length_px
        ),
        neoflash_ui::typography::font(TextStyle::MonoSmall),
        text_sec,
    );

    // Interactive Timeline Surface in the middle
    let magnifier_h = 112.0_f32;
    let surface_top = top_bar_rect.bottom();
    let surface_h = (tl_rect.height() - hdr_h - bar_h - magnifier_h - 8.0).max(190.0);
    let surface_rect = Rect::from_min_size(
        egui::pos2(tl_rect.left() + 4.0, surface_top),
        egui::vec2(tl_rect.width() - 8.0, surface_h),
    );

    let hit_ids = TimelineHitIds {
        layers_col: 3,
        ruler_playhead: 4,
        classic_tween: 5,
        shape_tween: 6,
        motion_tween: 7,
        broken_tween: 8,
        keyframes_badges: 9,
        audio_waveform: 10,
    };
    paint_interactive_timeline_surface(ui, state, surface_rect, computed, hit_ids, theme);

    // Bottom 2.5x Zoomed Tween Arrow & Keyframe Geometry Callout Strip
    let mag_rect = Rect::from_min_max(
        egui::pos2(tl_rect.left() + 8.0, surface_rect.bottom() + 6.0),
        egui::pos2(tl_rect.right() - 8.0, tl_rect.bottom() - 6.0),
    );
    if mag_rect.height() >= 64.0 {
        ui.painter().rect_filled(mag_rect, 4.0, bg_raised);
        ui.painter().rect_stroke(
            mag_rect,
            4.0,
            Stroke::new(1.0_f32, border_sub),
            StrokeKind::Inside,
        );
        let tg = &state.systems.timeline_geometry;
        ui.painter().text(
            egui::pos2(mag_rect.left() + 10.0, mag_rect.top() + 12.0),
            egui::Align2::LEFT_CENTER,
            format!(
                "ZOOMED TWEEN ARROW & KEYFRAME GEOMETRY CALLOUT (2.5×)   ·   Head Length: {:.1}px   ·   Wing: {:.1}px   ·   Start Inset: {:.1}px   ·   End Inset: {:.1}px   ·   Stroke: {:.2}px   ·   Y-Pos: {:.0}%",
                tg.tween_arrow_head_length_px,
                tg.tween_arrow_head_wing_px,
                tg.tween_arrow_start_inset_px,
                tg.tween_arrow_end_inset_px,
                tg.tween_arrow_stroke_width_px,
                tg.tween_arrow_y_pct
            ),
            neoflash_ui::typography::font(TextStyle::MonoSmall),
            text_sec,
        );

        let sample_span = Rect::from_min_max(
            egui::pos2(mag_rect.left() + 24.0, mag_rect.top() + 28.0),
            egui::pos2(mag_rect.right() - 24.0, mag_rect.bottom() - 10.0),
        );
        let classic_fill = palette.timeline_classic_tween;
        let marker = palette.timeline_marker;
        ui.painter().rect_filled(sample_span, 4.0, classic_fill);
        ui.painter().rect_stroke(
            sample_span,
            4.0,
            Stroke::new(1.0_f32, marker.gamma_multiply(0.5)),
            StrokeKind::Inside,
        );

        let zoom = 2.4_f32;
        let y_line = sample_span.top()
            + sample_span.height() * (tg.tween_arrow_y_pct.clamp(10.0, 90.0) / 100.0);
        let kf_y = sample_span.top()
            + sample_span.height() * (tg.keyframe_dot_y_pct.clamp(15.0, 85.0) / 100.0);
        let dot_x = sample_span.left() + (tg.frame_cell_width_px * 0.5 * zoom).clamp(10.0, 44.0);
        ui.painter().circle_filled(
            egui::pos2(dot_x, kf_y),
            (tg.keyframe_dot_radius_px * zoom).clamp(3.0, 18.0),
            marker,
        );

        let shaft_start_x = (sample_span.left() + tg.tween_arrow_start_inset_px * zoom)
            .min(sample_span.right() - 16.0);
        let tip_x =
            (sample_span.right() - tg.tween_arrow_end_inset_px * zoom).max(shaft_start_x + 12.0);
        let head_len = (tg.tween_arrow_head_length_px * zoom).clamp(4.0, tip_x - shaft_start_x);
        let wing = (tg.tween_arrow_head_wing_px * zoom).clamp(2.0, 32.0);
        let base_x = tip_x - head_len;

        ui.painter().line_segment(
            [
                egui::pos2(shaft_start_x, y_line),
                egui::pos2(base_x, y_line),
            ],
            Stroke::new(
                (tg.tween_arrow_stroke_width_px * zoom).clamp(1.0, 10.0),
                marker,
            ),
        );
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(tip_x, y_line),
                egui::pos2(base_x, y_line - wing),
                egui::pos2(base_x, y_line + wing),
            ],
            marker,
            Stroke::NONE,
        ));
    }
}

fn paint_interactive_timeline_surface(
    ui: &mut Ui,
    state: &mut UiLayoutStudioState,
    rect: Rect,
    computed: &mut ComputedStudioLayout,
    hit_ids: TimelineHitIds,
    theme: ThemeMode,
) {
    let palette = theme.nf2().palette();
    let bg_panel = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BgPanel);
    let border_sub = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BorderSubtle);

    ui.painter().rect_filled(rect, 4.0, bg_panel);
    ui.painter().rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, border_sub),
        StrokeKind::Inside,
    );

    let tg = state.systems.timeline_geometry.clone();
    let layer_col_w = tg.resolve_layer_list_width(rect.width());
    let ruler_h = tg.ruler_height_px.clamp(14.0, 40.0);
    let row_h = tg.layer_row_height_px.clamp(14.0, 48.0);
    let cell_w = tg.frame_cell_width_px.clamp(6.0, 32.0);

    let layers_col_rect = Rect::from_min_size(rect.min, egui::vec2(layer_col_w, rect.height()));
    let grid_area_rect =
        Rect::from_min_max(egui::pos2(layers_col_rect.right(), rect.top()), rect.max);
    let ruler_rect = Rect::from_min_size(
        grid_area_rect.min,
        egui::vec2(grid_area_rect.width(), ruler_h),
    );

    register_hit_box(
        computed,
        &state.systems,
        hit_ids.layers_col,
        layers_col_rect,
    );
    register_hit_box(computed, &state.systems, hit_ids.ruler_playhead, ruler_rect);

    // 1. Left Layer Tree Column Header + 6 Realistic Layers using real `tl::layers_header_with` & `tl::LayerRow`
    let layer_hdr_rect = Rect::from_min_size(rect.min, egui::vec2(layer_col_w, ruler_h));
    {
        let mut hdr_ui = ui.new_child(egui::UiBuilder::new().max_rect(layer_hdr_rect));
        let _ = tl::layers_header_with(
            &mut hdr_ui,
            layer_col_w,
            LayersHeaderColumns {
                mode: layer_col_w >= 210.0,
                solo: true,
            },
        );
    }

    let layers = [
        (
            "Character",
            LayerKind::FolderOpen,
            0,
            false,
            false,
            false,
            ModeCell::Empty,
            Color32::from_rgb(255, 146, 56),
        ),
        (
            "Spotlight",
            LayerKind::Mask,
            1,
            false,
            true,
            false,
            ModeCell::Mode("Normal"),
            Color32::from_rgb(176, 118, 255),
        ),
        (
            "Hero_MC",
            LayerKind::Masked,
            2,
            true,
            false,
            false,
            ModeCell::Mode("Multiply"),
            Color32::from_rgb(82, 162, 255),
        ),
        (
            "Motion Path",
            LayerKind::MotionGuide,
            1,
            false,
            false,
            true,
            ModeCell::Empty,
            Color32::from_rgb(0, 212, 224),
        ),
        (
            "Background",
            LayerKind::Layer,
            0,
            false,
            false,
            false,
            ModeCell::Mode("Normal"),
            Color32::from_rgb(72, 214, 134),
        ),
        (
            "Soundtrack",
            LayerKind::Sound,
            0,
            false,
            false,
            false,
            ModeCell::Empty,
            Color32::from_rgb(255, 158, 68),
        ),
    ];

    for (idx, &(name, kind, depth, selected, locked, outline, mode_cell, swatch_col)) in
        layers.iter().enumerate()
    {
        let ry = rect.top() + ruler_h + idx as f32 * row_h;
        if ry + 4.0 > rect.bottom() {
            break;
        }
        let r_rect = Rect::from_min_size(
            egui::pos2(rect.left(), ry),
            egui::vec2(layer_col_w, row_h.min(rect.bottom() - ry)),
        );
        let row_resp = ui.interact(
            r_rect,
            ui.id()
                .with(("studio-tl-layer-row", hit_ids.layers_col, idx)),
            Sense::hover(),
        );
        let _ = LayerRow {
            label: name,
            kind,
            selected,
            indent: depth,
            editing: selected,
            visible: true,
            locked,
            outline,
            color: swatch_col,
        }
        .paint_at_with(
            ui,
            r_rect,
            row_resp,
            LayerColumns {
                mode: if layer_col_w >= 210.0 {
                    mode_cell
                } else {
                    ModeCell::Hidden
                },
                solo: Some(selected),
            },
        );
    }

    ui.painter().vline(
        layers_col_rect.right(),
        rect.y_range(),
        Stroke::new(1.0_f32, border_sub),
    );

    // 2. Frame Ruler (1, 5, 10, 15, 20, 25...) + Onion Skin Range Brackets + Interactive Playhead
    let tl_geo = tl::Geometry {
        origin_x: grid_area_rect.left(),
        frame_width: cell_w,
        row_height: row_h,
    };
    tl::paint_ruler(
        &ui.painter().with_clip_rect(ruler_rect),
        ui.ctx(),
        ruler_rect,
        &tl_geo,
        24.0,
        ruler_h >= 26.0,
    );

    let max_frames = ((ruler_rect.width() / cell_w).floor() as usize).clamp(16, 80);

    // Interactive Playhead scrubbing when clicking/dragging on the ruler!
    if let Some(pos) = ui.input(|i| i.pointer.interact_pos())
        && ruler_rect.contains(pos)
        && ui.input(|i| i.pointer.primary_down())
    {
        let rel_x = (pos.x - ruler_rect.left()).max(0.0);
        let f = ((rel_x / cell_w).floor() as usize + 1).clamp(1, max_frames);
        state.systems.timeline_geometry.playhead_frame = f;
    }

    // Onion skin range overlay in ruler
    let onion_l = ruler_rect.left() + (tg.onion_start_frame.saturating_sub(1)) as f32 * cell_w;
    let onion_r = ruler_rect.left() + tg.onion_end_frame as f32 * cell_w;
    if onion_r > onion_l && onion_l < ruler_rect.right() {
        let o_rect = Rect::from_min_max(
            egui::pos2(onion_l.max(ruler_rect.left()), ruler_rect.top() + 2.0),
            egui::pos2(onion_r.min(ruler_rect.right()), ruler_rect.bottom() - 2.0),
        );
        let onion_col = palette.accent_subtle;
        let onion_stroke = palette.timeline_onion;
        ui.painter().rect_filled(o_rect, 2.0, onion_col);
        let bw = tg.onion_bracket_width_px.clamp(2.0, 14.0);
        let hr = tg.onion_handle_radius_px.clamp(1.5, 8.0);
        // Left bracket `[`
        ui.painter().vline(
            o_rect.left(),
            o_rect.y_range(),
            Stroke::new(1.5_f32, onion_stroke),
        );
        ui.painter().hline(
            o_rect.left()..=(o_rect.left() + bw),
            o_rect.top(),
            Stroke::new(1.5_f32, onion_stroke),
        );
        ui.painter().hline(
            o_rect.left()..=(o_rect.left() + bw),
            o_rect.bottom(),
            Stroke::new(1.5_f32, onion_stroke),
        );
        ui.painter().circle_filled(
            egui::pos2(o_rect.left(), o_rect.center().y),
            hr,
            onion_stroke,
        );
        // Right bracket `]`
        ui.painter().vline(
            o_rect.right(),
            o_rect.y_range(),
            Stroke::new(1.5_f32, onion_stroke),
        );
        ui.painter().hline(
            (o_rect.right() - bw)..=o_rect.right(),
            o_rect.top(),
            Stroke::new(1.5_f32, onion_stroke),
        );
        ui.painter().hline(
            (o_rect.right() - bw)..=o_rect.right(),
            o_rect.bottom(),
            Stroke::new(1.5_f32, onion_stroke),
        );
        ui.painter().circle_filled(
            egui::pos2(o_rect.right(), o_rect.center().y),
            hr,
            onion_stroke,
        );
    }

    // 3. Multi-Row Frame Grid (`tl::paint_grid`) + Tween Spans + Arrows + Keyframe Dots + Diamonds + Audio Waveform
    for r in 0..6_usize {
        let ry = rect.top() + ruler_h + r as f32 * row_h;
        if ry + 4.0 > rect.bottom() {
            break;
        }
        let row_rect = Rect::from_min_size(
            egui::pos2(grid_area_rect.left(), ry),
            egui::vec2(grid_area_rect.width(), row_h.min(rect.bottom() - ry)),
        );
        tl::paint_grid(
            &ui.painter().with_clip_rect(row_rect),
            ui.ctx(),
            row_rect,
            &tl_geo,
            if r == 0 {
                GridKind::Folder
            } else {
                GridKind::Layer
            },
        );
    }

    let span_rect_for = |row_idx: usize, start_f: usize, end_f: usize| -> Rect {
        let ry = (rect.top() + ruler_h + row_idx as f32 * row_h).min(rect.bottom() - 2.0);
        let rh = row_h.min((rect.bottom() - ry).max(2.0));
        let sx =
            (grid_area_rect.left() + (start_f - 1) as f32 * cell_w).min(grid_area_rect.right());
        let ex = (grid_area_rect.left() + end_f as f32 * cell_w).min(grid_area_rect.right());
        Rect::from_min_max(
            egui::pos2(sx, ry + 1.0),
            egui::pos2(ex.max(sx + 4.0), ry + rh - 1.0),
        )
    };

    let paint_keyframe_dot = |painter: &egui::Painter, cell_rect: Rect, hollow: bool| {
        let cx = cell_rect.left() + cell_w * 0.5;
        let cy = cell_rect.top()
            + cell_rect.height() * (tg.keyframe_dot_y_pct.clamp(15.0, 85.0) / 100.0);
        let rad = tg.keyframe_dot_radius_px.clamp(1.5, 8.0);
        if hollow {
            painter.circle_stroke(
                egui::pos2(cx, cy),
                rad,
                Stroke::new(1.2_f32, palette.timeline_marker),
            );
        } else {
            painter.circle_filled(egui::pos2(cx, cy), rad, palette.timeline_marker);
        }
    };

    let paint_span_end_marker = |painter: &egui::Painter, span_r: Rect| {
        let ex = span_r.right() - cell_w * 0.5;
        let ey = span_r.center().y;
        let m_rect = Rect::from_center_size(
            egui::pos2(ex, ey),
            egui::vec2(
                tg.span_end_rect_w_px.clamp(2.0, 12.0),
                tg.span_end_rect_h_px.clamp(3.0, 16.0),
            ),
        );
        painter.rect_filled(m_rect, 1.0, palette.timeline_empty);
        painter.rect_stroke(
            m_rect,
            1.0,
            Stroke::new(1.0_f32, palette.timeline_marker),
            StrokeKind::Inside,
        );
    };

    // Row 0 (`Folder: Character`) & Row 3 (`Guide: Motion Path`): Ordinary Keyframe Span + Label Flag + Action `a` + `[ ]`
    let kf_span = span_rect_for(3, 1, 22);
    register_hit_box(computed, &state.systems, hit_ids.keyframes_badges, kf_span);
    ui.painter()
        .rect_filled(kf_span, 2.0, palette.timeline_static);
    paint_keyframe_dot(ui.painter(), kf_span, false);
    paint_span_end_marker(ui.painter(), kf_span);

    // Red Frame Label Flag + `"intro"` text & Action `a` marker on Row 0 / Row 3
    let flag_sz = tg.label_flag_size_px.clamp(3.0, 12.0);
    let flag_x = kf_span.left() + 2.0;
    let flag_y = kf_span.top() + 2.0;
    let red_flag = palette.timeline_label;
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(flag_x, flag_y),
            egui::pos2(flag_x + flag_sz, flag_y + flag_sz * 0.5),
            egui::pos2(flag_x, flag_y + flag_sz),
        ],
        red_flag,
        Stroke::NONE,
    ));
    ui.painter().text(
        egui::pos2(flag_x + flag_sz + 3.0, flag_y + flag_sz * 0.5),
        egui::Align2::LEFT_CENTER,
        "motion_guide",
        egui::FontId::monospace(8.5),
        red_flag,
    );
    // Action marker `a` + Blank keyframe `○` at frame 23
    ui.painter().text(
        egui::pos2(kf_span.right() - cell_w * 1.5, kf_span.top() + 6.0),
        egui::Align2::CENTER_CENTER,
        "a",
        egui::FontId::monospace(tg.action_marker_size_px.clamp(5.0, 14.0)),
        palette.timeline_marker,
    );
    let blank_kf_rect = span_rect_for(3, 23, 25);
    paint_keyframe_dot(ui.painter(), blank_kf_rect, true);

    // Row 1 (`Mask: Spotlight`) & Row 2 (`Masked: Hero_MC`): Classic Tween Span (Purple + Parametric Arrow `──────►`)
    let classic_span = span_rect_for(1, 1, 20);
    register_hit_box(
        computed,
        &state.systems,
        hit_ids.classic_tween,
        classic_span,
    );
    let purple_fill = palette.timeline_classic_tween;
    let purple_arrow = palette.timeline_marker;
    ui.painter().rect_filled(classic_span, 2.0, purple_fill);
    paint_keyframe_dot(ui.painter(), classic_span, false);
    let (c_start, _c_tip, c_tri) = tg.compute_tween_arrow_points(classic_span);
    ui.painter().line_segment(
        [c_start, egui::pos2(c_tri[1].x, c_start.y)],
        Stroke::new(tg.tween_arrow_stroke_width_px.clamp(0.5, 5.0), purple_arrow),
    );
    ui.painter().add(egui::Shape::convex_polygon(
        c_tri.to_vec(),
        purple_arrow,
        Stroke::NONE,
    ));
    // End keyframe dot of classic tween
    let classic_end_cell = span_rect_for(1, 20, 20);
    paint_keyframe_dot(ui.painter(), classic_end_cell, false);

    // Row 2 (`Masked: Hero_MC`): Motion Tween Span (Blue + Property Diamonds `◆`) + Broken Tween Span (`- - - -`)
    let motion_span = span_rect_for(2, 1, 18);
    register_hit_box(computed, &state.systems, hit_ids.motion_tween, motion_span);
    let blue_fill = palette.timeline_motion_tween;
    let blue_diamond = palette.timeline_marker;
    ui.painter().rect_filled(motion_span, 2.0, blue_fill);
    paint_keyframe_dot(ui.painter(), motion_span, false);
    let dr = tg.motion_diamond_radius_px.clamp(1.5, 9.0);
    let dy = motion_span.top()
        + motion_span.height() * (tg.keyframe_dot_y_pct.clamp(15.0, 85.0) / 100.0);
    for df in [5_usize, 10, 14, 18] {
        let dx = grid_area_rect.left() + (df as f32 - 0.5) * cell_w;
        if dx < motion_span.right() {
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(dx, dy - dr),
                    egui::pos2(dx + dr, dy),
                    egui::pos2(dx, dy + dr),
                    egui::pos2(dx - dr, dy),
                ],
                blue_diamond,
                Stroke::NONE,
            ));
        }
    }

    // Broken Tween Span on Row 2 (frames 19..30)
    let broken_span = span_rect_for(2, 19, 30);
    register_hit_box(computed, &state.systems, hit_ids.broken_tween, broken_span);
    ui.painter()
        .rect_filled(broken_span, 2.0, palette.timeline_classic_tween);
    paint_keyframe_dot(ui.painter(), broken_span, false);
    let dash_w = tg.broken_tween_dash_px.clamp(1.0, 16.0);
    let gap_w = tg.broken_tween_gap_px.clamp(1.0, 16.0);
    let by =
        broken_span.top() + broken_span.height() * (tg.tween_arrow_y_pct.clamp(10.0, 90.0) / 100.0);
    let mut bx = broken_span.left() + tg.tween_arrow_start_inset_px.max(4.0);
    while bx + dash_w < broken_span.right() - 4.0 {
        ui.painter().line_segment(
            [egui::pos2(bx, by), egui::pos2(bx + dash_w, by)],
            Stroke::new(tg.tween_arrow_stroke_width_px.clamp(0.5, 5.0), purple_arrow),
        );
        bx += dash_w + gap_w;
    }

    // Row 4 (`Normal: Background`): Shape Tween Span (Green + Parametric Arrow `──────►`)
    let shape_span = span_rect_for(4, 1, 24);
    register_hit_box(computed, &state.systems, hit_ids.shape_tween, shape_span);
    let green_fill = palette.timeline_shape_tween;
    let green_arrow = palette.timeline_marker;
    ui.painter().rect_filled(shape_span, 2.0, green_fill);
    paint_keyframe_dot(ui.painter(), shape_span, false);
    let (s_start, _s_tip, s_tri) = tg.compute_tween_arrow_points(shape_span);
    ui.painter().line_segment(
        [s_start, egui::pos2(s_tri[1].x, s_start.y)],
        Stroke::new(tg.tween_arrow_stroke_width_px.clamp(0.5, 5.0), green_arrow),
    );
    ui.painter().add(egui::Shape::convex_polygon(
        s_tri.to_vec(),
        green_arrow,
        Stroke::NONE,
    ));
    let shape_end_cell = span_rect_for(4, 24, 24);
    paint_keyframe_dot(ui.painter(), shape_end_cell, false);

    // Row 5 (`Audio: Soundtrack`): Sound Waveform Span
    let audio_span = span_rect_for(5, 1, 30);
    register_hit_box(computed, &state.systems, hit_ids.audio_waveform, audio_span);
    ui.painter()
        .rect_filled(audio_span, 2.0, palette.timeline_static);
    paint_keyframe_dot(ui.painter(), audio_span, false);
    let wave_col = palette.success_default;
    let mid_y = audio_span.center().y;
    let max_amp = (audio_span.height() * 0.40).max(2.0);
    let mut wx = audio_span.left() + 8.0;
    let mut step = 0_usize;
    while wx < audio_span.right() - 6.0 {
        let amp = (((step as f32 * 0.65).sin().abs() * 0.7
            + ((step as f32 * 1.3).cos().abs() * 0.3))
            * max_amp)
            .max(1.5);
        ui.painter().vline(
            wx,
            (mid_y - amp)..=(mid_y + amp),
            Stroke::new(1.15_f32, wave_col),
        );
        wx += 3.0;
        step += 1;
    }

    // 4. Red Playhead Cap + Vertical Line across all layers
    let ph_frame = tg.playhead_frame.clamp(1, max_frames);
    let ph_x = ruler_rect.left() + (ph_frame as f32 - 0.5) * cell_w;
    if ph_x >= ruler_rect.left() && ph_x <= ruler_rect.right() {
        let playhead_red = palette.timeline_playhead;
        let cap_w = tg.playhead_cap_width_px.clamp(6.0, 24.0);
        let cap_h = tg.playhead_cap_height_px.clamp(6.0, ruler_h);
        let cap_rect = Rect::from_center_size(
            egui::pos2(ph_x, ruler_rect.bottom() - cap_h * 0.5),
            egui::vec2(cap_w, cap_h),
        );
        ui.painter().rect_filled(cap_rect, 2.0, playhead_red);
        ui.painter().vline(
            ph_x,
            cap_rect.bottom()..=rect.bottom(),
            Stroke::new(tg.playhead_line_width_px.clamp(0.5, 4.0), playhead_red),
        );
    }
}

fn paint_node_wysiwyg(
    ui: &mut Ui,
    node: &mut UiStudioNode,
    systems: &UiStudioSystems,
    computed: &ComputedStudioLayout,
    theme: ThemeMode,
) {
    if !node.visible {
        return;
    }
    let Some(box_info) = computed.node_box(node.id) else {
        return;
    };
    let rect = box_info.rect;
    let nf = theme.nf2().palette();

    // Optional custom background or panel surface
    let bg_fill = node
        .style
        .bg_token
        .map(|tok| systems.palette_overrides.resolve_color(theme, tok))
        .or_else(|| match node.kind {
            UiNodeKind::Panel { .. } => Some(
                systems
                    .palette_overrides
                    .resolve_color(theme, StudioColorRole::BgPanel),
            ),
            _ => None,
        });

    let rad = match node.style.corner_radius {
        RadiusToken::None if matches!(node.kind, UiNodeKind::Panel { .. }) => radius::LG,
        other => other.resolve_px(),
    };

    if let Some(fill) = bg_fill {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(rad as u8), fill);
    }
    if let Some(border_tok) = node.style.border_token {
        let bcol = systems.palette_overrides.resolve_color(theme, border_tok);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(rad as u8),
            Stroke::new(node.style.border_width.max(1.0), bcol),
            StrokeKind::Inside,
        );
    } else if matches!(node.kind, UiNodeKind::Panel { .. }) {
        let bcol = systems
            .palette_overrides
            .resolve_color(theme, StudioColorRole::BorderDefault);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(rad as u8),
            Stroke::new(1.0_f32, bcol),
            StrokeKind::Inside,
        );
    }

    let text_col = systems
        .palette_overrides
        .resolve_color(theme, box_info.typography.color_token);

    match &mut node.kind {
        UiNodeKind::Panel { tabs, active_tab } => {
            if !tabs.is_empty() {
                let hdr_h = systems.panel_header.height.max(24.0);
                let hdr_rect = Rect::from_min_size(rect.min, egui::vec2(rect.width(), hdr_h));
                let tab_refs: Vec<&str> = tabs.iter().map(String::as_str).collect();
                let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(hdr_rect));
                let resp = c::FloatingPanelHeader {
                    tabs: &tab_refs,
                    active: (*active_tab).min(tab_refs.len().saturating_sub(1)),
                    collapsed: false,
                }
                .show(&mut child_ui);
                if let Some(clicked_tab) = resp.tab {
                    *active_tab = clicked_tab;
                }
            }
            for child in &mut node.children {
                paint_node_wysiwyg(ui, child, systems, computed, theme);
            }
        }
        UiNodeKind::Section { expanded, .. } => {
            let hdr_h = systems.section_header.height.max(20.0);
            let hdr_rect = Rect::from_min_size(rect.min, egui::vec2(rect.width(), hdr_h));
            let chev = if *expanded {
                i16::CHEVDOWN
            } else {
                i16::CHEVRIGHT
            };
            icons::paint(
                ui.painter(),
                ui.ctx(),
                Rect::from_center_size(
                    egui::pos2(hdr_rect.left() + 8.0, hdr_rect.center().y),
                    egui::vec2(16.0, 16.0),
                ),
                chev,
                nf.text_secondary,
            );
            let display_lbl = if box_info.typography.uppercase {
                node.label.to_uppercase()
            } else {
                node.label.clone()
            };
            ui.painter().text(
                egui::pos2(hdr_rect.left() + 22.0, hdr_rect.center().y),
                egui::Align2::LEFT_CENTER,
                display_lbl,
                egui::FontId::proportional(box_info.typography.size_pt),
                text_col,
            );
            if *expanded {
                for child in &mut node.children {
                    paint_node_wysiwyg(ui, child, systems, computed, theme);
                }
            }
        }
        UiNodeKind::Row | UiNodeKind::Column | UiNodeKind::Grid { .. } => {
            for child in &mut node.children {
                paint_node_wysiwyg(ui, child, systems, computed, theme);
            }
        }
        UiNodeKind::Field {
            value,
            prefix,
            unit,
            decimals,
            stepper,
        } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            let mut fld = c::Field::new(value, &node.label)
                .prefix(prefix)
                .unit(unit)
                .decimals(*decimals)
                .width(rect.width());
            if *stepper {
                fld = fld.stepper();
            }
            child_ui.add(fld);
        }
        UiNodeKind::TextField {
            text, prefix, hint, ..
        } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            child_ui.add(
                c::TextField::new(text, if hint.is_empty() { &node.label } else { hint })
                    .prefix(prefix)
                    .width(rect.width()),
            );
        }
        UiNodeKind::Select { selected, options } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            let current = options
                .get(*selected)
                .map_or(node.label.as_str(), String::as_str);
            c::Select::new(("studio-canvas-sel", node.id), current)
                .width(rect.width())
                .show(&mut child_ui, |ui| {
                    for (idx, opt) in options.iter().enumerate() {
                        if ui
                            .add(c::MenuItem::new(opt).checked(idx == *selected))
                            .clicked()
                        {
                            *selected = idx;
                        }
                    }
                });
        }
        UiNodeKind::Button {
            button_kind,
            icon_name,
        } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            let mut btn = c::Button::new(&node.label)
                .kind(button_kind.to_nf2())
                .width(rect.width());
            if let Some(ico) = icon_name {
                btn = btn.icon(resolve_icon_by_name(ico));
            }
            let _ = child_ui.add(btn);
        }
        UiNodeKind::IconButton {
            icon_name,
            selected,
            ..
        } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            let ico = resolve_icon_by_name(icon_name);
            let side = rect.width().min(rect.height()).max(20.0);
            if child_ui
                .add(
                    c::IconButton::new(ico, &node.label)
                        .selected(*selected)
                        .side(side),
                )
                .clicked()
            {
                *selected = !*selected;
            }
        }
        UiNodeKind::Checkbox { checked } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            child_ui.add(c::Checkbox::new(checked, &node.label));
        }
        UiNodeKind::Switch { on } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            child_ui.add(c::Switch::new(on, &node.label));
        }
        UiNodeKind::ColorSwatch { rgba, is_stroke } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            let col = Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            child_ui.add(c::ColorWell {
                target: if *is_stroke {
                    c::ColorWellTarget::Stroke
                } else {
                    c::ColorWellTarget::Fill
                },
                color: Some(col),
                active: false,
            });
        }
        UiNodeKind::Slider {
            value,
            min,
            max,
            show_field,
        } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            let val_mode = if *show_field && rect.width() > 140.0 {
                c::SliderValue::Field
            } else {
                c::SliderValue::Inline
            };
            let track_w = if val_mode == c::SliderValue::Field {
                (rect.width() - 86.0).max(48.0)
            } else {
                (rect.width() - 48.0).max(48.0)
            };
            child_ui.add(
                c::Slider::new(value, *min..=*max, &node.label)
                    .width(track_w)
                    .value(val_mode),
            );
        }
        UiNodeKind::Label => {
            let align = match box_info.typography.align {
                StudioTextAlign::Left => (rect.left_center(), egui::Align2::LEFT_CENTER),
                StudioTextAlign::Center => (rect.center(), egui::Align2::CENTER_CENTER),
                StudioTextAlign::Right => (rect.right_center(), egui::Align2::RIGHT_CENTER),
            };
            let txt = if box_info.typography.uppercase {
                node.label.to_uppercase()
            } else {
                node.label.clone()
            };
            let family = match box_info.typography.font_family {
                StudioFontFamily::Inter => egui::FontFamily::Proportional,
                StudioFontFamily::RobotoMono => egui::FontFamily::Monospace,
            };
            ui.painter().text(
                align.0,
                align.1,
                txt,
                egui::FontId::new(box_info.typography.size_pt, family),
                text_col,
            );
        }
        UiNodeKind::Separator { vertical } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            c::divider(&mut child_ui, *vertical);
        }
        UiNodeKind::TableRow {
            cells,
            column_fractions,
            is_header,
            selected,
        } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            let cell_refs: Vec<&str> = cells.iter().map(String::as_str).collect();
            let kind = if *is_header {
                c::TableRowKind::Header
            } else if *selected {
                c::TableRowKind::Selected
            } else {
                c::TableRowKind::Row
            };
            child_ui.add(c::TableRow {
                cells: &cell_refs,
                kind,
                columns: column_fractions,
            });
        }
    }
}

fn paint_canvas_overlays(
    ui: &mut Ui,
    state: &UiLayoutStudioState,
    computed: &ComputedStudioLayout,
    theme: ThemeMode,
) {
    let accent = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::AccentDefault);
    let muted_border = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BorderStrong);
    let badge_bg = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::BgRaised);
    let badge_fg = state
        .systems
        .palette_overrides
        .resolve_color(theme, StudioColorRole::TextPrimary);

    if state.show_wireframe_outlines {
        for &id in &computed.order {
            if let Some(node) = state.document.find_node(id)
                && node.kind.is_container()
                && let Some(b) = computed.node_box(id)
            {
                ui.painter().rect_stroke(
                    b.rect,
                    2.0,
                    Stroke::new(1.0_f32, muted_border.gamma_multiply(0.45)),
                    StrokeKind::Inside,
                );
            }
        }
    }

    if let Some(hov_id) = state.hovered_node
        && !state.selected_nodes.contains(&hov_id)
        && let Some(b) = computed.node_box(hov_id)
    {
        ui.painter().rect_stroke(
            b.rect,
            3.0,
            Stroke::new(1.5_f32, accent.gamma_multiply(0.65)),
            StrokeKind::Outside,
        );
    }

    for &sel_id in &state.selected_nodes {
        let Some(b) = computed.node_box(sel_id) else {
            continue;
        };
        ui.painter().rect_stroke(
            b.rect,
            3.0,
            Stroke::new(2.0_f32, accent),
            StrokeKind::Outside,
        );

        if state.show_dimension_badges
            && let Some(node) = state.document.find_node(sel_id)
        {
            let badge_txt = format!(
                "{} · {} ({:.0}×{:.0}px)",
                node.kind.type_badge(),
                node.box_constraints.width.format_compact(),
                b.resolved_width,
                b.resolved_height
            );
            let font = neoflash_ui::typography::font(TextStyle::MonoSmall);
            let galley = ui.painter().layout_no_wrap(badge_txt, font, badge_fg);
            let pill_size = galley.size() + egui::vec2(10.0, 4.0);
            let pill_min = egui::pos2(
                b.rect.left(),
                (b.rect.top() - pill_size.y - 2.0).max(computed.root_rect.top()),
            );
            let pill_rect = Rect::from_min_size(pill_min, pill_size);
            ui.painter().rect_filled(pill_rect, 3.0, badge_bg);
            ui.painter().rect_stroke(
                pill_rect,
                3.0,
                Stroke::new(1.0_f32, accent),
                StrokeKind::Inside,
            );
            ui.painter()
                .galley(pill_rect.min + egui::vec2(5.0, 2.0), galley, badge_fg);
        }
    }
}
