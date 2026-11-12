// TrueType (`glyf` + composite), OpenType/CFF, and bitmap/color font table inspection
// plus `TextAutoSize` and advanced typography accessors for `neoflash-core`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextAutoSize {
    #[default]
    None,
    Left,
    Center,
    Right,
}

impl TextAutoSize {
    pub const ALL: [Self; 4] = [Self::None, Self::Left, Self::Center, Self::Right];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }

    pub fn from_mode_str(mode: &str) -> Self {
        match mode.trim().to_ascii_lowercase().as_str() {
            "left" | "true" => Self::Left,
            "center" => Self::Center,
            "right" => Self::Right,
            _ => Self::None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Auto-size: None",
            Self::Left => "Auto-size: Left",
            Self::Center => "Auto-size: Center",
            Self::Right => "Auto-size: Right",
        }
    }

    pub fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

impl AuthoredTextFieldOptions {
    pub fn auto_size_mode(&self) -> TextAutoSize {
        TextAutoSize::from_mode_str(&self.auto_size)
    }

    pub fn set_auto_size_mode(&mut self, mode: TextAutoSize) {
        self.auto_size = mode.as_str().to_string();
    }

    pub fn figure_style(&self) -> (&str, &str) {
        (self.digit_case.as_str(), self.digit_width.as_str())
    }

    pub fn set_figure_style(&mut self, digit_case: &str, digit_width: &str) {
        self.digit_case = digit_case.to_string();
        self.digit_width = digit_width.to_string();
    }

    pub fn case_transform(&self) -> (&str, &str) {
        (self.typographic_case.as_str(), self.locale.as_str())
    }

    pub fn set_case_transform(&mut self, typographic_case: &str, locale: &str) {
        self.typographic_case = typographic_case.to_string();
        self.locale = locale.to_string();
    }

    pub fn vertical_rotation(&self) -> &str {
        self.glyph_rotation.as_str()
    }

    pub fn set_vertical_rotation(&mut self, glyph_rotation: &str) {
        self.glyph_rotation = glyph_rotation.to_string();
    }
}

impl TextFieldState {
    pub fn auto_size_mode(&self) -> TextAutoSize {
        TextAutoSize::from_mode_str(&self.as2_viewport.auto_size)
    }

    pub fn set_auto_size_mode(&mut self, mode: TextAutoSize) {
        self.as2_viewport.auto_size = mode.as_str().to_string();
        if mode.is_active() {
            self.word_wrap = false;
        }
    }

    pub fn figure_style(&self) -> (&str, &str) {
        (self.digit_case.as_str(), self.digit_width.as_str())
    }

    pub fn set_figure_style(&mut self, digit_case: &str, digit_width: &str) {
        self.digit_case = digit_case.to_string();
        self.digit_width = digit_width.to_string();
    }

    pub fn case_transform(&self) -> (&str, &str) {
        (self.typographic_case.as_str(), self.locale.as_str())
    }

    pub fn set_case_transform(&mut self, typographic_case: &str, locale: &str) {
        self.typographic_case = typographic_case.to_string();
        self.locale = locale.to_string();
    }

    pub fn vertical_rotation(&self) -> &str {
        self.glyph_rotation.as_str()
    }

    pub fn set_vertical_rotation(&mut self, glyph_rotation: &str) {
        self.glyph_rotation = glyph_rotation.to_string();
    }
}

impl TextFormat {
    pub fn figure_style(&self) -> (String, String) {
        let opts = self.field_options().unwrap_or_default();
        (opts.digit_case, opts.digit_width)
    }

    pub fn set_figure_style(&mut self, digit_case: &str, digit_width: &str) {
        let mut opts = self.field_options().unwrap_or_default();
        opts.set_figure_style(digit_case, digit_width);
        self.set_field_options(Some(opts));
    }

    pub fn case_transform(&self) -> (String, String) {
        let opts = self.field_options().unwrap_or_default();
        (opts.typographic_case, opts.locale)
    }

    pub fn set_case_transform(&mut self, typographic_case: &str, locale: &str) {
        let mut opts = self.field_options().unwrap_or_default();
        opts.set_case_transform(typographic_case, locale);
        self.set_field_options(Some(opts));
    }

    pub fn vertical_rotation(&self) -> String {
        self.field_options().unwrap_or_default().glyph_rotation
    }

    pub fn set_vertical_rotation(&mut self, glyph_rotation: &str) {
        let mut opts = self.field_options().unwrap_or_default();
        opts.set_vertical_rotation(glyph_rotation);
        self.set_field_options(Some(opts));
    }

    /// Inspects SFNT (`.ttf`/`.otf`) or WOFF1 (`wOFF`) table tags.
    pub fn sfnt_table_tags(font_bytes: &[u8]) -> Vec<String> {
        if font_bytes.len() < 12 {
            return Vec::new();
        }
        let is_woff = &font_bytes[0..4] == b"wOFF";
        let num_tables = if is_woff {
            if font_bytes.len() < 44 {
                return Vec::new();
            }
            u16::from_be_bytes([font_bytes[12], font_bytes[13]]) as usize
        } else {
            let sig = &font_bytes[0..4];
            if sig != [0, 1, 0, 0] && sig != b"OTTO" && sig != b"true" && sig != b"typ1" {
                return Vec::new();
            }
            u16::from_be_bytes([font_bytes[4], font_bytes[5]]) as usize
        };
        let mut out = Vec::new();
        for i in 0..num_tables.min(128) {
            let entry = if is_woff { 44 + i * 20 } else { 12 + i * 16 };
            let entry_len = if is_woff { 20 } else { 16 };
            if entry + entry_len > font_bytes.len() {
                break;
            }
            let tag_bytes = &font_bytes[entry..entry + 4];
            if let Ok(tag) = std::str::from_utf8(tag_bytes) {
                out.push(tag.to_string());
            }
        }
        out
    }

    /// Returns any bitmap or color font tables (`CBDT`, `CBLC`, `sbix`, `COLR`, `CPAL`, `EBDT`, `SVG `)
    /// present in `font_bytes` that cannot be represented purely as monochrome vector outlines.
    pub fn unsupported_outline_font_tables(font_bytes: &[u8]) -> Vec<String> {
        const UNSUPPORTED_TABLES: &[&str] = &[
            "CBDT", "CBLC", "sbix", "COLR", "CPAL", "EBDT", "EBLC", "SVG ",
        ];
        let tags = Self::sfnt_table_tags(font_bytes);
        let mut found = Vec::new();
        for tag in tags {
            if UNSUPPORTED_TABLES.contains(&tag.as_str()) && !found.contains(&tag) {
                found.push(tag);
            }
        }
        found
    }

    /// Returns `(has_glyf, has_cff)` for `font_bytes`.
    pub fn has_cff_or_glyf_outlines(font_bytes: &[u8]) -> (bool, bool) {
        let tags = Self::sfnt_table_tags(font_bytes);
        let has_glyf = tags.iter().any(|t| t == "glyf");
        let has_cff = tags.iter().any(|t| t == "CFF " || t == "CFF2");
        (has_glyf, has_cff)
    }

    /// Returns non-whitespace Unicode scalar values in `text` that are not mapped in `cmap`
    /// of `font_bytes`.
    pub fn missing_outline_codepoints(font_bytes: &[u8], text: &str) -> Vec<u32> {
        let Some(cmap_map) = parse_sfnt_cmap_table(font_bytes) else {
            return Vec::new();
        };
        let mut missing = Vec::new();
        for ch in text.chars() {
            if ch.is_whitespace() {
                continue;
            }
            let cp = ch as u32;
            let gid = cmap_map.get(&cp).copied().unwrap_or(0);
            if gid == 0 && !missing.contains(&cp) {
                missing.push(cp);
            }
        }
        missing
    }

    /// Validates whether `font_bytes` can faithfully convert `text` to vector outlines at `location`.
    /// Emits actionable located diagnostics when bitmap/color tables (`CBDT`, `sbix`, `COLR`) are used
    /// or when `text` references glyphs missing from the font's coverage.
    pub fn validate_text_outlines_for_font(
        location: &str,
        family: &str,
        text: &str,
        font_bytes: &[u8],
    ) -> Vec<String> {
        let mut diagnostics = Vec::new();
        let unsupported = Self::unsupported_outline_font_tables(font_bytes);
        let (has_glyf, has_cff) = Self::has_cff_or_glyf_outlines(font_bytes);
        if !unsupported.is_empty() {
            diagnostics.push(format!(
                "{location}: font '{family}' contains unsupported bitmap/color table(s) [{}]; retaining visible text fallback instead of dropping glyphs",
                unsupported.join(", ")
            ));
        } else if !font_bytes.is_empty()
            && !font_bytes.starts_with(b"NFPM")
            && !has_glyf
            && !has_cff
        {
            diagnostics.push(format!(
                "{location}: font '{family}' has no TrueType ('glyf') or OpenType/CFF ('CFF '/'CFF2') outline tables; retaining visible text fallback"
            ));
        }

        let missing = Self::missing_outline_codepoints(font_bytes, text);
        if !missing.is_empty() {
            let formatted = missing
                .iter()
                .map(|cp| {
                    let ch = char::from_u32(*cp).unwrap_or('\u{FFFD}');
                    format!("U+{cp:04X} ('{ch}')")
                })
                .collect::<Vec<_>>()
                .join(", ");
            diagnostics.push(format!(
                "{location}: font '{family}' is missing outline glyph coverage for {formatted}; retaining visible text fallback"
            ));
        }
        diagnostics
    }
}

impl TextFormat {
    /// Extracts SFNT (`glyf` + composite) outline commands for `text` using `PortableFontMetrics`,
    /// returning `Err(missing_labels)` if any non-whitespace character lacks an outline.
    pub fn extract_sfnt_outline_commands(
        family: &str,
        text: &str,
        font_size: f64,
        font_bytes: &[u8],
    ) -> Result<Vec<crate::PathCommand>, Vec<String>> {
        let metrics = PortableFontMetrics::from_bytes(family, font_bytes)
            .unwrap_or_else(|_| PortableFontMetrics::builtin(family));
        let upm = f64::from(metrics.units_per_em.max(1));
        let scale = font_size.max(1.0) / upm;
        let line_h = font_size.max(1.0) * 1.2;
        let mut missing_chars = Vec::new();
        let mut commands = Vec::new();
        for (line_idx, line) in text.split('\n').enumerate() {
            let top_y = (line_idx as f64) * line_h;
            let mut cursor_x = 0.0;
            for ch in line.chars() {
                if ch == '\r' {
                    continue;
                }
                let adv = f64::from(metrics.advance_for_char(ch)) * scale;
                if ch.is_whitespace() {
                    cursor_x += adv;
                    continue;
                }
                if !metrics.has_outline_for_char(ch) {
                    let cp = ch as u32;
                    let item = format!("U+{cp:04X} ('{ch}')");
                    if !missing_chars.contains(&item) {
                        missing_chars.push(item);
                    }
                    cursor_x += adv;
                    continue;
                }
                if let Some(glyph_cmds) = metrics.try_glyph_outline(ch, cursor_x, top_y, font_size)
                {
                    commands.extend(glyph_cmds);
                }
                cursor_x += adv;
            }
        }
        if !missing_chars.is_empty() {
            return Err(missing_chars);
        }
        Ok(commands)
    }
}

impl PortableFontMetrics {
    pub fn has_outline_for_char(&self, ch: char) -> bool {
        if ch.is_whitespace() {
            return true;
        }
        let cp = ch as u32;
        self.glyphs.get(&cp).is_some_and(|cmds| !cmds.is_empty())
    }

    /// Returns `Some(commands)` only when real vector outlines exist for `ch` (or `Some(vec![])` for whitespace),
    /// returning `None` when outline coverage is absent so exporters can emit diagnostics and retain visible text.
    pub fn try_glyph_outline(
        &self,
        ch: char,
        x: f64,
        y: f64,
        size: f64,
    ) -> Option<Vec<crate::PathCommand>> {
        if ch.is_whitespace() {
            return Some(Vec::new());
        }
        let cp = ch as u32;
        let contours = self.glyphs.get(&cp)?;
        if contours.is_empty() {
            return None;
        }
        Some(self.glyph_outline(ch, x, y, size))
    }
}

fn parse_sfnt_cmap_table(bytes: &[u8]) -> Option<std::collections::BTreeMap<u32, u16>> {
    if bytes.len() < 12 {
        return None;
    }
    let is_woff = &bytes[0..4] == b"wOFF";
    let num_tables = if is_woff {
        if bytes.len() < 44 {
            return None;
        }
        u16::from_be_bytes([bytes[12], bytes[13]]) as usize
    } else {
        let sig = &bytes[0..4];
        if sig != [0, 1, 0, 0] && sig != b"OTTO" && sig != b"true" && sig != b"typ1" {
            return None;
        }
        u16::from_be_bytes([bytes[4], bytes[5]]) as usize
    };
    let mut cmap_slice: Option<&[u8]> = None;
    for i in 0..num_tables.min(128) {
        if is_woff {
            let entry = 44 + i * 20;
            if entry + 20 > bytes.len() {
                break;
            }
            let tag = &bytes[entry..entry + 4];
            let offset = u32::from_be_bytes([
                bytes[entry + 4],
                bytes[entry + 5],
                bytes[entry + 6],
                bytes[entry + 7],
            ]) as usize;
            let comp_len = u32::from_be_bytes([
                bytes[entry + 8],
                bytes[entry + 9],
                bytes[entry + 10],
                bytes[entry + 11],
            ]) as usize;
            let orig_len = u32::from_be_bytes([
                bytes[entry + 12],
                bytes[entry + 13],
                bytes[entry + 14],
                bytes[entry + 15],
            ]) as usize;
            if tag == b"cmap"
                && comp_len == orig_len
                && offset.saturating_add(comp_len) <= bytes.len()
            {
                cmap_slice = Some(&bytes[offset..offset + comp_len]);
                break;
            }
        } else {
            let entry = 12 + i * 16;
            if entry + 16 > bytes.len() {
                break;
            }
            let tag = &bytes[entry..entry + 4];
            let offset = u32::from_be_bytes([
                bytes[entry + 8],
                bytes[entry + 9],
                bytes[entry + 10],
                bytes[entry + 11],
            ]) as usize;
            let length = u32::from_be_bytes([
                bytes[entry + 12],
                bytes[entry + 13],
                bytes[entry + 14],
                bytes[entry + 15],
            ]) as usize;
            if tag == b"cmap" && offset.saturating_add(length) <= bytes.len() {
                cmap_slice = Some(&bytes[offset..offset + length]);
                break;
            }
        }
    }
    let cmap = cmap_slice?;
    if cmap.len() < 4 {
        return None;
    }
    let mut cmap_map = std::collections::BTreeMap::<u32, u16>::new();
    let subtables = u16::from_be_bytes([cmap[2], cmap[3]]) as usize;
    for i in 0..subtables.min(32) {
        let rec = 4 + i * 8;
        if rec + 8 > cmap.len() {
            break;
        }
        let sub_off =
            u32::from_be_bytes([cmap[rec + 4], cmap[rec + 5], cmap[rec + 6], cmap[rec + 7]])
                as usize;
        if sub_off + 4 > cmap.len() {
            continue;
        }
        let fmt = u16::from_be_bytes([cmap[sub_off], cmap[sub_off + 1]]);
        if fmt == 4 && sub_off + 14 <= cmap.len() {
            let seg_count_x2 = u16::from_be_bytes([cmap[sub_off + 6], cmap[sub_off + 7]]) as usize;
            let seg_count = seg_count_x2 / 2;
            let end_codes = sub_off + 14;
            let start_codes = end_codes + seg_count_x2 + 2;
            let id_deltas = start_codes + seg_count_x2;
            let id_range_offsets = id_deltas + seg_count_x2;
            if id_range_offsets + seg_count_x2 <= cmap.len() {
                for s in 0..seg_count {
                    let end_c =
                        u16::from_be_bytes([cmap[end_codes + s * 2], cmap[end_codes + s * 2 + 1]]);
                    let start_c = u16::from_be_bytes([
                        cmap[start_codes + s * 2],
                        cmap[start_codes + s * 2 + 1],
                    ]);
                    if start_c == 0xFFFF {
                        break;
                    }
                    let delta =
                        i16::from_be_bytes([cmap[id_deltas + s * 2], cmap[id_deltas + s * 2 + 1]])
                            as i32;
                    let ro_pos = id_range_offsets + s * 2;
                    let ro = u16::from_be_bytes([cmap[ro_pos], cmap[ro_pos + 1]]) as usize;
                    for c in start_c..=end_c {
                        let gid = if ro == 0 {
                            ((c as i32 + delta) & 0xFFFF) as u16
                        } else {
                            let glyph_idx_off = ro_pos + ro + ((c - start_c) as usize) * 2;
                            if glyph_idx_off + 2 <= cmap.len() {
                                let raw = u16::from_be_bytes([
                                    cmap[glyph_idx_off],
                                    cmap[glyph_idx_off + 1],
                                ]);
                                if raw == 0 {
                                    0
                                } else {
                                    ((raw as i32 + delta) & 0xFFFF) as u16
                                }
                            } else {
                                0
                            }
                        };
                        if gid > 0 {
                            cmap_map.insert(u32::from(c), gid);
                        }
                    }
                }
            }
        } else if fmt == 12 && sub_off + 16 <= cmap.len() {
            let n_groups = u32::from_be_bytes([
                cmap[sub_off + 12],
                cmap[sub_off + 13],
                cmap[sub_off + 14],
                cmap[sub_off + 15],
            ]) as usize;
            for g in 0..n_groups.min(4096) {
                let grp = sub_off + 16 + g * 12;
                if grp + 12 > cmap.len() {
                    break;
                }
                let start_c =
                    u32::from_be_bytes([cmap[grp], cmap[grp + 1], cmap[grp + 2], cmap[grp + 3]]);
                let end_c = u32::from_be_bytes([
                    cmap[grp + 4],
                    cmap[grp + 5],
                    cmap[grp + 6],
                    cmap[grp + 7],
                ]);
                let start_gid = u32::from_be_bytes([
                    cmap[grp + 8],
                    cmap[grp + 9],
                    cmap[grp + 10],
                    cmap[grp + 11],
                ]);
                if end_c.saturating_sub(start_c) <= 1024 {
                    for c in start_c..=end_c {
                        let gid = (start_gid + (c - start_c)) as u16;
                        if gid > 0 {
                            cmap_map.insert(c, gid);
                        }
                    }
                }
            }
        }
    }
    Some(cmap_map)
}

fn glyf_slice_for_gid<'a>(
    glyf: &'a [u8],
    loca: &[u8],
    index_to_loc_format: i16,
    gid: u16,
) -> Option<&'a [u8]> {
    let g = gid as usize;
    let (g_start, g_end) = if index_to_loc_format == 0 {
        if (g + 2) * 2 > loca.len() {
            return None;
        }
        (
            (u16::from_be_bytes([loca[g * 2], loca[g * 2 + 1]]) as usize) * 2,
            (u16::from_be_bytes([loca[(g + 1) * 2], loca[(g + 1) * 2 + 1]]) as usize) * 2,
        )
    } else {
        if (g + 2) * 4 > loca.len() {
            return None;
        }
        (
            u32::from_be_bytes([
                loca[g * 4],
                loca[g * 4 + 1],
                loca[g * 4 + 2],
                loca[g * 4 + 3],
            ]) as usize,
            u32::from_be_bytes([
                loca[(g + 1) * 4],
                loca[(g + 1) * 4 + 1],
                loca[(g + 1) * 4 + 2],
                loca[(g + 1) * 4 + 3],
            ]) as usize,
        )
    };
    if g_end > g_start && g_end <= glyf.len() {
        Some(&glyf[g_start..g_end])
    } else {
        None
    }
}

/// Decodes a TrueType `glyf` glyph outline, supporting both simple contours (`num_contours > 0`)
/// and composite glyphs (`num_contours < 0`) with recursive component assembly and 2x2 affine transforms.
pub(crate) fn parse_glyf_outline_with_composites(
    glyf: &[u8],
    loca: &[u8],
    index_to_loc_format: i16,
    gid: u16,
    depth: usize,
) -> Option<Vec<crate::PathCommand>> {
    if depth > 8 {
        return None;
    }
    let slice = glyf_slice_for_gid(glyf, loca, index_to_loc_format, gid)?;
    if slice.len() < 10 {
        return None;
    }
    let num_contours = i16::from_be_bytes([slice[0], slice[1]]);
    if num_contours > 0 {
        return parse_simple_glyf_outline(slice);
    }
    if num_contours == 0 {
        return None;
    }

    // Composite glyph (`num_contours < 0`)
    const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
    const ARGS_ARE_XY_VALUES: u16 = 0x0002;
    const WE_HAVE_A_SCALE: u16 = 0x0008;
    const MORE_COMPONENTS: u16 = 0x0020;
    const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
    const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;

    let mut pos = 10usize;
    let mut combined = Vec::new();
    for _ in 0..64 {
        if pos + 4 > slice.len() {
            break;
        }
        let flags = u16::from_be_bytes([slice[pos], slice[pos + 1]]);
        let comp_gid = u16::from_be_bytes([slice[pos + 2], slice[pos + 3]]);
        pos += 4;

        let (dx, dy) = if (flags & ARG_1_AND_2_ARE_WORDS) != 0 {
            if pos + 4 > slice.len() {
                break;
            }
            let a1 = i16::from_be_bytes([slice[pos], slice[pos + 1]]);
            let a2 = i16::from_be_bytes([slice[pos + 2], slice[pos + 3]]);
            pos += 4;
            if (flags & ARGS_ARE_XY_VALUES) != 0 {
                (f64::from(a1), f64::from(a2))
            } else {
                (0.0, 0.0)
            }
        } else {
            if pos + 2 > slice.len() {
                break;
            }
            let a1 = slice[pos] as i8;
            let a2 = slice[pos + 1] as i8;
            pos += 2;
            if (flags & ARGS_ARE_XY_VALUES) != 0 {
                (f64::from(a1), f64::from(a2))
            } else {
                (0.0, 0.0)
            }
        };

        let mut m00 = 1.0f64;
        let mut m01 = 0.0f64;
        let mut m10 = 0.0f64;
        let mut m11 = 1.0f64;
        if (flags & WE_HAVE_A_SCALE) != 0 {
            if pos + 2 > slice.len() {
                break;
            }
            let s = f64::from(i16::from_be_bytes([slice[pos], slice[pos + 1]])) / 16384.0;
            pos += 2;
            m00 = s;
            m11 = s;
        } else if (flags & WE_HAVE_AN_X_AND_Y_SCALE) != 0 {
            if pos + 4 > slice.len() {
                break;
            }
            m00 = f64::from(i16::from_be_bytes([slice[pos], slice[pos + 1]])) / 16384.0;
            m11 = f64::from(i16::from_be_bytes([slice[pos + 2], slice[pos + 3]])) / 16384.0;
            pos += 4;
        } else if (flags & WE_HAVE_A_TWO_BY_TWO) != 0 {
            if pos + 8 > slice.len() {
                break;
            }
            m00 = f64::from(i16::from_be_bytes([slice[pos], slice[pos + 1]])) / 16384.0;
            m01 = f64::from(i16::from_be_bytes([slice[pos + 2], slice[pos + 3]])) / 16384.0;
            m10 = f64::from(i16::from_be_bytes([slice[pos + 4], slice[pos + 5]])) / 16384.0;
            m11 = f64::from(i16::from_be_bytes([slice[pos + 6], slice[pos + 7]])) / 16384.0;
            pos += 8;
        }

        if let Some(sub_cmds) =
            parse_glyf_outline_with_composites(glyf, loca, index_to_loc_format, comp_gid, depth + 1)
        {
            let tx_pt =
                |x: f64, y: f64| -> (f64, f64) { (m00 * x + m10 * y + dx, m01 * x + m11 * y + dy) };
            for cmd in sub_cmds {
                let mapped = match cmd {
                    crate::PathCommand::MoveTo { x, y } => {
                        let (nx, ny) = tx_pt(x, y);
                        crate::PathCommand::MoveTo { x: nx, y: ny }
                    }
                    crate::PathCommand::LineTo { x, y } => {
                        let (nx, ny) = tx_pt(x, y);
                        crate::PathCommand::LineTo { x: nx, y: ny }
                    }
                    crate::PathCommand::QuadraticTo { cx, cy, x, y } => {
                        let (ncx, ncy) = tx_pt(cx, cy);
                        let (nx, ny) = tx_pt(x, y);
                        crate::PathCommand::QuadraticTo {
                            cx: ncx,
                            cy: ncy,
                            x: nx,
                            y: ny,
                        }
                    }
                    crate::PathCommand::CubicTo {
                        cx1,
                        cy1,
                        cx2,
                        cy2,
                        x,
                        y,
                    } => {
                        let (ncx1, ncy1) = tx_pt(cx1, cy1);
                        let (ncx2, ncy2) = tx_pt(cx2, cy2);
                        let (nx, ny) = tx_pt(x, y);
                        crate::PathCommand::CubicTo {
                            cx1: ncx1,
                            cy1: ncy1,
                            cx2: ncx2,
                            cy2: ncy2,
                            x: nx,
                            y: ny,
                        }
                    }
                    crate::PathCommand::Close => crate::PathCommand::Close,
                };
                combined.push(mapped);
            }
        }

        if (flags & MORE_COMPONENTS) == 0 {
            break;
        }
    }

    if combined.is_empty() {
        None
    } else {
        Some(combined)
    }
}

#[cfg(test)]
mod fonts_tests {
    use super::*;

    fn build_synthetic_sfnt_with_tables(extra_tags: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
        // Simple glyph at GID 1 ('A' = 65): 1 contour with 3 on-curve points (triangle)
        let mut g1 = Vec::new();
        g1.extend_from_slice(&1i16.to_be_bytes()); // numContours = 1
        g1.extend_from_slice(&0i16.to_be_bytes()); // xMin
        g1.extend_from_slice(&0i16.to_be_bytes()); // yMin
        g1.extend_from_slice(&500i16.to_be_bytes()); // xMax
        g1.extend_from_slice(&700i16.to_be_bytes()); // yMax
        g1.extend_from_slice(&2u16.to_be_bytes()); // endPtsOfContours[0] = 2 (3 points)
        g1.extend_from_slice(&0u16.to_be_bytes()); // instructionLength = 0
        // flags: 3 points, all on-curve (0x01) + x-short positive (0x02 | 0x10 = 0x12) + y-short positive (0x04 | 0x20 = 0x24) => 0x37
        g1.extend_from_slice(&[0x37, 0x37, 0x37]);
        // x coordinates (deltas): 10, 200, 100
        g1.extend_from_slice(&[10, 200, 100]);
        // y coordinates (deltas): 0, 250, 100
        g1.extend_from_slice(&[0, 250, 100]);
        if g1.len() % 2 != 0 {
            g1.push(0);
        }

        // Composite glyph at GID 2 ('Á' = 193): references GID 1 with translation (+40, +80)
        let mut g2 = Vec::new();
        g2.extend_from_slice(&(-1i16).to_be_bytes()); // numContours = -1 (composite)
        g2.extend_from_slice(&0i16.to_be_bytes());
        g2.extend_from_slice(&0i16.to_be_bytes());
        g2.extend_from_slice(&600i16.to_be_bytes());
        g2.extend_from_slice(&800i16.to_be_bytes());
        let flags: u16 = 0x0002; // ARGS_ARE_XY_VALUES (byte args)
        g2.extend_from_slice(&flags.to_be_bytes());
        g2.extend_from_slice(&1u16.to_be_bytes()); // component glyphIndex = 1
        g2.push(40u8); // dx = 40
        g2.push(80u8); // dy = 80
        if g2.len() % 2 != 0 {
            g2.push(0);
        }

        // GID 0 (.notdef) empty, GID 1 = g1, GID 2 = g2
        let mut glyf = Vec::new();
        let off0 = 0u32;
        let off1 = 0u32;
        glyf.extend_from_slice(&g1);
        let off2 = glyf.len() as u32;
        glyf.extend_from_slice(&g2);
        let off3 = glyf.len() as u32;

        // long loca (indexToLocFormat = 1)
        let mut loca = Vec::new();
        for off in [off0, off1, off2, off3] {
            loca.extend_from_slice(&off.to_be_bytes());
        }

        // head table (54 bytes)
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm = 1000
        head[50..52].copy_from_slice(&1i16.to_be_bytes()); // indexToLocFormat = 1

        // hhea table (36 bytes)
        let mut hhea = vec![0u8; 36];
        hhea[4..6].copy_from_slice(&800i16.to_be_bytes());
        hhea[6..8].copy_from_slice(&(-200i16).to_be_bytes());
        hhea[8..10].copy_from_slice(&200i16.to_be_bytes());
        hhea[34..36].copy_from_slice(&3u16.to_be_bytes()); // numOfLongHorMetrics = 3

        // maxp table (6 bytes)
        let mut maxp = vec![0u8; 6];
        maxp[4..6].copy_from_slice(&3u16.to_be_bytes()); // numGlyphs = 3

        // hmtx table (3 * 4 bytes)
        let mut hmtx = Vec::new();
        for adv in [500u16, 600u16, 620u16] {
            hmtx.extend_from_slice(&adv.to_be_bytes());
            hmtx.extend_from_slice(&0i16.to_be_bytes());
        }

        // cmap table with format 12 mapping 'A' (65) -> 1 and 'Á' (193) -> 2
        let mut cmap = Vec::new();
        cmap.extend_from_slice(&0u16.to_be_bytes()); // version
        cmap.extend_from_slice(&1u16.to_be_bytes()); // numTables = 1
        cmap.extend_from_slice(&3u16.to_be_bytes()); // platformID
        cmap.extend_from_slice(&10u16.to_be_bytes()); // encodingID
        cmap.extend_from_slice(&12u32.to_be_bytes()); // subtable offset = 12
        // format 12 header
        cmap.extend_from_slice(&12u16.to_be_bytes()); // format = 12
        cmap.extend_from_slice(&0u16.to_be_bytes()); // reserved
        cmap.extend_from_slice(&40u32.to_be_bytes()); // length
        cmap.extend_from_slice(&0u32.to_be_bytes()); // language
        cmap.extend_from_slice(&2u32.to_be_bytes()); // numGroups = 2
        // group 0: 65..=65 -> 1
        cmap.extend_from_slice(&65u32.to_be_bytes());
        cmap.extend_from_slice(&65u32.to_be_bytes());
        cmap.extend_from_slice(&1u32.to_be_bytes());
        // group 1: 193..=193 -> 2
        cmap.extend_from_slice(&193u32.to_be_bytes());
        cmap.extend_from_slice(&193u32.to_be_bytes());
        cmap.extend_from_slice(&2u32.to_be_bytes());

        let mut all_tables: Vec<([u8; 4], Vec<u8>)> = vec![
            (*b"cmap", cmap),
            (*b"glyf", glyf),
            (*b"head", head),
            (*b"hhea", hhea),
            (*b"hmtx", hmtx),
            (*b"loca", loca),
            (*b"maxp", maxp),
        ];
        for (tag, data) in extra_tags {
            all_tables.push((*tag, data.clone()));
        }

        let num_tables = all_tables.len() as u16;
        let mut sfnt = Vec::new();
        sfnt.extend_from_slice(&[0, 1, 0, 0]);
        sfnt.extend_from_slice(&num_tables.to_be_bytes());
        sfnt.extend_from_slice(&0u16.to_be_bytes());
        sfnt.extend_from_slice(&0u16.to_be_bytes());
        sfnt.extend_from_slice(&0u16.to_be_bytes());

        let dir_end = 12 + (num_tables as usize) * 16;
        let mut offset = dir_end;
        for (tag, data) in &all_tables {
            sfnt.extend_from_slice(tag);
            sfnt.extend_from_slice(&0u32.to_be_bytes()); // checksum
            sfnt.extend_from_slice(&(offset as u32).to_be_bytes());
            sfnt.extend_from_slice(&(data.len() as u32).to_be_bytes());
            offset += data.len();
        }
        for (_, data) in &all_tables {
            sfnt.extend_from_slice(data);
        }
        sfnt
    }

    #[test]
    fn composite_glyf_outline_and_typography_helpers_roundtrip() {
        let sfnt = build_synthetic_sfnt_with_tables(&[]);
        let metrics = PortableFontMetrics::from_bytes("SyntheticSans", &sfnt).unwrap();
        assert!(metrics.has_outline_for_char('A'));
        assert!(metrics.has_outline_for_char('Á'));
        assert!(!metrics.has_outline_for_char('Ω'));

        let simple = metrics.try_glyph_outline('A', 0.0, 0.0, 100.0).unwrap();
        let composite = metrics.try_glyph_outline('Á', 0.0, 0.0, 100.0).unwrap();
        assert!(!simple.is_empty());
        assert_eq!(simple.len(), composite.len());
        // Composite glyph translates X by +40 design units (+4.0 px at size 100 / 1000 UPM)
        match (&simple[0], &composite[0]) {
            (
                crate::PathCommand::MoveTo { x: x1, y: y1 },
                crate::PathCommand::MoveTo { x: x2, y: y2 },
            ) => {
                assert!((x2 - x1 - 4.0).abs() < 1e-6);
                assert!((y1 - y2 - 8.0).abs() < 1e-6);
            }
            _ => panic!("expected MoveTo commands"),
        }

        let mut field = TextFieldState::default();
        assert_eq!(field.auto_size_mode(), TextAutoSize::None);
        field.set_auto_size_mode(TextAutoSize::Center);
        assert_eq!(field.as2_viewport.auto_size, "center");
        assert!(!field.word_wrap);
        assert_eq!(field.auto_size_mode(), TextAutoSize::Center);

        let mut fmt = TextFormat::default();
        fmt.set_figure_style("oldstyle", "tabular");
        assert_eq!(
            fmt.figure_style(),
            ("oldstyle".to_string(), "tabular".to_string())
        );
        fmt.set_case_transform("small_caps", "tr_TR");
        assert_eq!(
            fmt.case_transform(),
            ("small_caps".to_string(), "tr_TR".to_string())
        );
        fmt.set_vertical_rotation("270");
        assert_eq!(fmt.vertical_rotation(), "270");
    }

    #[test]
    fn outline_diagnostics_report_bitmap_color_tables_and_missing_glyphs() {
        let color_sfnt =
            build_synthetic_sfnt_with_tables(&[(*b"COLR", vec![0; 16]), (*b"CBDT", vec![0; 16])]);
        let unsupported = TextFormat::unsupported_outline_font_tables(&color_sfnt);
        assert!(unsupported.contains(&"COLR".to_string()));
        assert!(unsupported.contains(&"CBDT".to_string()));

        let diags = TextFormat::validate_text_outlines_for_font(
            "root/layer[0]/frame[0]/element[1]",
            "EmojiColor",
            "AΩ",
            &color_sfnt,
        );
        assert_eq!(diags.len(), 2);
        assert!(diags[0].contains("root/layer[0]/frame[0]/element[1]"));
        assert!(diags[0].contains("COLR"));
        assert!(diags[0].contains("CBDT"));
        assert!(diags[1].contains("U+03A9"));
    }
}
