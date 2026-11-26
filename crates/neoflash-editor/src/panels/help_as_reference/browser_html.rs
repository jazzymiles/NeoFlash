use super::*;
use crate::panels::help_manual::{
    AUTHORING_CHAPTER_COUNT, HELP_BROWSER_TOPICS, HELP_CHAPTERS, LanguageScope,
};
use neoflash_ui::{Palette, Theme};
use std::fmt::Write as _;

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

fn convert_svg_var_attrs_to_css_style(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 512);
    let mut rest = raw;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let after_lt = &rest[lt..];
        let Some(gt_rel) = after_lt.find('>') else {
            out.push_str(after_lt);
            return out;
        };
        let tag = &after_lt[..=gt_rel];
        rest = &after_lt[gt_rel + 1..];
        if tag.starts_with("</")
            || tag.starts_with("<!")
            || (!tag.contains("fill=\"var(") && !tag.contains("stroke=\"var("))
        {
            out.push_str(tag);
            continue;
        }
        let self_closing = tag.ends_with("/>");
        let inner_end = if self_closing {
            tag.len() - 2
        } else {
            tag.len() - 1
        };
        let mut inner = tag[1..inner_end].to_owned();
        let mut styles: Vec<String> = Vec::new();
        for attr in ["fill", "stroke"] {
            let needle = format!("{attr}=\"var(");
            if let Some(start) = inner.find(&needle) {
                let val_start = start + attr.len() + 2;
                if let Some(val_end_rel) = inner[val_start..].find('"') {
                    let val = inner[val_start..val_start + val_end_rel].to_owned();
                    styles.push(format!("{attr}:{val}"));
                    let remove_end = val_start + val_end_rel + 1;
                    let remove_start = if start > 0 && inner.as_bytes()[start - 1] == b' ' {
                        start - 1
                    } else {
                        start
                    };
                    inner.replace_range(remove_start..remove_end, "");
                }
            } else if attr == "fill"
                && let Some(start) = inner.find("fill=\"none\"")
            {
                styles.push("fill:none".to_owned());
                let remove_end = start + "fill=\"none\"".len();
                let remove_start = if start > 0 && inner.as_bytes()[start - 1] == b' ' {
                    start - 1
                } else {
                    start
                };
                inner.replace_range(remove_start..remove_end, "");
            }
        }
        out.push('<');
        out.push_str(inner.trim_end());
        if !styles.is_empty() {
            out.push_str(" style=\"");
            out.push_str(&styles.join(";"));
            out.push('"');
        }
        if self_closing {
            out.push_str("/>");
        } else {
            out.push('>');
        }
    }
    out.push_str(rest);
    out
}

fn write_chapter_svg_illustration(index: usize, out: &mut String) {
    let svg_start = out.len();
    let caption = match index {
        0 => {
            "Illustration 1.1: Stage inside Pasteboard overlaid with NF2 Rulers, Ruler Corner (0,0), Cyan Guides, Stage Grid, and Selection Snap Ring."
        }
        1 => {
            "Illustration 2.1: Visual comparison of selection chrome across the 5 Stage object regimes: Merge Shape, Object (J), Primitive, Symbol (F8), and Text Block."
        }
        2 => {
            "Illustration 3.1–3.3: Selection (V) curve reshaping, Subselection (A) Bézier anchor/tangent handles, Free Transform (Q) 4 sub-modes, and Lasso / Magic Wand."
        }
        3 => {
            "Illustration 4.1: All 9 Pen Tool (P) pointer badges and how each modifies the active or selected Bézier path."
        }
        4 => {
            "Illustration 5.1: NF2 Brush calligraphic stroke, Gradient Transform Tool (F) 5 handles, and the 5 Brush Paint Modes."
        }
        5 => {
            "Illustration 6.1: Button Symbol 4 interaction frames (Up, Over, Down, Hit) and 9-Slice Scaling (scale9Grid) guides."
        }
        6 => {
            "Illustration 7.1: Authentic NF2 Timeline spans (Static, Motion ◆, Classic, Shape) and keyframe markers."
        }
        7 => {
            "Illustration 8.1: Stage Motion Path with ◆ property keyframes, Shape Hints (a–z), and Cubic-Bézier Ease Curve."
        }
        8 => {
            "Illustration 9.1: Timeline Sound stereo waveform with 8-handle volume envelope and live filter / TLF text pipeline."
        }
        9 => {
            "Illustration 10.1: Multi-format Import pipeline, 4-language Scripting compiler/debugger, and HTML5 + WASM + Sprite Sheet publishing."
        }
        10 => {
            "Illustration 11.1–11.2: AVM1 Target Path Hierarchy (_level0 / _root → parent → child) and Flash Player Depth Zones (-16383..1048575)."
        }
        11 => {
            "Illustration 12.1: The 4 pillars of the ActionScript 2.0 runtime library: Display & Text, flash.geom & BitmapData, Input & Audio, and Data/Persistence."
        }
        12 => {
            "Illustration 13.1–13.2: ActionScript 3.0 flash.display 2-tier Inheritance Hierarchy (EventDispatcher → DisplayObject → InteractiveObject → DisplayObjectContainer → Sprite → MovieClip) and Local ↔ Global coordinate spaces."
        }
        13 => {
            "Illustration 14.1–14.2: W3C 3-Phase Event Propagation (Capture → Target → Bubbling) and deterministic 6-step Frame Tick Lifecycle."
        }
        14 => {
            "Illustration 15.1: AS3 flash.display.Graphics vector stream, live Filters/BitmapData pass, and SoundChannel / ByteArray / E4X pipeline."
        }
        15 => {
            "Illustration 16.1: Multi-Language Execution Bridge mapping AS1/AS2, AS3, JavaScript, and TypeScript to the shared Rust / WebAssembly runtime."
        }
        16 => {
            "Illustration 17.1–17.2: JavaScript execution contexts (Frame `this`, Object `event`, Project Module `import`/`export`) and deterministic JSX (`__nfJsx`) lowering."
        }
        17 => {
            "Illustration 18.1–18.2: NeoFlash TypeScript compilation pipeline (Parse → Built-In `ts_check` → Strip & Lower → SourceMap) and verified vs bounded checker scope."
        }
        _ => {
            "Illustration 19.1–19.2: Published HTML5 Browser Host architecture (DOM, CanvasRenderingContext2D, fetch, Web Audio, localStorage) and CORS/CSP/Autoplay security boundaries."
        }
    };
    let view_h = if index == 12 { 156 } else { 136 };
    let _ = write!(
        out,
        "<figure class=\"diagram-card\"><svg xmlns=\"http://www.w3.org/2000/svg\" class=\"chapter-svg\" width=\"680\" height=\"{view_h}\" viewBox=\"0 0 680 {view_h}\" role=\"img\" aria-label=\"Chapter {} illustration\">",
        index + 1
    );
    let _ = write!(
        out,
        "<rect x=\"1\" y=\"1\" width=\"678\" height=\"{}\" rx=\"8\" fill=\"var(--bg-canvas)\" stroke=\"var(--border-subtle)\"/>",
        view_h - 2
    );
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
            // 2-tier non-overlapping AS3 Display List Hierarchy + 4 leaf pills
            for (i, (cls, mem)) in [
                ("1. EventDispatcher", "addEventListener / dispatchEvent"),
                ("2. DisplayObject", "x, y, scaleX, rotation, alpha"),
                ("3. InteractiveObject", "mouseEnabled, tabEnabled"),
            ]
            .iter()
            .enumerate()
            {
                let x = 14 + i * 218;
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"12\" width=\"206\" height=\"42\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--border-default)\"/>\
                     <text x=\"{}\" y=\"29\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">{cls}</text>\
                     <text x=\"{}\" y=\"45\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"9.5\" font-family=\"monospace\">{mem}</text>",
                    x + 103,
                    x + 103
                );
            }
            for (i, (cls, mem)) in [
                (
                    "4. DisplayObjectContainer",
                    "addChild, removeChild, numChildren",
                ),
                (
                    "5. Sprite → MovieClip",
                    "graphics, buttonMode, currentFrame",
                ),
            ]
            .iter()
            .enumerate()
            {
                let x = 14 + i * 332;
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"62\" width=\"320\" height=\"42\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\" stroke-width=\"1.4\"/>\
                     <text x=\"{}\" y=\"79\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">{cls}</text>\
                     <text x=\"{}\" y=\"95\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"9.5\" font-family=\"monospace\">{mem}</text>",
                    x + 160,
                    x + 160
                );
            }
            for (i, leaf) in [
                "Shape (vector leaf)",
                "Bitmap (BitmapData)",
                "TextField (text/input)",
                "SimpleButton (4-state)",
            ]
            .iter()
            .enumerate()
            {
                let x = 14 + i * 166;
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"114\" width=\"154\" height=\"28\" rx=\"4\" fill=\"var(--accent-subtle)\"/>\
                     <text x=\"{}\" y=\"132\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">{leaf}</text>",
                    x + 77
                );
            }
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
        15 => {
            out.push_str(
                "<rect x=\"18\" y=\"18\" width=\"204\" height=\"52\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"120\" y=\"40\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">AS1 / AS2 (AVM1)</text>\
                 <text x=\"120\" y=\"58\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"10\" font-family=\"monospace\">_x · _alpha(100) · onRelease</text>\
                 <rect x=\"238\" y=\"18\" width=\"204\" height=\"52\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"340\" y=\"40\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">AS3 (AVM2)</text>\
                 <text x=\"340\" y=\"58\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"10\" font-family=\"monospace\">x · alpha(1.0) · addEventListener</text>\
                 <rect x=\"458\" y=\"18\" width=\"204\" height=\"52\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"560\" y=\"40\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">JavaScript / TypeScript</text>\
                 <text x=\"560\" y=\"58\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"10\" font-family=\"monospace\">this · mc.x · addEventListener</text>\
                 <rect x=\"18\" y=\"82\" width=\"644\" height=\"36\" rx=\"5\" fill=\"var(--accent-subtle)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"340\" y=\"104\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\" font-weight=\"600\">Shared NeoFlash Rust / WebAssembly Scene Graph &amp; Timeline Engine</text>",
            );
        }
        16 => {
            out.push_str(
                "<rect x=\"18\" y=\"18\" width=\"204\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"30\" y=\"38\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">1. Frame &amp; Object Context</text>\
                 <text x=\"30\" y=\"56\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">this · event · trace()</text>\
                 <text x=\"30\" y=\"74\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">play / stop / gotoAndPlay</text>\
                 <rect x=\"238\" y=\"18\" width=\"204\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--warning-default)\"/>\
                 <text x=\"250\" y=\"38\" fill=\"var(--warning-default)\" font-size=\"11\" font-weight=\"600\">2. Project Modules &amp; JSX</text>\
                 <text x=\"250\" y=\"56\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">import / export (./module)</text>\
                 <text x=\"250\" y=\"74\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">&lt;Tag/&gt; -&gt; __nfJsx(...)</text>\
                 <rect x=\"458\" y=\"18\" width=\"204\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--success-default)\"/>\
                 <text x=\"470\" y=\"38\" fill=\"var(--success-default)\" font-size=\"11\" font-weight=\"600\">3. ES2023+ &amp; Async</text>\
                 <text x=\"470\" y=\"56\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">Promise · async / await</text>\
                 <text x=\"470\" y=\"74\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">Map · Set · JSON · Math</text>\
                 <rect x=\"18\" y=\"94\" width=\"644\" height=\"28\" rx=\"4\" fill=\"var(--accent-subtle)\"/>\
                 <text x=\"340\" y=\"112\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"10.5\" font-family=\"monospace\">Provenance: [NeoFlash binding] · [Project module] · [ECMAScript] · [Browser host]</text>",
            );
        }
        17 => {
            out.push_str(
                "<rect x=\"16\" y=\"18\" width=\"152\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"26\" y=\"40\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">1. Parse (.ts/.tsx)</text>\
                 <text x=\"26\" y=\"60\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">interface / type / enum</text>\
                 <rect x=\"180\" y=\"18\" width=\"152\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--warning-default)\"/>\
                 <text x=\"190\" y=\"40\" fill=\"var(--warning-default)\" font-size=\"11\" font-weight=\"600\">2. Built-In ts_check</text>\
                 <text x=\"190\" y=\"60\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">Members / Unions / Arity</text>\
                 <rect x=\"344\" y=\"18\" width=\"152\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--success-default)\"/>\
                 <text x=\"354\" y=\"40\" fill=\"var(--success-default)\" font-size=\"11\" font-weight=\"600\">3. Strip &amp; Lower</text>\
                 <text x=\"354\" y=\"60\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">Erase types + lower enum</text>\
                 <rect x=\"508\" y=\"18\" width=\"154\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"518\" y=\"40\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">4. SourceMap</text>\
                 <text x=\"518\" y=\"60\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">1-based line &amp; col</text>\
                 <rect x=\"16\" y=\"94\" width=\"646\" height=\"28\" rx=\"4\" fill=\"var(--accent-subtle)\"/>\
                 <text x=\"340\" y=\"112\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"10.5\" font-family=\"monospace\">Utilities: Array&lt;T&gt; · Promise&lt;T&gt; · Record&lt;K,V&gt; · Partial&lt;T&gt; · Pick&lt;T,K&gt; · Omit&lt;T,K&gt;</text>",
            );
        }
        _ => {
            out.push_str(
                "<rect x=\"18\" y=\"18\" width=\"204\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"30\" y=\"38\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">1. DOM &amp; Canvas 2D</text>\
                 <text x=\"30\" y=\"56\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">CanvasRenderingContext2D</text>\
                 <text x=\"30\" y=\"74\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">fillRect · arc · drawImage</text>\
                 <rect x=\"238\" y=\"18\" width=\"204\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--warning-default)\"/>\
                 <text x=\"250\" y=\"38\" fill=\"var(--warning-default)\" font-size=\"11\" font-weight=\"600\">2. Fetch &amp; Storage</text>\
                 <text x=\"250\" y=\"56\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">await fetch(url) (CORS)</text>\
                 <text x=\"250\" y=\"74\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">localStorage.getItem/setItem</text>\
                 <rect x=\"458\" y=\"18\" width=\"204\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--success-default)\"/>\
                 <text x=\"470\" y=\"38\" fill=\"var(--success-default)\" font-size=\"11\" font-weight=\"600\">3. Web Audio &amp; Timers</text>\
                 <text x=\"470\" y=\"56\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">AudioContext (user gesture)</text>\
                 <text x=\"470\" y=\"74\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">requestAnimationFrame</text>\
                 <rect x=\"18\" y=\"94\" width=\"644\" height=\"28\" rx=\"4\" fill=\"var(--warning-subtle)\"/>\
                 <text x=\"340\" y=\"112\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"10.5\" font-family=\"monospace\">Browser Boundaries: HTTP/HTTPS CORS · CSP connect-src · Autoplay Gesture · ~5 MB Origin Quota</text>",
            );
        }
    }
    out.push_str("</svg><figcaption class=\"diagram-caption\">");
    crate::panels::help_manual::escape_html(caption, out);
    out.push_str("</figcaption></figure>");
    let normalized = convert_svg_var_attrs_to_css_style(&out[svg_start..]);
    out.replace_range(svg_start.., &normalized);
}

fn write_support_badge_html(support: Option<Support>, status_raw: &str, out: &mut String) {
    match (support, status_raw) {
        (Some(Support::Working), _) => {
            out.push_str("<span class=\"status-working\">Working</span>");
        }
        (Some(Support::Partial), _) => {
            out.push_str("<span class=\"status-partial\">Partial</span>");
        }
        (Some(Support::Missing), _) => {
            out.push_str("<span class=\"status-missing\">Missing</span>");
        }
        (None, "members") => {
            out.push_str("<span class=\"status-missing\">Members only</span>");
        }
        _ => {
            out.push_str("<span class=\"status-missing\">Indexed</span>");
        }
    }
}

fn write_js_ts_entries_html(
    heading: &str,
    categories: &[&str],
    include_scope_summary: bool,
    out: &mut String,
) {
    let index = api_index(LanguageScope::JsTs);
    out.push_str("<h3>");
    crate::panels::help_manual::escape_html(heading, out);
    out.push_str("</h3>");
    if include_scope_summary {
        out.push_str("<p class=\"chapter-sub\"><strong>API Coverage:</strong> ");
        crate::panels::help_manual::escape_html(&api_footer_text(LanguageScope::JsTs), out);
        out.push_str("</p>");
    }
    out.push_str(
        "<table class=\"ref-table\"><thead><tr><th>Entry &amp; Signature</th><th>NeoFlash Status</th><th>Description, Boundary &amp; Example</th></tr></thead><tbody>",
    );
    for entry in JS_TS_API_ENTRIES
        .iter()
        .filter(|e| categories.contains(&e.category))
    {
        let member_key = format!("{}|{}", entry.category, entry.name);
        let indexed = index
            .class(entry.category)
            .and_then(|cls| cls.member(&member_key));
        let declaration = indexed.map_or(entry.signature, |m| m.declaration.as_str());
        let usage = indexed.map_or(entry.usage, |m| m.usage.as_str());
        let example = indexed.map_or(entry.example, |m| m.example.as_str());
        out.push_str("<tr><td><code>");
        crate::panels::help_manual::escape_html(entry.name, out);
        out.push_str("</code>");
        if !usage.is_empty() && usage != declaration {
            out.push_str("<br><code>");
            crate::panels::help_manual::escape_html(usage, out);
            out.push_str("</code>");
        }
        if !declaration.is_empty() {
            out.push_str("<pre class=\"code-block\"><code>");
            crate::panels::help_manual::escape_html(declaration, out);
            out.push_str("</code></pre>");
        }
        out.push_str("</td><td>");
        write_support_badge_html(Some(entry.support), "", out);
        out.push_str("<br><code>[");
        crate::panels::help_manual::escape_html(entry.provenance, out);
        out.push_str("]</code></td><td>");
        crate::panels::help_manual::escape_html(entry.summary, out);
        if !entry.support_note.is_empty() {
            out.push_str(" <strong>NeoFlash note:</strong> ");
            crate::panels::help_manual::escape_html(entry.support_note, out);
        }
        out.push_str(" <em>Available in: ");
        crate::panels::help_manual::escape_html(entry.availability, out);
        out.push_str(".</em>");
        if !example.is_empty() {
            out.push_str("<pre class=\"code-block\"><code>");
            crate::panels::help_manual::escape_html(example, out);
            out.push_str("</code></pre>");
        }
        out.push_str("</td></tr>");
    }
    out.push_str("</tbody></table>");
}

fn write_as_index_browser_html(
    scope: LanguageScope,
    heading: &str,
    core_class_keys: &[&str],
    filter_pkg: impl Fn(&str) -> bool,
    out: &mut String,
) {
    let index = api_index(scope);
    out.push_str("<details class=\"api-details\"><summary>");
    crate::panels::help_manual::escape_html(heading, out);
    out.push_str(
        "</summary><table class=\"ref-table\"><thead><tr><th>Class / Package</th><th>NeoFlash Status</th><th>Description</th></tr></thead><tbody>",
    );
    for pkg in index.packages.iter().filter(|p| filter_pkg(&p.label)) {
        for (class_key, _) in &pkg.classes {
            let Some(class) = index.class(class_key) else {
                continue;
            };
            let (working, partial, missing) = class.support_counts();
            out.push_str("<tr><td><code>");
            crate::panels::help_manual::escape_html(&class.name, out);
            out.push_str("</code><br><span class=\"search-terms\">");
            crate::panels::help_manual::escape_html(&class.package, out);
            out.push_str("</span></td><td>");
            write_support_badge_html(Support::of(&class.status), &class.status, out);
            let _ = write!(
                out,
                "<br><span class=\"search-terms\">{working}W / {partial}P / {missing}M</span></td><td>"
            );
            crate::panels::help_manual::escape_html(&class.description, out);
            out.push_str("</td></tr>");
        }
    }
    out.push_str("</tbody></table>");
    for &key in core_class_keys {
        let Some(class) = index.class(key) else {
            continue;
        };
        out.push_str("<details class=\"api-details\"><summary>");
        crate::panels::help_manual::escape_html(&class.name, out);
        out.push_str(" — Core Member Descriptions</summary><table class=\"ref-table\"><thead><tr><th>Member &amp; Signature</th><th>Status</th><th>Description &amp; Example</th></tr></thead><tbody>");
        for member in class.members.iter().take(24) {
            out.push_str("<tr><td><code>");
            crate::panels::help_manual::escape_html(&member.label, out);
            out.push_str("</code>");
            if !member.usage.is_empty() && member.usage != member.declaration {
                out.push_str("<br><code>");
                crate::panels::help_manual::escape_html(&member.usage, out);
                out.push_str("</code>");
            }
            if !member.declaration.is_empty() {
                out.push_str("<pre class=\"code-block\"><code>");
                crate::panels::help_manual::escape_html(&member.declaration, out);
                out.push_str("</code></pre>");
            }
            out.push_str("</td><td>");
            write_support_badge_html(member.support, "", out);
            out.push_str("</td><td>");
            crate::panels::help_manual::escape_html(&member.description, out);
            if !member.example.is_empty() {
                out.push_str("<pre class=\"code-block\"><code>");
                crate::panels::help_manual::escape_html(&member.example, out);
                out.push_str("</code></pre>");
            }
            out.push_str("</td></tr>");
        }
        out.push_str("</tbody></table></details>");
    }
    out.push_str("</details>");
}

fn write_as_chapter_reference_html(index: usize, out: &mut String) {
    match index {
        9 => {
            #[cfg(target_os = "macos")]
            const INSTALL_ROW: &str = "<tr><td><code>Install &amp; Manual Update</code></td><td>Replace <code>NeoFlash.app</code> in <code>/Applications</code></td><td>Preferences, custom workspaces, recent files, and crash-recovery snapshots in <code>~/Library/Application Support/NeoFlash</code> are preserved across manual updates.</td></tr>";
            #[cfg(target_os = "windows")]
            const INSTALL_ROW: &str = "<tr><td><code>Install &amp; Manual Update</code></td><td>Extract the Windows x64 portable ZIP and launch <code>neoflash-editor.exe</code></td><td>Preferences, custom workspaces, recent files, and crash-recovery snapshots in <code>%APPDATA%\\NeoFlash</code> are preserved across manual updates.</td></tr>";
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            const INSTALL_ROW: &str = "<tr><td><code>Install &amp; Manual Update</code></td><td>Install <code>neoflash_*_amd64.deb</code> (<code>neoflash</code>) or extract the Linux x64 portable tar.gz (<code>neoflash-editor</code>)</td><td>Preferences, custom workspaces, recent files, and crash-recovery snapshots in <code>~/.local/share/NeoFlash</code> are preserved across manual updates.</td></tr>";
            #[cfg(target_os = "macos")]
            const MEDIA_ROW: &str = "<tr><td><code>Bundled Media &amp; Licences</code></td><td><code>Contents/Resources/media/bin/</code></td><td><code>ffmpeg</code>/<code>ffprobe</code>, exact application/Rust/media licence evidence and checksum manifests are included; corresponding source is supplied beside the platform archive. Set <code>NEOFLASH_FFMPEG</code> / <code>NEOFLASH_FFPROBE</code> only for custom overrides.</td></tr>";
            #[cfg(not(target_os = "macos"))]
            const MEDIA_ROW: &str = "<tr><td><code>Bundled Media &amp; Licences</code></td><td><code>media/bin/</code> (or <code>/usr/lib/neoflash/media/bin/</code>)</td><td><code>ffmpeg</code>/<code>ffprobe</code>, exact application/Rust/media licence evidence and checksum manifests are included; corresponding source is supplied beside the platform archive. Set <code>NEOFLASH_FFMPEG</code> / <code>NEOFLASH_FFPROBE</code> only for custom overrides.</td></tr>";

            out.push_str(
                "<h3>NeoFlash 0.1.0-beta.1 — Release Identity, Install, Manual Update, Media &amp; Known Issues</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Area</th><th>0.1.0-beta.1 Specification</th><th>Details &amp; Operational Guidance</th></tr></thead><tbody>\
                 <tr><td><code>Release Identity</code></td><td><code>NeoFlash 0.1.0-beta.1</code> · macOS 15+ Apple Silicon · macOS 12+ Intel (<code>x86_64</code>) · Windows/Linux x64 Technical Previews</td><td>macOS DMG/ZIP bundles are Developer ID signed and Apple notarized; Windows is an unsigned portable ZIP; Linux ships as an Ubuntu 24.04 <code>amd64</code> <code>.deb</code> and portable <code>tar.gz</code>.</td></tr>",
            );
            out.push_str(INSTALL_ROW);
            out.push_str(MEDIA_ROW);
            out.push_str(
                "<tr><td><code>Backups &amp; Bug Reports</code></td><td>Versioned <code>.nflash</code> JSON + <code>About NeoFlash</code></td><td>Keep <code>.nflash</code> projects in Git or regular file backups. Attach a minimal <code>.nflash</code> repro, <code>Output</code> panel diagnostics, and the build commit from <code>NeoFlash &gt; About NeoFlash</code> when reporting bugs.</td></tr>\
                 <tr><td><code>Known Issues &amp; Limits</code></td><td>VoiceOver, separate macOS bundles, manual updates, Windows / Linux previews, graphics tablets</td><td>Spoken VoiceOver output has not been verified. Apple Silicon (<code>arm64</code>) and Intel (<code>x86_64</code>) macOS bundles are distributed separately (no fat universal binary). Updates are installed manually. The Windows preview is unsigned, so SmartScreen may warn before the first launch, and it has no connected debugger. The Linux preview targets Ubuntu 24.04 x64 with X11 and Wayland. Import of legacy Flash files is bounded. Graphics-tablet support is experimental.</td></tr>\
                 </tbody></table>\
                 <h3>Classic Flash &lt;ESC&gt; Two-Key Code Chords in Actions (All 50 Chords)</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Chord</th><th>Inserted Template</th><th>Purpose</th></tr></thead><tbody>",
            );
            for &(chord, expansion, desc) in &CLASSIC_ESC_CHORDS {
                out.push_str("<tr><td><code>&lt;ESC&gt;");
                crate::panels::help_manual::escape_html(chord, out);
                out.push_str("</code></td><td><code>");
                crate::panels::help_manual::escape_html(expansion, out);
                out.push_str("</code></td><td>");
                crate::panels::help_manual::escape_html(desc, out);
                out.push_str("</td></tr>");
            }
            out.push_str("</tbody></table>");
        }
        10 => {
            out.push_str(
                "<h3>AS1 / AS2 Timeline, function Statement, arguments &amp; Function Class Reference</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Construct / API</th><th>Scope &amp; Signature</th><th>Behavior</th></tr></thead><tbody>\
                 <tr><td><code>function name(p:Type):Return</code></td><td>Hoisted named declaration</td><td>Callable before or after its definition on the same frame; <code>return expr;</code> exits immediately (<code>Void</code> / omitted returns <code>undefined</code>).</td></tr>\
                 <tr><td><code>var fn:Function = function(...)</code></td><td>Anonymous expression</td><td>Evaluated at assignment time; passed as first-class callbacks to <code>Array.sort</code>, <code>setInterval</code>, and event handlers.</td></tr>\
                 <tr><td><code>arguments.length / callee / caller</code></td><td>Local call activation</td><td>Variadic argument array, anonymous self-recursion via <code>arguments.callee</code>, and caller inspection via <code>arguments.caller</code>.</td></tr>\
                 <tr><td><code>Function.call / Function.apply</code></td><td><code>fn.call(thisObj, ...)</code> / <code>fn.apply(thisObj, arr)</code></td><td>Invokes <code>fn</code> with explicit <code>this</code> MovieClip/object binding and individual or array-packed arguments.</td></tr>\
                 <tr><td><code>Function.prototype / new</code></td><td>Constructor inheritance</td><td>Attaches shared methods inherited via <code>__proto__</code> on <code>new Ctor()</code> and <code>Object.registerClass</code> symbols.</td></tr>\
                 <tr><td><code>var local</code> vs <code>this.prop</code></td><td>Lexical closures</td><td><code>var</code> binds to the function activation object (surviving inside nested closures); <code>this.prop</code> persists on the timeline.</td></tr>\
                 <tr><td><code>play / stop / gotoAndPlay / gotoAndStop</code></td><td>Timeline &amp; MovieClip</td><td>1-based frame index or frame label navigation on <code>this</code>, <code>_root</code>, <code>_parent</code>, or child clips.</td></tr>\
                 <tr><td><code>on(...) / onClipEvent(...)</code></td><td>Button / MovieClip instance</td><td>Attached classic object handlers and per-frame hooks.</td></tr>\
                 </tbody></table>\
                 <h3>AS2 Case 1 &amp; 2 — Named function Declarations, Hoisting, return &amp; Anonymous Literals</h3>\
                 <pre class=\"code-block\"><code>",
            );
            crate::panels::help_manual::escape_html(AS2_FN_DECLARATIONS_AND_LITERALS_SAMPLE, out);
            out.push_str(
                "</code></pre>\
                 <h3>AS2 Case 3 &amp; 4 — Variadic arguments (callee, caller) &amp; Function.call / Function.apply</h3>\
                 <pre class=\"code-block\"><code>",
            );
            crate::panels::help_manual::escape_html(AS2_FN_ARGUMENTS_CALL_APPLY_SAMPLE, out);
            out.push_str(
                "</code></pre>\
                 <h3>AS2 Case 5 &amp; 6 — Constructor Functions, Function.prototype &amp; Lexical Closures</h3>\
                 <pre class=\"code-block\"><code>",
            );
            crate::panels::help_manual::escape_html(AS2_FN_PROTOTYPE_AND_CLOSURES_SAMPLE, out);
            out.push_str(
                "</code></pre>\
                 <h3>AS2 Case 7 — Complete 1-Frame Game Pattern (onRelease, onEnterFrame, Key, setInterval &amp; trace)</h3>\
                 <pre class=\"code-block\"><code>",
            );
            crate::panels::help_manual::escape_html(AS2_ONE_FRAME_GAME_LOOP_SAMPLE, out);
            out.push_str("</code></pre>");
            write_as_index_browser_html(
                LanguageScope::As1As2,
                "ActionScript 1.0 / 2.0 Global Functions, Language Elements & Top-Level Classes",
                &[API_GLOBAL_KEY, "MovieClip", "Button"],
                |pkg| pkg == "Top level",
                out,
            );
        }
        11 => {
            out.push_str(
                "<h3>AS2 Core Runtime Classes Quick Reference</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Class / Object</th><th>Package</th><th>Key Members</th></tr></thead><tbody>\
                 <tr><td><code>Function &amp; arguments</code></td><td>Top-level</td><td><code>call(thisObj, ...)</code>, <code>apply(thisObj, argsArray)</code>, <code>prototype</code>, <code>arguments.length</code>, <code>arguments.callee</code>, <code>arguments.caller</code>.</td></tr>\
                 <tr><td><code>MovieClip</code> (Drawing API)</td><td>Top-level</td><td><code>createEmptyMovieClip</code>, <code>lineStyle</code>, <code>beginFill</code>, <code>beginGradientFill</code>, <code>moveTo</code>, <code>lineTo</code>, <code>curveTo</code>, <code>endFill</code>.</td></tr>\
                 <tr><td><code>TextField &amp; TextFormat</code></td><td>Top-level</td><td><code>createTextField</code>, <code>text</code>, <code>htmlText</code>, <code>setTextFormat</code>, <code>setNewTextFormat</code>, <code>autoSize</code>, <code>wordWrap</code>.</td></tr>\
                 <tr><td><code>BitmapData / Matrix / ColorTransform</code></td><td><code>flash.display</code>, <code>flash.geom</code></td><td><code>draw</code>, <code>copyPixels</code>, <code>fillRect</code>, <code>getPixel32</code>, <code>perlinNoise</code>, <code>threshold</code>.</td></tr>\
                 <tr><td><code>Sound / Stage / SharedObject / LoadVars</code></td><td>Top-level</td><td><code>attachSound</code>, <code>loadSound</code>, <code>Stage.scaleMode</code>, <code>Stage.align</code>, <code>Stage.displayState</code>, <code>getLocal</code>, <code>setInterval</code>, <code>clearInterval</code>.</td></tr>\
                 </tbody></table>\
                 <h3>AS2 Procedural Drawing, TextFormat &amp; BitmapData Example</h3>\
                 <pre class=\"code-block\"><code>import flash.display.BitmapData;\nimport flash.geom.Matrix;\nvar canvas:MovieClip = this.createEmptyMovieClip(\"badge_mc\", this.getNextHighestDepth());\ncanvas.lineStyle(2, 0x6C93FF, 100);\ncanvas.beginFill(0x1F2A48, 95);\ncanvas.moveTo(20, 20); canvas.lineTo(180, 20); canvas.lineTo(180, 70); canvas.lineTo(20, 70); canvas.endFill();\nthis.createTextField(\"label_txt\", this.getNextHighestDepth(), 32, 32, 140, 28);\nlabel_txt.text = \"READY PLAYER ONE\";\nlabel_txt.setTextFormat(new TextFormat(\"Inter\", 14, 0xFFFFFF, true));</code></pre>",
            );
            write_as_index_browser_html(
                LanguageScope::As1As2,
                "ActionScript 2.0 Indexed Packages & Class Descriptions (flash.*, mx.*, System.*)",
                &["TextField", "flash.display.BitmapData", "Sound"],
                |pkg| pkg != "Top level",
                out,
            );
        }
        12 => {
            out.push_str(
                "<h3>AS3 Display List &amp; Function Semantics Quick Reference</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Class</th><th>Package</th><th>Capabilities</th></tr></thead><tbody>\
                 <tr><td><code>Function</code></td><td>Top-level</td><td>Default parameter values, typed <code>...rest</code> arrays, bound class method closures, <code>call(thisArg, ...)</code>, and <code>apply(thisArg, array)</code>.</td></tr>\
                 <tr><td><code>DisplayObject</code></td><td><code>flash.display</code></td><td><code>x</code>, <code>y</code>, <code>scaleX</code>, <code>scaleY</code>, <code>rotation</code>, <code>alpha</code>, <code>visible</code>, <code>filters</code>, <code>mask</code>, <code>localToGlobal</code>, <code>getBounds</code>.</td></tr>\
                 <tr><td><code>InteractiveObject</code></td><td><code>flash.display</code></td><td><code>mouseEnabled</code>, <code>tabEnabled</code>, <code>tabIndex</code>, <code>doubleClickEnabled</code>, keyboard &amp; pointer focus.</td></tr>\
                 <tr><td><code>DisplayObjectContainer / Sprite</code></td><td><code>flash.display</code></td><td><code>addChild</code>, <code>addChildAt</code>, <code>removeChild</code>, <code>swapChildren</code>, <code>graphics</code>, <code>buttonMode</code>, <code>startDrag</code>.</td></tr>\
                 <tr><td><code>MovieClip &amp; Stage</code></td><td><code>flash.display</code></td><td><code>currentFrame</code>, <code>currentLabel</code>, <code>totalFrames</code>, <code>stage.scaleMode</code>, <code>stage.align</code>, <code>stage.displayState</code>.</td></tr>\
                 </tbody></table>\
                 <h3>AS3 Functions — Default Parameters, ...rest, Closures &amp; Function.call / apply</h3>\
                 <pre class=\"code-block\"><code>",
            );
            crate::panels::help_manual::escape_html(AS3_FUNCTIONS_REST_CALL_CLOSURES_SAMPLE, out);
            out.push_str(
                "</code></pre>\
                 <h3>AS3 Display List &amp; Coordinate Space Example</h3>\
                 <pre class=\"code-block\"><code>package {\n    import flash.display.Sprite;\n    import flash.events.Event;\n    import flash.geom.Point;\n    public class Main extends Sprite {\n        public function Main() {\n            addEventListener(Event.ADDED_TO_STAGE, init);\n        }\n        private function init(e:Event):void {\n            var child:Sprite = new Sprite();\n            child.x = 120; child.y = 80;\n            addChild(child);\n            var globalPt:Point = child.localToGlobal(new Point(0, -18));\n            trace(\"Global position:\", globalPt.x, globalPt.y); // Output: Global position: 120 62\n        }\n    }\n}</code></pre>",
            );
            write_as_index_browser_html(
                LanguageScope::As3,
                "ActionScript 3.0 Display List, Top-Level & Text Class Descriptions",
                &[
                    "flash.display.MovieClip",
                    "flash.display.DisplayObject",
                    "flash.text.TextField",
                ],
                |pkg| {
                    pkg == "Top level"
                        || pkg.starts_with("flash.display")
                        || pkg.starts_with("flash.text")
                },
                out,
            );
        }
        13 => {
            out.push_str(
                "<h3>AS3 Event Model &amp; Frame Lifecycle Quick Reference</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Event Class</th><th>Constants</th><th>Notes</th></tr></thead><tbody>\
                 <tr><td><code>Event</code></td><td><code>ADDED_TO_STAGE</code>, <code>ENTER_FRAME</code>, <code>RESIZE</code>, <code>COMPLETE</code></td><td>3-phase W3C propagation (Capture → Target → Bubble); deterministic frame tick order.</td></tr>\
                 <tr><td><code>MouseEvent</code></td><td><code>CLICK</code>, <code>MOUSE_DOWN</code>, <code>MOUSE_UP</code>, <code>MOUSE_MOVE</code>, <code>ROLL_OVER</code></td><td>Provides <code>localX/Y</code>, <code>stageX/Y</code>, <code>buttonDown</code>, <code>delta</code>; honors <code>Stage.align</code> &amp; fullscreen transforms.</td></tr>\
                 <tr><td><code>KeyboardEvent / FullScreenEvent</code></td><td><code>KEY_DOWN</code>, <code>KEY_UP</code>, <code>FULL_SCREEN</code></td><td>Stage-dispatched keyboard input and fullscreen transitions (<code>StageDisplayState</code>).</td></tr>\
                 <tr><td><code>Timer &amp; TimerEvent</code></td><td><code>TIMER</code>, <code>TIMER_COMPLETE</code></td><td>Repeatable millisecond intervals with <code>currentCount</code>, <code>reset()</code>, <code>start()</code>, and <code>stop()</code>.</td></tr>\
                 </tbody></table>\
                 <h3>AS3 3-Phase Events, Stage Keyboard &amp; Timer Example</h3>\
                 <pre class=\"code-block\"><code>import flash.events.MouseEvent;\nimport flash.events.KeyboardEvent;\nimport flash.events.TimerEvent;\nimport flash.utils.Timer;\ncard_mc.buttonMode = true;\ncard_mc.mouseChildren = false;\ncard_mc.addEventListener(MouseEvent.MOUSE_DOWN, function(e:MouseEvent):void { card_mc.startDrag(); });\nstage.addEventListener(MouseEvent.MOUSE_UP, function(e:MouseEvent):void { card_mc.stopDrag(); });\nvar timer:Timer = new Timer(500, 10);\ntimer.addEventListener(TimerEvent.TIMER, function(e:TimerEvent):void { trace(timer.currentCount); });\ntimer.start();</code></pre>",
            );
            write_as_index_browser_html(
                LanguageScope::As3,
                "ActionScript 3.0 Events, Input & Timer Class Descriptions (flash.events.*, flash.ui.*)",
                &[
                    "flash.events.EventDispatcher",
                    "flash.events.MouseEvent",
                    "flash.utils.Timer",
                ],
                |pkg| pkg.starts_with("flash.events") || pkg.starts_with("flash.ui"),
                out,
            );
        }
        14 => {
            out.push_str(
                "<h3>AS3 Graphics, Filters, Media &amp; Data Quick Reference</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Subsystem</th><th>Classes</th><th>Capabilities</th></tr></thead><tbody>\
                 <tr><td>Vector Graphics</td><td><code>Graphics</code>, <code>GradientType</code></td><td><code>drawRect</code>, <code>drawRoundRect</code>, <code>drawCircle</code>, <code>drawEllipse</code>, <code>cubicCurveTo</code>, <code>drawPath</code>.</td></tr>\
                 <tr><td>Live Filters</td><td><code>BlurFilter</code>, <code>GlowFilter</code>, <code>DropShadowFilter</code>, <code>ColorMatrixFilter</code></td><td>Composable per-display-object filter chains matching authored NF2 filter stacks.</td></tr>\
                 <tr><td>Media &amp; Loading</td><td><code>Sound</code>, <code>SoundChannel</code>, <code>Loader</code>, <code>URLRequest</code>, <code>NetStream</code></td><td>Runtime loading of SWF, published NeoFlash bundles, MP3, WAV, MP4, AVI, SVG, and bitmaps.</td></tr>\
                 <tr><td>Binary &amp; XML</td><td><code>ByteArray</code>, <code>XML</code>, <code>XMLList</code>, <code>SharedObject</code></td><td>Big/little-endian binary streams, E4X filtering, and persistent local state.</td></tr>\
                 </tbody></table>\
                 <h3>AS3 Graphics, GlowFilter, SoundChannel &amp; E4X XML Example</h3>\
                 <pre class=\"code-block\"><code>import flash.display.Sprite;\nimport flash.filters.GlowFilter;\nimport flash.media.Sound;\nimport flash.media.SoundTransform;\nvar orb:Sprite = new Sprite();\norb.graphics.beginFill(0x3C6BF0, 0.85);\norb.graphics.drawCircle(0, 0, 36);\norb.graphics.endFill();\norb.filters = [new GlowFilter(0x6C93FF, 0.9, 16, 16, 2, 2)];\naddChild(orb);\nvar xml:XML = &lt;items&gt;&lt;weapon id=\"railgun\" dmg=\"90\"/&gt;&lt;/items&gt;;\ntrace(xml.weapon.(Number(@dmg) &gt;= 50).@id); // Output: railgun</code></pre>",
            );
            write_as_index_browser_html(
                LanguageScope::As3,
                "ActionScript 3.0 Geometry, Filters, Media, Networking & Binary Class Descriptions",
                &[
                    "flash.geom.Point",
                    "flash.media.Sound",
                    "flash.utils.ByteArray",
                ],
                |pkg| {
                    pkg != "Top level"
                        && !pkg.starts_with("flash.display")
                        && !pkg.starts_with("flash.text")
                        && !pkg.starts_with("flash.events")
                        && !pkg.starts_with("flash.ui")
                },
                out,
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
        16 => {
            write_js_ts_entries_html(
                "JavaScript (ES2023+), Modules & JSX — NeoFlash Support Status & Reference",
                &[
                    "NeoFlash Context & Timeline",
                    "Modules & JSX",
                    "ECMAScript & Async",
                ],
                true,
                out,
            );
            out.push_str(
                "<h3>JavaScript Functions, Closures &amp; Function.call / apply Example</h3>\
                 <pre class=\"code-block\"><code>",
            );
            crate::panels::help_manual::escape_html(JS_FUNCTIONS_CLOSURES_AND_CALL_SAMPLE, out);
            out.push_str(
                "</code></pre>\
                 <h3>JavaScript Frame Script &amp; JSX Example</h3>\
                 <pre class=\"code-block\"><code>stop();\ntrace(\"Entering level:\", this.currentFrame, \"of\", this.totalFrames);\nconst badge = &lt;hud-badge label=\"SCORE\" value={0} /&gt;;\nthis.start_btn.addEventListener(\"click\", (event) =&gt; {\n    trace(\"Clicked:\", event.target.name);\n    gotoAndPlay(\"gameplay\");\n});</code></pre>",
            );
        }
        17 => {
            write_js_ts_entries_html(
                "TypeScript & Built-In Checker (ts_check) — NeoFlash Support Status & Reference",
                &["TypeScript & Checker"],
                false,
                out,
            );
            out.push_str(
                "<h3>TypeScript Typed &amp; Generic Functions Example</h3>\
                 <pre class=\"code-block\"><code>",
            );
            crate::panels::help_manual::escape_html(TS_TYPED_AND_GENERIC_FUNCTIONS_SAMPLE, out);
            out.push_str(
                "</code></pre>\
                 <h3>TypeScript Interfaces, Unions &amp; Utility Types Example</h3>\
                 <pre class=\"code-block\"><code>interface Actor {\n    readonly id: string;\n    x: number;\n    y: number;\n    hp: number;\n}\ntype ActorPatch = Partial&lt;Pick&lt;Actor, \"x\" | \"y\" | \"hp\"&gt;&gt;;\nfunction applyPatch(actor: Actor, patch: ActorPatch): Actor {\n    return { id: actor.id, x: patch.x ?? actor.x, y: patch.y ?? actor.y, hp: patch.hp ?? actor.hp };\n}\nconst moved = applyPatch({ id: \"p1\", x: 120, y: 240, hp: 100 }, { x: 160, hp: 95 });\ntrace(moved.id, moved.x, moved.hp); // Output: p1 160 95</code></pre>",
            );
        }
        18 => {
            write_js_ts_entries_html(
                "Browser & Web APIs (Published HTML5 Host) — NeoFlash Support Status & Boundaries",
                &["Browser DOM & Canvas 2D", "Web Fetch, Audio & Storage"],
                false,
                out,
            );
            out.push_str(
                "<h3>TypeScript + Browser Host Example</h3>\
                 <pre class=\"code-block\"><code>async function syncHighScore(canvas: HTMLCanvasElement, url: string): Promise&lt;void&gt; {\n    const ctx = canvas.getContext(\"2d\");\n    if (ctx) { ctx.fillRect(0, 0, 240, 48); }\n    const res = await fetch(url);\n    const top = await res.json();\n    localStorage.setItem(\"neoflash.topScore\", JSON.stringify(top));\n}</code></pre>",
            );
        }
        _ => {}
    }
}

const fn browser_theme_slug(theme: Theme) -> &'static str {
    match theme {
        Theme::Dark => "dark",
        Theme::Light => "light",
        Theme::MidNeutralGray => "mid-neutral-gray",
        Theme::DarkGraphite => "dark-graphite",
    }
}

const fn browser_theme_label(theme: Theme) -> &'static str {
    match theme {
        Theme::Dark => "Dark",
        Theme::Light => "Light",
        Theme::MidNeutralGray => "Mid Neutral Gray",
        Theme::DarkGraphite => "Dark Graphite",
    }
}

#[cfg(test)]
pub(crate) fn render_full_browser_help_html(initial_theme: Theme) -> String {
    render_browser_help_html_for_route(initial_theme, None, None, None)
}

fn chapter_html_lang_scopes(index: usize) -> &'static str {
    let mut as12 = LanguageScope::As1As2.matches_chapter(index);
    let mut as3 = LanguageScope::As3.matches_chapter(index);
    let mut jsts = LanguageScope::JsTs.matches_chapter(index);
    if index < AUTHORING_CHAPTER_COUNT {
        as12 = true;
        as3 = true;
        jsts = true;
    }
    match (as12, as3, jsts) {
        (true, true, true) => "ALL AS1_AS2 AS3 JS_TS",
        (true, false, false) => "ALL AS1_AS2",
        (false, true, false) => "ALL AS3",
        (false, false, true) => "ALL JS_TS",
        (true, true, false) => "ALL AS1_AS2 AS3",
        (true, false, true) => "ALL AS1_AS2 JS_TS",
        (false, true, true) => "ALL AS3 JS_TS",
        (false, false, false) => "ALL",
    }
}

pub(crate) fn render_browser_help_html_for_route(
    initial_theme: Theme,
    initial_mode: Option<crate::panels::help_manual::HelpManualMode>,
    active_page: Option<usize>,
    active_tag: Option<&str>,
) -> String {
    render_browser_help_html_for_project(
        initial_theme,
        initial_mode,
        active_page,
        active_tag,
        None,
        LanguageScope::All,
    )
}

pub(crate) fn render_browser_help_html_for_project(
    initial_theme: Theme,
    initial_mode: Option<crate::panels::help_manual::HelpManualMode>,
    active_page: Option<usize>,
    active_tag: Option<&str>,
    project_language: Option<neoflash_core::ScriptLanguage>,
    initial_lang_scope: LanguageScope,
) -> String {
    use crate::panels::help_manual::HelpManualMode;
    let hash = char::from(35u8);
    let clamped_page = active_page.map(|p| p.min(HELP_CHAPTERS.len().saturating_sub(1)));
    let initial_mode_slug = match initial_mode {
        None => "ALL",
        Some(HelpManualMode::AuthoringHelp) => "AUTHORING",
        Some(HelpManualMode::ScriptingReference) => "SCRIPTING",
    };
    let initial_lang_slug = initial_lang_scope.html_slug();
    let initial_tag = active_tag
        .filter(|t| HELP_ALL_TAGS.contains(t))
        .unwrap_or("ALL");
    let page_title = match initial_mode {
        Some(HelpManualMode::ScriptingReference) => {
            "NeoFlash Scripting Reference — NeoFlash Help &amp; Reference"
        }
        Some(HelpManualMode::AuthoringHelp) => {
            "NeoFlash Authoring Manual — NeoFlash Help &amp; Reference"
        }
        None => "NeoFlash Help &amp; Reference",
    };
    let mut html = String::with_capacity(114_688);
    let _ = write!(
        html,
        "<!doctype html><html lang=\"en\" data-initial-theme=\"{}\" data-initial-mode=\"{initial_mode_slug}\" data-initial-lang=\"{initial_lang_slug}\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <meta name=\"color-scheme\" content=\"dark light\">\
         <title>{page_title}</title><style>",
        browser_theme_slug(initial_theme)
    );
    html.push_str(":root{");
    let _ = write!(
        html,
        "color-scheme:{};",
        if initial_theme.is_dark() {
            "dark"
        } else {
            "light"
        }
    );
    write_palette_css_vars(initial_theme.palette(), &mut html);
    html.push('}');
    for theme in Theme::ALL {
        let _ = write!(
            html,
            "body:has(#nf2-theme-{}:checked){{color-scheme:{};",
            browser_theme_slug(theme),
            if theme.is_dark() { "dark" } else { "light" }
        );
        write_palette_css_vars(theme.palette(), &mut html);
        html.push('}');
    }
    html.push_str(
        "*{box-sizing:border-box}body{margin:0;font-family:Inter,-apple-system,BlinkMacSystemFont,sans-serif;font-size:13px;line-height:1.55;background:var(--bg-app);color:var(--text-primary)}\
         .sr-toggle{position:absolute;opacity:0;pointer-events:none}\
         .help-shell{min-height:100vh;background:var(--bg-app);color:var(--text-primary);display:flex;flex-direction:column}\
         .top-header{position:sticky;top:0;z-index:10;background:var(--bg-raised);border-bottom:1px solid var(--border-subtle);padding:12px 24px;display:flex;align-items:center;justify-content:space-between;gap:16px;flex-wrap:wrap}\
         .top-header h1{margin:0;font-size:16px;font-weight:600;color:var(--text-primary)}\
         .top-header p{margin:2px 0 0;font-size:12px;color:var(--text-secondary)}\
         .theme-btn{display:none;cursor:pointer;padding:6px 12px;border-radius:6px;background:var(--bg-panel);border:1px solid var(--border-default);color:var(--text-primary);font-size:12px;font-weight:500;user-select:none}\
         .theme-btn:hover{background:var(--bg-hover);border-color:var(--border-strong)}\
         body:has(#nf2-theme-dark:checked) .theme-from-dark,body:has(#nf2-theme-light:checked) .theme-from-light,body:has(#nf2-theme-mid-neutral-gray:checked) .theme-from-mid-neutral-gray,body:has(#nf2-theme-dark-graphite:checked) .theme-from-dark-graphite{display:inline-block}\
         .tag-bar{background:var(--bg-panel);border-bottom:1px solid var(--border-subtle);padding:8px 24px;display:flex;align-items:center;gap:6px;flex-wrap:wrap}\
         .tag-bar-label{font-size:11px;font-weight:600;text-transform:uppercase;letter-spacing:0.06em;color:var(--text-tertiary);margin-right:4px}\
         .tag-pill{cursor:pointer;padding:3px 10px;border-radius:999px;background:var(--bg-raised);border:1px solid var(--border-subtle);color:var(--text-secondary);font-size:11px;font-weight:500;user-select:none;text-decoration:none}\
         .tag-pill:hover{background:var(--bg-hover);color:var(--text-primary)}\
         .layout{display:grid;grid-template-columns:280px 1fr;gap:24px;max-width:1380px;width:100%;margin:0 auto;padding:20px 24px}\
         @media (max-width:860px){.layout{grid-template-columns:1fr}}\
         nav.sidebar{background:var(--bg-panel);border:1px solid var(--border-subtle);border-radius:8px;padding:12px;align-self:start;position:sticky;top:76px;max-height:calc(100vh - 96px);overflow-y:auto}\
         nav.sidebar ol{list-style:none;margin:0;padding:0;display:flex;flex-direction:column;gap:3px}\
         nav.sidebar .nav-group{margin:8px 6px 4px;font-size:10.5px;font-weight:700;text-transform:uppercase;letter-spacing:0.06em;color:var(--text-tertiary)}\
         nav.sidebar a{display:flex;align-items:center;justify-content:space-between;gap:6px;padding:6px 10px;border-radius:6px;color:var(--text-primary);text-decoration:none;font-size:12px;font-weight:500;overflow-wrap:anywhere}\
         nav.sidebar a:hover{background:var(--bg-hover);color:var(--accent-default)}\
         .nav-dialect{font-size:10.5px;color:var(--text-tertiary);font-family:'Roboto Mono',ui-monospace,monospace;flex-shrink:0}\
         main{display:flex;flex-direction:column;gap:20px;min-width:0}\
         section.chapter-card{background:var(--bg-panel);border:1px solid var(--border-subtle);border-radius:8px;padding:20px;scroll-margin-top:84px;user-select:text;-webkit-user-select:text}\
         section.chapter-card:target,section.chapter-card.is-opened-chapter{border-color:var(--accent-default);box-shadow:0 0 0 1px var(--accent-default)}\
         nav.sidebar a.is-opened-nav{background:var(--accent-subtle);color:var(--accent-default);font-weight:600}\
         .chapter-head{display:flex;align-items:baseline;justify-content:space-between;gap:12px;flex-wrap:wrap;border-bottom:1px solid var(--border-subtle);padding-bottom:10px;margin-bottom:12px}\
         .chapter-head h2{margin:0;font-size:16px;font-weight:600;color:var(--text-primary)}\
         .chapter-sub{margin:4px 0 10px;color:var(--text-secondary);font-size:12px}\
         .chapter-tags{display:flex;gap:6px;flex-wrap:wrap;margin-bottom:12px}\
         .badge{display:inline-flex;align-items:center;gap:5px;padding:2px 8px;border-radius:999px;background:var(--accent-subtle);color:var(--accent-default);font-size:11px;font-weight:500;cursor:pointer}\
         .diagram-card{margin:12px 0;padding:0}\
         .chapter-svg{width:100%;height:auto;display:block;border-radius:8px}\
         .diagram-caption{margin-top:6px;font-size:11.5px;color:var(--text-secondary)}\
         ul.topic-list{margin:10px 0;padding-left:20px;color:var(--text-primary)}\
         ul.topic-list li{margin-bottom:6px}\
         pre.code-block{background:var(--bg-field);border:1px solid var(--border-subtle);border-radius:6px;padding:12px;overflow-x:auto;font-family:'Roboto Mono',ui-monospace,monospace;font-size:11.5px;line-height:1.45;color:var(--text-primary);user-select:text;-webkit-user-select:text}\
         table.ref-table{width:100%;border-collapse:collapse;margin:10px 0;font-size:12px;user-select:text;-webkit-user-select:text}\
         table.ref-table pre.code-block{margin:6px 0 0;padding:8px 10px}\
         table.ref-table th,table.ref-table td{text-align:left;padding:7px 10px;border-bottom:1px solid var(--border-subtle);vertical-align:top}\
         table.ref-table th{color:var(--accent-default);font-weight:600;background:var(--bg-raised)}\
         .status-working{color:var(--success-default);font-weight:600}\
         .status-partial{color:var(--warning-default);font-weight:600}\
         .status-missing{color:var(--text-tertiary);font-weight:600}\
         details.api-details{margin:10px 0}\
         details.api-details>summary{cursor:pointer;font-weight:600;color:var(--accent-default);padding:4px 0}\
         .cross-links{display:flex;align-items:center;gap:8px;flex-wrap:wrap;margin-top:14px;padding-top:10px;border-top:1px solid var(--border-subtle)}\
         .cross-link{display:inline-block;padding:4px 10px;border-radius:6px;background:var(--bg-raised);border:1px solid var(--border-default);color:var(--accent-default);text-decoration:none;font-size:11.5px;font-weight:500}\
         .cross-link:hover{background:var(--accent-subtle);border-color:var(--accent-default)}\
         .search-terms{margin-top:8px;font-size:11px;color:var(--text-tertiary)}",
    );

    // Pure-CSS tag & language-scope filter rules using :has()
    for tag in HELP_ALL_TAGS {
        let _ = write!(
            html,
            ".help-shell:has(input[data-tag=\"{tag}\"]:checked) label[data-tag-pill=\"{tag}\"]{{background:var(--accent-subtle);color:var(--accent-default);border-color:var(--accent-default)}}\
             .help-shell:has(input[data-tag=\"{tag}\"]:checked) section.chapter-card:not([data-tags~=\"{tag}\"]){{display:none}}\
             .help-shell:has(input[data-tag=\"{tag}\"]:checked) nav.sidebar li.chapter-nav-item:not([data-tags~=\"{tag}\"]){{display:none}}"
        );
    }
    for lang_slug in ["AS1_AS2", "AS3", "JS_TS"] {
        let _ = write!(
            html,
            ".help-shell:has(input[data-lang=\"{lang_slug}\"]:checked) label[data-lang-pill=\"{lang_slug}\"]{{background:var(--accent-subtle);color:var(--accent-default);border-color:var(--accent-default)}}\
             .help-shell:has(input[data-lang=\"{lang_slug}\"]:checked) section.chapter-card[data-section=\"scripting\"]:not([data-lang-scopes~=\"{lang_slug}\"]){{display:none}}\
             .help-shell:has(input[data-lang=\"{lang_slug}\"]:checked) nav.sidebar li.chapter-nav-item[data-section=\"scripting\"]:not([data-lang-scopes~=\"{lang_slug}\"]){{display:none}}"
        );
    }
    html.push_str(
        ".help-shell:has(input[data-lang=\"ALL\"]:checked) label[data-lang-pill=\"ALL\"]{background:var(--accent-subtle);color:var(--accent-default);border-color:var(--accent-default)}\
         .help-shell:has(input[data-tag=\"ALL\"]:checked) label[data-tag-pill=\"ALL\"]{background:var(--accent-subtle);color:var(--accent-default);border-color:var(--accent-default)}\
         .help-shell:has(input[data-mode=\"ALL\"]:checked) label[data-mode-pill=\"ALL\"],\
         .help-shell:has(input[data-mode=\"AUTHORING\"]:checked) label[data-mode-pill=\"AUTHORING\"],\
         .help-shell:has(input[data-mode=\"SCRIPTING\"]:checked) label[data-mode-pill=\"SCRIPTING\"]{background:var(--accent-subtle);color:var(--accent-default);border-color:var(--accent-default)}\
         .help-shell:has(input[data-mode=\"AUTHORING\"]:checked) section.chapter-card[data-section=\"scripting\"],\
         .help-shell:has(input[data-mode=\"AUTHORING\"]:checked) nav.sidebar li[data-section=\"scripting\"]{display:none}\
         .help-shell:has(input[data-mode=\"SCRIPTING\"]:checked) section.chapter-card[data-section=\"authoring\"],\
         .help-shell:has(input[data-mode=\"SCRIPTING\"]:checked) nav.sidebar li[data-section=\"authoring\"]{display:none}",
    );
    html.push_str("</style></head><body>");

    // Theme, section-mode, language-scope and tag filters remain 100% declarative (zero <script>).
    for theme in Theme::ALL {
        let _ = write!(
            html,
            "<input type=\"radio\" name=\"nf2-theme\" id=\"nf2-theme-{}\" class=\"sr-toggle\" aria-label=\"{} theme\"{}>",
            browser_theme_slug(theme),
            browser_theme_label(theme),
            if theme == initial_theme {
                " checked"
            } else {
                ""
            }
        );
    }
    html.push_str("<div class=\"help-shell\">");
    for mode_slug in ["ALL", "AUTHORING", "SCRIPTING"] {
        let _ = write!(
            html,
            "<input type=\"radio\" name=\"nf2-mode\" id=\"nf2-mode-{mode_slug}\" data-mode=\"{mode_slug}\" class=\"sr-toggle\" {}>",
            if mode_slug == initial_mode_slug {
                "checked"
            } else {
                ""
            }
        );
    }
    for lang_slug in ["ALL", "AS1_AS2", "AS3", "JS_TS"] {
        let _ = write!(
            html,
            "<input type=\"radio\" name=\"nf2-lang\" id=\"nf2-lang-{lang_slug}\" data-lang=\"{lang_slug}\" class=\"sr-toggle\" {}>",
            if lang_slug == initial_lang_slug {
                "checked"
            } else {
                ""
            }
        );
    }
    let _ = write!(
        html,
        "<input type=\"radio\" name=\"nf2-tag\" id=\"nf2-tag-ALL\" data-tag=\"ALL\" class=\"sr-toggle\" {}>",
        if initial_tag == "ALL" { "checked" } else { "" }
    );
    for tag in HELP_ALL_TAGS {
        let _ = write!(
            html,
            "<input type=\"radio\" name=\"nf2-tag\" id=\"nf2-tag-{tag}\" data-tag=\"{tag}\" class=\"sr-toggle\" {}>",
            if *tag == initial_tag { "checked" } else { "" }
        );
    }

    #[cfg(target_os = "macos")]
    const HEADER_SUMMARY: &str = "Illustrated NF2 Authoring Manual (Chapters 1–10) &amp; Scripting Reference: AS1/AS2, AS3, JavaScript, TypeScript &amp; Web APIs (Chapters 11–19) · Manual macOS update preserves ~/Library/Application Support/NeoFlash · Bundled ffmpeg/ffprobe and licence evidence are included.";
    #[cfg(target_os = "windows")]
    const HEADER_SUMMARY: &str = "Illustrated NF2 Authoring Manual (Chapters 1–10) &amp; Scripting Reference: AS1/AS2, AS3, JavaScript, TypeScript &amp; Web APIs (Chapters 11–19) · Manual Windows update preserves %APPDATA%\\NeoFlash · Bundled ffmpeg/ffprobe and licence evidence are included.";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    const HEADER_SUMMARY: &str = "Illustrated NF2 Authoring Manual (Chapters 1–10) &amp; Scripting Reference: AS1/AS2, AS3, JavaScript, TypeScript &amp; Web APIs (Chapters 11–19) · Manual Linux update preserves ~/.local/share/NeoFlash · Bundled ffmpeg/ffprobe and licence evidence are included.";

    html.push_str(
        "<header class=\"top-header\" id=\"top\"><div>\
         <h1>NeoFlash Help &amp; Reference (0.1.0-beta.1 · macOS Apple Silicon &amp; Intel + Windows/Linux x64 previews)</h1>\
         <p>",
    );
    html.push_str(HEADER_SUMMARY);
    html.push_str(
        "</p></div><div style=\"display:flex;align-items:center;gap:8px;flex-wrap:wrap\">",
    );
    if let Some(proj_lang) = project_language {
        let _ = write!(
            html,
            "<span class=\"badge\">Current project: {}</span>",
            proj_lang.display_label()
        );
    }
    if let Some(page) = clamped_page {
        let _ = write!(
            html,
            "<a class=\"cross-link\" href=\"{hash}chapter-{}\">Opened: Chapter {} →</a>",
            page + 1,
            page + 1
        );
    }
    html.push_str(
        "<label for=\"nf2-theme-light\" class=\"theme-btn theme-from-dark\" aria-label=\"Switch Help theme from Dark to Light\">Theme: Dark</label>\
         <label for=\"nf2-theme-mid-neutral-gray\" class=\"theme-btn theme-from-light\" aria-label=\"Switch Help theme from Light to Mid Neutral Gray\">Theme: Light</label>\
         <label for=\"nf2-theme-dark-graphite\" class=\"theme-btn theme-from-mid-neutral-gray\" aria-label=\"Switch Help theme from Mid Neutral Gray to Dark Graphite\">Theme: Mid Neutral Gray</label>\
         <label for=\"nf2-theme-dark\" class=\"theme-btn theme-from-dark-graphite\" aria-label=\"Switch Help theme from Dark Graphite to Dark\">Theme: Dark Graphite</label>\
         </div></header>",
    );

    // Interactive Section Mode + Language Scope + Tag Filter Bar
    let _ = write!(
        html,
        "<div class=\"tag-bar\" role=\"toolbar\" aria-label=\"Filter by section, language scope and topic tag\">\
         <span class=\"tag-bar-label\">Section:</span>\
         <label for=\"nf2-mode-ALL\" data-mode-pill=\"ALL\" class=\"tag-pill\">All Chapters ({})</label>\
         <label for=\"nf2-mode-AUTHORING\" data-mode-pill=\"AUTHORING\" class=\"tag-pill\">Authoring Manual (1–{})</label>\
         <label for=\"nf2-mode-SCRIPTING\" data-mode-pill=\"SCRIPTING\" class=\"tag-pill\">Scripting Reference ({}–{})</label>\
         <span class=\"tag-bar-label\" style=\"margin-left:10px\">Language:</span>\
         <label for=\"nf2-lang-ALL\" data-lang-pill=\"ALL\" class=\"tag-pill\">All languages</label>\
         <label for=\"nf2-lang-AS1_AS2\" data-lang-pill=\"AS1_AS2\" class=\"tag-pill\">AS1 / AS2</label>\
         <label for=\"nf2-lang-AS3\" data-lang-pill=\"AS3\" class=\"tag-pill\">AS3</label>\
         <label for=\"nf2-lang-JS_TS\" data-lang-pill=\"JS_TS\" class=\"tag-pill\">JS / TS</label>\
         <span class=\"tag-bar-label\" style=\"margin-left:10px\">Tag:</span>",
        HELP_CHAPTERS.len(),
        AUTHORING_CHAPTER_COUNT,
        AUTHORING_CHAPTER_COUNT + 1,
        HELP_CHAPTERS.len()
    );
    let _ = write!(
        html,
        "<label for=\"nf2-tag-ALL\" data-tag-pill=\"ALL\" class=\"tag-pill\">All ({})</label>",
        HELP_CHAPTERS.len()
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
        if index == 0 {
            html.push_str(
                "<li class=\"nav-group\" data-section=\"authoring\">Authoring Help (1–10)</li>",
            );
        } else if index == AUTHORING_CHAPTER_COUNT {
            html.push_str(
                "<li class=\"nav-group\" data-section=\"scripting\">Scripting Ref — ActionScript</li>",
            );
        } else if index == 16 {
            html.push_str(
                "<li class=\"nav-group\" data-section=\"scripting\">Scripting Ref — JS / TS / Web</li>",
            );
        }
        let section_slug = if index < AUTHORING_CHAPTER_COUNT {
            "authoring"
        } else {
            "scripting"
        };
        let nav_cls = if clamped_page == Some(index) {
            " class=\"is-opened-nav\""
        } else {
            ""
        };
        let tags_joined = chapter.tags.join(" ");
        let lang_scopes = chapter_html_lang_scopes(index);
        let _ = write!(
            html,
            "<li class=\"chapter-nav-item\" data-section=\"{section_slug}\" data-tags=\"{tags_joined}\" data-lang-scopes=\"{lang_scopes}\"><a href=\"{hash}chapter-{}\"{nav_cls}><span>",
            index + 1
        );
        crate::panels::help_manual::escape_html(chapter.title, &mut html);
        html.push_str("</span>");
        if !chapter.sidebar_badge.is_empty() {
            html.push_str("<span class=\"nav-dialect\">");
            crate::panels::help_manual::escape_html(chapter.sidebar_badge, &mut html);
            html.push_str("</span>");
        }
        html.push_str("</a></li>");
    }
    html.push_str("</ol></nav><main>");

    for (index, chapter) in HELP_CHAPTERS.iter().enumerate() {
        let tags_joined = chapter.tags.join(" ");
        let lang_scopes = chapter_html_lang_scopes(index);
        let section_slug = if index < AUTHORING_CHAPTER_COUNT {
            "authoring"
        } else {
            "scripting"
        };
        let card_cls = if clamped_page == Some(index) {
            "chapter-card is-opened-chapter"
        } else {
            "chapter-card"
        };
        let mode_badge = if index < AUTHORING_CHAPTER_COUNT {
            format!("Help Ch. {} of {}", index + 1, AUTHORING_CHAPTER_COUNT)
        } else {
            format!(
                "Scripting Ref Ch. {} of {}",
                index - AUTHORING_CHAPTER_COUNT + 1,
                HELP_CHAPTERS.len() - AUTHORING_CHAPTER_COUNT
            )
        };
        let _ = write!(
            html,
            "<section class=\"{card_cls}\" id=\"chapter-{}\" data-section=\"{section_slug}\" data-tags=\"{tags_joined}\" data-lang-scopes=\"{lang_scopes}\"><div class=\"chapter-head\"><h2>",
            index + 1
        );
        crate::panels::help_manual::escape_html(chapter.title, &mut html);
        html.push_str(
            "</h2><div style=\"display:flex;gap:6px;flex-wrap:wrap;align-items:center\">",
        );
        for &db in chapter.dialect_badges {
            html.push_str("<span class=\"badge\">");
            crate::panels::help_manual::escape_html(db, &mut html);
            html.push_str("</span>");
        }
        let _ = write!(
            html,
            "<span class=\"badge\">{mode_badge} · Chapter {} of {}</span></div></div><p class=\"chapter-sub\">",
            index + 1,
            HELP_CHAPTERS.len()
        );
        crate::panels::help_manual::escape_html(chapter.subtitle, &mut html);
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
            crate::panels::help_manual::escape_html(topic, &mut html);
            html.push_str("</li>");
        }
        html.push_str("</ul>");

        // Extended ActionScript / JavaScript / TypeScript / Web API code blocks & tables for chapters 11..=19
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
                crate::panels::help_manual::escape_html(rel_ch.title, &mut html);
                html.push_str("</a>");
            }
        }
        let _ = write!(
            html,
            "<a class=\"cross-link\" href=\"{hash}top\">↑ Top</a></div>"
        );

        html.push_str("<p class=\"search-terms\"><strong>Search terms:</strong> ");
        crate::panels::help_manual::escape_html(chapter.keywords, &mut html);
        html.push_str("</p></section>");
    }
    html.push_str("</main></div></div></body></html>");
    html
}
