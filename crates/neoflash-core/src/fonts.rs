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

    /// Inspects SFNT (`.ttf`/`.otf`) or WOFF1 (`wOFF`) table tags for the first face (`face_index = 0`).
    pub fn sfnt_table_tags(font_bytes: &[u8]) -> Vec<String> {
        Self::sfnt_table_tags_for_face(font_bytes, 0)
    }

    /// Inspects SFNT (`.ttf`/`.otf`), TTC (`ttcf`), or WOFF1 (`wOFF`) table tags for `face_index`.
    pub fn sfnt_table_tags_for_face(font_bytes: &[u8], face_index: u32) -> Vec<String> {
        if font_bytes.len() < 12 {
            return Vec::new();
        }
        let is_woff = &font_bytes[0..4] == b"wOFF";
        let (dir_off, num_tables) = if is_woff {
            if face_index != 0 || font_bytes.len() < 44 {
                return Vec::new();
            }
            (
                0usize,
                u16::from_be_bytes([font_bytes[12], font_bytes[13]]) as usize,
            )
        } else {
            let Some(off) = sfnt_dir_offset(font_bytes, face_index) else {
                return Vec::new();
            };
            (
                off,
                u16::from_be_bytes([font_bytes[off + 4], font_bytes[off + 5]]) as usize,
            )
        };
        let mut out = Vec::new();
        for i in 0..num_tables.min(128) {
            let entry = if is_woff {
                44 + i * 20
            } else {
                dir_off + 12 + i * 16
            };
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
        Self::unsupported_outline_font_tables_for_face(font_bytes, 0)
    }

    /// Face-indexed variant of [`Self::unsupported_outline_font_tables`] for `.ttc` collections.
    pub fn unsupported_outline_font_tables_for_face(
        font_bytes: &[u8],
        face_index: u32,
    ) -> Vec<String> {
        const UNSUPPORTED_TABLES: &[&str] = &[
            "CBDT", "CBLC", "sbix", "COLR", "CPAL", "EBDT", "EBLC", "SVG ",
        ];
        let tags = Self::sfnt_table_tags_for_face(font_bytes, face_index);
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
        Self::has_cff_or_glyf_outlines_for_face(font_bytes, 0)
    }

    /// Face-indexed variant of [`Self::has_cff_or_glyf_outlines`] for `.ttc` collections.
    pub fn has_cff_or_glyf_outlines_for_face(font_bytes: &[u8], face_index: u32) -> (bool, bool) {
        let tags = Self::sfnt_table_tags_for_face(font_bytes, face_index);
        let has_glyf = tags.iter().any(|t| t == "glyf");
        let has_cff = tags.iter().any(|t| t == "CFF " || t == "CFF2");
        (has_glyf, has_cff)
    }

    /// Returns non-whitespace Unicode scalar values in `text` that are not mapped in `cmap`
    /// of `font_bytes`.
    pub fn missing_outline_codepoints(font_bytes: &[u8], text: &str) -> Vec<u32> {
        Self::missing_outline_codepoints_for_face(font_bytes, 0, text)
    }

    /// Face-indexed variant of [`Self::missing_outline_codepoints`] for `.ttc` collections,
    /// using untruncated binary-search `cmap` lookup across BMP and supplementary planes.
    pub fn missing_outline_codepoints_for_face(
        font_bytes: &[u8],
        face_index: u32,
        text: &str,
    ) -> Vec<u32> {
        let Some(sfnt) = sfnt_view_bytes(font_bytes) else {
            return Vec::new();
        };
        let Some(dir_off) = sfnt_dir_offset(&sfnt, face_index) else {
            return Vec::new();
        };
        if sfnt_table_slice_at(&sfnt, dir_off, b"cmap").is_none() {
            return Vec::new();
        }
        let mut missing = Vec::new();
        for ch in text.chars() {
            if ch.is_whitespace() {
                continue;
            }
            let cp = ch as u32;
            let gid = sfnt_cmap_glyph_id_at(&sfnt, dir_off, cp).unwrap_or(0);
            if gid == 0 && !missing.contains(&cp) {
                missing.push(cp);
            }
        }
        missing
    }

    /// Validates whether `font_bytes` (face 0) can faithfully convert `text` to vector outlines at `location`.
    /// Emits actionable located diagnostics when bitmap/color tables (`CBDT`, `sbix`, `COLR`) are used
    /// or when `text` references glyphs missing from the font's coverage.
    pub fn validate_text_outlines_for_font(
        location: &str,
        family: &str,
        text: &str,
        font_bytes: &[u8],
    ) -> Vec<String> {
        Self::validate_text_outlines_for_font_face(location, family, text, font_bytes, 0)
    }

    /// Validates whether `face_index` of `font_bytes` (standalone SFNT, `.ttc` collection, or WOFF1)
    /// can faithfully convert `text` to vector outlines at `location`.
    pub fn validate_text_outlines_for_font_face(
        location: &str,
        family: &str,
        text: &str,
        font_bytes: &[u8],
        face_index: u32,
    ) -> Vec<String> {
        let mut diagnostics = Vec::new();
        let unsupported = Self::unsupported_outline_font_tables_for_face(font_bytes, face_index);
        let (has_glyf, has_cff) = Self::has_cff_or_glyf_outlines_for_face(font_bytes, face_index);
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

        let missing = Self::missing_outline_codepoints_for_face(font_bytes, face_index, text);
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

#[derive(Clone, Debug, Default, Deserialize)]
struct CoreEmbeddedFontMetadata {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    family: String,
    #[serde(default)]
    style: String,
    #[serde(default)]
    export_for_actionscript: bool,
    #[serde(default)]
    class_name: String,
}

fn core_requested_style(bold: bool, italic: bool) -> &'static str {
    match (bold, italic) {
        (true, true) => "Bold Italic",
        (true, false) => "Bold",
        (false, true) => "Italic",
        (false, false) => "Regular",
    }
}

/// Returns true when `resource` represents a font binary rather than metadata or another asset.
pub fn is_font_resource_unit(resource: &crate::ResourceUnit) -> bool {
    let mime = resource.media_type.trim().to_ascii_lowercase();
    if matches!(
        mime.as_str(),
        "font/ttf"
            | "font/otf"
            | "font/collection"
            | "font/woff"
            | "font/woff2"
            | "application/font-sfnt"
            | "application/font-collection"
            | "application/x-font-ttf"
            | "application/x-font-ttc"
            | "application/x-font-opentype"
            | "application/vnd.ms-opentype"
            | "application/font-woff"
    ) {
        return true;
    }
    let path_lower = resource.path.to_ascii_lowercase();
    path_lower.ends_with(".ttf")
        || path_lower.ends_with(".otf")
        || path_lower.ends_with(".ttc")
        || path_lower.ends_with(".otc")
        || path_lower.ends_with(".woff")
        || path_lower.ends_with(".woff2")
}

/// Derives the fallback CSS family name from a font resource's path stem or ID.
pub fn font_family_for_resource_unit(resource: &crate::ResourceUnit) -> String {
    let stem = std::path::Path::new(&resource.path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&resource.id)
        .trim();
    if stem.is_empty() {
        resource.id.clone()
    } else {
        stem.to_string()
    }
}

/// Resolves the decoded font bytes from `project.resources` matching `(family, bold, italic)`.
pub fn resolve_project_font_bytes(
    project: &crate::Project,
    family: &str,
    bold: bool,
    italic: bool,
) -> Option<Vec<u8>> {
    let metadata: BTreeMap<String, CoreEmbeddedFontMetadata> = project
        .resources
        .iter()
        .find(|resource| resource.path.eq_ignore_ascii_case("fonts/embedding.json"))
        .and_then(|resource| resource.decoded_bytes().ok())
        .and_then(|bytes| serde_json::from_slice::<Vec<CoreEmbeddedFontMetadata>>(&bytes).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| !entry.id.is_empty())
        .map(|entry| (entry.id.clone(), entry))
        .collect();
    let target_style = core_requested_style(bold, italic);
    let mut fallback: Option<Vec<u8>> = None;
    for res in &project.resources {
        if !is_font_resource_unit(res) {
            continue;
        }
        let Ok(bytes) = res.decoded_bytes() else {
            continue;
        };
        let entry = metadata.get(&res.id);
        let res_family = entry
            .map(|e| e.family.trim())
            .filter(|f| !f.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| font_family_for_resource_unit(res));
        let res_style = entry
            .map(|e| e.style.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("Regular");
        if res_family.eq_ignore_ascii_case(family.trim())
            && res_style.eq_ignore_ascii_case(target_style)
        {
            return Some(bytes);
        }
        if fallback.is_none()
            && (res_family.eq_ignore_ascii_case(family.trim())
                || res.id.eq_ignore_ascii_case(family.trim()))
        {
            fallback = Some(bytes);
        }
    }
    fallback
}

/// Computes OpenType big-endian 32-bit table checksum over `bytes` (zero-padded to 4 bytes).
pub fn sfnt_checksum(bytes: &[u8]) -> u32 {
    let mut sum = 0u32;
    let (chunks, rem) = bytes.as_chunks::<4>();
    for chunk in chunks {
        sum = sum.wrapping_add(u32::from_be_bytes(*chunk));
    }
    if !rem.is_empty() {
        let mut tail = [0u8; 4];
        tail[..rem.len()].copy_from_slice(rem);
        sum = sum.wrapping_add(u32::from_be_bytes(tail));
    }
    sum
}

/// Sniffs the canonical MIME type for SFNT/WOFF/WOFF2 font bytes.
pub fn sniff_font_media_type<'a>(bytes: &[u8], fallback: &'a str) -> &'a str {
    if bytes.len() >= 4 {
        match &bytes[0..4] {
            b"wOFF" => return "font/woff",
            b"wOF2" => return "font/woff2",
            b"OTTO" => return "font/otf",
            [0, 1, 0, 0] | b"true" | b"typ1" => return "font/ttf",
            _ => {}
        }
    }
    if fallback.trim().is_empty() {
        "font/ttf"
    } else {
        fallback.trim()
    }
}

/// Decompresses a WOFF1 (`wOFF`) container into a valid 4-byte-aligned SFNT binary.
pub fn unpack_woff_to_sfnt(bytes: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read as _;

    const MAX_SFNT_BYTES: usize = 64 * 1024 * 1024;
    if bytes.len() < 44 || &bytes[0..4] != b"wOFF" {
        return None;
    }
    let flavor = [bytes[4], bytes[5], bytes[6], bytes[7]];
    if flavor != [0, 1, 0, 0] && &flavor != b"OTTO" && &flavor != b"true" && &flavor != b"typ1" {
        return None;
    }
    let num_tables = u16::from_be_bytes([bytes[12], bytes[13]]) as usize;
    let total_sfnt_size = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as usize;
    if num_tables == 0 || num_tables > 256 || total_sfnt_size > MAX_SFNT_BYTES {
        return None;
    }
    let dir_end = 44usize.checked_add(num_tables.checked_mul(20)?)?;
    if dir_end > bytes.len() {
        return None;
    }

    let mut tables = Vec::<([u8; 4], Vec<u8>)>::with_capacity(num_tables);
    let mut sum_raw_padded = 12usize.checked_add(num_tables * 16)?;
    for i in 0..num_tables {
        let entry = 44 + i * 20;
        let tag = [
            bytes[entry],
            bytes[entry + 1],
            bytes[entry + 2],
            bytes[entry + 3],
        ];
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
        let end = offset.checked_add(comp_len)?;
        if end > bytes.len() || comp_len > orig_len || orig_len > MAX_SFNT_BYTES {
            return None;
        }
        sum_raw_padded = sum_raw_padded.checked_add((orig_len + 3) & !3)?;
        if sum_raw_padded > MAX_SFNT_BYTES {
            return None;
        }
        let raw_table = if comp_len == orig_len {
            bytes[offset..end].to_vec()
        } else {
            let mut decoded = Vec::with_capacity(orig_len);
            let decoder = flate2::read::ZlibDecoder::new(&bytes[offset..end]);
            decoder
                .take((orig_len as u64).saturating_add(1))
                .read_to_end(&mut decoded)
                .ok()?;
            if decoded.len() != orig_len {
                return None;
            }
            decoded
        };
        tables.push((tag, raw_table));
    }
    tables.sort_by_key(|(tag, _)| *tag);

    let table_count = tables.len() as u16;
    let entry_selector = 15u16.saturating_sub(table_count.leading_zeros() as u16);
    let search_range = (1u16 << entry_selector) * 16;
    let range_shift = table_count * 16 - search_range;

    let mut out = Vec::with_capacity(sum_raw_padded);
    out.extend_from_slice(&flavor);
    out.extend_from_slice(&table_count.to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes());

    let dir_pos = out.len();
    out.resize(dir_pos + tables.len() * 16, 0);
    let mut head_offset = None;
    for (idx, (tag, data)) in tables.iter().enumerate() {
        let mut table_bytes = data.clone();
        if tag == b"head" && table_bytes.len() >= 12 {
            table_bytes[8..12].copy_from_slice(&[0, 0, 0, 0]);
            head_offset = Some(out.len());
        }
        let offset = out.len() as u32;
        let length = table_bytes.len() as u32;
        let checksum = sfnt_checksum(&table_bytes);
        let rec_pos = dir_pos + idx * 16;
        out[rec_pos..rec_pos + 4].copy_from_slice(tag);
        out[rec_pos + 4..rec_pos + 8].copy_from_slice(&checksum.to_be_bytes());
        out[rec_pos + 8..rec_pos + 12].copy_from_slice(&offset.to_be_bytes());
        out[rec_pos + 12..rec_pos + 16].copy_from_slice(&length.to_be_bytes());
        out.extend_from_slice(&table_bytes);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }
    if let Some(head_off) = head_offset {
        let adjustment = 0xB1B0_AFBAu32.wrapping_sub(sfnt_checksum(&out));
        out[head_off + 8..head_off + 12].copy_from_slice(&adjustment.to_be_bytes());
    }
    Some(out)
}

/// Packs a valid SFNT font into a zlib-compressed WOFF1 (`wOFF`) container.
pub fn pack_sfnt_to_woff(sfnt: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write as _;

    if sfnt.len() < 12 {
        return None;
    }
    let flavor = [sfnt[0], sfnt[1], sfnt[2], sfnt[3]];
    if flavor != [0, 1, 0, 0] && &flavor != b"OTTO" && &flavor != b"true" && &flavor != b"typ1" {
        return None;
    }
    let num_tables = u16::from_be_bytes([sfnt[4], sfnt[5]]) as usize;
    let dir_end = 12usize.checked_add(num_tables.checked_mul(16)?)?;
    if num_tables == 0 || dir_end > sfnt.len() {
        return None;
    }

    struct EncodedTable {
        tag: [u8; 4],
        payload: Vec<u8>,
        orig_len: u32,
        orig_checksum: u32,
    }
    let mut encoded = Vec::<EncodedTable>::with_capacity(num_tables);
    let mut total_sfnt_size = dir_end;
    for i in 0..num_tables {
        let rec = &sfnt[12 + i * 16..12 + (i + 1) * 16];
        let tag = [rec[0], rec[1], rec[2], rec[3]];
        let orig_checksum = u32::from_be_bytes([rec[4], rec[5], rec[6], rec[7]]);
        let offset = u32::from_be_bytes([rec[8], rec[9], rec[10], rec[11]]) as usize;
        let orig_len = u32::from_be_bytes([rec[12], rec[13], rec[14], rec[15]]) as usize;
        let end = offset.checked_add(orig_len)?;
        if end > sfnt.len() {
            return None;
        }
        total_sfnt_size = total_sfnt_size.checked_add((orig_len + 3) & !3)?;
        let raw = &sfnt[offset..end];
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        let compressed = encoder
            .write_all(raw)
            .ok()
            .and_then(|()| encoder.finish().ok())
            .filter(|c| c.len() < raw.len());
        encoded.push(EncodedTable {
            tag,
            payload: compressed.unwrap_or_else(|| raw.to_vec()),
            orig_len: orig_len as u32,
            orig_checksum,
        });
    }
    encoded.sort_by_key(|t| t.tag);

    let data_start = 44usize.checked_add(encoded.len() * 20)?;
    let mut out = vec![0u8; data_start];
    for (idx, table) in encoded.iter().enumerate() {
        let offset = out.len() as u32;
        let comp_len = table.payload.len() as u32;
        let dir = 44 + idx * 20;
        out[dir..dir + 4].copy_from_slice(&table.tag);
        out[dir + 4..dir + 8].copy_from_slice(&offset.to_be_bytes());
        out[dir + 8..dir + 12].copy_from_slice(&comp_len.to_be_bytes());
        out[dir + 12..dir + 16].copy_from_slice(&table.orig_len.to_be_bytes());
        out[dir + 16..dir + 20].copy_from_slice(&table.orig_checksum.to_be_bytes());
        out.extend_from_slice(&table.payload);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }
    let total_len = out.len() as u32;
    out[0..4].copy_from_slice(b"wOFF");
    out[4..8].copy_from_slice(&flavor);
    out[8..12].copy_from_slice(&total_len.to_be_bytes());
    out[12..14].copy_from_slice(&(encoded.len() as u16).to_be_bytes());
    out[14..16].copy_from_slice(&0u16.to_be_bytes());
    out[16..20].copy_from_slice(&(total_sfnt_size as u32).to_be_bytes());
    out[20..22].copy_from_slice(&1u16.to_be_bytes());
    out[22..24].copy_from_slice(&0u16.to_be_bytes());
    Some(out)
}

fn is_sfnt_flavor(sig: &[u8]) -> bool {
    sig == [0, 1, 0, 0] || sig == b"OTTO" || sig == b"true" || sig == b"typ1"
}

/// Returns the SFNT table-directory byte offsets inside `sfnt`: `[0]` for standalone SFNT,
/// or each valid face's `tableDirectoryOffset` (from `12 + i * 4`, relative to `&sfnt[0..]`)
/// for an OpenType/TrueType Collection (`ttcf`).
pub(crate) fn sfnt_face_dir_offsets(sfnt: &[u8]) -> Option<Vec<usize>> {
    if sfnt.len() < 12 {
        return None;
    }
    let sig = &sfnt[0..4];
    if is_sfnt_flavor(sig) {
        return Some(vec![0]);
    }
    if sig == b"ttcf" {
        if sfnt.len() < 16 {
            return None;
        }
        let num_fonts = u32::from_be_bytes([sfnt[8], sfnt[9], sfnt[10], sfnt[11]]) as usize;
        if num_fonts == 0 {
            return None;
        }
        let count = num_fonts.min(64);
        if 12usize.checked_add(count * 4)? > sfnt.len() {
            return None;
        }
        let mut offsets = Vec::with_capacity(count);
        for i in 0..count {
            let pos = 12 + i * 4;
            let dir_off =
                u32::from_be_bytes([sfnt[pos], sfnt[pos + 1], sfnt[pos + 2], sfnt[pos + 3]])
                    as usize;
            if dir_off.checked_add(12).is_some_and(|end| end <= sfnt.len())
                && is_sfnt_flavor(&sfnt[dir_off..dir_off + 4])
            {
                offsets.push(dir_off);
            }
        }
        return (!offsets.is_empty()).then_some(offsets);
    }
    None
}

pub(crate) fn sfnt_dir_offset(sfnt: &[u8], face_index: u32) -> Option<usize> {
    if sfnt.len() < 12 {
        return None;
    }
    let sig = &sfnt[0..4];
    if is_sfnt_flavor(sig) {
        return (face_index == 0).then_some(0);
    }
    if sig == b"ttcf" {
        if sfnt.len() < 16 {
            return None;
        }
        let num_fonts = u32::from_be_bytes([sfnt[8], sfnt[9], sfnt[10], sfnt[11]]);
        if face_index >= num_fonts {
            return None;
        }
        let pos = 12usize.checked_add((face_index as usize).checked_mul(4)?)?;
        if pos.checked_add(4)? > sfnt.len() {
            return None;
        }
        let dir_off =
            u32::from_be_bytes([sfnt[pos], sfnt[pos + 1], sfnt[pos + 2], sfnt[pos + 3]]) as usize;
        if dir_off.checked_add(12).is_some_and(|end| end <= sfnt.len())
            && is_sfnt_flavor(&sfnt[dir_off..dir_off + 4])
        {
            return Some(dir_off);
        }
    }
    None
}

pub(crate) fn sfnt_first_dir_offset(sfnt: &[u8]) -> Option<usize> {
    sfnt_face_dir_offsets(sfnt)?.into_iter().next()
}

/// Extracts `face_index` from an OpenType/TrueType Collection (`ttcf`) or standalone SFNT/WOFF1
/// into a standalone 0-indexed SFNT byte buffer so single-face consumers can parse any `.ttc` face.
pub fn extract_ttc_face_to_sfnt(font_bytes: &[u8], face_index: u32) -> Option<Vec<u8>> {
    let sfnt = sfnt_view_bytes(font_bytes)?;
    if sfnt.len() < 12 {
        return None;
    }
    if is_sfnt_flavor(&sfnt[0..4]) {
        return (face_index == 0).then(|| sfnt.into_owned());
    }
    let dir_off = sfnt_dir_offset(&sfnt, face_index)?;
    let num_tables = u16::from_be_bytes([sfnt[dir_off + 4], sfnt[dir_off + 5]]) as usize;
    if num_tables == 0 || num_tables > 256 {
        return None;
    }
    let dir_len = 12usize.checked_add(num_tables * 16)?;
    if dir_off.checked_add(dir_len)? > sfnt.len() {
        return None;
    }

    let mut out = Vec::new();
    out.extend_from_slice(&sfnt[dir_off..dir_off + dir_len]);
    for i in 0..num_tables {
        let src_rec = dir_off + 12 + i * 16;
        let tbl_off = u32::from_be_bytes([
            sfnt[src_rec + 8],
            sfnt[src_rec + 9],
            sfnt[src_rec + 10],
            sfnt[src_rec + 11],
        ]) as usize;
        let tbl_len = u32::from_be_bytes([
            sfnt[src_rec + 12],
            sfnt[src_rec + 13],
            sfnt[src_rec + 14],
            sfnt[src_rec + 15],
        ]) as usize;
        let tbl_end = tbl_off.checked_add(tbl_len)?;
        if tbl_end > sfnt.len() {
            return None;
        }
        let new_off = u32::try_from(out.len()).ok()?;
        let dst_rec = 12 + i * 16;
        out[dst_rec + 8..dst_rec + 12].copy_from_slice(&new_off.to_be_bytes());
        out.extend_from_slice(&sfnt[tbl_off..tbl_end]);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }
    Some(out)
}

/// Returns the number of faces in a `ttcf` collection (or `1` for a standalone SFNT/WOFF1).
pub fn sfnt_face_count(font_bytes: &[u8]) -> u32 {
    let Some(sfnt) = sfnt_view_bytes(font_bytes) else {
        return 0;
    };
    if sfnt.len() >= 16 && &sfnt[0..4] == b"ttcf" {
        u32::from_be_bytes([sfnt[8], sfnt[9], sfnt[10], sfnt[11]]).min(256)
    } else if sfnt.len() >= 12 && is_sfnt_flavor(&sfnt[0..4]) {
        1
    } else {
        0
    }
}

/// Selects the best face index inside `font_bytes` (especially `.ttc` collections) matching
/// `(family, bold, italic)`.
pub fn select_best_sfnt_face_index(
    font_bytes: &[u8],
    family: &str,
    bold: bool,
    italic: bool,
) -> u32 {
    let Some(sfnt) = sfnt_view_bytes(font_bytes) else {
        return 0;
    };
    let count = sfnt_face_count(&sfnt);
    if count <= 1 {
        return 0;
    }
    let wanted_style = match (bold, italic) {
        (true, true) => "boldItalic",
        (true, false) => "bold",
        (false, true) => "italic",
        (false, false) => "regular",
    };
    let clean_family = family.trim();
    let mut best: Option<((bool, bool, u32), u32)> = None;
    for idx in 0..count {
        let Some(dir_off) = sfnt_dir_offset(&sfnt, idx) else {
            continue;
        };
        let Some(desc) = inspect_sfnt_font_descriptor_at(&sfnt, dir_off, clean_family) else {
            continue;
        };
        let fam_mismatch =
            !clean_family.is_empty() && !desc.font_name.eq_ignore_ascii_case(clean_family);
        let style_mismatch = desc.font_style != wanted_style;
        let score = (fam_mismatch, style_mismatch, idx);
        if best.is_none_or(|(b_score, _)| score < b_score) {
            best = Some((score, idx));
        }
    }
    best.map_or(0, |(_, idx)| idx)
}

/// Returns an SFNT/TTC view over raw SFNT, OpenType/TrueType Collection (`ttcf`), or WOFF1 bytes.
pub fn sfnt_view_bytes(bytes: &[u8]) -> Option<std::borrow::Cow<'_, [u8]>> {
    if bytes.len() < 12 {
        return None;
    }
    if &bytes[0..4] == b"wOFF" {
        return unpack_woff_to_sfnt(bytes).map(std::borrow::Cow::Owned);
    }
    if sfnt_face_dir_offsets(bytes).is_some() {
        return Some(std::borrow::Cow::Borrowed(bytes));
    }
    None
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

fn sfnt_table_slice_at<'a>(
    sfnt: &'a [u8],
    dir_off: usize,
    wanted_tag: &[u8; 4],
) -> Option<&'a [u8]> {
    if dir_off.checked_add(12)? > sfnt.len() {
        return None;
    }
    let num_tables = u16::from_be_bytes([sfnt[dir_off + 4], sfnt[dir_off + 5]]) as usize;
    for i in 0..num_tables.min(256) {
        let entry = dir_off.checked_add(12 + i * 16)?;
        if entry + 16 > sfnt.len() {
            break;
        }
        let tag = &sfnt[entry..entry + 4];
        // Per OpenType TTC specification, table offsets are always measured from byte 0 of `sfnt`.
        let offset = u32::from_be_bytes([
            sfnt[entry + 8],
            sfnt[entry + 9],
            sfnt[entry + 10],
            sfnt[entry + 11],
        ]) as usize;
        let length = u32::from_be_bytes([
            sfnt[entry + 12],
            sfnt[entry + 13],
            sfnt[entry + 14],
            sfnt[entry + 15],
        ]) as usize;
        if tag == wanted_tag
            && offset
                .checked_add(length)
                .is_some_and(|end| end <= sfnt.len())
        {
            return Some(&sfnt[offset..offset + length]);
        }
    }
    None
}

fn cmap_subtable_glyph_id(cmap: &[u8], sub_off: usize, codepoint: u32) -> Option<u32> {
    if sub_off.checked_add(4)? > cmap.len() {
        return None;
    }
    let fmt = u16::from_be_bytes([cmap[sub_off], cmap[sub_off + 1]]);
    match fmt {
        // Format 12 (segmented coverage) & Format 13 (many-to-one range mappings): BMP + SMP 32-bit
        12 | 13 => {
            if sub_off.checked_add(16)? > cmap.len() {
                return None;
            }
            let n_groups = u32::from_be_bytes([
                cmap[sub_off + 12],
                cmap[sub_off + 13],
                cmap[sub_off + 14],
                cmap[sub_off + 15],
            ]) as usize;
            let groups_end = sub_off
                .checked_add(16)?
                .checked_add(n_groups.checked_mul(12)?)?;
            if groups_end > cmap.len() {
                return None;
            }
            let mut lo = 0usize;
            let mut hi = n_groups;
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                let grp = sub_off + 16 + mid * 12;
                let start_c =
                    u32::from_be_bytes([cmap[grp], cmap[grp + 1], cmap[grp + 2], cmap[grp + 3]]);
                let end_c = u32::from_be_bytes([
                    cmap[grp + 4],
                    cmap[grp + 5],
                    cmap[grp + 6],
                    cmap[grp + 7],
                ]);
                if codepoint < start_c {
                    hi = mid;
                } else if codepoint > end_c {
                    lo = mid + 1;
                } else {
                    let start_gid = u32::from_be_bytes([
                        cmap[grp + 8],
                        cmap[grp + 9],
                        cmap[grp + 10],
                        cmap[grp + 11],
                    ]);
                    let gid = if fmt == 12 {
                        start_gid.wrapping_add(codepoint - start_c)
                    } else {
                        start_gid
                    };
                    return Some(gid);
                }
            }
            Some(0)
        }
        // Format 4: Segment mapping to delta values (BMP U+0000..=U+FFFF)
        4 => {
            if codepoint > 0xFFFF {
                return Some(0);
            }
            if sub_off.checked_add(14)? > cmap.len() {
                return None;
            }
            let seg_count_x2 = u16::from_be_bytes([cmap[sub_off + 6], cmap[sub_off + 7]]) as usize;
            if seg_count_x2 < 2 || !seg_count_x2.is_multiple_of(2) {
                return None;
            }
            let seg_count = seg_count_x2 / 2;
            let end_codes = sub_off + 14;
            let start_codes = end_codes.checked_add(seg_count_x2)?.checked_add(2)?;
            let id_deltas = start_codes.checked_add(seg_count_x2)?;
            let id_range_offsets = id_deltas.checked_add(seg_count_x2)?;
            if id_range_offsets.checked_add(seg_count_x2)? > cmap.len() {
                return None;
            }
            let cp16 = codepoint as u16;
            let mut lo = 0usize;
            let mut hi = seg_count;
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                let end_c =
                    u16::from_be_bytes([cmap[end_codes + mid * 2], cmap[end_codes + mid * 2 + 1]]);
                if cp16 > end_c {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            if lo >= seg_count {
                return Some(0);
            }
            let end_c =
                u16::from_be_bytes([cmap[end_codes + lo * 2], cmap[end_codes + lo * 2 + 1]]);
            let start_c =
                u16::from_be_bytes([cmap[start_codes + lo * 2], cmap[start_codes + lo * 2 + 1]]);
            if cp16 < start_c || cp16 > end_c || (start_c == 0xFFFF && cp16 == 0xFFFF) {
                return Some(0);
            }
            let delta =
                i16::from_be_bytes([cmap[id_deltas + lo * 2], cmap[id_deltas + lo * 2 + 1]]) as i32;
            let ro_pos = id_range_offsets + lo * 2;
            let ro = u16::from_be_bytes([cmap[ro_pos], cmap[ro_pos + 1]]) as usize;
            let gid = if ro == 0 {
                ((i32::from(cp16) + delta) & 0xFFFF) as u32
            } else {
                let glyph_idx_off = ro_pos
                    .checked_add(ro)?
                    .checked_add(usize::from(cp16 - start_c) * 2)?;
                if glyph_idx_off + 2 > cmap.len() {
                    return Some(0);
                }
                let raw = u16::from_be_bytes([cmap[glyph_idx_off], cmap[glyph_idx_off + 1]]);
                if raw == 0 {
                    0
                } else {
                    ((i32::from(raw) + delta) & 0xFFFF) as u32
                }
            };
            Some(gid)
        }
        // Format 6: Trimmed table mapping (BMP contiguous range)
        6 => {
            if codepoint > 0xFFFF || sub_off.checked_add(10)? > cmap.len() {
                return Some(0);
            }
            let first_code = u16::from_be_bytes([cmap[sub_off + 6], cmap[sub_off + 7]]);
            let entry_count = u16::from_be_bytes([cmap[sub_off + 8], cmap[sub_off + 9]]);
            let cp16 = codepoint as u16;
            if cp16 < first_code || cp16.saturating_sub(first_code) >= entry_count {
                return Some(0);
            }
            let idx = usize::from(cp16 - first_code);
            let pos = sub_off + 10 + idx * 2;
            if pos + 2 > cmap.len() {
                return Some(0);
            }
            Some(u32::from(u16::from_be_bytes([cmap[pos], cmap[pos + 1]])))
        }
        // Format 0: Byte encoding table
        0 => {
            if codepoint > 0xFF || sub_off.checked_add(6 + 256)? > cmap.len() {
                return Some(0);
            }
            Some(u32::from(cmap[sub_off + 6 + (codepoint as usize)]))
        }
        _ => None,
    }
}

fn sfnt_cmap_glyph_id_at(sfnt: &[u8], dir_off: usize, codepoint: u32) -> Option<u32> {
    let cmap = sfnt_table_slice_at(sfnt, dir_off, b"cmap")?;
    if cmap.len() < 4 {
        return None;
    }
    let num_subtables = u16::from_be_bytes([cmap[2], cmap[3]]) as usize;
    if num_subtables == 0 {
        return None;
    }

    #[derive(Clone, Copy)]
    struct SubtableCand {
        rank: u8,
        sub_off: usize,
    }
    let mut cands = Vec::with_capacity(num_subtables.min(64));
    for i in 0..num_subtables.min(64) {
        let rec = 4 + i * 8;
        if rec + 8 > cmap.len() {
            break;
        }
        let platform_id = u16::from_be_bytes([cmap[rec], cmap[rec + 1]]);
        let encoding_id = u16::from_be_bytes([cmap[rec + 2], cmap[rec + 3]]);
        let sub_off =
            u32::from_be_bytes([cmap[rec + 4], cmap[rec + 5], cmap[rec + 6], cmap[rec + 7]])
                as usize;
        if sub_off + 2 > cmap.len() {
            continue;
        }
        let fmt = u16::from_be_bytes([cmap[sub_off], cmap[sub_off + 1]]);
        let rank = match (platform_id, encoding_id, fmt) {
            (3, 10, 12 | 13) | (0, 4 | 6, 12 | 13) => 0,
            (_, _, 12 | 13) => 1,
            (3, 1, 4) | (0, _, 4) => 2,
            (_, _, 4) => 3,
            (0 | 3, _, 6 | 0) => 4,
            (_, _, 6 | 0) => 5,
            _ => continue,
        };
        cands.push(SubtableCand { rank, sub_off });
    }
    if cands.is_empty() {
        return None;
    }
    cands.sort_by_key(|c| c.rank);

    let mut saw_valid_subtable = false;
    for cand in cands {
        if let Some(gid) = cmap_subtable_glyph_id(cmap, cand.sub_off, codepoint) {
            saw_valid_subtable = true;
            if gid > 0 {
                return Some(gid);
            }
        }
    }
    saw_valid_subtable.then_some(0)
}

/// Looks up the exact glyph index for `codepoint` in an SFNT/TTC/WOFF1 font's `cmap` table
/// without any glyph-count truncation (`Format 12`/`13` for BMP + supplementary planes, `Format 4`/`6`/`0` for BMP).
/// Returns `None` if `font_bytes` is not a valid SFNT/TTC/WOFF1 font containing a `cmap` table,
/// or `Some(glyph_id)` (`0` means `.notdef` / missing glyph).
pub fn sfnt_cmap_glyph_id(font_bytes: &[u8], codepoint: u32) -> Option<u32> {
    let sfnt = sfnt_view_bytes(font_bytes)?;
    let dir_off = sfnt_first_dir_offset(&sfnt)?;
    sfnt_cmap_glyph_id_at(&sfnt, dir_off, codepoint)
}

fn sfnt_has_glyphs_at(sfnt: &[u8], dir_off: usize, text: &str) -> Option<bool> {
    // Validate that this face actually contains a readable SFNT `cmap` table.
    let _ = sfnt_cmap_glyph_id_at(sfnt, dir_off, 0x0020)?;
    if text.is_empty() {
        return Some(true);
    }
    for ch in text.chars() {
        let cp = ch as u32;
        // C0 control characters (including \n, \r, \t, \0) and DEL are not renderable glyphs in Flash Font.hasGlyphs.
        if cp < 0x20 || cp == 0x7F {
            return Some(false);
        }
        if sfnt_cmap_glyph_id_at(sfnt, dir_off, cp).unwrap_or(0) == 0 {
            return Some(false);
        }
    }
    Some(true)
}

/// Evaluates `Font.hasGlyphs(str)` against a real SFNT/TTC/WOFF1 binary's `cmap` table
/// (Format 4 + Format 12/13, supporting BMP and supplementary plane scalars).
/// Returns `None` if `font_bytes` is not a valid SFNT/TTC/WOFF1 font with a `cmap` table.
pub fn sfnt_has_glyphs(font_bytes: &[u8], text: &str) -> Option<bool> {
    let sfnt = sfnt_view_bytes(font_bytes)?;
    let dir_off = sfnt_first_dir_offset(&sfnt)?;
    sfnt_has_glyphs_at(&sfnt, dir_off, text)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddedFontDescriptor {
    pub resource_id: String,
    pub class_name: String,
    pub font_name: String,
    pub font_style: String,
    pub font_type: String,
}

fn decode_sfnt_name_string(platform_id: u16, bytes: &[u8]) -> Option<String> {
    match platform_id {
        0 | 3 => {
            if !bytes.len().is_multiple_of(2) {
                return None;
            }
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_be_bytes)
                .collect();
            let s = String::from_utf16(&units).ok()?;
            let trimmed = s.trim_matches('\0').trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        1 => {
            let s: String = bytes
                .iter()
                .copied()
                .filter(|b| (0x20..0x7F).contains(b))
                .map(char::from)
                .collect();
            let trimmed = s.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        _ => None,
    }
}

fn normalize_as3_font_style(raw: &str, bold_flag: bool, italic_flag: bool) -> String {
    let lower = raw.trim().to_ascii_lowercase();
    let is_bold =
        bold_flag || lower.contains("bold") || lower.contains("black") || lower.contains("heavy");
    let is_italic = italic_flag || lower.contains("italic") || lower.contains("oblique");
    match (is_bold, is_italic) {
        (true, true) => "boldItalic".to_owned(),
        (true, false) => "bold".to_owned(),
        (false, true) => "italic".to_owned(),
        (false, false) => "regular".to_owned(),
    }
}

fn inspect_sfnt_font_descriptor_at(
    sfnt: &[u8],
    dir_off: usize,
    fallback_family: &str,
) -> Option<EmbeddedFontDescriptor> {
    // Require a valid cmap table so non-font dummy bytes do not masquerade as SFNT binaries.
    let _ = sfnt_cmap_glyph_id_at(sfnt, dir_off, 0x0020)?;

    let mut family_16: Option<(u8, String)> = None;
    let mut family_1: Option<(u8, String)> = None;
    let mut subfamily_2: Option<(u8, String)> = None;
    if let Some(name_tbl) = sfnt_table_slice_at(sfnt, dir_off, b"name")
        && name_tbl.len() >= 6
    {
        let count = u16::from_be_bytes([name_tbl[2], name_tbl[3]]) as usize;
        let storage_off = u16::from_be_bytes([name_tbl[4], name_tbl[5]]) as usize;
        for i in 0..count.min(256) {
            let rec = 6 + i * 12;
            if rec + 12 > name_tbl.len() {
                break;
            }
            let platform_id = u16::from_be_bytes([name_tbl[rec], name_tbl[rec + 1]]);
            let language_id = u16::from_be_bytes([name_tbl[rec + 4], name_tbl[rec + 5]]);
            let name_id = u16::from_be_bytes([name_tbl[rec + 6], name_tbl[rec + 7]]);
            let length = u16::from_be_bytes([name_tbl[rec + 8], name_tbl[rec + 9]]) as usize;
            let offset = u16::from_be_bytes([name_tbl[rec + 10], name_tbl[rec + 11]]) as usize;
            let str_start = storage_off.saturating_add(offset);
            let str_end = str_start.saturating_add(length);
            if str_end > name_tbl.len() {
                continue;
            }
            let prio = match (platform_id, language_id) {
                (3, 0x0409) => 0,
                (3, _) => 1,
                (0, _) => 2,
                (1, 0) => 3,
                (1, _) => 4,
                _ => continue,
            };
            let Some(decoded) = decode_sfnt_name_string(platform_id, &name_tbl[str_start..str_end])
            else {
                continue;
            };
            match name_id {
                16 if family_16.as_ref().is_none_or(|(p, _)| prio < *p) => {
                    family_16 = Some((prio, decoded));
                }
                1 if family_1.as_ref().is_none_or(|(p, _)| prio < *p) => {
                    family_1 = Some((prio, decoded));
                }
                2 if subfamily_2.as_ref().is_none_or(|(p, _)| prio < *p) => {
                    subfamily_2 = Some((prio, decoded));
                }
                _ => {}
            }
        }
    }

    let mut bold_flag = false;
    let mut italic_flag = false;
    if let Some(os2) = sfnt_table_slice_at(sfnt, dir_off, b"OS/2") {
        if os2.len() >= 6 {
            let weight_class = u16::from_be_bytes([os2[4], os2[5]]);
            if weight_class >= 600 {
                bold_flag = true;
            }
        }
        if os2.len() >= 64 {
            let fs_selection = u16::from_be_bytes([os2[62], os2[63]]);
            if fs_selection & 0x0001 != 0 {
                italic_flag = true;
            }
            if fs_selection & 0x0020 != 0 {
                bold_flag = true;
            }
        }
    } else if let Some(head) = sfnt_table_slice_at(sfnt, dir_off, b"head")
        && head.len() >= 46
    {
        let mac_style = u16::from_be_bytes([head[44], head[45]]);
        bold_flag |= mac_style & 0x0001 != 0;
        italic_flag |= mac_style & 0x0002 != 0;
    }

    let font_name = family_16
        .map(|(_, v)| v)
        .or_else(|| family_1.map(|(_, v)| v))
        .unwrap_or_else(|| fallback_family.trim().to_owned());
    let subfam = subfamily_2.map(|(_, v)| v).unwrap_or_default();
    let font_style = normalize_as3_font_style(&subfam, bold_flag, italic_flag);
    let has_cff = sfnt_table_slice_at(sfnt, dir_off, b"CFF ").is_some()
        || sfnt_table_slice_at(sfnt, dir_off, b"CFF2").is_some();
    let has_glyf = sfnt_table_slice_at(sfnt, dir_off, b"glyf").is_some();
    let font_type = if has_cff && !has_glyf {
        "embeddedCFF".to_owned()
    } else {
        "embedded".to_owned()
    };

    Some(EmbeddedFontDescriptor {
        resource_id: String::new(),
        class_name: String::new(),
        font_name,
        font_style,
        font_type,
    })
}

/// Extracts AS3 `Font` descriptor metadata (`fontName`, `fontStyle`, `fontType`) from SFNT/TTC/WOFF1 bytes.
pub fn inspect_sfnt_font_descriptor(
    font_bytes: &[u8],
    fallback_family: &str,
) -> Option<EmbeddedFontDescriptor> {
    let sfnt = sfnt_view_bytes(font_bytes)?;
    let dir_off = sfnt_first_dir_offset(&sfnt)?;
    inspect_sfnt_font_descriptor_at(&sfnt, dir_off, fallback_family)
}

/// Enumerates embedded font descriptors in `project.resources` (combining real SFNT/TTC/WOFF1
/// binaries — including multi-face `ttcf` collections — and `fonts/embedding.json` metadata).
pub fn enumerate_project_embedded_fonts(project: &crate::Project) -> Vec<EmbeddedFontDescriptor> {
    let metadata_list: Vec<CoreEmbeddedFontMetadata> = project
        .resources
        .iter()
        .find(|resource| resource.path.eq_ignore_ascii_case("fonts/embedding.json"))
        .and_then(|resource| resource.decoded_bytes().ok())
        .and_then(|bytes| serde_json::from_slice::<Vec<CoreEmbeddedFontMetadata>>(&bytes).ok())
        .unwrap_or_default();
    let metadata_by_id: BTreeMap<String, &CoreEmbeddedFontMetadata> = metadata_list
        .iter()
        .filter(|entry| !entry.id.is_empty())
        .map(|entry| (entry.id.clone(), entry))
        .collect();

    let mut out = Vec::new();
    let mut seen_ids = std::collections::BTreeSet::new();
    for res in &project.resources {
        if !is_font_resource_unit(res) {
            continue;
        }
        seen_ids.insert(res.id.clone());
        let fallback_family = font_family_for_resource_unit(res);
        let meta = metadata_by_id.get(&res.id).copied();
        let decoded = res.decoded_bytes().ok();
        let faces: Vec<EmbeddedFontDescriptor> = decoded
            .as_deref()
            .and_then(sfnt_view_bytes)
            .and_then(|sfnt| {
                let offsets = sfnt_face_dir_offsets(&sfnt)?;
                let parsed: Vec<EmbeddedFontDescriptor> = offsets
                    .into_iter()
                    .filter_map(|off| inspect_sfnt_font_descriptor_at(&sfnt, off, &fallback_family))
                    .collect();
                (!parsed.is_empty()).then_some(parsed)
            })
            .unwrap_or_default();

        if faces.len() > 1 {
            for face in faces {
                out.push(EmbeddedFontDescriptor {
                    resource_id: res.id.clone(),
                    class_name: String::new(),
                    font_name: face.font_name,
                    font_style: face.font_style,
                    font_type: face.font_type,
                });
            }
            continue;
        }

        let inspected = faces.into_iter().next();
        let font_name = meta
            .filter(|m| !m.family.trim().is_empty())
            .map(|m| m.family.trim().to_owned())
            .or_else(|| {
                inspected
                    .as_ref()
                    .filter(|d| !d.font_name.is_empty())
                    .map(|d| d.font_name.clone())
            })
            .unwrap_or(fallback_family);
        let font_style = meta
            .filter(|m| !m.style.trim().is_empty())
            .map(|m| normalize_as3_font_style(&m.style, false, false))
            .or_else(|| inspected.as_ref().map(|d| d.font_style.clone()))
            .unwrap_or_else(|| "regular".to_owned());
        let font_type = inspected
            .as_ref()
            .map(|d| d.font_type.clone())
            .unwrap_or_else(|| "embedded".to_owned());
        let class_name = meta
            .filter(|m| m.export_for_actionscript && !m.class_name.trim().is_empty())
            .map(|m| m.class_name.trim().to_owned())
            .unwrap_or_default();

        out.push(EmbeddedFontDescriptor {
            resource_id: res.id.clone(),
            class_name,
            font_name,
            font_style,
            font_type,
        });
    }

    for meta in &metadata_list {
        if !meta.id.is_empty() && seen_ids.contains(&meta.id) {
            continue;
        }
        let family = if !meta.family.trim().is_empty() {
            meta.family.trim().to_owned()
        } else if !meta.name.trim().is_empty() {
            meta.name.trim().to_owned()
        } else {
            continue;
        };
        out.push(EmbeddedFontDescriptor {
            resource_id: meta.id.clone(),
            class_name: if meta.export_for_actionscript {
                meta.class_name.trim().to_owned()
            } else {
                String::new()
            },
            font_name: family,
            font_style: normalize_as3_font_style(&meta.style, false, false),
            font_type: "embedded".to_owned(),
        });
    }
    out
}

/// Checks `Font.hasGlyphs(text)` against a matching SFNT/TTC/WOFF1 binary in `project.resources`.
/// Returns `Some(bool)` when a real SFNT binary with `cmap` exists for `(font_name, font_style, font_type)`,
/// or `None` when no matching SFNT binary is embedded in `project.resources`.
pub fn project_font_has_glyphs(
    project: &crate::Project,
    font_name: &str,
    font_style: &str,
    font_type: &str,
    text: &str,
) -> Option<bool> {
    let wanted = font_name.trim();
    if wanted.is_empty() || font_type.eq_ignore_ascii_case("device") {
        return None;
    }
    let wanted_style = if font_style.trim().is_empty() {
        "regular".to_owned()
    } else {
        normalize_as3_font_style(font_style, false, false)
    };
    let metadata_list: Vec<CoreEmbeddedFontMetadata> = project
        .resources
        .iter()
        .find(|resource| resource.path.eq_ignore_ascii_case("fonts/embedding.json"))
        .and_then(|resource| resource.decoded_bytes().ok())
        .and_then(|bytes| serde_json::from_slice::<Vec<CoreEmbeddedFontMetadata>>(&bytes).ok())
        .unwrap_or_default();
    let metadata_by_id: BTreeMap<String, &CoreEmbeddedFontMetadata> = metadata_list
        .iter()
        .filter(|entry| !entry.id.is_empty())
        .map(|entry| (entry.id.clone(), entry))
        .collect();

    let mut family_fallback_face: Option<(Vec<u8>, usize)> = None;
    for res in &project.resources {
        if !is_font_resource_unit(res) {
            continue;
        }
        let Ok(bytes) = res.decoded_bytes() else {
            continue;
        };
        let Some(sfnt) = sfnt_view_bytes(&bytes) else {
            continue;
        };
        let Some(dir_offsets) = sfnt_face_dir_offsets(&sfnt) else {
            continue;
        };
        let fallback_family = font_family_for_resource_unit(res);
        let meta = metadata_by_id.get(&res.id).copied();
        let multi_face = dir_offsets.len() > 1;
        for dir_off in dir_offsets {
            let Some(inspected) = inspect_sfnt_font_descriptor_at(&sfnt, dir_off, &fallback_family)
            else {
                continue;
            };
            let matches_name = inspected.font_name.eq_ignore_ascii_case(wanted)
                || (!multi_face && fallback_family.eq_ignore_ascii_case(wanted))
                || res.id.eq_ignore_ascii_case(wanted)
                || (!multi_face
                    && meta.is_some_and(|m| {
                        m.family.trim().eq_ignore_ascii_case(wanted)
                            || m.name.trim().eq_ignore_ascii_case(wanted)
                            || m.class_name.trim().eq_ignore_ascii_case(wanted)
                    }));
            if !matches_name {
                continue;
            }
            let eff_style = if !multi_face {
                meta.filter(|m| !m.style.trim().is_empty())
                    .map(|m| normalize_as3_font_style(&m.style, false, false))
                    .unwrap_or_else(|| inspected.font_style.clone())
            } else {
                inspected.font_style.clone()
            };
            if eff_style == wanted_style {
                return sfnt_has_glyphs_at(&sfnt, dir_off, text);
            }
            if family_fallback_face.is_none() {
                family_fallback_face = Some((sfnt.as_ref().to_vec(), dir_off));
            }
        }
    }
    family_fallback_face
        .and_then(|(sfnt_bytes, dir_off)| sfnt_has_glyphs_at(&sfnt_bytes, dir_off, text))
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

    #[test]
    fn woff1_container_roundtrips_metrics_outlines_and_portable_player_resolution() {
        let sfnt = build_synthetic_sfnt_with_tables(&[]);
        let woff = crate::pack_sfnt_to_woff(&sfnt).expect("pack synthetic SFNT to WOFF1");
        assert!(woff.starts_with(b"wOFF"));
        assert_eq!(
            crate::sniff_font_media_type(&woff, "application/octet-stream"),
            "font/woff"
        );
        let unpacked = crate::unpack_woff_to_sfnt(&woff).expect("unpack WOFF1 back to SFNT");
        assert_eq!(crate::sfnt_checksum(&unpacked), 0xB1B0_AFBA);

        let woff_metrics =
            PortableFontMetrics::from_bytes("SyntheticWoff", &woff).expect("WOFF1 metrics");
        assert_eq!(woff_metrics.units_per_em, 1000);
        assert_eq!(woff_metrics.advances.get(&('A' as u32)).copied(), Some(600));
        assert!(woff_metrics.has_outline_for_char('Á'));

        // Build a project containing ONLY the WOFF1 font under an unknown custom family name and
        // ensure Player::object_text_fit_portable uses its embedded metrics (advance 600 vs builtin).
        let mut project = crate::Project::blank();
        project.resources.push(
            crate::ResourceUnit::from_bytes(
                "font-woff",
                "fonts/SyntheticWoff.woff",
                "font/woff",
                &woff,
            )
            .expect("woff resource"),
        );
        let mut fmt = TextFormat {
            font: "SyntheticWoff".to_owned(),
            size: 100.0,
            ..TextFormat::default()
        };
        fmt.set_field_options(Some(AuthoredTextFieldOptions {
            auto_size: "left".to_owned(),
            ..AuthoredTextFieldOptions::default()
        }));
        project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(crate::Element {
                id: "woff-box".to_owned(),
                name: "woffBox".to_owned(),
                kind: crate::ElementKind::TextBox {
                    text: "AAAA".to_owned(),
                    width: 40.0,
                    height: 30.0,
                    format: fmt,
                    word_wrap: false,
                    multiline: false,
                },
                ..crate::Element::default()
            });

        let mut player = crate::Player::new(&project);
        let child_path = player
            .display_nodes(&project)
            .into_iter()
            .find(|node| node.name == "woffBox")
            .expect("woffBox display node")
            .path;
        let child_ref = player.object_ref(&child_path).expect("woffBox object_ref");
        assert!(
            player
                .object_text_fit_portable(&project, &child_ref)
                .expect("fit portable")
        );
        let fitted = player
            .object_text_info(&project, &child_ref)
            .expect("fitted text info");
        // 4 * (600 / 1000 * 100) = 240 + 4px TextField padding = 244 (vs builtin 722 -> 292.8)
        assert!(
            (fitted.box_size.as_ref().expect("box_size").width - 244.0).abs() < 2.5,
            "expected WOFF1 embedded advance width (~244.0), got {:?}",
            fitted.box_size
        );
    }

    fn build_sfnt_from_tables(sfnt_tag: [u8; 4], tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
        let num_tables = tables.len() as u16;
        let mut out = Vec::new();
        out.extend_from_slice(&sfnt_tag);
        out.extend_from_slice(&num_tables.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        let mut offset = 12 + tables.len() * 16;
        for (tag, data) in tables {
            out.extend_from_slice(tag);
            out.extend_from_slice(&0u32.to_be_bytes());
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            offset += data.len();
        }
        for (_, data) in tables {
            out.extend_from_slice(data);
        }
        out
    }

    fn build_utf16be_name_table(family: &str, subfamily: &str) -> Vec<u8> {
        let enc = |s: &str| -> Vec<u8> { s.encode_utf16().flat_map(u16::to_be_bytes).collect() };
        let fam_bytes = enc(family);
        let sub_bytes = enc(subfamily);
        let mut tbl = Vec::new();
        tbl.extend_from_slice(&0u16.to_be_bytes()); // format 0
        tbl.extend_from_slice(&2u16.to_be_bytes()); // count = 2
        let storage_off = (6 + 2 * 12) as u16;
        tbl.extend_from_slice(&storage_off.to_be_bytes());
        // Record 0: nameID 1 (Family)
        tbl.extend_from_slice(&3u16.to_be_bytes()); // Windows
        tbl.extend_from_slice(&1u16.to_be_bytes()); // Unicode BMP
        tbl.extend_from_slice(&0x0409u16.to_be_bytes()); // en-US
        tbl.extend_from_slice(&1u16.to_be_bytes()); // nameID 1
        tbl.extend_from_slice(&(fam_bytes.len() as u16).to_be_bytes());
        tbl.extend_from_slice(&0u16.to_be_bytes());
        // Record 1: nameID 2 (Subfamily)
        tbl.extend_from_slice(&3u16.to_be_bytes());
        tbl.extend_from_slice(&1u16.to_be_bytes());
        tbl.extend_from_slice(&0x0409u16.to_be_bytes());
        tbl.extend_from_slice(&2u16.to_be_bytes()); // nameID 2
        tbl.extend_from_slice(&(sub_bytes.len() as u16).to_be_bytes());
        tbl.extend_from_slice(&(fam_bytes.len() as u16).to_be_bytes());
        tbl.extend_from_slice(&fam_bytes);
        tbl.extend_from_slice(&sub_bytes);
        tbl
    }

    #[test]
    fn t326_sfnt_cmap_format4_and_format12_font_has_glyphs_and_descriptor() {
        // 1. Format-4-only TrueType font mapping space (0x20), 'A'..'Z' (65..=90),
        // Cyrillic 'Я' (0x042F) via idDelta, Greek 'Ω' (0x03A9) via non-zero idRangeOffset,
        // and Hiragana 'あ' (0x3042) via idDelta.
        let seg_count: u16 = 6; // 0x20, 65..90, 0x03A9, 0x042F, 0x3042, 0xFFFF
        let end_codes: [u16; 6] = [0x0020, 0x005A, 0x03A9, 0x042F, 0x3042, 0xFFFF];
        let start_codes: [u16; 6] = [0x0020, 0x0041, 0x03A9, 0x042F, 0x3042, 0xFFFF];
        // Segment 2 (0x03A9) uses glyphIdArray (1 u16 entry = glyph 30) via idRangeOffset:
        // range_off_pos is at sub_start + 16 + 3*segCount*2 + 2*2; glyphIdArray starts at sub_start + 16 + 4*segCount*2.
        // Byte distance from range_off_pos (segment index 2 out of 6) to glyphIdArray[0] = (6 - 2) * 2 = 8!
        let id_deltas: [i16; 6] = [1 - 0x0020, 2 - 0x0041, 0, 40 - 0x042F, 50 - 0x3042, 1];
        let id_range_offsets: [u16; 6] = [0, 0, 8, 0, 0, 0];

        let mut fmt4 = Vec::new();
        let sub_len: u16 = 16 + 4 * seg_count * 2 + 2; // + 1 u16 in glyphIdArray
        fmt4.extend_from_slice(&4u16.to_be_bytes());
        fmt4.extend_from_slice(&sub_len.to_be_bytes());
        fmt4.extend_from_slice(&0u16.to_be_bytes());
        fmt4.extend_from_slice(&(seg_count * 2).to_be_bytes());
        fmt4.extend_from_slice(&8u16.to_be_bytes());
        fmt4.extend_from_slice(&2u16.to_be_bytes());
        fmt4.extend_from_slice(&4u16.to_be_bytes());
        for v in end_codes {
            fmt4.extend_from_slice(&v.to_be_bytes());
        }
        fmt4.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
        for v in start_codes {
            fmt4.extend_from_slice(&v.to_be_bytes());
        }
        for v in id_deltas {
            fmt4.extend_from_slice(&(v as u16).to_be_bytes());
        }
        for v in id_range_offsets {
            fmt4.extend_from_slice(&v.to_be_bytes());
        }
        fmt4.extend_from_slice(&30u16.to_be_bytes()); // glyphIdArray[0] -> glyph 30 for 'Ω'

        let mut cmap4 = Vec::new();
        cmap4.extend_from_slice(&0u16.to_be_bytes());
        cmap4.extend_from_slice(&1u16.to_be_bytes());
        cmap4.extend_from_slice(&3u16.to_be_bytes()); // Windows
        cmap4.extend_from_slice(&1u16.to_be_bytes()); // Unicode BMP
        cmap4.extend_from_slice(&12u32.to_be_bytes());
        cmap4.extend_from_slice(&fmt4);

        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes());
        head[44..46].copy_from_slice(&0x0003u16.to_be_bytes()); // macStyle bold + italic
        let sfnt4 = build_sfnt_from_tables(
            [0, 1, 0, 0],
            &[
                (*b"cmap", cmap4),
                (*b"head", head.clone()),
                (
                    *b"name",
                    build_utf16be_name_table("NeoBmpSans", "Bold Italic"),
                ),
            ],
        );

        assert_eq!(sfnt_has_glyphs(&sfnt4, "AZЯあΩ "), Some(true));
        assert_eq!(sfnt_has_glyphs(&sfnt4, "AЖ"), Some(false)); // unmapped BMP Cyrillic 'Ж'
        assert_eq!(sfnt_has_glyphs(&sfnt4, "A😀"), Some(false)); // SMP not in Format 4
        assert_eq!(sfnt_has_glyphs(&sfnt4, "A\nB"), Some(false)); // newline control rejected
        let desc4 = inspect_sfnt_font_descriptor(&sfnt4, "Fallback").expect("desc4");
        assert_eq!(desc4.font_name, "NeoBmpSans");
        assert_eq!(desc4.font_style, "boldItalic");
        assert_eq!(desc4.font_type, "embedded");

        // 2. Format-12 OpenType/CFF font mapping space (0x20), BMP '漢' (0x6F22),
        // SMP CJK Extension B '𠮷' (0x20BB7), and SMP emoji '😀' (0x1F600).
        let mut cmap12 = Vec::new();
        cmap12.extend_from_slice(&0u16.to_be_bytes());
        cmap12.extend_from_slice(&1u16.to_be_bytes());
        cmap12.extend_from_slice(&3u16.to_be_bytes()); // Windows
        cmap12.extend_from_slice(&10u16.to_be_bytes()); // Unicode full repertoire
        cmap12.extend_from_slice(&12u32.to_be_bytes());
        cmap12.extend_from_slice(&12u16.to_be_bytes()); // format 12
        cmap12.extend_from_slice(&0u16.to_be_bytes());
        cmap12.extend_from_slice(&(16u32 + 4 * 12).to_be_bytes());
        cmap12.extend_from_slice(&0u32.to_be_bytes());
        cmap12.extend_from_slice(&4u32.to_be_bytes()); // 4 groups (sorted by startCharCode!)
        for (start, end, gid) in [
            (0x0020u32, 0x0020u32, 1u32),
            (0x6F22u32, 0x6F22u32, 2u32),
            (0x1F600u32, 0x1F605u32, 10u32),
            (0x20BB7u32, 0x20BB7u32, 20u32),
        ] {
            cmap12.extend_from_slice(&start.to_be_bytes());
            cmap12.extend_from_slice(&end.to_be_bytes());
            cmap12.extend_from_slice(&gid.to_be_bytes());
        }
        let mut head_reg = vec![0u8; 54];
        head_reg[18..20].copy_from_slice(&1000u16.to_be_bytes());
        let sfnt12 = build_sfnt_from_tables(
            *b"OTTO",
            &[
                (*b"CFF ", vec![1, 0, 4, 1]),
                (*b"cmap", cmap12),
                (*b"head", head_reg),
                (*b"name", build_utf16be_name_table("NeoSmpCff", "Regular")),
            ],
        );

        assert_eq!(sfnt_has_glyphs(&sfnt12, "漢 𠮷 😀"), Some(true));
        assert_eq!(sfnt_has_glyphs(&sfnt12, "漢🚀"), Some(false)); // 0x1F680 unmapped
        let desc12 = inspect_sfnt_font_descriptor(&sfnt12, "Fallback").expect("desc12");
        assert_eq!(desc12.font_name, "NeoSmpCff");
        assert_eq!(desc12.font_style, "regular");
        assert_eq!(desc12.font_type, "embeddedCFF");

        // Project enumeration + lookup end-to-end
        let mut project = crate::Project::blank();
        project.resources.push(
            crate::ResourceUnit::from_bytes("f4", "fonts/NeoBmpSans.ttf", "font/ttf", &sfnt4)
                .unwrap(),
        );
        project.resources.push(
            crate::ResourceUnit::from_bytes("f12", "fonts/NeoSmpCff.otf", "font/otf", &sfnt12)
                .unwrap(),
        );
        let enumerated = enumerate_project_embedded_fonts(&project);
        assert_eq!(enumerated.len(), 2);
        assert_eq!(
            project_font_has_glyphs(&project, "NeoBmpSans", "boldItalic", "", "AZΩ"),
            Some(true)
        );
        assert_eq!(
            project_font_has_glyphs(&project, "NeoBmpSans", "boldItalic", "", "😀"),
            Some(false)
        );
        assert_eq!(
            project_font_has_glyphs(&project, "NeoSmpCff", "regular", "embeddedCFF", "𠮷😀"),
            Some(true)
        );

        // 3. Synthetic 2-face OpenType Collection (`ttcf` v1.0, `numFonts = 2`, table record
        //    offsets relative to `&ttc[0..]` per OpenType TTC spec): face 0 = sfnt4, face 1 = sfnt12.
        fn pack_sfnts_into_ttc(faces: &[&[u8]]) -> Vec<u8> {
            let header_len = 12 + faces.len() * 4;
            let mut out = Vec::new();
            out.extend_from_slice(b"ttcf");
            out.extend_from_slice(&1u16.to_be_bytes());
            out.extend_from_slice(&0u16.to_be_bytes());
            out.extend_from_slice(&(faces.len() as u32).to_be_bytes());
            out.resize(header_len, 0);
            for (idx, face) in faces.iter().enumerate() {
                while !out.len().is_multiple_of(4) {
                    out.push(0);
                }
                let base = out.len();
                let off_pos = 12 + idx * 4;
                out[off_pos..off_pos + 4].copy_from_slice(&(base as u32).to_be_bytes());
                let mut relocated = face.to_vec();
                let num_tables = u16::from_be_bytes([relocated[4], relocated[5]]) as usize;
                for t in 0..num_tables {
                    let rec_off = 12 + t * 16 + 8;
                    let orig_table_off = u32::from_be_bytes([
                        relocated[rec_off],
                        relocated[rec_off + 1],
                        relocated[rec_off + 2],
                        relocated[rec_off + 3],
                    ]) as usize;
                    let new_table_off = (base + orig_table_off) as u32;
                    relocated[rec_off..rec_off + 4].copy_from_slice(&new_table_off.to_be_bytes());
                }
                out.extend_from_slice(&relocated);
            }
            out
        }

        let ttc = pack_sfnts_into_ttc(&[&sfnt4, &sfnt12]);
        assert_eq!(sfnt_has_glyphs(&ttc, "AZЯあΩ "), Some(true));
        assert_eq!(sfnt_has_glyphs(&ttc, "A😀"), Some(false));
        let ttc_desc = inspect_sfnt_font_descriptor(&ttc, "Fallback").expect("ttc face 0 desc");
        assert_eq!(ttc_desc.font_name, "NeoBmpSans");

        let mut ttc_project = crate::Project::blank();
        ttc_project.resources.push(
            crate::ResourceUnit::from_bytes("ttc1", "fonts/NeoBundle.ttc", "font/collection", &ttc)
                .unwrap(),
        );
        let ttc_enum = enumerate_project_embedded_fonts(&ttc_project);
        assert_eq!(ttc_enum.len(), 2);
        assert_eq!(ttc_enum[0].font_name, "NeoBmpSans");
        assert_eq!(ttc_enum[1].font_name, "NeoSmpCff");
        assert_eq!(
            project_font_has_glyphs(&ttc_project, "NeoBmpSans", "boldItalic", "", "AZΩ"),
            Some(true)
        );
        assert_eq!(
            project_font_has_glyphs(&ttc_project, "NeoBmpSans", "boldItalic", "", "😀"),
            Some(false)
        );
        assert_eq!(
            project_font_has_glyphs(&ttc_project, "NeoSmpCff", "regular", "embeddedCFF", "𠮷😀"),
            Some(true)
        );
        assert_eq!(
            project_font_has_glyphs(&ttc_project, "NeoSmpCff", "regular", "embeddedCFF", "🚀"),
            Some(false)
        );

        // Face-indexed outline validation & standalone SFNT extraction for `.ttc` faces > 0:
        let color_face0 = build_synthetic_sfnt_with_tables(&[(*b"CBDT", vec![0; 16])]);
        let mut clean_bold_face1 = build_synthetic_sfnt_with_tables(&[(
            *b"name",
            build_utf16be_name_table("NeoBundle", "Bold"),
        )]);
        // Mark head.macStyle = 0x0001 (Bold) inside clean_bold_face1
        if let Some(head_slice) = sfnt_table_slice_at(&clean_bold_face1, 0, b"head") {
            let head_ptr_off = head_slice.as_ptr() as usize - clean_bold_face1.as_ptr() as usize;
            clean_bold_face1[head_ptr_off + 44..head_ptr_off + 46]
                .copy_from_slice(&0x0001u16.to_be_bytes());
        }
        let mixed_ttc = pack_sfnts_into_ttc(&[&color_face0, &clean_bold_face1]);
        assert!(
            !TextFormat::validate_text_outlines_for_font("loc", "NeoBundle", "AÁ", &mixed_ttc)
                .is_empty(),
            "face 0 has CBDT and must be flagged"
        );
        assert!(
            TextFormat::validate_text_outlines_for_font_face(
                "loc",
                "NeoBundle",
                "AÁ",
                &mixed_ttc,
                1,
            )
            .is_empty(),
            "face 1 is a clean vector glyf face and must pass validation"
        );
        assert_eq!(
            select_best_sfnt_face_index(&mixed_ttc, "NeoBundle", true, false),
            1
        );
        let standalone_face1 =
            extract_ttc_face_to_sfnt(&mixed_ttc, 1).expect("extract face 1 SFNT");
        assert!(
            TextFormat::unsupported_outline_font_tables(&standalone_face1).is_empty(),
            "extracted face 1 SFNT must not inherit face 0's CBDT table"
        );
        let f1_metrics = PortableFontMetrics::from_bytes("NeoBundle", &standalone_face1)
            .expect("face 1 metrics");
        assert!(f1_metrics.has_outline_for_char('A'));
        assert!(f1_metrics.has_outline_for_char('Á'));
    }
}
