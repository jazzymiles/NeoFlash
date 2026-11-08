use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use font_kit::{
    family_name::FamilyName,
    properties::{Properties, Style, Weight},
    source::SystemSource,
};
use neoflash_core::{
    PathCommand, TextCharBox, TextFieldState, TextFormat, TextLayout, layout_text,
};
use std::collections::HashSet;

const BUNDLED_INTER_REGULAR: &[u8] =
    include_bytes!("../../neoflash-ui/assets/fonts/Inter-Regular.ttf");
const BUNDLED_INTER_SEMIBOLD: &[u8] =
    include_bytes!("../../neoflash-ui/assets/fonts/Inter-SemiBold.ttf");
const BUNDLED_ROBOTO_MONO: &[u8] =
    include_bytes!("../../neoflash-ui/assets/fonts/RobotoMono-Regular.ttf");

#[derive(Clone)]
pub(crate) struct TextCharacterPlacement {
    pub ch: char,
    pub bounds: TextCharBox,
    pub format: TextFormat,
}

#[derive(Clone)]
pub(crate) struct TextOutlineFragment {
    pub commands: Vec<PathCommand>,
    pub fill: [u8; 4],
}

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

    /// Extract the exact renderer glyph outlines for editable Break Apart shapes. Unlike the
    /// portable fallback metrics, this follows the selected system face, style and per-character
    /// fallback font and supports TrueType and CFF/composite outlines through `ab_glyph`.
    pub(crate) fn text_outline_commands(
        &mut self,
        family: &str,
        bold: bool,
        italic: bool,
        size: f64,
        text: &str,
    ) -> Result<Vec<PathCommand>, String> {
        use ab_glyph::{Font, OutlineCurve, PxScale, ScaleFont};

        if !size.is_finite() || size <= 0.0 {
            return Err("Text outline size must be positive and finite.".into());
        }
        let keys = self.ensure_font_data(family, bold, italic);
        let fonts: Vec<ab_glyph::FontRef<'_>> = keys
            .iter()
            .filter_map(|key| self.definitions.font_data.get(key))
            .filter_map(|data| {
                ab_glyph::FontRef::try_from_slice_and_index(&data.font, data.index).ok()
            })
            .collect();
        if fonts.is_empty() {
            return Err(format!(
                "Font '{family}' is unavailable, so its editable outlines cannot be created."
            ));
        }

        let mut commands = Vec::new();
        for (line_index, line) in text.split('\n').enumerate() {
            let mut cursor_x = 0.0_f64;
            let mut previous: Option<(usize, ab_glyph::GlyphId)> = None;
            for ch in line.chars().filter(|ch| *ch != '\r') {
                if ch == '\t' {
                    cursor_x += size * 2.0;
                    previous = None;
                    continue;
                }
                let Some((font_index, glyph_id)) =
                    fonts.iter().enumerate().find_map(|(i, font)| {
                        let id = font.glyph_id(ch);
                        (id.0 != 0).then_some((i, id))
                    })
                else {
                    if ch.is_whitespace() {
                        cursor_x += size * 0.33;
                        previous = None;
                        continue;
                    }
                    return Err(format!(
                        "No installed fallback font contains glyph U+{:04X}.",
                        ch as u32
                    ));
                };
                let font = &fonts[font_index];
                let px_scale = match font.units_per_em() {
                    Some(units) if units > 0.0 => {
                        PxScale::from(size as f32 * font.height_unscaled() / units)
                    }
                    _ => PxScale::from(size as f32),
                };
                let scaled = font.as_scaled(px_scale);
                if let Some((previous_font, previous_id)) = previous
                    && previous_font == font_index
                {
                    cursor_x += f64::from(scaled.kern(previous_id, glyph_id));
                }
                let baseline = line_index as f64 * size * 1.2 + f64::from(scaled.ascent());
                if let Some(outline) = font.outline(glyph_id) {
                    let factor_x = f64::from(scaled.h_scale_factor());
                    let factor_y = f64::from(scaled.v_scale_factor());
                    let point = |point: ab_glyph::Point| {
                        [
                            cursor_x + f64::from(point.x) * factor_x,
                            baseline - f64::from(point.y) * factor_y,
                        ]
                    };
                    let mut contour_end: Option<[f64; 2]> = None;
                    for curve in outline.curves {
                        let (start, command, end) = match curve {
                            OutlineCurve::Line(a, b) => {
                                let (a, b) = (point(a), point(b));
                                (a, PathCommand::LineTo { x: b[0], y: b[1] }, b)
                            }
                            OutlineCurve::Quad(a, control, b) => {
                                let (a, control, b) = (point(a), point(control), point(b));
                                (
                                    a,
                                    PathCommand::QuadraticTo {
                                        cx: control[0],
                                        cy: control[1],
                                        x: b[0],
                                        y: b[1],
                                    },
                                    b,
                                )
                            }
                            OutlineCurve::Cubic(a, c1, c2, b) => {
                                let (a, c1, c2, b) = (point(a), point(c1), point(c2), point(b));
                                (
                                    a,
                                    PathCommand::CubicTo {
                                        cx1: c1[0],
                                        cy1: c1[1],
                                        cx2: c2[0],
                                        cy2: c2[1],
                                        x: b[0],
                                        y: b[1],
                                    },
                                    b,
                                )
                            }
                        };
                        let continues = contour_end.is_some_and(|last| {
                            (last[0] - start[0]).hypot(last[1] - start[1]) <= 1e-5
                        });
                        if !continues {
                            if contour_end.is_some() {
                                commands.push(PathCommand::Close);
                            }
                            commands.push(PathCommand::MoveTo {
                                x: start[0],
                                y: start[1],
                            });
                        }
                        commands.push(command);
                        contour_end = Some(end);
                    }
                    if contour_end.is_some() {
                        commands.push(PathCommand::Close);
                    }
                } else if !ch.is_whitespace() {
                    return Err(format!(
                        "Glyph U+{:04X} has no editable vector outline in the selected font.",
                        ch as u32
                    ));
                }
                cursor_x += f64::from(scaled.h_advance(glyph_id));
                previous = Some((font_index, glyph_id));
            }
        }
        if commands.is_empty() && text.chars().any(|ch| !ch.is_whitespace()) {
            return Err("Text has no editable vector outlines.".into());
        }
        Ok(commands)
    }

    fn field_layout(&mut self, field: &TextFieldState) -> Result<TextLayout, String> {
        layout_text(field, |strings, format| {
            Ok(strings
                .iter()
                .map(|text| {
                    self.measure_width(&format.font, format.bold, format.italic, format.size, text)
                })
                .collect())
        })
        .map_err(|err| err.to_string())
    }

    /// Lay out every authored character with the exact font, size and face selected by its rich-
    /// text run. The returned positions are the same run positions consumed by the native text
    /// painter, including wrapping, alignment and mixed-size baseline adjustment.
    pub(crate) fn field_character_placements(
        &mut self,
        field: &TextFieldState,
    ) -> Result<Vec<TextCharacterPlacement>, String> {
        let layout = self.field_layout(field)?;
        let positioned_runs: Vec<_> = if layout.runs.is_empty() {
            layout
                .lines
                .into_iter()
                .map(|line| (line.text, line.x, line.y, field.format.clone()))
                .collect()
        } else {
            layout
                .runs
                .into_iter()
                .map(|run| {
                    (
                        run.text,
                        run.x,
                        run.y,
                        run.format.unwrap_or_else(|| field.format.clone()),
                    )
                })
                .collect()
        };
        let mut placements = Vec::new();
        for (text, run_x, run_y, format) in positioned_runs {
            for (index, (ch, x, advance)) in self
                .char_x_offsets(&format.font, format.bold, format.italic, format.size, &text)
                .into_iter()
                .enumerate()
            {
                placements.push(TextCharacterPlacement {
                    ch,
                    bounds: TextCharBox {
                        x: run_x + x + index as f64 * format.letter_spacing,
                        y: run_y,
                        width: advance.max(0.0),
                        height: (format.size * 1.2).max(0.0),
                    },
                    format: format.clone(),
                });
            }
        }
        Ok(placements)
    }

    /// Create exact editable contours without collapsing rich-text paints. Each fragment is one
    /// visible character with its authored colour; callers may combine adjacent equal-paint
    /// fragments, but must not merge different paints into one Path.
    pub(crate) fn field_outline_fragments(
        &mut self,
        field: &TextFieldState,
    ) -> Result<Vec<TextOutlineFragment>, String> {
        let mut fragments = Vec::new();
        for placement in self.field_character_placements(field)? {
            if placement.ch.is_whitespace() {
                continue;
            }
            let format = placement.format;
            let commands = self
                .text_outline_commands(
                    &format.font,
                    format.bold,
                    format.italic,
                    format.size,
                    &placement.ch.to_string(),
                )?
                .into_iter()
                .map(|command| {
                    translate_path_command(command, placement.bounds.x, placement.bounds.y)
                })
                .collect();
            fragments.push(TextOutlineFragment {
                commands,
                fill: [
                    ((format.color >> 16) & 0xff) as u8,
                    ((format.color >> 8) & 0xff) as u8,
                    (format.color & 0xff) as u8,
                    field.alpha,
                ],
            });
        }
        if fragments.is_empty() && field.text.chars().any(|ch| !ch.is_whitespace()) {
            return Err("Text has no editable vector outlines.".into());
        }
        Ok(fragments)
    }

    pub(crate) fn field_outline_commands(
        &mut self,
        field: &TextFieldState,
    ) -> Result<Vec<PathCommand>, String> {
        Ok(self
            .field_outline_fragments(field)?
            .into_iter()
            .flat_map(|fragment| fragment.commands)
            .collect())
    }
}

fn translate_path_command(command: PathCommand, dx: f64, dy: f64) -> PathCommand {
    match command {
        PathCommand::MoveTo { x, y } => PathCommand::MoveTo {
            x: x + dx,
            y: y + dy,
        },
        PathCommand::LineTo { x, y } => PathCommand::LineTo {
            x: x + dx,
            y: y + dy,
        },
        PathCommand::QuadraticTo { cx, cy, x, y } => PathCommand::QuadraticTo {
            cx: cx + dx,
            cy: cy + dy,
            x: x + dx,
            y: y + dy,
        },
        PathCommand::CubicTo {
            cx1,
            cy1,
            cx2,
            cy2,
            x,
            y,
        } => PathCommand::CubicTo {
            cx1: cx1 + dx,
            cy1: cy1 + dy,
            cx2: cx2 + dx,
            cy2: cy2 + dy,
            x: x + dx,
            y: y + dy,
        },
        PathCommand::Close => PathCommand::Close,
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

/// Return a complete, renderable font binary suitable for an authored `ResourceUnit`.
/// Bundled OFL faces are deterministic; other families use the exact selected system face.
pub(crate) fn embeddable_font_bytes(family: &str, style: &str) -> Result<Vec<u8>, String> {
    let family = family.trim();
    let style = style.trim();
    let bundled = match (family, style) {
        ("Inter", "Regular") => Some(BUNDLED_INTER_REGULAR),
        ("Inter", "Semi Bold") => Some(BUNDLED_INTER_SEMIBOLD),
        ("Roboto Mono", "Regular") => Some(BUNDLED_ROBOTO_MONO),
        _ => None,
    };
    if let Some(bytes) = bundled {
        ensure_font_embedding_allowed(bytes, family, style)?;
        return Ok(bytes.to_vec());
    }

    let properties = Properties {
        weight: if style.eq_ignore_ascii_case("Bold") {
            Weight::BOLD
        } else if style.eq_ignore_ascii_case("Semi Bold") {
            Weight::SEMIBOLD
        } else {
            Weight::NORMAL
        },
        style: if style.eq_ignore_ascii_case("Italic") {
            Style::Italic
        } else {
            Style::Normal
        },
        ..Properties::new()
    };
    let handle = SystemSource::new()
        .select_best_match(&[FamilyName::Title(family.to_owned())], &properties)
        .map_err(|error| format!("Font '{family}' ({style}) is unavailable: {error:?}"))?;
    let bytes = handle
        .load()
        .map_err(|error| format!("Cannot load font '{family}' ({style}): {error:?}"))?
        .copy_font_data()
        .ok_or_else(|| format!("Font '{family}' ({style}) does not expose embeddable data."))?
        .as_ref()
        .clone();
    ab_glyph::FontRef::try_from_slice_and_index(&bytes, 0).map_err(|_| {
        format!(
            "Font '{family}' ({style}) is a collection or unsupported outline; choose a standalone TrueType face."
        )
    })?;
    ensure_font_embedding_allowed(&bytes, family, style)?;
    Ok(bytes)
}

/// Refuse faces whose OpenType permissions explicitly prohibit embedding.
///
/// OS/2.fsType bit 1 is the OpenType `Restricted License Embedding` flag. A missing OS/2 table
/// does not assert a restriction, while a malformed permissions table is rejected rather than
/// guessed.
fn ensure_font_embedding_allowed(bytes: &[u8], family: &str, style: &str) -> Result<(), String> {
    if bytes.starts_with(b"ttcf") {
        return Err(format!(
            "Font '{family}' ({style}) is a collection; choose a standalone TrueType or OpenType face so embedding permissions can be verified."
        ));
    }
    let num_tables = bytes
        .get(4..6)
        .and_then(|value| value.try_into().ok())
        .map(u16::from_be_bytes)
        .ok_or_else(|| format!("Font '{family}' ({style}) has an invalid OpenType header."))?;
    let records_end = usize::from(num_tables)
        .checked_mul(16)
        .and_then(|length| 12usize.checked_add(length))
        .ok_or_else(|| format!("Font '{family}' ({style}) has an invalid OpenType table list."))?;
    if records_end > bytes.len() {
        return Err(format!(
            "Font '{family}' ({style}) has an incomplete OpenType table list."
        ));
    }
    for record in bytes[12..records_end].as_chunks::<16>().0 {
        if &record[..4] != b"OS/2" {
            continue;
        }
        let offset =
            u32::from_be_bytes(record[8..12].try_into().expect("four-byte slice")) as usize;
        let length =
            u32::from_be_bytes(record[12..16].try_into().expect("four-byte slice")) as usize;
        let table_end = offset
            .checked_add(length)
            .ok_or_else(|| format!("Font '{family}' ({style}) has an invalid OS/2 table range."))?;
        if length < 10 || table_end > bytes.len() {
            return Err(format!(
                "Font '{family}' ({style}) has an invalid OS/2 permissions table."
            ));
        }
        let fs_type = u16::from_be_bytes(
            bytes[offset + 8..offset + 10]
                .try_into()
                .expect("two-byte slice"),
        );
        if fs_type & 0x0002 != 0 {
            return Err(format!(
                "Font '{family}' ({style}) forbids embedding (Restricted License Embedding)."
            ));
        }
        if fs_type & 0x0004 != 0 && fs_type & 0x0008 == 0 {
            return Err(format!(
                "Font '{family}' ({style}) permits Preview & Print embedding only, not embedding in an editable authoring document."
            ));
        }
        break;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restricted_font_embedding_permission_is_rejected() {
        let mut font = vec![0_u8; 38];
        font[..4].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
        font[4..6].copy_from_slice(&1_u16.to_be_bytes());
        font[12..16].copy_from_slice(b"OS/2");
        font[20..24].copy_from_slice(&28_u32.to_be_bytes());
        font[24..28].copy_from_slice(&10_u32.to_be_bytes());
        font[36..38].copy_from_slice(&0x0002_u16.to_be_bytes());

        let error = ensure_font_embedding_allowed(&font, "Restricted", "Regular")
            .expect_err("restricted embedding must fail");
        assert!(error.contains("Restricted License Embedding"));
        font[36..38].copy_from_slice(&0x0004_u16.to_be_bytes());
        let error = ensure_font_embedding_allowed(&font, "Preview only", "Regular")
            .expect_err("preview-and-print embedding must fail for editable projects");
        assert!(error.contains("Preview & Print"));
        font[36..38].copy_from_slice(&0x0008_u16.to_be_bytes());
        ensure_font_embedding_allowed(&font, "Editable", "Regular")
            .expect("editable embedding is allowed");
        font[36..38].copy_from_slice(&0x0100_u16.to_be_bytes());
        ensure_font_embedding_allowed(&font, "No subsetting", "Regular")
            .expect("no-subsetting is allowed because complete bytes are stored");
        font[36..38].copy_from_slice(&0_u16.to_be_bytes());
        ensure_font_embedding_allowed(&font, "Installable", "Regular")
            .expect("installable embedding is allowed");

        let mut collection = b"ttcf\0\x01\0\0\0\0\0\x01\0\0\0\x10".to_vec();
        collection.resize(32, 0);
        let error = ensure_font_embedding_allowed(&collection, "Collection", "Regular")
            .expect_err("collection permissions cannot be inferred from the TTC header");
        assert!(error.contains("collection"));
    }

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
