use serde::{Deserialize, Serialize};

use super::graphemes::{clamp_utf16, utf16_byte};
use super::*;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontPolicy {
    Device,
    #[default]
    Embed,
    Subset,
    Outlines,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PortableFontMetrics {
    pub family: String,
    pub units_per_em: u16,
    pub ascent: i16,
    pub descent: i16,
    pub line_gap: i16,
    pub default_advance: u16,
    pub advances: std::collections::BTreeMap<u32, u16>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub glyphs: std::collections::BTreeMap<u32, Vec<crate::PathCommand>>,
}

impl PortableFontMetrics {
    pub fn builtin(family: &str) -> Self {
        let lower = family.trim().to_ascii_lowercase();
        let is_mono = lower.contains("mono") || lower.contains("courier") || lower.contains("code");
        let is_serif = !is_mono
            && (lower.contains("serif") && !lower.contains("sans")
                || lower.contains("times")
                || lower.contains("georgia"));
        let mut advances = std::collections::BTreeMap::new();
        for cp in 32u32..=126 {
            let ch = char::from_u32(cp).unwrap_or(' ');
            let adv = if is_mono {
                600
            } else {
                match ch {
                    ' ' => 278,
                    '!' | '\'' | '|' | 'i' | 'l' | 'I' | '.' | ',' | ':' | ';' => {
                        if is_serif {
                            278
                        } else {
                            250
                        }
                    }
                    'j' | 'f' | 't' | 'r' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '`' => {
                        if is_serif {
                            333
                        } else {
                            310
                        }
                    }
                    's' | 'c' | 'z' | 'v' | 'x' | 'y' | 'k' | 'J' => 500,
                    'a' | 'b' | 'd' | 'e' | 'g' | 'h' | 'n' | 'o' | 'p' | 'q' | 'u' | '0'..='9' => {
                        556
                    }
                    'm' | 'w' => 833,
                    'M' | 'W' => 889,
                    'A'..='Z' => {
                        if is_serif {
                            667
                        } else {
                            680
                        }
                    }
                    '@' | '%' => 889,
                    _ => 556,
                }
            };
            advances.insert(cp, adv);
        }
        Self {
            family: family.to_string(),
            units_per_em: 1000,
            ascent: 800,
            descent: -200,
            line_gap: 200,
            default_advance: if is_mono { 600 } else { 580 },
            advances,
            glyphs: std::collections::BTreeMap::new(),
        }
    }

    pub fn from_bytes(family: &str, bytes: &[u8]) -> Result<Self, TextError> {
        if bytes.starts_with(b"NFPM") && bytes.len() >= 16 {
            let units_per_em = u16::from_be_bytes([bytes[4], bytes[5]]).max(1);
            let ascent = i16::from_be_bytes([bytes[6], bytes[7]]);
            let descent = i16::from_be_bytes([bytes[8], bytes[9]]);
            let line_gap = i16::from_be_bytes([bytes[10], bytes[11]]);
            let count = u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
            let mut advances = std::collections::BTreeMap::new();
            let mut pos = 16usize;
            for _ in 0..count {
                if pos + 6 > bytes.len() {
                    break;
                }
                let cp = u32::from_be_bytes([
                    bytes[pos],
                    bytes[pos + 1],
                    bytes[pos + 2],
                    bytes[pos + 3],
                ]);
                let adv = u16::from_be_bytes([bytes[pos + 4], bytes[pos + 5]]);
                advances.insert(cp, adv);
                pos += 6;
            }
            return Ok(Self {
                family: family.to_string(),
                units_per_em,
                ascent,
                descent,
                line_gap,
                default_advance: (units_per_em * 3) / 5,
                advances,
                glyphs: std::collections::BTreeMap::new(),
            });
        }
        if bytes.len() >= 12 {
            let tag = &bytes[0..4];
            if (tag == [0, 1, 0, 0] || tag == b"OTTO" || tag == b"true" || tag == b"wOFF")
                && let Some(parsed) = parse_sfnt_metrics(family, bytes)
            {
                return Ok(parsed);
            }
        }
        Ok(Self::builtin(family))
    }

    pub fn advance_for_char(&self, ch: char) -> u16 {
        let cp = ch as u32;
        if let Some(&adv) = self.advances.get(&cp) {
            return adv;
        }
        if (0x2E80..=0x9FFF).contains(&cp) {
            return self.units_per_em;
        }
        self.default_advance
    }

    pub fn measure_string(&self, text: &str, format: &TextFormat) -> f64 {
        let upm = f64::from(self.units_per_em.max(1));
        let scale = format.size.max(1.0) / upm;
        let bold_factor = if format.bold { 1.06 } else { 1.0 };
        let mut total = 0.0;
        let mut count = 0usize;
        for ch in text.chars() {
            if ch == '\r' || ch == '\n' {
                continue;
            }
            total += f64::from(self.advance_for_char(ch)) * scale * bold_factor;
            count += 1;
        }
        if count > 1 && format.letter_spacing.is_finite() {
            total += (count.saturating_sub(1) as f64) * format.letter_spacing;
        }
        total.max(0.0)
    }

    pub fn measure_strings(&self, strings: &[String], format: &TextFormat) -> Vec<f64> {
        strings
            .iter()
            .map(|s| self.measure_string(s, format))
            .collect()
    }

    pub fn subset(&self, codepoints: &std::collections::BTreeSet<u32>) -> Self {
        let mut advances = std::collections::BTreeMap::new();
        let mut glyphs = std::collections::BTreeMap::new();
        for &cp in codepoints.iter().chain([0u32, 32u32].iter()) {
            let ch = char::from_u32(cp).unwrap_or(' ');
            let adv = self
                .advances
                .get(&cp)
                .copied()
                .unwrap_or_else(|| self.advance_for_char(ch));
            advances.insert(cp, adv);
            if let Some(g) = self.glyphs.get(&cp) {
                glyphs.insert(cp, g.clone());
            }
        }
        Self {
            family: self.family.clone(),
            units_per_em: self.units_per_em,
            ascent: self.ascent,
            descent: self.descent,
            line_gap: self.line_gap,
            default_advance: self.default_advance,
            advances,
            glyphs,
        }
    }

    pub fn encode_subset_binary(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.advances.len() * 6);
        out.extend_from_slice(b"NFPM");
        out.extend_from_slice(&self.units_per_em.to_be_bytes());
        out.extend_from_slice(&self.ascent.to_be_bytes());
        out.extend_from_slice(&self.descent.to_be_bytes());
        out.extend_from_slice(&self.line_gap.to_be_bytes());
        out.extend_from_slice(&(self.advances.len() as u32).to_be_bytes());
        for (&cp, &adv) in &self.advances {
            out.extend_from_slice(&cp.to_be_bytes());
            out.extend_from_slice(&adv.to_be_bytes());
        }
        out
    }

    pub fn glyph_outline(&self, ch: char, x: f64, y: f64, size: f64) -> Vec<crate::PathCommand> {
        if ch.is_whitespace() {
            return Vec::new();
        }
        let cp = ch as u32;
        if let Some(contours) = self.glyphs.get(&cp)
            && !contours.is_empty()
        {
            let upm = f64::from(self.units_per_em.max(1));
            let scale = size.max(1.0) / upm;
            let baseline = y + f64::from(self.ascent.max(1)) * scale;
            return contours
                .iter()
                .map(|cmd| match *cmd {
                    crate::PathCommand::MoveTo { x: gx, y: gy } => crate::PathCommand::MoveTo {
                        x: x + gx * scale,
                        y: baseline - gy * scale,
                    },
                    crate::PathCommand::LineTo { x: gx, y: gy } => crate::PathCommand::LineTo {
                        x: x + gx * scale,
                        y: baseline - gy * scale,
                    },
                    crate::PathCommand::QuadraticTo {
                        cx,
                        cy,
                        x: gx,
                        y: gy,
                    } => crate::PathCommand::QuadraticTo {
                        cx: x + cx * scale,
                        cy: baseline - cy * scale,
                        x: x + gx * scale,
                        y: baseline - gy * scale,
                    },
                    crate::PathCommand::CubicTo {
                        cx1,
                        cy1,
                        cx2,
                        cy2,
                        x: gx,
                        y: gy,
                    } => crate::PathCommand::CubicTo {
                        cx1: x + cx1 * scale,
                        cy1: baseline - cy1 * scale,
                        cx2: x + cx2 * scale,
                        cy2: baseline - cy2 * scale,
                        x: x + gx * scale,
                        y: baseline - gy * scale,
                    },
                    crate::PathCommand::Close => crate::PathCommand::Close,
                })
                .collect();
        }
        synthesize_glyph_outline(ch, x, y, size, self.advance_for_char(ch), self.units_per_em)
    }
}

fn parse_sfnt_metrics(family: &str, bytes: &[u8]) -> Option<PortableFontMetrics> {
    let sfnt = sfnt_view_bytes(bytes)?;
    let bytes = sfnt.as_ref();
    let dir_off = sfnt_first_dir_offset(bytes)?;
    let num_tables = u16::from_be_bytes([bytes[dir_off + 4], bytes[dir_off + 5]]) as usize;
    let mut tables = std::collections::HashMap::<[u8; 4], (usize, usize)>::new();
    for i in 0..num_tables.min(128) {
        let entry = dir_off + 12 + i * 16;
        if entry + 16 > bytes.len() {
            break;
        }
        let tag = [
            bytes[entry],
            bytes[entry + 1],
            bytes[entry + 2],
            bytes[entry + 3],
        ];
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
        if offset.saturating_add(length) <= bytes.len() {
            tables.insert(tag, (offset, length));
        }
    }
    let &(head_off, head_len) = tables.get(b"head")?;
    if head_len < 54 {
        return None;
    }
    let head = &bytes[head_off..head_off + head_len];
    let units_per_em = u16::from_be_bytes([head[18], head[19]]).max(16);
    let index_to_loc_format = i16::from_be_bytes([head[50], head[51]]);

    let mut ascent = ((units_per_em as i32 * 4) / 5) as i16;
    let mut descent = -((units_per_em as i32) / 5) as i16;
    let mut line_gap = ((units_per_em as i32) / 5) as i16;
    let mut num_h_metrics = 1usize;
    if let Some(&(hhea_off, hhea_len)) = tables.get(b"hhea")
        && hhea_len >= 36
    {
        let hhea = &bytes[hhea_off..hhea_off + hhea_len];
        ascent = i16::from_be_bytes([hhea[4], hhea[5]]);
        descent = i16::from_be_bytes([hhea[6], hhea[7]]);
        line_gap = i16::from_be_bytes([hhea[8], hhea[9]]);
        num_h_metrics = u16::from_be_bytes([hhea[34], hhea[35]]).max(1) as usize;
    }
    let mut num_glyphs = num_h_metrics;
    if let Some(&(maxp_off, maxp_len)) = tables.get(b"maxp")
        && maxp_len >= 6
    {
        let maxp = &bytes[maxp_off..maxp_off + maxp_len];
        num_glyphs = u16::from_be_bytes([maxp[4], maxp[5]]).max(1) as usize;
    }

    let mut glyph_advances = Vec::with_capacity(num_glyphs);
    if let Some(&(hmtx_off, hmtx_len)) = tables.get(b"hmtx") {
        let hmtx = &bytes[hmtx_off..hmtx_off + hmtx_len];
        let mut last_adv = (units_per_em * 3) / 5;
        for g in 0..num_glyphs.min(65535) {
            if g < num_h_metrics && (g + 1) * 4 <= hmtx.len() {
                last_adv = u16::from_be_bytes([hmtx[g * 4], hmtx[g * 4 + 1]]);
            }
            glyph_advances.push(last_adv);
        }
    }

    let mut cmap_map = std::collections::BTreeMap::<u32, u16>::new();
    if let Some(&(cmap_off, cmap_len)) = tables.get(b"cmap")
        && cmap_len >= 4
    {
        let cmap = &bytes[cmap_off..cmap_off + cmap_len];
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
                let seg_count_x2 =
                    u16::from_be_bytes([cmap[sub_off + 6], cmap[sub_off + 7]]) as usize;
                let seg_count = seg_count_x2 / 2;
                let end_codes = sub_off + 14;
                let start_codes = end_codes + seg_count_x2 + 2;
                let id_deltas = start_codes + seg_count_x2;
                let id_range_offsets = id_deltas + seg_count_x2;
                if id_range_offsets + seg_count_x2 <= cmap.len() {
                    for s in 0..seg_count {
                        let end_c = u16::from_be_bytes([
                            cmap[end_codes + s * 2],
                            cmap[end_codes + s * 2 + 1],
                        ]);
                        let start_c = u16::from_be_bytes([
                            cmap[start_codes + s * 2],
                            cmap[start_codes + s * 2 + 1],
                        ]);
                        if start_c == 0xFFFF {
                            break;
                        }
                        let delta = i16::from_be_bytes([
                            cmap[id_deltas + s * 2],
                            cmap[id_deltas + s * 2 + 1],
                        ]) as i32;
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
                    let start_c = u32::from_be_bytes([
                        cmap[grp],
                        cmap[grp + 1],
                        cmap[grp + 2],
                        cmap[grp + 3],
                    ]);
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
    }

    let mut base = PortableFontMetrics::builtin(family);
    base.units_per_em = units_per_em;
    base.ascent = ascent;
    base.descent = descent;
    base.line_gap = line_gap;
    base.default_advance = glyph_advances
        .first()
        .copied()
        .unwrap_or((units_per_em * 3) / 5);
    for (&cp, &gid) in &cmap_map {
        if let Some(&adv) = glyph_advances.get(gid as usize) {
            base.advances.insert(cp, adv);
        }
    }

    if let (Some(&(loca_off, loca_len)), Some(&(glyf_off, glyf_len))) =
        (tables.get(b"loca"), tables.get(b"glyf"))
    {
        let loca = &bytes[loca_off..loca_off + loca_len];
        let glyf = &bytes[glyf_off..glyf_off + glyf_len];
        for (&cp, &gid) in &cmap_map {
            if let Some(cmds) =
                parse_glyf_outline_with_composites(glyf, loca, index_to_loc_format, gid, 0)
            {
                base.glyphs.insert(cp, cmds);
            }
        }
    }

    Some(base)
}

pub(super) fn parse_simple_glyf_outline(slice: &[u8]) -> Option<Vec<crate::PathCommand>> {
    if slice.len() < 12 {
        return None;
    }
    let num_contours = i16::from_be_bytes([slice[0], slice[1]]);
    if num_contours <= 0 || num_contours > 64 {
        return None;
    }
    let c_count = num_contours as usize;
    let end_pts_end = 10 + c_count * 2;
    if end_pts_end + 2 > slice.len() {
        return None;
    }
    let mut end_pts = Vec::with_capacity(c_count);
    for i in 0..c_count {
        let p = 10 + i * 2;
        end_pts.push(u16::from_be_bytes([slice[p], slice[p + 1]]) as usize);
    }
    let num_points = (*end_pts.last()?) + 1;
    if num_points > 2048 {
        return None;
    }
    let insn_len = u16::from_be_bytes([slice[end_pts_end], slice[end_pts_end + 1]]) as usize;
    let mut pos = end_pts_end + 2 + insn_len;
    if pos > slice.len() {
        return None;
    }
    let mut flags = Vec::with_capacity(num_points);
    while flags.len() < num_points && pos < slice.len() {
        let f = slice[pos];
        pos += 1;
        flags.push(f);
        if (f & 0x08) != 0 && pos < slice.len() {
            let repeat = slice[pos] as usize;
            pos += 1;
            for _ in 0..repeat.min(num_points - flags.len()) {
                flags.push(f);
            }
        }
    }
    if flags.len() != num_points {
        return None;
    }
    let mut xs = Vec::with_capacity(num_points);
    let mut cur_x = 0i32;
    for &f in &flags {
        if (f & 0x02) != 0 {
            if pos >= slice.len() {
                return None;
            }
            let dx = slice[pos] as i32;
            pos += 1;
            cur_x += if (f & 0x10) != 0 { dx } else { -dx };
        } else if (f & 0x10) == 0 {
            if pos + 2 > slice.len() {
                return None;
            }
            cur_x += i16::from_be_bytes([slice[pos], slice[pos + 1]]) as i32;
            pos += 2;
        }
        xs.push(cur_x as f64);
    }
    let mut ys = Vec::with_capacity(num_points);
    let mut cur_y = 0i32;
    for &f in &flags {
        if (f & 0x04) != 0 {
            if pos >= slice.len() {
                return None;
            }
            let dy = slice[pos] as i32;
            pos += 1;
            cur_y += if (f & 0x20) != 0 { dy } else { -dy };
        } else if (f & 0x20) == 0 {
            if pos + 2 > slice.len() {
                return None;
            }
            cur_y += i16::from_be_bytes([slice[pos], slice[pos + 1]]) as i32;
            pos += 2;
        }
        ys.push(cur_y as f64);
    }
    let mut cmds = Vec::new();
    let mut start = 0usize;
    for &end in &end_pts {
        if end < start || end >= num_points {
            break;
        }
        let len = end - start + 1;
        if len >= 2 {
            let x0 = xs[start];
            let y0 = ys[start];
            cmds.push(crate::PathCommand::MoveTo { x: x0, y: y0 });
            let mut i = 1usize;
            while i < len {
                let idx = start + i;
                let on_curve = (flags[idx] & 0x01) != 0;
                if on_curve {
                    cmds.push(crate::PathCommand::LineTo {
                        x: xs[idx],
                        y: ys[idx],
                    });
                    i += 1;
                } else {
                    let next_idx = start + ((i + 1) % len);
                    let next_on = (flags[next_idx] & 0x01) != 0;
                    if next_on {
                        cmds.push(crate::PathCommand::QuadraticTo {
                            cx: xs[idx],
                            cy: ys[idx],
                            x: xs[next_idx],
                            y: ys[next_idx],
                        });
                        i += 2;
                    } else {
                        let mx = (xs[idx] + xs[next_idx]) * 0.5;
                        let my = (ys[idx] + ys[next_idx]) * 0.5;
                        cmds.push(crate::PathCommand::QuadraticTo {
                            cx: xs[idx],
                            cy: ys[idx],
                            x: mx,
                            y: my,
                        });
                        i += 1;
                    }
                }
            }
            cmds.push(crate::PathCommand::Close);
        }
        start = end + 1;
    }
    (!cmds.is_empty()).then_some(cmds)
}

fn synthesize_glyph_outline(
    ch: char,
    x: f64,
    y: f64,
    size: f64,
    advance: u16,
    units_per_em: u16,
) -> Vec<crate::PathCommand> {
    if ch.is_whitespace() {
        return Vec::new();
    }
    let upm = f64::from(units_per_em.max(1));
    let w = (f64::from(advance) / upm * size).max(size * 0.25);
    let h = size.max(1.0);
    let left = x + w * 0.1;
    let right = x + w * 0.9;
    let top = y + h * 0.15;
    let bottom = y + h * 0.88;
    let mid_x = (left + right) * 0.5;
    let mid_y = (top + bottom) * 0.5;
    let stroke = (h * 0.12).max(0.8);
    let mut cmds = Vec::new();
    let add_rect = |cmds: &mut Vec<crate::PathCommand>, rx: f64, ry: f64, rw: f64, rh: f64| {
        if rw <= 0.0 || rh <= 0.0 {
            return;
        }
        cmds.push(crate::PathCommand::MoveTo { x: rx, y: ry });
        cmds.push(crate::PathCommand::LineTo { x: rx + rw, y: ry });
        cmds.push(crate::PathCommand::LineTo {
            x: rx + rw,
            y: ry + rh,
        });
        cmds.push(crate::PathCommand::LineTo { x: rx, y: ry + rh });
        cmds.push(crate::PathCommand::Close);
    };
    match ch {
        '.' | ',' => add_rect(
            &mut cmds,
            mid_x - stroke * 0.6,
            bottom - stroke * 1.2,
            stroke * 1.2,
            stroke * 1.2,
        ),
        '-' | '_' => {
            let bar_y = if ch == '_' {
                bottom - stroke
            } else {
                mid_y - stroke * 0.5
            };
            add_rect(&mut cmds, left, bar_y, (right - left).max(stroke), stroke);
        }
        'I' | 'i' | 'l' | '|' | '!' => {
            add_rect(
                &mut cmds,
                mid_x - stroke * 0.5,
                top,
                stroke,
                (bottom - top).max(stroke),
            );
        }
        'L' => {
            add_rect(&mut cmds, left, top, stroke, (bottom - top).max(stroke));
            add_rect(
                &mut cmds,
                left,
                bottom - stroke,
                (right - left).max(stroke),
                stroke,
            );
        }
        'T' => {
            add_rect(&mut cmds, left, top, (right - left).max(stroke), stroke);
            add_rect(
                &mut cmds,
                mid_x - stroke * 0.5,
                top,
                stroke,
                (bottom - top).max(stroke),
            );
        }
        'H' => {
            add_rect(&mut cmds, left, top, stroke, (bottom - top).max(stroke));
            add_rect(
                &mut cmds,
                right - stroke,
                top,
                stroke,
                (bottom - top).max(stroke),
            );
            add_rect(
                &mut cmds,
                left,
                mid_y - stroke * 0.5,
                (right - left).max(stroke),
                stroke,
            );
        }
        'E' | 'F' => {
            add_rect(&mut cmds, left, top, stroke, (bottom - top).max(stroke));
            add_rect(&mut cmds, left, top, (right - left).max(stroke), stroke);
            add_rect(
                &mut cmds,
                left,
                mid_y - stroke * 0.5,
                (right - left) * 0.8,
                stroke,
            );
            if ch == 'E' {
                add_rect(
                    &mut cmds,
                    left,
                    bottom - stroke,
                    (right - left).max(stroke),
                    stroke,
                );
            }
        }
        'O' | '0' | 'o' => {
            let t = if ch == 'o' {
                mid_y - (bottom - top) * 0.25
            } else {
                top
            };
            cmds.push(crate::PathCommand::MoveTo { x: mid_x, y: t });
            cmds.push(crate::PathCommand::QuadraticTo {
                cx: right,
                cy: t,
                x: right,
                y: (t + bottom) * 0.5,
            });
            cmds.push(crate::PathCommand::QuadraticTo {
                cx: right,
                cy: bottom,
                x: mid_x,
                y: bottom,
            });
            cmds.push(crate::PathCommand::QuadraticTo {
                cx: left,
                cy: bottom,
                x: left,
                y: (t + bottom) * 0.5,
            });
            cmds.push(crate::PathCommand::QuadraticTo {
                cx: left,
                cy: t,
                x: mid_x,
                y: t,
            });
            cmds.push(crate::PathCommand::Close);
        }
        _ => {
            let is_lower = ch.is_lowercase();
            let g_top = if is_lower {
                top + (bottom - top) * 0.22
            } else {
                top
            };
            add_rect(&mut cmds, left, g_top, (right - left).max(stroke), stroke);
            add_rect(&mut cmds, left, g_top, stroke, (bottom - g_top).max(stroke));
            add_rect(
                &mut cmds,
                right - stroke,
                g_top,
                stroke,
                (bottom - g_top).max(stroke),
            );
            if (ch as u32).is_multiple_of(2) {
                add_rect(
                    &mut cmds,
                    left,
                    bottom - stroke,
                    (right - left).max(stroke),
                    stroke,
                );
            } else {
                add_rect(
                    &mut cmds,
                    left,
                    (g_top + bottom) * 0.5,
                    (right - left).max(stroke),
                    stroke,
                );
            }
        }
    }
    cmds
}

impl TextFormat {
    pub fn portable_measure_strings(
        strings: &[String],
        format: &TextFormat,
        font_bytes: Option<&[u8]>,
    ) -> Vec<f64> {
        let metrics = font_bytes
            .and_then(|bytes| PortableFontMetrics::from_bytes(&format.font, bytes).ok())
            .unwrap_or_else(|| PortableFontMetrics::builtin(&format.font));
        metrics.measure_strings(strings, format)
    }

    pub fn portable_font_metrics_json(
        family: &str,
        font_bytes: Option<&[u8]>,
    ) -> Result<String, TextError> {
        let metrics = match font_bytes {
            Some(bytes) => PortableFontMetrics::from_bytes(family, bytes)?,
            None => PortableFontMetrics::builtin(family),
        };
        serde_json::to_string(&metrics).map_err(|e| TextError(e.to_string()))
    }

    pub fn subset_font_bytes(
        bytes: &[u8],
        codepoints: &std::collections::BTreeSet<u32>,
    ) -> Result<Vec<u8>, TextError> {
        let metrics = PortableFontMetrics::from_bytes("SubsetFont", bytes)?;
        Ok(metrics.subset(codepoints).encode_subset_binary())
    }

    pub fn subset_font_codepoints(bytes: &[u8]) -> Vec<u32> {
        PortableFontMetrics::from_bytes("SubsetFont", bytes)
            .map(|m| m.advances.keys().copied().collect())
            .unwrap_or_default()
    }

    pub fn glyph_outline_commands(
        ch: char,
        x: f64,
        y: f64,
        font_family: &str,
        font_size: f64,
        font_bytes: Option<&[u8]>,
    ) -> Vec<crate::PathCommand> {
        let metrics = font_bytes
            .and_then(|b| PortableFontMetrics::from_bytes(font_family, b).ok())
            .unwrap_or_else(|| PortableFontMetrics::builtin(font_family));
        metrics.glyph_outline(ch, x, y, font_size)
    }
}

impl TextFieldState {
    pub fn layout_portable(&self, font_bytes: Option<&[u8]>) -> Result<TextLayout, TextError> {
        layout_text(self, |strings, fmt| {
            Ok(TextFormat::portable_measure_strings(
                strings, fmt, font_bytes,
            ))
        })
    }

    pub fn to_outline_commands(&self, font_bytes: Option<&[u8]>) -> Vec<crate::PathCommand> {
        let Ok(boxes) = layout_char_boxes(self, |strings, fmt| {
            Ok(TextFormat::portable_measure_strings(
                strings, fmt, font_bytes,
            ))
        }) else {
            return Vec::new();
        };
        let metrics = font_bytes
            .and_then(|b| PortableFontMetrics::from_bytes(&self.format.font, b).ok())
            .unwrap_or_else(|| PortableFontMetrics::builtin(&self.format.font));
        let mut out = Vec::new();
        for (ch, char_box) in boxes {
            out.extend(metrics.glyph_outline(ch, char_box.x, char_box.y, self.format.size));
        }
        out
    }

    pub fn static_text_to_outline_commands(
        text: &str,
        font_family: &str,
        font_size: f64,
        font_bytes: Option<&[u8]>,
    ) -> Vec<crate::PathCommand> {
        let metrics = font_bytes
            .and_then(|b| PortableFontMetrics::from_bytes(font_family, b).ok())
            .unwrap_or_else(|| PortableFontMetrics::builtin(font_family));
        let upm = f64::from(metrics.units_per_em.max(1));
        let scale = font_size.max(1.0) / upm;
        let line_step = font_size.max(1.0) * 1.2;
        let mut out = Vec::new();
        for (line_idx, line) in text.split('\n').enumerate() {
            let mut cursor_x = 0.0;
            let line_y = (line_idx as f64) * line_step;
            for ch in line.chars() {
                if ch == '\r' {
                    continue;
                }
                out.extend(metrics.glyph_outline(ch, cursor_x, line_y, font_size));
                cursor_x += f64::from(metrics.advance_for_char(ch)) * scale;
            }
        }
        out
    }

    pub fn apply_auto_size(
        &mut self,
        mode: &str,
        x: &mut f64,
        layout: &TextLayout,
    ) -> Result<bool, TextError> {
        let normalized = match mode.trim().to_ascii_lowercase().as_str() {
            "left" | "true" => "left",
            "center" => "center",
            "right" => "right",
            _ => "none",
        };
        self.as2_viewport.auto_size = normalized.to_string();
        if normalized == "none" {
            return Ok(false);
        }
        let old = self.box_size.unwrap_or(TextBoxSize {
            width: 100.0,
            height: 20.0,
        });
        let new_width = if self.word_wrap {
            old.width
        } else {
            (layout.text_width
                + self.format.left_margin
                + self.format.right_margin
                + self.format.block_indent
                + self.format.indent.max(0.0)
                + if self.format.bullet {
                    self.format.size
                } else {
                    0.0
                }
                + 4.0)
                .max(4.0)
        };
        let new_height = (layout.text_height + 4.0).max(4.0);
        if (new_width - old.width).abs() < 1e-9 && (new_height - old.height).abs() < 1e-9 {
            return Ok(false);
        }
        let delta = (old.width - new_width)
            * match normalized {
                "center" => 0.5,
                "right" => 1.0,
                _ => 0.0,
            };
        *x += delta;
        self.box_size = Some(TextBoxSize {
            width: new_width,
            height: new_height,
        });
        self.revision = self
            .revision
            .parse::<u64>()
            .unwrap_or(0)
            .saturating_add(1)
            .to_string();
        Ok(true)
    }

    pub fn parse_html(
        source: &str,
        base_format: &TextFormat,
        css_styles: Option<
            &std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
        >,
        condense_white: bool,
    ) -> Result<Self, TextError> {
        let mut field = Self {
            format: base_format.clone(),
            default_format: base_format.clone(),
            ..Self::default()
        };
        field.set_html_text(source, css_styles, condense_white)?;
        Ok(field)
    }

    pub fn set_html_text(
        &mut self,
        source: &str,
        css_styles: Option<
            &std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
        >,
        condense_white: bool,
    ) -> Result<(), TextError> {
        if source.len() > MAX_TEXT_BYTES {
            return Err(TextError("HTML text exceeds maximum byte budget".into()));
        }
        let mut stack: Vec<(String, TextFormat)> =
            vec![("root".into(), self.default_format.clone())];
        let mut plain = String::new();
        let mut char_formats: Vec<TextFormat> = Vec::new();
        let mut chars = source.chars().peekable();
        let mut last_was_space = false;

        let apply_css_props =
            |fmt: &mut TextFormat, props: &std::collections::BTreeMap<String, String>| {
                for (k, v) in props {
                    let key = k.trim().to_ascii_lowercase();
                    let val = v.trim();
                    match key.as_str() {
                        "font-family" | "fontfamily" => {
                            let family = val
                                .split(',')
                                .next()
                                .unwrap_or(val)
                                .trim()
                                .trim_matches(['"', '\'']);
                            if !family.is_empty() {
                                fmt.font = family.to_string();
                            }
                        }
                        "font-size" | "fontsize" => {
                            let num = val.trim_end_matches("px").trim_end_matches("pt").trim();
                            if let Ok(sz) = num.parse::<f64>()
                                && sz.is_finite()
                            {
                                fmt.size = sz.clamp(1.0, 127.0);
                            }
                        }
                        "color" => {
                            if let Some(hex) = val.strip_prefix('#')
                                && let Ok(c) = u32::from_str_radix(hex, 16)
                            {
                                fmt.color = c & 0xFFFFFF;
                            }
                        }
                        "font-weight" | "fontweight" => {
                            fmt.bold = val.eq_ignore_ascii_case("bold")
                                || val == "700"
                                || val == "800"
                                || val == "900";
                        }
                        "font-style" | "fontstyle" => {
                            fmt.italic = val.eq_ignore_ascii_case("italic")
                                || val.eq_ignore_ascii_case("oblique");
                        }
                        "text-decoration" | "textdecoration" => {
                            fmt.underline = val.to_ascii_lowercase().contains("underline");
                        }
                        "text-align" | "textalign" => {
                            let lower = val.to_ascii_lowercase();
                            if matches!(lower.as_str(), "left" | "center" | "right" | "justify") {
                                fmt.set_align_mode(Some(lower.clone()));
                                fmt.align = match lower.as_str() {
                                    "center" => TextAlign::Center,
                                    "right" => TextAlign::Right,
                                    _ => TextAlign::Left,
                                };
                            }
                        }
                        "letter-spacing" | "letterspacing" => {
                            let num = val.trim_end_matches("px").trim();
                            if let Ok(ls) = num.parse::<f64>()
                                && ls.is_finite()
                            {
                                fmt.letter_spacing = ls.clamp(-60.0, 60.0);
                            }
                        }
                        _ => {}
                    }
                }
            };

        while let Some(ch) = chars.next() {
            if ch == '<' {
                let mut tag_buf = String::new();
                for tc in chars.by_ref() {
                    if tc == '>' {
                        break;
                    }
                    tag_buf.push(tc);
                }
                let trimmed = tag_buf.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Some(closing) = trimmed.strip_prefix('/') {
                    let close_name = closing.trim().to_ascii_lowercase();
                    if matches!(close_name.as_str(), "p" | "li" | "div")
                        && !plain.is_empty()
                        && !plain.ends_with('\n')
                    {
                        plain.push('\n');
                        let cur_fmt = stack
                            .last()
                            .map(|(_, f)| f.clone())
                            .unwrap_or_else(|| self.default_format.clone());
                        for _ in 0..'\n'.len_utf16() {
                            char_formats.push(cur_fmt.clone());
                        }
                        last_was_space = false;
                    }
                    if stack.len() > 1 {
                        if let Some(pos) = stack.iter().rposition(|(t, _)| t == &close_name) {
                            stack.truncate(pos);
                        } else {
                            stack.pop();
                        }
                    }
                    continue;
                }
                let self_closing = trimmed.ends_with('/');
                let content = trimmed.trim_end_matches('/').trim();
                let mut parts = content.splitn(2, char::is_whitespace);
                let tag_name = parts.next().unwrap_or("").to_ascii_lowercase();
                let attr_str = parts.next().unwrap_or("");
                let attrs = parse_html_attributes(attr_str);

                if tag_name == "br" {
                    plain.push('\n');
                    let cur_fmt = stack
                        .last()
                        .map(|(_, f)| f.clone())
                        .unwrap_or_else(|| self.default_format.clone());
                    char_formats.push(cur_fmt);
                    last_was_space = false;
                    continue;
                }
                if matches!(tag_name.as_str(), "p" | "li" | "div")
                    && !plain.is_empty()
                    && !plain.ends_with('\n')
                {
                    plain.push('\n');
                    let cur_fmt = stack
                        .last()
                        .map(|(_, f)| f.clone())
                        .unwrap_or_else(|| self.default_format.clone());
                    char_formats.push(cur_fmt);
                    last_was_space = false;
                }

                let mut next_fmt = stack
                    .last()
                    .map(|(_, f)| f.clone())
                    .unwrap_or_else(|| self.default_format.clone());
                if let Some(styles) = css_styles {
                    if let Some(tag_style) = styles.get(&tag_name) {
                        apply_css_props(&mut next_fmt, tag_style);
                    }
                    if let Some(cls) = attrs.get("class") {
                        for item in cls.split_whitespace() {
                            let dot_cls = format!(".{item}");
                            if let Some(cls_style) =
                                styles.get(&dot_cls).or_else(|| styles.get(item))
                            {
                                apply_css_props(&mut next_fmt, cls_style);
                            }
                        }
                    }
                }
                if let Some(inline_style) = attrs.get("style") {
                    let mut inline_map = std::collections::BTreeMap::new();
                    for decl in inline_style.split(';') {
                        if let Some((k, v)) = decl.split_once(':') {
                            inline_map.insert(k.trim().to_string(), v.trim().to_string());
                        }
                    }
                    apply_css_props(&mut next_fmt, &inline_map);
                }

                match tag_name.as_str() {
                    "b" | "strong" => next_fmt.bold = true,
                    "i" | "em" => next_fmt.italic = true,
                    "u" => next_fmt.underline = true,
                    "li" => next_fmt.bullet = true,
                    "p" | "div" => {
                        if let Some(align) = attrs.get("align") {
                            let lower = align.trim().to_ascii_lowercase();
                            if matches!(lower.as_str(), "left" | "center" | "right" | "justify") {
                                next_fmt.set_align_mode(Some(lower.clone()));
                                next_fmt.align = match lower.as_str() {
                                    "center" => TextAlign::Center,
                                    "right" => TextAlign::Right,
                                    _ => TextAlign::Left,
                                };
                            }
                        }
                    }
                    "font" => {
                        if let Some(face) = attrs.get("face")
                            && !face.trim().is_empty()
                        {
                            next_fmt.font = face.trim().to_string();
                        }
                        if let Some(size_str) = attrs.get("size") {
                            let s = size_str.trim();
                            if let Some(rel) = s.strip_prefix('+') {
                                if let Ok(d) = rel.parse::<f64>() {
                                    next_fmt.size = (next_fmt.size + d).clamp(1.0, 127.0);
                                }
                            } else if let Some(rel) = s.strip_prefix('-') {
                                if let Ok(d) = rel.parse::<f64>() {
                                    next_fmt.size = (next_fmt.size - d).clamp(1.0, 127.0);
                                }
                            } else if let Ok(abs) = s.parse::<f64>() {
                                next_fmt.size = abs.clamp(1.0, 127.0);
                            }
                        }
                        if let Some(col_str) = attrs.get("color")
                            && let Some(hex) = col_str.trim().strip_prefix('#')
                            && let Ok(c) = u32::from_str_radix(hex, 16)
                        {
                            next_fmt.color = c & 0xFFFFFF;
                        }
                        if let Some(ls_str) = attrs.get("letterspacing")
                            && let Ok(ls) = ls_str.trim().parse::<f64>()
                            && ls.is_finite()
                        {
                            next_fmt.letter_spacing = ls.clamp(-60.0, 60.0);
                        }
                    }
                    "a" => {
                        if let Some(href) = attrs.get("href") {
                            next_fmt.url = href.clone();
                        }
                        if let Some(target) = attrs.get("target") {
                            next_fmt.set_target_str(target.clone());
                        }
                    }
                    _ => {}
                }
                if !self_closing {
                    stack.push((tag_name, next_fmt));
                }
            } else if ch == '&' {
                let mut ent = String::new();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc == ';' {
                        break;
                    }
                    ent.push(nc);
                    if ent.len() > 12 {
                        break;
                    }
                }
                let decoded = match ent.as_str() {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" | "#39" => '\'',
                    "nbsp" => '\u{00A0}',
                    _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                        u32::from_str_radix(&ent[2..], 16)
                            .ok()
                            .and_then(char::from_u32)
                            .unwrap_or('?')
                    }
                    _ if ent.starts_with('#') => ent[1..]
                        .parse::<u32>()
                        .ok()
                        .and_then(char::from_u32)
                        .unwrap_or('?'),
                    _ => '?',
                };
                let cur_fmt = stack
                    .last()
                    .map(|(_, f)| f.clone())
                    .unwrap_or_else(|| self.default_format.clone());
                plain.push(decoded);
                for _ in 0..decoded.len_utf16() {
                    char_formats.push(cur_fmt.clone());
                }
                last_was_space = false;
            } else {
                if condense_white && ch.is_whitespace() {
                    if last_was_space {
                        continue;
                    }
                    last_was_space = true;
                    let cur_fmt = stack
                        .last()
                        .map(|(_, f)| f.clone())
                        .unwrap_or_else(|| self.default_format.clone());
                    plain.push(' ');
                    char_formats.push(cur_fmt);
                    continue;
                }
                last_was_space = false;
                let cur_fmt = stack
                    .last()
                    .map(|(_, f)| f.clone())
                    .unwrap_or_else(|| self.default_format.clone());
                plain.push(ch);
                for _ in 0..ch.len_utf16() {
                    char_formats.push(cur_fmt.clone());
                }
            }
        }

        if plain.ends_with('\n') && !source.contains("<br") {
            plain.pop();
            char_formats.pop();
        }

        self.text = plain;
        self.format_runs.clear();
        if !char_formats.is_empty() {
            self.format = char_formats[0].clone();
            let mut run_start = 0u32;
            let mut run_fmt = char_formats[0].clone();
            for (idx, fmt) in char_formats.iter().enumerate().skip(1) {
                if *fmt != run_fmt {
                    self.format_runs.push(TextFormatRun {
                        start: run_start,
                        end: idx as u32,
                        format: run_fmt,
                    });
                    run_start = idx as u32;
                    run_fmt = fmt.clone();
                }
            }
            self.format_runs.push(TextFormatRun {
                start: run_start,
                end: char_formats.len() as u32,
                format: run_fmt,
            });
            if self.format_runs.len() == 1 {
                self.format_runs.clear();
            }
        }
        self.selection_begin = clamp_utf16(&self.text, i64::from(self.selection_begin));
        self.selection_end = clamp_utf16(&self.text, i64::from(self.selection_end));
        self.caret = clamp_utf16(&self.text, i64::from(self.caret));
        self.revision = self
            .revision
            .parse::<u64>()
            .unwrap_or(0)
            .saturating_add(1)
            .to_string();
        Ok(())
    }

    pub fn to_html_text(&self) -> String {
        let escape = |s: &str| {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
        };
        let wrap_span = |text: &str, fmt: &TextFormat| {
            if text.is_empty() {
                return String::new();
            }
            let mut inner = escape(text);
            if fmt.underline {
                inner = format!("<u>{inner}</u>");
            }
            if fmt.italic {
                inner = format!("<i>{inner}</i>");
            }
            if fmt.bold {
                inner = format!("<b>{inner}</b>");
            }
            if !fmt.url.is_empty() {
                let tgt = fmt.target_str();
                let target_attr = if tgt.is_empty() {
                    String::new()
                } else {
                    format!(" target=\"{}\"", escape(tgt))
                };
                inner = format!("<a href=\"{}\"{target_attr}>{inner}</a>", escape(&fmt.url));
            }
            format!(
                "<font face=\"{}\" size=\"{}\" color=\"#{:06x}\">{inner}</font>",
                escape(&fmt.font),
                fmt.size.round() as i32,
                fmt.color & 0xFFFFFF
            )
        };
        let align = self.format.effective_align_mode();
        if self.format_runs.is_empty() {
            let body = self
                .text
                .split('\n')
                .map(|line| format!("<p align=\"{align}\">{}</p>", wrap_span(line, &self.format)))
                .collect::<Vec<_>>()
                .join("");
            return body;
        }
        let mut out = format!("<p align=\"{align}\">");
        for run in &self.format_runs {
            let Ok(sb) = utf16_byte(&self.text, run.start) else {
                continue;
            };
            let Ok(eb) = utf16_byte(&self.text, run.end) else {
                continue;
            };
            let slice = &self.text[sb..eb];
            let mut parts = slice.split('\n').peekable();
            while let Some(part) = parts.next() {
                out.push_str(&wrap_span(part, &run.format));
                if parts.peek().is_some() {
                    let next_align = run.format.effective_align_mode();
                    out.push_str(&format!("</p><p align=\"{next_align}\">"));
                }
            }
        }
        out.push_str("</p>");
        out
    }
}

fn parse_html_attributes(input: &str) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    let mut chars = input.chars().peekable();
    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' || c.is_whitespace() {
                break;
            }
            key.push(c);
            chars.next();
        }
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else {
                break;
            }
        }
        if chars.peek() != Some(&'=') {
            if !key.is_empty() {
                map.insert(key.to_ascii_lowercase(), String::new());
            }
            continue;
        }
        chars.next(); // consume '='
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else {
                break;
            }
        }
        let mut val = String::new();
        if let Some(&quote @ ('"' | '\'')) = chars.peek() {
            chars.next();
            for c in chars.by_ref() {
                if c == quote {
                    break;
                }
                val.push(c);
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                val.push(c);
                chars.next();
            }
        }
        if !key.is_empty() {
            map.insert(key.to_ascii_lowercase(), val);
        }
    }
    map
}
