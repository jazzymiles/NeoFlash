//! WASM bridge for `T-326`: SFNT/TTC/WOFF1 `cmap` Format 4 + Format 12 `Font.hasGlyphs`,
//! embedded font descriptor enumeration, UAX #29 grapheme boundaries, and rich text-run input.

use super::{WebPlayer, display_error, object_ref};
use serde::Deserialize;
use wasm_bindgen::prelude::*;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RichTextInputRequest {
    expected_revision: String,
    begin: u32,
    end: u32,
    text: String,
    #[serde(default)]
    runs: Vec<neoflash_core::TextFormatRun>,
    #[serde(default)]
    typing_format: Option<neoflash_core::TextFormat>,
    #[serde(default)]
    selection_begin: Option<i64>,
    #[serde(default)]
    selection_end: Option<i64>,
}

fn format_to_patch(f: &neoflash_core::TextFormat) -> neoflash_core::TextFormatPatch {
    neoflash_core::TextFormatPatch {
        font: Some(f.font.clone()),
        size: Some(f.size),
        color: Some(f.color),
        bold: Some(f.bold),
        italic: Some(f.italic),
        underline: Some(f.underline),
        align: Some(f.align),
        align_mode: f.align_mode().map(str::to_owned),
        leading: Some(f.leading),
        letter_spacing: Some(f.letter_spacing),
        left_margin: Some(f.left_margin),
        right_margin: Some(f.right_margin),
        indent: Some(f.indent),
        block_indent: Some(f.block_indent),
        bullet: Some(f.bullet),
        kerning: f.kerning,
        url: Some(f.url.clone()),
        target: Some(f.target_str().to_owned()),
        tab_stops: Some(f.tab_stops.clone()),
        display: Some(f.display().unwrap_or("block").to_owned()),
    }
}

/// Evaluates `Font.hasGlyphs(text)` directly against an SFNT/TTC/WOFF1 binary's `cmap` table
/// (Format 4 + Format 12/13). Returns `1` (true), `0` (false), or `-1` (invalid SFNT / no `cmap`).
#[wasm_bindgen]
pub fn sfnt_font_has_glyphs(font_bytes: &[u8], text: &str) -> i32 {
    if font_bytes.len() > 32 * 1024 * 1024 || text.len() > 1024 * 1024 {
        return -1;
    }
    match neoflash_core::sfnt_has_glyphs(font_bytes, text) {
        Some(true) => 1,
        Some(false) => 0,
        None => -1,
    }
}

/// Inspects an SFNT/TTC/WOFF1 binary and returns JSON `EmbeddedFontDescriptor` (or `"null"` if invalid).
#[wasm_bindgen]
pub fn sfnt_font_info_json(font_bytes: &[u8], fallback_family: &str) -> String {
    if font_bytes.len() > 32 * 1024 * 1024 {
        return "null".to_owned();
    }
    match neoflash_core::inspect_sfnt_font_descriptor(font_bytes, fallback_family) {
        Some(desc) => serde_json::to_string(&desc).unwrap_or_else(|_| "null".to_owned()),
        None => "null".to_owned(),
    }
}

/// Returns all UAX #29 extended grapheme cluster boundaries in UTF-16 code-unit offsets.
#[wasm_bindgen]
pub fn grapheme_boundaries_utf16(text: &str) -> Vec<u32> {
    neoflash_core::grapheme_utf16_boundaries(text)
}

#[wasm_bindgen]
impl WebPlayer {
    /// Returns JSON array of `EmbeddedFontDescriptor`s present in `project.resources`.
    pub fn embedded_fonts_json(&self) -> String {
        let descriptors = neoflash_core::enumerate_project_embedded_fonts(&self.project);
        serde_json::to_string(&descriptors).unwrap_or_else(|_| "[]".to_owned())
    }

    /// Evaluates `Font.hasGlyphs(text)` against embedded SFNT/TTC/WOFF1 fonts in `project.resources`.
    /// Returns `1` (true), `0` (false), or `-1` when no matching SFNT binary is embedded.
    pub fn font_has_glyphs_check(
        &self,
        font_name: &str,
        font_style: &str,
        font_type: &str,
        text: &str,
    ) -> i32 {
        match neoflash_core::project_font_has_glyphs(
            &self.project,
            font_name,
            font_style,
            font_type,
            text,
        ) {
            Some(true) => 1,
            Some(false) => 0,
            None => -1,
        }
    }

    /// Applies a grapheme-safe rich user edit (with optional per-run `TextFormatRun`s, e.g. rich paste
    /// or IME commit) to an attached visible input `TextField`, preserving surrounding runs.
    pub fn object_text_rich_input(
        &mut self,
        reference: &str,
        request_json: &str,
    ) -> Result<String, JsValue> {
        if request_json.len() > 4 * 1024 * 1024 {
            return Err(JsValue::from_str("Rich text input request exceeds 4 MiB"));
        }
        let obj_ref = object_ref(reference)?;
        let req: RichTextInputRequest =
            serde_json::from_str(request_json).map_err(display_error)?;

        let mut state = self
            .player
            .runtime_snapshot(&self.project)
            .objects
            .into_iter()
            .find(|node| {
                node.object_ref == obj_ref && node.on_stage && node.display.effective_visible
            })
            .and_then(|node| node.text)
            .ok_or_else(|| {
                JsValue::from_str("user editing requires an attached visible TextField")
            })?;

        if state.field_type != neoflash_core::TextFieldType::Input {
            return Err(JsValue::from_str(
                "user editing requires an input TextField",
            ));
        }
        if state.revision != req.expected_revision {
            return Err(JsValue::from_str(
                "stale input event did not match TextField revision",
            ));
        }
        // Strip single-line newlines (while remapping `req.runs`) before delegating to
        // `state.replace_utf16_range_with_runs`, which filters `state.restrict` BEFORE `state.max_chars`.
        let floor_begin =
            neoflash_core::snap_utf16_to_grapheme_floor(&state.text, i64::from(req.begin));
        let ceil_end = if req.end > req.begin {
            neoflash_core::snap_utf16_to_grapheme_ceil(&state.text, i64::from(req.end))
        } else {
            floor_begin
        };
        let (normalized_raw, norm_runs) =
            neoflash_core::normalize_text_and_runs(&req.text, &req.runs);
        let mut filtered_text = String::new();
        let mut filtered_runs = Vec::<neoflash_core::TextFormatRun>::new();
        let mut out_u16 = 0u32;
        for (c_start, c_end, cluster) in neoflash_core::grapheme_clusters_utf16(&normalized_raw) {
            if cluster == "\n" && !state.multiline {
                continue;
            }
            let c_units = c_end - c_start;
            if (out_u16 as usize) + (c_units as usize) > 1_048_576 {
                break;
            }
            filtered_text.push_str(cluster);
            if !norm_runs.is_empty() {
                for r in &norm_runs {
                    let s = r.start.max(c_start);
                    let e = r.end.min(c_end);
                    if s < e {
                        let mapped_s = out_u16 + (s - c_start);
                        let mapped_e = out_u16 + (e - c_start);
                        if let Some(last) = filtered_runs.last_mut()
                            && last.end == mapped_s
                            && last.format == r.format
                        {
                            last.end = mapped_e;
                        } else {
                            filtered_runs.push(neoflash_core::TextFormatRun {
                                start: mapped_s,
                                end: mapped_e,
                                format: r.format.clone(),
                            });
                        }
                    }
                }
            }
            out_u16 += c_units;
        }
        if !req.text.is_empty() && filtered_text.is_empty() {
            return serde_json::to_string(&state).map_err(display_error);
        }

        let new_caret = state
            .replace_utf16_range_with_runs(
                floor_begin as usize,
                ceil_end as usize,
                &filtered_text,
                &filtered_runs,
                req.typing_format.as_ref(),
            )
            .map_err(display_error)? as i64;

        let sel_begin = req.selection_begin.unwrap_or(new_caret);
        let sel_end = req.selection_end.unwrap_or(new_caret);
        let patch = neoflash_core::TextPatch {
            text: Some(state.text.clone()),
            format: Some(format_to_patch(&state.format)),
            default_format: Some(format_to_patch(&state.default_format)),
            format_runs: Some(state.format_runs.clone()),
            selection: Some(neoflash_core::TextSelection {
                begin: sel_begin,
                end: sel_end,
            }),
            ..neoflash_core::TextPatch::default()
        };
        self.player
            .object_text_update(&self.project, &obj_ref, &patch)
            .map_err(display_error)?;
        let updated = self
            .player
            .object_text_info(&self.project, &obj_ref)
            .map_err(display_error)?;
        serde_json::to_string(&updated).map_err(display_error)
    }
}
