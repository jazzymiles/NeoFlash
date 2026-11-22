use super::*;

impl Editor {
    fn log_export_report(&mut self, project: &Project) {
        if let Ok(summary) = neoflash_export::compute_load_summary(project) {
            self.push_output_coalesced(crate::output_pipeline::OutputEvent::new(
                crate::output_pipeline::OutputSeverity::Info,
                crate::output_pipeline::OutputSource::Publish,
                summary.summary_line(),
            ));
        }
        if let Ok(report) = neoflash_export::format_size_report(project) {
            for line in report.lines() {
                if !line.trim().is_empty() {
                    self.push_output_coalesced(crate::output_pipeline::OutputEvent::new(
                        crate::output_pipeline::OutputSeverity::Info,
                        crate::output_pipeline::OutputSource::Publish,
                        line.to_string(),
                    ));
                }
            }
        }
    }

    fn emit_export_warnings(&mut self, warnings: impl IntoIterator<Item = String>) {
        let mut seen = std::collections::HashSet::new();
        for warning in warnings {
            self.status = warning.clone();
            if seen.insert(warning.clone()) {
                let _ = self.push_output_unique(crate::output_pipeline::OutputEvent::new(
                    crate::output_pipeline::OutputSeverity::Warning,
                    crate::output_pipeline::OutputSource::Publish,
                    warning,
                ));
            }
        }
    }

    fn emit_script_export_diagnostics(&mut self) {
        use crate::output_pipeline::{OutputEvent, OutputSeverity, OutputSource};

        self.refresh_compiler_diagnostics();
        let batch: Vec<OutputEvent> = self
            .compiler_errors
            .items
            .iter()
            .map(|item| {
                let severity = if item.is_error {
                    OutputSeverity::Error
                } else {
                    OutputSeverity::Warning
                };
                let location = if let Some(inc) = item.included_path.as_deref() {
                    let file_path = self
                        .resolve_external_as_path_for_navigation(None, std::path::Path::new(inc))
                        .unwrap_or_else(|| PathBuf::from(inc));
                    let mut loc = crate::output_pipeline::OutputLocation::for_file(file_path)
                        .with_document(self.clipboard.session.clone());
                    if item.line > 0 {
                        loc = loc.with_line_col(item.line, item.column.max(1));
                    }
                    Some(loc)
                } else {
                    item.script.as_ref().map(|script| {
                        self.output_location_from_script(
                            script,
                            (item.line > 0).then_some(item.line),
                            (item.column > 0).then_some(item.column),
                        )
                    })
                };
                let msg = if item.location_label.is_empty() {
                    item.message.clone()
                } else {
                    format!("{}: {}", item.location_label, item.message)
                };
                OutputEvent::new(severity, OutputSource::Compiler, msg)
                    .with_optional_location(location)
            })
            .collect();
        if !batch.is_empty() {
            self.push_output_batch_coalesced(batch);
        }
    }

    /// Publish the same draft used by the estimate without committing workspace defaults.
    pub(crate) fn publish_html5_with_settings(
        &mut self,
        output: &std::path::Path,
        runtime_dir: &std::path::Path,
        settings: &crate::workspace::PublishPrefs,
    ) -> bool {
        self.publish_html5_atomic(output, runtime_dir, settings)
    }

    pub(crate) fn publish_desktop_app_with_settings(
        &mut self,
        fallback_parent: Option<&std::path::Path>,
        runtime_dir: &std::path::Path,
        settings: &crate::workspace::PublishPrefs,
    ) -> bool {
        use crate::output_pipeline::{OutputLocation, OutputSeverity, OutputSource};

        self.finish_edits();
        let mut resolved_project = self
            .project_with_hydrated_includes()
            .with_resolved_script_language()
            .into_owned();
        resolved_project.set_html_render_mode(settings.render_mode);
        let options = match settings.export_options(&resolved_project) {
            Ok(options) => options,
            Err(error) => {
                let _ = self.message_event(
                    OutputSeverity::Error,
                    OutputSource::Publish,
                    format!("Publish failed: {error}"),
                    None,
                );
                self.show_panel(PanelId::Output);
                return false;
            }
        };
        let target_dir = if let Some(folder) = settings
            .desktop_app_folder
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            PathBuf::from(folder)
        } else if let Some(parent) = fallback_parent {
            parent.to_path_buf()
        } else if let Some(configured) = self.configured_publish_folder() {
            configured
        } else if let Some(folder) = rfd::FileDialog::new()
            .set_title("Choose desktop app output folder")
            .pick_folder()
        {
            folder
        } else {
            return false;
        };
        match neoflash_export::export_desktop_app(
            &resolved_project,
            &target_dir,
            runtime_dir,
            &options,
            &settings.publish_html_config(),
        ) {
            Ok(app_path) => {
                let _ = self.message_event(
                    OutputSeverity::Info,
                    OutputSource::Publish,
                    format!("Desktop app exported: {}", app_path.display()),
                    Some(OutputLocation::for_file(app_path)),
                );
                true
            }
            Err(e) => {
                let _ = self.message_event(
                    OutputSeverity::Error,
                    OutputSource::Publish,
                    format!("Desktop app export failed: {e}"),
                    Some(OutputLocation::for_file(target_dir)),
                );
                if matches!(e, neoflash_export::ExportError::Scripts(_)) {
                    self.emit_script_export_diagnostics();
                    self.show_panel(PanelId::Output);
                    self.show_panel(PanelId::CompilerErrors);
                } else {
                    self.refresh_compiler_diagnostics();
                    self.show_panel(PanelId::Output);
                }
                false
            }
        }
    }

    pub(crate) fn publish_image_sequence_with_settings(
        &mut self,
        fallback_parent: Option<&std::path::Path>,
        settings: &crate::workspace::PublishPrefs,
    ) -> bool {
        use crate::output_pipeline::{OutputLocation, OutputSeverity, OutputSource};

        self.finish_edits();
        let target_dir = if let Some(folder) = settings
            .image_sequence_folder
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            PathBuf::from(folder)
        } else if let Some(parent) = fallback_parent {
            parent.to_path_buf()
        } else if let Some(configured) = self.configured_publish_folder() {
            configured
        } else if let Some(folder) = rfd::FileDialog::new()
            .set_title("Choose image sequence / sprite sheet output folder")
            .pick_folder()
        {
            folder
        } else {
            return false;
        };
        if settings.image_sequence_sprite_sheet {
            let is_meta_file = target_dir
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| {
                    e.eq_ignore_ascii_case("json")
                        || e.eq_ignore_ascii_case("xml")
                        || e.eq_ignore_ascii_case("css")
                });
            let default_ext = settings.atlas_format.extension();
            let meta_path = if is_meta_file {
                if let Some(parent) = target_dir.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                target_dir
            } else {
                let _ = std::fs::create_dir_all(&target_dir);
                target_dir.join(format!("spritesheet.{default_ext}"))
            };
            let opts = neoflash_export::AtlasOptions {
                format: settings.atlas_format,
                image_format: settings.atlas_image_format,
                jpeg_quality: settings.jpeg_quality,
                extrude: settings.atlas_extrude,
                ..neoflash_export::AtlasOptions::default()
            };
            match neoflash_export::export_project_atlas(&self.project, &meta_path, &opts) {
                Ok(report) => {
                    let _ = self.message_event(
                        OutputSeverity::Info,
                        OutputSource::Publish,
                        format!(
                            "Sprite sheet exported: {} ({} page(s))",
                            report.image_path.display(),
                            report.page_paths.len().max(1)
                        ),
                        Some(OutputLocation::for_file(report.image_path)),
                    );
                    true
                }
                Err(e) => {
                    let _ = self.message_event(
                        OutputSeverity::Error,
                        OutputSource::Publish,
                        format!("Sprite sheet export failed: {e}"),
                        Some(OutputLocation::for_file(meta_path)),
                    );
                    self.show_panel(PanelId::Output);
                    false
                }
            }
        } else {
            let _ = std::fs::create_dir_all(&target_dir);
            let mut opts = neoflash_export::ImageExportOptions::for_project(&self.project, 0);
            opts.format = settings.image_sequence_format;
            opts.jpeg_quality = settings.jpeg_quality;
            opts.range = neoflash_export::ImageExportRange::WholeMovie;
            opts.symbol_id = None;
            let out_file = target_dir.join(format!("frame.{}", opts.format.extension()));
            match neoflash_export::export_project_image(&self.project, &out_file, &opts) {
                Ok(report) => {
                    let shown_path = report
                        .files
                        .first()
                        .cloned()
                        .unwrap_or_else(|| out_file.clone());
                    let shown = shown_path.display().to_string();
                    let _ = self.message_event(
                        OutputSeverity::Info,
                        OutputSource::Publish,
                        format!("Image sequence exported: {shown}"),
                        Some(OutputLocation::for_file(shown_path)),
                    );
                    true
                }
                Err(e) => {
                    let _ = self.message_event(
                        OutputSeverity::Error,
                        OutputSource::Publish,
                        format!("Image sequence export failed: {e}"),
                        Some(OutputLocation::for_file(out_file)),
                    );
                    self.show_panel(PanelId::Output);
                    false
                }
            }
        }
    }

    pub(crate) fn publish_svg_with_settings(
        &mut self,
        fallback_parent: Option<&std::path::Path>,
        settings: &crate::workspace::PublishPrefs,
    ) -> bool {
        use crate::output_pipeline::{OutputLocation, OutputSeverity, OutputSource};

        self.finish_edits();
        let svg_path = if let Some(file) = settings
            .svg_file
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            PathBuf::from(file)
        } else if let Some(parent) = fallback_parent {
            parent.join("animation.svg")
        } else if let Some(configured) = self.configured_publish_folder() {
            configured.join("animation.svg")
        } else if let Some(file) = rfd::FileDialog::new()
            .set_title("Export SVG animation")
            .add_filter("SVG", &["svg"])
            .set_file_name("animation.svg")
            .save_file()
        {
            file
        } else {
            return false;
        };
        if let Some(parent) = svg_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let opts = neoflash_export::SvgOptions::default();
        match neoflash_export::export_project_svg(&self.project, &svg_path, &opts) {
            Ok(report) => {
                let shown_path = report
                    .files
                    .first()
                    .cloned()
                    .unwrap_or_else(|| svg_path.clone());
                let shown = shown_path.display().to_string();
                let _ = self.message_event(
                    OutputSeverity::Info,
                    OutputSource::Publish,
                    format!("SVG exported: {shown}"),
                    Some(OutputLocation::for_file(shown_path)),
                );
                true
            }
            Err(e) => {
                let _ = self.message_event(
                    OutputSeverity::Error,
                    OutputSource::Publish,
                    format!("SVG export failed: {e}"),
                    Some(OutputLocation::for_file(svg_path)),
                );
                self.show_panel(PanelId::Output);
                false
            }
        }
    }

    /// Route every enabled publish checkbox through the same target implementations. Interactive
    /// Publish may open a target-specific chooser; publish-on-save never interrupts saving and
    /// instead reports a missing destination for that target.
    pub(crate) fn publish_enabled_targets_with_settings(
        &mut self,
        shared_output: Option<&std::path::Path>,
        runtime_dir: &std::path::Path,
        settings: &crate::workspace::PublishPrefs,
        allow_target_dialogs: bool,
    ) -> bool {
        use crate::output_pipeline::{OutputSeverity, OutputSource};

        fn configured(value: &Option<String>) -> bool {
            value
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
        }

        if !settings.has_any_enabled_target() {
            let _ = self.message_event(
                OutputSeverity::Warning,
                OutputSource::Publish,
                "Publish skipped: no publish target is enabled.",
                None,
            );
            return false;
        }

        let mut ok = true;
        if settings.html5_enabled {
            if let Some(output) = shared_output {
                ok &= self.publish_html5_with_settings(output, runtime_dir, settings);
            } else {
                let _ = self.message_event(
                    OutputSeverity::Warning,
                    OutputSource::Publish,
                    "HTML5 publish skipped: set an Output folder in Preferences ▸ Publish.",
                    None,
                );
                ok = false;
            }
        }
        if settings.desktop_app_enabled {
            if allow_target_dialogs
                || shared_output.is_some()
                || configured(&settings.desktop_app_folder)
            {
                ok &= self.publish_desktop_app_with_settings(shared_output, runtime_dir, settings);
            } else {
                let _ = self.message_event(
                    OutputSeverity::Warning,
                    OutputSource::Publish,
                    "Desktop app publish skipped: set its output folder or the shared Output folder.",
                    None,
                );
                ok = false;
            }
        }
        if settings.video_enabled {
            if allow_target_dialogs || shared_output.is_some() || configured(&settings.video_file) {
                ok &= self.publish_video_with_settings(shared_output, settings);
            } else {
                let _ = self.message_event(
                    OutputSeverity::Warning,
                    OutputSource::Publish,
                    "Video publish skipped: set its output file or the shared Output folder.",
                    None,
                );
                ok = false;
            }
        }
        if settings.image_sequence_enabled {
            if allow_target_dialogs
                || shared_output.is_some()
                || configured(&settings.image_sequence_folder)
            {
                ok &= self.publish_image_sequence_with_settings(shared_output, settings);
            } else {
                let _ = self.message_event(
                    OutputSeverity::Warning,
                    OutputSource::Publish,
                    "Image sequence publish skipped: set its output folder or the shared Output folder.",
                    None,
                );
                ok = false;
            }
        }
        if settings.svg_enabled {
            if allow_target_dialogs || shared_output.is_some() || configured(&settings.svg_file) {
                ok &= self.publish_svg_with_settings(shared_output, settings);
            } else {
                let _ = self.message_event(
                    OutputSeverity::Warning,
                    OutputSource::Publish,
                    "SVG publish skipped: set its output file or the shared Output folder.",
                    None,
                );
                ok = false;
            }
        }
        ok
    }

    pub fn export(&mut self, test: bool) {
        use crate::output_pipeline::{OutputLocation, OutputSeverity, OutputSource};

        if !self.has_document() || self.is_standalone_script_session() {
            return;
        }
        if test {
            if self.restart_debug_movie_if_commandable() {
                return;
            }
            let output = std::env::temp_dir()
                .join("neoflash-preview")
                .join(std::process::id().to_string());
            self.finish_edits();
            let resolved_project = self
                .project_with_hydrated_includes()
                .with_resolved_script_language()
                .into_owned();
            let preview_config = self.workspace.publish.preview_html_config();
            let preview_options = self
                .workspace
                .publish
                .options
                .clone()
                .validated()
                .ok()
                .filter(|opts| !matches!(opts.size, neoflash_export::SizeLevel::Shared(_)))
                .unwrap_or_default();
            match neoflash_export::export_project_with_config(
                &resolved_project,
                &output,
                &Self::runtime_dir(),
                &preview_options,
                &preview_config,
            ) {
                Ok(index) => {
                    self.begin_preview_output_session(format!(
                        "Test Movie: {}",
                        resolved_project.name
                    ));
                    let _ = self.message_event(
                        OutputSeverity::Info,
                        OutputSource::Publish,
                        format!("HTML5 exported: {}", index.display()),
                        Some(OutputLocation::for_file(index.clone())),
                    );
                    self.log_export_report(&resolved_project);
                    let warnings = neoflash_export::export_warnings(&resolved_project);
                    self.emit_export_warnings(warnings);
                    self.show_panel(PanelId::Output);
                    if !cfg!(test)
                        && let Err(e) = crate::preview::open_preview_in_browser(&index)
                            .map(|_| ())
                            .or_else(|_| crate::platform_open::open_file(&index))
                    {
                        let _ = self.message_event(
                            OutputSeverity::Warning,
                            OutputSource::Publish,
                            format!("Cannot open preview: {e}"),
                            Some(OutputLocation::for_file(index)),
                        );
                    }
                }
                Err(e) => {
                    let _ = self.message_event(
                        OutputSeverity::Error,
                        OutputSource::Publish,
                        format!("Export failed: {e}"),
                        Some(OutputLocation::for_file(output)),
                    );
                    if matches!(e, neoflash_export::ExportError::Scripts(_)) {
                        self.emit_script_export_diagnostics();
                        self.show_panel(PanelId::Output);
                        self.show_panel(PanelId::CompilerErrors);
                    } else {
                        self.refresh_compiler_diagnostics();
                        self.show_panel(PanelId::Output);
                    }
                }
            }
            return;
        }
        self.publish_document_with_runtime(&Self::runtime_dir());
    }

    pub(crate) fn publish_document_with_runtime(&mut self, runtime_dir: &std::path::Path) {
        use crate::output_pipeline::{OutputSeverity, OutputSource};

        let settings = self.effective_publish_preferences();
        if !settings.has_any_enabled_target() {
            let _ = self.message_event(
                OutputSeverity::Warning,
                OutputSource::Publish,
                "Select at least one publish target in Publish settings.",
                None,
            );
            self.open_publish_settings();
            return;
        }
        let needs_shared_output = settings.html5_enabled
            || (settings.desktop_app_enabled && settings.desktop_app_folder.is_none())
            || (settings.image_sequence_enabled && settings.image_sequence_folder.is_none())
            || (settings.svg_enabled && settings.svg_file.is_none());
        if !needs_shared_output {
            self.publish_enabled_targets_with_settings(None, runtime_dir, &settings, true);
            return;
        }
        let output = if let Some(configured) = self.configured_publish_folder() {
            configured
        } else if !needs_shared_output {
            PathBuf::new()
        } else {
            let Some(base) = rfd::FileDialog::new()
                .set_title("Choose publish output parent folder")
                .pick_folder()
            else {
                return;
            };
            crate::output_paths::project_output_dir(&base, &self.project)
        };
        let parent_opt = (!output.as_os_str().is_empty()).then_some(output.as_path());
        self.publish_enabled_targets_with_settings(parent_opt, runtime_dir, &settings, true);
    }
    /// Export the document (or a specific `symbol_id`) to a Bodymovin / Lottie `.json` file.
    #[cfg(test)]
    pub fn export_lottie_to_path(
        &mut self,
        path: &std::path::Path,
        symbol_id: Option<&str>,
    ) -> Result<neoflash_export::LottieExportReport, String> {
        let options = neoflash_export::LottieOptions {
            symbol: symbol_id.map(str::to_owned),
            include_background: symbol_id.is_none(),
            ..neoflash_export::LottieOptions::default()
        };
        self.export_lottie_with_options_to_path(path, &options)
    }
    pub fn export_lottie_dialog(&mut self, symbol_id: Option<&str>) {
        self.open_export_lottie(symbol_id);
    }

    /// Export the document (or a specific `symbol_id`) to an Adobe After Effects interchange
    /// package (`.aep` + `.aepx` + self-building `.jsx` ExtendScript + `<stem>_Footage/`) (`T-246`).
    pub fn export_aep_dialog(&mut self, symbol_id: Option<&str>, dest: Option<PathBuf>) -> bool {
        if !self.has_document() {
            return false;
        }
        self.finish_edits();
        let raw_name = symbol_id
            .and_then(|sid| self.project.symbol(sid))
            .map(|s| s.name.as_str())
            .unwrap_or(self.project.name.as_str());
        let clean_stem: String = raw_name
            .trim()
            .chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | ':' | '\0') {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let stem = if clean_stem.is_empty() {
            "composition"
        } else {
            clean_stem.as_str()
        };
        let default_name = format!("{stem}.aep");
        let aep_path = if let Some(path) = dest {
            path
        } else if let Some(file) = rfd::FileDialog::new()
            .set_title("Export After Effects Project (.aep)")
            .add_filter("After Effects Project", &["aep", "aepx", "jsx"])
            .set_file_name(&default_name)
            .save_file()
        {
            file
        } else {
            return false;
        };
        let opts = neoflash_export::AepOptions {
            symbol: symbol_id.map(str::to_owned),
            jpeg_quality: self.workspace.publish.jpeg_quality,
            base_dir: self
                .path
                .as_ref()
                .and_then(|p| p.parent())
                .filter(|p| !p.as_os_str().is_empty())
                .map(std::path::Path::to_path_buf),
            ..neoflash_export::AepOptions::default()
        };
        match neoflash_export::export_project_aep(&self.project, &aep_path, &opts) {
            Ok(report) => {
                let aep_loc =
                    crate::output_pipeline::OutputLocation::for_file(report.output_path.clone());
                let mut seen_diags = std::collections::HashSet::new();
                for diag in &report.diagnostics {
                    let line = format!("[aep:{}] {}: {}", diag.code, diag.location, diag.message);
                    if seen_diags.insert(line.clone()) {
                        self.push_output_coalesced(
                            crate::output_pipeline::OutputEvent::new(
                                crate::output_pipeline::OutputSeverity::Warning,
                                crate::output_pipeline::OutputSource::Publish,
                                line,
                            )
                            .with_location(aep_loc.clone()),
                        );
                    }
                }
                let _ = self.message_event(
                    crate::output_pipeline::OutputSeverity::Info,
                    crate::output_pipeline::OutputSource::Publish,
                    format!(
                        "After Effects exported: {} ({} comp(s), {} vector layer(s), {} expression(s), {} footage)",
                        report.output_path.display(),
                        report.composition_count,
                        report.vector_shape_layer_count,
                        report.expression_count,
                        report.footage_files.len()
                    ),
                    Some(aep_loc),
                );
                true
            }
            Err(e) => {
                let _ = self.message_event(
                    crate::output_pipeline::OutputSeverity::Error,
                    crate::output_pipeline::OutputSource::Publish,
                    format!("After Effects export failed: {e}"),
                    Some(crate::output_pipeline::OutputLocation::for_file(aep_path)),
                );
                self.show_panel(PanelId::Output);
                false
            }
        }
    }

    /// Export a packed Texture Atlas (`JSON-Hash`, `JSON-Array`, `Starling XML`, `EaselJS`, `CSS`) (`T-250`).
    pub fn export_texture_atlas_dialog(&mut self, dest: Option<PathBuf>) -> bool {
        if !self.has_document() {
            return false;
        }
        self.finish_edits();
        let fmt = self.workspace.publish.atlas_format;
        let default_ext = fmt.extension();
        let clean_stem: String = self
            .project
            .name
            .trim()
            .chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | ':' | '\0') {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let stem = if clean_stem.is_empty() {
            "spritesheet"
        } else {
            clean_stem.as_str()
        };
        let default_name = format!("{stem}.{default_ext}");
        let meta_path = if let Some(path) = dest {
            path
        } else if let Some(file) = rfd::FileDialog::new()
            .set_title("Export Texture Atlas")
            .add_filter("Atlas Metadata", &["json", "xml", "css"])
            .set_file_name(&default_name)
            .save_file()
        {
            file
        } else {
            return false;
        };
        let inferred_fmt = match meta_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref()
        {
            Some("xml") => neoflash_export::AtlasFormat::StarlingXml,
            Some("css") => neoflash_export::AtlasFormat::Css,
            _ => fmt,
        };
        let opts = neoflash_export::AtlasOptions {
            format: inferred_fmt,
            image_format: self.workspace.publish.atlas_image_format,
            jpeg_quality: self.workspace.publish.jpeg_quality,
            extrude: self.workspace.publish.atlas_extrude,
            ..neoflash_export::AtlasOptions::default()
        };
        match neoflash_export::export_project_atlas(&self.project, &meta_path, &opts) {
            Ok(report) => {
                let _ = self.message_event(
                    crate::output_pipeline::OutputSeverity::Info,
                    crate::output_pipeline::OutputSource::Publish,
                    format!(
                        "Texture atlas exported: {} ({} frame(s), {} page(s))",
                        report.image_path.display(),
                        report.frame_count,
                        report.page_paths.len().max(1)
                    ),
                    Some(crate::output_pipeline::OutputLocation::for_file(
                        report.image_path,
                    )),
                );
                true
            }
            Err(e) => {
                let _ = self.message_event(
                    crate::output_pipeline::OutputSeverity::Error,
                    crate::output_pipeline::OutputSource::Publish,
                    format!("Texture atlas export failed: {e}"),
                    Some(crate::output_pipeline::OutputLocation::for_file(meta_path)),
                );
                self.show_panel(PanelId::Output);
                false
            }
        }
    }

    /// Export the active project to a standalone Flash Movie (`.swf`) via `publish_swf_to_path` (`T-192`).
    pub fn export_swf_dialog(&mut self, dest: Option<PathBuf>) -> bool {
        if !self.has_document() {
            return false;
        }
        let default_profile = match self
            .project
            .with_resolved_script_language()
            .effective_script_language()
        {
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
                neoflash_export::SwfPublishProfile::FlashPlayer8Avm1
            }
            ScriptLanguage::ActionScript3
            | ScriptLanguage::JavaScript
            | ScriptLanguage::TypeScript => neoflash_export::SwfPublishProfile::FlashPlayer10Avm2,
        };
        self.export_swf_profile_dialog(default_profile, false, dest)
    }

    /// Export the active project to an explicit Flash Player `.swf` target profile (`T-311`).
    pub fn export_swf_profile_dialog(
        &mut self,
        profile: neoflash_export::SwfPublishProfile,
        scriptless_mode: bool,
        dest: Option<PathBuf>,
    ) -> bool {
        if !self.has_document() {
            return false;
        }
        self.finish_edits();
        let default_name = format!(
            "{}.swf",
            self.project.name.trim().replace(['/', '\\', ':'], "_")
        );
        let profile_label = match profile {
            neoflash_export::SwfPublishProfile::FlashPlayer8Avm1 => "Flash Player 8, AVM1",
            neoflash_export::SwfPublishProfile::FlashPlayer9Avm2 => "Flash Player 9, AVM2",
            neoflash_export::SwfPublishProfile::FlashPlayer10Avm2 => "Flash Player 10, AVM2",
        };
        let title = if scriptless_mode {
            format!("Export SWF ({profile_label}, Visuals Only)")
        } else {
            format!("Export Flash Movie ({profile_label})")
        };
        let swf_path = if let Some(path) = dest {
            path
        } else if let Some(file) = rfd::FileDialog::new()
            .set_title(&title)
            .add_filter("SWF Movie", &["swf"])
            .set_file_name(&default_name)
            .save_file()
        {
            file
        } else {
            return false;
        };
        // The current SWF writer cannot emit a proven editable font table. The
        // editor therefore chooses its supported visual conversion explicitly;
        // rich/input text that cannot be outlined faithfully still blocks in
        // compatibility preflight instead of disappearing from the movie.
        let opts = neoflash_export::SwfPublishOptions {
            profile,
            scriptless_mode,
            outline_text: true,
            ..neoflash_export::SwfPublishOptions::default()
        };
        let hydrated = self.project_with_hydrated_includes();
        let resolved_project = hydrated.with_resolved_script_language();
        match neoflash_export::publish_swf_to_path(&resolved_project, &swf_path, &opts) {
            Ok(report) => {
                let _ = self.message_event(
                    crate::output_pipeline::OutputSeverity::Info,
                    crate::output_pipeline::OutputSource::Publish,
                    format!(
                        "SWF exported ({}, {} compatibility items): {}",
                        report.profile.label(),
                        report.items.len(),
                        swf_path.display()
                    ),
                    Some(crate::output_pipeline::OutputLocation::for_file(swf_path)),
                );
                true
            }
            Err(e) => {
                let _ = self.message_event(
                    crate::output_pipeline::OutputSeverity::Error,
                    crate::output_pipeline::OutputSource::Publish,
                    format!("SWF export failed: {e}"),
                    Some(crate::output_pipeline::OutputLocation::for_file(swf_path)),
                );
                self.show_panel(PanelId::Output);
                false
            }
        }
    }

    /// Export the active project to a versioned XFL/FLA or Legacy Binary FLA target (`T-188`, `T-189`).
    pub fn export_fla_profile_dialog(
        &mut self,
        profile: neoflash_export::FlaTargetProfile,
        dest: Option<PathBuf>,
    ) -> bool {
        if !self.has_document() || self.is_standalone_script_session() {
            return false;
        }
        self.finish_edits();
        let stem = self.project.name.trim().replace(['/', '\\', ':'], "_");
        let target_path = if let Some(path) = dest {
            path
        } else if matches!(profile, neoflash_export::FlaTargetProfile::UncompressedXfl) {
            let Some(folder) = rfd::FileDialog::new()
                .set_title("Choose output folder for Uncompressed XFL")
                .pick_folder()
            else {
                return false;
            };
            folder.join(format!("{stem}-xfl"))
        } else {
            let default_name = format!("{stem}.fla");
            let Some(file) = rfd::FileDialog::new()
                .set_title(format!("Export {}", profile.label()))
                .add_filter("Flash Document (.fla)", &["fla"])
                .set_file_name(&default_name)
                .save_file()
            else {
                return false;
            };
            file
        };
        let opts = neoflash_export::FlaExportOptions::default();
        let hydrated = self.project_with_hydrated_includes();
        let resolved_project = hydrated.with_resolved_script_language();
        match neoflash_export::export_fla_with_profile(
            &resolved_project,
            profile,
            &opts,
            &target_path,
        ) {
            Ok(report) => {
                let _ = self.message_event(
                    crate::output_pipeline::OutputSeverity::Info,
                    crate::output_pipeline::OutputSource::Publish,
                    format!(
                        "Exported {} ({} compatibility items): {}",
                        profile.label(),
                        report.items.len(),
                        target_path.display()
                    ),
                    Some(crate::output_pipeline::OutputLocation::for_file(
                        target_path,
                    )),
                );
                true
            }
            Err(e) => {
                let _ = self.message_event(
                    crate::output_pipeline::OutputSeverity::Error,
                    crate::output_pipeline::OutputSource::Publish,
                    format!("FLA/XFL export failed ({}): {e}", profile.label()),
                    Some(crate::output_pipeline::OutputLocation::for_file(
                        target_path,
                    )),
                );
                self.show_panel(PanelId::Output);
                false
            }
        }
    }

    /// Execute a JSFL script in the deterministic sandboxed `JsflSandbox` and commit any project
    /// modifications as a single Undo step (`T-187`).
    pub fn run_jsfl_script(
        &mut self,
        script_name: &str,
        source: &str,
    ) -> Result<neoflash_core::JsflResult, String> {
        if !self.has_document() {
            return Err("No open document.".into());
        }
        self.finish_edits();
        let mut state = neoflash_core::JsflAuthoringState::for_project(&self.project);
        state.active_symbol = self.symbol_id().to_owned();
        state.current_layer = self.layer;
        state.current_frame = self.frame;
        state.selected_element_ids = self
            .selection
            .targets
            .iter()
            .map(|t| t.element.clone())
            .collect();
        state.selected_layers = self.target_layer_indices(self.layer);
        if let Some(lib_id) = &self.library_selected {
            state.selected_library_items = vec![lib_id.clone()];
        }
        let sandbox = neoflash_core::JsflSandbox::new();
        match sandbox.execute(&self.project, &state, script_name, source) {
            Ok(result) => {
                for line in &result.output_lines {
                    let _ = self.message_event(
                        crate::output_pipeline::OutputSeverity::Trace,
                        crate::output_pipeline::OutputSource::Trace,
                        line.clone(),
                        None,
                    );
                }
                if result.mutations > 0 && result.project != self.project {
                    let undo_label = if result.undo_label.trim().is_empty() {
                        format!("Run JSFL: {script_name}")
                    } else {
                        result.undo_label.clone()
                    };
                    let next_project = result.project.clone();
                    self.transact_named(&undo_label, move |editor| {
                        editor.project = next_project;
                    });
                }
                let _ = self.message_event(
                    crate::output_pipeline::OutputSeverity::Info,
                    crate::output_pipeline::OutputSource::Runtime,
                    format!(
                        "JSFL '{script_name}' completed ({} step(s), {} mutation(s)).",
                        result.steps_executed, result.mutations
                    ),
                    None,
                );
                Ok(result)
            }
            Err(diag) => {
                let msg = format!(
                    "JSFL error in {} ({}:{}): {}",
                    diag.file, diag.line, diag.column, diag.message
                );
                let loc = crate::output_pipeline::OutputLocation::for_file(&diag.file)
                    .with_line_col(diag.line as usize, diag.column as usize);
                let _ = self.message_event(
                    crate::output_pipeline::OutputSeverity::Error,
                    crate::output_pipeline::OutputSource::Runtime,
                    msg.clone(),
                    Some(loc),
                );
                self.show_panel(PanelId::Output);
                Err(msg)
            }
        }
    }

    /// Pick and run a `.jsfl` script file on the active document (`T-187`).
    pub fn run_jsfl_script_dialog(&mut self, path: Option<PathBuf>) -> bool {
        if !self.has_document() {
            return false;
        }
        let script_path = if let Some(p) = path {
            p
        } else if let Some(file) = rfd::FileDialog::new()
            .set_title("Run JSFL Script (.jsfl)")
            .add_filter("JSFL Script", &["jsfl", "js"])
            .pick_file()
        {
            file
        } else {
            return false;
        };
        let script_name = script_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("script.jsfl")
            .to_owned();
        match std::fs::read_to_string(&script_path) {
            Ok(source) => self.run_jsfl_script(&script_name, &source).is_ok(),
            Err(e) => {
                let _ = self.message_event(
                    crate::output_pipeline::OutputSeverity::Error,
                    crate::output_pipeline::OutputSource::Runtime,
                    format!("Cannot read JSFL script {}: {e}", script_path.display()),
                    Some(crate::output_pipeline::OutputLocation::for_file(
                        script_path,
                    )),
                );
                self.show_panel(PanelId::Output);
                false
            }
        }
    }
    /// Single-scene copy of `self.project` rooted at the currently edited scene (`Control ▸ Test scene`).
    pub(crate) fn test_scene_project(&self) -> Project {
        let mut cloned = self.project_with_hydrated_includes().into_owned();
        cloned.script_language = Some(self.project.effective_script_language());
        let scene_id = self.current_scene().to_owned();
        cloned
            .symbols
            .retain(|s| s.id == scene_id || !self.project.is_scene(&s.id));
        cloned.root = scene_id.clone();
        cloned.scenes.clear();
        let doc_class = cloned.document_class.clone();
        if let Some(root_sym) = cloned.symbol_mut(&scene_id) {
            root_sym.folder.clear();
            root_sym.linkage_id = None;
            if doc_class.is_some()
                && root_sym.class_name.is_some()
                && root_sym.class_name != doc_class
            {
                root_sym.class_name = None;
            }
        }
        crate::timeline_commands::upgrade_all_format_versions(&mut cloned);
        cloned
    }

    /// `Control ▸ Test scene` (`⌥⌘↵`): export and open only the currently edited scene in the browser.
    pub fn test_scene(&mut self) {
        if !self.has_document() || self.is_standalone_script_session() {
            return;
        }
        self.finish_edits();
        let project = self.test_scene_project();
        let output = std::env::temp_dir()
            .join("neoflash-preview-scene")
            .join(std::process::id().to_string());
        let preview_config = self.workspace.publish.preview_html_config();
        let preview_options = self
            .workspace
            .publish
            .options
            .clone()
            .validated()
            .ok()
            .filter(|opts| !matches!(opts.size, neoflash_export::SizeLevel::Shared(_)))
            .unwrap_or_default();
        match neoflash_export::export_project_with_config(
            &project,
            &output,
            &Self::runtime_dir(),
            &preview_options,
            &preview_config,
        ) {
            Ok(index) => {
                let scene_name = self
                    .project
                    .symbol(self.current_scene())
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                let banner = if scene_name.is_empty() {
                    format!("Test Scene: {}", project.name)
                } else {
                    format!("Test Scene: {} ({scene_name})", project.name)
                };
                self.begin_preview_output_session(banner);
                let _ = self.message_event(
                    crate::output_pipeline::OutputSeverity::Info,
                    crate::output_pipeline::OutputSource::Publish,
                    format!("HTML5 scene exported: {}", index.display()),
                    Some(crate::output_pipeline::OutputLocation::for_file(
                        index.clone(),
                    )),
                );
                self.log_export_report(&project);
                let warnings = neoflash_export::export_warnings(&project);
                self.emit_export_warnings(warnings);
                self.show_panel(PanelId::Output);
                if !cfg!(test)
                    && let Err(e) = crate::preview::open_preview_in_browser(&index)
                        .map(|_| ())
                        .or_else(|_| crate::platform_open::open_file(&index))
                {
                    let _ = self.message_event(
                        crate::output_pipeline::OutputSeverity::Warning,
                        crate::output_pipeline::OutputSource::Publish,
                        format!("Cannot open preview: {e}"),
                        Some(crate::output_pipeline::OutputLocation::for_file(index)),
                    );
                }
            }
            Err(e) => {
                let _ = self.message_event(
                    crate::output_pipeline::OutputSeverity::Error,
                    crate::output_pipeline::OutputSource::Publish,
                    format!("Export failed: {e}"),
                    Some(crate::output_pipeline::OutputLocation::for_file(output)),
                );
                if matches!(e, neoflash_export::ExportError::Scripts(_)) {
                    self.emit_script_export_diagnostics();
                    self.show_panel(PanelId::Output);
                    self.show_panel(PanelId::CompilerErrors);
                } else {
                    self.refresh_compiler_diagnostics();
                    self.show_panel(PanelId::Output);
                }
            }
        }
    }

    /// Export the active project to a native Apple Xcode + Swift 6 (`@MainActor`) project (`T-455`),
    /// preserving any developer-authored `App/` Swift files on re-export while regenerating `Generated/`
    /// and appending newly declared `SwiftHostProtocol` stubs to `App/AppSwiftHost.swift`.
    pub fn export_xcode_dialog(&mut self, dest_dir: Option<PathBuf>) -> bool {
        if !self.has_document() || self.is_standalone_script_session() {
            return false;
        }
        self.finish_edits();

        let target_name = neoflash_export::pascal_project_name(&self.project.name);
        let out_dir = if let Some(explicit_dir) = dest_dir {
            explicit_dir
        } else {
            let mut picker =
                rfd::FileDialog::new().set_title("Choose output folder for Xcode Project (Swift)");
            if let Some(doc_dir) = self.path.as_deref().and_then(std::path::Path::parent) {
                picker = picker.set_directory(doc_dir);
            }
            let Some(chosen_folder) = picker.pick_folder() else {
                return false;
            };
            // If the user selected an existing exported Xcode project folder directly (containing
            // `<Target>.xcodeproj` or `Generated/` + `App/`), update it in-place instead of nesting!
            let xcodeproj_name = format!("{target_name}.xcodeproj");
            let looks_like_existing_export = chosen_folder.join(&xcodeproj_name).exists()
                || (chosen_folder.join("Generated").is_dir() && chosen_folder.join("App").is_dir());
            if looks_like_existing_export {
                chosen_folder
            } else {
                chosen_folder.join(&target_name)
            }
        };

        let opts = neoflash_export::XcodeExportOptions {
            jpeg_quality: self.workspace.publish.jpeg_quality,
            base_dir: self
                .path
                .as_deref()
                .and_then(std::path::Path::parent)
                .map(PathBuf::from),
            ..neoflash_export::XcodeExportOptions::default()
        };

        let hydrated = self.project_with_hydrated_includes();
        let resolved_project = hydrated.with_resolved_script_language();
        match neoflash_export::export_xcode_project(&resolved_project, &out_dir, &opts, None) {
            Ok(report) => {
                let proj_loc =
                    crate::output_pipeline::OutputLocation::for_file(report.xcodeproj_path.clone());
                for item in &report.preflight_items {
                    let sev = match item.severity {
                        neoflash_export::XcodePreflightSeverity::Info => {
                            crate::output_pipeline::OutputSeverity::Info
                        }
                        neoflash_export::XcodePreflightSeverity::Warning => {
                            crate::output_pipeline::OutputSeverity::Warning
                        }
                    };
                    let _ = self.message_event(
                        sev,
                        crate::output_pipeline::OutputSource::Publish,
                        format!(
                            "[Xcode {}] ({}): {}",
                            item.code, item.location, item.message
                        ),
                        Some(proj_loc.clone()),
                    );
                }
                let preserved_note = if report.preserved_app_files.is_empty() {
                    String::new()
                } else {
                    format!(
                        ", {} App/ file(s) preserved",
                        report.preserved_app_files.len()
                    )
                };
                let appended_note = if report.appended_host_methods.is_empty() {
                    String::new()
                } else {
                    format!(
                        ", appended SwiftHost stub(s): {}",
                        report.appended_host_methods.join(", ")
                    )
                };
                let _ = self.message_event(
                    crate::output_pipeline::OutputSeverity::Info,
                    crate::output_pipeline::OutputSource::Publish,
                    format!(
                        "Xcode project exported ({} symbol(s){preserved_note}{appended_note}): {}",
                        report.symbol_count,
                        report.xcodeproj_path.display()
                    ),
                    Some(proj_loc),
                );
                self.show_panel(PanelId::Output);
                true
            }
            Err(err) => {
                let _ = self.message_event(
                    crate::output_pipeline::OutputSeverity::Error,
                    crate::output_pipeline::OutputSource::Publish,
                    format!("Xcode export failed: {err}"),
                    Some(crate::output_pipeline::OutputLocation::for_file(out_dir)),
                );
                self.show_panel(PanelId::Output);
                false
            }
        }
    }
}
