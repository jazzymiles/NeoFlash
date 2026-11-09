//! Comprehensive ActionScript 1.0, 2.0 & 3.0 Language & API Reference chapters
//! (`Chapters 11–16` of `Help ▸ NeoFlash Help & Reference…`), interactive cross-links,
//! tag filters, and the full NF2-styled + SVG-illustrated HTML5 browser manual generator.
//!
//! Built strictly from `neoflash_ui` (NF2) tokens, typography, icons, and components.

use super::help_manual::{
    HELP_BROWSER_TOPICS, HELP_CHAPTERS, illustration_canvas, mode_card, paragraph,
    section_overline, sub_bullet,
};
use eframe::egui::{self, Pos2, Rect, Stroke, StrokeKind, Ui, vec2};
use neoflash_ui::{
    Palette, TextStyle,
    components::{self as c, Badge, BadgeType, Button},
    icons::{self, i16},
    tokens::{DARK, LIGHT, radius, space},
    typography::{font, rich},
};
use std::fmt::Write as _;

/// Formats an `egui::Color32` as a CSS `rgb(...)` or `rgba(...)` functional color string.
/// Uses decimal channel notation so no literal hex strings exist in source.
pub(crate) fn css_rgb(color: egui::Color32) -> String {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    if a == 255 {
        format!("rgb({r}, {g}, {b})")
    } else {
        format!("rgba({r}, {g}, {b}, {:.3})", f32::from(a) / 255.0)
    }
}

/// All filterable topic tags across the 16 Help & Reference chapters.
pub(crate) const HELP_ALL_TAGS: &[&str] = &[
    "Authoring",
    "Stage",
    "Vector",
    "Tools",
    "Symbols",
    "Timeline",
    "Animation",
    "Text",
    "Audio",
    "Export",
    "AS1",
    "AS2",
    "AS3",
    "JS/TS",
    "DisplayList",
    "Events",
    "Graphics",
];

/// A copyable, syntax-tinted code block for ActionScript 1.0 / 2.0 / 3.0 and JS/TS examples.
pub(crate) struct CodeSample {
    pub title: &'static str,
    pub lang_badge: &'static str,
    pub code: &'static str,
}

pub(crate) fn code_block(ui: &mut Ui, sample: &CodeSample) {
    let p = neoflash_ui::palette(ui.ctx());
    let r = egui::CornerRadius::same(radius::MD as u8);
    egui::Frame::new()
        .fill(p.bg_field)
        .stroke(Stroke::new(1.0_f32, p.border_subtle))
        .corner_radius(r)
        .inner_margin(egui::Margin::same(space::S3 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                icons::show(ui, i16::CODE, p.accent_default);
                ui.label(rich(sample.title, TextStyle::BodyStrong).color(p.text_primary));
                ui.add(Badge::new(sample.lang_badge, BadgeType::Accent));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(Button::ghost("Copy snippet")).clicked() {
                        ui.ctx().copy_text(sample.code.to_owned());
                    }
                });
            });
            ui.add_space(space::S1);
            c::divider(ui, false);
            ui.add_space(space::S1);
            for (line_idx, line) in sample.code.lines().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S2;
                    let ln = format!("{:>2}", line_idx + 1);
                    ui.label(rich(&ln, TextStyle::MonoSmall).color(p.text_disabled));
                    let trimmed = line.trim_start();
                    let line_color = if trimmed.starts_with("//") || trimmed.starts_with("/*") {
                        p.syntax_comment
                    } else if trimmed.starts_with("package")
                        || trimmed.starts_with("import ")
                        || trimmed.starts_with("public class")
                        || trimmed.starts_with("class ")
                        || trimmed.starts_with("function ")
                        || trimmed.starts_with("on(")
                        || trimmed.starts_with("onClipEvent")
                    {
                        p.syntax_keyword
                    } else if trimmed.starts_with("var ")
                        || trimmed.starts_with("const ")
                        || trimmed.starts_with("let ")
                    {
                        p.syntax_type
                    } else {
                        p.text_primary
                    };
                    ui.label(rich(line, TextStyle::MonoSmall).color(line_color));
                });
            }
        });
    ui.add_space(space::S2);
}

/// Renders a structured 3-column API quick-reference table using NF2 tokens.
pub(crate) fn api_signature_table(
    ui: &mut Ui,
    title: &str,
    headers: [&str; 3],
    rows: &[(&str, &str, &str)],
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
                icons::show(ui, i16::BOOKMARK, p.accent_default);
                ui.label(rich(title, TextStyle::BodyStrong).color(p.text_primary));
            });
            ui.add_space(space::S1);
            let avail = ui.available_width();
            let col0 = (avail * 0.28).max(110.0);
            let col1 = (avail * 0.28).max(110.0);
            let col2 = (avail - col0 - col1 - space::S3 * 2.0).max(120.0);
            egui::Grid::new(egui::Id::new(("help-api-table", title)))
                .num_columns(3)
                .spacing(vec2(space::S2, space::S1))
                .striped(true)
                .show(ui, |ui| {
                    ui.label(rich(headers[0], TextStyle::SmallStrong).color(p.accent_default));
                    ui.label(rich(headers[1], TextStyle::SmallStrong).color(p.accent_default));
                    ui.label(rich(headers[2], TextStyle::SmallStrong).color(p.accent_default));
                    ui.end_row();
                    for (sig, ret_or_target, desc) in rows {
                        ui.add_sized(
                            vec2(col0, 18.0),
                            egui::Label::new(
                                rich(*sig, TextStyle::MonoSmall).color(p.syntax_function),
                            )
                            .wrap(),
                        );
                        ui.add_sized(
                            vec2(col1, 18.0),
                            egui::Label::new(
                                rich(*ret_or_target, TextStyle::MonoSmall).color(p.syntax_type),
                            )
                            .wrap(),
                        );
                        ui.add_sized(
                            vec2(col2, 18.0),
                            egui::Label::new(rich(*desc, TextStyle::Small).color(p.text_secondary))
                                .wrap(),
                        );
                        ui.end_row();
                    }
                });
        });
    ui.add_space(space::S2);
}

// ── Chapter 11: AS1 & AS2 Fundamentals & Timeline ──

pub(crate) fn chapter_as1_as2_fundamentals(
    ui: &mut Ui,
    diagram_tab: &mut usize,
    nav_target: &mut Option<usize>,
) {
    paragraph(
        ui,
        "ActionScript 1.0 (Flash 5–MX) and ActionScript 2.0 (Flash MX 2004–CS6) run on the AVM1 execution model. Scripts can be placed directly on Timeline keyframes, on Button instances via on(...), on MovieClip instances via onClipEvent(...), or organized into external .as classes linked to Library symbols.",
    );

    let tabs = [
        (0, "Timeline Scope (_root / _parent / this)"),
        (1, "AVM1 Depth Zones & Prototype Chain"),
    ];
    c::Segmented::show(ui, diagram_tab, &tabs, false);
    ui.add_space(space::S2);

    if *diagram_tab == 0 {
        illustration_canvas(
            ui,
            162.0,
            "Illustration 11.1: AVM1 Target Path Hierarchy (_level0 / _root → parent MovieClip → nested child MovieClip) and Timeline vs Instance script attachment points.",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let y_mid = rect.center().y + 4.0;
                let boxes = [
                    (
                        rect.left() + 18.0,
                        "_level0 / _root",
                        "Main Timeline",
                        "stop(); gotoAndPlay(\"intro\");",
                    ),
                    (
                        rect.left() + rect.width() * 0.36,
                        "_root.hero_mc",
                        "Parent MovieClip",
                        "this.onEnterFrame = step;",
                    ),
                    (
                        rect.left() + rect.width() * 0.70,
                        "hero_mc.blade_mc",
                        "Nested Child Clip",
                        "_parent._x += 4; _root.score++;",
                    ),
                ];
                let bw = (rect.width() * 0.27).clamp(130.0, 210.0);
                let bh = 88.0;
                let mut centers = [Pos2::ZERO; 3];
                for (i, (x, title, sub, code_line)) in boxes.iter().enumerate() {
                    let b = Rect::from_min_size(egui::pos2(*x, y_mid - bh * 0.5), vec2(bw, bh));
                    centers[i] = b.center();
                    painter.rect_filled(b, 6.0, p.bg_panel);
                    painter.rect_stroke(
                        b,
                        6.0,
                        Stroke::new(
                            1.5_f32,
                            if i == 1 {
                                p.accent_default
                            } else {
                                p.border_default
                            },
                        ),
                        StrokeKind::Inside,
                    );
                    painter.text(
                        egui::pos2(b.left() + 10.0, b.top() + 18.0),
                        egui::Align2::LEFT_CENTER,
                        *title,
                        font(TextStyle::MonoSmall),
                        p.accent_default,
                    );
                    painter.text(
                        egui::pos2(b.left() + 10.0, b.top() + 36.0),
                        egui::Align2::LEFT_CENTER,
                        *sub,
                        font(TextStyle::BodyStrong),
                        p.text_primary,
                    );
                    painter.text(
                        egui::pos2(b.left() + 10.0, b.top() + 62.0),
                        egui::Align2::LEFT_CENTER,
                        *code_line,
                        font(TextStyle::MonoSmall),
                        p.syntax_function,
                    );
                }
                for i in 0..2 {
                    let start = egui::pos2(boxes[i].0 + bw + 4.0, y_mid - 10.0);
                    let end = egui::pos2(boxes[i + 1].0 - 4.0, y_mid - 10.0);
                    painter.line_segment([start, end], Stroke::new(1.5_f32, p.accent_default));
                    let rev_s = egui::pos2(boxes[i + 1].0 - 4.0, y_mid + 14.0);
                    let rev_e = egui::pos2(boxes[i].0 + bw + 4.0, y_mid + 14.0);
                    painter.line_segment([rev_s, rev_e], Stroke::new(1.2_f32, p.warning_default));
                    painter.text(
                        egui::pos2((start.x + end.x) * 0.5, y_mid + 26.0),
                        egui::Align2::CENTER_CENTER,
                        "_parent",
                        font(TextStyle::MonoSmall),
                        p.warning_default,
                    );
                }
                painter.text(
                    egui::pos2(rect.center().x, rect.top() + 14.0),
                    egui::Align2::CENTER_CENTER,
                    "AVM1 Target Paths: Dot Syntax (_root.hero_mc.blade_mc) & Relative Scope (this / _parent)",
                    font(TextStyle::Small),
                    p.text_primary,
                );
            },
        );
    } else {
        illustration_canvas(
            ui,
            162.0,
            "Illustration 11.2: Flash Player AVM1 Depth Zones: Timeline Author-Time Zone (-16383..-1), Dynamic Script Zone (0..1048575), and Reserved Zone (1048576..2130690045).",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let zones = [
                    (
                        "1. Timeline Zone (-16383 to -1)",
                        "Author-time instances placed on layers",
                        "swapDepths() moves to Dynamic Zone before removeMovieClip()",
                        p.timeline_classic_tween,
                    ),
                    (
                        "2. Dynamic Zone (0 to 1,048,575)",
                        "attachMovie / duplicateMovieClip / createEmptyMovieClip",
                        "getNextHighestDepth() returns next free slot >= 0",
                        p.timeline_motion_tween,
                    ),
                    (
                        "3. Reserved Zone (1,048,576+)",
                        "V2 Components & PopUpManager depth slots",
                        "removeMovieClip() ignores depths >= 1,048,576",
                        p.bg_panel,
                    ),
                ];
                let pad = 12.0;
                let h = 36.0;
                for (i, (title, sub, note, fill_c)) in zones.iter().enumerate() {
                    let y = rect.top() + 26.0 + i as f32 * (h + 8.0);
                    let row = Rect::from_min_max(
                        egui::pos2(rect.left() + pad, y),
                        egui::pos2(rect.right() - pad, y + h),
                    );
                    painter.rect_filled(row, 4.0, *fill_c);
                    painter.rect_stroke(
                        row,
                        4.0,
                        Stroke::new(1.0_f32, p.border_default),
                        StrokeKind::Inside,
                    );
                    painter.text(
                        egui::pos2(row.left() + 10.0, row.center().y - 6.0),
                        egui::Align2::LEFT_CENTER,
                        *title,
                        font(TextStyle::BodyStrong),
                        p.text_primary,
                    );
                    painter.text(
                        egui::pos2(row.left() + 10.0, row.center().y + 8.0),
                        egui::Align2::LEFT_CENTER,
                        *sub,
                        font(TextStyle::Small),
                        p.text_secondary,
                    );
                    painter.text(
                        egui::pos2(row.right() - 10.0, row.center().y),
                        egui::Align2::RIGHT_CENTER,
                        *note,
                        font(TextStyle::MonoSmall),
                        p.text_primary,
                    );
                }
            },
        );
    }

    section_overline(ui, "Global Timeline Navigation & Frame Control");
    api_signature_table(
        ui,
        "Core AS1 / AS2 Timeline Functions & MovieClip Methods",
        [
            "Function / Method",
            "Target / Parameters",
            "Runtime Behavior",
        ],
        &[
            (
                "stop()",
                "Current Timeline / mc.stop()",
                "Halts playhead advancement on the active timeline while nested child MovieClips continue running.",
            ),
            (
                "play()",
                "Current Timeline / mc.play()",
                "Resumes sequential frame playback at the document frame rate (fps).",
            ),
            (
                "gotoAndPlay(frame)",
                "1-based frame index or \"label\"",
                "Jumps playhead to frame number, frame label, or (\"Scene\", frame) and continues playing.",
            ),
            (
                "gotoAndStop(frame)",
                "1-based frame index or \"label\"",
                "Moves playhead to the target frame or label and stops immediately.",
            ),
            (
                "nextFrame() / prevFrame()",
                "Current Timeline / mc",
                "Steps 1 frame forward or backward and implicitly stops the playhead.",
            ),
            (
                "stopAllSounds()",
                "Global Audio Mixer",
                "Silences all currently playing Event and Stream sounds across the player.",
            ),
        ],
    );

    mode_card(
        ui,
        Some(i16::CODE),
        "Button on(...) Handlers vs MovieClip onClipEvent(...) vs Dynamic Callbacks",
        "3 Event Styles",
        BadgeType::Accent,
        "F9 Actions",
        &[
            "1) Button Instance Scripts — on(press, release, releaseOutside, rollOver, rollOut, dragOver, dragOut, keyPress \"<Space>\"): Attached directly to a Button symbol instance on Stage; `this` inside a Button `on(release)` block resolves to the parent Timeline containing the button!",
            "2) MovieClip Instance Scripts — onClipEvent(load, unload, enterFrame, mouseDown, mouseUp, mouseMove, keyDown, keyUp, data): Attached to a MovieClip instance on Stage; `this` resolves to that MovieClip instance itself.",
            "3) Frame Callback Properties (Preferred AS2 Pattern): Assign `mc.onRelease = function() { ... };` or `mc.onEnterFrame = function() { ... };` inside a timeline keyframe on an `Actions` layer so all code stays centralized and searchable.",
        ],
    );

    mode_card(
        ui,
        Some(i16::LAYERS),
        "Dynamic MovieClip Instantiation, Depths & Prototype Inheritance",
        "AVM1 Object Model",
        BadgeType::Accent,
        "Linkage ID",
        &[
            "attachMovie(idName, newName, depth, initObject): Instantiates a Library symbol exported for ActionScript (`Linkage Identifier`) as a child of the target MovieClip at `depth` (commonly `this.getNextHighestDepth()`).",
            "createEmptyMovieClip(name, depth) & duplicateMovieClip(newName, depth, initObj): Creates an empty container clip for runtime vector drawing (`lineStyle`/`beginFill`) or clones an existing stage clip.",
            "removeMovieClip(): Removes a clip at positive depth `0..1048575`. To remove an author-time timeline clip (negative depth), first call `mc.swapDepths(this.getNextHighestDepth())`.",
            "AS1 Prototype vs AS2 Class Linkage: In AS1, register a constructor with `Object.registerClass(\"EnemySymbol\", Enemy)` and inherit via `Enemy.prototype = new MovieClip()`. In AS2, declare `class com.game.Enemy extends MovieClip` and set the symbol's AS2 Class in Library Properties.",
        ],
    );

    mode_card(
        ui,
        Some(i16::BOOKMARK),
        "ActionScript 1.0 (Flash 4 / Flash 5) Slash Paths, Legacy Operators & Directives",
        "AS1 Legacy",
        BadgeType::Warning,
        "Flash 4–6",
        &[
            "Slash & Colon Target Paths: `/hero/arm:angle` addresses variable `angle` inside `_root.hero.arm`; `../:score` addresses `score` on `_parent`; `tellTarget(\"/hero\") { gotoAndPlay(2); }` switches the implicit timeline target.",
            "getProperty & setProperty Constants: `getProperty(target, prop)` and `setProperty(target, prop, value)` accept all 19 classic property tokens (`_x`, `_y`, `_xscale`, `_yscale`, `_currentframe`, `_totalframes`, `_alpha`, `_visible`, `_width`, `_height`, `_rotation`, `_target`, `_framesloaded`, `_name`, `_droptarget`, `_url`, `_highquality`, `_focusrect`, `_soundbuftime`).",
            "Flash 4 String & Logical Operators: String comparison (`eq`, `ne`, `lt`, `gt`, `le`, `ge`), string concatenation (`add`), logical operators (`and`, `or`, `not`, `<>`), and byte/multibyte string functions (`length()`, `substring()`, `chr()`, `ord()`, `mbchr()`, `mbord()`, `mblength()`, `mbsubstring()`).",
            "Pre-Class Component Directives: `#include \"path/file.as\"` splices external script text at compile time; `#initclip [order]` ... `#endinitclip` executes component registration once on the frame before symbol construction.",
        ],
    );

    code_block(
        ui,
        &CodeSample {
            title: "AS2 Frame Script — Spawning Library Symbols, EnterFrame Loop & Hit Testing",
            lang_badge: "ActionScript 2.0",
            code: "// Frame 1 on 'Actions' layer (1-based timeline indexing)
stop();
var score:Number = 0;
var gravity:Number = 0.65;

// Spawn a linked Library symbol ('CoinSymbol') at the next available positive depth
var depth:Number = this.getNextHighestDepth();
var coin:MovieClip = this.attachMovie(\"CoinSymbol\", \"coin_\" + depth, depth, {_x: 320, _y: 180});

hero_mc.vy = 0;
hero_mc.onEnterFrame = function():Void {
    this.vy += _root.gravity;
    this._y += this.vy;
    if (Key.isDown(Key.RIGHT)) { this._x += 5; this._xscale = 100; }
    if (Key.isDown(Key.LEFT))  { this._x -= 5; this._xscale = -100; }
    // Bounding-box or shape-flag (true) collision test
    if (this.hitTest(_root.coin._x, _root.coin._y, true)) {
        _root.score += 100;
        _root.score_txt.text = \"Score: \" + _root.score;
        _root.coin.removeMovieClip();
    }
};",
        },
    );

    super::help_manual::inline_cross_links(
        ui,
        nav_target,
        &[
            (6, "7. Timeline, Layers & Masks"),
            (11, "12. AS2 Core Classes & Built-Ins"),
            (15, "16. AS1/AS2 ↔ AS3 ↔ JS/TS Migration Guide"),
        ],
    );
}

// ── Chapter 12: AS2 Core Classes & Built-Ins ──

pub(crate) fn chapter_as2_core_classes(
    ui: &mut Ui,
    _diagram_tab: &mut usize,
    nav_target: &mut Option<usize>,
) {
    paragraph(
        ui,
        "Flash MX 2004 and Flash 8 expanded ActionScript 2.0 with a rich standard library spanning display control, dynamic typography, procedural vector drawing, pixel-level BitmapData manipulation, 2D affine matrices, audio mixing, and XML/LoadVars networking.",
    );

    section_overline(
        ui,
        "Interactive Diagram — ActionScript 2.0 Built-In Class & Package Architecture",
    );
    illustration_canvas(
        ui,
        156.0,
        "Illustration 12.1: The 4 pillars of the ActionScript 2.0 runtime library in NeoFlash: Display & Stage, flash.geom / BitmapData (Flash 8), Input & Sound, and Data / Persistence.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let cols = [
                (
                    "Display & Text",
                    &[
                        "MovieClip (Drawing API)",
                        "Button (onRelease/enabled)",
                        "TextField & TextFormat",
                        "Stage (scaleMode / align)",
                        "Color (setRGB / setTransform)",
                    ],
                ),
                (
                    "flash.geom & BitmapData",
                    &[
                        "BitmapData (draw / perlin)",
                        "Matrix (a,b,c,d,tx,ty)",
                        "Point (length / interpolate)",
                        "Rectangle (union / intersects)",
                        "ColorTransform & Filters",
                    ],
                ),
                (
                    "Input & Audio",
                    &[
                        "Key (isDown / addListener)",
                        "Mouse (hide / onMouseWheel)",
                        "Sound (attach / loadSound)",
                        "Selection (setFocus / Caret)",
                        "System.capabilities",
                    ],
                ),
                (
                    "Data, Net & Timers",
                    &[
                        "LoadVars (sendAndLoad)",
                        "XML & XMLNode",
                        "MovieClipLoader (listeners)",
                        "SharedObject.getLocal()",
                        "setInterval / clearInterval",
                    ],
                ),
            ];
            let pad = 10.0;
            let cw = (rect.width() - pad * 5.0) / 4.0;
            let ch = rect.height() - 20.0;
            for (i, (heading, items)) in cols.iter().enumerate() {
                let x = rect.left() + pad + i as f32 * (cw + pad);
                let card = Rect::from_min_size(egui::pos2(x, rect.top() + 10.0), vec2(cw, ch));
                painter.rect_filled(card, 5.0, p.bg_panel);
                painter.rect_stroke(
                    card,
                    5.0,
                    Stroke::new(1.0_f32, p.border_subtle),
                    StrokeKind::Inside,
                );
                painter.text(
                    egui::pos2(card.left() + 8.0, card.top() + 14.0),
                    egui::Align2::LEFT_CENTER,
                    *heading,
                    font(TextStyle::BodyStrong),
                    p.accent_default,
                );
                for (j, item) in items.iter().enumerate() {
                    painter.text(
                        egui::pos2(card.left() + 8.0, card.top() + 36.0 + j as f32 * 20.0),
                        egui::Align2::LEFT_CENTER,
                        *item,
                        font(TextStyle::MonoSmall),
                        p.text_primary,
                    );
                }
            }
        },
    );

    api_signature_table(
        ui,
        "AS2 Core Classes — Essential Properties & Methods",
        ["Class", "Key Members", "Usage & Notes"],
        &[
            (
                "MovieClip",
                "_x, _y, _xscale, _yscale, _rotation, _alpha, _visible, _currentframe, _totalframes",
                "In AS2, _alpha and _xscale/_yscale use 0..100 percentages; _framesloaded tracks streaming progress.",
            ),
            (
                "MovieClip (Drawing)",
                "lineStyle(w, rgb, a), beginFill(rgb, a), beginGradientFill(...), moveTo, lineTo, curveTo, endFill, clear",
                "Procedural vector drawing directly inside any MovieClip; curveTo(cx, cy, ax, ay) draws a quadratic Bézier.",
            ),
            (
                "TextField & TextFormat",
                "createTextField(name, depth, x, y, w, h), text, htmlText, setTextFormat(fmt), setNewTextFormat(fmt)",
                "Supports border, background, wordWrap, multiline, embedFonts, autoSize, and scroll / maxscroll.",
            ),
            (
                "Sound",
                "new Sound(target_mc), attachSound(id), loadSound(url, isStreaming), start(offsetSec, loops), stop(), setVolume(0..100), setPan(-100..100)",
                "Passing a target_mc scopes volume/pan to that timeline; omitting it controls global audio.",
            ),
            (
                "Key & Mouse",
                "Key.isDown(code), Key.getCode(), Key.addListener(obj), Mouse.hide(), Mouse.show(), Mouse.addListener(obj)",
                "Listeners receive onKeyDown / onKeyUp and onMouseDown / onMouseUp / onMouseMove / onMouseWheel.",
            ),
            (
                "flash.display.BitmapData",
                "new BitmapData(w, h, transparent, fillARGB), draw(mc, matrix, colorTransform, blend), getPixel32, setPixel32, perlinNoise, copyPixels",
                "Introduced in Flash 8; attach to a MovieClip via mc.attachBitmap(bmp, depth, \"auto\", smoothing).",
            ),
            (
                "LoadVars & SharedObject",
                "lv.load(url), lv.sendAndLoad(url, target, method), lv.onLoad(ok); SharedObject.getLocal(\"save\")",
                "LoadVars parses URL-encoded key/value pairs; SharedObject.data persists structured local state via flush().",
            ),
        ],
    );

    code_block(
        ui,
        &CodeSample {
            title: "AS2 Procedural Vector Drawing, Dynamic Text & Flash 8 BitmapData",
            lang_badge: "ActionScript 2.0",
            code: "import flash.display.BitmapData;
import flash.geom.Matrix;

// 1. Create an empty MovieClip and draw a rounded badge with the AS2 Drawing API
var canvas:MovieClip = this.createEmptyMovieClip(\"badge_mc\", this.getNextHighestDepth());
canvas.lineStyle(2, 0x6C93FF, 100);
canvas.beginFill(0x1F2A48, 95);
canvas.moveTo(20, 20);
canvas.lineTo(180, 20);
canvas.lineTo(180, 70);
canvas.lineTo(20, 70);
canvas.lineTo(20, 20);
canvas.endFill();

// 2. Create a Dynamic TextField and style it with TextFormat
this.createTextField(\"label_txt\", this.getNextHighestDepth(), 32, 32, 140, 28);
var fmt:TextFormat = new TextFormat(\"Inter\", 14, 0xFFFFFF, true);
label_txt.text = \"READY PLAYER ONE\";
label_txt.setTextFormat(fmt);

// 3. Rasterize vector art into a Flash 8 BitmapData buffer
var bmp:BitmapData = new BitmapData(200, 90, true, 0x00000000);
bmp.draw(canvas, new Matrix());",
        },
    );

    super::help_manual::inline_cross_links(
        ui,
        nav_target,
        &[
            (8, "9. Filters, Text & Audio"),
            (10, "11. AS1 & AS2 Fundamentals & Timeline"),
            (14, "15. AS3 Graphics, Filters, Sound & Data"),
        ],
    );
}

// ── Chapter 13: AS3 Architecture & Display List ──

pub(crate) fn chapter_as3_display_list(
    ui: &mut Ui,
    diagram_tab: &mut usize,
    nav_target: &mut Option<usize>,
) {
    paragraph(
        ui,
        "ActionScript 3.0 (introduced with Flash Player 9 / AVM2) replaces AVM1's depth-numbered MovieClip tree and silent failure semantics with a strongly typed, class-based architecture, explicit package namespaces, traits-backed sealed classes, and a unified Display List scene graph.",
    );

    let tabs = [
        (0, "Display List Class Hierarchy"),
        (1, "Local ↔ Global Coordinate Spaces"),
    ];
    c::Segmented::show(ui, diagram_tab, &tabs, false);
    ui.add_space(space::S2);

    if *diagram_tab == 0 {
        illustration_canvas(
            ui,
            168.0,
            "Illustration 13.1: The ActionScript 3.0 flash.display Inheritance Hierarchy: EventDispatcher → DisplayObject → InteractiveObject → DisplayObjectContainer → Sprite → MovieClip, alongside leaf display nodes (Shape, Bitmap, TextField).",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let chain = [
                    ("EventDispatcher", "addEventListener / dispatchEvent"),
                    ("DisplayObject", "x, y, scaleX, rotation, alpha, filters"),
                    ("InteractiveObject", "mouseEnabled, tabEnabled, focusRect"),
                    (
                        "DisplayObjectContainer",
                        "addChild, removeChild, numChildren",
                    ),
                    ("Sprite / MovieClip", "graphics, buttonMode, currentFrame"),
                ];
                let pad_x = 12.0;
                let w = (rect.width() - pad_x * 6.0) / 5.0;
                let top_y = rect.top() + 18.0;
                let h = 68.0;
                for (i, (cls, members)) in chain.iter().enumerate() {
                    let x = rect.left() + pad_x + i as f32 * (w + pad_x);
                    let node = Rect::from_min_size(egui::pos2(x, top_y), vec2(w, h));
                    painter.rect_filled(node, 5.0, p.bg_panel);
                    painter.rect_stroke(
                        node,
                        5.0,
                        Stroke::new(
                            1.5_f32,
                            if i >= 3 {
                                p.accent_default
                            } else {
                                p.border_default
                            },
                        ),
                        StrokeKind::Inside,
                    );
                    painter.text(
                        egui::pos2(node.center().x, node.top() + 20.0),
                        egui::Align2::CENTER_CENTER,
                        *cls,
                        font(TextStyle::BodyStrong),
                        p.accent_default,
                    );
                    painter.text(
                        egui::pos2(node.center().x, node.top() + 44.0),
                        egui::Align2::CENTER_CENTER,
                        *members,
                        font(TextStyle::MonoSmall),
                        p.text_secondary,
                    );
                    if i + 1 < chain.len() {
                        painter.line_segment(
                            [
                                egui::pos2(node.right() + 2.0, node.center().y),
                                egui::pos2(node.right() + pad_x - 2.0, node.center().y),
                            ],
                            Stroke::new(1.5_f32, p.accent_default),
                        );
                    }
                }
                let leaves = [
                    "Leaf DisplayObjects: Shape (lightweight vector)",
                    "Bitmap (wraps BitmapData)",
                    "TextField (InteractiveObject)",
                    "SimpleButton (up/over/down/hitTestState)",
                ];
                let leaf_y = rect.bottom() - 36.0;
                let lw = (rect.width() - pad_x * 5.0) / 4.0;
                for (i, ltxt) in leaves.iter().enumerate() {
                    let x = rect.left() + pad_x + i as f32 * (lw + pad_x);
                    let lr = Rect::from_min_size(egui::pos2(x, leaf_y), vec2(lw, 24.0));
                    painter.rect_filled(lr, 4.0, p.accent_subtle);
                    painter.text(
                        lr.center(),
                        egui::Align2::CENTER_CENTER,
                        *ltxt,
                        font(TextStyle::MonoSmall),
                        p.text_primary,
                    );
                }
            },
        );
    } else {
        illustration_canvas(
            ui,
            168.0,
            "Illustration 13.2: Coordinate Space Mapping between Stage Global Coordinates (0,0 top-left) and a transformed child Sprite's Local Space via localToGlobal(pt) and globalToLocal(pt).",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let stage_box = Rect::from_min_size(
                    rect.min + vec2(20.0, 18.0),
                    vec2(rect.width() - 40.0, rect.height() - 36.0),
                );
                painter.rect_filled(stage_box, 4.0, p.bg_stage);
                painter.rect_stroke(
                    stage_box,
                    4.0,
                    Stroke::new(1.0_f32, p.border_default),
                    StrokeKind::Inside,
                );
                let g_origin = stage_box.min + vec2(18.0, 18.0);
                painter.circle_filled(g_origin, 4.0, p.guide);
                painter.text(
                    g_origin + vec2(10.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    "Stage Global (0, 0)",
                    font(TextStyle::MonoSmall),
                    p.guide,
                );

                let local_box =
                    Rect::from_center_size(stage_box.center() + vec2(40.0, 8.0), vec2(210.0, 76.0));
                painter.rect_filled(local_box, 4.0, p.accent_subtle);
                painter.rect_stroke(
                    local_box,
                    4.0,
                    Stroke::new(1.5_f32, p.accent_default),
                    StrokeKind::Inside,
                );
                let l_origin = local_box.min + vec2(16.0, 16.0);
                let pt = local_box.center() + vec2(36.0, 8.0);
                painter.circle_filled(l_origin, 4.0, p.warning_default);
                painter.circle_filled(pt, 4.5, p.success_default);
                painter.line_segment([g_origin, pt], Stroke::new(1.5_f32, p.guide));
                painter.line_segment([l_origin, pt], Stroke::new(1.5_f32, p.warning_default));
                painter.text(
                    l_origin + vec2(8.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    "container Local (0, 0)",
                    font(TextStyle::MonoSmall),
                    p.warning_default,
                );
                painter.text(
                    pt + vec2(-10.0, 16.0),
                    egui::Align2::CENTER_CENTER,
                    "globalPt = container.localToGlobal(new Point(lx, ly))",
                    font(TextStyle::MonoSmall),
                    p.text_primary,
                );
            },
        );
    }

    mode_card(
        ui,
        Some(i16::CLASS),
        "AVM2 Language Semantics — Strong Typing, Vector.<T>, Dictionary & Sealed Classes",
        "AVM2 Core",
        BadgeType::Accent,
        "AS3",
        &[
            "Numeric Primitives: `int` (signed 32-bit integer, default `0`), `uint` (unsigned 32-bit integer for ARGB colours/bitmasks, default `0`), and `Number` (IEEE-754 double-precision float, default `NaN`).",
            "Typed Collections: `Vector.<T>` provides contiguous, fixed-type arrays (`new <Sprite>[a, b]`, optional `fixed = true`), while `flash.utils.Dictionary(weakKeys)` maps object references directly to values without stringifying keys.",
            "Sealed vs Dynamic Classes & Namespaces: Classes are sealed by default (accessing an undeclared property throws `ReferenceError 1069` at runtime or compiler error `1119`), except `MovieClip` and classes explicitly marked `dynamic`. Access control uses `public`, `private`, `protected`, and `internal`.",
        ],
    );

    api_signature_table(
        ui,
        "DisplayObjectContainer & DisplayObject — Scene Graph Operations",
        ["Method / Property", "Signature / Type", "Behavior"],
        &[
            (
                "addChild / addChildAt",
                "(child:DisplayObject, index:int):DisplayObject",
                "Instantiates or reparents `child` onto this container's display list (automatically removing it from any previous parent) and dispatches `Event.ADDED` + `Event.ADDED_TO_STAGE`.",
            ),
            (
                "removeChild / removeChildAt",
                "(child:DisplayObject) / (index:int)",
                "Detaches `child` from the display list and shifts higher-index siblings down to keep child indices `0..numChildren - 1` contiguous.",
            ),
            (
                "setChildIndex / swapChildren",
                "(child, index:int) / (child1, child2)",
                "Reorders z-stacking without removing or re-dispatching `ADDED_TO_STAGE`.",
            ),
            (
                "getObjectsUnderPoint",
                "(point:Point):Array",
                "Returns all display objects underneath `point` (expressed in Stage global coordinates).",
            ),
            (
                "getBounds / getRect",
                "(targetCoordinateSpace:DisplayObject):Rectangle",
                "`getBounds` includes stroke widths; `getRect` measures fill bounds only, transformed into `targetCoordinateSpace`.",
            ),
            (
                "hitTestObject / hitTestPoint",
                "(obj:DisplayObject) / (x, y, shapeFlag:Boolean)",
                "Tests bounding-box intersection (`hitTestObject`) or global point vs actual vector/pixel silhouette (`shapeFlag = true`).",
            ),
        ],
    );

    code_block(
        ui,
        &CodeSample {
            title: "AS3 Document Class — Building a Display List Hierarchy & Coordinate Transform",
            lang_badge: "ActionScript 3.0",
            code: "package {
    import flash.display.Sprite;
    import flash.display.Shape;
    import flash.events.Event;
    import flash.geom.Point;

    public class Main extends Sprite {
        private varship:Sprite = new Sprite();

        public function Main() {
            if (stage) init();
            else addEventListener(Event.ADDED_TO_STAGE, init);
        }

        private function init(e:Event = null):void {
            removeEventListener(Event.ADDED_TO_STAGE, init);
            var hull:Shape = new Shape();
            hull.graphics.beginFill(0x6C93FF, 1.0);
            hull.graphics.drawTriangles(new <Number>[0,-18, 14,14, -14,14]);
            hull.graphics.endFill();

            varship.addChild(hull);
            varship.x = stage.stageWidth * 0.5;
            varship.y = stage.stageHeight * 0.5;
            addChild(varship);

            // Convert muzzle tip from local ship coordinates to Stage global coordinates
            var tipGlobal:Point = varship.localToGlobal(new Point(0, -18));
            trace(\"Ship nose in Stage coordinates:\", tipGlobal.x, tipGlobal.y);
        }
    }
}",
        },
    );

    super::help_manual::inline_cross_links(
        ui,
        nav_target,
        &[
            (5, "6. Symbols, 9-Slice & Library"),
            (13, "14. AS3 Events, Input & Lifecycle"),
            (14, "15. AS3 Graphics, Filters, Sound & Data"),
        ],
    );
}

// ── Chapter 14: AS3 Events, Input & Lifecycle ──

pub(crate) fn chapter_as3_events_lifecycle(
    ui: &mut Ui,
    diagram_tab: &mut usize,
    nav_target: &mut Option<usize>,
) {
    paragraph(
        ui,
        "Every interactive object and asynchronous subsystem in ActionScript 3.0 inherits from flash.events.EventDispatcher. Display list events travel through a deterministic W3C three-phase propagation path (Capture → Target → Bubbling), and frame updates follow a strict multi-step lifecycle on every tick.",
    );

    let tabs = [
        (0, "3-Phase Event Flow (Capture → Target → Bubble)"),
        (1, "Deterministic Frame Tick Lifecycle"),
    ];
    c::Segmented::show(ui, diagram_tab, &tabs, false);
    ui.add_space(space::S2);

    if *diagram_tab == 0 {
        illustration_canvas(
            ui,
            164.0,
            "Illustration 14.1: W3C 3-Phase Event Propagation across the Display List: Phase 1 Capture (Stage → Panel, useCapture=true), Phase 2 At Target (Button), and Phase 3 Bubbling (Panel → Stage, bubbles=true).",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let nodes = [
                    ("1. Stage", "event.currentTarget"),
                    ("2.root / HUD_mc", "DisplayObjectContainer"),
                    ("3. dialog_mc", "Parent Sprite"),
                    ("4. play_btn (Target)", "event.target"),
                ];
                let pad = 14.0;
                let w = (rect.width() - pad * 5.0) / 4.0;
                let mid_y = rect.center().y;
                for (i, (title, sub)) in nodes.iter().enumerate() {
                    let x = rect.left() + pad + i as f32 * (w + pad);
                    let box_r = Rect::from_min_size(egui::pos2(x, mid_y - 26.0), vec2(w, 52.0));
                    let is_target = i == 3;
                    painter.rect_filled(
                        box_r,
                        5.0,
                        if is_target {
                            p.warning_subtle
                        } else {
                            p.bg_panel
                        },
                    );
                    painter.rect_stroke(
                        box_r,
                        5.0,
                        Stroke::new(
                            1.5_f32,
                            if is_target {
                                p.warning_default
                            } else {
                                p.accent_default
                            },
                        ),
                        StrokeKind::Inside,
                    );
                    painter.text(
                        egui::pos2(box_r.center().x, box_r.top() + 18.0),
                        egui::Align2::CENTER_CENTER,
                        *title,
                        font(TextStyle::BodyStrong),
                        if is_target {
                            p.warning_default
                        } else {
                            p.text_primary
                        },
                    );
                    painter.text(
                        egui::pos2(box_r.center().x, box_r.top() + 36.0),
                        egui::Align2::CENTER_CENTER,
                        *sub,
                        font(TextStyle::MonoSmall),
                        p.text_secondary,
                    );
                }
                painter.line_segment(
                    [
                        egui::pos2(rect.left() + 40.0, rect.top() + 26.0),
                        egui::pos2(rect.right() - 40.0, rect.top() + 26.0),
                    ],
                    Stroke::new(2.0_f32, p.guide),
                );
                painter.text(
                    egui::pos2(rect.center().x, rect.top() + 14.0),
                    egui::Align2::CENTER_CENTER,
                    "Phase 1: CAPTURING_PHASE (Stage → Parent, useCapture = true) ──►",
                    font(TextStyle::MonoSmall),
                    p.guide,
                );
                painter.line_segment(
                    [
                        egui::pos2(rect.right() - 40.0, rect.bottom() - 28.0),
                        egui::pos2(rect.left() + 40.0, rect.bottom() - 28.0),
                    ],
                    Stroke::new(2.0_f32, p.success_default),
                );
                painter.text(
                    egui::pos2(rect.center().x, rect.bottom() - 14.0),
                    egui::Align2::CENTER_CENTER,
                    "◄── Phase 3: BUBBLING_PHASE (Target → Stage, useCapture = false, bubbles = true)",
                    font(TextStyle::MonoSmall),
                    p.success_default,
                );
            },
        );
    } else {
        illustration_canvas(
            ui,
            164.0,
            "Illustration 14.2: Flash Player / NeoFlash AVM2 Frame Execution Order on every playhead advancement or tick.",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let steps = [
                    ("1. ENTER_FRAME", "Broadcast to all DisplayObjects"),
                    ("2. Construct", "Instantiate new timeline children"),
                    ("3. FRAME_CONSTRUCTED", "Inspect newly created instances"),
                    ("4. Frame Scripts", "Execute timeline keyframe code"),
                    ("5. EXIT_FRAME", "Post-script cleanup & layout"),
                    ("6. RENDER", "Fires if stage.invalidate() called"),
                ];
                let cols = 3;
                let rows = 2;
                let pad_x = 12.0;
                let pad_y = 12.0;
                let cw = (rect.width() - pad_x * 4.0) / cols as f32;
                let ch = (rect.height() - pad_y * 3.0) / rows as f32;
                for (idx, (step, desc)) in steps.iter().enumerate() {
                    let r_i = idx / cols;
                    let c_i = idx % cols;
                    let x = rect.left() + pad_x + c_i as f32 * (cw + pad_x);
                    let y = rect.top() + pad_y + r_i as f32 * (ch + pad_y);
                    let cell = Rect::from_min_size(egui::pos2(x, y), vec2(cw, ch));
                    painter.rect_filled(cell, 5.0, p.bg_panel);
                    painter.rect_stroke(
                        cell,
                        5.0,
                        Stroke::new(1.2_f32, p.accent_default),
                        StrokeKind::Inside,
                    );
                    painter.text(
                        egui::pos2(cell.left() + 10.0, cell.center().y - 8.0),
                        egui::Align2::LEFT_CENTER,
                        *step,
                        font(TextStyle::MonoSmall),
                        p.accent_default,
                    );
                    painter.text(
                        egui::pos2(cell.left() + 10.0, cell.center().y + 10.0),
                        egui::Align2::LEFT_CENTER,
                        *desc,
                        font(TextStyle::Small),
                        p.text_primary,
                    );
                }
            },
        );
    }

    api_signature_table(
        ui,
        "EventDispatcher & Core AS3 Event Classes",
        [
            "Method / Event Class",
            "Key Constants & Properties",
            "Behavior & Best Practices",
        ],
        &[
            (
                "addEventListener",
                "(type, listener, useCapture=false, priority=0, useWeakReference=false)",
                "Higher `priority` listeners execute first within the same phase; `useWeakReference=true` prevents listener memory leaks on short-lived objects.",
            ),
            (
                "event.target vs currentTarget",
                "Object / EventDispatcher",
                "`event.target` is the innermost dispatcher that originated the event (e.g. a child label inside a button); `event.currentTarget` is the object whose listener is currently running.",
            ),
            (
                "stopPropagation vs Immediate",
                "stopPropagation() / stopImmediatePropagation()",
                "`stopPropagation()` lets remaining listeners on the current node finish before halting onward flow; `stopImmediatePropagation()` halts even sibling listeners on the same node.",
            ),
            (
                "Event (Lifecycle)",
                "ENTER_FRAME, FRAME_CONSTRUCTED, EXIT_FRAME, RENDER, ADDED_TO_STAGE, REMOVED_FROM_STAGE, RESIZE, COMPLETE",
                "Always wait for `ADDED_TO_STAGE` before reading `stage.stageWidth` or registering global `stage` keyboard listeners.",
            ),
            (
                "MouseEvent",
                "CLICK, DOUBLE_CLICK, MOUSE_DOWN, MOUSE_UP, MOUSE_MOVE, MOUSE_OVER, MOUSE_OUT, ROLL_OVER, ROLL_OUT, MOUSE_WHEEL",
                "`ROLL_OVER`/`ROLL_OUT` do not bubble when moving between children of the same container; set `doubleClickEnabled = true` to receive `DOUBLE_CLICK`.",
            ),
            (
                "KeyboardEvent & TimerEvent",
                "KEY_DOWN, KEY_UP (keyCode, charCode, shiftKey, ctrlKey); TimerEvent.TIMER, TIMER_COMPLETE",
                "Attach `KeyboardEvent` listeners to `stage` so key presses register regardless of which child display object currently holds focus.",
            ),
        ],
    );

    code_block(
        ui,
        &CodeSample {
            title: "AS3 Event Handling — Stage Keyboard Input, Mouse Dragging & Timer Loop",
            lang_badge: "ActionScript 3.0",
            code: "import flash.events.Event;
import flash.events.MouseEvent;
import flash.events.KeyboardEvent;
import flash.events.TimerEvent;
import flash.ui.Keyboard;
import flash.utils.Timer;

// 1. Mouse interaction with target vs currentTarget awareness
card_mc.buttonMode = true;
card_mc.mouseChildren = false; // Ensure event.target is always card_mc
card_mc.addEventListener(MouseEvent.MOUSE_DOWN, function(e:MouseEvent):void {
    card_mc.startDrag();
});
stage.addEventListener(MouseEvent.MOUSE_UP, function(e:MouseEvent):void {
    card_mc.stopDrag();
});

// 2. Global Stage keyboard listener
stage.addEventListener(KeyboardEvent.KEY_DOWN, function(e:KeyboardEvent):void {
    if (e.keyCode == Keyboard.SPACE) {
        trace(\"Spacebar pressed! Shift held:\", e.shiftKey);
    }
});

// 3. Precision Timer (fires every 500 ms, 10 times)
var spawner:Timer = new Timer(500, 10);
spawner.addEventListener(TimerEvent.TIMER, function(e:TimerEvent):void {
    trace(\"Wave tick:\", spawner.currentCount);
});
spawner.start();",
        },
    );

    super::help_manual::inline_cross_links(
        ui,
        nav_target,
        &[
            (12, "13. AS3 Architecture & Display List"),
            (14, "15. AS3 Graphics, Filters, Sound & Data"),
            (15, "16. AS1/AS2 ↔ AS3 ↔ JS/TS Migration Guide"),
        ],
    );
}

// ── Chapter 15: AS3 Graphics, Filters, Sound & Data ──

pub(crate) fn chapter_as3_graphics_filters_sound(
    ui: &mut Ui,
    _diagram_tab: &mut usize,
    nav_target: &mut Option<usize>,
) {
    paragraph(
        ui,
        "ActionScript 3.0 exposes hardware-accelerated and software-deterministic APIs for retained vector paths (`flash.display.Graphics`), GPU/CPU display filters (`flash.filters.*`), multi-channel audio synthesis and spectrum analysis (`flash.media.*`), binary `ByteArray` streams, and E4X (`XML`).",
    );

    section_overline(
        ui,
        "Interactive Diagram — Vector Graphics Pipeline, Live Filter Pass & SoundChannel Mixer",
    );
    illustration_canvas(
        ui,
        156.0,
        "Illustration 15.1: Left — flash.display.Graphics command stream (strokes, fills, curves, drawTriangles). Centre — DisplayObject.filters & BitmapData pass. Right — Sound → SoundChannel → SoundTransform → SoundMixer.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let w3 = rect.width() / 3.0;
            let panels = [
                (
                    "1. flash.display.Graphics",
                    &[
                        "lineStyle(2, 0x6C93FF, 1.0)",
                        "beginGradientFill(LINEAR, ...)",
                        "moveTo / lineTo / curveTo",
                        "cubicCurveTo(cx1,cy1,cx2,cy2,x,y)",
                        "drawRoundRect / drawTriangles",
                    ],
                    p.accent_default,
                ),
                (
                    "2. Filters & BitmapData",
                    &[
                        "DropShadowFilter / GlowFilter",
                        "BlurFilter(blurX, blurY, quality)",
                        "ColorMatrixFilter (4x5 RGBA)",
                        "BitmapData.perlinNoise / draw",
                        "copyPixels / paletteMap / threshold",
                    ],
                    p.warning_default,
                ),
                (
                    "3. Audio & ByteArray I/O",
                    &[
                        "ch = sound.play(startMs, loops)",
                        "ch.soundTransform = new SoundTransform(vol, pan)",
                        "SoundMixer.computeSpectrum(ba, true)",
                        "ByteArray (compress / readFloat)",
                        "JSON.parse / XML E4X (@attr, ..tag)",
                    ],
                    p.success_default,
                ),
            ];
            for (i, (title, lines, accent)) in panels.iter().enumerate() {
                let cell = Rect::from_min_size(
                    egui::pos2(rect.left() + 8.0 + i as f32 * w3, rect.top() + 10.0),
                    vec2(w3 - 16.0, rect.height() - 20.0),
                );
                painter.rect_filled(cell, 5.0, p.bg_panel);
                painter.rect_stroke(cell, 5.0, Stroke::new(1.2_f32, *accent), StrokeKind::Inside);
                painter.text(
                    egui::pos2(cell.left() + 10.0, cell.top() + 16.0),
                    egui::Align2::LEFT_CENTER,
                    *title,
                    font(TextStyle::BodyStrong),
                    *accent,
                );
                for (j, ln) in lines.iter().enumerate() {
                    painter.text(
                        egui::pos2(cell.left() + 10.0, cell.top() + 38.0 + j as f32 * 20.0),
                        egui::Align2::LEFT_CENTER,
                        *ln,
                        font(TextStyle::MonoSmall),
                        p.text_primary,
                    );
                }
            }
        },
    );

    api_signature_table(
        ui,
        "AS3 Graphics, Geometry, Filters, Sound & Serialization Reference",
        ["Package / Class", "Core APIs", "Key Details"],
        &[
            (
                "flash.display.Graphics",
                "clear, lineStyle, beginFill, beginGradientFill, beginBitmapFill, drawRect, drawRoundRect, drawCircle, drawEllipse, moveTo, lineTo, curveTo, cubicCurveTo, drawPath, drawTriangles, endFill",
                "Note: AS3 `alpha` arguments in `beginFill(color, alpha)` and `lineStyle` are `0.0..1.0` (unlike AS2's `0..100`).",
            ),
            (
                "flash.geom.*",
                "Point, Rectangle, Matrix(a,b,c,d,tx,ty), Matrix3D, ColorTransform, Transform",
                "`matrix.createGradientBox(w, h, rot, tx, ty)` prepares linear/radial gradient UV space for `beginGradientFill`.",
            ),
            (
                "flash.filters.*",
                "BlurFilter, DropShadowFilter, GlowFilter, BevelFilter, GradientGlowFilter, ColorMatrixFilter, ConvolutionFilter",
                "Mutating an element of `mc.filters` in-place has no effect until you reassign the array: `mc.filters = [blur, glow];`.",
            ),
            (
                "flash.media.Sound*",
                "Sound, SoundChannel (position, leftPeak, rightPeak, stop, SOUND_COMPLETE), SoundTransform(volume, pan), SoundMixer",
                "`sound.play()` returns a new `SoundChannel` handle for each playing voice; `SoundDataEvent.SAMPLE_DATA` synthesizes dynamic PCM.",
            ),
            (
                "flash.utils.ByteArray",
                "endian, position, length, writeUTFBytes, readUTFBytes, writeInt, readFloat, compress(), uncompress()",
                "Supports `Endian.BIG_ENDIAN` and `LITTLE_ENDIAN` plus `CompressionAlgorithm.ZLIB`, `DEFLATE`, and `LZMA`.",
            ),
            (
                "XML (E4X) & JSON",
                "new XML(str), xml.item.(@id == \"hero\"), xml..node, JSON.parse(str), JSON.stringify(obj)",
                "E4X (`..` descendant axis, `@` attribute axis, predicate filters `.(@price < 50)`) queries XML trees natively.",
            ),
        ],
    );

    code_block(
        ui,
        &CodeSample {
            title: "AS3 Vector Graphics, Live Filters, SoundChannel & E4X XML Query",
            lang_badge: "ActionScript 3.0",
            code: "import flash.display.Sprite;
import flash.filters.GlowFilter;
import flash.media.Sound;
import flash.media.SoundChannel;
import flash.media.SoundTransform;

// 1. Draw a smooth badge and attach a live GlowFilter
var orb:Sprite = new Sprite();
orb.graphics.lineStyle(2, 0x86A5FF, 1.0);
orb.graphics.beginFill(0x3C6BF0, 0.85);
orb.graphics.drawCircle(0, 0, 36);
orb.graphics.endFill();
orb.filters = [new GlowFilter(0x6C93FF, 0.9, 16, 16, 2, 2)];
addChild(orb);

// 2. Play a Library Sound with stereo panning
var sfx:Sound = new LaserSound();
var channel:SoundChannel = sfx.play(0, 1, new SoundTransform(0.8, -0.25));

// 3. E4X XML attribute & descendant filtering
var inventory:XML = <items>
    <weapon id=\"plasma\" dmg=\"45\"/>
    <weapon id=\"railgun\" dmg=\"90\"/>
</items>;
var heavy:XMLList = inventory.weapon.(Number(@dmg) >= 50);
trace(\"Selected weapon:\", heavy.@id); // railgun",
        },
    );

    super::help_manual::inline_cross_links(
        ui,
        nav_target,
        &[
            (7, "8. Tweens, Motion & IK"),
            (8, "9. Filters, Text & Audio"),
            (12, "13. AS3 Architecture & Display List"),
        ],
    );
}

// ── Chapter 16: AS1/AS2 ↔ AS3 ↔ JS/TS Migration Guide ──

pub(crate) fn chapter_as_migration_guide(
    ui: &mut Ui,
    _diagram_tab: &mut usize,
    nav_target: &mut Option<usize>,
) {
    paragraph(
        ui,
        "NeoFlash is uniquely designed to author and publish ActionScript 1.0/2.0, ActionScript 3.0, JavaScript (ES2022), and TypeScript projects side by side. Use this side-by-side equivalence guide when porting legacy FLA scripts to AS3 or modern HTML5 JS/TS.",
    );

    section_overline(
        ui,
        "Interactive Diagram — Multi-Language Execution Bridge to the Shared Rust / WASM Engine",
    );
    illustration_canvas(
        ui,
        142.0,
        "Illustration 16.1: All four NeoFlash scripting languages compile and bind to the same deterministic Rust scene graph, timeline evaluator, filter compositor, and Web Audio scheduler.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let langs = [
                ("AS1 / AS2 (AVM1)", "_root, _x, onEnterFrame, attachMovie"),
                ("AS3 (AVM2)", "stage, x, addEventListener, addChild"),
                (
                    "JavaScript / TypeScript",
                    "stage, mc.x, on(\"enterFrame\"), new Symbol()",
                ),
            ];
            let pad = 12.0;
            let lw = (rect.width() - pad * 4.0) / 3.0;
            for (i, (name, ex)) in langs.iter().enumerate() {
                let x = rect.left() + pad + i as f32 * (lw + pad);
                let top_box = Rect::from_min_size(egui::pos2(x, rect.top() + 10.0), vec2(lw, 52.0));
                painter.rect_filled(top_box, 5.0, p.bg_panel);
                painter.rect_stroke(
                    top_box,
                    5.0,
                    Stroke::new(1.2_f32, p.accent_default),
                    StrokeKind::Inside,
                );
                painter.text(
                    egui::pos2(top_box.center().x, top_box.top() + 18.0),
                    egui::Align2::CENTER_CENTER,
                    *name,
                    font(TextStyle::BodyStrong),
                    p.accent_default,
                );
                painter.text(
                    egui::pos2(top_box.center().x, top_box.top() + 36.0),
                    egui::Align2::CENTER_CENTER,
                    *ex,
                    font(TextStyle::MonoSmall),
                    p.text_secondary,
                );
                painter.line_segment(
                    [
                        egui::pos2(top_box.center().x, top_box.bottom() + 2.0),
                        egui::pos2(top_box.center().x, rect.bottom() - 44.0),
                    ],
                    Stroke::new(1.5_f32, p.accent_default),
                );
            }
            let core_bar = Rect::from_min_max(
                egui::pos2(rect.left() + pad, rect.bottom() - 42.0),
                egui::pos2(rect.right() - pad, rect.bottom() - 10.0),
            );
            painter.rect_filled(core_bar, 5.0, p.accent_subtle);
            painter.rect_stroke(
                core_bar,
                5.0,
                Stroke::new(1.5_f32, p.accent_default),
                StrokeKind::Inside,
            );
            painter.text(
                core_bar.center(),
                egui::Align2::CENTER_CENTER,
                "NeoFlash Shared Rust / WebAssembly Runtime (Display List · Timeline · Vector/Filter Renderer · Web Audio)",
                font(TextStyle::BodyStrong),
                p.text_primary,
            );
        },
    );

    api_signature_table(
        ui,
        "Rosetta Stone — AS1/AS2 vs AS3 vs JavaScript/TypeScript Equivalents",
        [
            "ActionScript 1.0 / 2.0",
            "ActionScript 3.0",
            "JavaScript / TypeScript & Scale Note",
        ],
        &[
            (
                "_x, _y, _rotation, _visible",
                "x, y, rotation, visible",
                "`mc.x`, `mc.y`, `mc.rotation`, `mc.visible` (leading underscores removed in AS3 & JS/TS).",
            ),
            (
                "_xscale, _yscale (0..100%)",
                "scaleX, scaleY (0.0..1.0)",
                "`mc.scaleX = 1.0` equals `100%`! Dividing AS2 `_xscale` by 100 converts to AS3/JS scale.",
            ),
            (
                "_alpha (0..100%)",
                "alpha (0.0..1.0)",
                "`mc.alpha = 0.5` equals AS2 `_alpha = 50`.",
            ),
            (
                "_currentframe, _totalframes",
                "currentFrame, totalFrames",
                "`mc.currentFrame`, `mc.totalFrames` (1-based frame index across all languages).",
            ),
            (
                "_root / _parent / _name",
                "root / parent / name",
                "`this.root` / `this.parent` / `mc.name`.",
            ),
            (
                "mc.onRelease = fn",
                "mc.addEventListener(MouseEvent.CLICK, fn)",
                "`mc.addEventListener(\"click\", fn)` or `mc.on(\"click\", fn)`.",
            ),
            (
                "mc.onEnterFrame = fn",
                "mc.addEventListener(Event.ENTER_FRAME, fn)",
                "`mc.addEventListener(\"enterFrame\", fn)`.",
            ),
            (
                "attachMovie(\"LinkID\", \"n\", d)",
                "var c = new LinkClass(); addChild(c);",
                "Instantiate the exported symbol class and call `this.addChild(c)`.",
            ),
            (
                "removeMovieClip(mc)",
                "mc.parent.removeChild(mc)",
                "`mc.parent.removeChild(mc)`.",
            ),
            (
                "getURL(url, \"_blank\")",
                "navigateToURL(new URLRequest(url), \"_blank\")",
                "`window.open(url, \"_blank\")`.",
            ),
            (
                "getTimer()",
                "flash.utils.getTimer()",
                "`performance.now()` (milliseconds elapsed since movie start).",
            ),
        ],
    );

    mode_card(
        ui,
        Some(i16::WARNING),
        "Top 5 Migration Pitfalls When Porting Legacy AS2 FLA Projects to AS3",
        "Checklist",
        BadgeType::Warning,
        "Compiler ⌥F2",
        &[
            "1) Percentage vs Unit Ranges: AS2 `_alpha = 50` and `_xscale = 200` become `alpha = 0.5` and `scaleX = 2.0` in AS3. Setting `alpha = 50` in AS3 clamps to `1.0` (fully opaque)!",
            "2) Depth Numbers vs Contiguous Display List Indices: AS2 allows sparse depths (`swapDepths(9999)`), whereas AS3 `addChildAt(child, index)` requires `0 <= index <= numChildren` (out-of-bounds indices throw `RangeError`).",
            "3) Button Instance Scope (`this`): In an AS1/AS2 Button `on(release)` script, `this` refers to the parent MovieClip timeline; in an AS3 `MouseEvent.CLICK` listener method on a class, `this` is the class instance where the method is defined.",
            "4) Drawing Target Separation: In AS2, you draw directly on a MovieClip (`mc.beginFill(...)`); in AS3, drawing commands live on the `.graphics` property (`mc.graphics.beginFill(...)`).",
            "5) Strict Null & Undefined Checks: In AS2, calling a method on an `undefined` target path (`_root.missing_mc.play()`) silently does nothing; in AS3, calling a method on `null` throws `TypeError 1009`.",
        ],
    );

    if let (Some(as2_api), Some(as3_api)) = (
        neoflash_script::language_service::api(neoflash_core::ScriptLanguage::ActionScript2),
        neoflash_script::language_service::api(neoflash_core::ScriptLanguage::ActionScript3),
    ) {
        let summary_title = format!(
            "Built-In ActionScript Catalog Summary — AS1/AS2 ({} Classes · {} Members) & AS3 ({} Classes · {} Members)",
            as2_api.classes.len(),
            as2_api.members.len(),
            as3_api.classes.len(),
            as3_api.members.len(),
        );
        mode_card(
            ui,
            Some(i16::CODE),
            &summary_title,
            "Live API Index",
            BadgeType::Accent,
            "⌃Space Hints",
            &[
                "All 25 Classic AS1/AS2 Variable Suffixes for Automatic Code Hints: `_mc` (MovieClip), `_txt` (TextField), `_btn` (Button), `_array` (Array), `_str` (String), `_sound` (Sound), `_xml` (XML), `_xmlnode` (XMLNode), `_xmlsocket` (XMLSocket), `_so` (SharedObject), `_nc` (NetConnection), `_ns` (NetStream), `_video` (Video), `_fmt` (TextFormat), `_lv` (LoadVars), `_lc` (LocalConnection), `_color` (Color), `_date` (Date), `_err` (Error), `_cam` (Camera), `_mic` (Microphone), `_mcl` (MovieClipLoader), `_pj` (PrintJob), `_cm` (ContextMenu), `_cmi` (ContextMenuItem).",
                "Comment Type Directives: Place `// MovieClip hero;` or `/* flash.display.Sprite enemy; */` in frame scripts to activate full member completion on untyped variables.",
                "Actions Toolbox & Code Hints (F9): The Actions panel left toolbox and `⌃Space` completion engine query this exact audited AS2/AS3 declaration catalog plus your project's `.as` / `.ts` source files in real time.",
            ],
        );
    }

    sub_bullet(
        ui,
        "Switch Document Script Language",
        Some("⌘J"),
        "Open Document ▸ Document Settings… (⌘J) or the Actions panel header selector to choose ActionScript 3.0, ActionScript 2.0, JavaScript, or TypeScript.",
    );

    super::help_manual::inline_cross_links(
        ui,
        nav_target,
        &[
            (9, "10. Scripting, Import & Publish"),
            (10, "11. AS1 & AS2 Fundamentals & Timeline"),
            (12, "13. AS3 Architecture & Display List"),
            (13, "14. AS3 Events, Input & Lifecycle"),
        ],
    );
}

// ── Full NF2-Styled & SVG-Illustrated Browser HTML Generator ──

fn write_palette_css_vars(p: &Palette, out: &mut String) {
    let vars = [
        ("--bg-app", p.bg_app),
        ("--bg-panel", p.bg_panel),
        ("--bg-raised", p.bg_raised),
        ("--bg-field", p.bg_field),
        ("--bg-hover", p.bg_hover),
        ("--bg-selected", p.bg_selected),
        ("--bg-canvas", p.bg_canvas),
        ("--bg-stage", p.bg_stage),
        ("--border-subtle", p.border_subtle),
        ("--border-default", p.border_default),
        ("--border-strong", p.border_strong),
        ("--text-primary", p.text_primary),
        ("--text-secondary", p.text_secondary),
        ("--text-tertiary", p.text_tertiary),
        ("--text-on-accent", p.text_on_accent),
        ("--accent-default", p.accent_default),
        ("--accent-hover", p.accent_hover),
        ("--accent-subtle", p.accent_subtle),
        ("--warning-default", p.warning_default),
        ("--warning-subtle", p.warning_subtle),
        ("--success-default", p.success_default),
        ("--danger-default", p.danger_default),
        ("--guide", p.guide),
        ("--tl-static", p.timeline_static),
        ("--tl-motion", p.timeline_motion_tween),
        ("--tl-classic", p.timeline_classic_tween),
        ("--tl-shape", p.timeline_shape_tween),
        ("--syn-fn", p.syntax_function),
        ("--syn-kw", p.syntax_keyword),
        ("--syn-ty", p.syntax_type),
        ("--syn-str", p.syntax_string),
        ("--syn-cmt", p.syntax_comment),
    ];
    for (name, color) in vars {
        let _ = write!(out, "{name}:{};", css_rgb(color));
    }
}

fn write_chapter_svg_illustration(index: usize, out: &mut String) {
    let _ = write!(
        out,
        "<figure class=\"diagram-card\"><svg class=\"chapter-svg\" viewBox=\"0 0 680 136\" role=\"img\" aria-label=\"Chapter {} illustration\">",
        index + 1
    );
    out.push_str("<rect x=\"1\" y=\"1\" width=\"678\" height=\"134\" rx=\"8\" fill=\"var(--bg-canvas)\" stroke=\"var(--border-subtle)\"/>");
    match index {
        0 => {
            out.push_str(
                "<rect x=\"48\" y=\"26\" width=\"584\" height=\"94\" fill=\"var(--bg-stage)\" stroke=\"var(--border-default)\"/>\
                 <rect x=\"12\" y=\"8\" width=\"620\" height=\"18\" fill=\"var(--bg-panel)\"/>\
                 <rect x=\"12\" y=\"26\" width=\"36\" height=\"94\" fill=\"var(--bg-panel)\"/>\
                 <line x1=\"48\" y1=\"68\" x2=\"632\" y2=\"68\" stroke=\"var(--guide)\" stroke-width=\"1.5\"/>\
                 <line x1=\"260\" y1=\"26\" x2=\"260\" y2=\"120\" stroke=\"var(--guide)\" stroke-width=\"1.5\"/>\
                 <rect x=\"218\" y=\"46\" width=\"84\" height=\"44\" rx=\"4\" fill=\"var(--accent-subtle)\" stroke=\"var(--accent-default)\" stroke-width=\"1.5\"/>\
                 <circle cx=\"260\" cy=\"68\" r=\"6\" fill=\"none\" stroke=\"var(--accent-default)\" stroke-width=\"2\"/>\
                 <text x=\"316\" y=\"64\" fill=\"var(--text-primary)\" font-size=\"11\" font-family=\"monospace\">Stage + Rulers (0,0) + Guides + Snap Ring</text>",
            );
        }
        1 => {
            for (i, label) in [
                "1. Merge Shape",
                "2. Object (J)",
                "3. Primitive",
                "4. Symbol (F8)",
                "5. Text Block",
            ]
            .iter()
            .enumerate()
            {
                let x = 16 + i * 130;
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"18\" width=\"118\" height=\"76\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--border-subtle)\"/>\
                     <rect x=\"{}\" y=\"32\" width=\"86\" height=\"44\" rx=\"3\" fill=\"var(--accent-subtle)\" stroke=\"var(--accent-default)\"/>\
                     <text x=\"{}\" y=\"114\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\">{label}</text>",
                    x + 16,
                    x + 59
                );
            }
        }
        2 => {
            out.push_str(
                "<path d=\"M 44 96 Q 170 18 296 96\" fill=\"none\" stroke=\"var(--accent-default)\" stroke-width=\"2.5\"/>\
                 <circle cx=\"170\" cy=\"56\" r=\"5\" fill=\"var(--warning-default)\"/>\
                 <line x1=\"340\" y1=\"12\" x2=\"340\" y2=\"124\" stroke=\"var(--border-subtle)\"/>\
                 <line x1=\"400\" y1=\"92\" x2=\"510\" y2=\"44\" stroke=\"var(--accent-default)\" stroke-width=\"2\"/>\
                 <line x1=\"510\" y1=\"44\" x2=\"620\" y2=\"92\" stroke=\"var(--accent-default)\" stroke-width=\"2\"/>\
                 <line x1=\"456\" y1=\"24\" x2=\"564\" y2=\"64\" stroke=\"var(--guide)\" stroke-width=\"1.2\"/>\
                 <rect x=\"506\" y=\"40\" width=\"8\" height=\"8\" fill=\"var(--accent-default)\"/>\
                 <text x=\"170\" y=\"120\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\">Selection (V) Curve Drag vs Subselection (A) Bézier Nodes</text>\
                 <text x=\"510\" y=\"120\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\">Free Transform (Q) · Scale · Rotate · Distort · Envelope</text>",
            );
        }
        3 => {
            for (i, state) in [
                "1. Pen x (Start)",
                "2. Pen (Next)",
                "3. Pen + (Add)",
                "4. Pen - (Del)",
                "5. Pen o (Close)",
                "6. Convert (^)",
            ]
            .iter()
            .enumerate()
            {
                let col = i % 3;
                let row = i / 3;
                let x = 20 + col * 216;
                let y = 18 + row * 54;
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"{y}\" width=\"200\" height=\"42\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--border-default)\"/>\
                     <text x=\"{}\" y=\"{}\" fill=\"var(--text-primary)\" font-size=\"11\" font-family=\"monospace\">{state}</text>",
                    x + 14,
                    y + 25
                );
            }
        }
        4 => {
            out.push_str(
                "<rect x=\"20\" y=\"22\" width=\"190\" height=\"92\" rx=\"6\" fill=\"var(--bg-panel)\" stroke=\"var(--border-subtle)\"/>\
                 <path d=\"M 42 84 C 82 32, 138 104, 188 48\" fill=\"none\" stroke=\"var(--accent-default)\" stroke-width=\"10\" stroke-linecap=\"round\"/>\
                 <circle cx=\"336\" cy=\"68\" r=\"40\" fill=\"var(--accent-subtle)\" stroke=\"var(--accent-default)\" stroke-width=\"1.5\"/>\
                 <line x1=\"296\" y1=\"68\" x2=\"376\" y2=\"68\" stroke=\"var(--guide)\" stroke-width=\"1.5\"/>\
                 <rect x=\"448\" y=\"22\" width=\"212\" height=\"92\" rx=\"6\" fill=\"var(--bg-panel)\" stroke=\"var(--border-subtle)\"/>\
                 <text x=\"462\" y=\"48\" fill=\"var(--text-primary)\" font-size=\"11\">5 Brush Modes: Normal · Fills</text>\
                 <text x=\"462\" y=\"68\" fill=\"var(--text-primary)\" font-size=\"11\">Behind · Selection · Inside</text>\
                 <text x=\"462\" y=\"92\" fill=\"var(--text-secondary)\" font-size=\"11\">Gradient Transform (F): 5 Handles</text>",
            );
        }
        5 => {
            for (i, st) in ["Up", "Over", "Down", "Hit"].iter().enumerate() {
                let x = 32 + i * 68;
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"44\" width=\"64\" height=\"36\" fill=\"var(--bg-panel)\" stroke=\"var(--border-default)\"/>\
                     <circle cx=\"{}\" cy=\"62\" r=\"4\" fill=\"var(--accent-default)\"/>\
                     <text x=\"{}\" y=\"34\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\">{st}</text>",
                    x + 32,
                    x + 32
                );
            }
            out.push_str(
                "<rect x=\"388\" y=\"26\" width=\"248\" height=\"84\" rx=\"10\" fill=\"var(--accent-subtle)\" stroke=\"var(--accent-default)\" stroke-width=\"1.5\"/>\
                 <line x1=\"436\" y1=\"20\" x2=\"436\" y2=\"116\" stroke=\"var(--guide)\" stroke-dasharray=\"4 3\"/>\
                 <line x1=\"588\" y1=\"20\" x2=\"588\" y2=\"116\" stroke=\"var(--guide)\" stroke-dasharray=\"4 3\"/>\
                 <line x1=\"380\" y1=\"48\" x2=\"644\" y2=\"48\" stroke=\"var(--guide)\" stroke-dasharray=\"4 3\"/>\
                 <line x1=\"380\" y1=\"88\" x2=\"644\" y2=\"88\" stroke=\"var(--guide)\" stroke-dasharray=\"4 3\"/>\
                 <text x=\"512\" y=\"72\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\" font-family=\"monospace\">9-Slice (scale9Grid)</text>",
            );
        }
        6 => {
            for (row_i, (label, fill_var)) in [
                ("Actions (a)", "var(--tl-static)"),
                ("Motion (◆)", "var(--tl-motion)"),
                ("Classic (→)", "var(--tl-classic)"),
                ("Shape (→)", "var(--tl-shape)"),
            ]
            .iter()
            .enumerate()
            {
                let y = 16 + row_i * 26;
                let _ = write!(
                    out,
                    "<rect x=\"18\" y=\"{y}\" width=\"128\" height=\"22\" fill=\"var(--bg-panel)\"/>\
                     <text x=\"26\" y=\"{}\" fill=\"var(--text-primary)\" font-size=\"11\">{label}</text>\
                     <rect x=\"152\" y=\"{y}\" width=\"508\" height=\"22\" fill=\"{fill_var}\" stroke=\"var(--border-subtle)\"/>\
                     <circle cx=\"166\" cy=\"{}\" r=\"3.5\" fill=\"var(--text-primary)\"/>\
                     <circle cx=\"642\" cy=\"{}\" r=\"3.5\" fill=\"var(--text-primary)\"/>",
                    y + 15,
                    y + 11,
                    y + 11
                );
            }
        }
        7 => {
            out.push_str(
                "<path d=\"M 36 104 Q 124 22 216 100\" fill=\"none\" stroke=\"var(--accent-default)\" stroke-width=\"2\" stroke-dasharray=\"4 3\"/>\
                 <polygon points=\"124,54 130,60 124,66 118,60\" fill=\"var(--warning-default)\"/>\
                 <rect x=\"256\" y=\"32\" width=\"164\" height=\"72\" rx=\"6\" fill=\"var(--accent-subtle)\" stroke=\"var(--accent-default)\"/>\
                 <circle cx=\"256\" cy=\"32\" r=\"9\" fill=\"var(--warning-default)\"/>\
                 <circle cx=\"420\" cy=\"32\" r=\"9\" fill=\"var(--success-default)\"/>\
                 <rect x=\"456\" y=\"24\" width=\"196\" height=\"88\" rx=\"4\" fill=\"var(--bg-panel)\" stroke=\"var(--border-subtle)\"/>\
                 <path d=\"M 468 102 C 520 102, 580 34, 640 34\" fill=\"none\" stroke=\"var(--accent-default)\" stroke-width=\"2\"/>\
                 <text x=\"338\" y=\"72\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\">Shape Hints (a-z) &amp; Ease Curve</text>",
            );
        }
        8 => {
            out.push_str(
                "<rect x=\"24\" y=\"24\" width=\"632\" height=\"88\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--border-default)\"/>\
                 <polyline points=\"36,96 140,38 510,44 644,94\" fill=\"none\" stroke=\"var(--accent-default)\" stroke-width=\"2\"/>\
                 <rect x=\"136\" y=\"34\" width=\"8\" height=\"8\" fill=\"var(--accent-default)\"/>\
                 <rect x=\"506\" y=\"40\" width=\"8\" height=\"8\" fill=\"var(--accent-default)\"/>\
                 <text x=\"340\" y=\"72\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\">7 Live Filters · 14 Blend Modes · TLF Multi-Column Text · 8-Point Stereo Sound Envelope</text>",
            );
        }
        9 => {
            out.push_str(
                "<rect x=\"24\" y=\"26\" width=\"196\" height=\"84\" rx=\"6\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"122\" y=\"58\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"12\">FLA / XFL / SWF / PSD</text>\
                 <text x=\"122\" y=\"80\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"11\">Importer + Diagnostics</text>\
                 <rect x=\"242\" y=\"26\" width=\"196\" height=\"84\" rx=\"6\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"340\" y=\"58\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"12\">AS2 · AS3 · JS · TS</text>\
                 <text x=\"340\" y=\"80\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"11\">Compiler + Debugger</text>\
                 <rect x=\"460\" y=\"26\" width=\"196\" height=\"84\" rx=\"6\" fill=\"var(--accent-subtle)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"558\" y=\"58\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"12\">HTML5 + WASM + Atlas</text>\
                 <text x=\"558\" y=\"80\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"11\">Lottie · SVG · MP4 · WAV</text>",
            );
        }
        10 => {
            out.push_str(
                "<rect x=\"20\" y=\"26\" width=\"194\" height=\"84\" rx=\"6\" fill=\"var(--bg-panel)\" stroke=\"var(--border-default)\"/>\
                 <text x=\"32\" y=\"50\" fill=\"var(--accent-default)\" font-size=\"11\" font-family=\"monospace\">_level0 / _root</text>\
                 <text x=\"32\" y=\"72\" fill=\"var(--text-primary)\" font-size=\"11\">stop(); gotoAndPlay(2);</text>\
                 <text x=\"32\" y=\"92\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">attachMovie(\"Id\", \"n\", d)</text>\
                 <line x1=\"214\" y1=\"68\" x2=\"244\" y2=\"68\" stroke=\"var(--accent-default)\" stroke-width=\"2\"/>\
                 <rect x=\"244\" y=\"26\" width=\"194\" height=\"84\" rx=\"6\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\" stroke-width=\"1.5\"/>\
                 <text x=\"256\" y=\"50\" fill=\"var(--accent-default)\" font-size=\"11\" font-family=\"monospace\">_root.hero_mc (this)</text>\
                 <text x=\"256\" y=\"72\" fill=\"var(--text-primary)\" font-size=\"11\">onClipEvent(enterFrame)</text>\
                 <text x=\"256\" y=\"92\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">this.onRelease = fn;</text>\
                 <line x1=\"438\" y1=\"68\" x2=\"468\" y2=\"68\" stroke=\"var(--warning-default)\" stroke-width=\"2\"/>\
                 <rect x=\"468\" y=\"26\" width=\"192\" height=\"84\" rx=\"6\" fill=\"var(--bg-panel)\" stroke=\"var(--border-default)\"/>\
                 <text x=\"480\" y=\"50\" fill=\"var(--warning-default)\" font-size=\"11\" font-family=\"monospace\">hero_mc.blade_mc</text>\
                 <text x=\"480\" y=\"72\" fill=\"var(--text-primary)\" font-size=\"11\">_parent._x += 4;</text>\
                 <text x=\"480\" y=\"92\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">swapDepths / remove</text>",
            );
        }
        11 => {
            for (i, (head, b1, b2)) in [
                (
                    "Display & Text",
                    "MovieClip · Button",
                    "TextField · Stage · Color",
                ),
                (
                    "flash.geom & Pixels",
                    "BitmapData (draw)",
                    "Matrix · Point · Rect",
                ),
                ("Input & Audio", "Key · Mouse", "Sound · Selection"),
                ("Net & Storage", "LoadVars · XML", "SharedObject.getLocal"),
            ]
            .iter()
            .enumerate()
            {
                let x = 16 + i * 164;
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"22\" width=\"152\" height=\"92\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--border-default)\"/>\
                     <text x=\"{}\" y=\"44\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">{head}</text>\
                     <text x=\"{}\" y=\"68\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">{b1}</text>\
                     <text x=\"{}\" y=\"88\" fill=\"var(--text-secondary)\" font-size=\"10\" font-family=\"monospace\">{b2}</text>",
                    x + 10,
                    x + 10,
                    x + 10
                );
            }
        }
        12 => {
            for (i, cls) in [
                "EventDispatcher",
                "DisplayObject",
                "InteractiveObj",
                "DOC Container",
                "Sprite / MC",
            ]
            .iter()
            .enumerate()
            {
                let x = 14 + i * 132;
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"20\" width=\"120\" height=\"52\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                     <text x=\"{}\" y=\"48\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\" font-family=\"monospace\">{cls}</text>",
                    x + 60
                );
            }
            out.push_str(
                "<rect x=\"14\" y=\"84\" width=\"652\" height=\"34\" rx=\"4\" fill=\"var(--accent-subtle)\"/>\
                 <text x=\"340\" y=\"105\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\" font-family=\"monospace\">addChild · removeChild · getBounds · hitTestPoint · localToGlobal · globalToLocal</text>",
            );
        }
        13 => {
            out.push_str(
                "<line x1=\"40\" y1=\"28\" x2=\"640\" y2=\"28\" stroke=\"var(--guide)\" stroke-width=\"2\"/>\
                 <text x=\"340\" y=\"18\" text-anchor=\"middle\" fill=\"var(--guide)\" font-size=\"11\" font-family=\"monospace\">Phase 1: CAPTURING_PHASE (Stage -&gt; Parent, useCapture=true)</text>\
                 <rect x=\"40\" y=\"42\" width=\"170\" height=\"46\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"125\" y=\"69\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\">1. Stage / Root</text>\
                 <rect x=\"255\" y=\"42\" width=\"170\" height=\"46\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"340\" y=\"69\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\">2. Parent Sprite</text>\
                 <rect x=\"470\" y=\"42\" width=\"170\" height=\"46\" rx=\"5\" fill=\"var(--warning-subtle)\" stroke=\"var(--warning-default)\" stroke-width=\"1.5\"/>\
                 <text x=\"555\" y=\"69\" text-anchor=\"middle\" fill=\"var(--warning-default)\" font-size=\"11\">3. Target (AT_TARGET)</text>\
                 <line x1=\"640\" y1=\"104\" x2=\"40\" y2=\"104\" stroke=\"var(--success-default)\" stroke-width=\"2\"/>\
                 <text x=\"340\" y=\"122\" text-anchor=\"middle\" fill=\"var(--success-default)\" font-size=\"11\" font-family=\"monospace\">Phase 3: BUBBLING_PHASE (Target -&gt; Stage, bubbles=true)</text>",
            );
        }
        14 => {
            out.push_str(
                "<rect x=\"18\" y=\"20\" width=\"204\" height=\"96\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"30\" y=\"44\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">1. Graphics Vector API</text>\
                 <text x=\"30\" y=\"66\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">beginGradientFill / lineStyle</text>\
                 <text x=\"30\" y=\"86\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">curveTo / cubicCurveTo / Path</text>\
                 <rect x=\"238\" y=\"20\" width=\"204\" height=\"96\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--warning-default)\"/>\
                 <text x=\"250\" y=\"44\" fill=\"var(--warning-default)\" font-size=\"11\" font-weight=\"600\">2. Filters &amp; Geometry</text>\
                 <text x=\"250\" y=\"66\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">Blur / Glow / DropShadow</text>\
                 <text x=\"250\" y=\"86\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">Matrix / ColorTransform</text>\
                 <rect x=\"458\" y=\"20\" width=\"204\" height=\"96\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--success-default)\"/>\
                 <text x=\"470\" y=\"44\" fill=\"var(--success-default)\" font-size=\"11\" font-weight=\"600\">3. Sound &amp; ByteArray</text>\
                 <text x=\"470\" y=\"66\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">SoundChannel / SoundMixer</text>\
                 <text x=\"470\" y=\"86\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">ByteArray / JSON / XML E4X</text>",
            );
        }
        _ => {
            out.push_str(
                "<rect x=\"18\" y=\"18\" width=\"204\" height=\"52\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"120\" y=\"40\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">AS1 / AS2 (AVM1)</text>\
                 <text x=\"120\" y=\"58\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"10\" font-family=\"monospace\">_x · _alpha(100) · onRelease</text>\
                 <rect x=\"238\" y=\"18\" width=\"204\" height=\"52\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"340\" y=\"40\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">AS3 (AVM2)</text>\
                 <text x=\"340\" y=\"58\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"10\" font-family=\"monospace\">x · alpha(1.0) · addEventListener</text>\
                 <rect x=\"458\" y=\"18\" width=\"204\" height=\"52\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"560\" y=\"40\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">JavaScript / TypeScript</text>\
                 <text x=\"560\" y=\"58\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"10\" font-family=\"monospace\">mc.x · mc.alpha · on(\"click\")</text>\
                 <rect x=\"18\" y=\"82\" width=\"644\" height=\"36\" rx=\"5\" fill=\"var(--accent-subtle)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"340\" y=\"104\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\" font-weight=\"600\">Shared NeoFlash Rust / WebAssembly Scene Graph &amp; Timeline Engine</text>",
            );
        }
    }
    out.push_str("</svg></figure>");
}

fn write_as_chapter_reference_html(index: usize, out: &mut String) {
    match index {
        10 => {
            out.push_str(
                "<h3>AS1 / AS2 Timeline &amp; Instance Code Example</h3>\
                 <pre class=\"code-block\"><code>stop();\nvar depth:Number = this.getNextHighestDepth();\nvar coin:MovieClip = this.attachMovie(\"CoinSymbol\", \"coin_\" + depth, depth, {_x: 320, _y: 180});\nhero_mc.onEnterFrame = function():Void {\n    if (Key.isDown(Key.RIGHT)) { this._x += 5; }\n    if (this.hitTest(_root.coin._x, _root.coin._y, true)) {\n        _root.coin.removeMovieClip();\n    }\n};</code></pre>",
            );
        }
        11 => {
            out.push_str(
                "<h3>AS2 Procedural Drawing, TextFormat &amp; BitmapData Example</h3>\
                 <pre class=\"code-block\"><code>import flash.display.BitmapData;\nimport flash.geom.Matrix;\nvar canvas:MovieClip = this.createEmptyMovieClip(\"badge_mc\", this.getNextHighestDepth());\ncanvas.lineStyle(2, 0x6C93FF, 100);\ncanvas.beginFill(0x1F2A48, 95);\ncanvas.moveTo(20, 20); canvas.lineTo(180, 20); canvas.lineTo(180, 70); canvas.lineTo(20, 70); canvas.endFill();\nthis.createTextField(\"label_txt\", this.getNextHighestDepth(), 32, 32, 140, 28);\nlabel_txt.text = \"READY PLAYER ONE\";\nlabel_txt.setTextFormat(new TextFormat(\"Inter\", 14, 0xFFFFFF, true));</code></pre>",
            );
        }
        12 => {
            out.push_str(
                "<h3>AS3 Display List &amp; Coordinate Space Example</h3>\
                 <pre class=\"code-block\"><code>package {\n    import flash.display.Sprite;\n    import flash.events.Event;\n    import flash.geom.Point;\n    public class Main extends Sprite {\n        public function Main() {\n            addEventListener(Event.ADDED_TO_STAGE, init);\n        }\n        private function init(e:Event):void {\n            var child:Sprite = new Sprite();\n            child.x = 120; child.y = 80;\n            addChild(child);\n            var globalPt:Point = child.localToGlobal(new Point(0, -18));\n            trace(\"Global position:\", globalPt.x, globalPt.y);\n        }\n    }\n}</code></pre>",
            );
        }
        13 => {
            out.push_str(
                "<h3>AS3 3-Phase Events, Stage Keyboard &amp; Timer Example</h3>\
                 <pre class=\"code-block\"><code>import flash.events.MouseEvent;\nimport flash.events.KeyboardEvent;\nimport flash.events.TimerEvent;\nimport flash.utils.Timer;\ncard_mc.buttonMode = true;\ncard_mc.mouseChildren = false;\ncard_mc.addEventListener(MouseEvent.MOUSE_DOWN, function(e:MouseEvent):void { card_mc.startDrag(); });\nstage.addEventListener(MouseEvent.MOUSE_UP, function(e:MouseEvent):void { card_mc.stopDrag(); });\nvar timer:Timer = new Timer(500, 10);\ntimer.addEventListener(TimerEvent.TIMER, function(e:TimerEvent):void { trace(timer.currentCount); });\ntimer.start();</code></pre>",
            );
        }
        14 => {
            out.push_str(
                "<h3>AS3 Graphics, GlowFilter, SoundChannel &amp; E4X XML Example</h3>\
                 <pre class=\"code-block\"><code>import flash.display.Sprite;\nimport flash.filters.GlowFilter;\nimport flash.media.SoundTransform;\nvar orb:Sprite = new Sprite();\norb.graphics.beginFill(0x3C6BF0, 0.85);\norb.graphics.drawCircle(0, 0, 36);\norb.graphics.endFill();\norb.filters = [new GlowFilter(0x6C93FF, 0.9, 16, 16, 2, 2)];\naddChild(orb);\nvar xml:XML = &lt;items&gt;&lt;weapon id=\"railgun\" dmg=\"90\"/&gt;&lt;/items&gt;;\ntrace(xml.weapon.(Number(@dmg) &gt;= 50).@id);</code></pre>",
            );
        }
        15 => {
            out.push_str(
                "<h3>AS1/AS2 ↔ AS3 ↔ JS/TS Quick Equivalence Table</h3>\
                 <table class=\"ref-table\"><thead><tr><th>AS1 / AS2 (AVM1)</th><th>ActionScript 3.0 (AVM2)</th><th>JavaScript / TypeScript</th></tr></thead><tbody>\
                 <tr><td><code>_x, _y, _rotation, _visible</code></td><td><code>x, y, rotation, visible</code></td><td><code>mc.x, mc.y, mc.rotation, mc.visible</code></td></tr>\
                 <tr><td><code>_xscale, _yscale (0..100)</code></td><td><code>scaleX, scaleY (0.0..1.0)</code></td><td><code>mc.scaleX, mc.scaleY (0.0..1.0)</code></td></tr>\
                 <tr><td><code>_alpha (0..100)</code></td><td><code>alpha (0.0..1.0)</code></td><td><code>mc.alpha (0.0..1.0)</code></td></tr>\
                 <tr><td><code>mc.onRelease = fn</code></td><td><code>mc.addEventListener(MouseEvent.CLICK, fn)</code></td><td><code>mc.addEventListener(\"click\", fn)</code></td></tr>\
                 <tr><td><code>mc.onEnterFrame = fn</code></td><td><code>mc.addEventListener(Event.ENTER_FRAME, fn)</code></td><td><code>mc.addEventListener(\"enterFrame\", fn)</code></td></tr>\
                 <tr><td><code>attachMovie(\"Id\", \"n\", d)</code></td><td><code>addChild(new IdClass())</code></td><td><code>this.addChild(new IdClass())</code></td></tr>\
                 </tbody></table>",
            );
        }
        _ => {}
    }
}

pub(crate) fn render_full_browser_help_html() -> String {
    let hash = char::from(35u8);
    let mut html = String::with_capacity(65_536);
    html.push_str(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <meta name=\"color-scheme\" content=\"dark light\">\
         <title>NeoFlash Help &amp; Reference</title><style>",
    );
    html.push_str(":root,.help-shell{");
    write_palette_css_vars(&DARK, &mut html);
    html.push_str("}@media (prefers-color-scheme: light){:root{");
    write_palette_css_vars(&LIGHT, &mut html);
    html.push_str("}}");
    html.push_str(".theme-toggle-input:checked ~ .help-shell{");
    write_palette_css_vars(&LIGHT, &mut html);
    html.push('}');
    html.push_str(
        "*{box-sizing:border-box}body{margin:0;font-family:Inter,-apple-system,BlinkMacSystemFont,sans-serif;font-size:13px;line-height:1.55;background:var(--bg-app);color:var(--text-primary)}\
         .sr-toggle{position:absolute;opacity:0;pointer-events:none}\
         .help-shell{min-height:100vh;background:var(--bg-app);color:var(--text-primary);display:flex;flex-direction:column}\
         .top-header{position:sticky;top:0;z-index:10;background:var(--bg-raised);border-bottom:1px solid var(--border-subtle);padding:12px 24px;display:flex;align-items:center;justify-content:space-between;gap:16px;flex-wrap:wrap}\
         .top-header h1{margin:0;font-size:16px;font-weight:600;color:var(--text-primary)}\
         .top-header p{margin:2px 0 0;font-size:12px;color:var(--text-secondary)}\
         .theme-btn{cursor:pointer;padding:6px 12px;border-radius:6px;background:var(--bg-panel);border:1px solid var(--border-default);color:var(--text-primary);font-size:12px;font-weight:500;user-select:none}\
         .theme-btn:hover{background:var(--bg-hover);border-color:var(--border-strong)}\
         .tag-bar{background:var(--bg-panel);border-bottom:1px solid var(--border-subtle);padding:8px 24px;display:flex;align-items:center;gap:6px;flex-wrap:wrap}\
         .tag-bar-label{font-size:11px;font-weight:600;text-transform:uppercase;letter-spacing:0.06em;color:var(--text-tertiary);margin-right:4px}\
         .tag-pill{cursor:pointer;padding:3px 10px;border-radius:999px;background:var(--bg-raised);border:1px solid var(--border-subtle);color:var(--text-secondary);font-size:11px;font-weight:500;user-select:none;text-decoration:none}\
         .tag-pill:hover{background:var(--bg-hover);color:var(--text-primary)}\
         .layout{display:grid;grid-template-columns:270px 1fr;gap:24px;max-width:1360px;width:100%;margin:0 auto;padding:20px 24px}\
         @media (max-width:860px){.layout{grid-template-columns:1fr}}\
         nav.sidebar{background:var(--bg-panel);border:1px solid var(--border-subtle);border-radius:8px;padding:12px;align-self:start;position:sticky;top:76px;max-height:calc(100vh - 96px);overflow-y:auto}\
         nav.sidebar ol{list-style:none;margin:0;padding:0;display:flex;flex-direction:column;gap:3px}\
         nav.sidebar a{display:block;padding:6px 10px;border-radius:6px;color:var(--text-primary);text-decoration:none;font-size:12px;font-weight:500}\
         nav.sidebar a:hover{background:var(--bg-hover);color:var(--accent-default)}\
         main{display:flex;flex-direction:column;gap:20px;min-width:0}\
         section.chapter-card{background:var(--bg-panel);border:1px solid var(--border-subtle);border-radius:8px;padding:20px;scroll-margin-top:84px}\
         section.chapter-card:target{border-color:var(--accent-default)}\
         .chapter-head{display:flex;align-items:baseline;justify-content:space-between;gap:12px;flex-wrap:wrap;border-bottom:1px solid var(--border-subtle);padding-bottom:10px;margin-bottom:12px}\
         .chapter-head h2{margin:0;font-size:16px;font-weight:600;color:var(--text-primary)}\
         .chapter-sub{margin:4px 0 10px;color:var(--text-secondary);font-size:12px}\
         .chapter-tags{display:flex;gap:6px;flex-wrap:wrap;margin-bottom:12px}\
         .badge{display:inline-flex;align-items:center;gap:5px;padding:2px 8px;border-radius:999px;background:var(--accent-subtle);color:var(--accent-default);font-size:11px;font-weight:500;cursor:pointer}\
         .diagram-card{margin:12px 0;padding:0}\
         .chapter-svg{width:100%;height:auto;display:block;border-radius:8px}\
         ul.topic-list{margin:10px 0;padding-left:20px;color:var(--text-primary)}\
         ul.topic-list li{margin-bottom:6px}\
         pre.code-block{background:var(--bg-field);border:1px solid var(--border-subtle);border-radius:6px;padding:12px;overflow-x:auto;font-family:'Roboto Mono',ui-monospace,monospace;font-size:11.5px;line-height:1.45;color:var(--text-primary)}\
         table.ref-table{width:100%;border-collapse:collapse;margin:10px 0;font-size:12px}\
         table.ref-table th,table.ref-table td{text-align:left;padding:7px 10px;border-bottom:1px solid var(--border-subtle)}\
         table.ref-table th{color:var(--accent-default);font-weight:600;background:var(--bg-raised)}\
         .cross-links{display:flex;align-items:center;gap:8px;flex-wrap:wrap;margin-top:14px;padding-top:10px;border-top:1px solid var(--border-subtle)}\
         .cross-link{display:inline-block;padding:4px 10px;border-radius:6px;background:var(--bg-raised);border:1px solid var(--border-default);color:var(--accent-default);text-decoration:none;font-size:11.5px;font-weight:500}\
         .cross-link:hover{background:var(--accent-subtle);border-color:var(--accent-default)}\
         .search-terms{margin-top:8px;font-size:11px;color:var(--text-tertiary)}",
    );

    // Pure-CSS tag filter rules using :has() so selecting a tag radio highlights the pill and filters chapters
    for tag in HELP_ALL_TAGS {
        let _ = write!(
            html,
            ".help-shell:has(input[data-tag=\"{tag}\"]:checked) label[data-tag-pill=\"{tag}\"]{{background:var(--accent-subtle);color:var(--accent-default);border-color:var(--accent-default)}}\
             .help-shell:has(input[data-tag=\"{tag}\"]:checked) section.chapter-card:not([data-tags~=\"{tag}\"]){{display:none}}\
             .help-shell:has(input[data-tag=\"{tag}\"]:checked) nav.sidebar li:not([data-tags~=\"{tag}\"]){{display:none}}"
        );
    }
    html.push_str(
        ".help-shell:has(input[data-tag=\"ALL\"]:checked) label[data-tag-pill=\"ALL\"]{background:var(--accent-subtle);color:var(--accent-default);border-color:var(--accent-default)}",
    );
    html.push_str("</style></head><body>");

    // Theme checkbox + Tag filter radio inputs (100% declarative CSS state)
    html.push_str(
        "<input type=\"checkbox\" id=\"nf2-theme-toggle\" class=\"sr-toggle theme-toggle-input\">\
         <div class=\"help-shell\">\
         <input type=\"radio\" name=\"nf2-tag\" id=\"nf2-tag-ALL\" data-tag=\"ALL\" class=\"sr-toggle\" checked>",
    );
    for tag in HELP_ALL_TAGS {
        let _ = write!(
            html,
            "<input type=\"radio\" name=\"nf2-tag\" id=\"nf2-tag-{tag}\" data-tag=\"{tag}\" class=\"sr-toggle\">"
        );
    }

    html.push_str(
        "<header class=\"top-header\" id=\"top\"><div>\
         <h1>NeoFlash Help &amp; Reference</h1>\
         <p>Illustrated NF2 manual &amp; ActionScript 1.0 / 2.0 / 3.0 Language Reference.</p>\
         </div>\
         <label for=\"nf2-theme-toggle\" class=\"theme-btn\">Switch Dark / Light Theme</label>\
         </header>",
    );

    // Interactive Tag Filter Bar
    html.push_str("<div class=\"tag-bar\" role=\"toolbar\" aria-label=\"Filter by topic tag\"><span class=\"tag-bar-label\">Filter by Tag:</span>");
    html.push_str(
        "<label for=\"nf2-tag-ALL\" data-tag-pill=\"ALL\" class=\"tag-pill\">All (16)</label>",
    );
    for tag in HELP_ALL_TAGS {
        let _ = write!(
            html,
            "<label for=\"nf2-tag-{tag}\" data-tag-pill=\"{tag}\" class=\"tag-pill\">{tag}</label>"
        );
    }
    html.push_str("</div>");

    html.push_str("<div class=\"layout\"><nav class=\"sidebar\" aria-label=\"Chapters\"><ol>");
    for (index, chapter) in HELP_CHAPTERS.iter().enumerate() {
        let tags_joined = chapter.tags.join(" ");
        let _ = write!(
            html,
            "<li data-tags=\"{tags_joined}\"><a href=\"{hash}chapter-{}\">",
            index + 1
        );
        super::help_manual::escape_html(chapter.title, &mut html);
        html.push_str("</a></li>");
    }
    html.push_str("</ol></nav><main>");

    for (index, chapter) in HELP_CHAPTERS.iter().enumerate() {
        let tags_joined = chapter.tags.join(" ");
        let _ = write!(
            html,
            "<section class=\"chapter-card\" id=\"chapter-{}\" data-tags=\"{tags_joined}\"><div class=\"chapter-head\"><h2>",
            index + 1
        );
        super::help_manual::escape_html(chapter.title, &mut html);
        let _ = write!(
            html,
            "</h2><span class=\"badge\">Chapter {} of {}</span></div><p class=\"chapter-sub\">",
            index + 1,
            HELP_CHAPTERS.len()
        );
        super::help_manual::escape_html(chapter.subtitle, &mut html);
        html.push_str("</p><div class=\"chapter-tags\">");
        for tag in chapter.tags {
            let _ = write!(
                html,
                "<label for=\"nf2-tag-{tag}\" class=\"badge\" title=\"Filter by {tag}\">{tag}</label>"
            );
        }
        html.push_str("</div>");

        // Crisp inline SVG diagram for every chapter
        write_chapter_svg_illustration(index, &mut html);

        html.push_str("<ul class=\"topic-list\">");
        for topic in HELP_BROWSER_TOPICS[index] {
            html.push_str("<li>");
            super::help_manual::escape_html(topic, &mut html);
            html.push_str("</li>");
        }
        html.push_str("</ul>");

        // Extended ActionScript code blocks & tables for chapters 11..=16
        write_as_chapter_reference_html(index, &mut html);

        // Interactive Cross-Links to related chapters
        html.push_str("<div class=\"cross-links\"><span class=\"tag-bar-label\">See also:</span>");
        for &rel_idx in chapter.related {
            if let Some(rel_ch) = HELP_CHAPTERS.get(rel_idx) {
                let _ = write!(
                    html,
                    "<a class=\"cross-link\" href=\"{hash}chapter-{}\">→ ",
                    rel_idx + 1
                );
                super::help_manual::escape_html(rel_ch.title, &mut html);
                html.push_str("</a>");
            }
        }
        let _ = write!(
            html,
            "<a class=\"cross-link\" href=\"{hash}top\">↑ Top</a></div>"
        );

        html.push_str("<p class=\"search-terms\"><strong>Search terms:</strong> ");
        super::help_manual::escape_html(chapter.keywords, &mut html);
        html.push_str("</p></section>");
    }
    html.push_str("</main></div></div></body></html>");
    html
}
