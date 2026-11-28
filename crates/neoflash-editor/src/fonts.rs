use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use font_kit::{
    family_name::FamilyName,
    properties::{Properties, Style, Weight},
    source::SystemSource,
};
use neoflash_core::{
    PathCommand, Project, TextCharBox, TextFieldState, TextFormat, TextLayout, layout_text,
};
use std::collections::{BTreeMap, HashMap, HashSet};

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
    #[allow(dead_code)]
    pub rotation: f64,
}

#[derive(Clone)]
pub(crate) struct TextOutlineFragment {
    pub commands: Vec<PathCommand>,
    pub fill: [u8; 4],
}

pub struct Fonts {
    pub families: Vec<String>,
    definitions: FontDefinitions,
    embedded: HashMap<(String, bool, bool), FontData>,
    loaded: HashSet<(String, bool, bool)>,
    failed: HashSet<(String, bool, bool)>,
}

fn is_zero_width_cluster_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{00AD}'
            | '\u{0300}'..='\u{036F}'
            | '\u{0483}'..='\u{0489}'
            | '\u{0591}'..='\u{05BD}'
            | '\u{05BF}'
            | '\u{05C1}'..='\u{05C2}'
            | '\u{05C4}'..='\u{05C5}'
            | '\u{05C7}'
            | '\u{0610}'..='\u{061A}'
            | '\u{061C}'
            | '\u{064B}'..='\u{065F}'
            | '\u{0670}'
            | '\u{06D6}'..='\u{06ED}'
            | '\u{0711}'
            | '\u{0730}'..='\u{074A}'
            | '\u{07A6}'..='\u{07B0}'
            | '\u{07EB}'..='\u{07F3}'
            | '\u{0900}'..='\u{0902}'
            | '\u{093A}'
            | '\u{093C}'
            | '\u{0941}'..='\u{0948}'
            | '\u{094D}'
            | '\u{0951}'..='\u{0957}'
            | '\u{0962}'..='\u{0963}'
            | '\u{0E31}'
            | '\u{0E34}'..='\u{0E3A}'
            | '\u{0E47}'..='\u{0E4E}'
            | '\u{180B}'..='\u{180F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FE20}'..='\u{FE2F}'
            | '\u{FEFF}'
    )
}

fn is_ignorable_or_whitespace(ch: char) -> bool {
    ch.is_whitespace() || is_zero_width_cluster_char(ch)
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
            embedded: HashMap::new(),
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

    /// Synchronizes embedded font resources from `project` so measurement, caret layout, and
    /// `Break Apart` / `Convert text to outlines` shape against embedded OpenType font bytes even
    /// when the font is not installed system-wide.
    pub(crate) fn sync_project_embedded_fonts(&mut self, project: &Project) {
        self.embedded.clear();
        for res in &project.resources {
            if !neoflash_export::is_font_resource(res) {
                continue;
            }
            let fam = neoflash_export::font_family_for_resource(res);
            for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                if let Some(raw) =
                    neoflash_export::resolve_project_font_bytes(project, &fam, bold, italic)
                {
                    let unpacked = neoflash_core::sfnt_view_bytes(&raw)
                        .map(|cow| cow.into_owned())
                        .unwrap_or(raw);
                    if ab_glyph::FontRef::try_from_slice_and_index(&unpacked, 0).is_ok() {
                        self.embedded.insert(
                            (fam.trim().to_ascii_lowercase(), bold, italic),
                            FontData::from_owned(unpacked.clone()),
                        );
                        self.embedded.insert(
                            (res.id.trim().to_ascii_lowercase(), bold, italic),
                            FontData::from_owned(unpacked),
                        );
                    }
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn register_embedded_font_bytes(
        &mut self,
        family: &str,
        bold: bool,
        italic: bool,
        bytes: Vec<u8>,
    ) {
        let unpacked = neoflash_core::sfnt_view_bytes(&bytes)
            .map(|cow| cow.into_owned())
            .unwrap_or(bytes);
        self.embedded.insert(
            (family.trim().to_ascii_lowercase(), bold, italic),
            FontData::from_owned(unpacked),
        );
    }

    fn candidate_font_entries(
        &mut self,
        family: &str,
        bold: bool,
        italic: bool,
    ) -> Vec<(Vec<u8>, u32)> {
        let fam_lower = family.trim().to_ascii_lowercase();
        let mut entries = Vec::new();
        if let Some(embed) = self
            .embedded
            .get(&(fam_lower.clone(), bold, italic))
            .or_else(|| self.embedded.get(&(fam_lower, false, false)))
        {
            entries.push((embed.font.to_vec(), embed.index));
        }
        let keys = self.ensure_font_data(family, bold, italic);
        for key in keys {
            if let Some(data) = self.definitions.font_data.get(&key) {
                entries.push((data.font.to_vec(), data.index));
            }
        }
        entries
    }

    /// Measure per-character `(ch, x_offset, advance_width)` along a single line using
    /// `harfrust` + `unicode_bidi` OpenType `GSUB`/`GPOS` shaping.
    pub(crate) fn char_x_offsets(
        &mut self,
        family: &str,
        bold: bool,
        italic: bool,
        size: f64,
        line: &str,
    ) -> Vec<(char, f64, f64)> {
        self.measure_char_advances(family, bold, italic, size, true, 0.0, None, line)
    }

    #[cfg(test)]
    pub(crate) fn char_x_offsets_kerned(
        &mut self,
        family: &str,
        bold: bool,
        italic: bool,
        size: f64,
        kerning: bool,
        line: &str,
    ) -> Vec<(char, f64, f64)> {
        self.measure_char_advances(family, bold, italic, size, kerning, 0.0, None, line)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn measure_char_advances(
        &mut self,
        family: &str,
        bold: bool,
        italic: bool,
        size: f64,
        kerning: bool,
        letter_spacing: f64,
        base_rtl: Option<bool>,
        line: &str,
    ) -> Vec<(char, f64, f64)> {
        use ab_glyph::{Font, PxScale, ScaleFont};
        if !size.is_finite() || size <= 0.0 || line.is_empty() {
            return Vec::new();
        }
        let entries = self.candidate_font_entries(family, bold, italic);
        let fonts: Vec<ab_glyph::FontRef<'_>> = entries
            .iter()
            .filter_map(|(bytes, index)| {
                ab_glyph::FontRef::try_from_slice_and_index(bytes, *index).ok()
            })
            .collect();
        if fonts.is_empty() {
            let fallback_adv = (size * 0.56).max(0.0);
            return line
                .chars()
                .enumerate()
                .map(|(i, ch)| (ch, i as f64 * (fallback_adv + letter_spacing), fallback_adv))
                .collect();
        }

        let char_list: Vec<(usize, char)> = line.char_indices().collect();
        let mut byte_to_char_idx = BTreeMap::<usize, usize>::new();
        for (ci, &(byte_pos, _)) in char_list.iter().enumerate() {
            byte_to_char_idx.insert(byte_pos, ci);
        }
        byte_to_char_idx.insert(line.len(), char_list.len());
        let mut out: Vec<(char, f64, f64)> =
            char_list.iter().map(|&(_, ch)| (ch, 0.0, 0.0)).collect();

        let pick_font_idx = |ch: char, prev_idx: Option<usize>| -> usize {
            if is_zero_width_cluster_char(ch)
                && let Some(prev) = prev_idx
                && fonts[prev].glyph_id(ch).0 != 0
            {
                return prev;
            }
            fonts
                .iter()
                .enumerate()
                .find_map(|(idx, f)| (f.glyph_id(ch).0 != 0).then_some(idx))
                .unwrap_or(0)
        };

        let default_level = base_rtl.map(|rtl| {
            if rtl {
                unicode_bidi::RTL_LEVEL
            } else {
                unicode_bidi::LTR_LEVEL
            }
        });
        let bidi = unicode_bidi::BidiInfo::new(line, default_level);
        let mut cursor_x = 0.0_f64;

        for para in &bidi.paragraphs {
            if para.range.is_empty() {
                continue;
            }
            let (levels, visual_runs) = bidi.visual_runs(para, para.range.clone());
            for run in visual_runs {
                if run.is_empty() {
                    continue;
                }
                let is_rtl = levels[run.start].is_rtl();

                // Partition directional run into contiguous `(byte_range, Option<font_idx>)`
                // segments (`None` = `\t` tab stop).
                let mut segments: Vec<(std::ops::Range<usize>, Option<usize>)> = Vec::new();
                let mut prev_font: Option<usize> = None;
                for (rel, ch) in line[run.clone()].char_indices() {
                    let abs_start = run.start + rel;
                    let abs_end = abs_start + ch.len_utf8();
                    if ch == '\t' {
                        segments.push((abs_start..abs_end, None));
                        prev_font = None;
                        continue;
                    }
                    let fidx = if ch == '\r' {
                        prev_font.unwrap_or(0)
                    } else {
                        let chosen = pick_font_idx(ch, prev_font);
                        prev_font = Some(chosen);
                        chosen
                    };
                    if let Some((last_range, Some(last_fidx))) = segments.last_mut()
                        && *last_fidx == fidx
                        && last_range.end == abs_start
                    {
                        last_range.end = abs_end;
                    } else {
                        segments.push((abs_start..abs_end, Some(fidx)));
                    }
                }
                if is_rtl && segments.len() > 1 {
                    segments.reverse();
                }

                for (seg_range, maybe_fidx) in segments {
                    let Some(&start_ci) = byte_to_char_idx.get(&seg_range.start) else {
                        continue;
                    };
                    let Some(&end_ci) = byte_to_char_idx.get(&seg_range.end) else {
                        continue;
                    };
                    let Some(fidx) = maybe_fidx else {
                        let tab_adv = (size * 2.0).max(0.0);
                        for slot in out.iter_mut().take(end_ci).skip(start_ci) {
                            slot.1 = cursor_x.max(0.0);
                            slot.2 = tab_adv;
                            cursor_x += tab_adv + letter_spacing;
                        }
                        continue;
                    };

                    let (ref font_bytes, face_index) = entries[fidx];
                    if let Some(shaped) = neoflash_export::shape_opentype_line_with_direction(
                        font_bytes,
                        face_index,
                        &line[seg_range.clone()],
                        size,
                        kerning,
                        0.0,
                        Some(is_rtl),
                    ) {
                        if shaped.is_empty() {
                            for slot in out.iter_mut().take(end_ci).skip(start_ci) {
                                slot.1 = cursor_x.max(0.0);
                                slot.2 = 0.0;
                            }
                            continue;
                        }
                        // Build exact byte boundaries covering `seg_range` from emitted clusters.
                        let mut cluster_starts: Vec<usize> = shaped
                            .iter()
                            .map(|g| {
                                (seg_range.start + g.cluster as usize)
                                    .clamp(seg_range.start, seg_range.end)
                            })
                            .filter(|&b| byte_to_char_idx.contains_key(&b))
                            .collect();
                        cluster_starts.sort_unstable();
                        cluster_starts.dedup();
                        if cluster_starts.first().copied() != Some(seg_range.start) {
                            if cluster_starts.is_empty() {
                                cluster_starts.push(seg_range.start);
                            } else {
                                cluster_starts[0] = seg_range.start;
                            }
                        }
                        cluster_starts.push(seg_range.end);

                        let map_cluster_start = |raw_rel: u32| -> usize {
                            let abs = (seg_range.start + raw_rel as usize)
                                .clamp(seg_range.start, seg_range.end);
                            match cluster_starts[..cluster_starts.len() - 1].binary_search(&abs) {
                                Ok(idx) => idx,
                                Err(idx) => idx.saturating_sub(1),
                            }
                        };

                        // Group contiguous visual glyphs sharing the same cluster span index.
                        let mut g_idx = 0usize;
                        while g_idx < shaped.len() {
                            let span_idx = map_cluster_start(shaped[g_idx].cluster);
                            let mut cluster_adv = 0.0_f64;
                            while g_idx < shaped.len()
                                && map_cluster_start(shaped[g_idx].cluster) == span_idx
                            {
                                cluster_adv += shaped[g_idx].x_advance;
                                g_idx += 1;
                            }
                            cluster_adv = cluster_adv.max(0.0);
                            let b_start = cluster_starts[span_idx];
                            let b_end = cluster_starts[span_idx + 1];
                            let c_start =
                                byte_to_char_idx.get(&b_start).copied().unwrap_or(start_ci);
                            let c_end = byte_to_char_idx.get(&b_end).copied().unwrap_or(end_ci);
                            let count = c_end.saturating_sub(c_start);
                            if count == 0 {
                                cursor_x = (cursor_x + cluster_adv + letter_spacing).max(0.0);
                                continue;
                            }
                            let base_count = char_list[c_start..c_end]
                                .iter()
                                .filter(|&&(_, ch)| !is_zero_width_cluster_char(ch) && ch != '\r')
                                .count();
                            let mut sub_x = cursor_x.max(0.0);
                            if base_count > 0 {
                                let part_w = cluster_adv / base_count as f64;
                                let indices: Vec<usize> = if is_rtl {
                                    (c_start..c_end).rev().collect()
                                } else {
                                    (c_start..c_end).collect()
                                };
                                for ci in indices {
                                    let ch = out[ci].0;
                                    if is_zero_width_cluster_char(ch) || ch == '\r' {
                                        out[ci].1 = sub_x;
                                        out[ci].2 = 0.0;
                                    } else {
                                        out[ci].1 = sub_x;
                                        out[ci].2 = part_w;
                                        sub_x += part_w;
                                    }
                                }
                            } else {
                                let part_w = cluster_adv / count as f64;
                                for slot in &mut out[c_start..c_end] {
                                    slot.1 = sub_x;
                                    slot.2 = part_w;
                                    sub_x += part_w;
                                }
                            }
                            cursor_x = (cursor_x + cluster_adv + letter_spacing).max(0.0);
                        }
                    } else {
                        // Graceful fallback to `ab_glyph` per-char advances.
                        let font = &fonts[fidx];
                        let scale = match font.units_per_em() {
                            Some(upem) if upem > 0.0 => {
                                PxScale::from(size as f32 * font.height_unscaled() / upem)
                            }
                            _ => PxScale::from(size as f32),
                        };
                        let scaled = font.as_scaled(scale);
                        let mut prev_gid = None;
                        for slot in out.iter_mut().take(end_ci).skip(start_ci) {
                            let ch = slot.0;
                            let gid = font.glyph_id(ch);
                            if kerning && let Some(prev) = prev_gid {
                                cursor_x = (cursor_x + f64::from(scaled.kern(prev, gid))).max(0.0);
                            }
                            let adv = f64::from(scaled.h_advance(gid)).max(0.0);
                            slot.1 = cursor_x;
                            slot.2 = adv;
                            cursor_x = (cursor_x + adv + letter_spacing).max(0.0);
                            prev_gid = Some(gid);
                        }
                    }
                }
            }
        }
        out
    }

    /// Measure a single-line string width in pixels at `size` using `harfrust` + `unicode_bidi`.
    pub(crate) fn measure_width(
        &mut self,
        family: &str,
        bold: bool,
        italic: bool,
        size: f64,
        text: &str,
    ) -> f64 {
        self.measure_width_kerned(family, bold, italic, size, true, text)
    }

    pub(crate) fn measure_width_kerned(
        &mut self,
        family: &str,
        bold: bool,
        italic: bool,
        size: f64,
        kerning: bool,
        text: &str,
    ) -> f64 {
        self.measure_line_width(family, bold, italic, size, kerning, 0.0, None, text)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn measure_line_width(
        &mut self,
        family: &str,
        bold: bool,
        italic: bool,
        size: f64,
        kerning: bool,
        letter_spacing: f64,
        base_rtl: Option<bool>,
        text: &str,
    ) -> f64 {
        self.measure_char_advances(
            family,
            bold,
            italic,
            size,
            kerning,
            letter_spacing,
            base_rtl,
            text,
        )
        .into_iter()
        .map(|(_, x, w)| x + w)
        .fold(0.0_f64, f64::max)
    }

    /// Extract the exact shaped glyph outlines (`harfrust` `GSUB`/`GPOS` + `unicode_bidi` +
    /// `ab_glyph` TrueType/CFF curves) for editable `Break Apart` / `Convert text to outlines` shapes.
    #[cfg(test)]
    pub(crate) fn text_outline_commands(
        &mut self,
        family: &str,
        bold: bool,
        italic: bool,
        size: f64,
        text: &str,
    ) -> Result<Vec<PathCommand>, String> {
        self.text_outline_commands_with_options(family, bold, italic, size, true, 0.0, None, text)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn text_outline_commands_with_options(
        &mut self,
        family: &str,
        bold: bool,
        italic: bool,
        size: f64,
        kerning: bool,
        letter_spacing: f64,
        base_rtl: Option<bool>,
        text: &str,
    ) -> Result<Vec<PathCommand>, String> {
        use ab_glyph::{Font, OutlineCurve, PxScale, ScaleFont};

        if !size.is_finite() || size <= 0.0 {
            return Err("Text outline size must be positive and finite.".into());
        }
        let entries = self.candidate_font_entries(family, bold, italic);
        let fonts: Vec<ab_glyph::FontRef<'_>> = entries
            .iter()
            .filter_map(|(bytes, index)| {
                ab_glyph::FontRef::try_from_slice_and_index(bytes, *index).ok()
            })
            .collect();
        if fonts.is_empty() {
            return Err(format!(
                "Font '{family}' is unavailable, so its editable outlines cannot be created."
            ));
        }

        let append_glyph_outline = |commands: &mut Vec<PathCommand>,
                                    font: &ab_glyph::FontRef<'_>,
                                    scaled: &ab_glyph::PxScaleFont<&ab_glyph::FontRef<'_>>,
                                    glyph_id: ab_glyph::GlyphId,
                                    origin_x: f64,
                                    baseline_y: f64|
         -> bool {
            let Some(outline) = font.outline(glyph_id) else {
                return false;
            };
            if outline.curves.is_empty() {
                return false;
            }
            let factor_x = f64::from(scaled.h_scale_factor());
            let factor_y = f64::from(scaled.v_scale_factor());
            let point = |point: ab_glyph::Point| {
                [
                    origin_x + f64::from(point.x) * factor_x,
                    baseline_y - f64::from(point.y) * factor_y,
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
                let continues = contour_end
                    .is_some_and(|last| (last[0] - start[0]).hypot(last[1] - start[1]) <= 1e-5);
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
            true
        };

        let pick_font_idx = |ch: char, prev_idx: Option<usize>| -> Result<Option<usize>, String> {
            if is_zero_width_cluster_char(ch)
                && let Some(prev) = prev_idx
                && fonts[prev].glyph_id(ch).0 != 0
            {
                return Ok(Some(prev));
            }
            if let Some(idx) = fonts
                .iter()
                .enumerate()
                .find_map(|(i, f)| (f.glyph_id(ch).0 != 0).then_some(i))
            {
                Ok(Some(idx))
            } else if is_ignorable_or_whitespace(ch) {
                Ok(None)
            } else {
                Err(format!(
                    "No installed fallback font contains glyph U+{:04X}.",
                    ch as u32
                ))
            }
        };

        let default_level = base_rtl.map(|rtl| {
            if rtl {
                unicode_bidi::RTL_LEVEL
            } else {
                unicode_bidi::LTR_LEVEL
            }
        });
        let mut commands = Vec::new();

        for (line_index, line) in text.split('\n').enumerate() {
            if line.is_empty() {
                continue;
            }
            let bidi = unicode_bidi::BidiInfo::new(line, default_level);
            let mut cursor_x = 0.0_f64;

            for para in &bidi.paragraphs {
                if para.range.is_empty() {
                    continue;
                }
                let (levels, visual_runs) = bidi.visual_runs(para, para.range.clone());
                for run in visual_runs {
                    if run.is_empty() {
                        continue;
                    }
                    let is_rtl = levels[run.start].is_rtl();
                    let mut segments: Vec<(std::ops::Range<usize>, Option<usize>)> = Vec::new();
                    let mut prev_font: Option<usize> = None;

                    for (rel, ch) in line[run.clone()].char_indices() {
                        let abs_start = run.start + rel;
                        let abs_end = abs_start + ch.len_utf8();
                        if ch == '\r' {
                            continue;
                        }
                        if ch == '\t' {
                            segments.push((abs_start..abs_end, None));
                            prev_font = None;
                            continue;
                        }
                        match pick_font_idx(ch, prev_font)? {
                            Some(fidx) => {
                                prev_font = Some(fidx);
                                if let Some((last_range, Some(last_fidx))) = segments.last_mut()
                                    && *last_fidx == fidx
                                    && last_range.end == abs_start
                                {
                                    last_range.end = abs_end;
                                } else {
                                    segments.push((abs_start..abs_end, Some(fidx)));
                                }
                            }
                            None => {
                                segments.push((abs_start..abs_end, None));
                                prev_font = None;
                            }
                        }
                    }
                    if is_rtl && segments.len() > 1 {
                        segments.reverse();
                    }

                    for (seg_range, maybe_fidx) in segments {
                        let seg_text = &line[seg_range];
                        let Some(fidx) = maybe_fidx else {
                            for ch in seg_text.chars() {
                                if ch == '\t' {
                                    cursor_x += size * 2.0 + letter_spacing;
                                } else if ch.is_whitespace() {
                                    cursor_x += size * 0.33 + letter_spacing;
                                }
                            }
                            continue;
                        };
                        let font = &fonts[fidx];
                        let px_scale = match font.units_per_em() {
                            Some(units) if units > 0.0 => {
                                PxScale::from(size as f32 * font.height_unscaled() / units)
                            }
                            _ => PxScale::from(size as f32),
                        };
                        let scaled = font.as_scaled(px_scale);
                        let mut baseline_y =
                            line_index as f64 * size * 1.2 + f64::from(scaled.ascent());
                        let (ref font_bytes, face_index) = entries[fidx];

                        if let Some(shaped_seg) =
                            neoflash_export::shape_opentype_line_with_direction(
                                font_bytes,
                                face_index,
                                seg_text,
                                size,
                                kerning,
                                letter_spacing,
                                Some(is_rtl),
                            )
                        {
                            let mut boundaries: Vec<usize> = shaped_seg
                                .iter()
                                .map(|g| (g.cluster as usize).min(seg_text.len()))
                                .filter(|&b| seg_text.is_char_boundary(b))
                                .collect();
                            boundaries.push(0);
                            boundaries.push(seg_text.len());
                            boundaries.sort_unstable();
                            boundaries.dedup();
                            let mut spans = BTreeMap::new();
                            for pair in boundaries.windows(2) {
                                spans.insert(pair[0] as u32, (pair[0], pair[1]));
                            }

                            for shaped in &shaped_seg {
                                let cluster_slice = spans
                                    .get(&shaped.cluster)
                                    .and_then(|&(s, e)| seg_text.get(s..e))
                                    .unwrap_or("");
                                let all_ignorable = !cluster_slice.is_empty()
                                    && cluster_slice.chars().all(is_ignorable_or_whitespace);
                                if shaped.glyph_id == 0 {
                                    if !all_ignorable {
                                        let missing_cp = cluster_slice
                                            .chars()
                                            .find(|c| !is_ignorable_or_whitespace(*c))
                                            .map_or(0, |c| c as u32);
                                        return Err(format!(
                                            "No installed fallback font contains glyph U+{missing_cp:04X}."
                                        ));
                                    }
                                    cursor_x += shaped.x_advance;
                                    baseline_y -= shaped.y_advance;
                                    continue;
                                }
                                let emitted = append_glyph_outline(
                                    &mut commands,
                                    font,
                                    &scaled,
                                    ab_glyph::GlyphId(shaped.glyph_id),
                                    cursor_x + shaped.x_offset,
                                    baseline_y - shaped.y_offset,
                                );
                                if !emitted && !all_ignorable {
                                    let missing_cp = cluster_slice
                                        .chars()
                                        .find(|c| !is_ignorable_or_whitespace(*c))
                                        .map_or(0, |c| c as u32);
                                    return Err(format!(
                                        "Glyph U+{missing_cp:04X} has no editable vector outline in the selected font."
                                    ));
                                }
                                cursor_x += shaped.x_advance;
                                baseline_y -= shaped.y_advance;
                            }
                        } else {
                            let mut prev_id: Option<ab_glyph::GlyphId> = None;
                            for ch in seg_text.chars() {
                                let glyph_id = font.glyph_id(ch);
                                if kerning && let Some(prev) = prev_id {
                                    cursor_x += f64::from(scaled.kern(prev, glyph_id));
                                }
                                let emitted = append_glyph_outline(
                                    &mut commands,
                                    font,
                                    &scaled,
                                    glyph_id,
                                    cursor_x,
                                    baseline_y,
                                );
                                if !emitted && !is_ignorable_or_whitespace(ch) {
                                    return Err(format!(
                                        "Glyph U+{:04X} has no editable vector outline in the selected font.",
                                        ch as u32
                                    ));
                                }
                                cursor_x += f64::from(scaled.h_advance(glyph_id)) + letter_spacing;
                                prev_id = Some(glyph_id);
                            }
                        }
                    }
                }
            }
        }
        if commands.is_empty() && text.chars().any(|ch| !is_ignorable_or_whitespace(ch)) {
            return Err("Text has no editable vector outlines.".into());
        }
        Ok(commands)
    }

    fn field_layout(&mut self, field: &TextFieldState) -> Result<TextLayout, String> {
        let base_rtl =
            (!field.direction.is_empty()).then(|| field.direction.eq_ignore_ascii_case("rtl"));
        layout_text(field, |strings, format| {
            let kerning = format.kerning.unwrap_or(true);
            Ok(strings
                .iter()
                .map(|text| {
                    self.measure_line_width(
                        &format.font,
                        format.bold,
                        format.italic,
                        format.size,
                        kerning,
                        0.0,
                        base_rtl,
                        text,
                    )
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
        let base_rtl =
            (!field.direction.is_empty()).then(|| field.direction.eq_ignore_ascii_case("rtl"));
        let layout = self.field_layout(field)?;
        let positioned_runs: Vec<_> = if layout.runs.is_empty() {
            layout
                .lines
                .into_iter()
                .map(|line| (line.text, line.x, line.y, field.format.clone(), 0.0))
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
                        run.rotation,
                    )
                })
                .collect()
        };
        let mut placements = Vec::new();
        for (text, run_x, run_y, format, rotation) in positioned_runs {
            let kerning = format.kerning.unwrap_or(true);
            for (ch, x, advance) in self.measure_char_advances(
                &format.font,
                format.bold,
                format.italic,
                format.size,
                kerning,
                format.letter_spacing,
                base_rtl,
                &text,
            ) {
                placements.push(TextCharacterPlacement {
                    ch,
                    bounds: TextCharBox {
                        x: run_x + x,
                        y: run_y,
                        width: advance.max(0.0),
                        height: (format.size * 1.2).max(0.0),
                    },
                    format: format.clone(),
                    rotation,
                });
            }
        }
        Ok(placements)
    }

    /// Create exact editable contours without collapsing rich-text paints. Each uniform-format
    /// positioned run is shaped as a complete OpenType run (`GSUB` ligatures, `GPOS` kerning and
    /// mark positioning, and `unicode_bidi` directional runs) while preserving separate fragments
    /// per authored colour.
    pub(crate) fn field_outline_fragments(
        &mut self,
        field: &TextFieldState,
    ) -> Result<Vec<TextOutlineFragment>, String> {
        let base_rtl =
            (!field.direction.is_empty()).then(|| field.direction.eq_ignore_ascii_case("rtl"));
        let layout = self.field_layout(field)?;
        let positioned_runs: Vec<_> = if layout.runs.is_empty() {
            layout
                .lines
                .into_iter()
                .map(|line| (line.text, line.x, line.y, field.format.clone(), 0.0))
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
                        run.rotation,
                    )
                })
                .collect()
        };
        let mut fragments = Vec::new();
        for (text, run_x, run_y, format, rotation) in positioned_runs {
            if !text.chars().any(|ch| !is_ignorable_or_whitespace(ch)) {
                continue;
            }
            let kerning = format.kerning.unwrap_or(true);
            let run_width = self.measure_line_width(
                &format.font,
                format.bold,
                format.italic,
                format.size,
                kerning,
                format.letter_spacing,
                base_rtl,
                &text,
            );
            let cx = run_width * 0.5;
            let cy = (format.size * 1.2).max(0.0) * 0.5;
            let raw_commands = self.text_outline_commands_with_options(
                &format.font,
                format.bold,
                format.italic,
                format.size,
                kerning,
                format.letter_spacing,
                base_rtl,
                &text,
            )?;
            if raw_commands.is_empty() {
                continue;
            }
            let commands = raw_commands
                .into_iter()
                .map(|command| {
                    let cmd = if rotation.abs() > 1e-4 {
                        rotate_path_command(command, cx, cy, rotation)
                    } else {
                        command
                    };
                    translate_path_command(cmd, run_x, run_y)
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
        if fragments.is_empty() && field.text.chars().any(|ch| !is_ignorable_or_whitespace(ch)) {
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

fn rotate_point(x: f64, y: f64, cx: f64, cy: f64, cos_a: f64, sin_a: f64) -> (f64, f64) {
    let dx = x - cx;
    let dy = y - cy;
    (cx + dx * cos_a - dy * sin_a, cy + dx * sin_a + dy * cos_a)
}

fn rotate_path_command(command: PathCommand, cx: f64, cy: f64, degrees: f64) -> PathCommand {
    let rad = degrees.to_radians();
    let (cos_a, sin_a) = (rad.cos(), rad.sin());
    match command {
        PathCommand::MoveTo { x, y } => {
            let (rx, ry) = rotate_point(x, y, cx, cy, cos_a, sin_a);
            PathCommand::MoveTo { x: rx, y: ry }
        }
        PathCommand::LineTo { x, y } => {
            let (rx, ry) = rotate_point(x, y, cx, cy, cos_a, sin_a);
            PathCommand::LineTo { x: rx, y: ry }
        }
        PathCommand::QuadraticTo {
            cx: qx,
            cy: qy,
            x,
            y,
        } => {
            let (rqx, rqy) = rotate_point(qx, qy, cx, cy, cos_a, sin_a);
            let (rx, ry) = rotate_point(x, y, cx, cy, cos_a, sin_a);
            PathCommand::QuadraticTo {
                cx: rqx,
                cy: rqy,
                x: rx,
                y: ry,
            }
        }
        PathCommand::CubicTo {
            cx1,
            cy1,
            cx2,
            cy2,
            x,
            y,
        } => {
            let (rcx1, rcy1) = rotate_point(cx1, cy1, cx, cy, cos_a, sin_a);
            let (rcx2, rcy2) = rotate_point(cx2, cy2, cx, cy, cos_a, sin_a);
            let (rx, ry) = rotate_point(x, y, cx, cy, cos_a, sin_a);
            PathCommand::CubicTo {
                cx1: rcx1,
                cy1: rcy1,
                cx2: rcx2,
                cy2: rcy2,
                x: rx,
                y: ry,
            }
        }
        PathCommand::Close => PathCommand::Close,
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
        validate_embeddable_font_container(bytes, family, style)?;
        return Ok(bytes.to_vec());
    }

    let style_lower = style.to_ascii_lowercase();
    let properties = Properties {
        weight: if style_lower.contains("semi bold") || style_lower.contains("semibold") {
            Weight::SEMIBOLD
        } else if style_lower.contains("bold") {
            Weight::BOLD
        } else {
            Weight::NORMAL
        },
        style: if style_lower.contains("italic") {
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
    validate_embeddable_font_container(&bytes, family, style)?;
    Ok(bytes)
}

/// Validate the standalone sfnt container before storing it in the authored document.
///
/// OpenType `OS/2.fsType` records a font vendor's embedding policy. NeoFlash does not know what
/// separate licence or permission the author holds, so those policy bits are deliberately not an
/// authoring gate. The author remains responsible for using the font lawfully. Malformed tables
/// and unsupported collection containers still fail as technical errors.
fn validate_embeddable_font_container(
    bytes: &[u8],
    family: &str,
    style: &str,
) -> Result<(), String> {
    if bytes.starts_with(b"ttcf") {
        return Err(format!(
            "Font '{family}' ({style}) is a collection; choose a standalone TrueType or OpenType face that can be stored as one renderable font resource."
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
                "Font '{family}' ({style}) has an invalid OS/2 table."
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
    fn font_embedding_license_bits_are_not_an_authoring_gate() {
        let mut font = vec![0_u8; 38];
        font[..4].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
        font[4..6].copy_from_slice(&1_u16.to_be_bytes());
        font[12..16].copy_from_slice(b"OS/2");
        font[20..24].copy_from_slice(&28_u32.to_be_bytes());
        font[24..28].copy_from_slice(&10_u32.to_be_bytes());
        font[36..38].copy_from_slice(&0x0002_u16.to_be_bytes());

        validate_embeddable_font_container(&font, "Restricted", "Regular")
            .expect("a restricted flag does not reveal the author's separate licence");
        font[36..38].copy_from_slice(&0x0004_u16.to_be_bytes());
        validate_embeddable_font_container(&font, "Preview only", "Regular")
            .expect("preview-and-print policy is advisory, not a technical failure");
        font[36..38].copy_from_slice(&0x0008_u16.to_be_bytes());
        validate_embeddable_font_container(&font, "Editable", "Regular")
            .expect("editable embedding is allowed");
        font[36..38].copy_from_slice(&0x0100_u16.to_be_bytes());
        validate_embeddable_font_container(&font, "No subsetting", "Regular")
            .expect("no-subsetting is allowed because complete bytes are stored");
        font[36..38].copy_from_slice(&0_u16.to_be_bytes());
        validate_embeddable_font_container(&font, "Installable", "Regular")
            .expect("installable embedding is allowed");

        font[24..28].copy_from_slice(&9_u32.to_be_bytes());
        let error = validate_embeddable_font_container(&font, "Malformed", "Regular")
            .expect_err("a short OS/2 table remains a technical failure");
        assert!(error.contains("invalid OS/2 table"));

        let mut collection = b"ttcf\0\x01\0\0\0\0\0\x01\0\0\0\x10".to_vec();
        collection.resize(32, 0);
        let error = validate_embeddable_font_container(&collection, "Collection", "Regular")
            .expect_err("collections remain unsupported as authored font resources");
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

    #[test]
    fn opentype_shaping_measures_gpos_kerning_combining_marks_and_ligature_clusters() {
        let mut fonts = Fonts::new();
        fonts.register_embedded_font_bytes("Inter", false, false, BUNDLED_INTER_REGULAR.to_vec());

        // GPOS pair kerning on Inter-Regular.ttf (which has GPOS and NO legacy `kern` table!).
        let kern_on = fonts.measure_width_kerned("Inter", false, false, 48.0, true, "AV");
        let kern_off = fonts.measure_width_kerned("Inter", false, false, 48.0, false, "AV");
        assert!(
            kern_on < kern_off - 0.5,
            "expected Inter GPOS pair kerning to narrow 'AV': kern_on={kern_on} vs kern_off={kern_off}"
        );

        // Combining mark cluster mapping (`e + \u{0301}`) maps cleanly back to 2 chars without
        // panicking or adding spurious spacing advance for the combining acute accent.
        let plain_e = fonts.measure_width_kerned("Inter", false, false, 36.0, true, "e");
        let accented_offsets =
            fonts.char_x_offsets_kerned("Inter", false, false, 36.0, true, "e\u{0301}x");
        assert_eq!(accented_offsets.len(), 3);
        assert_eq!(accented_offsets[0].0, 'e');
        assert_eq!(accented_offsets[1].0, '\u{0301}');
        assert_eq!(accented_offsets[2].0, 'x');
        assert!(
            (accented_offsets[0].2 - plain_e).abs() < 1.0 && accented_offsets[1].2 == 0.0,
            "combining mark must have zero cursor width while base keeps advance: {accented_offsets:?}"
        );

        // `fi` cluster-to-char mapping and Break Apart shaped outlines succeed without misindexing.
        let fi_offsets = fonts.char_x_offsets_kerned("Inter", false, false, 36.0, true, "fi");
        assert_eq!(fi_offsets.len(), 2);
        assert!(fi_offsets[0].2 > 0.0 && fi_offsets[1].2 > 0.0);
        assert!((fi_offsets[1].1 - fi_offsets[0].2).abs() < 1e-6);

        let outlines = fonts
            .text_outline_commands("Inter", false, false, 36.0, "AV e\u{0301} fi")
            .expect("shaped outlines");
        assert!(outlines.iter().any(|cmd| matches!(
            cmd,
            PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. }
        )));
    }
}
