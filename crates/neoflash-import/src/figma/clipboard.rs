#![forbid(unsafe_code)]
//! Figma Desktop & Web clipboard (`public.html` / `text/html` `<!--(figmeta)...(/figmeta)-->` +
//! `<!--(figma)...(/figma)-->` binary `fig-kiwi` payload) extractor and 1:1 editable scene importer (`T-413`).

use std::io::Cursor;

use neoflash_core::{PathBounds, Project};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zip::ZipArchive;

use super::convert::{build_clipboard_project_from_figma_doc, collect_unresolved_image_refs};
use super::json_doc::parse_figma_json_doc;
use super::kiwi::{looks_like_raster_image_blob, parse_fig_kiwi_canvas_with_limit, sha1_hex};
use super::{
    FigmaDoc, FigmaImageArchive, FigmaImportOptions, FigmaNode, FigmaPageMapping, FigmaPaint,
    FigmaZipReader, MAX_ZIP_IMAGE_ENTRY_BYTES, is_figma_bytes, scan_figma_zip_archive,
};
use crate::{ImportError, ImportReport};

/// Maximum HTML/text clipboard wrapper size accepted synchronously on Stage paste (`128 MiB`).
pub const MAX_STAGE_HTML_CLIPBOARD_BYTES: usize = 128 * 1024 * 1024;
/// Maximum decoded `fig-kiwi` container or decompressed chunk size on Stage clipboard paste (`64 MiB`).
pub const MAX_CLIPBOARD_KIWI_DECODE_BYTES: usize = 64 * 1024 * 1024;

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

/// Fast O(N) check whether `html_or_text` contains a valid Figma clipboard comment marker.
pub fn contains_figma_clipboard_marker(html_or_text: &str) -> bool {
    extract_comment_tag_slice(html_or_text, "figma").is_some()
}

fn extract_comment_tag_slice<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open_pat = format!("!--({tag})");
    let close_pat = format!("(/{tag})--");
    let mut search_from = 0usize;
    while let Some(rel_open) = text[search_from..].find(&open_pat) {
        let start_marker = search_from + rel_open;
        let content_start = start_marker + open_pat.len();
        search_from = content_start;

        // Verify `!--` is preceded by `<` or an HTML entity (`&lt;`, `&#60;`, `&#060;`, `&#x3c;`, `&#x03c;`)
        // strictly on `text.as_bytes()` so non-ASCII UTF-8 (Cyrillic/emoji) never panics!
        let pbytes = &text.as_bytes()[..start_marker];
        let valid_prefix = if pbytes.last() == Some(&b'<') {
            pbytes.iter().rposition(|&b| b == b'<') == Some(pbytes.len() - 1)
        } else if pbytes.last() == Some(&b';') {
            let tail = &pbytes[pbytes.len().saturating_sub(8)..];
            tail.iter().rposition(|&b| b == b'&').is_some_and(|amp| {
                let ent = &tail[amp..];
                ent.eq_ignore_ascii_case(b"&lt;")
                    || ent.eq_ignore_ascii_case(b"&#60;")
                    || ent.eq_ignore_ascii_case(b"&#060;")
                    || ent.eq_ignore_ascii_case(b"&#x3c;")
                    || ent.eq_ignore_ascii_case(b"&#x03c;")
            })
        } else {
            false
        };
        if !valid_prefix {
            continue;
        }

        let rel_end = text[content_start..].find(&close_pat)?;
        let content_end = content_start + rel_end;
        let sbytes = &text.as_bytes()[content_end + close_pat.len()..];
        let valid_suffix = if sbytes.first() == Some(&b'>') {
            sbytes.iter().position(|&b| b == b'>') == Some(0)
        } else if sbytes.first() == Some(&b'&') {
            let head = &sbytes[..sbytes.len().min(8)];
            head.iter().position(|&b| b == b';').is_some_and(|semi| {
                let ent = &head[..=semi];
                ent.eq_ignore_ascii_case(b"&gt;")
                    || ent.eq_ignore_ascii_case(b"&#62;")
                    || ent.eq_ignore_ascii_case(b"&#062;")
                    || ent.eq_ignore_ascii_case(b"&#x3e;")
                    || ent.eq_ignore_ascii_case(b"&#x03e;")
            })
        } else {
            false
        };
        if !valid_suffix {
            continue;
        }
        return Some(&text[content_start..content_end]);
    }
    None
}

#[inline]
fn b64_sextet(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

/// Stream-decode standard, URL-safe, or HTML-entity-escaped base64 directly into a bounded
/// `Vec<u8>` without allocating an intermediate `cleaned: String`.
fn decode_html_base64(raw: &str, max_decoded_bytes: usize) -> Option<Vec<u8>> {
    if max_decoded_bytes == 0 || raw.len() > max_decoded_bytes.saturating_mul(2) {
        return None;
    }
    let bytes = raw.as_bytes();
    let est_cap = (bytes.len() / 4 * 3)
        .min(max_decoded_bytes)
        .min(4 * 1024 * 1024);
    let mut out = Vec::with_capacity(est_cap);
    let mut quantum = [0u8; 4];
    let mut qlen = 0usize;
    let mut saw_pad = false;
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let ch = if b == b'&' {
            let rem = &bytes[i..];
            let window = &rem[..rem.len().min(9)];
            let semi = window.iter().position(|&c| c == b';')?;
            let ent = &window[..=semi];
            if ent.eq_ignore_ascii_case(b"&amp;") {
                i += semi + 1;
                continue;
            } else if ent.eq_ignore_ascii_case(b"&#43;") || ent.eq_ignore_ascii_case(b"&#x2b;") {
                i += semi + 1;
                b'+'
            } else if ent.eq_ignore_ascii_case(b"&#47;") || ent.eq_ignore_ascii_case(b"&#x2f;") {
                i += semi + 1;
                b'/'
            } else if ent.eq_ignore_ascii_case(b"&#61;") || ent.eq_ignore_ascii_case(b"&#x3d;") {
                i += semi + 1;
                b'='
            } else {
                return None;
            }
        } else {
            i += 1;
            b
        };

        if ch == b'=' {
            saw_pad = true;
            continue;
        }
        if saw_pad {
            return None;
        }
        let val = b64_sextet(ch)?;
        quantum[qlen] = val;
        qlen += 1;
        if qlen == 4 {
            if out.len().saturating_add(3) > max_decoded_bytes {
                return None;
            }
            out.push((quantum[0] << 2) | (quantum[1] >> 4));
            out.push((quantum[1] << 4) | (quantum[2] >> 2));
            out.push((quantum[2] << 6) | quantum[3]);
            qlen = 0;
        }
    }

    match qlen {
        0 => {}
        1 => return None,
        2 => {
            if out.len().saturating_add(1) > max_decoded_bytes {
                return None;
            }
            out.push((quantum[0] << 2) | (quantum[1] >> 4));
        }
        3 => {
            if out.len().saturating_add(2) > max_decoded_bytes {
                return None;
            }
            out.push((quantum[0] << 2) | (quantum[1] >> 4));
            out.push((quantum[1] << 4) | (quantum[2] >> 2));
        }
        _ => unreachable!(),
    }

    (!out.is_empty()).then_some(out)
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
    let mut remaining_budget = MAX_CLIPBOARD_KIWI_DECODE_BYTES;
    let mut cursor = html;
    while let Some(pos) = cursor.find("data:image/") {
        if out.len() >= 16 || remaining_budget == 0 {
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
        let max_entry = MAX_ZIP_IMAGE_ENTRY_BYTES.min(remaining_budget);
        if let Some(bytes) = decode_html_base64(raw_b64, max_entry)
            && looks_like_raster_image_blob(&bytes)
        {
            remaining_budget = remaining_budget.saturating_sub(bytes.len());
            out.push(bytes);
        }
        cursor = &b64_start[b64_end..];
    }
    out
}

fn extract_figma_clipboard_kiwi_core(
    html_or_text: &str,
) -> Option<(Vec<u8>, Option<FigmaClipboardMeta>)> {
    if html_or_text.len() > MAX_STAGE_HTML_CLIPBOARD_BYTES {
        return None;
    }
    let b64_kiwi = extract_comment_tag_slice(html_or_text, "figma")?;
    let kiwi_bytes = decode_html_base64(b64_kiwi, MAX_CLIPBOARD_KIWI_DECODE_BYTES)?;
    if !is_figma_bytes(&kiwi_bytes) {
        return None;
    }
    let meta = extract_comment_tag_slice(html_or_text, "figmeta")
        .and_then(|raw| decode_html_base64(raw, 1024 * 1024))
        .and_then(|bytes| parse_figmeta_json(&bytes));
    Some((kiwi_bytes, meta))
}

/// Extract a Figma clipboard `fig-kiwi` payload (`<!--(figma)...(/figma)-->` or HTML-escaped
/// `&lt;!--(figma)...(/figma)--&gt;`), optional `figmeta` JSON metadata, and any inline HTML
/// `data:image/...;base64,...` fallback images from `html_or_text`.
pub fn extract_figma_clipboard_kiwi(html_or_text: &str) -> Option<FigmaClipboardPayload> {
    let (kiwi_bytes, meta) = extract_figma_clipboard_kiwi_core(html_or_text)?;
    let inline_images = extract_inline_html_images(html_or_text);
    Some(FigmaClipboardPayload {
        kiwi_bytes,
        meta,
        inline_images,
    })
}

/// Returns `true` ONLY when the copied Figma selection is a single unstroked, childless,
/// full-bleed `IMAGE` leaf node where `public.png` (`fallback_png`) genuinely represents that
/// single image rather than a flat composite screenshot of a multi-node card/selection.
fn is_single_full_bleed_image_leaf(doc: &FigmaDoc) -> bool {
    let user_pages: Vec<&FigmaNode> = {
        let non_internal: Vec<&FigmaNode> = doc.pages.iter().filter(|p| !p.internal_only).collect();
        if non_internal.is_empty() {
            doc.pages.iter().collect()
        } else {
            non_internal
        }
    };
    let any_visible = user_pages
        .iter()
        .flat_map(|p| &p.children)
        .any(|c| c.visible);
    let top_nodes: Vec<&FigmaNode> = user_pages
        .iter()
        .flat_map(|p| &p.children)
        .filter(|c| c.visible || !any_visible)
        .collect();
    let [node] = top_nodes.as_slice() else {
        return false;
    };
    if !node.children.is_empty()
        || node.component_id.is_some()
        || node.text.is_some()
        || node.is_mask
        || (!node.strokes.is_empty() && (node.stroke_weight > 0.0 || node.border_weights.is_some()))
        || !node.effects.is_empty()
        || !node.fill_paths.is_empty()
        || !node.corner_radii.iter().all(|&r| r <= 0.0)
        || !matches!(
            node.node_type.as_str(),
            "RECTANGLE" | "ROUNDED_RECTANGLE" | "FRAME"
        )
        || node.rotation_deg.abs() > 1e-4
        || node.relative_transform.is_some_and(|[a, b, c, d, _, _]| {
            b.abs() > 1e-4 || c.abs() > 1e-4 || a <= 0.0 || d <= 0.0
        })
    {
        return false;
    }
    matches!(
        node.fills.as_slice(),
        [FigmaPaint::Image {
            scale_mode,
            image_transform: None,
            ..
        }] if scale_mode == "FILL"
    )
}

fn attach_clipboard_images(
    doc: &mut FigmaDoc,
    lazy_html_source: Option<&str>,
    fallback_png: Option<&[u8]>,
) {
    let mut unresolved = collect_unresolved_image_refs(doc, &doc.inline_images);
    if unresolved.is_empty() {
        return;
    }

    // 1. Lazily decode inline HTML `<img src="data:image/...;base64,...">` assets ONLY when
    //    `doc` actually has unresolved image hashes!
    if let Some(html) = lazy_html_source {
        let html_imgs = extract_inline_html_images(html);
        if !html_imgs.is_empty() {
            for bytes in &html_imgs {
                let digest = sha1_hex(bytes);
                doc.inline_images
                    .entry(digest)
                    .or_insert_with(|| bytes.clone());
            }
            unresolved = collect_unresolved_image_refs(doc, &doc.inline_images);
            if unresolved.len() == 1 && html_imgs.len() == 1 {
                doc.inline_images
                    .insert(unresolved[0].clone(), html_imgs[0].clone());
                unresolved.clear();
            } else if unresolved.len() > 1 && unresolved.len() == html_imgs.len() {
                for (key, bytes) in unresolved.drain(..).zip(html_imgs) {
                    doc.inline_images.insert(key, bytes);
                }
            }
        }
    }
    if unresolved.is_empty() {
        return;
    }

    // 2. `fallback_png` (`public.png` / `flavors.png` selection preview): bind if its SHA-1 digest
    //    matches an unresolved hash directly, OR ONLY when the entire copied selection is a single
    //    childless, unstroked leaf node with a single full-bleed `IMAGE` fill!
    let Some(png) = fallback_png.filter(|b| {
        !b.is_empty() && b.len() <= MAX_ZIP_IMAGE_ENTRY_BYTES && looks_like_raster_image_blob(b)
    }) else {
        return;
    };
    let png_digest = sha1_hex(png);
    if unresolved
        .iter()
        .any(|k| k.eq_ignore_ascii_case(&png_digest))
    {
        doc.inline_images.insert(png_digest, png.to_vec());
        return;
    }
    if unresolved.len() == 1 && is_single_full_bleed_image_leaf(doc) {
        doc.inline_images
            .insert(unresolved[0].clone(), png.to_vec());
    }
}

fn import_figma_clipboard_inner(
    bytes: &[u8],
    lazy_html_source: Option<&str>,
    fallback_png: Option<&[u8]>,
    meta: Option<FigmaClipboardMeta>,
) -> Result<ImportOutput, ImportError> {
    if bytes.len() > MAX_STAGE_HTML_CLIPBOARD_BYTES {
        return Err(ImportError::Container(format!(
            "Figma clipboard payload exceeds {} MiB input limit",
            MAX_STAGE_HTML_CLIPBOARD_BYTES / (1024 * 1024)
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
        let mut doc = parse_fig_kiwi_canvas_with_limit(
            bytes,
            source_name,
            &parse_opts,
            MAX_CLIPBOARD_KIWI_DECODE_BYTES,
        )?;
        attach_clipboard_images(&mut doc, lazy_html_source, fallback_png);
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
            let mut doc = parse_fig_kiwi_canvas_with_limit(
                &cb,
                effective_name,
                &parse_opts,
                MAX_CLIPBOARD_KIWI_DECODE_BYTES,
            )?;
            if let Some(mtitle) = payload.meta_name {
                doc.name = mtitle;
            }
            attach_clipboard_images(&mut doc, lazy_html_source, fallback_png);
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
            attach_clipboard_images(&mut doc, lazy_html_source, fallback_png);
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
        if let Some((kiwi_bytes, extracted_meta)) = extract_figma_clipboard_kiwi_core(text) {
            return import_figma_clipboard_inner(
                &kiwi_bytes,
                Some(text),
                fallback_png,
                extracted_meta.or(meta),
            );
        }
        if super::json_doc::looks_like_figma_json(bytes) {
            let mut doc = parse_figma_json_doc(text, source_name)?;
            attach_clipboard_images(&mut doc, lazy_html_source, fallback_png);
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
    if html_or_text.len() > MAX_STAGE_HTML_CLIPBOARD_BYTES {
        return Err(ImportError::Container(format!(
            "Figma HTML clipboard exceeds {} MiB limit",
            MAX_STAGE_HTML_CLIPBOARD_BYTES / (1024 * 1024)
        )));
    }
    if let Some((kiwi_bytes, meta)) = extract_figma_clipboard_kiwi_core(html_or_text) {
        return import_figma_clipboard_inner(&kiwi_bytes, Some(html_or_text), fallback_png, meta);
    }
    import_figma_clipboard_inner(html_or_text.as_bytes(), None, fallback_png, None)
}

/// Import raw `fig-kiwi` / `fig-jam.` binary bytes (or UTF-8 Figma clipboard HTML bytes) at exact
/// `1:1` scale (`scale = 1.0`), hydrating embedded or fallback bitmap assets.
pub fn import_figma_clipboard_bytes(
    bytes: &[u8],
    fallback_png: Option<&[u8]>,
) -> Result<ImportOutput, ImportError> {
    import_figma_clipboard_inner(bytes, None, fallback_png, None)
}
