//! `[T-501]` Integration tests for Complex OpenType `GSUB`/`GPOS` shaping (`harfrust`) and
//! Unicode Bidirectional (`unicode-bidi`) text runs, outline extraction, and font subsetting.

use neoflash_core::{
    AuthoredTextFieldOptions, Element, ElementKind, PathCommand, Project, ResourceUnit,
    TextFieldState, TextFormat, sfnt_checksum,
};
use neoflash_export::{
    FontExportPolicy, apply_font_policy, extract_text_field_outline_commands_with_diagnostics,
    extract_text_outline_commands_with_diagnostics, measure_opentype_text, outline_glyph,
    shape_opentype_line, subset_ttf_font_bytes, subset_ttf_font_bytes_with_texts,
};
use std::collections::{BTreeMap, BTreeSet};

const BUNDLED_INTER: &[u8] = include_bytes!("../../neoflash-ui/assets/fonts/Inter-Regular.ttf");

// Fixed glyph IDs in our deterministic programmatic OpenType test font:
const GID_NOTDEF: u16 = 0;
const GID_SPACE: u16 = 1;
const GID_A_CAP: u16 = 2;
const GID_V_CAP: u16 = 3;
const GID_T_CAP: u16 = 4;
const GID_O_LOW: u16 = 5;
const GID_F_LOW: u16 = 6;
const GID_I_LOW: u16 = 7; // 2 contours (stem + dot)
const GID_A_LOW: u16 = 8;
const GID_ACUTE_MARK: u16 = 9; // U+0301 combining acute accent (advance = 0, GDEF class = 3 Mark)
const GID_FI_LIG: u16 = 10; // GSUB `liga` target for `'f' + 'i'` (1 merged contour, GDEF class = 2 Ligature)
const GID_HEB_ALEF: u16 = 11; // U+05D0 'א'
const GID_HEB_BET: u16 = 12; // U+05D1 'ב'
const GID_HEB_GIMEL: u16 = 13; // U+05D2 'ג'
const GID_ARAB_BEH: u16 = 14; // U+0628 'ب' isolated
const GID_ARAB_BEH_INIT: u16 = 15; // GSUB `init` target (no cmap entry!)
const GID_ARAB_BEH_MEDI: u16 = 16; // GSUB `medi` target (no cmap entry!)
const GID_ARAB_BEH_FINA: u16 = 17; // GSUB `fina` target (no cmap entry!)
const GID_B_CAP: u16 = 18;
const GID_C_CAP: u16 = 19;
const GID_D_CAP: u16 = 20;
const FIRST_FILLER_GID: u16 = 21;
const NUM_GLYPHS: u16 = 48;

fn push_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_be_bytes());
}

fn push_i16(buf: &mut Vec<u8>, v: i16) {
    buf.extend_from_slice(&v.to_be_bytes());
}

fn push_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_be_bytes());
}

/// Encodes a simple TrueType `glyf` contour list of axis-aligned rectangles `[(x0, y0, x1, y1)]`.
fn encode_rect_contours_glyph(rects: &[(i16, i16, i16, i16)]) -> Vec<u8> {
    if rects.is_empty() {
        return Vec::new();
    }
    let x_min = rects.iter().map(|r| r.0).min().unwrap_or(0);
    let y_min = rects.iter().map(|r| r.1).min().unwrap_or(0);
    let x_max = rects.iter().map(|r| r.2).max().unwrap_or(0);
    let y_max = rects.iter().map(|r| r.3).max().unwrap_or(0);

    let mut out = Vec::new();
    push_i16(&mut out, rects.len() as i16); // numberOfContours
    push_i16(&mut out, x_min);
    push_i16(&mut out, y_min);
    push_i16(&mut out, x_max);
    push_i16(&mut out, y_max);

    // endPtsOfContours: 4 points per rectangle
    for c in 0..rects.len() {
        push_u16(&mut out, (c as u16) * 4 + 3);
    }
    push_u16(&mut out, 0); // instructionLength = 0

    let total_pts = rects.len() * 4;
    // Flags: 0x01 = ON_CURVE_POINT (16-bit signed delta x & y follow)
    out.resize(out.len() + total_pts, 0x01);

    let mut pts = Vec::with_capacity(total_pts);
    for &(x0, y0, x1, y1) in rects {
        pts.push((x0, y0));
        pts.push((x0, y1));
        pts.push((x1, y1));
        pts.push((x1, y0));
    }

    let mut prev_x = 0i16;
    for &(x, _) in &pts {
        push_i16(&mut out, x.wrapping_sub(prev_x));
        prev_x = x;
    }
    let mut prev_y = 0i16;
    for &(_, y) in &pts {
        push_i16(&mut out, y.wrapping_sub(prev_y));
        prev_y = y;
    }
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    out
}

fn build_cmap_format4(mappings: &BTreeMap<u16, u16>) -> Vec<u8> {
    let mut segments: Vec<(u16, u16, u16)> = Vec::new();
    for (&cp, &gid) in mappings {
        let delta = gid.wrapping_sub(cp);
        if let Some((_s, end, prev_delta)) = segments.last_mut()
            && cp == *end + 1
            && delta == *prev_delta
        {
            *end = cp;
        } else {
            segments.push((cp, cp, delta));
        }
    }
    segments.push((0xFFFF, 0xFFFF, 1));
    let seg_count = segments.len() as u16;
    let seg_count_x2 = seg_count * 2;
    let entry_selector = 15u16.saturating_sub(seg_count.leading_zeros() as u16);
    let search_range = 2 * (1u16 << entry_selector);
    let range_shift = seg_count_x2 - search_range;
    let f4_len = 16 + (seg_count as usize) * 8;

    let mut f4 = Vec::with_capacity(f4_len);
    push_u16(&mut f4, 4);
    push_u16(&mut f4, f4_len as u16);
    push_u16(&mut f4, 0);
    push_u16(&mut f4, seg_count_x2);
    push_u16(&mut f4, search_range);
    push_u16(&mut f4, entry_selector);
    push_u16(&mut f4, range_shift);
    for &(_, end, _) in &segments {
        push_u16(&mut f4, end);
    }
    push_u16(&mut f4, 0);
    for &(start, _, _) in &segments {
        push_u16(&mut f4, start);
    }
    for &(_, _, delta) in &segments {
        push_u16(&mut f4, delta);
    }
    for _ in &segments {
        push_u16(&mut f4, 0);
    }

    let mut cmap = Vec::new();
    push_u16(&mut cmap, 0); // version
    push_u16(&mut cmap, 2); // numTables
    // (0, 3) Unicode BMP & (3, 1) Windows Unicode BMP
    push_u16(&mut cmap, 0);
    push_u16(&mut cmap, 3);
    push_u32(&mut cmap, 20);
    push_u16(&mut cmap, 3);
    push_u16(&mut cmap, 1);
    push_u32(&mut cmap, 20);
    cmap.extend_from_slice(&f4);
    cmap
}

fn build_gdef_table() -> Vec<u8> {
    // GDEF v1.0 with GlyphClassDef Format 2:
    //   1 = Base (2..=8, 11..=47)
    //   2 = Ligature (10 = fi)
    //   3 = Mark (9 = U+0301 combining acute)
    let mut gdef = Vec::new();
    push_u16(&mut gdef, 1); // majorVersion
    push_u16(&mut gdef, 0); // minorVersion
    push_u16(&mut gdef, 12); // glyphClassDefOffset
    push_u16(&mut gdef, 0); // attachListOffset
    push_u16(&mut gdef, 0); // ligCaretListOffset
    push_u16(&mut gdef, 0); // markAttachClassDefOffset

    // ClassDefFormat2 at offset 12
    push_u16(&mut gdef, 2); // classFormat = 2
    push_u16(&mut gdef, 4); // classRangeCount = 4
    // Range 0: 2..=8 -> Base (1)
    push_u16(&mut gdef, 2);
    push_u16(&mut gdef, 8);
    push_u16(&mut gdef, 1);
    // Range 1: 9..=9 -> Mark (3)
    push_u16(&mut gdef, GID_ACUTE_MARK);
    push_u16(&mut gdef, GID_ACUTE_MARK);
    push_u16(&mut gdef, 3);
    // Range 2: 10..=10 -> Ligature (2)
    push_u16(&mut gdef, GID_FI_LIG);
    push_u16(&mut gdef, GID_FI_LIG);
    push_u16(&mut gdef, 2);
    // Range 3: 11..=(NUM_GLYPHS - 1) -> Base (1)
    push_u16(&mut gdef, 11);
    push_u16(&mut gdef, NUM_GLYPHS - 1);
    push_u16(&mut gdef, 1);
    gdef
}

fn encode_single_subst_f2(from_gid: u16, to_gid: u16) -> Vec<u8> {
    // LookupTable (8 bytes) + SingleSubstFormat2 (8 bytes) + CoverageFormat1 (6 bytes) = 22 bytes
    let mut lk = Vec::new();
    push_u16(&mut lk, 1); // lookupType = 1 (SingleSubst)
    push_u16(&mut lk, 0); // lookupFlag = 0
    push_u16(&mut lk, 1); // subTableCount = 1
    push_u16(&mut lk, 8); // subtableOffsets[0] = 8

    // SingleSubstFormat2 at +8:
    push_u16(&mut lk, 2); // substFormat = 2
    push_u16(&mut lk, 8); // coverageOffset = 8 (relative to SingleSubstFormat2)
    push_u16(&mut lk, 1); // glyphCount = 1
    push_u16(&mut lk, to_gid); // substituteGlyphIDs[0]

    // CoverageFormat1 at +16 (relative to subtable = +8):
    push_u16(&mut lk, 1); // coverageFormat = 1
    push_u16(&mut lk, 1); // glyphCount = 1
    push_u16(&mut lk, from_gid);
    lk
}

fn encode_fi_ligature_lookup() -> Vec<u8> {
    // LookupType 4 (`LigatureSubstFormat1`): `'f' (6) + 'i' (7) -> GID_FI_LIG (10)`
    let mut lk = Vec::new();
    push_u16(&mut lk, 4); // lookupType = 4 (LigatureSubst)
    push_u16(&mut lk, 0); // lookupFlag = 0
    push_u16(&mut lk, 1); // subTableCount = 1
    push_u16(&mut lk, 8); // subtableOffsets[0] = 8

    // LigatureSubstFormat1 at +6 (relative = 0):
    //   substFormat(2) + coverageOffset(2) + ligatureSetCount(2) + ligatureSetOffsets[0](2) = 8 bytes
    push_u16(&mut lk, 1); // substFormat = 1
    push_u16(&mut lk, 18); // coverageOffset = 18 (after LigatureSet(4) + Ligature(6))
    push_u16(&mut lk, 1); // ligatureSetCount = 1
    push_u16(&mut lk, 8); // ligatureSetOffsets[0] = 8

    // LigatureSet at relative +8:
    push_u16(&mut lk, 1); // ligatureCount = 1
    push_u16(&mut lk, 4); // ligatureOffsets[0] = 4 (relative to LigatureSet -> +12)

    // Ligature at relative +12 (6 bytes):
    push_u16(&mut lk, GID_FI_LIG); // ligatureGlyph
    push_u16(&mut lk, 2); // componentCount = 2
    push_u16(&mut lk, GID_I_LOW); // componentGlyphIDs[0]

    // CoverageFormat1 at relative +18:
    push_u16(&mut lk, 1); // coverageFormat = 1
    push_u16(&mut lk, 1); // glyphCount = 1
    push_u16(&mut lk, GID_F_LOW);
    lk
}

fn build_gsub_table() -> Vec<u8> {
    // Features sorted alphabetically:
    //   0: `fina` -> Lookup 3
    //   1: `init` -> Lookup 1
    //   2: `liga` -> Lookup 0
    //   3: `medi` -> Lookup 2
    // Scripts sorted alphabetically: `DFLT`, `arab`, `hebr`, `latn` (all enabling features 0..=3)
    let mut script_list = Vec::new();
    let scripts: [&[u8; 4]; 4] = [b"DFLT", b"arab", b"hebr", b"latn"];
    push_u16(&mut script_list, scripts.len() as u16);
    let shared_script_off = (2 + scripts.len() * 6) as u16;
    for tag in scripts {
        script_list.extend_from_slice(tag);
        push_u16(&mut script_list, shared_script_off);
    }
    // Shared ScriptTable + LangSysTable:
    push_u16(&mut script_list, 4); // defaultLangSysOffset = 4
    push_u16(&mut script_list, 0); // langSysCount = 0
    push_u16(&mut script_list, 0); // lookupOrderOffset = 0
    push_u16(&mut script_list, 0xFFFF); // requiredFeatureIndex
    push_u16(&mut script_list, 4); // featureIndexCount = 4
    for f_idx in 0..4u16 {
        push_u16(&mut script_list, f_idx);
    }

    let features: [(&[u8; 4], u16); 4] = [(b"fina", 3), (b"init", 1), (b"liga", 0), (b"medi", 2)];
    let mut feature_list = Vec::new();
    push_u16(&mut feature_list, features.len() as u16);
    let feat_tables_start = 2 + features.len() * 6;
    for (i, (tag, _)) in features.iter().enumerate() {
        feature_list.extend_from_slice(*tag);
        push_u16(&mut feature_list, (feat_tables_start + i * 6) as u16);
    }
    for &(_, lookup_idx) in &features {
        push_u16(&mut feature_list, 0); // featureParamsOffset
        push_u16(&mut feature_list, 1); // lookupIndexCount = 1
        push_u16(&mut feature_list, lookup_idx);
    }

    let lookups = [
        encode_fi_ligature_lookup(),
        encode_single_subst_f2(GID_ARAB_BEH, GID_ARAB_BEH_INIT),
        encode_single_subst_f2(GID_ARAB_BEH, GID_ARAB_BEH_MEDI),
        encode_single_subst_f2(GID_ARAB_BEH, GID_ARAB_BEH_FINA),
    ];
    let mut lookup_list = Vec::new();
    push_u16(&mut lookup_list, lookups.len() as u16);
    let mut running_off = 2 + lookups.len() * 2;
    for lk in &lookups {
        push_u16(&mut lookup_list, running_off as u16);
        running_off += lk.len();
    }
    for lk in &lookups {
        lookup_list.extend_from_slice(lk);
    }

    let script_off = 10u16;
    let feat_off = script_off + script_list.len() as u16;
    let lookup_off = feat_off + feature_list.len() as u16;
    let mut gsub = Vec::new();
    push_u16(&mut gsub, 1);
    push_u16(&mut gsub, 0);
    push_u16(&mut gsub, script_off);
    push_u16(&mut gsub, feat_off);
    push_u16(&mut gsub, lookup_off);
    gsub.extend_from_slice(&script_list);
    gsub.extend_from_slice(&feature_list);
    gsub.extend_from_slice(&lookup_list);
    gsub
}

fn encode_gpos_kern_pair_lookup() -> Vec<u8> {
    // LookupType 2 (`PairPosFormat1`):
    //   ('A' [2], 'V' [3]) -> valueRecord1 XAdvance = -180
    //   ('T' [4], 'o' [5]) -> valueRecord1 XAdvance = -140
    let mut sub = Vec::new();
    // PairPosFormat1 header (14 bytes for pairSetCount = 2):
    //   posFormat(2) + coverageOffset(2) + valueFormat1(2) + valueFormat2(2) + pairSetCount(2) + pairSetOffsets[2](4) = 14
    //   PairSet[0] (6 bytes) at +14
    //   PairSet[1] (6 bytes) at +20
    //   CoverageFormat1 (8 bytes) at +26
    push_u16(&mut sub, 1); // posFormat = 1
    push_u16(&mut sub, 26); // coverageOffset = 26
    push_u16(&mut sub, 0x0004); // valueFormat1 = X_ADVANCE
    push_u16(&mut sub, 0x0000); // valueFormat2 = 0
    push_u16(&mut sub, 2); // pairSetCount = 2
    push_u16(&mut sub, 14); // pairSetOffsets[0]
    push_u16(&mut sub, 20); // pairSetOffsets[1]

    // PairSet[0] for GID_A_CAP:
    push_u16(&mut sub, 1); // pairValueCount = 1
    push_u16(&mut sub, GID_V_CAP); // secondGlyph
    push_i16(&mut sub, -180); // xAdvance adjustment

    // PairSet[1] for GID_T_CAP:
    push_u16(&mut sub, 1); // pairValueCount = 1
    push_u16(&mut sub, GID_O_LOW); // secondGlyph
    push_i16(&mut sub, -140); // xAdvance adjustment

    // CoverageFormat1 at +26 (sorted glyph IDs: GID_A_CAP=2, GID_T_CAP=4):
    push_u16(&mut sub, 1); // coverageFormat = 1
    push_u16(&mut sub, 2); // glyphCount = 2
    push_u16(&mut sub, GID_A_CAP);
    push_u16(&mut sub, GID_T_CAP);

    let mut lk = Vec::new();
    push_u16(&mut lk, 2); // lookupType = 2 (PairPos)
    push_u16(&mut lk, 0); // lookupFlag = 0
    push_u16(&mut lk, 1); // subTableCount = 1
    push_u16(&mut lk, 8); // subtableOffsets[0] = 8
    lk.extend_from_slice(&sub);
    lk
}

fn encode_gpos_mark_to_base_lookup() -> Vec<u8> {
    // LookupType 4 (`MarkBasePosFormat1`):
    //   Mark glyph `GID_ACUTE_MARK` (9) anchor (100, 450) -> Base glyph `GID_A_LOW` (8) anchor (320, 660)
    // MarkBasePosFormat1 header = 12 bytes:
    //   +0: posFormat = 1
    //   +2: markCoverageOffset = 12 (6 bytes -> +12..+18)
    //   +4: baseCoverageOffset = 18 (6 bytes -> +18..+24)
    //   +6: markClassCount = 1
    //   +8: markArrayOffset = 24 (12 bytes -> +24..+36)
    //   +10: baseArrayOffset = 36 (10 bytes -> +36..+46)
    let mut sub = Vec::new();
    push_u16(&mut sub, 1); // posFormat = 1
    push_u16(&mut sub, 12); // markCoverageOffset
    push_u16(&mut sub, 18); // baseCoverageOffset
    push_u16(&mut sub, 1); // markClassCount = 1
    push_u16(&mut sub, 24); // markArrayOffset
    push_u16(&mut sub, 36); // baseArrayOffset

    // MarkCoverage at +12:
    push_u16(&mut sub, 1);
    push_u16(&mut sub, 1);
    push_u16(&mut sub, GID_ACUTE_MARK);

    // BaseCoverage at +18:
    push_u16(&mut sub, 1);
    push_u16(&mut sub, 1);
    push_u16(&mut sub, GID_A_LOW);

    // MarkArray at +24:
    //   markCount(2) + MarkRecord(markClass: 2, markAnchorOffset: 2) = 6 bytes -> Anchor at +6
    push_u16(&mut sub, 1); // markCount = 1
    push_u16(&mut sub, 0); // markClass = 0
    push_u16(&mut sub, 6); // markAnchorOffset = 6
    // AnchorFormat1 (6 bytes):
    push_u16(&mut sub, 1); // anchorFormat = 1
    push_i16(&mut sub, 100); // xCoordinate = 100
    push_i16(&mut sub, 450); // yCoordinate = 450

    // BaseArray at +36:
    //   baseCount(2) + BaseRecord(baseAnchorOffsets[0]: 2) = 4 bytes -> Anchor at +4
    push_u16(&mut sub, 1); // baseCount = 1
    push_u16(&mut sub, 4); // baseAnchorOffsets[0] = 4
    // AnchorFormat1 (6 bytes):
    push_u16(&mut sub, 1); // anchorFormat = 1
    push_i16(&mut sub, 320); // xCoordinate = 320
    push_i16(&mut sub, 660); // yCoordinate = 660

    let mut lk = Vec::new();
    push_u16(&mut lk, 4); // lookupType = 4 (MarkBasePos)
    push_u16(&mut lk, 0); // lookupFlag = 0
    push_u16(&mut lk, 1); // subTableCount = 1
    push_u16(&mut lk, 8); // subtableOffsets[0] = 8
    lk.extend_from_slice(&sub);
    lk
}

fn build_gpos_table() -> Vec<u8> {
    let scripts: [&[u8; 4]; 2] = [b"DFLT", b"latn"];
    let mut script_list = Vec::new();
    push_u16(&mut script_list, scripts.len() as u16);
    let shared_off = (2 + scripts.len() * 6) as u16;
    for tag in scripts {
        script_list.extend_from_slice(tag);
        push_u16(&mut script_list, shared_off);
    }
    push_u16(&mut script_list, 4); // defaultLangSysOffset = 4
    push_u16(&mut script_list, 0);
    push_u16(&mut script_list, 0);
    push_u16(&mut script_list, 0xFFFF);
    push_u16(&mut script_list, 2); // 2 features: kern(0), mark(1)
    push_u16(&mut script_list, 0);
    push_u16(&mut script_list, 1);

    let features: [(&[u8; 4], u16); 2] = [(b"kern", 0), (b"mark", 1)];
    let mut feature_list = Vec::new();
    push_u16(&mut feature_list, 2);
    let feat_tables_start = 2 + 2 * 6;
    for (i, (tag, _)) in features.iter().enumerate() {
        feature_list.extend_from_slice(*tag);
        push_u16(&mut feature_list, (feat_tables_start + i * 6) as u16);
    }
    for &(_, lk_idx) in &features {
        push_u16(&mut feature_list, 0);
        push_u16(&mut feature_list, 1);
        push_u16(&mut feature_list, lk_idx);
    }

    let lookups = [
        encode_gpos_kern_pair_lookup(),
        encode_gpos_mark_to_base_lookup(),
    ];
    let mut lookup_list = Vec::new();
    push_u16(&mut lookup_list, lookups.len() as u16);
    let mut running_off = 2 + lookups.len() * 2;
    for lk in &lookups {
        push_u16(&mut lookup_list, running_off as u16);
        running_off += lk.len();
    }
    for lk in &lookups {
        lookup_list.extend_from_slice(lk);
    }

    let script_off = 10u16;
    let feat_off = script_off + script_list.len() as u16;
    let lookup_off = feat_off + feature_list.len() as u16;
    let mut gpos = Vec::new();
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 0);
    push_u16(&mut gpos, script_off);
    push_u16(&mut gpos, feat_off);
    push_u16(&mut gpos, lookup_off);
    gpos.extend_from_slice(&script_list);
    gpos.extend_from_slice(&feature_list);
    gpos.extend_from_slice(&lookup_list);
    gpos
}

/// Builds a complete, standalone OpenType TrueType SFNT (`0x00010000`) font binary containing
/// `GDEF`, `GSUB` (`liga`, Arabic `init`/`medi`/`fina`), and `GPOS` (`kern`, `mark`) WITHOUT any
/// legacy `kern` table.
fn build_opentype_test_font() -> Vec<u8> {
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    let mut hmtx = Vec::new();

    for gid in 0..NUM_GLYPHS {
        push_u32(&mut loca, glyf.len() as u32);
        let (rects, advance): (Vec<(i16, i16, i16, i16)>, u16) = match gid {
            GID_NOTDEF => (vec![(50, 0, 450, 700)], 500),
            GID_SPACE => (vec![], 260),
            GID_A_CAP => (vec![(40, 0, 560, 700)], 600),
            GID_V_CAP => (vec![(40, 0, 560, 700)], 600),
            GID_T_CAP => (vec![(20, 0, 540, 700)], 560),
            GID_O_LOW => (vec![(40, 0, 460, 500)], 500),
            GID_F_LOW => (vec![(40, 0, 300, 720)], 340), // 1 contour
            GID_I_LOW => (vec![(60, 0, 200, 500), (60, 580, 200, 720)], 260), // 2 contours (stem + dot)
            GID_A_LOW => (vec![(40, 0, 460, 500)], 500),
            GID_ACUTE_MARK => (vec![(40, 460, 180, 620)], 0), // 0 advance! Positioned via GPOS `mark`
            GID_FI_LIG => (vec![(40, 0, 500, 720)], 540), // 1 merged ligature contour (vs 1+2=3 unshaped!)
            GID_HEB_ALEF => (vec![(40, 0, 500, 650)], 540),
            GID_HEB_BET => (vec![(40, 0, 480, 650)], 520),
            GID_HEB_GIMEL => (vec![(40, 0, 420, 650)], 460),
            GID_ARAB_BEH => (vec![(40, 0, 520, 420)], 560),
            GID_ARAB_BEH_INIT => (vec![(0, 0, 380, 420)], 380),
            GID_ARAB_BEH_MEDI => (vec![(0, 0, 340, 420)], 340),
            GID_ARAB_BEH_FINA => (vec![(0, 0, 500, 420)], 500),
            GID_B_CAP | GID_C_CAP | GID_D_CAP => (vec![(40, 0, 520, 700)], 560),
            _ => {
                // Multi-contour bulky filler glyphs so subsetting shrinks `glyf` substantially.
                let mut r = Vec::new();
                for k in 0..8i16 {
                    let x0 = k * 60;
                    r.push((x0, 0, x0 + 45, 650));
                }
                (r, 520)
            }
        };
        let lsb = rects.iter().map(|r| r.0).min().unwrap_or(0);
        let glyph_bytes = encode_rect_contours_glyph(&rects);
        glyf.extend_from_slice(&glyph_bytes);
        push_u16(&mut hmtx, advance);
        push_i16(&mut hmtx, lsb);
    }
    push_u32(&mut loca, glyf.len() as u32);

    let mut cmap_map = BTreeMap::<u16, u16>::new();
    cmap_map.insert(' ' as u16, GID_SPACE);
    cmap_map.insert('A' as u16, GID_A_CAP);
    cmap_map.insert('B' as u16, GID_B_CAP);
    cmap_map.insert('C' as u16, GID_C_CAP);
    cmap_map.insert('D' as u16, GID_D_CAP);
    cmap_map.insert('T' as u16, GID_T_CAP);
    cmap_map.insert('V' as u16, GID_V_CAP);
    cmap_map.insert('a' as u16, GID_A_LOW);
    cmap_map.insert('f' as u16, GID_F_LOW);
    cmap_map.insert('i' as u16, GID_I_LOW);
    cmap_map.insert('o' as u16, GID_O_LOW);
    cmap_map.insert('\u{0301}' as u16, GID_ACUTE_MARK);
    cmap_map.insert('\u{05D0}' as u16, GID_HEB_ALEF);
    cmap_map.insert('\u{05D1}' as u16, GID_HEB_BET);
    cmap_map.insert('\u{05D2}' as u16, GID_HEB_GIMEL);
    cmap_map.insert('\u{0628}' as u16, GID_ARAB_BEH);
    for idx in 0..(NUM_GLYPHS - FIRST_FILLER_GID) {
        cmap_map.insert(0x0100 + idx, FIRST_FILLER_GID + idx);
    }
    let cmap = build_cmap_format4(&cmap_map);

    let mut head = vec![0u8; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes()); // version 1.0
    head[4..8].copy_from_slice(&0x0001_0000u32.to_be_bytes()); // fontRevision
    head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes()); // magicNumber
    head[16..18].copy_from_slice(&0x000Bu16.to_be_bytes()); // flags
    head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm = 1000
    head[36..38].copy_from_slice(&0i16.to_be_bytes()); // xMin
    head[38..40].copy_from_slice(&(-200i16).to_be_bytes()); // yMin
    head[40..42].copy_from_slice(&600i16.to_be_bytes()); // xMax
    head[42..44].copy_from_slice(&800i16.to_be_bytes()); // yMax
    head[50..52].copy_from_slice(&1i16.to_be_bytes()); // indexToLocFormat = 1 (long offsets)

    let mut hhea = vec![0u8; 36];
    hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    hhea[4..6].copy_from_slice(&800i16.to_be_bytes()); // ascent
    hhea[6..8].copy_from_slice(&(-200i16).to_be_bytes()); // descent
    hhea[8..10].copy_from_slice(&0i16.to_be_bytes()); // lineGap
    hhea[10..12].copy_from_slice(&600u16.to_be_bytes()); // advanceWidthMax
    hhea[34..36].copy_from_slice(&NUM_GLYPHS.to_be_bytes()); // numberOfHMetrics

    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&NUM_GLYPHS.to_be_bytes());
    maxp[6..8].copy_from_slice(&64u16.to_be_bytes()); // maxPoints
    maxp[8..10].copy_from_slice(&16u16.to_be_bytes()); // maxContours

    let mut os2 = vec![0u8; 96];
    os2[0..2].copy_from_slice(&4u16.to_be_bytes()); // version 4
    os2[2..4].copy_from_slice(&500i16.to_be_bytes()); // xAvgCharWidth
    os2[4..6].copy_from_slice(&400u16.to_be_bytes()); // usWeightClass = Regular
    os2[6..8].copy_from_slice(&5u16.to_be_bytes()); // usWidthClass = Medium
    os2[62..64].copy_from_slice(&0x0040u16.to_be_bytes()); // fsSelection = REGULAR
    os2[64..66].copy_from_slice(&0x0020u16.to_be_bytes()); // usFirstCharIndex
    os2[66..68].copy_from_slice(&0x0628u16.to_be_bytes()); // usLastCharIndex
    os2[68..70].copy_from_slice(&800i16.to_be_bytes()); // sTypoAscender
    os2[70..72].copy_from_slice(&(-200i16).to_be_bytes()); // sTypoDescender
    os2[74..76].copy_from_slice(&800u16.to_be_bytes()); // usWinAscent
    os2[76..78].copy_from_slice(&200u16.to_be_bytes()); // usWinDescent

    let mut post = vec![0u8; 32];
    post[0..4].copy_from_slice(&0x0003_0000u32.to_be_bytes()); // version 3.0

    // Minimal `name` table naming family "OTShapeTest" / Regular.
    let family_utf16: Vec<u8> = "OTShapeTest"
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    let style_utf16: Vec<u8> = "Regular"
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    let mut name_tbl = Vec::new();
    push_u16(&mut name_tbl, 0); // format
    push_u16(&mut name_tbl, 2); // count
    push_u16(&mut name_tbl, 6 + 2 * 12); // stringOffset = 30
    // Record 0: nameID 1 (Font Family)
    push_u16(&mut name_tbl, 3);
    push_u16(&mut name_tbl, 1);
    push_u16(&mut name_tbl, 0x0409);
    push_u16(&mut name_tbl, 1);
    push_u16(&mut name_tbl, family_utf16.len() as u16);
    push_u16(&mut name_tbl, 0);
    // Record 1: nameID 2 (Font Subfamily)
    push_u16(&mut name_tbl, 3);
    push_u16(&mut name_tbl, 1);
    push_u16(&mut name_tbl, 0x0409);
    push_u16(&mut name_tbl, 2);
    push_u16(&mut name_tbl, style_utf16.len() as u16);
    push_u16(&mut name_tbl, family_utf16.len() as u16);
    name_tbl.extend_from_slice(&family_utf16);
    name_tbl.extend_from_slice(&style_utf16);

    // Assemble SFNT tables (notice: NO `kern` table!).
    let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"GDEF", build_gdef_table()),
        (*b"GPOS", build_gpos_table()),
        (*b"GSUB", build_gsub_table()),
        (*b"OS/2", os2),
        (*b"cmap", cmap),
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
        (*b"name", name_tbl),
        (*b"post", post),
    ];
    tables.sort_by_key(|(tag, _)| *tag);

    let table_count = tables.len() as u16;
    let entry_selector = 15u16.saturating_sub(table_count.leading_zeros() as u16);
    let search_range = (1u16 << entry_selector) * 16;
    let range_shift = table_count * 16 - search_range;

    let mut sfnt = Vec::new();
    push_u32(&mut sfnt, 0x0001_0000);
    push_u16(&mut sfnt, table_count);
    push_u16(&mut sfnt, search_range);
    push_u16(&mut sfnt, entry_selector);
    push_u16(&mut sfnt, range_shift);
    let dir_pos = sfnt.len();
    sfnt.resize(dir_pos + (table_count as usize) * 16, 0);

    let mut head_offset = 0usize;
    for (i, (tag, data)) in tables.iter().enumerate() {
        let offset = sfnt.len() as u32;
        let length = data.len() as u32;
        let sum = sfnt_checksum(data);
        if tag == b"head" {
            head_offset = offset as usize;
        }
        let rec = dir_pos + i * 16;
        sfnt[rec..rec + 4].copy_from_slice(tag);
        sfnt[rec + 4..rec + 8].copy_from_slice(&sum.to_be_bytes());
        sfnt[rec + 8..rec + 12].copy_from_slice(&offset.to_be_bytes());
        sfnt[rec + 12..rec + 16].copy_from_slice(&length.to_be_bytes());
        sfnt.extend_from_slice(data);
        while !sfnt.len().is_multiple_of(4) {
            sfnt.push(0);
        }
    }
    let adj = 0xB1B0_AFBAu32.wrapping_sub(sfnt_checksum(&sfnt));
    sfnt[head_offset + 8..head_offset + 12].copy_from_slice(&adj.to_be_bytes());
    sfnt
}

fn sfnt_tags(bytes: &[u8]) -> BTreeSet<[u8; 4]> {
    let count = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
    (0..count)
        .map(|i| {
            let s = 12 + i * 16;
            bytes[s..s + 4].try_into().unwrap()
        })
        .collect()
}

fn count_move_to(commands: &[PathCommand]) -> usize {
    commands
        .iter()
        .filter(|c| matches!(c, PathCommand::MoveTo { .. }))
        .count()
}

fn path_bbox(commands: &[PathCommand]) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                for (px, py) in [(cx, cy), (x, y)] {
                    min_x = min_x.min(px);
                    min_y = min_y.min(py);
                    max_x = max_x.max(px);
                    max_y = max_y.max(py);
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
                for (px, py) in [(cx1, cy1), (cx2, cy2), (x, y)] {
                    min_x = min_x.min(px);
                    min_y = min_y.min(py);
                    max_x = max_x.max(px);
                    max_y = max_y.max(py);
                }
            }
            PathCommand::Close => {}
        }
    }
    (min_x, min_y, max_x, max_y)
}

#[test]
fn gsub_ligature_fi_substitutes_single_glyph_and_reduces_outline_contours() {
    let font_bytes = build_opentype_test_font();
    assert!(!sfnt_tags(&font_bytes).contains(b"kern"));

    let shaped_fi = shape_opentype_line(&font_bytes, 0, "fi", 100.0, true, 0.0)
        .expect("harfrust shapes OpenType test font");
    assert_eq!(
        shaped_fi.len(),
        1,
        "GSUB `liga` must substitute 'f' + 'i' into one ligature glyph: {shaped_fi:?}"
    );
    assert_eq!(shaped_fi[0].glyph_id, GID_FI_LIG);
    assert_eq!(shaped_fi[0].cluster, 0);

    // Unshaped per-char 'f' (1 contour) + 'i' (2 contours = stem + dot) has 3 contours and
    // advance 34 + 26 = 60 at size 100; shaped `fi` ligature has 1 contour and advance 54.
    let ab_font = ab_glyph::FontRef::try_from_slice(&font_bytes).expect("valid ab_glyph FontRef");
    let unshaped_f = outline_glyph(&ab_font, 'f', 0.0, 0.0, 100.0).expect("f outline");
    let unshaped_i = outline_glyph(&ab_font, 'i', 34.0, 0.0, 100.0).expect("i outline");
    let unshaped_contours = count_move_to(&unshaped_f) + count_move_to(&unshaped_i);
    assert_eq!(unshaped_contours, 3);

    let shaped_cmds = extract_text_outline_commands_with_diagnostics(
        "test/fi",
        "fi",
        "OTShapeTest",
        100.0,
        Some(&font_bytes),
    )
    .expect("shaped fi outline commands");
    assert!(!shaped_cmds.is_empty());
    let shaped_contours = count_move_to(&shaped_cmds);
    assert_eq!(shaped_contours, 1);
    assert!(
        shaped_contours < unshaped_contours,
        "shaped 'fi' ligature must have fewer contours than unshaped 'f' + 'i'"
    );
    assert!(
        (shaped_fi[0].x_advance - 54.0).abs() < 1e-6,
        "ligature advance should be 540/1000 * 100 = 54.0, got {}",
        shaped_fi[0].x_advance
    );
}

#[test]
fn gpos_pair_kerning_narrows_advance_without_legacy_kern_table() {
    // 1. Programmatic OpenType font (`GPOS` PairPosFormat1, NO `kern` table)
    let font_bytes = build_opentype_test_font();
    assert!(!sfnt_tags(&font_bytes).contains(b"kern"));
    assert!(sfnt_tags(&font_bytes).contains(b"GPOS"));

    for (pair, expected_delta_100) in [("AV", 18.0), ("To", 14.0)] {
        let width_on =
            measure_opentype_text(&font_bytes, 0, pair, 100.0, true, 0.0).expect("kern on");
        let width_off =
            measure_opentype_text(&font_bytes, 0, pair, 100.0, false, 0.0).expect("kern off");
        assert!(
            (width_off - width_on - expected_delta_100).abs() < 1e-4,
            "{pair}: expected GPOS kerning delta {expected_delta_100}, got on={width_on}, off={width_off}"
        );
    }

    // Verify `TextFieldState` with `kerning: Some(true)` vs `Some(false)` produces narrower
    // outline bounding box from `GPOS`!
    let make_field = |kerning: bool| -> TextFieldState {
        let kind = ElementKind::TextBox {
            text: "AV To".to_owned(),
            width: 500.0,
            height: 120.0,
            format: TextFormat {
                font: "OTShapeTest".to_owned(),
                size: 100.0,
                kerning: Some(kerning),
                ..TextFormat::default()
            },
            word_wrap: false,
            multiline: false,
        };
        TextFieldState::from_kind(&kind).expect("text field state")
    };
    let cmds_kern_on = extract_text_field_outline_commands_with_diagnostics(
        "test/kern-on",
        &make_field(true),
        Some(&font_bytes),
    )
    .expect("outlines kern on");
    let cmds_kern_off = extract_text_field_outline_commands_with_diagnostics(
        "test/kern-off",
        &make_field(false),
        Some(&font_bytes),
    )
    .expect("outlines kern off");
    let (_, _, max_x_on, _) = path_bbox(&cmds_kern_on);
    let (_, _, max_x_off, _) = path_bbox(&cmds_kern_off);
    assert!(
        max_x_on + 30.0 < max_x_off,
        "TextFieldState kerning=Some(true) must position outlines narrower than Some(false): max_x_on={max_x_on} vs max_x_off={max_x_off}"
    );

    // 2. Real production bundled `Inter-Regular.ttf` (also has `GPOS` and NO legacy `kern` table!)
    assert!(!sfnt_tags(BUNDLED_INTER).contains(b"kern"));
    assert!(sfnt_tags(BUNDLED_INTER).contains(b"GPOS"));
    for pair in ["AV", "To"] {
        let inter_on =
            measure_opentype_text(BUNDLED_INTER, 0, pair, 64.0, true, 0.0).expect("Inter kern on");
        let inter_off = measure_opentype_text(BUNDLED_INTER, 0, pair, 64.0, false, 0.0)
            .expect("Inter kern off");
        assert!(
            inter_on < inter_off - 0.5,
            "Inter-Regular.ttf GPOS pair kerning must narrow '{pair}': on={inter_on} vs off={inter_off}"
        );
    }
}

#[test]
fn gpos_mark_to_base_diacritic_and_unicode_bidi_rtl_visual_ordering() {
    let font_bytes = build_opentype_test_font();

    // 1. GPOS MarkBasePos (`mark`): `"a\u{0301}"` positions combining acute over base `'a'`
    //    with non-zero `(x_offset, y_offset)` and zero horizontal advance on the mark!
    let shaped_a = shape_opentype_line(&font_bytes, 0, "a", 100.0, true, 0.0).unwrap();
    let shaped_accented =
        shape_opentype_line(&font_bytes, 0, "a\u{0301}", 100.0, true, 0.0).unwrap();
    assert_eq!(shaped_accented.len(), 2, "{shaped_accented:?}");
    assert_eq!(shaped_accented[0].glyph_id, GID_A_LOW);
    assert_eq!(shaped_accented[1].glyph_id, GID_ACUTE_MARK);
    assert_eq!(
        shaped_accented[1].x_advance, 0.0,
        "combining mark must not advance horizontal cursor: {shaped_accented:?}"
    );
    assert!(
        shaped_accented[1].x_offset.abs() > 1.0 && shaped_accented[1].y_offset.abs() > 1.0,
        "GPOS `mark` must attach combining acute with non-zero x_offset & y_offset: {shaped_accented:?}"
    );
    let width_a = measure_opentype_text(&font_bytes, 0, "a", 100.0, true, 0.0).unwrap();
    let width_accented =
        measure_opentype_text(&font_bytes, 0, "a\u{0301}", 100.0, true, 0.0).unwrap();
    assert!(
        (width_a - width_accented).abs() < 1e-6,
        "combining diacritic must not widen spacing advance: a={width_a} vs a+acute={width_accented}"
    );
    let _ = shaped_a;

    // Outline verification: acute mark's X range sits directly over base `'a'` (not to its right!),
    // and its top Y extends higher (smaller Y in Y-down stage coords) than plain `'a'`.
    let cmds_plain_a = extract_text_outline_commands_with_diagnostics(
        "test/a",
        "a",
        "OTShapeTest",
        100.0,
        Some(&font_bytes),
    )
    .unwrap();
    let cmds_accented = extract_text_outline_commands_with_diagnostics(
        "test/a-acute",
        "a\u{0301}",
        "OTShapeTest",
        100.0,
        Some(&font_bytes),
    )
    .unwrap();
    let (a_min_x, a_min_y, a_max_x, _) = path_bbox(&cmds_plain_a);
    let (acc_min_x, acc_min_y, acc_max_x, _) = path_bbox(&cmds_accented);
    assert!(
        acc_min_x >= a_min_x && acc_max_x <= a_max_x,
        "combining acute outline X [{acc_min_x}, {acc_max_x}] must lie over base 'a' [{a_min_x}, {a_max_x}]"
    );
    assert!(
        acc_min_y < a_min_y - 10.0,
        "GPOS y_offset must raise acute accent above 'a' in Y-down coordinates: acc_min_y={acc_min_y} vs a_min_y={a_min_y}"
    );

    // 2. RTL Hebrew `"אבג"` & mixed Bidi `"AB אבג CD"` visual ordering via `unicode_bidi`!
    let heb = "\u{05D0}\u{05D1}\u{05D2}"; // logical: Alef(0), Bet(2), Gimel(4)
    let shaped_heb = shape_opentype_line(&font_bytes, 0, heb, 100.0, true, 0.0).unwrap();
    let heb_gids: Vec<u16> = shaped_heb.iter().map(|g| g.glyph_id).collect();
    let heb_clusters: Vec<u32> = shaped_heb.iter().map(|g| g.cluster).collect();
    assert_eq!(
        heb_gids,
        vec![GID_HEB_GIMEL, GID_HEB_BET, GID_HEB_ALEF],
        "pure RTL Hebrew must be ordered visually left-to-right [Gimel, Bet, Alef]"
    );
    assert_eq!(heb_clusters, vec![4, 2, 0]);

    let mixed = "AB \u{05D0}\u{05D1}\u{05D2} CD";
    let shaped_mixed = shape_opentype_line(&font_bytes, 0, mixed, 100.0, true, 0.0).unwrap();
    let mixed_gids: Vec<u16> = shaped_mixed.iter().map(|g| g.glyph_id).collect();
    assert_eq!(
        mixed_gids,
        vec![
            GID_A_CAP,
            GID_B_CAP,
            GID_SPACE,
            GID_HEB_GIMEL,
            GID_HEB_BET,
            GID_HEB_ALEF,
            GID_SPACE,
            GID_C_CAP,
            GID_D_CAP
        ],
        "mixed LTR + RTL + LTR Bidi line must keep LTR runs forward and reverse only the embedded RTL run"
    );

    // 3. RTL Arabic `"ببب"` contextual `init`/`medi`/`fina` GSUB + visual RTL ordering!
    let arab = "\u{0628}\u{0628}\u{0628}"; // logical: [init(0), medi(2), fina(4)] -> visual: [fina(4), medi(2), init(0)]
    let shaped_arab = shape_opentype_line(&font_bytes, 0, arab, 100.0, true, 0.0).unwrap();
    let arab_gids: Vec<u16> = shaped_arab.iter().map(|g| g.glyph_id).collect();
    let arab_clusters: Vec<u32> = shaped_arab.iter().map(|g| g.cluster).collect();
    assert_eq!(
        arab_gids,
        vec![GID_ARAB_BEH_FINA, GID_ARAB_BEH_MEDI, GID_ARAB_BEH_INIT],
        "Arabic dual-joining Beh sequence must substitute init/medi/fina via GSUB in RTL visual order"
    );
    assert_eq!(arab_clusters, vec![4, 2, 0]);
}

#[test]
fn subsetted_font_retains_gsub_gpos_gdef_and_reshapes_identically() {
    let font_bytes = build_opentype_test_font();
    let rich_sample = "fi AV To a\u{0301} \u{05D0}\u{05D1}\u{05D2} \u{0628}\u{0628}\u{0628}";

    // 1. Direct `subset_ttf_font_bytes_with_texts` & `apply_font_policy(FontExportPolicy::Subset)` on our OpenType font:
    let mut project = Project::blank();
    project.resources.push(
        ResourceUnit::from_bytes("ot-test", "fonts/OTShapeTest.ttf", "font/ttf", &font_bytes)
            .expect("font resource"),
    );
    let mut fmt = TextFormat {
        font: "OTShapeTest".to_owned(),
        size: 48.0,
        kerning: Some(true),
        ..TextFormat::default()
    };
    fmt.set_field_options(Some(AuthoredTextFieldOptions {
        selectable: false,
        ..AuthoredTextFieldOptions::default()
    }));
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "ot-static-box".to_owned(),
            name: String::new(),
            kind: ElementKind::TextBox {
                text: rich_sample.to_owned(),
                width: 600.0,
                height: 80.0,
                format: fmt,
                word_wrap: false,
                multiline: false,
            },
            ..Element::default()
        });

    let bundle = apply_font_policy(&mut project, FontExportPolicy::Subset);
    assert_eq!(bundle.subset_stats.len(), 1);
    assert!(
        bundle.subset_stats[0].subset_applied,
        "{:?}",
        bundle.subset_stats[0]
    );
    assert!(
        bundle.subset_stats[0].subset_bytes < bundle.subset_stats[0].original_bytes,
        "subsetting must prune unused filler glyphs while keeping GSUB/GPOS/GDEF: {:?}",
        bundle.subset_stats[0]
    );

    let subset_bytes = project.resources[0]
        .decoded_bytes()
        .expect("subset font bytes");
    let tags = sfnt_tags(&subset_bytes);
    assert!(tags.contains(b"GSUB"), "subsetted font lost GSUB");
    assert!(tags.contains(b"GPOS"), "subsetted font lost GPOS");
    assert!(tags.contains(b"GDEF"), "subsetted font lost GDEF");

    for probe in [
        "fi",
        "AV",
        "To",
        "a\u{0301}",
        "\u{05D0}\u{05D1}\u{05D2}",
        "\u{0628}\u{0628}\u{0628}",
        rich_sample,
    ] {
        for kerning in [true, false] {
            let before = shape_opentype_line(&font_bytes, 0, probe, 48.0, kerning, 0.0)
                .expect("shape original");
            let after = shape_opentype_line(&subset_bytes, 0, probe, 48.0, kerning, 0.0)
                .expect("shape subset");
            assert_eq!(
                before, after,
                "subsetted font must re-shape '{probe}' (kerning={kerning}) identically to original font"
            );
        }
        let outlines_before = extract_text_outline_commands_with_diagnostics(
            "orig",
            probe,
            "OTShapeTest",
            48.0,
            Some(&font_bytes),
        )
        .expect("outlines before");
        let outlines_after = extract_text_outline_commands_with_diagnostics(
            "subset",
            probe,
            "OTShapeTest",
            48.0,
            Some(&subset_bytes),
        )
        .expect("outlines after");
        assert_eq!(
            outlines_before, outlines_after,
            "subsetted font must produce identical vector outline PathCommands for '{probe}'"
        );
    }

    // 2. Also verify real bundled `Inter-Regular.ttf` subsetting retains GSUB+GPOS+GDEF and re-shapes identically!
    let inter_probe = "AV To fi a\u{0301}";
    let codepoints: BTreeSet<u32> = inter_probe.chars().map(|c| c as u32).collect();
    let inter_subset =
        subset_ttf_font_bytes_with_texts(BUNDLED_INTER, &codepoints, &[inter_probe.to_owned()])
            .expect("Inter subset");
    assert!(inter_subset.len() * 2 < BUNDLED_INTER.len());
    let inter_tags = sfnt_tags(&inter_subset);
    assert!(inter_tags.contains(b"GSUB"));
    assert!(inter_tags.contains(b"GPOS"));
    assert!(inter_tags.contains(b"GDEF"));
    for kerning in [true, false] {
        let orig_shaped =
            shape_opentype_line(BUNDLED_INTER, 0, inter_probe, 36.0, kerning, 0.0).unwrap();
        let sub_shaped =
            shape_opentype_line(&inter_subset, 0, inter_probe, 36.0, kerning, 0.0).unwrap();
        assert_eq!(orig_shaped, sub_shaped);
    }
    // Also verify standalone `subset_ttf_font_bytes` without explicit text list still closes GSUB ligatures!
    let fi_sub = subset_ttf_font_bytes(&font_bytes, &BTreeSet::from(['f' as u32, 'i' as u32]))
        .expect("standalone fi subset");
    assert_eq!(
        shape_opentype_line(&font_bytes, 0, "fi", 48.0, true, 0.0),
        shape_opentype_line(&fi_sub, 0, "fi", 48.0, true, 0.0)
    );
}
