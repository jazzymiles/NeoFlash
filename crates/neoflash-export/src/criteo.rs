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
        let project =
            Project::from_json(include_str!("../../../examples/moon-tide-300x250.nflash")).unwrap();
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
}
