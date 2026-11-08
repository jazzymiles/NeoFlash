//! In-app Illustrated Help & Reference manual (`Help ▸ NeoFlash Help & Reference…`),
//! also reachable from the `Search functions… (F1)` command palette and `About NeoFlash`.
//!
//! Built strictly from `neoflash_ui` (NF2) tokens, typography, icons, canvas overlays,
//! timeline painters, and ease graphs.

use crate::theme::ScrollTheme as _;
use eframe::egui::{self, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, vec2};
use neoflash_ui::{
    Icon, TextStyle,
    components::{
        self as c, Badge, BadgeType, Button, DialogHeader, Search, Surface, TreeItem, TreeItemType,
        brushes::{BrushStroke, brush_stroke_sample},
        canvas::{
            AnchorState, GuideState, MotionHandleState, MotionKeyState, Orientation, SelectionKind,
            ShapeHintState, TransformMode, paint_anchor, paint_bezier_handle,
            paint_gradient_transform, paint_guide, paint_motion_handle, paint_motion_key,
            paint_motion_path, paint_ruler_corner, paint_selection_box, paint_shape_hint,
            paint_shape_selection, paint_snap_ring, paint_stage_grid, paint_stage_ruler,
            paint_transform_box,
        },
        divider, kbd,
        surface::dialog_footer,
        timeline::{
            Geometry, GridKind, MarkerKind, SpanType, paint_grid, paint_marker, paint_span,
        },
    },
    icons::{self, i16},
    metrics::FrameView,
    tokens::{radius, space},
    typography::{font, rich},
};
use std::{fmt::Write as _, path::PathBuf};

const DIALOG_WIDTH: f32 = 920.0;
const DIALOG_HEIGHT: f32 = 760.0;
const SIDEBAR_WIDTH: f32 = 212.0;

#[derive(Clone, Debug, Default)]
struct HelpManualState {
    open: bool,
    page: usize,
    query: String,
    diagram_tab: usize,
}

fn state_id() -> egui::Id {
    egui::Id::new("neoflash-help-manual-state")
}

pub(crate) fn open_help_manual(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
        s.open = true;
    });
}

#[cfg(test)]
pub(crate) fn close_help_manual(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
        s.open = false;
    });
}

#[cfg(test)]
pub(crate) fn is_help_manual_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| {
        d.get_temp::<HelpManualState>(state_id())
            .is_some_and(|s| s.open)
    })
}

#[cfg(test)]
pub(crate) fn set_help_manual_page(ctx: &egui::Context, page: usize) {
    ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
        s.page = page.min(HELP_CHAPTERS.len() - 1);
        s.diagram_tab = 0;
    });
}

pub(crate) struct HelpChapterMeta {
    pub title: &'static str,
    pub subtitle: &'static str,
    pub icon: Icon,
    pub keywords: &'static str,
}

pub(crate) const HELP_CHAPTERS: [HelpChapterMeta; 10] = [
    HelpChapterMeta {
        title: "1. Workspace & Stage",
        subtitle: "Stage, Pasteboard, Rulers, Guides, Grid & Snapping priority",
        icon: i16::DESKTOP,
        keywords: "workspace stage pasteboard rulers guides grid snapping zoom hand pan focus mode units",
    },
    HelpChapterMeta {
        title: "2. Drawing Modes & Objects",
        subtitle: "Merge Drawing vs Object Drawing (J), Primitives, Groups & Combine",
        icon: i16::LAYERS,
        keywords: "merge drawing object drawing primitive group break apart union intersect punch crop boolean",
    },
    HelpChapterMeta {
        title: "3. Selection & Transform",
        subtitle: "Selection (V), Subselection (A), Free Transform (Q) & Lasso (L) modes",
        icon: i16::SELECT,
        keywords: "selection subselection free transform rotate skew scale distort taper envelope lasso magic wand",
    },
    HelpChapterMeta {
        title: "4. Vector & Pen States",
        subtitle: "Pen (P) 9 pointer states, Line (N), Shapes (R/O) & Pencil (Y) modes",
        icon: i16::PEN,
        keywords: "pen anchor point bezier tangent line rectangle oval polystar pencil straighten smooth ink",
    },
    HelpChapterMeta {
        title: "5. Brush, Eraser & Deco",
        subtitle: "Brush (B) 5 paint modes, Paint Bucket (K), Gradient (F), Eraser (E) & Deco",
        icon: i16::BRUSH,
        keywords: "brush paint normal fills behind selection inside lock fill pressure tilt bucket gap ink bottle eyedropper eraser faucet spray deco",
    },
    HelpChapterMeta {
        title: "6. Symbols, 9-Slice & Library",
        subtitle: "MovieClip, Button (4 states), Graphic syncing, 9-Slice & Colour Effects",
        icon: i16::SYMBOL,
        keywords: "symbol movieclip button up over down hit graphic loop play once single frame 9-slice scale9 color effect tint brightness alpha library edit in place",
    },
    HelpChapterMeta {
        title: "7. Timeline, Layers & Masks",
        subtitle: "Keyframes, Span selection, Layer roles, Masking, Onion Skin & Scenes",
        icon: i16::FILM,
        keywords: "timeline frame keyframe blank span layer normal guide guided mask masked folder solo outline onion skin edit multiple frames scene",
    },
    HelpChapterMeta {
        title: "8. Tweens, Motion & IK",
        subtitle: "Motion Tween ◆ keys, Motion Editor, Classic Tween, Shape Hints & IK Bones",
        icon: i16::TWEEN_MOTION,
        keywords: "motion tween property keyframe roving motion path motion editor ease curve classic tween motion guide orient to path shape tween shape hints bone bind armature inverse kinematics",
    },
    HelpChapterMeta {
        title: "9. Filters, Text & Audio",
        subtitle: "7 Live Filters, 14 Blend Modes, Classic/TLF Text, Font Embedding & Sound",
        icon: i16::FILTER,
        keywords: "filter drop shadow blur glow bevel gradient adjust color blend mode text static dynamic input embed font tlf sound event start stop stream envelope video",
    },
    HelpChapterMeta {
        title: "10. Scripting, Import & Publish",
        subtitle: "AS2/AS3/JS/TS, Code Snippets, Debugger, FLA/SWF/PSD Import & HTML5/Atlas",
        icon: i16::CODE,
        keywords: "actionscript as2 as3 javascript typescript actions code snippets debugger profiler import fla xfl swf psd ai svg trace bitmap publish html5 wasm sprite sheet lottie shortcuts",
    },
];

/// Portable text edition used by the browser handoff. The in-app manual remains the
/// illustrated edition; these points mirror its complete chapter scope without depending
/// on a second, separately maintained product-help document.
const HELP_BROWSER_TOPICS: [&[&str]; 10] = [
    &[
        "Use the Stage for authored content and the Pasteboard for temporary off-stage artwork; rulers, guides and the document grid share one snapping priority.",
        "The Hand and Zoom tools, Focus Mode, Fit Stage and percentage controls change only the view and never mutate authored geometry.",
        "Ruler guides can be dragged from either ruler, moved on the Pasteboard, locked, hidden or removed by dropping them back on a ruler.",
        "Object, pixel, grid, guide and alignment snapping can be combined; visible snap rings identify the winning target.",
    ],
    &[
        "Merge Drawing lets fills and strokes cut, join and reshape each other; Object Drawing keeps every stroke or fill isolated until Break Apart.",
        "Primitive rectangles and ovals retain editable corner, radius and inner-radius controls, while ordinary shapes expose raw anchors and Bézier handles.",
        "Group, Break Apart, Union, Intersect, Punch and Crop preserve the distinction between editable objects and destructive Merge geometry.",
        "The J shortcut toggles Object Drawing for supported drawing tools without changing existing artwork.",
    ],
    &[
        "Selection (V) selects objects and Merge regions; Subselection (A) edits anchors, handles and multiple selected points.",
        "Free Transform (Q) supports scale, rotate/skew, distort and envelope modes plus an editable transformation point.",
        "Lasso (L) supports freehand, polygon and Magic Wand selection with smoothing and threshold settings.",
        "Arrow keys nudge selected objects or anchors by one pixel, and Shift increases the step to ten pixels.",
    ],
    &[
        "Pen (P) changes its pointer state for starting, continuing, closing, adding, deleting and converting anchors.",
        "Alt/Option converts corner and smooth points, while dragging creates or adjusts symmetric and independent tangent handles.",
        "Line (N), Rectangle (R), Oval (O), PolyStar and Pencil (Y) share snapping and Object Drawing behavior but retain their own live modifiers.",
        "Pencil modes Straighten, Smooth and Ink choose how aggressively the captured stroke is simplified.",
    ],
    &[
        "Brush (B) supports normal, fill, behind, selection and inside paint modes together with pressure, tilt, size, shape and Lock Fill.",
        "Paint Bucket (K) closes configurable gaps, applies solid or gradient fills and can drag a gradient direction directly on the Stage.",
        "Gradient Transform (F) edits linear and radial boundaries, width, radius, rotation, centre and focal point handles.",
        "Eraser (E) includes normal, fills, lines, selected fills, inside and whole-object modes plus round or square tips and Faucet behavior.",
    ],
    &[
        "MovieClip symbols run independent timelines; Graphic symbols can loop, play once or display a single frame; Buttons use Up, Over, Down and Hit states.",
        "Edit in Place preserves the parent-stage context while symbol isolation edits only the selected symbol definition.",
        "9-slice scaling protects corners and edges, while alpha, tint, brightness and advanced colour effects remain instance properties.",
        "The Library supports folders, shared libraries, duplicate/swap/update workflows and stable symbol identity.",
    ],
    &[
        "Frames, keyframes, blank keyframes and selected spans are edited independently from layer properties and visibility controls.",
        "Normal, Guide, Guided, Mask, Masked and Folder layers retain their role when reordered or nested.",
        "Onion Skin and Edit Multiple Frames expose neighbouring poses without making authored evaluation stateful.",
        "Scenes are authored separately but play and publish as one ordered main timeline.",
    ],
    &[
        "Motion Tween stores keyed X, Y, Scale, Rotation, Skew, Alpha and Colour tracks; the Motion Editor adjusts curves and easing.",
        "Classic Tween interpolates matching object poses and can follow a motion guide with Orient to Path.",
        "Shape Tween morphs compatible shapes and uses shape hints to control correspondence.",
        "IK bones, bindings, pins and spring controls build editable armatures without replacing the original artwork.",
    ],
    &[
        "Seven live filters and fourteen blend modes remain editable and are evaluated by the editor and published runtime.",
        "Static, Dynamic and Input text support embedded fonts, paragraph and character controls, multi-column flow and authored links.",
        "Frame sounds use Event, Start, Stop or Stream synchronization, looping and editable stereo envelopes.",
        "Video and audio resources remain document resources; scrubbing is silent unless audible scrubbing is explicitly enabled.",
    ],
    &[
        "Frame and object scripts may use ActionScript 2, ActionScript 3, JavaScript or TypeScript, with Actions, Code Snippets, compiler diagnostics and debugger panels.",
        "Import accepts legacy FLA, XFL, SWF, PSD/PSB, AI, SVG, PDF, media, swatches and image sequences with explicit diagnostics for approximations.",
        "Publish targets include HTML5 plus WebAssembly, sprite sheets, animated SVG, Lottie, audio and video.",
        "Core shortcuts include F5/F6/F7 for Timeline frames, F8 for Convert to Symbol, Command-B for Break Apart, F9 for Actions and Shift-F12 for Publish.",
    ],
];

fn escape_html(value: &str, output: &mut String) {
    for ch in value.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(ch),
        }
    }
}

fn help_browser_html() -> String {
    let mut html = String::from(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"color-scheme\" content=\"light dark\"><title>NeoFlash Help &amp; Reference</title></head><body><header><h1>NeoFlash Help &amp; Reference</h1><p>Portable text edition of the illustrated in-app manual.</p></header><nav aria-label=\"Chapters\"><ol>",
    );
    for (index, chapter) in HELP_CHAPTERS.iter().enumerate() {
        let _ = write!(html, "<li><a href=\"#chapter-{}\">", index + 1);
        escape_html(chapter.title, &mut html);
        html.push_str("</a></li>");
    }
    html.push_str("</ol></nav><main>");
    for (index, chapter) in HELP_CHAPTERS.iter().enumerate() {
        let _ = write!(html, "<section id=\"chapter-{}\"><h2>", index + 1);
        escape_html(chapter.title, &mut html);
        html.push_str("</h2><p>");
        escape_html(chapter.subtitle, &mut html);
        html.push_str("</p><ul>");
        for topic in HELP_BROWSER_TOPICS[index] {
            html.push_str("<li>");
            escape_html(topic, &mut html);
            html.push_str("</li>");
        }
        html.push_str("</ul><p><strong>Search terms:</strong> ");
        escape_html(chapter.keywords, &mut html);
        html.push_str("</p></section>");
    }
    html.push_str("</main></body></html>");
    html
}

fn write_help_browser_document() -> Result<PathBuf, String> {
    let directory = std::env::temp_dir()
        .join("neoflash-help")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("Cannot create the browser Help folder: {error}"))?;
    let path = directory.join("help-reference.html");
    std::fs::write(&path, help_browser_html())
        .map_err(|error| format!("Cannot write the browser Help document: {error}"))?;
    Ok(path)
}

impl crate::app::Editor {
    pub(crate) fn help_manual_dialog(&mut self, ctx: &egui::Context) {
        let mut state = ctx.data(|d| {
            d.get_temp::<HelpManualState>(state_id())
                .unwrap_or_default()
        });
        if !state.open {
            return;
        }
        state.page = state.page.min(HELP_CHAPTERS.len() - 1);
        let p = neoflash_ui::palette(ctx);
        let mut should_close = false;
        let mut open_cmd_search = false;
        let mut open_in_browser = false;
        let available = ctx.content_rect().size() - vec2(2.0 * space::S5, 2.0 * space::S5);
        // Never demand more than the current logical viewport. This matters at the supported
        // 1.25×/1.5× UI scales where a 1024×720 native window exposes fewer logical points.
        let dialog_width = DIALOG_WIDTH.min(available.x.max(1.0));
        let dialog_height = DIALOG_HEIGHT.min(available.y.max(1.0));
        let modal_id = egui::Id::new("neoflash-help-manual-modal");
        let modal = egui::Modal::new(modal_id)
            .area(
                egui::Modal::default_area(modal_id)
                    .anchor(egui::Align2::CENTER_TOP, vec2(0.0, space::S5)),
            )
            .frame(Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                // Declare both dimensions before the first child is laid out. This keeps the
                // modal, its close button and the article viewport stationary while search
                // results or chapters change.
                ui.set_min_width(dialog_width);
                ui.set_max_width(dialog_width);
                ui.set_min_height(dialog_height);
                ui.set_max_height(dialog_height);
                let top = ui.cursor().min;
                should_close |= DialogHeader {
                    title: "NeoFlash Help & Reference — Tools, Modes & Capabilities",
                }
                .show(ui);
                ui.painter().hline(
                    top.x..=top.x + dialog_width,
                    top.y + neoflash_ui::metrics::dialog_header() - 0.5,
                    Stroke::new(1.0_f32, p.border_subtle),
                );
                let chapter_badge =
                    format!("Chapter {} of {}", state.page + 1, HELP_CHAPTERS.len());
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        ui.set_width(dialog_width - 2.0 * space::S5);
                        ui.spacing_mut().item_spacing = vec2(space::S2, space::S2);

                        // Top filter bar: Search + chapter counter badge + Prev/Next chapter buttons
                        ui.horizontal(|ui| {
                            let search_width = (ui.available_width() - 350.0).clamp(120.0, 340.0);
                            ui.add(
                                Search::new(
                                    &mut state.query,
                                    "Filter topics, tools, modes or shortcuts…",
                                )
                                .width(search_width),
                            );
                            if !state.query.is_empty() && ui.add(Button::ghost("Clear")).clicked() {
                                state.query.clear();
                            }
                            ui.add_space(space::S2);
                            ui.add(Badge::new(&chapter_badge, BadgeType::Neutral));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_enabled(
                                            state.page + 1 < HELP_CHAPTERS.len(),
                                            Button::new("Next topic"),
                                        )
                                        .clicked()
                                    {
                                        state.page = (state.page + 1).min(HELP_CHAPTERS.len() - 1);
                                        state.diagram_tab = 0;
                                    }
                                    if ui
                                        .add_enabled(state.page > 0, Button::new("Previous"))
                                        .clicked()
                                    {
                                        state.page = state.page.saturating_sub(1);
                                        state.diagram_tab = 0;
                                    }
                                },
                            );
                        });

                        divider(ui, false);

                        let footer_h = neoflash_ui::metrics::control() + 2.0 * space::S3;
                        let body_height = (dialog_height
                            - neoflash_ui::metrics::dialog_header()
                            - neoflash_ui::metrics::control()
                            - 4.0 * space::S5
                            - footer_h)
                            .max(1.0);

                        let q = state.query.trim().to_ascii_lowercase();
                        ui.horizontal_top(|ui| {
                            // Left Chapter Sidebar
                            let sidebar_rect = Rect::from_min_size(
                                ui.next_widget_position(),
                                vec2(SIDEBAR_WIDTH, body_height),
                            );
                            let mut sidebar = ui.new_child(
                                egui::UiBuilder::new()
                                    .id_salt("help-manual-sidebar")
                                    .max_rect(sidebar_rect)
                                    .layout(egui::Layout::top_down(egui::Align::Min)),
                            );
                            sidebar.set_clip_rect(ui.clip_rect().intersect(sidebar_rect));
                            sidebar.spacing_mut().item_spacing.y = 2.0;
                            egui::ScrollArea::vertical()
                                .id_salt("help-manual-sidebar-scroll")
                                .max_height(body_height)
                                .auto_shrink([false, false])
                                .themed()
                                .show(&mut sidebar, |ui| {
                                    ui.set_min_width(SIDEBAR_WIDTH);
                                    for (idx, ch) in HELP_CHAPTERS.iter().enumerate() {
                                        let matches_q = q.is_empty()
                                            || ch.title.to_ascii_lowercase().contains(&q)
                                            || ch.subtitle.to_ascii_lowercase().contains(&q)
                                            || ch.keywords.contains(&q);
                                        if !matches_q {
                                            continue;
                                        }
                                        if TreeItem::new(ch.title, TreeItemType::Symbol(ch.icon))
                                            .selected(state.page == idx)
                                            .show(ui)
                                            .row
                                            .clicked()
                                        {
                                            state.page = idx;
                                            state.diagram_tab = 0;
                                        }
                                    }
                                });
                            ui.advance_cursor_after_rect(sidebar_rect);

                            ui.add_space(space::S4);

                            // Right Scrollable Illustrated Article Area. A fixed child prevents
                            // long chapter rows from changing the modal width.
                            let content_w =
                                (dialog_width - 2.0 * space::S5 - SIDEBAR_WIDTH - space::S4)
                                    .max(1.0);
                            let content_rect = Rect::from_min_size(
                                ui.next_widget_position(),
                                vec2(content_w, body_height),
                            );
                            let mut content = ui.new_child(
                                egui::UiBuilder::new()
                                    .id_salt("help-manual-content")
                                    .max_rect(content_rect)
                                    .layout(egui::Layout::top_down(egui::Align::Min)),
                            );
                            content.set_clip_rect(ui.clip_rect().intersect(content_rect));
                            egui::ScrollArea::vertical()
                                .id_salt(("help-manual-scroll", state.page))
                                .max_height(body_height)
                                .auto_shrink([false, false])
                                .themed()
                                .show(&mut content, |ui| {
                                    ui.set_min_width(content_w - space::S3);
                                    ui.set_max_width(content_w - space::S3);
                                    render_chapter(ui, &mut state);
                                });
                            ui.advance_cursor_after_rect(content_rect);
                        });
                    });

                dialog_footer(
                    ui,
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::S2;
                            ui.label(rich("Tip: Press", TextStyle::Small).color(p.text_secondary));
                            kbd(ui, "F1", false);
                            ui.label(
                                rich(
                                    "anytime to search & execute menu commands directly.",
                                    TextStyle::Small,
                                )
                                .color(p.text_secondary),
                            );
                        });
                    },
                    |ui| {
                        if ui.add(Button::new("Open in browser")).clicked() {
                            open_in_browser = true;
                        }
                        if ui.add(Button::new("Command Search (F1)")).clicked() {
                            should_close = true;
                            open_cmd_search = true;
                        }
                        should_close |= ui.add(Button::primary("Close").shortcut("Esc")).clicked();
                    },
                );
            });

        let esc = !ctx.wants_keyboard_input()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if should_close || esc || modal.should_close() {
            state.open = false;
        }
        ctx.data_mut(|d| d.insert_temp(state_id(), state));
        if open_in_browser {
            match write_help_browser_document().and_then(|path| {
                std::process::Command::new("open")
                    .arg(&path)
                    .spawn()
                    .map(|_| path)
                    .map_err(|error| format!("Cannot open Help in the browser: {error}"))
            }) {
                Ok(path) => self.message(format!("Opened Help in browser: {}", path.display())),
                Err(error) => self.notify_with_output(c::ToastType::Error, error),
            }
        }
        if open_cmd_search {
            self.open_function_search(ctx);
        }
    }
}

fn render_chapter(ui: &mut Ui, state: &mut HelpManualState) {
    let p = neoflash_ui::palette(ui.ctx());
    let meta = &HELP_CHAPTERS[state.page.min(HELP_CHAPTERS.len() - 1)];

    // Chapter Header Banner
    ui.horizontal(|ui| {
        icons::show(ui, meta.icon, p.accent_default);
        ui.vertical(|ui| {
            ui.label(rich(meta.title, TextStyle::DialogTitle).color(p.text_primary));
            ui.label(rich(meta.subtitle, TextStyle::Small).color(p.text_secondary));
        });
    });
    ui.add_space(space::S2);
    divider(ui, false);
    ui.add_space(space::S2);

    match state.page {
        0 => chapter_workspace_and_stage(ui, state),
        1 => chapter_drawing_modes_and_objects(ui, state),
        2 => chapter_selection_and_transform(ui, state),
        3 => chapter_vector_and_pen_states(ui, state),
        4 => chapter_brush_eraser_and_deco(ui, state),
        5 => chapter_symbols_9slice_and_library(ui, state),
        6 => chapter_timeline_layers_and_masks(ui, state),
        7 => chapter_tweens_motion_and_ik(ui, state),
        8 => chapter_filters_text_and_audio(ui, state),
        _ => chapter_scripting_import_and_publish(ui, state),
    }
}

// ── Shared structural helpers (100% NF2 tokens & components) ──

fn section_overline(ui: &mut Ui, title: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.add_space(space::S2);
    ui.label(rich(title, TextStyle::Overline).color(p.text_tertiary));
    ui.add_space(space::S1);
}

fn paragraph(ui: &mut Ui, text: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.add(egui::Label::new(rich(text, TextStyle::DialogBody).color(p.text_primary)).wrap());
}

fn sub_bullet(ui: &mut Ui, lead: &str, shortcut: Option<&str>, body: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = space::S1;
        ui.label(rich("•", TextStyle::BodyStrong).color(p.accent_default));
        ui.label(rich(lead, TextStyle::BodyStrong).color(p.text_primary));
        if let Some(sc) = shortcut {
            kbd(ui, sc, false);
        }
        ui.label(rich("—", TextStyle::Body).color(p.text_tertiary));
        ui.label(rich(body, TextStyle::Body).color(p.text_secondary));
    });
}

fn mode_card(
    ui: &mut Ui,
    icon: Option<Icon>,
    mode_name: &str,
    badge_text: &str,
    badge_kind: BadgeType,
    pointer_or_hotkey: &str,
    behaviors: &[&str],
) {
    let p = neoflash_ui::palette(ui.ctx());
    let r = egui::CornerRadius::same(radius::MD as u8);
    egui::Frame::new()
        .fill(p.bg_panel)
        .stroke(Stroke::new(1.0_f32, p.border_subtle))
        .corner_radius(r)
        .inner_margin(egui::Margin::same(space::S3 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if let Some(ic) = icon {
                    icons::show(ui, ic, p.accent_default);
                }
                ui.label(rich(mode_name, TextStyle::BodyStrong).color(p.text_primary));
                ui.add(Badge::new(badge_text, badge_kind));
                if !pointer_or_hotkey.is_empty() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        kbd(ui, pointer_or_hotkey, false);
                    });
                }
            });
            ui.add_space(space::S1);
            for b in behaviors {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S1;
                    ui.label(rich("▸", TextStyle::Small).color(p.accent_default));
                    ui.label(rich(*b, TextStyle::Small).color(p.text_secondary));
                });
            }
        });
}

/// Allocates a themed illustration canvas box (`bg_canvas` with `border_subtle` frame) and passes
/// `(painter, rect)` to `draw`.
fn illustration_canvas(
    ui: &mut Ui,
    height: f32,
    caption_text: &str,
    draw: impl FnOnce(&egui::Painter, &egui::Context, Rect),
) {
    let p = neoflash_ui::palette(ui.ctx());
    let width = ui.available_width().max(260.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let r = egui::CornerRadius::same(radius::LG as u8);
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, r, p.bg_canvas);
    draw(&painter, ui.ctx(), rect);
    painter.rect_stroke(
        rect,
        r,
        Stroke::new(1.0_f32, p.border_subtle),
        StrokeKind::Inside,
    );
    ui.horizontal(|ui| {
        icons::show(ui, i16::INFO, p.text_tertiary);
        ui.label(rich(caption_text, TextStyle::Small).color(p.text_secondary));
    });
    ui.add_space(space::S2);
}

// ── Chapter 1: Workspace & Stage ──

fn chapter_workspace_and_stage(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "NeoFlash combines the classic frame-and-symbol authoring model of Adobe Flash Professional (CS4–CS6) with a deterministic Rust engine and native HTML5 / WebAssembly publishing.",
    );

    section_overline(
        ui,
        "Interactive Diagram — Stage, Rulers, Guides, Grid & Snap Ring",
    );
    illustration_canvas(
        ui,
        176.0,
        "Illustration 1.1: Stage (bg/stage) inside the Pasteboard (bg/canvas), overlaid with NF2 Rulers, Ruler Corner (0,0), Cyan Guides, Stage Grid, and the Selection Snap Ring.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let inner = rect.shrink(6.0);
            let page =
                Rect::from_min_max(inner.min + vec2(38.0, 30.0), inner.max - vec2(24.0, 14.0));
            painter.rect_filled(page, 0.0, p.bg_stage);
            paint_stage_grid(painter, ctx, page, 1.0, vec2(20.0, 20.0), None);
            painter.rect_stroke(
                page,
                0.0,
                Stroke::new(1.0_f32, p.border_default),
                StrokeKind::Outside,
            );

            // Rulers & Corner
            let corner = Rect::from_min_size(inner.min, vec2(20.0, 20.0));
            let h_ruler = Rect::from_min_max(
                egui::pos2(corner.right(), inner.top()),
                egui::pos2(inner.right(), corner.bottom()),
            );
            let v_ruler = Rect::from_min_max(
                egui::pos2(inner.left(), corner.bottom()),
                egui::pos2(corner.right(), inner.bottom()),
            );
            paint_stage_ruler(
                painter,
                ctx,
                h_ruler,
                Orientation::Horizontal,
                page.left(),
                1.0,
                Some(100.0),
            );
            paint_stage_ruler(
                painter,
                ctx,
                v_ruler,
                Orientation::Vertical,
                page.top(),
                1.0,
                Some(50.0),
            );
            paint_ruler_corner(painter, ctx, corner);

            // Guides: one selected horizontal guide, one locked vertical guide
            let extent = Rect::from_min_max(corner.max, inner.max);
            let gy = page.top() + 56.0;
            let gx = page.left() + 180.0;
            paint_guide(
                painter,
                ctx,
                Orientation::Horizontal,
                gy,
                extent,
                GuideState::Selected,
                None,
            );
            paint_guide(
                painter,
                ctx,
                Orientation::Vertical,
                gx,
                extent,
                GuideState::Locked,
                None,
            );

            // Symbol instance snapped to the guide intersection
            let obj = Rect::from_center_size(egui::pos2(gx, gy), vec2(76.0, 44.0));
            painter.rect_filled(obj, 4.0, p.accent_subtle);
            paint_selection_box(painter, ctx, obj, SelectionKind::Symbol);
            paint_snap_ring(painter, ctx, egui::pos2(gx, gy), true);

            painter.text(
                egui::pos2(gx + 46.0, gy - 18.0),
                egui::Align2::LEFT_CENTER,
                "Snapped to Guide + Grid (Snap Ring enlarged)",
                font(TextStyle::MonoSmall),
                p.text_primary,
            );
        },
    );

    section_overline(ui, "Stage Navigation & Viewport Modes");
    mode_card(
        ui,
        Some(i16::HAND),
        "Hand Tool (H) & Temporary Spacebar Pan",
        "Navigation",
        BadgeType::Accent,
        "H / Space",
        &[
            "Hold Spacebar while any drawing or selection tool is active to temporarily pan the Stage canvas; releasing Spacebar immediately restores your active tool.",
            "Double-click the Hand Tool icon in the Tools panel (or press ⌘2 / View ▸ Magnification ▸ Fit in Window) to center and fit the Stage into the viewport.",
        ],
    );
    mode_card(
        ui,
        Some(i16::ZOOMIN),
        "Zoom Tool (Z) — Enlarge (+) vs Reduce (-) & Marquee Zoom",
        "Magnification",
        BadgeType::Accent,
        "Z / ⌥-Click",
        &[
            "Enlarge Mode (+): Single-clicking zooms in 2× centered on the click point; dragging a marquee rectangle magnifies that exact region to fill the viewport.",
            "Reduce Mode (-): Hold Option (⌥) while clicking—or switch the modifier in Tool Options—to zoom out.",
            "Double-clicking the Zoom Tool icon in the Tools rail resets magnification to 100% (⌘1). Press ⌘3 (Show All) to frame every object on both the Stage and Pasteboard.",
        ],
    );
    mode_card(
        ui,
        Some(i16::SNAP_GUIDES),
        "Snapping Priority Hierarchy & Ruler Origin",
        "Precision",
        BadgeType::Accent,
        "View ▸ Snapping",
        &[
            "Deterministic Snap Priority: When multiple snap targets are enabled, NeoFlash evaluates them in strict priority order: 1) Ruler Guides → 2) Objects (vertices/centres/paths) → 3) Stage Grid → 4) Pixels.",
            "Ruler Origin (0,0): Drag from the top-left 20×20 Ruler Corner onto the Stage to relocate the coordinate origin; double-click the Ruler Corner to reset (0,0) to the top-left corner of the Stage (or symbol registration point inside Symbol-Editing Mode).",
            "Focus Mode (Tab): Press Tab (when not editing text) to hide all docked panels and maximize the Stage canvas; press Tab again to restore your workspace layout.",
        ],
    );
}

// ── Chapter 2: Drawing Modes & Objects ──

fn chapter_drawing_modes_and_objects(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "Understanding how Flash and NeoFlash represent vector artwork on a layer is essential. The same tool (such as Rectangle, Oval, Brush, or Line) behaves differently depending on whether Merge Drawing, Object Drawing (J), or Primitive Mode is active.",
    );

    section_overline(
        ui,
        "Interactive Diagram — The 5 Graphic Object Regimes on Stage",
    );
    illustration_canvas(
        ui,
        158.0,
        "Illustration 2.1: Visual comparison of selection chrome across the 5 Stage object regimes: (1) Merge Drawing 4pt crosshatch, (2) Drawing Object / Group box, (3) Parametric Primitive handles, (4) Symbol Instance registration + transform point, (5) Text block handle.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let cols = 5;
            let pad = 10.0;
            let card_w = (rect.width() - pad * (cols as f32 + 1.0)) / cols as f32;
            let top = rect.top() + 12.0;
            let box_h = 84.0;

            let titles = [
                ("1. Merge Shape", "Planar split / hatch"),
                ("2. Object (J)", "Isolated container"),
                ("3. Primitive", "Live corner/angle"),
                ("4. Symbol (F8)", "+ Reg & ○ Pivot"),
                ("5. Text Block", "○ Auto / □ Fixed"),
            ];

            for (i, (title, sub)) in titles.iter().enumerate() {
                let x = rect.left() + pad + i as f32 * (card_w + pad);
                let cell = Rect::from_min_size(egui::pos2(x, top), vec2(card_w, box_h));
                painter.rect_filled(cell, 4.0, p.bg_panel);
                let shape = cell.shrink2(vec2(18.0, 18.0));
                painter.rect_filled(shape, 3.0, p.accent_subtle);

                match i {
                    0 => {
                        paint_shape_selection(painter, ctx, shape);
                    }
                    1 => {
                        paint_selection_box(painter, ctx, shape, SelectionKind::Group);
                    }
                    2 => {
                        paint_selection_box(painter, ctx, shape, SelectionKind::Primitive);
                    }
                    3 => {
                        paint_selection_box(painter, ctx, shape, SelectionKind::Symbol);
                    }
                    _ => {
                        paint_selection_box(painter, ctx, shape, SelectionKind::TextStatic);
                    }
                }

                painter.text(
                    egui::pos2(cell.center().x, cell.bottom() + 14.0),
                    egui::Align2::CENTER_CENTER,
                    *title,
                    font(TextStyle::BodyStrong),
                    p.text_primary,
                );
                painter.text(
                    egui::pos2(cell.center().x, cell.bottom() + 28.0),
                    egui::Align2::CENTER_CENTER,
                    *sub,
                    font(TextStyle::Small),
                    p.text_secondary,
                );
            }
        },
    );

    section_overline(
        ui,
        "Per-Mode Behavior: Merge vs Object Drawing vs Primitives",
    );
    mode_card(
        ui,
        Some(i16::SCISSORS),
        "Mode 1: Merge Drawing Mode (Default when Object Drawing J is Off)",
        "Planar Topology",
        BadgeType::Warning,
        "J = Off",
        &[
            "Same-Colour Union: Drawing or moving a fill over another fill of the exact same colour on the same layer permanently merges them into a single unified contour.",
            "Different-Colour Cutout (Cookie-Cutter): Drawing a shape of a different colour over an existing fill, deselected and then dragged away, punches out the overlapped area from the underlying shape.",
            "Stroke Splitting: Overlapping lines automatically split at their intersection vertices so individual line segments can be selected and deleted.",
            "Partial Marquee & Lasso Slicing: Dragging a Selection marquee or Lasso loop across half of a Merge shape selects only that region (highlighted with the 4 pt two-tone hatch) and slices the fill cleanly when moved.",
        ],
    );
    mode_card(
        ui,
        Some(i16::RECT),
        "Mode 2: Object Drawing Mode (Toggle J in Tools Panel)",
        "Non-Destructive",
        BadgeType::Accent,
        "J = On",
        &[
            "Each stroke or shape drawn while Object Drawing (J) is active is wrapped in an independent Drawing Object container with an accent bounding box.",
            "Overlapping Drawing Objects on the same layer never split or punch holes in one another.",
            "Double-clicking a Drawing Object enters its isolated editing context; Modify ▸ Break Apart (⌘B) unpacks it back into raw Merge Drawing geometry.",
        ],
    );
    mode_card(
        ui,
        Some(i16::OVAL),
        "Mode 3: Parametric Shape Primitives (Rectangle Primitive & Oval Primitive)",
        "Live Parameters",
        BadgeType::Accent,
        "R / O",
        &[
            "Rectangle Primitive: Retains live, non-destructive Corner Radius controls (including negative values for inverted concave corners) that can be dragged on Stage or edited in Properties at any time.",
            "Oval Primitive: Retains live Start Angle (0–360°), End Angle (0–360°), Inner Radius (0–99% donut hole), and Close Path toggles for pie wedges, rings, and arcs.",
        ],
    );
    mode_card(
        ui,
        Some(i16::LAYERS),
        "Mode 4: Boolean Combine Objects & Break Apart (⌘B) Across Types",
        "Modify Menu",
        BadgeType::Neutral,
        "⌘B / ⌘G",
        &[
            "Modify ▸ Combine Objects: Union (merges into one Drawing Object), Intersect (keeps overlapping region with top object's style), Punch (top object cuts through all underlying objects and disappears), Crop (clips underlying objects to top object's silhouette).",
            "Modify ▸ Break Apart (⌘B) is context-sensitive: (1) On a Group or Drawing Object → unwraps to Merge shapes; (2) On a Symbol Instance → detaches from Library into current-frame elements; (3) On a Bitmap → converts to a vector shape painted with a Bitmap Fill; (4) On Text → 1st ⌘B splits string into individual character text boxes, 2nd ⌘B converts glyphs into editable Bézier outlines.",
        ],
    );
}

// ── Chapter 3: Selection, Subselection & Transform ──

fn chapter_selection_and_transform(ui: &mut Ui, state: &mut HelpManualState) {
    let tabs = [
        (0, "Selection (V) & Subselection (A)"),
        (1, "Free Transform (Q) — 4 Sub-Modes"),
        (2, "Lasso (L) & Magic Wand"),
    ];
    c::Segmented::show(ui, &mut state.diagram_tab, &tabs, false);
    ui.add_space(space::S2);

    match state.diagram_tab {
        0 => {
            illustration_canvas(
                ui,
                148.0,
                "Illustration 3.1: Left — Selection Tool (V) reshaping a curve segment by direct edge drag vs ⌥-dragging to split a new corner vertex. Right — Subselection Tool (A) exposing hollow/filled anchor squares and Bézier tangent handles.",
                |painter, ctx, rect| {
                    let p = neoflash_ui::palette(ctx);
                    let mid_x = rect.center().x;
                    painter.vline(mid_x, rect.y_range(), Stroke::new(1.0_f32, p.border_subtle));

                    // Left: Selection Tool (V) curve bend
                    let l_center = egui::pos2((rect.left() + mid_x) * 0.5, rect.center().y + 6.0);
                    let a = l_center + vec2(-90.0, 26.0);
                    let b = l_center + vec2(90.0, 26.0);
                    let ctrl = l_center + vec2(0.0, -42.0);
                    let pts: Vec<Pos2> = (0..=32)
                        .map(|i| {
                            let t = i as f32 / 32.0;
                            let u = 1.0 - t;
                            egui::pos2(
                                u * u * a.x + 2.0 * u * t * ctrl.x + t * t * b.x,
                                u * u * a.y + 2.0 * u * t * ctrl.y + t * t * b.y,
                            )
                        })
                        .collect();
                    painter.add(egui::Shape::line(
                        pts,
                        Stroke::new(2.0_f32, p.accent_default),
                    ));
                    painter.circle_filled(l_center + vec2(0.0, -8.0), 4.0, p.warning_default);
                    painter.text(
                        egui::pos2(l_center.x, rect.top() + 16.0),
                        egui::Align2::CENTER_CENTER,
                        "Selection (V): Drag edge = Smooth Curve | ⌥+Drag = New Corner",
                        font(TextStyle::Small),
                        p.text_primary,
                    );

                    // Right: Subselection Tool (A) anchors & handles
                    let r_center = egui::pos2((mid_x + rect.right()) * 0.5, rect.center().y + 10.0);
                    let p0 = r_center + vec2(-85.0, 22.0);
                    let p1 = r_center + vec2(0.0, -20.0);
                    let p2 = r_center + vec2(85.0, 22.0);
                    painter.line_segment([p0, p1], Stroke::new(1.5_f32, p.accent_default));
                    painter.line_segment([p1, p2], Stroke::new(1.5_f32, p.accent_default));
                    paint_bezier_handle(painter, ctx, p1, p1 + vec2(-48.0, -18.0));
                    paint_bezier_handle(painter, ctx, p1, p1 + vec2(48.0, 18.0));
                    paint_anchor(painter, ctx, p0, AnchorState::Unselected);
                    paint_anchor(painter, ctx, p1, AnchorState::Selected);
                    paint_anchor(painter, ctx, p2, AnchorState::Hover);
                    painter.text(
                        egui::pos2(r_center.x, rect.top() + 16.0),
                        egui::Align2::CENTER_CENTER,
                        "Subselection (A): □ Unselected · ■ Selected · ⌥+Drag Handle = Corner",
                        font(TextStyle::Small),
                        p.text_primary,
                    );
                },
            );
        }
        1 => {
            illustration_canvas(
                ui,
                148.0,
                "Illustration 3.2: Free Transform Tool (Q) sub-modes: (1) Scale 8 square handles, (2) Rotate & Skew round corners + edge bars, (3) Distort / ⇧-Perspective Taper, (4) Envelope 8-anchor / 16-handle bicubic Coons mesh.",
                |painter, ctx, rect| {
                    let p = neoflash_ui::palette(ctx);
                    let cols = 4;
                    let pad = 12.0;
                    let w = (rect.width() - pad * (cols as f32 + 1.0)) / cols as f32;
                    for i in 0..cols {
                        let x = rect.left() + pad + i as f32 * (w + pad);
                        let cell =
                            Rect::from_min_size(egui::pos2(x, rect.top() + 26.0), vec2(w, 86.0));
                        let b = cell.shrink2(vec2(26.0, 16.0));
                        painter.rect_filled(b, 2.0, p.accent_subtle);
                        let label = match i {
                            0 => {
                                paint_transform_box(painter, ctx, b, TransformMode::Scale);
                                "1. Scale (⇧ Prop, ⌥ Corner)"
                            }
                            1 => {
                                paint_transform_box(painter, ctx, b, TransformMode::RotateSkew);
                                "2. Rotate & Skew (○ / ▬)"
                            }
                            2 => {
                                let quad = vec![
                                    b.left_top() + vec2(12.0, 0.0),
                                    b.right_top() + vec2(-12.0, 0.0),
                                    b.right_bottom() + vec2(8.0, 0.0),
                                    b.left_bottom() + vec2(-8.0, 0.0),
                                    b.left_top() + vec2(12.0, 0.0),
                                ];
                                painter.add(egui::Shape::line(
                                    quad.clone(),
                                    Stroke::new(1.0_f32, p.accent_default),
                                ));
                                for pt in &quad[..4] {
                                    paint_anchor(painter, ctx, *pt, AnchorState::Selected);
                                }
                                "3. Distort & ⇧⌘ Taper"
                            }
                            _ => {
                                paint_transform_box(painter, ctx, b, TransformMode::Scale);
                                paint_bezier_handle(
                                    painter,
                                    ctx,
                                    b.center_top(),
                                    b.center_top() + vec2(-18.0, -10.0),
                                );
                                paint_bezier_handle(
                                    painter,
                                    ctx,
                                    b.center_top(),
                                    b.center_top() + vec2(18.0, 10.0),
                                );
                                "4. Envelope (8 pts / 16 tangents)"
                            }
                        };
                        painter.text(
                            egui::pos2(cell.center().x, rect.top() + 14.0),
                            egui::Align2::CENTER_CENTER,
                            label,
                            font(TextStyle::Small),
                            p.text_primary,
                        );
                    }
                },
            );
        }
        _ => {
            illustration_canvas(
                ui,
                148.0,
                "Illustration 3.3: Lasso Tool (L) modes: Freehand loop, Polygon straight-edge clicks, Hybrid (hold ⌥ during Freehand drag), and Magic Wand contiguous colour threshold.",
                |painter, ctx, rect| {
                    let p = neoflash_ui::palette(ctx);
                    let c1 = egui::pos2(rect.left() + rect.width() * 0.28, rect.center().y + 6.0);
                    let c2 = egui::pos2(rect.left() + rect.width() * 0.72, rect.center().y + 6.0);
                    let poly = vec![
                        c1 + vec2(-70.0, -28.0),
                        c1 + vec2(10.0, -36.0),
                        c1 + vec2(74.0, 10.0),
                        c1 + vec2(-20.0, 38.0),
                        c1 + vec2(-70.0, -28.0),
                    ];
                    painter.extend(egui::Shape::dashed_line(
                        &poly,
                        Stroke::new(1.2_f32, p.accent_default),
                        4.0,
                        3.0,
                    ));
                    for pt in &poly[..4] {
                        paint_anchor(painter, ctx, *pt, AnchorState::Unselected);
                    }
                    painter.text(
                        egui::pos2(c1.x, rect.top() + 16.0),
                        egui::Align2::CENTER_CENTER,
                        "Lasso Freehand + Hold ⌥ for Straight Polygon Edges",
                        font(TextStyle::Small),
                        p.text_primary,
                    );

                    let wand_rect = Rect::from_center_size(c2, vec2(130.0, 68.0));
                    painter.rect_filled(wand_rect, 4.0, p.accent_subtle);
                    paint_shape_selection(painter, ctx, wand_rect.shrink(14.0));
                    painter.text(
                        egui::pos2(c2.x, rect.top() + 16.0),
                        egui::Align2::CENTER_CENTER,
                        "Magic Wand: Selects Bitmap Pixels Within Threshold (0–200)",
                        font(TextStyle::Small),
                        p.text_primary,
                    );
                },
            );
        }
    }

    section_overline(ui, "Detailed Tool Modes & Pointer States");
    mode_card(
        ui,
        Some(i16::SELECT),
        "Selection Tool (V) — 6 Contextual Pointer States & Temporary ⌘ Override",
        "Primary Tool",
        BadgeType::Accent,
        "V / Hold ⌘",
        &[
            "1) Empty Stage Marquee: Dragging on empty Stage draws a selection marquee (`NF2/Marquee`). Single click on a fill selects the fill; double-clicking a fill selects both the fill and its enclosing stroke; double-clicking a stroke selects all connected stroke segments.",
            "2) Move / Duplicate Cursor (Arrow + Black Box): Dragging inside a selected item moves it (`⇧` constrains to 45° axes; `⌥`-drag duplicates the selection).",
            "3) Curve Reshape Cursor (Arrow + Arc): Hovering over an unselected smooth edge and dragging bends the curve without moving its endpoint corners.",
            "4) Corner / Endpoint Cursor (Arrow + Right Angle): Hovering over a corner vertex or open line end moves that vertex.",
            "5) Split New Corner (⌥-Drag on Curve): Holding Option (⌥) while dragging an unselected curve segment splits the segment and pulls out a sharp corner vertex.",
            "6) Smooth & Straighten Modifiers: Clicking Smooth or Straighten in Tool Options progressively refines selected strokes (repeated Straighten triggers geometric shape recognition for detached ovals, rectangles, and triangles).",
        ],
    );
    mode_card(
        ui,
        Some(i16::SUBSELECT),
        "Subselection Tool (A) — Direct Node, Tangent & Curve Editing",
        "Bézier Nodes",
        BadgeType::Accent,
        "A",
        &[
            "Clicking a path outline reveals hollow square Anchor Points (`□`). Clicking an anchor fills it (`■`) and exposes its Bézier tangent handles (`●`).",
            "Dragging a tangent handle rotates both handles symmetrically around a smooth point; holding Option (⌥) breaks symmetry so each handle moves independently (corner point).",
            "Deleting Nodes: Pressing Delete/Backspace in Subselection removes the selected anchor AND its adjoining curve segments (opening the path), whereas clicking the anchor with the Delete Anchor Point Tool (-) heals the path into a single continuous curve.",
        ],
    );
    mode_card(
        ui,
        Some(i16::TRANSFORM),
        "Free Transform Tool (Q) — 4 Sub-Modes & Pivot Point",
        "Transform",
        BadgeType::Accent,
        "Q",
        &[
            "Transformation Point (○): Drag the white centre circle to relocate the pivot for rotation and scaling; double-click the circle to snap it back to the object's geometric centre.",
            "Sub-Mode 1 — Rotate and Skew: Corner handles only rotate (`⇧` = 45° steps, `⌥` = pivot around opposite corner); edge handles only skew.",
            "Sub-Mode 2 — Scale: Corner and edge handles only scale (`⇧` = proportional aspect ratio, `⌥` = scale from opposite corner vs centre).",
            "Sub-Mode 3 — Distort (Vector shapes): Drag any corner or edge freely (`⌘`-drag in default mode); hold `⇧⌘` while dragging a corner to apply a symmetric perspective Taper (moving the dragged corner and the adjacent corner on the same axis equally in opposite directions).",
            "Sub-Mode 4 — Envelope (Vector shapes): Wraps the selection in an 8-point, 16-tangent-handle bicubic Coons patch for fluid non-linear warping.",
        ],
    );
}

// ── Chapter 4: Vector Drawing & Pen States ──

fn chapter_vector_and_pen_states(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "The Pen Tool (P) and geometric shape tools construct clean quadratic and cubic Bézier paths. In Flash Professional and NeoFlash, the Pen cursor changes badge to show which of the 9 path operations will occur on click.",
    );

    section_overline(
        ui,
        "Interactive Diagram — The 9 Pointer States of the Pen Tool (P)",
    );
    illustration_canvas(
        ui,
        152.0,
        "Illustration 4.1: All 9 Pen Tool (P) pointer badges documented in flash_reference.pdf (pp. 160–163) and how each modifies the active or selected Bézier path.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let badges = [
                ("1. Pen ×", "Start new path"),
                ("2. Pen", "Next point"),
                ("3. Pen +", "Insert on seg"),
                ("4. Pen −", "Smooth→Corner→Del"),
                ("5. Pen /", "Continue end"),
                ("6. Pen ○", "Close path"),
                ("7. Pen ⛓", "Join 2 paths"),
                ("8. Pen ∧", "Retract handle"),
                ("9. ⇧C / ⌥", "Convert point"),
            ];
            let cols = 3;
            let rows = 3;
            let pad_x = 10.0;
            let pad_y = 8.0;
            let cw = (rect.width() - pad_x * (cols as f32 + 1.0)) / cols as f32;
            let ch = (rect.height() - pad_y * (rows as f32 + 1.0)) / rows as f32;
            for (idx, (code, desc)) in badges.iter().enumerate() {
                let r_idx = idx / cols;
                let c_idx = idx % cols;
                let x = rect.left() + pad_x + c_idx as f32 * (cw + pad_x);
                let y = rect.top() + pad_y + r_idx as f32 * (ch + pad_y);
                let cell = Rect::from_min_size(egui::pos2(x, y), vec2(cw, ch));
                painter.rect_filled(cell, 4.0, p.bg_panel);
                icons::paint(
                    painter,
                    ctx,
                    Rect::from_min_size(cell.min + vec2(8.0, (ch - 16.0) * 0.5), vec2(16.0, 16.0)),
                    i16::PEN,
                    p.accent_default,
                );
                painter.text(
                    egui::pos2(cell.left() + 30.0, cell.center().y - 6.0),
                    egui::Align2::LEFT_CENTER,
                    *code,
                    font(TextStyle::BodyStrong),
                    p.text_primary,
                );
                painter.text(
                    egui::pos2(cell.left() + 30.0, cell.center().y + 7.0),
                    egui::Align2::LEFT_CENTER,
                    *desc,
                    font(TextStyle::Small),
                    p.text_secondary,
                );
            }
        },
    );

    mode_card(
        ui,
        Some(i16::PEN),
        "Pen Tool Family (P, =, -, ⇧C) — Two-Step Smooth Node Deletion & Open Paths",
        "9 Cursor States",
        BadgeType::Accent,
        "P / = / - / ⇧C",
        &[
            "Two-Step Smooth Point Deletion (Pen −): Clicking an existing smooth curve point with the Pen or Delete Anchor Point Tool (-) first converts it into a corner point (retracting its handles); clicking it a second time deletes the vertex while keeping the path closed/continuous.",
            "Finishing an Open Path: Double-click the last point, press Escape, or Command-click (⌘) away from the path to leave a stroke open without closing its fill.",
            "Modifier Keys While Drawing: Hold Shift (⇧) to constrain new anchors or tangent handles to 45° increments; hold Option (⌥) to split paired tangent handles into an acute V-corner; hold Command (⌘) to temporarily switch to Subselection.",
        ],
    );
    mode_card(
        ui,
        Some(i16::PENCIL),
        "Pencil Tool (Y) — Straighten, Smooth & Ink Modes",
        "Freehand Stroke",
        BadgeType::Accent,
        "Y",
        &[
            "1) Straighten Mode: Straightens jittery segments and recognizes hand-drawn triangles, rectangles, squares, ovals, and circles, converting them into crisp geometric primitives.",
            "2) Smooth Mode: Fits smooth Bézier curves using the Smoothing slider (0–100, default 50) in the Properties inspector.",
            "3) Ink Mode: Disables shape recognition and heavy smoothing (the Smoothing slider is disabled) to preserve raw hand-drawn linework.",
        ],
    );
    mode_card(
        ui,
        Some(i16::RECT),
        "Line (N), Rectangle (R), Oval (O) & PolyStar — Interactive Drag Modifiers",
        "Geometric",
        BadgeType::Accent,
        "N / R / O",
        &[
            "Up / Down Arrow Keys While Dragging Rectangle: Adjusts the rounded corner radius live before releasing the mouse button.",
            "Option (⌥) + Single Click on Stage: Opens the numeric Rectangle / Oval Settings dialog to enter exact pixel Width, Height, Corner Radius, and Draw from Center.",
            "PolyStar Tool (under Rectangle flyout): Click Options… in Properties to choose Polygon or Star, Number of Sides (3–32), and Star Point Size (0.00–1.00; values closer to 0 produce sharper needle points).",
        ],
    );
}

// ── Chapter 5: Brush, Eraser & Deco ──

fn chapter_brush_eraser_and_deco(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "Unlike the Pencil Tool (which creates centre-line strokes), the Brush Tool (B) paints closed filled vector contours. Its 5 Paint Modes control which existing z-planes on the active layer are affected.",
    );

    section_overline(
        ui,
        "Interactive Diagram — Brush Stroke, Gradient Transform (5 Handles) & Paint Modes",
    );
    illustration_canvas(
        ui,
        152.0,
        "Illustration 5.1: Left — NF2 Brush calligraphic stroke preview. Centre — Gradient Transform Tool (F) showing all 5 radial handles (Centre, Focal Point ▼, Width □, Size ○, Rotation ●). Right — The 5 Brush Paint Modes.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let w3 = rect.width() / 3.0;
            let r1 = Rect::from_min_max(rect.min, egui::pos2(rect.left() + w3, rect.bottom()));
            let r2 = Rect::from_min_max(
                egui::pos2(r1.right(), rect.top()),
                egui::pos2(r1.right() + w3, rect.bottom()),
            );
            let r3 = Rect::from_min_max(egui::pos2(r2.right(), rect.top()), rect.max);
            painter.vline(
                r1.right(),
                rect.y_range(),
                Stroke::new(1.0_f32, p.border_subtle),
            );
            painter.vline(
                r2.right(),
                rect.y_range(),
                Stroke::new(1.0_f32, p.border_subtle),
            );

            // Left: Brush stroke sample
            let stroke_box = r1.shrink2(vec2(18.0, 32.0));
            brush_stroke_sample(painter, stroke_box, BrushStroke::Chisel, 14.0);
            painter.text(
                egui::pos2(r1.center().x, r1.top() + 16.0),
                egui::Align2::CENTER_CENTER,
                "Brush (B): Closed Vector Fill",
                font(TextStyle::Small),
                p.text_primary,
            );
            painter.text(
                egui::pos2(r1.center().x, r1.bottom() - 16.0),
                egui::Align2::CENTER_CENTER,
                "[ / ] or ⌥⌘+Drag = Resize",
                font(TextStyle::MonoSmall),
                p.text_secondary,
            );

            // Centre: Gradient Transform 5 handles
            let g_center = r2.center() + vec2(0.0, 6.0);
            painter.circle_filled(g_center, 42.0, p.accent_subtle);
            paint_gradient_transform(painter, ctx, g_center, 42.0, -0.35);
            painter.text(
                egui::pos2(r2.center().x, r2.top() + 16.0),
                egui::Align2::CENTER_CENTER,
                "Gradient Transform (F): 5 Handles",
                font(TextStyle::Small),
                p.text_primary,
            );

            // Right: 5 Paint Modes summary
            let modes = [
                "1. Paint Normal (Over all)",
                "2. Paint Fills (Under strokes)",
                "3. Paint Behind (Behind all)",
                "4. Paint Selection (Selected only)",
                "5. Paint Inside (Start fill only)",
            ];
            for (i, m) in modes.iter().enumerate() {
                painter.text(
                    egui::pos2(r3.left() + 14.0, r3.top() + 22.0 + i as f32 * 24.0),
                    egui::Align2::LEFT_CENTER,
                    *m,
                    font(TextStyle::Small),
                    p.text_primary,
                );
            }
        },
    );

    mode_card(
        ui,
        Some(i16::BRUSH),
        "Brush Tool (B) — All 5 Paint Modes, Lock Fill & Stage Zoom Invariant",
        "5 Paint Modes",
        BadgeType::Accent,
        "B / [ / ]",
        &[
            "1) Paint Normal: Paints over both lines and fills on the active layer.",
            "2) Paint Fills: Paints over existing fills and empty areas, leaving all vector strokes untouched.",
            "3) Paint Behind: Paints only in empty areas of the Stage behind all existing strokes and fills on the layer.",
            "4) Paint Selection: Restricts brush strokes strictly to currently selected fills, ignoring unselected regions and strokes.",
            "5) Paint Inside: Restricts the stroke to the single enclosed fill where the mouse button was first pressed (starting on empty Stage paints only over empty Stage).",
            "Constant Screen Size Invariant: Brush tip size stays constant in screen pixels when zooming the Stage—painting at 50% zoom produces a stroke twice as wide in Stage units as painting at 100% zoom.",
        ],
    );
    mode_card(
        ui,
        Some(i16::ERASER),
        "Eraser Tool (E) — 5 Erase Modes, Faucet Modifier & Double-Click Clear",
        "5 Erase Modes",
        BadgeType::Warning,
        "E",
        &[
            "5 Erase Modes: Erase Normal (strokes + fills), Erase Fills (fills only; strokes untouched), Erase Lines (strokes only; fills untouched), Erase Selected Fills (only pre-selected fills), and Erase Inside (only the fill where the drag began).",
            "Faucet Modifier: Single-click any stroke segment or enclosed fill to erase the entire connected segment or fill region at once.",
            "Double-Click Eraser Tool Icon: Immediately deletes all content on the Stage and Pasteboard in the current frame.",
        ],
    );
    mode_card(
        ui,
        Some(i16::BUCKET),
        "Paint Bucket (K), Ink Bottle (S), Eyedropper (I), Spray Brush & Deco (U)",
        "Fill & Procedural",
        BadgeType::Accent,
        "K / S / I / U",
        &[
            "Paint Bucket (K) Gap Size: Choose Don't Close Gaps, or Close Small / Medium / Large Gaps to bridge open contour gaps automatically; Lock Fill tiles a single shared gradient/bitmap matrix across multiple shapes.",
            "Eyedropper (I): Sampling a gradient fill copies the gradient into the active Fill well and automatically enables Lock Fill (`brush_lock_fill = true`).",
            "Deco Tool (U): Generates procedural vector patterns—Vine Fill (branching vines with leaves/flowers), Grid Fill (tiled symbols), and Symmetry Brush (Reflect across line/point, Rotate around, Grid translation), plus the 10 CS5 procedural brushes documented in the audit.",
        ],
    );
    mode_card(
        ui,
        Some(crate::ui_icons::Icon::Gradient.nf2(false)),
        "Gradient Transform Tool (F) & Color Mixer Overflow (Extend / Reflect / Repeat)",
        "Linear & Radial",
        BadgeType::Accent,
        "F / Shift+F9",
        &[
            "Linear Gradient Boundaries & Handles: Displays two parallel perpendicular boundary lines with a Center circle (moves origin), a Width square [→] on the right line (scales span width without rotating), and a Rotation circle at the top-right corner (Shift constrains to 45°).",
            "Radial Gradient Boundaries & Handles: Displays an elliptical boundary ring + horizontal diameter axis line with a Center circle, a Focal point inverted triangle ▽ on the axis, a Width square [→] (stretches X axis), a Size/Radius circle with inward arrow (scales XY proportionally), and a Rotation circle.",
            "Color Panel & Popover: Places Fill on the left and Stroke on the right, renders a visual Overflow dropdown (Extend, Reflect, Repeat) + Linear RGB toggle, and lets you click either the 20 px gradient bar or the 20 px pointer rail below it to insert and immediately edit color stops.",
        ],
    );
}

// ── Chapter 6: Symbols, 9-Slice & Library ──

fn chapter_symbols_9slice_and_library(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "Symbols (F8 to Convert to Symbol, ⌘F8 for New Symbol) are reusable master assets stored in the Library (⌘L). Placing a symbol on the Stage creates a lightweight Instance that can have its own transform, Colour Effect, Blend Mode, Filters, and Playback mode.",
    );

    section_overline(
        ui,
        "Interactive Diagram — Button 4-State Timeline & 9-Slice Scaling Grid",
    );
    illustration_canvas(
        ui,
        148.0,
        "Illustration 6.1: Left — Button Symbol's 4 dedicated interaction frames (Up, Over, Down, and invisible Hit area). Right — 9-Slice Scaling (scale9Grid) preserving corner geometry while stretching central regions.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let mid = rect.center().x;
            painter.vline(mid, rect.y_range(), Stroke::new(1.0_f32, p.border_subtle));

            // Left: Button 4 states (Up, Over, Down, Hit)
            let states = ["Up", "Over", "Down", "Hit"];
            let cell_w = 56.0;
            let start_x = (rect.left() + mid - cell_w * 4.0) * 0.5;
            let row_y = rect.center().y - 6.0;
            for (i, name) in states.iter().enumerate() {
                let c = Rect::from_min_size(
                    egui::pos2(start_x + i as f32 * cell_w, row_y),
                    vec2(cell_w, 32.0),
                );
                painter.rect_filled(
                    c,
                    0.0,
                    if i == 3 {
                        p.accent_subtle
                    } else {
                        p.timeline_static
                    },
                );
                painter.rect_stroke(
                    c,
                    0.0,
                    Stroke::new(1.0_f32, p.timeline_grid),
                    StrokeKind::Inside,
                );
                paint_marker(painter, ctx, c, MarkerKind::Keyframe, false);
                painter.text(
                    egui::pos2(c.center().x, c.top() - 12.0),
                    egui::Align2::CENTER_CENTER,
                    *name,
                    font(TextStyle::BodyStrong),
                    p.text_primary,
                );
            }
            painter.text(
                egui::pos2((rect.left() + mid) * 0.5, rect.bottom() - 18.0),
                egui::Align2::CENTER_CENTER,
                "Button Symbol: 'Hit' frame defines invisible click polygon",
                font(TextStyle::Small),
                p.text_secondary,
            );

            // Right: 9-Slice grid
            let box_r = Rect::from_center_size(
                egui::pos2((mid + rect.right()) * 0.5, rect.center().y - 2.0),
                vec2(170.0, 78.0),
            );
            painter.rect_filled(box_r, 10.0, p.accent_subtle);
            painter.rect_stroke(
                box_r,
                10.0,
                Stroke::new(1.5_f32, p.accent_default),
                StrokeKind::Inside,
            );
            let gx1 = box_r.left() + 28.0;
            let gx2 = box_r.right() - 28.0;
            let gy1 = box_r.top() + 22.0;
            let gy2 = box_r.bottom() - 22.0;
            let dash_s = Stroke::new(1.0_f32, p.guide);
            for x in [gx1, gx2] {
                painter.extend(egui::Shape::dashed_line(
                    &[
                        egui::pos2(x, box_r.top() - 8.0),
                        egui::pos2(x, box_r.bottom() + 8.0),
                    ],
                    dash_s,
                    4.0,
                    3.0,
                ));
            }
            for y in [gy1, gy2] {
                painter.extend(egui::Shape::dashed_line(
                    &[
                        egui::pos2(box_r.left() - 8.0, y),
                        egui::pos2(box_r.right() + 8.0, y),
                    ],
                    dash_s,
                    4.0,
                    3.0,
                ));
            }
            painter.text(
                box_r.center(),
                egui::Align2::CENTER_CENTER,
                "Scale X + Y",
                font(TextStyle::MonoSmall),
                p.text_primary,
            );
            painter.text(
                egui::pos2(box_r.center().x, rect.bottom() - 18.0),
                egui::Align2::CENTER_CENTER,
                "9-Slice Guides: 4 Corners Fixed · Edges 1-Axis · Centre 2-Axis",
                font(TextStyle::Small),
                p.text_secondary,
            );
        },
    );

    mode_card(
        ui,
        Some(i16::MOVIECLIP),
        "Movie Clip vs Button vs Graphic — Runtime & Scrubbing Behavior",
        "3 Symbol Types",
        BadgeType::Accent,
        "F8 / ⌘F8",
        &[
            "1) Movie Clip: Runs on its own independent clock at runtime (continues playing even if the main Timeline is stopped on frame 1). Supports Instance Name targeting in AS2/AS3/JS/TS, Display Filters, Blend Modes, 3D Transforms, and Cache as Bitmap.",
            "2) Button: Interactive 4-frame state machine (Up, Over, Down, Hit). Enable Control ▸ Enable Simple Buttons (⌥⌘B) to test button states directly on the authoring Stage.",
            "3) Graphic: Deterministically locked to the parent Timeline's playhead so multi-frame animations scrub live on the Stage. Offers 3 Looping modes in Properties: Loop, Play Once, and Single Frame (with interactive Frame Picker).",
        ],
    );
    mode_card(
        ui,
        Some(i16::COLORWHEEL),
        "Instance Colour Effects, Swap Symbol & Edit in Place",
        "Inspector",
        BadgeType::Accent,
        "Properties",
        &[
            "5 Colour Effect Styles: None, Brightness (-100% black to +100% white), Tint (RGB tint colour + 0–100% strength), Advanced (independent RGBA multiplier % and constant -255..+255 offset channels), and Alpha (0–100% opacity).",
            "Swap Symbol & Option-Drag Replacement: Click Swap… in Properties (or hold Option ⌥ while dragging a symbol from the Library onto an instance on Stage) to replace the symbol while preserving all transforms, filters, and keyframes.",
            "Edit in Place (Double-Click Instance on Stage): Edits the symbol inside its exact affine-transformed Stage position while dimming the surrounding parent scene context.",
        ],
    );
}

// ── Chapter 7: Timeline, Layers & Masks ──

fn chapter_timeline_layers_and_masks(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "The Timeline organizes content across depth (Layers and Layer Folders) and time (Frames, Keyframes, and Tween Spans). Every Timeline operation is transactional and preserves motion-span invariants.",
    );

    section_overline(
        ui,
        "Interactive Diagram — Authentic NF2 Timeline Spans & Keyframe Markers",
    );
    illustration_canvas(
        ui,
        142.0,
        "Illustration 7.1: Live NF2 Timeline rendering showing Static (grey), Motion Tween (blue + ◆ property keys), Classic Tween (purple + arrow), Shape Tween (green + arrow), and Sound waveform spans.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let row_h = 24.0;
            let label_w = 132.0;
            let origin_x = rect.left() + label_w;
            let geo = Geometry::new(origin_x, FrameView::Normal, false);
            let rows = [
                ("Actions / Labels", SpanType::Static, i16::LAYER),
                ("Motion Tween", SpanType::MotionTween, i16::TWEEN_MOTION),
                ("Classic Tween", SpanType::ClassicTween, i16::TWEEN_CLASSIC),
                ("Shape Morph", SpanType::ShapeTween, i16::TWEEN_SHAPE),
            ];
            for (idx, (name, span_kind, icon)) in rows.iter().enumerate() {
                let y = rect.top() + 10.0 + idx as f32 * row_h;
                let l_rect = Rect::from_min_size(
                    egui::pos2(rect.left() + 8.0, y),
                    vec2(label_w - 12.0, row_h),
                );
                let f_rect = Rect::from_min_max(
                    egui::pos2(origin_x, y),
                    egui::pos2(rect.right() - 10.0, y + row_h),
                );
                painter.rect_filled(l_rect, 0.0, p.bg_panel);
                icons::paint(
                    painter,
                    ctx,
                    Rect::from_min_size(l_rect.min + vec2(4.0, 4.0), vec2(16.0, 16.0)),
                    *icon,
                    p.text_secondary,
                );
                painter.text(
                    egui::pos2(l_rect.left() + 24.0, l_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    *name,
                    font(TextStyle::Small),
                    p.text_primary,
                );

                paint_grid(painter, ctx, f_rect, &geo, GridKind::Layer);
                let span_r = geo.span(f_rect, 1, 20);
                paint_span(painter, ctx, span_r, *span_kind, true);
                match idx {
                    0 => {
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 1),
                            MarkerKind::ActionKeyframe,
                            false,
                        );
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 12),
                            MarkerKind::BlankKeyframe,
                            false,
                        );
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 20),
                            MarkerKind::SpanEnd,
                            false,
                        );
                    }
                    1 => {
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 1),
                            MarkerKind::Keyframe,
                            false,
                        );
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 8),
                            MarkerKind::PropertyKeyframe,
                            false,
                        );
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 15),
                            MarkerKind::PropertyKeyframe,
                            false,
                        );
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 20),
                            MarkerKind::PropertyKeyframe,
                            false,
                        );
                    }
                    _ => {
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 1),
                            MarkerKind::Keyframe,
                            false,
                        );
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 20),
                            MarkerKind::Keyframe,
                            false,
                        );
                    }
                }
            }
        },
    );

    mode_card(
        ui,
        Some(i16::KEYFRAME_ADD),
        "Frame & Keyframe Shortcuts + Span-Based vs Frame-Based Selection",
        "Timeline Editing",
        BadgeType::Accent,
        "F5 / F6 / F7",
        &[
            "Insert Frame (F5) & Remove Frames (⇧F5): Extends or shortens the duration of a span without adding keyframes.",
            "Insert Keyframe (F6), Insert Blank Keyframe (F7) & Clear Keyframe (⇧F6): F6 duplicates the current pose into a new keyframe (or records a ◆ property keyframe inside a Motion Tween); F7 inserts an empty keyframe; ⇧F6 reverts a keyframe to a held frame.",
            "Span-Based Selection (Preferences ▸ Timeline): Clicking inside a span selects the entire keyframe-to-end sequence; Command-clicking (⌘) selects a single individual frame. Hold Option (⌥) while dragging a keyframe to duplicate it.",
        ],
    );
    mode_card(
        ui,
        Some(i16::MASK),
        "Layer Roles: Normal, Guide, Motion Guide, Mask, Masked & Folder",
        "6 Layer Roles",
        BadgeType::Accent,
        "Layer Properties",
        &[
            "Guide & Guided Layers: Guide layers are excluded from published output; linking a Normal layer beneath a Guide layer creates a Motion Guide for Classic Tweens.",
            "Mask & Masked Layers: Shapes/symbols on a Mask layer cut a visibility window for indented Masked layers beneath it (supports Geometry, Alpha, and Luminance masking plus Invert Mask).",
            "Quick Column Modifiers: Option-clicking (⌥) in the Eye (Visibility), Lock, or Outline column toggles all other layers except the clicked row; Solo isolates selected layers.",
        ],
    );
    mode_card(
        ui,
        Some(i16::ONION),
        "Onion Skinning, Edit Multiple Frames & Scenes",
        "Multi-Frame",
        BadgeType::Neutral,
        "Timeline Bar",
        &[
            "Onion Skin & Onion Skin Outlines: Displays translucent ghost frames (or coloured wireframes) before and after the playhead within the draggable Onion range markers.",
            "Edit Multiple Frames: Allows selecting, moving, and transforming artwork across all unlocked frames inside the Onion range simultaneously.",
            "Scenes (Window ▸ Panels ▸ Scene): Author separate scenes on independent timelines while the runtime and HTML5 exporter seamlessly flatten them in play order.",
        ],
    );
}

// ── Chapter 8: Tweens, Motion & IK ──

fn chapter_tweens_motion_and_ik(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "NeoFlash supports all four Flash animation paradigms: object-based Motion Tweens with ◆ property tracks, pose-to-pose Classic Tweens with Motion Guides, contour-morphing Shape Tweens with Shape Hints (a–z), and Inverse Kinematics (IK) armatures.",
    );

    section_overline(
        ui,
        "Interactive Diagram — Motion Path (◆ Keys & Handles), Shape Hints (a–z) & Ease Curve",
    );
    illustration_canvas(
        ui,
        152.0,
        "Illustration 8.1: Left — Stage Motion Path with per-frame dots, ◆ property keyframes, and Bézier handles. Centre — Shape Tween Hints (Unplaced red, Start yellow, End green). Right — Cubic-Bézier Ease Graph.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let w3 = rect.width() / 3.0;
            let r1 = Rect::from_min_max(rect.min, egui::pos2(rect.left() + w3, rect.bottom()));
            let r2 = Rect::from_min_max(
                egui::pos2(r1.right(), rect.top()),
                egui::pos2(r1.right() + w3, rect.bottom()),
            );
            let r3 = Rect::from_min_max(egui::pos2(r2.right(), rect.top()), rect.max);
            painter.vline(
                r1.right(),
                rect.y_range(),
                Stroke::new(1.0_f32, p.border_subtle),
            );
            painter.vline(
                r2.right(),
                rect.y_range(),
                Stroke::new(1.0_f32, p.border_subtle),
            );

            // Left: Motion Path
            let k0 = r1.min + vec2(32.0, 108.0);
            let k1 = r1.center() + vec2(0.0, -24.0);
            let k2 = egui::pos2(r1.right() - 32.0, r1.top() + 104.0);
            let frames: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let t = i as f32 / 16.0;
                    let u = 1.0 - t;
                    egui::pos2(
                        u * u * k0.x + 2.0 * u * t * k1.x + t * t * k2.x,
                        u * u * k0.y + 2.0 * u * t * k1.y + t * t * k2.y,
                    )
                })
                .collect();
            paint_motion_path(painter, ctx, &frames);
            let mid_pt = frames[8];
            paint_motion_handle(
                painter,
                ctx,
                mid_pt,
                mid_pt + vec2(-36.0, -14.0),
                MotionHandleState::Default,
            );
            paint_motion_handle(
                painter,
                ctx,
                mid_pt,
                mid_pt + vec2(36.0, 14.0),
                MotionHandleState::Hover,
            );
            paint_motion_key(painter, ctx, k0, MotionKeyState::Default);
            paint_motion_key(painter, ctx, mid_pt, MotionKeyState::Selected);
            paint_motion_key(painter, ctx, k2, MotionKeyState::Hover);
            painter.text(
                egui::pos2(r1.center().x, r1.top() + 16.0),
                egui::Align2::CENTER_CENTER,
                "Motion Path: ◆ Keys & Bézier Handles",
                font(TextStyle::Small),
                p.text_primary,
            );

            // Centre: Shape Hints (a, b, c)
            let sh_c = r2.center() + vec2(0.0, 8.0);
            let shape_box = Rect::from_center_size(sh_c, vec2(110.0, 62.0));
            painter.rect_filled(shape_box, 6.0, p.accent_subtle);
            painter.rect_stroke(
                shape_box,
                6.0,
                Stroke::new(1.5_f32, p.accent_default),
                StrokeKind::Inside,
            );
            paint_shape_hint(
                painter,
                ctx,
                shape_box.left_top(),
                'a',
                ShapeHintState::Start,
            );
            paint_shape_hint(
                painter,
                ctx,
                shape_box.right_top(),
                'b',
                ShapeHintState::End,
            );
            paint_shape_hint(
                painter,
                ctx,
                shape_box.right_bottom(),
                'c',
                ShapeHintState::Unplaced,
            );
            painter.text(
                egui::pos2(r2.center().x, r2.top() + 16.0),
                egui::Align2::CENTER_CENTER,
                "Shape Hints (⇧⌘H): Start · End · Unplaced",
                font(TextStyle::Small),
                p.text_primary,
            );

            // Right: Ease curve visualization
            let g_box = r3.shrink2(vec2(28.0, 26.0));
            painter.rect_filled(g_box, 4.0, p.bg_panel);
            painter.rect_stroke(
                g_box,
                4.0,
                Stroke::new(1.0_f32, p.border_subtle),
                StrokeKind::Inside,
            );
            let curve: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0;
                    let eased = t * t * (3.0 - 2.0 * t);
                    egui::pos2(
                        g_box.left() + t * g_box.width(),
                        g_box.bottom() - eased * g_box.height(),
                    )
                })
                .collect();
            painter.add(egui::Shape::line(
                curve,
                Stroke::new(2.0_f32, p.accent_default),
            ));
            painter.text(
                egui::pos2(r3.center().x, r3.top() + 14.0),
                egui::Align2::CENTER_CENTER,
                "Ease & Motion Editor Curves",
                font(TextStyle::Small),
                p.text_primary,
            );
        },
    );

    mode_card(
        ui,
        Some(i16::TWEEN_MOTION),
        "Motion Tween & Motion Editor — ◆ Property Keyframes & Roving Keys",
        "Property Tracks",
        BadgeType::Accent,
        "Insert ▸ Motion Tween",
        &[
            "Selective ◆ Property Keyframes: Moving or transforming an instance inside a Motion Span records a ◆ key only for the changed property (X, Y, ScaleX, ScaleY, Rotation, Skew, Alpha, Colour), leaving other tracks smooth.",
            "Interactive Stage Motion Path: Select the motion path with Selection (V) to bend segments directly, or with Subselection (A) to adjust individual ◆ keyframe handles. Enable Roving Keyframes for uniform speed across multi-segment paths.",
            "Motion Editor & Ease Panels: Inspect and edit individual property graphs, add custom cubic-Bézier ease curves (including overshoot |y| ≤ 100), or Copy / Paste Motion Special across instances.",
        ],
    );
    mode_card(
        ui,
        Some(i16::TWEEN_SHAPE),
        "Classic Tween (Motion Guide + Orient to Path) & Shape Tween (Shape Hints a–z)",
        "Classic & Morph",
        BadgeType::Accent,
        "⇧⌘H Add Hint",
        &[
            "Classic Tween: Interpolates between explicit keyframes (● → ●). Link the layer under a Guide layer and enable Orient to Path to steer the instance along the guide stroke tangent.",
            "Shape Tween & Shape Hints (Modify ▸ Shape ▸ Add Shape Hint, ⇧⌘H): Morphs raw vector shapes using arc-length resampling and paint blending. Place up to 26 lettered Shape Hints (a–z) counter-clockwise from the top-left corner (Yellow = start keyframe, Green = end keyframe, Red = unsnapped).",
        ],
    );
    mode_card(
        ui,
        Some(i16::ARMATURE),
        "Inverse Kinematics — Bone Tool (X) & Bind Tool (Z) Modes",
        "Armatures",
        BadgeType::Accent,
        "X / Z",
        &[
            "Symbol Armatures vs IK Shapes: Dragging with Bone (X) from one symbol instance to another chains them into a parent-child skeleton on a Pose Layer; dragging inside a selected vector shape creates a deformable IK Shape container.",
            "Posing Modifiers: Drag any bone with Selection (V) to solve inverse kinematics across the chain; hold Shift (⇧) while dragging a bone to rotate only that bone and its children without moving parent bones.",
            "Bind Tool (Z): Click a bone to highlight its bound contour points in yellow (squares = bound to 1 bone, triangles = bound to multiple bones); ⇧-click adds points, ⌥-click unbinds points.",
        ],
    );
}

// ── Chapter 9: Filters, Text & Audio ──

fn chapter_filters_text_and_audio(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "NeoFlash provides live GPU/software display filters, 14 Porter-Duff and Flash compositing blend modes, full typography with selective Font Embedding, and sample-accurate timeline audio with 8-handle stereo envelopes.",
    );

    section_overline(
        ui,
        "Interactive Diagram — Stereo Audio Waveform & 8-Handle Edit Envelope",
    );
    illustration_canvas(
        ui,
        128.0,
        "Illustration 9.1: Timeline Sound waveform (`SpanType::Sound`) with interactive Left/Right volume envelope handles and In/Out trim markers (`Properties ▸ Sound ▸ Envelope…`).",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let lane = rect.shrink2(vec2(24.0, 22.0));
            paint_span(painter, ctx, lane, SpanType::Sound, true);
            painter.rect_stroke(
                lane,
                0.0,
                Stroke::new(1.0_f32, p.border_default),
                StrokeKind::Inside,
            );

            // Envelope polyline + square handles
            let env = [
                egui::pos2(lane.left(), lane.bottom() - 8.0),
                egui::pos2(lane.left() + lane.width() * 0.15, lane.top() + 12.0),
                egui::pos2(lane.left() + lane.width() * 0.75, lane.top() + 16.0),
                egui::pos2(lane.right(), lane.bottom() - 6.0),
            ];
            painter.add(egui::Shape::line(
                env.to_vec(),
                Stroke::new(1.5_f32, p.accent_default),
            ));
            for pt in env {
                paint_anchor(painter, ctx, pt, AnchorState::Selected);
            }
        },
    );

    mode_card(
        ui,
        Some(i16::FILTER),
        "7 Live Filters & 14 Blend Modes (Instance & Layer Level)",
        "Compositing",
        BadgeType::Accent,
        "Filters Panel",
        &[
            "7 Live Filters: Drop Shadow, Blur, Glow, Bevel, Gradient Glow, Gradient Bevel, and Adjust Color (Brightness, Contrast, Saturation, Hue).",
            "14 Blend Modes: Normal, Layer, Darken, Multiply, Lighten, Screen, Overlay, Hard Light, Add, Subtract, Difference, Invert, Alpha, and Erase (available both on MovieClip/Button instances and directly on Timeline Layers).",
        ],
    );
    mode_card(
        ui,
        Some(i16::TEXT),
        "Text Engine (TLF CS5 / Static / Dynamic / Input), Multi-Column Flow & Font Embedding",
        "Typography",
        BadgeType::Accent,
        "T / Embedding…",
        &[
            "Point vs Area Text & Inline Editing: Click on Stage with Text (T) for auto-expanding Point Text (round top-right handle ○); drag for fixed-width word-wrapping Area Text (square handle □; double-click handle to revert to auto-width); double-click any TextBox with Selection (V) or Free Transform (Q) to edit text inline.",
            "Flash CS5 Character, Paragraph & Container Flow: Configure Highlight colour, Strikethrough, Underline, Superscript/Subscript, Leading (pt or %), Orientation (Horizontal, Vertical LTR/RTL), Advanced Character (Case, Digit Case/Width, Ligatures, Baseline Shift, Locale), 5-mode Paragraph alignment (including Justify All) with Space Before/After, and Container & Flow (1–50 Columns with Gutter px, 4-sided L/R/T/B Padding with Lock, Top/Center/Bottom/Justify Vertical Alignment, Border/Background slash-swatches, and 1st Line Offset).",
            "Font Embedding & 2-Step Break Apart (⌘B × 2): Select Unicode ranges or custom glyphs in Font Embedding… to embed outlines; press ⌘B once to split words into individual letter boxes and a second time to convert glyphs into editable vector paths.",
        ],
    );
    mode_card(
        ui,
        Some(i16::SOUND),
        "Timeline Sound Sync Modes (Event, Start, Stop, Stream) & Edit Envelope",
        "Audio Engine",
        BadgeType::Accent,
        "Properties ▸ Sound",
        &[
            "1) Event: Triggers when the playhead enters the keyframe and plays independently to completion (re-entering layers another overlapping voice).",
            "2) Start: Like Event, but never starts a duplicate instance if that sound is already playing.",
            "3) Stop: Immediately silences the specified sound resource.",
            "4) Stream: Locks audio playback frame-by-frame to the Timeline clock and plays short slices while scrubbing the playhead.",
            "Edit Envelope: Up to 8 volume control points per stereo channel with presets (Fade In, Fade Out, Fade Left to Right, etc.) and In/Out trim bars.",
        ],
    );
}

// ── Chapter 10: Scripting, Import & Publish ──

fn chapter_scripting_import_and_publish(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "NeoFlash supports four scripting languages in the same authoring environment—ActionScript 2.0, ActionScript 3.0, JavaScript, and TypeScript—and publishes self-contained HTML5 + WebAssembly bundles, texture atlases, SVG, Lottie, and video.",
    );

    section_overline(ui, "Scripting, Debugging & Code Snippets");
    mode_card(
        ui,
        Some(i16::CODE),
        "Actions (F9), Code Snippets, Compiler Errors (⌥F2), Output (F2) & Debugger",
        "4 Languages",
        BadgeType::Accent,
        "F9 / F2 / ⌥F2",
        &[
            "Frame & Object Scripts: Attach scripts to keyframes (indicated by the 'a' marker in the Timeline) or directly to MovieClip/Button instances (`on(release)`, `onClipEvent`, or AS3/JS/TS event listeners).",
            "Code Snippets Panel: Browse ready-made templates for Timeline Navigation, Actions, Events, Animation, and Audio—automatically adapted to your document's active script language.",
            "Interactive Debugger & Bandwidth Profiler: Set line breakpoints, inspect the Call Stack, Locals, and Watch expressions, step Into/Over/Out, and inspect per-frame byte budgets in Test Movie (⌘Return).",
        ],
    );

    section_overline(
        ui,
        "Import Pipeline & Native HTML5 / Sprite Sheet Publishing",
    );
    mode_card(
        ui,
        Some(i16::PUBLISH),
        "Legacy FLA / XFL / SWF / PSD / AI / SVG Import & Export Targets",
        "Interchange",
        BadgeType::Accent,
        "⌘R / ⇧F12",
        &[
            "Comprehensive Import (⌘R / ⌘O): Opens binary `.fla` (Flash 3–CS4), uncompressed/zipped `.xfl`/`.fla` (CS5–Animate), compiled `.swf` (FWS, Deflate CWS, LZMA ZWS), layered Photoshop `.psd`/`.psb`, `.ai`/`.svg`/`.pdf` vectors, `.act`/`.clr` swatch tables, audio (`.wav`/`.mp3`/`.ogg`/`.flac`), video (`.mp4`/`.webm`), and numbered image sequences.",
            "HTML5 + WASM Publishing (⇧F12): Generates a zero-plugin browser bundle (`index.html`, `player.js`, `audio.js`, `neoflash_web_bg.wasm`, `project.json`) with full vector, filter, blend, sound, and script fidelity.",
            "Sprite Sheet Generator, SVG, Lottie & Video: Export packed texture atlases (`Shelf` / `MaxRects` with `Starling XML`, `CreateJS`, or `JSON-Array` metadata, trim & frame deduplication), animated SVG, Lottie JSON (⌥⇧⌘L), WAV audio (⌥⇧⌘A), and MP4/WebM/GIF sequences (⌥⇧⌘V).",
        ],
    );

    section_overline(ui, "Essential Keyboard Shortcuts Quick Reference");
    sub_bullet(
        ui,
        "Selection / Subselection / Free Transform / Gradient Transform",
        Some("V / A / Q / F"),
        "Primary selection, node editing, bounding-box/envelope transform, and gradient/bitmap matrix handles.",
    );
    sub_bullet(
        ui,
        "Pen / Add / Delete / Convert Anchor Point",
        Some("P / = / - / ⇧C"),
        "Bézier curve construction and non-destructive node editing.",
    );
    sub_bullet(
        ui,
        "Text / Line / Rectangle / Oval / Pencil / Brush",
        Some("T / N / R / O / Y / B"),
        "Core typography, geometric primitives, and freehand vector tools.",
    );
    sub_bullet(
        ui,
        "Deco / Bone / Bind / Paint Bucket / Ink Bottle / Eyedropper / Eraser",
        Some("U / X / Z / K / S / I / E"),
        "Procedural patterns, inverse kinematics, fill/stroke styling, and erasing.",
    );
    sub_bullet(
        ui,
        "Object Drawing Toggle / Hand / Zoom",
        Some("J / H / Z"),
        "Toggle isolated Object Drawing mode, pan stage (or hold Space), and zoom.",
    );
    sub_bullet(
        ui,
        "Timeline Frames & Keyframes",
        Some("F5 / ⇧F5 / F6 / F7 / ⇧F6"),
        "Insert frame, remove frame, insert keyframe, insert blank keyframe, clear keyframe.",
    );
    sub_bullet(
        ui,
        "Convert to Symbol / New Symbol / Break Apart / Group",
        Some("F8 / ⌘F8 / ⌘B / ⌘G"),
        "Symbol packaging, ungrouping, and 2-step text-to-outlines conversion.",
    );
    sub_bullet(
        ui,
        "Test Movie / Publish / Actions / Library / Properties",
        Some("⌘Return / ⇧F12 / F9 / ⌘L / ⌘F3"),
        "Preview runtime, publish bundle, and toggle core authoring panels.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Editor;
    use neoflash_core::Project;

    #[test]
    fn help_manual_renders_all_chapters_diagrams_and_search_without_mutating_document() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        let before = serde_json::to_value(&editor.project).unwrap();

        assert!(!is_help_manual_open(&ctx));
        open_help_manual(&ctx);
        assert!(is_help_manual_open(&ctx));

        for page in 0..HELP_CHAPTERS.len() {
            set_help_manual_page(&ctx, page);
            for tab in 0..3 {
                ctx.data_mut(|d| {
                    d.get_temp_mut_or_default::<HelpManualState>(state_id())
                        .diagram_tab = tab;
                });
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    editor.help_manual_dialog(ctx);
                });
            }
        }

        // Filtering never moves the article underneath the reader. The user selects a
        // matching chapter explicitly from the stable sidebar.
        ctx.data_mut(|d| {
            let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
            s.page = 0;
            s.query = "armature".to_string();
        });
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            editor.help_manual_dialog(ctx);
        });
        let matched_page = ctx.data(|d| {
            d.get_temp::<HelpManualState>(state_id())
                .map(|s| s.page)
                .unwrap_or(0)
        });
        assert_eq!(HELP_CHAPTERS[matched_page].title, "1. Workspace & Stage");

        close_help_manual(&ctx);
        assert!(!is_help_manual_open(&ctx));
        assert_eq!(before, serde_json::to_value(&editor.project).unwrap());
    }

    #[test]
    fn browser_help_is_a_complete_escaped_text_edition() {
        let html = help_browser_html();
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("NeoFlash Help &amp; Reference"));
        for (chapter, topics) in HELP_CHAPTERS.iter().zip(HELP_BROWSER_TOPICS) {
            let mut escaped_title = String::new();
            escape_html(chapter.title, &mut escaped_title);
            assert!(html.contains(&escaped_title));
            for topic in topics {
                let mut escaped_topic = String::new();
                escape_html(topic, &mut escaped_topic);
                assert!(html.contains(&escaped_topic));
            }
        }
        assert!(html.contains("HTML5 plus WebAssembly"));
        assert!(!html.contains("<script"));
    }

    #[test]
    fn help_modal_bounds_stay_fixed_across_search_and_chapters() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        let input = || egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 900.0))),
            ..Default::default()
        };
        let render = |editor: &mut Editor, ctx: &egui::Context| {
            let _ = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
            let _ = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
            ctx.memory(|memory| {
                memory
                    .area_rect(egui::Id::new("neoflash-help-manual-modal"))
                    .expect("Help modal area")
            })
        };

        open_help_manual(&ctx);
        let initial = render(&mut editor, &ctx);
        ctx.data_mut(|data| {
            let state = data.get_temp_mut_or_default::<HelpManualState>(state_id());
            state.query = "armature".into();
        });
        let filtered = render(&mut editor, &ctx);
        set_help_manual_page(&ctx, HELP_CHAPTERS.len() - 1);
        let last_chapter = render(&mut editor, &ctx);

        assert_eq!(initial, filtered);
        assert_eq!(initial, last_chapter);
    }

    #[test]
    fn help_modal_fits_the_minimum_window_at_supported_ui_scales() {
        for zoom in [1.0_f32, 1.25, 1.5] {
            let ctx = egui::Context::default();
            ctx.set_zoom_factor(zoom);
            // RawInput is expressed in native viewport points. `zoom_factor` then reduces
            // the logical content rectangle available to widgets, matching a scaled app.
            let screen = Rect::from_min_size(Pos2::ZERO, vec2(1024.0, 720.0));
            let input = || egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            // In the real app scale is established before Help opens. Prime a frame so the
            // synthetic context has the same viewport transform before modal geometry is saved.
            let _ = ctx.run(input(), |_| {});
            let mut editor = Editor::from_project(&ctx, Project::demo());
            open_help_manual(&ctx);
            let _ = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
            let _ = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
            let bounds = ctx.memory(|memory| {
                memory
                    .area_rect(egui::Id::new("neoflash-help-manual-modal"))
                    .expect("Help modal area")
            });
            assert!(
                screen.contains_rect(bounds),
                "Help must fit 1024×720 at {zoom}×: {bounds:?} within {screen:?}"
            );

            ctx.data_mut(|data| {
                let state = data.get_temp_mut_or_default::<HelpManualState>(state_id());
                state.query = "publish".into();
                state.page = HELP_CHAPTERS.len() - 1;
            });
            let _ = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
            let after = ctx.memory(|memory| {
                memory
                    .area_rect(egui::Id::new("neoflash-help-manual-modal"))
                    .expect("Help modal area after search")
            });
            assert_eq!(
                bounds, after,
                "search/chapter must not move Help at {zoom}×"
            );
        }
    }

    #[test]
    fn help_controls_do_not_overlap_at_minimum_window_and_high_scale() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        ctx.set_zoom_factor(1.5);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1024.0, 720.0));
        let input = || egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let _ = ctx.run(input(), |_| {});
        let mut editor = Editor::from_project(&ctx, Project::demo());
        open_help_manual(&ctx);
        ctx.data_mut(|data| {
            let state = data.get_temp_mut_or_default::<HelpManualState>(state_id());
            state.query = "publish".into();
            state.page = 4;
        });
        let _ = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
        let output = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
        let nodes = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("Help accessibility tree")
            .nodes;
        let bounds = |label: &str| {
            nodes
                .iter()
                .filter_map(|(_, node)| {
                    (node.label() == Some(label))
                        .then(|| node.bounds())
                        .flatten()
                })
                .collect::<Vec<_>>()
        };
        let search = bounds("Filter topics, tools, modes or shortcuts…")[0];
        let clear = bounds("Clear")[0];
        let previous = bounds("Previous")[0];
        let next = bounds("Next topic")[0];
        let command = bounds("Command Search (F1)")[0];
        let browser = bounds("Open in browser")[0];
        let footer_close = bounds("Close")
            .into_iter()
            .find(|rect| rect.y0 > 600.0)
            .expect("footer Close");
        for rect in [
            search,
            clear,
            previous,
            next,
            footer_close,
            command,
            browser,
        ] {
            assert!(rect.x0 >= 0.0 && rect.x1 <= 1024.0);
            assert!(rect.y0 >= 0.0 && rect.y1 <= 720.0);
        }
        assert!(search.x1 <= clear.x0);
        assert!(previous.x1 <= next.x0);
        assert!(footer_close.x1 <= command.x0 && command.x1 <= browser.x0);
    }
}
