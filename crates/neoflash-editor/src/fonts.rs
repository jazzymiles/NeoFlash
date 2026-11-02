use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use font_kit::{
    family_name::FamilyName,
    properties::{Properties, Style, Weight},
    source::SystemSource,
};
use std::collections::HashSet;

pub struct Fonts {
    pub families: Vec<String>,
    definitions: FontDefinitions,
    loaded: HashSet<(String, bool, bool)>,
    failed: HashSet<(String, bool, bool)>,
}

pub(crate) fn styled_family(family: &str, bold: bool, italic: bool) -> FontFamily {
    let name = if !bold && !italic {
        family.to_owned()
    } else {
        format!("NeoFlash font: {family} [bold={bold}, italic={italic}]")
    };
    FontFamily::Name(name.into())
}

impl Fonts {
    pub fn new() -> Self {
        let mut families = SystemSource::new().all_families().unwrap_or_default();
        families.sort_by_key(|s| s.to_lowercase());
        families.dedup();
        Self {
            families,
            definitions: FontDefinitions::default(),
            loaded: HashSet::new(),
            failed: HashSet::new(),
        }
    }

    /// Ensure `family` is installed in `ctx` once if it is a custom system font, caching failures
    /// so unavailable families are not retried every frame.
    pub(crate) fn ensure_loaded(&mut self, ctx: &egui::Context, family: &str) {
        let trimmed = family.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("Roboto Mono") {
            return;
        }
        let key = (trimmed.to_owned(), false, false);
        if self.loaded.contains(&key) || self.failed.contains(&key) {
            return;
        }
        if self.load(ctx, trimmed).is_err() {
            self.failed.insert(key);
        }
    }

    /// Editor chrome uses the bundled NF2 fonts (Inter, Roboto Mono) from `neoflash-ui`, so the
    /// interface matches the design on every Mac. Authored font choices are never rewritten.
    pub(crate) fn install_ui(&mut self, ctx: &egui::Context) {
        neoflash_ui::typography::add_fonts(&mut self.definitions);
        ctx.set_fonts(self.definitions.clone());
    }

    pub fn load(&mut self, ctx: &egui::Context, family: &str) -> Result<FontFamily, String> {
        self.load_style(ctx, family, false, false)
    }

    pub fn load_style(
        &mut self,
        ctx: &egui::Context,
        family: &str,
        bold: bool,
        italic: bool,
    ) -> Result<FontFamily, String> {
        let named = styled_family(family, bold, italic);
        let key = (family.to_owned(), bold, italic);
        if self.loaded.contains(&key) {
            return Ok(named);
        }
        let font_key = match &named {
            FontFamily::Name(name) => name.to_string(),
            _ => unreachable!(),
        };
        if !self.definitions.font_data.contains_key(&font_key) {
            let properties = Properties {
                weight: if bold { Weight::BOLD } else { Weight::NORMAL },
                style: if italic { Style::Italic } else { Style::Normal },
                ..Properties::new()
            };
            let data = load_system_font(family, &properties)?;
            self.definitions
                .font_data
                .insert(font_key.clone(), data.into());
        }
        let mut fallback = vec![font_key];
        fallback.extend(
            self.definitions
                .families
                .get(&FontFamily::Proportional)
                .cloned()
                .unwrap_or_default(),
        );
        self.definitions.families.insert(named.clone(), fallback);
        self.loaded.insert(key);
        ctx.set_fonts(self.definitions.clone());
        Ok(named)
    }

    fn ensure_font_data(&mut self, family: &str, bold: bool, italic: bool) -> Vec<String> {
        let named = styled_family(family, bold, italic);
        let font_key = match &named {
            FontFamily::Name(name) => name.to_string(),
            _ => unreachable!(),
        };
        let key = (family.to_owned(), bold, italic);
        if !self.definitions.font_data.contains_key(&font_key) && !self.failed.contains(&key) {
            let properties = Properties {
                weight: if bold { Weight::BOLD } else { Weight::NORMAL },
                style: if italic { Style::Italic } else { Style::Normal },
                ..Properties::new()
            };
            match load_system_font(family, &properties) {
                Ok(data) => {
                    self.definitions
                        .font_data
                        .insert(font_key.clone(), data.into());
                }
                Err(_) => {
                    self.failed.insert(key);
                }
            }
        }
        let mut keys = Vec::new();
        if self.definitions.font_data.contains_key(&font_key) {
            keys.push(font_key);
        }
        if let Some(prop) = self.definitions.families.get(&FontFamily::Proportional) {
            for k in prop {
                if !keys.contains(k) && self.definitions.font_data.contains_key(k) {
                    keys.push(k.clone());
                }
            }
        }
        keys
    }

    /// Measure per-character `(ch, x_offset, advance_width)` along a single line using `ab_glyph`.
    pub(crate) fn char_x_offsets(
        &mut self,
        family: &str,
        bold: bool,
        italic: bool,
        size: f64,
        line: &str,
    ) -> Vec<(char, f64, f64)> {
        use ab_glyph::{Font, PxScale, ScaleFont};
        if !size.is_finite() || size <= 0.0 || line.is_empty() {
            return Vec::new();
        }
        let keys = self.ensure_font_data(family, bold, italic);
        let fonts: Vec<ab_glyph::FontRef<'_>> = keys
            .iter()
            .filter_map(|k| self.definitions.font_data.get(k))
            .filter_map(|d| ab_glyph::FontRef::try_from_slice_and_index(&d.font, d.index).ok())
            .collect();
        if fonts.is_empty() {
            let fallback_adv = (size * 0.56).max(0.0);
            return line
                .chars()
                .enumerate()
                .map(|(i, ch)| (ch, i as f64 * fallback_adv, fallback_adv))
                .collect();
        }
        let mut out = Vec::with_capacity(line.chars().count());
        let mut cursor_x = 0.0_f64;
        let mut prev_glyph: Option<(usize, ab_glyph::GlyphId)> = None;
        for ch in line.chars() {
            if ch == '\t' {
                let adv = (size * 2.0).max(0.0);
                out.push((ch, cursor_x, adv));
                cursor_x += adv;
                prev_glyph = None;
                continue;
            }
            let (font_idx, gid) = fonts
                .iter()
                .enumerate()
                .find_map(|(idx, f)| {
                    let g = f.glyph_id(ch);
                    (g.0 != 0).then_some((idx, g))
                })
                .unwrap_or_else(|| (0, fonts[0].glyph_id(ch)));
            let font = &fonts[font_idx];
            let scale = match font.units_per_em() {
                Some(upem) if upem > 0.0 => {
                    PxScale::from(size as f32 * font.height_unscaled() / upem)
                }
                _ => PxScale::from(size as f32),
            };
            let scaled = font.as_scaled(scale);
            if let Some((prev_idx, prev_id)) = prev_glyph
                && prev_idx == font_idx
            {
                cursor_x = (cursor_x + scaled.kern(prev_id, gid) as f64).max(0.0);
            }
            let adv = (scaled.h_advance(gid) as f64).max(0.0);
            out.push((ch, cursor_x, adv));
            cursor_x += adv;
            prev_glyph = Some((font_idx, gid));
        }
        out
    }

    /// Measure a single-line string width in pixels at `size` using `ab_glyph`.
    pub(crate) fn measure_width(
        &mut self,
        family: &str,
        bold: bool,
        italic: bool,
        size: f64,
        text: &str,
    ) -> f64 {
        self.char_x_offsets(family, bold, italic, size, text)
            .last()
            .map_or(0.0, |(_, x, w)| (x + w).max(0.0))
    }
}

fn load_system_font(family: &str, properties: &Properties) -> Result<FontData, String> {
    let handle = SystemSource::new()
        .select_best_match(&[FamilyName::Title(family.to_owned())], properties)
        .map_err(|e| format!("Font '{family}' is unavailable: {e:?}"))?;
    let (mut bytes, mut font_index) = match &handle {
        font_kit::handle::Handle::Path { path, font_index } => {
            (std::fs::read(path).map_err(|e| e.to_string())?, *font_index)
        }
        font_kit::handle::Handle::Memory { bytes, font_index } => {
            (bytes.as_ref().clone(), *font_index)
        }
    };
    // CoreText's loader may unpack a collection to a standalone sfnt. In that
    // case the original collection index no longer refers to the returned data.
    if ab_glyph::FontRef::try_from_slice_and_index(&bytes, font_index).is_err() {
        bytes = handle
            .load()
            .map_err(|e| format!("Cannot load font: {e:?}"))?
            .copy_font_data()
            .ok_or("Font data is not accessible")?
            .as_ref()
            .clone();
        font_index = 0;
    }
    ab_glyph::FontRef::try_from_slice_and_index(&bytes,font_index)
        .map_err(|_|format!("Font '{family}' cannot be rendered by the current text backend; a fallback will be shown."))?;
    let mut data = FontData::from_owned(bytes);
    data.index = font_index;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(target_os = "macos")]
    fn system_collection_font_can_be_installed_without_panicking() {
        let mut fonts = Fonts::new();
        let ctx = egui::Context::default();
        fonts.install_ui(&ctx);
        // NF2: Inter for chrome, Roboto Mono for code, both bundled.
        let first = |family: FontFamily| fonts.definitions.families[&family].first().cloned();
        assert_eq!(
            first(FontFamily::Proportional).as_deref(),
            Some("nf2-inter-regular")
        );
        assert_eq!(
            first(FontFamily::Monospace).as_deref(),
            Some("nf2-roboto-mono")
        );
        if fonts.families.iter().any(|s| s == "Helvetica Neue") {
            let authored = fonts.load(&ctx, "Helvetica Neue").unwrap();
            assert_ne!(authored, FontFamily::Proportional);
            assert!(fonts.definitions.families.contains_key(&authored));
            let bold = fonts
                .load_style(&ctx, "Helvetica Neue", true, false)
                .unwrap();
            let italic = fonts
                .load_style(&ctx, "Helvetica Neue", false, true)
                .unwrap();
            assert_ne!(bold, italic);
            assert_eq!(
                fonts
                    .load_style(&ctx, "Helvetica Neue", true, false)
                    .unwrap(),
                bold
            );
            let _ = ctx.run(egui::RawInput::default(), |_| {});
        }
    }
}
