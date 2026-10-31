//! `NF2/*` text styles: Inter (UI) and Roboto Mono (code, timecode, ruler numbers, hex).
//!
//! The fonts are bundled (SIL Open Font License, see `assets/fonts/*-LICENSE.txt`), so the UI looks
//! the same on every Mac. egui has no font weights, so every weight is its own family:
//! Regular = `Proportional`, Medium = [`INTER_MEDIUM`], Semi Bold = [`INTER_SEMIBOLD`], mono = `Monospace`.
use crate::tokens::{Family, TextSpec, TextStyle};
use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontId, RichText};
use std::sync::Arc;

pub const INTER_MEDIUM: &str = "nf2-inter-medium";
pub const INTER_SEMIBOLD: &str = "nf2-inter-semibold";

const INTER_REGULAR_TTF: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM_TTF: &[u8] = include_bytes!("../assets/fonts/Inter-Medium.ttf");
const INTER_SEMIBOLD_TTF: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");
const ROBOTO_MONO_TTF: &[u8] = include_bytes!("../assets/fonts/RobotoMono-Regular.ttf");

/// Add the NF2 fonts to existing definitions (keeps egui's fallback fonts for symbols and emoji).
/// Call once at start-up before `ctx.set_fonts`.
pub fn add_fonts(fonts: &mut FontDefinitions) {
    for (name, data) in [
        ("nf2-inter-regular", INTER_REGULAR_TTF),
        (INTER_MEDIUM, INTER_MEDIUM_TTF),
        (INTER_SEMIBOLD, INTER_SEMIBOLD_TTF),
        ("nf2-roboto-mono", ROBOTO_MONO_TTF),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(data)));
    }
    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let prepend = |fonts: &mut FontDefinitions, family: FontFamily, first: &str| {
        let list = fonts.families.entry(family).or_default();
        list.retain(|f| f != first);
        list.insert(0, first.to_owned());
    };
    prepend(fonts, FontFamily::Proportional, "nf2-inter-regular");
    prepend(fonts, FontFamily::Monospace, "nf2-roboto-mono");
    for weight in [INTER_MEDIUM, INTER_SEMIBOLD] {
        let mut list = vec![weight.to_owned()];
        list.extend(fallbacks.iter().cloned());
        fonts.families.insert(FontFamily::Name(weight.into()), list);
    }
}

/// Font definitions containing only egui defaults + NF2 fonts.
pub fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    add_fonts(&mut fonts);
    fonts
}

pub fn family(spec: &TextSpec) -> FontFamily {
    match (spec.family, spec.weight) {
        (Family::Mono, _) => FontFamily::Monospace,
        (Family::Inter, w) if w >= 600 => FontFamily::Name(INTER_SEMIBOLD.into()),
        (Family::Inter, w) if w >= 500 => FontFamily::Name(INTER_MEDIUM.into()),
        (Family::Inter, _) => FontFamily::Proportional,
    }
}

pub fn font(style: TextStyle) -> FontId {
    let spec = style.spec();
    FontId::new(spec.size, family(&spec))
}

/// Single-line text in an NF2 style (labels in rows, buttons, toolbars): family, size, tracking and
/// case. No extra leading, so the glyphs centre exactly in 28/32/40 pt rows — egui puts the extra
/// space of a taller line height below the glyphs, which pushes centred labels ~2 pt up.
/// Colour stays with the caller (`palette(ui).text_primary`, `text_secondary`, …).
pub fn rich(text: impl Into<String>, style: TextStyle) -> RichText {
    let spec = style.spec();
    let text: String = text.into();
    let text = if spec.uppercase {
        text.to_uppercase()
    } else {
        text
    };
    RichText::new(text)
        .font(FontId::new(spec.size, family(&spec)))
        .extra_letter_spacing(spec.tracking_em * spec.size)
}

/// Multi-line text (descriptions, dialog body, empty states) with the style's line height.
pub fn rich_block(text: impl Into<String>, style: TextStyle) -> RichText {
    rich(text, style).line_height(Some(style.spec().line_height))
}

/// Single-line `TextFormat` for painted labels (no extra leading; see [`rich`]).
pub fn format_line(style: TextStyle, color: egui::Color32) -> egui::TextFormat {
    egui::TextFormat {
        line_height: None,
        ..format(style, color)
    }
}

/// Multi-line `TextFormat` with the style's line height (code editor, paragraphs).
pub fn format(style: TextStyle, color: egui::Color32) -> egui::TextFormat {
    let spec = style.spec();
    egui::TextFormat {
        font_id: FontId::new(spec.size, family(&spec)),
        color,
        line_height: Some(spec.line_height),
        extra_letter_spacing: spec.tracking_em * spec.size,
        ..Default::default()
    }
}

/// Map egui's built-in text styles onto NF2 styles and register every NF2 style by name
/// (`egui::TextStyle::Name("NF2/Overline")`).
pub fn install_text_styles(style: &mut egui::Style) {
    use egui::TextStyle as E;
    style.text_styles.insert(E::Body, font(TextStyle::Body));
    style.text_styles.insert(E::Button, font(TextStyle::Body));
    style.text_styles.insert(E::Small, font(TextStyle::Small));
    style
        .text_styles
        .insert(E::Monospace, font(TextStyle::Mono));
    style.text_styles.insert(E::Heading, font(TextStyle::Title));
    for s in TextStyle::ALL {
        style
            .text_styles
            .insert(E::Name(s.spec().name.into()), font(s));
    }
}
