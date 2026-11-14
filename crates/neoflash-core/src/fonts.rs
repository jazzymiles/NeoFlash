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

#[derive(Clone, Debug, Default, Deserialize)]
struct CoreEmbeddedFontMetadata {
    #[serde(default)]
    id: String,
    #[serde(default)]
    family: String,
    #[serde(default)]
    style: String,
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
            | "font/woff"
            | "font/woff2"
            | "application/font-sfnt"
            | "application/x-font-ttf"
            | "application/x-font-opentype"
            | "application/vnd.ms-opentype"
            | "application/font-woff"
    ) {
        return true;
    }
    let path_lower = resource.path.to_ascii_lowercase();
    path_lower.ends_with(".ttf")
        || path_lower.ends_with(".otf")
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

/// Returns an SFNT view over raw SFNT or WOFF1 bytes.
pub fn sfnt_view_bytes(bytes: &[u8]) -> Option<std::borrow::Cow<'_, [u8]>> {
    if bytes.len() < 12 {
        return None;
    }
    let sig = &bytes[0..4];
    if sig == [0, 1, 0, 0] || sig == b"OTTO" || sig == b"true" || sig == b"typ1" {
        return Some(std::borrow::Cow::Borrowed(bytes));
    }
    if sig == b"wOFF" {
        return unpack_woff_to_sfnt(bytes).map(std::borrow::Cow::Owned);
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

pub(crate) fn parse_cmap_subtables(cmap: &[u8]) -> BTreeMap<u32, u16> {
    let mut cmap_map = BTreeMap::<u32, u16>::new();
    if cmap.len() < 4 {
        return cmap_map;
    }
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
                        if cmap_map.len() >= MAX_GLYPHS {
                            return cmap_map;
                        }
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
            for g in 0..n_groups.min(16384) {
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
                if end_c >= start_c && end_c - start_c <= MAX_GLYPHS as u32 {
                    for c in start_c..=end_c {
                        if cmap_map.len() >= MAX_GLYPHS {
                            return cmap_map;
                        }
                        let gid = (start_gid + (c - start_c)) as u16;
                        if gid > 0 {
                            cmap_map.insert(c, gid);
                        }
                    }
                }
            }
        }
    }
    cmap_map
}

fn parse_sfnt_cmap_table(bytes: &[u8]) -> Option<std::collections::BTreeMap<u32, u16>> {
    let sfnt = sfnt_view_bytes(bytes)?;
    let num_tables = u16::from_be_bytes([sfnt[4], sfnt[5]]) as usize;
    let mut cmap_slice: Option<&[u8]> = None;
    for i in 0..num_tables.min(128) {
        let entry = 12 + i * 16;
        if entry + 16 > sfnt.len() {
            break;
        }
        let tag = &sfnt[entry..entry + 4];
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
        if tag == b"cmap" && offset.saturating_add(length) <= sfnt.len() {
            cmap_slice = Some(&sfnt[offset..offset + length]);
            break;
        }
    }
    Some(parse_cmap_subtables(cmap_slice?))
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
        assert_eq!(crate::sniff_font_media_type(&woff, "application/octet-stream"), "font/woff");
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
            crate::ResourceUnit::from_bytes("font-woff", "fonts/SyntheticWoff.woff", "font/woff", &woff)
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
        let child_ref = player
            .object_ref(&child_path)
            .expect("woffBox object_ref");
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
}
