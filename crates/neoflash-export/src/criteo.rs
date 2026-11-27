//! Single-entry, offline HTML5 creative archive for the Criteo publish preset.

use crate::{BannerOptions, ExportError, atomic_write, banner::compile_criteo_banner_html};
use neoflash_core::Project;
use serde::Serialize;
use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

/// NeoFlash's initial ZIP budget. The receiving placement supplies its actual limit.
pub const DEFAULT_CRITEO_MAX_ZIP_BYTES: usize = 150 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CriteoBannerReport {
    pub html_bytes: usize,
    /// Complete archive size, including entry headers and the central directory.
    pub zip_bytes: usize,
    pub within_budget: bool,
}

/// Retain authors' distribution notices inside the single-file creative.
pub(crate) fn authored_license_notices(project: &Project) -> Result<String, ExportError> {
    let mut notices = String::new();
    for resource in &project.resources {
        if !resource.path.starts_with("licenses/")
            || !resource.path.ends_with(".txt")
            || !resource.media_type.eq_ignore_ascii_case("text/plain")
        {
            continue;
        }
        let bytes = resource
            .decoded_bytes()
            .map_err(|error| ExportError::InvalidProject(format!("{}: {error}", resource.path)))?;
        let notice = std::str::from_utf8(&bytes)
            .map_err(|error| ExportError::Options(format!("{}: {error}", resource.path)))?;
        if notices.is_empty() {
            notices.push_str("<template id=\"nf-licenses\">");
        }
        notices.push_str("<pre>");
        notices.push_str(&crate::html_escape(&resource.path));
        notices.push('\n');
        notices.push_str(&crate::html_escape(notice));
        notices.push_str("</pre>");
    }
    if !notices.is_empty() {
        notices.push_str("</template>");
    }
    Ok(notices)
}

fn build_archive(
    project: &Project,
    max_zip_bytes: usize,
    cancel: &AtomicBool,
) -> Result<(Vec<u8>, CriteoBannerReport), ExportError> {
    let options = BannerOptions {
        loop_timeline: false,
        text_as_outlines: false,
        max_gzip_bytes: None,
        ..BannerOptions::default()
    };
    let html = compile_criteo_banner_html(project, &options, cancel)?;
    validate_ad_creative_html(&html, project.width, project.height)?;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let entry = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    zip.start_file("index.html", entry)
        .map_err(std::io::Error::from)?;
    zip.write_all(html.as_bytes())?;
    let bytes = zip.finish().map_err(std::io::Error::from)?.into_inner();
    let report = CriteoBannerReport {
        html_bytes: html.len(),
        zip_bytes: bytes.len(),
        within_budget: bytes.len() <= max_zip_bytes,
    };
    Ok((bytes, report))
}

/// Measure the exact archive the preset will write without changing the filesystem.
pub fn estimate_criteo_banner(
    project: &Project,
    max_zip_bytes: usize,
) -> Result<CriteoBannerReport, ExportError> {
    estimate_criteo_banner_cancellable(project, max_zip_bytes, &AtomicBool::new(false))
}

pub fn estimate_criteo_banner_cancellable(
    project: &Project,
    max_zip_bytes: usize,
    cancel: &AtomicBool,
) -> Result<CriteoBannerReport, ExportError> {
    Ok(build_archive(project, max_zip_bytes, cancel)?.1)
}

/// Write the ZIP atomically after creative validation and the final byte-budget check.
pub fn export_criteo_banner_zip(
    project: &Project,
    output: &Path,
    max_zip_bytes: usize,
) -> Result<CriteoBannerReport, ExportError> {
    if !output
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    {
        return Err(ExportError::Options(
            "Choose a .zip file for the Criteo banner.".into(),
        ));
    }
    let (archive, report) = build_archive(project, max_zip_bytes, &AtomicBool::new(false))?;
    if !report.within_budget {
        return Err(ExportError::Options(format!(
            "Criteo banner ZIP is {} bytes, exceeding the {}-byte ZIP budget.",
            report.zip_bytes, max_zip_bytes
        )));
    }
    atomic_write(output, &archive)?;
    Ok(report)
}

/// Offline IAB / Criteo / Google Ad Manager HTML5 single-file ad-creative validator.
/// Enforces standard `ad.size` metadata, `var clickTag` declaration, favicon suppression,
/// zero WebAssembly/network APIs, and zero external HTTP(S)/protocol-relative asset loads.
pub fn validate_ad_creative_html(
    html: &str,
    expected_width: u32,
    expected_height: u32,
) -> Result<(), ExportError> {
    let lower = html.to_ascii_lowercase();
    if !lower.trim_start().starts_with("<!doctype html>") {
        return Err(ExportError::Options(
            "Ad creative HTML must start with <!doctype html>.".into(),
        ));
    }
    if !lower.contains("<meta charset=\"utf-8\">") {
        return Err(ExportError::Options(
            "Ad creative HTML must declare <meta charset=\"utf-8\">.".into(),
        ));
    }
    let expected_ad_size = format!(
        "<meta name=\"ad.size\" content=\"width={expected_width},height={expected_height}\">"
    );
    if !lower.contains(&expected_ad_size) {
        return Err(ExportError::Options(format!(
            "Ad creative HTML is missing exact ad.size meta tag ({expected_ad_size})."
        )));
    }
    if !lower.contains("<link rel=\"icon\" href=\"data:,\">") {
        return Err(ExportError::Options(
            "Ad creative HTML must include <link rel=\"icon\" href=\"data:,\"> to suppress /favicon.ico network requests.".into(),
        ));
    }
    if !html.contains("var clickTag") {
        return Err(ExportError::Options(
            "Ad creative HTML must declare top-level `var clickTag` for ad-network validators."
                .into(),
        ));
    }
    if !html.contains("window.open(") {
        return Err(ExportError::Options(
            "Ad creative HTML must wire click-through navigation via `window.open(...)`.".into(),
        ));
    }
    // Strip inert `<template ...>...</template>` blocks (e.g. `<template id="nf-licenses">`)
    // before scanning active markup, CSS, and script blocks. Keep `<script ...>` / `<style ...>`
    // opening tags intact while stripping only their bodies so `<script src="https://...">` is caught.
    let active_html = strip_tag_blocks(html, "template");
    let markup_only = strip_tag_bodies(&strip_tag_bodies(&active_html, "script"), "style");
    check_external_markup_attributes(&markup_only)?;
    for style_block in extract_tag_contents(&active_html, "style") {
        check_external_css_urls(&style_block)?;
    }
    for script_block in extract_tag_contents(&active_html, "script") {
        let scrubbed_js = blank_js_literals_and_comments(&script_block);
        for forbidden_api in ["WebAssembly", "XMLHttpRequest", "new WebSocket", "fetch("] {
            if scrubbed_js.contains(forbidden_api) {
                return Err(ExportError::Options(format!(
                    "Ad creative HTML contains forbidden runtime/network API `{forbidden_api}`."
                )));
            }
        }
    }
    Ok(())
}

fn strip_tag_bodies(input: &str, tag: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let open_pat = format!("<{tag}");
    let close_pat = format!("</{tag}>");
    let mut out = String::with_capacity(input.len());
    let mut pos = 0usize;
    while let Some(rel) = lower[pos..].find(&open_pat) {
        let start = pos + rel;
        let after_tag = start + open_pat.len();
        let boundary = lower
            .as_bytes()
            .get(after_tag)
            .is_none_or(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/');
        if !boundary {
            out.push_str(&input[pos..after_tag]);
            pos = after_tag;
            continue;
        }
        let Some(gt_rel) = lower[after_tag..].find('>') else {
            out.push_str(&input[pos..]);
            return out;
        };
        let open_end = after_tag + gt_rel + 1;
        out.push_str(&input[pos..open_end]);
        if let Some(close_rel) = lower[open_end..].find(&close_pat) {
            let close_start = open_end + close_rel;
            let close_end = close_start + close_pat.len();
            out.push_str(&input[close_start..close_end]);
            pos = close_end;
        } else {
            pos = input.len();
            break;
        }
    }
    out.push_str(&input[pos..]);
    out
}

fn strip_tag_blocks(input: &str, tag: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let open_pat = format!("<{tag}");
    let close_pat = format!("</{tag}>");
    let mut out = String::with_capacity(input.len());
    let mut pos = 0usize;
    while let Some(rel) = lower[pos..].find(&open_pat) {
        let start = pos + rel;
        let after_tag = start + open_pat.len();
        let boundary = lower
            .as_bytes()
            .get(after_tag)
            .is_none_or(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/');
        if !boundary {
            out.push_str(&input[pos..after_tag]);
            pos = after_tag;
            continue;
        }
        out.push_str(&input[pos..start]);
        if let Some(close_rel) = lower[after_tag..].find(&close_pat) {
            pos = after_tag + close_rel + close_pat.len();
        } else {
            pos = input.len();
            break;
        }
    }
    out.push_str(&input[pos..]);
    out
}

fn extract_tag_contents(input: &str, tag: &str) -> Vec<String> {
    let lower = input.to_ascii_lowercase();
    let open_pat = format!("<{tag}");
    let close_pat = format!("</{tag}>");
    let mut blocks = Vec::new();
    let mut pos = 0usize;
    while let Some(rel) = lower[pos..].find(&open_pat) {
        let start = pos + rel;
        let after_tag = start + open_pat.len();
        let boundary = lower
            .as_bytes()
            .get(after_tag)
            .is_none_or(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/');
        if !boundary {
            pos = after_tag;
            continue;
        }
        let Some(gt_rel) = lower[after_tag..].find('>') else {
            break;
        };
        let content_start = after_tag + gt_rel + 1;
        if let Some(close_rel) = lower[content_start..].find(&close_pat) {
            blocks.push(input[content_start..content_start + close_rel].to_owned());
            pos = content_start + close_rel + close_pat.len();
        } else {
            blocks.push(input[content_start..].to_owned());
            break;
        }
    }
    blocks
}

fn check_external_css_urls(css: &str) -> Result<(), ExportError> {
    let lower = css.to_ascii_lowercase();
    let mut pos = 0usize;
    while let Some(rel) = lower[pos..].find("url(") {
        let after = pos + rel + 4;
        let rest = lower[after..].trim_start();
        let unquoted = rest
            .strip_prefix('"')
            .or_else(|| rest.strip_prefix('\''))
            .unwrap_or(rest)
            .trim_start();
        for scheme in ["http://", "https://", "//"] {
            if unquoted.starts_with(scheme) {
                return Err(ExportError::Options(format!(
                    "Ad creative HTML contains forbidden external CSS asset reference `url({scheme}...)`."
                )));
            }
        }
        pos = after;
    }
    Ok(())
}

fn check_external_markup_attributes(markup: &str) -> Result<(), ExportError> {
    let bytes = markup.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        if bytes.get(i + 1) == Some(&b'/') || bytes.get(i + 1) == Some(&b'!') {
            i += 1;
            continue;
        }
        // Scan until closing `>` while respecting quoted attribute values.
        let tag_start = i + 1;
        let mut j = tag_start;
        let mut quote: Option<u8> = None;
        while j < bytes.len() {
            let b = bytes[j];
            if let Some(q) = quote {
                if b == q {
                    quote = None;
                }
            } else if b == b'"' || b == b'\'' {
                quote = Some(b);
            } else if b == b'>' {
                break;
            }
            j += 1;
        }
        let tag_slice = &markup[tag_start..j.min(bytes.len())];
        let tag_lower = tag_slice.to_ascii_lowercase();
        let mut k = 0usize;
        let tb = tag_lower.as_bytes();
        while k < tb.len() {
            if tb[k].is_ascii_alphabetic() || tb[k] == b':' {
                let name_start = k;
                while k < tb.len()
                    && (tb[k].is_ascii_alphanumeric()
                        || tb[k] == b'-'
                        || tb[k] == b'_'
                        || tb[k] == b':')
                {
                    k += 1;
                }
                let attr_name = &tag_lower[name_start..k];
                while k < tb.len() && tb[k].is_ascii_whitespace() {
                    k += 1;
                }
                if k < tb.len() && tb[k] == b'=' {
                    k += 1;
                    while k < tb.len() && tb[k].is_ascii_whitespace() {
                        k += 1;
                    }
                    let val = if k < tb.len() && (tb[k] == b'"' || tb[k] == b'\'') {
                        let q = tb[k];
                        let vs = k + 1;
                        k += 1;
                        while k < tb.len() && tb[k] != q {
                            k += 1;
                        }
                        let out = &tag_lower[vs..k.min(tb.len())];
                        if k < tb.len() {
                            k += 1;
                        }
                        out
                    } else {
                        let vs = k;
                        while k < tb.len() && !tb[k].is_ascii_whitespace() && tb[k] != b'/' {
                            k += 1;
                        }
                        &tag_lower[vs..k]
                    };
                    if matches!(attr_name, "src" | "href" | "xlink:href") {
                        let trimmed = val.trim();
                        for scheme in ["http://", "https://", "//"] {
                            if trimmed.starts_with(scheme) {
                                return Err(ExportError::Options(format!(
                                    "Ad creative HTML contains forbidden external asset reference `{attr_name}=\"{scheme}...\"`."
                                )));
                            }
                        }
                    } else if attr_name == "style" {
                        check_external_css_urls(val)?;
                    }
                }
            } else {
                k += 1;
            }
        }
        i = (j + 1).min(bytes.len());
    }
    Ok(())
}

fn blank_js_literals_and_comments(js: &str) -> String {
    let bytes = js.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' {
            out.push(b' ');
            i = (i + 2).min(bytes.len());
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            out.push(b' ');
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            out.push(b' ');
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        if b == b'"' || b == b'\'' || b == b'`' {
            let q = b;
            out.push(q);
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                    continue;
                }
                if bytes[i] == q {
                    out.push(q);
                    i += 1;
                    break;
                }
                out.push(b' ');
                i += 1;
            }
            continue;
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Validate a packaged `Criteo · HTML5 ZIP` archive against flat single-entry ZIP structure,
/// byte budget, and [`validate_ad_creative_html`].
pub fn validate_criteo_banner_zip_bytes(
    zip_bytes: &[u8],
    expected_width: u32,
    expected_height: u32,
    max_zip_bytes: usize,
) -> Result<(), ExportError> {
    if zip_bytes.len() > max_zip_bytes {
        return Err(ExportError::Options(format!(
            "Criteo banner ZIP is {} bytes, exceeding the {}-byte ZIP budget.",
            zip_bytes.len(),
            max_zip_bytes
        )));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes))
        .map_err(|e| ExportError::Options(format!("Invalid Criteo ZIP archive: {e}")))?;
    if archive.len() != 1 {
        return Err(ExportError::Options(format!(
            "Criteo banner ZIP must contain exactly 1 entry (`index.html`), found {}.",
            archive.len()
        )));
    }
    let mut entry = archive
        .by_index(0)
        .map_err(|e| ExportError::Options(format!("Invalid Criteo ZIP entry: {e}")))?;
    if entry.name() != "index.html" {
        return Err(ExportError::Options(format!(
            "Criteo banner ZIP root entry must be `index.html`, found `{}`.",
            entry.name()
        )));
    }
    const MAX_CRITEO_HTML_BYTES: u64 = 8 * 1024 * 1024;
    let mut html = String::new();
    std::io::Read::read_to_string(
        &mut std::io::Read::take(&mut entry, MAX_CRITEO_HTML_BYTES + 1),
        &mut html,
    )?;
    if html.len() as u64 > MAX_CRITEO_HTML_BYTES {
        return Err(ExportError::Options(format!(
            "Criteo banner HTML exceeds the {}-byte uncompressed safety limit.",
            MAX_CRITEO_HTML_BYTES
        )));
    }
    validate_ad_creative_html(&html, expected_width, expected_height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Read};

    #[test]
    fn exact_archive_budget_preserves_previous_output_on_failure() {
        let project = Project::blank();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("banner.zip");
        let estimate = estimate_criteo_banner(&project, DEFAULT_CRITEO_MAX_ZIP_BYTES).unwrap();
        let report = export_criteo_banner_zip(&project, &path, estimate.zip_bytes).unwrap();
        assert_eq!(report, estimate);
        let bytes = fs::read(&path).unwrap();
        assert_eq!(bytes.len(), report.zip_bytes);
        let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        assert_eq!(zip.len(), 1);
        let mut html = String::new();
        zip.by_name("index.html")
            .unwrap()
            .read_to_string(&mut html)
            .unwrap();
        assert_eq!(html.len(), report.html_bytes);
        assert!(html.contains("ad.size"));
        let ceiling = report.zip_bytes - 1;
        assert!(
            !estimate_criteo_banner(&project, ceiling)
                .unwrap()
                .within_budget
        );
        assert!(export_criteo_banner_zip(&project, &path, ceiling).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn moon_tide_keeps_its_embedded_thai_face_and_notice_in_the_creative() {
        let project = Project::from_json(include_str!(
            "../../../examples/web-and-ads/moon-tide-300x250.nflash"
        ))
        .unwrap();
        let (bytes, report) = build_archive(
            &project,
            DEFAULT_CRITEO_MAX_ZIP_BYTES,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(report.within_budget);
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut html = String::new();
        zip.by_name("index.html")
            .unwrap()
            .read_to_string(&mut html)
            .unwrap();
        assert!(html.contains("width=300,height=250"));
        assert!(html.contains("@font-face") && html.contains("data:font/"));
        assert!(html.contains("ล่องเรือชมจันทร์"));
        assert!(html.contains("nf-licenses") && html.contains("SIL OPEN FONT LICENSE"));
    }

    #[test]
    fn disabled_filters_export_identically_to_unfiltered_while_active_filters_report_layer_and_frame()
     {
        use neoflash_core::{Element, ElementKind, FilterSpec};

        let mut base = Project::blank();
        base.width = 300;
        base.height = 250;
        base.symbols[0].timeline.frame_count = 12;
        base.symbols[0].timeline.layers[0].name = "HeroLayer".into();
        base.symbols[0].timeline.layers[0].keyframes[0].elements = vec![Element {
            id: "cta-card".into(),
            kind: ElementKind::Rectangle {
                width: 180.0,
                height: 60.0,
                fill: [20, 120, 220, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        }];
        base.validate().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let clean_zip_path = dir.path().join("clean.zip");
        export_criteo_banner_zip(&base, &clean_zip_path, DEFAULT_CRITEO_MAX_ZIP_BYTES).unwrap();
        let clean_bytes = fs::read(&clean_zip_path).unwrap();
        let mut clean_zip = zip::ZipArchive::new(Cursor::new(&clean_bytes)).unwrap();
        let mut clean_html = String::new();
        clean_zip
            .by_name("index.html")
            .unwrap()
            .read_to_string(&mut clean_html)
            .unwrap();

        let disabled_blur = FilterSpec::Blur {
            blur_x: 8.0,
            blur_y: 8.0,
            quality: 1,
        }
        .with_enabled(false);
        let disabled_glow = FilterSpec::Glow {
            color: 0xFFCC00,
            alpha: 0.8,
            blur_x: 6.0,
            blur_y: 6.0,
            strength: 2.0,
            quality: 1,
            inner: false,
            knockout: false,
        }
        .with_enabled(false);

        let mut with_disabled = base.clone();
        with_disabled.symbols[0].timeline.layers[0]
            .properties
            .filters
            .push(disabled_blur.clone());
        with_disabled.symbols[0].timeline.layers[0].keyframes[0]
            .filters
            .push(disabled_glow.clone());
        with_disabled.symbols[0].timeline.layers[0].keyframes[0].elements[0]
            .filters
            .push(disabled_blur);
        with_disabled.upgrade_format_for_features();
        with_disabled.validate().unwrap();

        let disabled_zip_path = dir.path().join("disabled.zip");
        export_criteo_banner_zip(
            &with_disabled,
            &disabled_zip_path,
            DEFAULT_CRITEO_MAX_ZIP_BYTES,
        )
        .expect("300x250 Criteo banner with disabled filters must export cleanly to ZIP");
        let disabled_bytes = fs::read(&disabled_zip_path).unwrap();
        let mut disabled_zip = zip::ZipArchive::new(Cursor::new(&disabled_bytes)).unwrap();
        let mut disabled_html = String::new();
        disabled_zip
            .by_name("index.html")
            .unwrap()
            .read_to_string(&mut disabled_html)
            .unwrap();
        assert_eq!(
            disabled_html, clean_html,
            "disabled filters must render identically to removing the disabled effect"
        );

        let mut with_active = with_disabled;
        with_active.symbols[0].timeline.layers[0].keyframes[0].elements[0].filters[0]
            .set_enabled(true);
        let err = export_criteo_banner_zip(
            &with_active,
            &dir.path().join("active.zip"),
            DEFAULT_CRITEO_MAX_ZIP_BYTES,
        )
        .expect_err("active display filter must block Criteo banner export");
        let msg = err.to_string();
        assert!(
            msg.contains("HeroLayer") && msg.contains("frame 1") && msg.contains("Display filters"),
            "active filter diagnostic must retain layer and frame location, got: {msg}"
        );
    }

    #[test]
    fn t497_ad_validator_accepts_criteo_and_native_banners_and_rejects_external_leaks() {
        let mut p = Project::blank();
        p.width = 300;
        p.height = 250;
        p.symbols[0].timeline.layers[0].keyframes[0].elements = vec![neoflash_core::Element {
            id: "box".into(),
            kind: neoflash_core::ElementKind::Rectangle {
                width: 120.0,
                height: 80.0,
                fill: [240, 80, 40, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..neoflash_core::Element::default()
        }];

        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("criteo.zip");
        export_criteo_banner_zip(&p, &zip_path, DEFAULT_CRITEO_MAX_ZIP_BYTES).unwrap();
        let zip_bytes = fs::read(&zip_path).unwrap();
        validate_criteo_banner_zip_bytes(&zip_bytes, 300, 250, DEFAULT_CRITEO_MAX_ZIP_BYTES)
            .expect("Criteo ZIP must pass offline IAB/Criteo/GAM validator");

        let native_html_path = dir.path().join("native.html");
        crate::banner::export_native_banner_html(
            &p,
            &native_html_path,
            &BannerOptions {
                click_tag_url: Some(
                    "https://example.com/offer?WebAssembly=1&cb=fetch(1)&bg=url(https://example.com/bg.png)".into(),
                ),
                ..BannerOptions::default()
            },
        )
        .unwrap();
        let native_html = fs::read_to_string(&native_html_path).unwrap();
        validate_ad_creative_html(&native_html, 300, 250).expect(
            "Web Banner · Native HTML with query params must pass offline IAB/GAM validator",
        );
        assert!(native_html.contains("var clickTag="));

        let with_license_template = native_html.replace(
            "</body>",
            "<template id=\"nf-licenses\">WebAssembly XMLHttpRequest fetch( url(https://example.org)</template></body>",
        );
        validate_ad_creative_html(&with_license_template, 300, 250)
            .expect("inert <template> license text must not false-positive");

        let bad_ext = native_html.replace(
            "</head>",
            "<script src = 'https://cdn.example.com/tracker.js'></script></head>",
        );
        assert!(validate_ad_creative_html(&bad_ext, 300, 250).is_err());

        let bad_xlink = native_html.replace(
            "</body>",
            "<svg><image xlink:href=\"https://cdn.example.com/pixel.png\"/></svg></body>",
        );
        assert!(validate_ad_creative_html(&bad_xlink, 300, 250).is_err());

        let bad_inline_fetch = native_html.replace(
            "</script>",
            ";fetch('https://cdn.example.com/ping');</script>",
        );
        assert!(
            validate_ad_creative_html(&bad_inline_fetch, 300, 250).is_err(),
            "single-line <script> after /^https?:\\/\\//i regex must still catch fetch("
        );

        let missing_click_tag = native_html.replace("var clickTag", "let otherVar");
        assert!(validate_ad_creative_html(&missing_click_tag, 300, 250).is_err());
    }
}
