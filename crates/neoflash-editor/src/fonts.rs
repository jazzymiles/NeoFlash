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
        }
    }

    pub(crate) fn install_ui(&mut self, ctx: &egui::Context) {
        const UI_KEY: &str = "NeoFlash UI";
        for family in [
            "Helvetica Neue",
            "SF Pro Text",
            ".AppleSystemUIFont",
            "Helvetica",
        ] {
            if !self.families.iter().any(|available| available == family) {
                continue;
            }
            let Ok(data) = load_system_font(family, &Properties::new()) else {
                continue;
            };
            self.definitions
                .font_data
                .insert(UI_KEY.to_owned(), data.into());
            let proportional = self
                .definitions
                .families
                .entry(FontFamily::Proportional)
                .or_default();
            proportional.retain(|key| key != UI_KEY);
            proportional.insert(0, UI_KEY.to_owned());
            ctx.set_fonts(self.definitions.clone());
            return;
        }
        // Keep the bundled, validated proportional fallback on systems without
        // a renderable native face. Authoring font choices are never rewritten.
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
        let properties = Properties {
            weight: if bold { Weight::BOLD } else { Weight::NORMAL },
            style: if italic { Style::Italic } else { Style::Normal },
            ..Properties::new()
        };
        let data = load_system_font(family, &properties)?;
        let font_key = match &named {
            FontFamily::Name(name) => name.to_string(),
            _ => unreachable!(),
        };
        self.definitions
            .font_data
            .insert(font_key.clone(), data.into());
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
        let code_fonts = fonts.definitions.families[&FontFamily::Monospace].clone();
        fonts.install_ui(&ctx);
        assert_eq!(
            fonts.definitions.families[&FontFamily::Monospace],
            code_fonts
        );
        if fonts.families.iter().any(|s| s == "Helvetica Neue") {
            assert_eq!(
                fonts.definitions.families[&FontFamily::Proportional]
                    .first()
                    .map(String::as_str),
                Some("NeoFlash UI")
            );
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
