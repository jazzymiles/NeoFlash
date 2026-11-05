//! Menu paths use ▸ (and the triangles ◂ ▾ ▴); Inter and Roboto Mono lack them, so the bundled
//! NF2 Symbols fallback must provide them in every NF2 family.
use eframe::egui::{self, FontFamily, FontId};
use neoflash_ui::typography::{self, INTER_MEDIUM, INTER_SEMIBOLD};

#[test]
fn triangles_render_in_every_nf2_family() {
    let ctx = egui::Context::default();
    ctx.set_fonts(typography::font_definitions());
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    let families = [
        FontFamily::Proportional,
        FontFamily::Monospace,
        FontFamily::Name(INTER_MEDIUM.into()),
        FontFamily::Name(INTER_SEMIBOLD.into()),
    ];
    ctx.fonts_mut(|f| {
        for family in families {
            let id = FontId::new(13.0, family.clone());
            for c in ['▸', '◂', '▾', '▴', '›'] {
                assert!(f.has_glyph(&id, c), "{c} missing in {family:?}");
            }
        }
    });
}
