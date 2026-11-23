use super::*;

impl Editor {
    pub(super) fn code_fingerprint(&self, script: &Script) -> u64 {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        script.source.hash(&mut hash);
        std::mem::discriminant(&script.language).hash(&mut hash);
        self.project.script_language.hash(&mut hash);
        self.project.also_compile_compatible_scripts.hash(&mut hash);
        self.project.root.hash(&mut hash);
        self.project.document_class.hash(&mut hash);
        for unit in &self.project.sources {
            unit.id.hash(&mut hash);
            unit.path.hash(&mut hash);
            unit.source.hash(&mut hash);
            std::mem::discriminant(&unit.language).hash(&mut hash);
        }
        // Includes are compilation inputs even when the visible source has not changed.
        // Binary media contents do not affect the language service or compiler diagnostics.
        for resource in &self.project.resources {
            resource.id.hash(&mut hash);
            resource.path.hash(&mut hash);
            if resource.path.ends_with(".as") {
                resource.data_base64.hash(&mut hash);
            }
        }
        self.standalone_script.hash(&mut hash);
        self.hydrated_project_and_fingerprint().1.hash(&mut hash);
        for symbol in &self.project.symbols {
            symbol.id.hash(&mut hash);
            std::mem::discriminant(&symbol.kind).hash(&mut hash);
            symbol.class_name.hash(&mut hash);
            symbol.linkage_id.hash(&mut hash);
            for entry in &symbol.timeline.scripts {
                entry.frame.hash(&mut hash);
                std::mem::discriminant(&entry.script.language).hash(&mut hash);
                entry.script.source.hash(&mut hash);
            }
            for element in symbol
                .timeline
                .layers
                .iter()
                .flat_map(|layer| &layer.keyframes)
                .flat_map(|key| &key.elements)
            {
                element.id.hash(&mut hash);
                element.name.hash(&mut hash);
                std::mem::discriminant(&element.kind).hash(&mut hash);
                if let neoflash_core::ElementKind::Instance { symbol_id, .. } = &element.kind {
                    symbol_id.hash(&mut hash);
                }
                for entry in &element.scripts {
                    std::mem::discriminant(&entry.event).hash(&mut hash);
                    std::mem::discriminant(&entry.script.language).hash(&mut hash);
                    entry.script.source.hash(&mut hash);
                }
            }
        }
        hash.finish()
    }

    /// `announce` also writes the result to Output (explicit Check); live checks stay silent.
    pub(super) fn record_code_check(
        &mut self,
        script: &Script,
        diagnostics: Vec<Diagnostic>,
        announce: bool,
    ) {
        let success = !diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error);
        let text = if diagnostics.is_empty() {
            "Code accepted for current language support.".to_owned()
        } else {
            diagnostics
                .iter()
                .map(|d| format!("{}: {}", diagnostic_severity(d.severity).label(), d))
                .collect::<Vec<_>>()
                .join("\n")
        };
        if announce {
            self.status = text.clone();
            if diagnostics.is_empty() {
                let loc = self.output_location_for_diagnostic(None);
                self.push_output_coalesced(
                    crate::output_pipeline::OutputEvent::info(
                        crate::output_pipeline::OutputSource::Compiler,
                        text.clone(),
                    )
                    .with_optional_location(loc),
                );
            } else {
                let mut seen_batch = std::collections::HashSet::new();
                let mut batch = Vec::with_capacity(diagnostics.len());
                for d in &diagnostics {
                    let line_msg = format!("{}: {}", diagnostic_severity(d.severity).label(), d);
                    let loc = self.output_location_for_diagnostic(Some(d));
                    let severity =
                        crate::output_pipeline::OutputSeverity::from_diagnostic(d.severity);
                    if seen_batch.insert((severity, line_msg.clone())) {
                        batch.push(
                            crate::output_pipeline::OutputEvent::new(
                                severity,
                                crate::output_pipeline::OutputSource::Compiler,
                                line_msg,
                            )
                            .with_optional_location(loc),
                        );
                    }
                }
                self.push_output_batch_coalesced(batch);
            }
        }
        self.code_feedback = Some(CodeFeedback {
            target: self.actions_editor_id(),
            fingerprint: self.code_fingerprint(script),
            text,
            success,
            diagnostics,
        });
    }

    pub(crate) fn is_standalone_class_or_package_unit(unit: &SourceUnit) -> bool {
        if unit.language == ScriptLanguage::ActionScript1 {
            return false;
        }
        if crate::script_api::scan_class(unit).is_some() {
            return true;
        }
        let skeleton =
            crate::app::standalone_script::strip_comments_and_strings_for_detection(&unit.source);
        let mut brace_depth = 0usize;
        for line in skeleton.lines() {
            let trimmed = line.trim();
            if brace_depth <= 1 {
                if let Some(rest) = trimmed.strip_prefix("package")
                    && (rest.is_empty()
                        || rest.starts_with(|c: char| c.is_whitespace() || c == '{'))
                {
                    return true;
                }
                let mut rest_decl = trimmed;
                loop {
                    let mut stripped = false;
                    for kw in ["public ", "internal ", "dynamic ", "final ", "intrinsic "] {
                        if let Some(next) = rest_decl.strip_prefix(kw) {
                            rest_decl = next.trim_start();
                            stripped = true;
                        }
                    }
                    if !stripped {
                        break;
                    }
                }
                if rest_decl.starts_with("class ") || rest_decl.starts_with("interface ") {
                    return true;
                }
            }
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth = brace_depth.saturating_sub(1),
                    _ => {}
                }
            }
        }
        false
    }

    /// Check syntax of a standalone `.as` file tab (`T-412`), handling both class/package
    /// definitions (`compile_project`) and pure top-level / `#include` ActionScript files
    /// (`compile_with_program` against hydrated includes).
    pub(crate) fn check_standalone_script(&mut self, announce: bool) {
        let Some(unit) = self.project.sources.first().cloned() else {
            return;
        };
        let script = Script {
            language: unit.language,
            source: unit.source.clone(),
        };
        let (hydrated, _) = self.hydrated_project_and_fingerprint();
        let mut diagnostics = if Self::is_standalone_class_or_package_unit(&unit) {
            let mut check_proj = hydrated.into_owned();
            check_proj.script_language = Some(unit.language);
            let mut diags = neoflash_script::compile_project(&check_proj)
                .err()
                .unwrap_or_default();
            diags.extend(
                neoflash_script::check_warnings_with_path(&script, Some(&unit.path))
                    .iter()
                    .map(|warning| warning.to_diagnostic_with_source(&script.source)),
            );
            if let Some(context) = self.source_hint_context(&unit) {
                diags.extend(
                    context
                        .semantic_analysis(&script.source)
                        .diagnostics()
                        .iter()
                        .cloned()
                        .map(|mut d| {
                            if d.path.is_none() {
                                d.path = Some(unit.path.clone());
                            }
                            d
                        }),
                );
            }
            diags
        } else {
            let compile_script = Script {
                language: unit.language,
                source: hydrated
                    .sources
                    .first()
                    .map(|s| s.source.clone())
                    .unwrap_or_else(|| unit.source.clone()),
            };
            let mut check_proj = hydrated.into_owned();
            check_proj.sources.clear();
            check_proj.document_class = None;
            check_proj.script_language = Some(unit.language);
            let mut diags = match neoflash_script::compile_project(&check_proj) {
                Ok(program) => neoflash_script::compile_with_program(&compile_script, &program)
                    .err()
                    .unwrap_or_default(),
                Err(_) => neoflash_script::compile(&compile_script)
                    .err()
                    .unwrap_or_default(),
            };
            diags.extend(
                neoflash_script::check_warnings(&script)
                    .iter()
                    .map(|warning| warning.to_diagnostic_with_source(&script.source)),
            );
            if let Some(context) =
                HintContext::new(&self.project, unit.language, ThisKind::MovieClip)
            {
                diags.extend(
                    context
                        .semantic_analysis(&script.source)
                        .diagnostics()
                        .iter()
                        .cloned(),
                );
            }
            diags
        };
        diagnostics.dedup();
        self.record_code_check(&script, diagnostics, announce);
    }

    pub(crate) fn check_action_source(&mut self, script: &Script, announce: bool) {
        // R-102 item 7: `Check script` checks the open script in its own language (`script.language`).
        let proj_lang = self.project.effective_script_language();
        let is_skipped_foreign = self.project.script_disposition_for(
            proj_lang,
            self.project.also_compile_compatible_scripts,
            script.language,
        ) == neoflash_core::ScriptDisposition::SkippedForeign;
        let mut probe_project = self.project.clone();
        probe_project.script_language = Some(script.language);
        if is_skipped_foreign {
            probe_project.document_class = None;
        }
        if script.language.is_actionscript()
            && script.source.contains("include")
            && let Some(first_sym) = probe_project.symbols.first_mut()
        {
            first_sym.timeline.scripts.push(neoflash_core::FrameScript {
                layer: None,
                frame: 0,
                script: script.clone(),
            });
        }
        let mut lang_project = self
            .hydrate_arbitrary_project_with_editor_context(&probe_project)
            .0
            .into_owned();
        lang_project.symbols = self.project.symbols.clone();

        let compile_script_against_resources = |proj: &neoflash_core::Project| {
            let mut res_only = proj.clone();
            res_only.sources.clear();
            res_only.document_class = None;
            match neoflash_script::compile_project(&res_only) {
                Ok(res_prog) => neoflash_script::compile_with_program(script, &res_prog)
                    .err()
                    .unwrap_or_default(),
                Err(_) => neoflash_script::compile(script).err().unwrap_or_default(),
            }
        };

        let mut diagnostics = match neoflash_script::compile_project(&lang_project) {
            Ok(program) => neoflash_script::compile_with_program(script, &program)
                .err()
                .unwrap_or_default(),
            Err(mut proj_errs) => {
                let own_errs = compile_script_against_resources(&lang_project);
                if is_skipped_foreign {
                    own_errs
                } else {
                    proj_errs.extend(own_errs);
                    proj_errs
                }
            }
        };
        diagnostics.extend(
            neoflash_script::check_warnings(script)
                .iter()
                .map(|warning| warning.to_diagnostic_with_source(&script.source)),
        );
        if let Some(context) = self.hint_context(script.language) {
            diagnostics.extend(
                context
                    .semantic_analysis(&script.source)
                    .diagnostics()
                    .iter()
                    .cloned(),
            );
        }
        self.record_code_check(script, diagnostics, announce);
    }

    /// "Check project": compile project sources; when the selected file is in a foreign language,
    /// check it under its own language so editing feedback in Actions follows the file's language.
    pub(crate) fn check_project_sources(&mut self, announce: bool) {
        if self.is_standalone_script_session() {
            self.check_standalone_script(announce);
            return;
        }
        let active_source_id = match self.effective_actions_location() {
            Some(crate::panels::actions_sidebar::ScriptLocation::Source(id)) => Some(id),
            _ => self.selected_source.clone(),
        };
        let selected_unit = self
            .project
            .sources
            .iter()
            .find(|unit| Some(&unit.id) == active_source_id.as_ref())
            .cloned();
        let proj_lang = self.project.effective_script_language();
        let check_lang = selected_unit
            .as_ref()
            .map(|u| u.language)
            .unwrap_or(proj_lang);
        let is_skipped_foreign = selected_unit.as_ref().is_some_and(|u| {
            self.project.script_disposition_for(
                proj_lang,
                self.project.also_compile_compatible_scripts,
                u.language,
            ) == neoflash_core::ScriptDisposition::SkippedForeign
        });
        let mut check_proj = self.project.clone();
        check_proj.script_language = Some(check_lang);
        if is_skipped_foreign {
            check_proj.document_class = None;
        }
        let check_proj = self
            .hydrate_arbitrary_project_with_editor_context(&check_proj)
            .0
            .into_owned();
        let mut diagnostics = neoflash_script::compile_project(&check_proj)
            .err()
            .unwrap_or_default();
        for unit in &self.project.sources {
            if !check_proj
                .script_disposition_for(
                    check_lang,
                    check_proj.also_compile_compatible_scripts,
                    unit.language,
                )
                .is_compiled()
            {
                continue;
            }
            let script = Script {
                language: unit.language,
                source: unit.source.clone(),
            };
            diagnostics.extend(
                neoflash_script::check_warnings_with_path(&script, Some(&unit.path))
                    .iter()
                    .map(|warning| warning.to_diagnostic_with_source(&script.source)),
            );
            if let Some(context) = self.source_hint_context(unit) {
                diagnostics.extend(
                    context
                        .semantic_analysis(&script.source)
                        .diagnostics()
                        .iter()
                        .cloned()
                        .map(|mut diagnostic| {
                            diagnostic.path = Some(unit.path.clone());
                            diagnostic
                        }),
                );
            }
        }
        if let Some(unit) = selected_unit {
            self.record_code_check(
                &Script {
                    language: unit.language,
                    source: unit.source,
                },
                diagnostics,
                announce,
            );
        }
    }

    /// Result of the last check if it still describes `script` in the current editor.
    pub(super) fn current_feedback(&self, script: &Script) -> Option<&CodeFeedback> {
        self.code_feedback.as_ref().filter(|feedback| {
            feedback.target == self.actions_editor_id()
                && feedback.fingerprint == self.code_fingerprint(script)
        })
    }

    pub(super) fn diagnostic_is_current(&self, diagnostic: &Diagnostic) -> bool {
        if self.is_standalone_script_session() {
            return match diagnostic.path.as_deref() {
                None => true,
                Some(path) => self
                    .project
                    .sources
                    .first()
                    .is_some_and(|unit| unit.path == path),
            };
        }
        match (
            diagnostic.path.as_deref(),
            self.effective_actions_location(),
        ) {
            (None, loc) => !matches!(
                loc,
                Some(crate::panels::actions_sidebar::ScriptLocation::Source(_))
            ),
            (Some(path), Some(crate::panels::actions_sidebar::ScriptLocation::Source(id))) => self
                .project
                .sources
                .iter()
                .any(|unit| unit.id == id && unit.path == path),
            (Some(_), _) => false,
        }
    }

    pub(super) fn jump_to_diagnostic(
        &mut self,
        ctx: &egui::Context,
        source: &str,
        diagnostic: &Diagnostic,
    ) {
        if let Some(path) = diagnostic.path.as_deref() {
            if let Some(unit) = self
                .project
                .sources
                .iter()
                .find(|unit| unit.path == path)
                .cloned()
            {
                let loc = crate::panels::actions_sidebar::ScriptLocation::Source(unit.id.clone());
                self.selected_source = Some(unit.id);
                self.script_target = ScriptTarget::Source;
                self.actions_ui.active_pinned =
                    self.actions_ui.pinned_tabs.contains(&loc).then_some(loc);
                jump_to(
                    ctx,
                    self.actions_editor_id(),
                    &unit.source,
                    diagnostic.line,
                    diagnostic.column,
                );
            } else if let Some(disk_path) =
                self.resolve_external_as_path_for_navigation(None, std::path::Path::new(path))
                && self.open_standalone_script_session(ctx, disk_path)
            {
                let target_source = self.standalone_script_source().unwrap_or("").to_owned();
                jump_to(
                    ctx,
                    self.actions_editor_id(),
                    &target_source,
                    diagnostic.line,
                    diagnostic.column,
                );
            }
        } else if self.diagnostic_is_current(diagnostic) {
            jump_to(
                ctx,
                self.actions_editor_id(),
                source,
                diagnostic.line,
                diagnostic.column,
            );
        }
    }

    pub(super) fn jump_to_next_bar_diagnostic(
        &mut self,
        ctx: &egui::Context,
        source: &str,
        caret: (usize, usize),
        diagnostics: &[Diagnostic],
    ) {
        if diagnostics.is_empty() {
            return;
        }
        let chosen = diagnostics
            .iter()
            .find(|d| self.diagnostic_is_current(d) && (d.line, d.column) > caret)
            .or_else(|| diagnostics.iter().find(|d| !self.diagnostic_is_current(d)))
            .unwrap_or(&diagnostics[0])
            .clone();
        self.jump_to_diagnostic(ctx, source, &chosen);
    }

    pub(crate) fn format_script_status_tooltip(
        success: bool,
        text: &str,
        diagnostics: &[Diagnostic],
    ) -> String {
        if diagnostics.is_empty() {
            return if success {
                format!("✓ {text}")
            } else {
                text.to_owned()
            };
        }
        let max_shown = 5;
        let mut lines: Vec<String> = diagnostics
            .iter()
            .take(max_shown)
            .map(|d| {
                let code_prefix = d
                    .code
                    .as_deref()
                    .map_or_else(String::new, |c| format!("[{c}] "));
                let path_prefix = d
                    .path
                    .as_deref()
                    .map_or_else(String::new, |p| format!("{p} · "));
                format!(
                    "{code_prefix}{path_prefix}Line {}, col {}: {} — click to jump",
                    d.line, d.column, d.message
                )
            })
            .collect();
        if diagnostics.len() > max_shown {
            lines.push(format!("…and {} more", diagnostics.len() - max_shown));
        }
        lines.join("\n")
    }

    pub(super) fn diagnostic_action(
        &mut self,
        action: Option<diagnostics_ui::Action>,
        diagnostics: &[Diagnostic],
    ) {
        match action {
            Some(diagnostics_ui::Action::Fix(fix)) => {
                self.actions_ui.pending = Some(CodeCommand::ApplyQuickFix(fix));
            }
            Some(diagnostics_ui::Action::CompilerErrors) => {
                self.show_actions_compiler_diagnostics(diagnostics);
            }
            Some(diagnostics_ui::Action::NoFix) => self.message("No quick fix at the caret."),
            None => {}
        }
    }
}
