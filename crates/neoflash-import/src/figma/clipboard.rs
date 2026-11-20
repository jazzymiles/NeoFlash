#![forbid(unsafe_code)]
//! Figma Desktop & Web clipboard (`public.html` / `text/html` `<!--(figmeta)...(/figmeta)-->` +
//! `<!--(figma)...(/figma)-->` binary `fig-kiwi` payload) extractor and 1:1 editable scene importer (`T-413`).

use std::io::Cursor;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use neoflash_core::{PathBounds, Project};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zip::ZipArchive;

use super::convert::{build_clipboard_project_from_figma_doc, collect_unresolved_image_refs};
use super::json_doc::parse_figma_json_doc;
use super::kiwi::{looks_like_raster_image_blob, parse_fig_kiwi_canvas_with_options, sha1_hex};
use super::{
    FigmaDoc, FigmaImageArchive, FigmaImportOptions, FigmaPageMapping, FigmaZipReader,
    MAX_FIGMA_DECODE_BYTES, MAX_FIGMA_FILE_BYTES, MAX_ZIP_IMAGE_ENTRY_BYTES, is_figma_bytes,
    scan_figma_zip_archive,
};
use crate::{ImportError, ImportReport};

/// Optional decoded JSON metadata from `<!--(figmeta)...(/figmeta)-->` on the Figma clipboard.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FigmaClipboardMeta {
    pub file_key: Option<String>,
    pub paste_id: Option<String>,
    pub data_type: Option<String>,
}

/// Extracted binary `fig-kiwi` container, optional `figmeta` JSON metadata, and any inline
/// `data:image/...;base64,...` fallback images embedded in a Figma HTML clipboard payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FigmaClipboardPayload {
    pub kiwi_bytes: Vec<u8>,
    pub meta: Option<FigmaClipboardMeta>,
    pub inline_images: Vec<Vec<u8>>,
}

impl FigmaClipboardPayload {
    /// Returns the first inline `data:image/...` fallback image from the HTML wrapper, if any.
    pub fn primary_inline_image(&self) -> Option<&[u8]> {
        self.inline_images.first().map(Vec::as_slice)
    }
}

/// Result of importing a Figma clipboard payload at exact `1:1` (`scale = 1.0`) scale (`T-413`).
#[derive(Clone, Debug)]
pub struct ImportOutput {
    pub project: Project,
    pub report: ImportReport,
    /// Bounding box of the copied selection in Figma's source canvas coordinate space.
    pub figma_bounds: Option<PathBounds>,
    /// Bounding box of the top-level converted elements inside `project.root` layer 0.
    pub local_bounds: Option<PathBounds>,
    pub meta: Option<FigmaClipboardMeta>,
}

/// Type alias for [`ImportOutput`] on `neoflash_import::figma`.
pub type FigmaClipboardImport = ImportOutput;

/// Fast O(N) check whether `html_or_text` contains a Figma clipboard comment marker.
pub fn contains_figma_clipboard_marker(html_or_text: &str) -> bool {
    let Some(open_pos) = html_or_text.find("!--(figma)") else {
        return false;
    };
    html_or_text[open_pos + "!--(figma)".len()..].contains("(/figma)--")
}

fn extract_comment_tag_slice<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open_pat = format!("!--({tag})");
    let close_pat = format!("(/{tag})--");
    let start_marker = text.find(&open_pat)?;
    // Verify the `!--` is preceded by `<` or an HTML entity (`&lt;`, `&LT;`, `&#60;`, `&#x3c;`).
    let prefix = &text[..start_marker];
    let valid_prefix = prefix.ends_with('<')
        || prefix.ends_with(';') && {
            let tail_start = prefix.len().saturating_sub(8);
            let tail = prefix[tail_start..].to_ascii_lowercase();
            tail.ends_with("&lt;")
                || tail.ends_with("&#60;")
                || tail.ends_with("&#060;")
                || tail.ends_with("&#x3c;")
                || tail.ends_with("&#x03c;")
        };
    if !valid_prefix {
        return None;
    }
    let content_start = start_marker + open_pat.len();
    let rel_end = text[content_start..].find(&close_pat)?;
    let content_end = content_start + rel_end;
    let suffix = &text[content_end + close_pat.len()..];
    let valid_suffix = suffix.starts_with('>') || {
        let head_end = suffix.len().min(8);
        let head = suffix[..head_end].to_ascii_lowercase();
        head.starts_with("&gt;")
            || head.starts_with("&#62;")
            || head.starts_with("&#062;")
            || head.starts_with("&#x3e;")
            || head.starts_with("&#x03e;")
    };
    if !valid_suffix {
        return None;
    }
    Some(&text[content_start..content_end])
}

fn decode_html_base64(raw: &str, max_decoded_bytes: usize) -> Option<Vec<u8>> {
    if raw.len() > max_decoded_bytes.saturating_mul(2) {
        return None;
    }
    let mut cleaned = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b == b'&'
            && let Some(semi_rel) = raw[i..].find(';')
            && semi_rel <= 8
        {
            let entity = &raw[i..=i + semi_rel];
            if entity.eq_ignore_ascii_case("&#43;") || entity.eq_ignore_ascii_case("&#x2b;") {
                cleaned.push('+');
                i += semi_rel + 1;
                continue;
            } else if entity.eq_ignore_ascii_case("&#47;") || entity.eq_ignore_ascii_case("&#x2f;")
            {
                cleaned.push('/');
                i += semi_rel + 1;
                continue;
            } else if entity.eq_ignore_ascii_case("&#61;") || entity.eq_ignore_ascii_case("&#x3d;")
            {
                cleaned.push('=');
                i += semi_rel + 1;
                continue;
            } else if entity.eq_ignore_ascii_case("&amp;") {
                i += semi_rel + 1;
                continue;
            }
        }
        let ch = match b {
            b'-' => '+',
            b'_' => '/',
            other => other as char,
        };
        cleaned.push(ch);
        i += 1;
    }
    let trimmed = cleaned.trim_end_matches('=');
    if trimmed.is_empty() {
        return None;
    }
    let decoded = STANDARD_NO_PAD
        .decode(trimmed)
        .or_else(|_| STANDARD.decode(&cleaned))
        .ok()?;
    (decoded.len() <= max_decoded_bytes).then_some(decoded)
}

fn parse_figmeta_json(bytes: &[u8]) -> Option<FigmaClipboardMeta> {
    let val: Value = serde_json::from_slice(bytes).ok()?;
    let obj = val.as_object()?;
    let str_or_num = |keys: &[&str]| -> Option<String> {
        for &k in keys {
            if let Some(v) = obj.get(k) {
                if let Some(s) = v.as_str() {
                    let t = s.trim();
                    if !t.is_empty() {
                        return Some(t.to_owned());
                    }
                } else if let Some(n) = v.as_u64() {
                    return Some(n.to_string());
                } else if let Some(n) = v.as_i64() {
                    return Some(n.to_string());
                }
            }
        }
        None
    };
    Some(FigmaClipboardMeta {
        file_key: str_or_num(&["fileKey", "file_key"]),
        paste_id: str_or_num(&["pasteID", "pasteId", "paste_id"]),
        data_type: str_or_num(&["dataType", "data_type"]),
    })
}

fn extract_inline_html_images(html: &str) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut cursor = html;
    while let Some(pos) = cursor.find("data:image/") {
        if out.len() >= 16 {
            break;
        }
        let after = &cursor[pos + "data:image/".len()..];
        cursor = after;
        let Some(semi_pos) = after.find(";base64,") else {
            continue;
        };
        if semi_pos > 24 {
            continue;
        }
        let fmt = &after[..semi_pos];
        if !matches!(
            fmt.to_ascii_lowercase().as_str(),
            "png" | "jpeg" | "jpg" | "webp" | "gif" | "bmp" | "tiff"
        ) {
            continue;
        }
        let b64_start = &after[semi_pos + ";base64,".len()..];
        let b64_end = b64_start
            .find(['"', '\'', '>', '<', ')', ' '])
            .unwrap_or(b64_start.len());
        let raw_b64 = &b64_start[..b64_end];
        if let Some(bytes) = decode_html_base64(raw_b64, MAX_ZIP_IMAGE_ENTRY_BYTES)
            && looks_like_raster_image_blob(&bytes)
        {
            out.push(bytes);
        }
        cursor = &b64_start[b64_end..];
    }
    out
}

/// Extract a Figma clipboard `fig-kiwi` payload (`<!--(figma)...(/figma)-->` or HTML-escaped
/// `&lt;!--(figma)...(/figma)--&gt;`), optional `figmeta` JSON metadata, and any inline HTML
/// `data:image/...;base64,...` fallback images from `html_or_text`.
pub fn extract_figma_clipboard_kiwi(html_or_text: &str) -> Option<FigmaClipboardPayload> {
    if html_or_text.len() > MAX_FIGMA_FILE_BYTES.saturating_mul(2) {
        return None;
    }
    let b64_kiwi = extract_comment_tag_slice(html_or_text, "figma")?;
    let kiwi_bytes = decode_html_base64(b64_kiwi, MAX_FIGMA_DECODE_BYTES)?;
    if !is_figma_bytes(&kiwi_bytes) {
        return None;
    }
    let meta = extract_comment_tag_slice(html_or_text, "figmeta")
        .and_then(|raw| decode_html_base64(raw, 1024 * 1024))
        .and_then(|bytes| parse_figmeta_json(&bytes));
    let inline_images = extract_inline_html_images(html_or_text);
    Some(FigmaClipboardPayload {
        kiwi_bytes,
        meta,
        inline_images,
    })
}

fn attach_extra_clipboard_images(doc: &mut FigmaDoc, extra_images: &[&[u8]]) {
    let valid_extras: Vec<&[u8]> = extra_images
        .iter()
        .copied()
        .filter(|b| {
            !b.is_empty() && b.len() <= MAX_ZIP_IMAGE_ENTRY_BYTES && looks_like_raster_image_blob(b)
        })
        .collect();
    if valid_extras.is_empty() {
        return;
    }
    for &bytes in &valid_extras {
        let digest = sha1_hex(bytes);
        doc.inline_images
            .entry(digest)
            .or_insert_with(|| bytes.to_vec());
    }
    let unresolved = collect_unresolved_image_refs(doc, &doc.inline_images);
    if unresolved.len() == 1 {
        doc.inline_images
            .insert(unresolved[0].clone(), valid_extras[0].to_vec());
    } else if unresolved.len() > 1 && unresolved.len() == valid_extras.len() {
        for (key, &bytes) in unresolved.into_iter().zip(valid_extras.iter()) {
            doc.inline_images.insert(key, bytes.to_vec());
        }
    }
}

fn import_figma_clipboard_inner(
    bytes: &[u8],
    extra_images: &[&[u8]],
    meta: Option<FigmaClipboardMeta>,
) -> Result<ImportOutput, ImportError> {
    if bytes.len() > MAX_FIGMA_FILE_BYTES {
        return Err(ImportError::Container(format!(
            "Figma clipboard payload exceeds {} MiB input limit",
            MAX_FIGMA_FILE_BYTES / (1024 * 1024)
        )));
    }
    let parse_opts = FigmaImportOptions {
        import_hidden_layers: true,
        resize_stage_to_artboard: false,
        page_mapping: FigmaPageMapping::Layers,
        ..FigmaImportOptions::default()
    };
    let source_name = "Figma Clipboard";

    if bytes.starts_with(b"fig-kiwi") || bytes.starts_with(b"fig-jam.") {
        let mut doc = parse_fig_kiwi_canvas_with_options(bytes, source_name, &parse_opts)?;
        attach_extra_clipboard_images(&mut doc, extra_images);
        let inline = std::mem::take(&mut doc.inline_images);
        let mut archive = FigmaImageArchive::from_inline(inline);
        let (project, report, figma_bounds, local_bounds) =
            build_clipboard_project_from_figma_doc(doc, &mut archive, source_name)?;
        return Ok(ImportOutput {
            project,
            report,
            figma_bounds,
            local_bounds,
            meta,
        });
    }

    if bytes.starts_with(b"PK\x03\x04") {
        let mut zip = ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| ImportError::Container(format!("Invalid .fig ZIP archive: {e}")))?;
        let payload = scan_figma_zip_archive(&mut zip, true)?;
        let effective_name = payload.meta_name.as_deref().unwrap_or(source_name);
        if let Some(cb) = payload.canvas_bytes {
            let mut doc = parse_fig_kiwi_canvas_with_options(&cb, effective_name, &parse_opts)?;
            if let Some(mtitle) = payload.meta_name {
                doc.name = mtitle;
            }
            attach_extra_clipboard_images(&mut doc, extra_images);
            let inline = std::mem::take(&mut doc.inline_images);
            let mut archive = FigmaImageArchive::from_zip(
                FigmaZipReader::Memory(zip),
                payload.zip_image_index,
                inline,
            );
            let (project, report, figma_bounds, local_bounds) =
                build_clipboard_project_from_figma_doc(doc, &mut archive, source_name)?;
            return Ok(ImportOutput {
                project,
                report,
                figma_bounds,
                local_bounds,
                meta,
            });
        }
        if let Some(jb) = payload.json_bytes {
            let s = std::str::from_utf8(&jb)
                .map_err(|_| ImportError::Container("Figma JSON entry must be UTF-8".into()))?;
            let mut doc = parse_figma_json_doc(s, effective_name)?;
            attach_extra_clipboard_images(&mut doc, extra_images);
            let inline = std::mem::take(&mut doc.inline_images);
            let mut archive = FigmaImageArchive::from_zip(
                FigmaZipReader::Memory(zip),
                payload.zip_image_index,
                inline,
            );
            let (project, report, figma_bounds, local_bounds) =
                build_clipboard_project_from_figma_doc(doc, &mut archive, source_name)?;
            return Ok(ImportOutput {
                project,
                report,
                figma_bounds,
                local_bounds,
                meta,
            });
        }
        return Err(ImportError::Container(
            ".fig ZIP archive contains neither canvas.fig nor a Figma JSON document".into(),
        ));
    }

    if let Ok(text) = std::str::from_utf8(bytes) {
        if let Some(payload) = extract_figma_clipboard_kiwi(text) {
            let mut combined: Vec<&[u8]> =
                payload.inline_images.iter().map(Vec::as_slice).collect();
            combined.extend_from_slice(extra_images);
            return import_figma_clipboard_inner(
                &payload.kiwi_bytes,
                &combined,
                payload.meta.or(meta),
            );
        }
        if super::json_doc::looks_like_figma_json(bytes) {
            let mut doc = parse_figma_json_doc(text, source_name)?;
            attach_extra_clipboard_images(&mut doc, extra_images);
            let inline = std::mem::take(&mut doc.inline_images);
            let mut archive = FigmaImageArchive::from_inline(inline);
            let (project, report, figma_bounds, local_bounds) =
                build_clipboard_project_from_figma_doc(doc, &mut archive, source_name)?;
            return Ok(ImportOutput {
                project,
                report,
                figma_bounds,
                local_bounds,
                meta,
            });
        }
    }

    Err(ImportError::Container(
        "Clipboard data is neither a Figma HTML payload nor a binary fig-kiwi stream".into(),
    ))
}

/// Import a Figma HTML clipboard payload (`<!--(figma)...(/figma)-->`) at exact `1:1` scale,
/// preserving editable vectors, strokes, gradients, corner radii, editable text, filters,
/// clipping masks, and `Internal Only Canvas` component symbols.
pub fn import_figma_clipboard(
    html_or_text: &str,
    fallback_png: Option<&[u8]>,
) -> Result<ImportOutput, ImportError> {
    if let Some(payload) = extract_figma_clipboard_kiwi(html_or_text) {
        let mut extras: Vec<&[u8]> = payload.inline_images.iter().map(Vec::as_slice).collect();
        if let Some(png) = fallback_png {
            extras.push(png);
        }
        return import_figma_clipboard_inner(&payload.kiwi_bytes, &extras, payload.meta);
    }
    let extras: Vec<&[u8]> = fallback_png.into_iter().collect();
    import_figma_clipboard_inner(html_or_text.as_bytes(), &extras, None)
}

/// Import raw `fig-kiwi` / `fig-jam.` binary bytes (or UTF-8 Figma clipboard HTML bytes) at exact
/// `1:1` scale (`scale = 1.0`), hydrating embedded or fallback bitmap assets.
pub fn import_figma_clipboard_bytes(
    bytes: &[u8],
    fallback_png: Option<&[u8]>,
) -> Result<ImportOutput, ImportError> {
    let extras: Vec<&[u8]> = fallback_png.into_iter().collect();
    import_figma_clipboard_inner(bytes, &extras, None)
}
