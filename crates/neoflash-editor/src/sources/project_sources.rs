use super::*;

impl Editor {
    pub(crate) fn project_sources(&mut self, ui: &mut egui::Ui) {
        let pinned_source = match self.effective_actions_location() {
            Some(crate::panels::actions_sidebar::ScriptLocation::Source(id))
                if self.actions_ui.active_pinned.is_some() =>
            {
                Some(id)
            }
            _ => None,
        };
        let target_id = if let Some(pinned_id) = pinned_source {
            Some(pinned_id)
        } else {
            if !self
                .project
                .sources
                .iter()
                .any(|source| Some(&source.id) == self.selected_source.as_ref())
            {
                self.selected_source = self.project.sources.first().map(|source| source.id.clone());
            }
            self.selected_source.clone()
        };
        let index = self
            .project
            .sources
            .iter()
            .position(|s| Some(&s.id) == target_id.as_ref());
        let original = index.map(|index| self.project.sources[index].clone());
        let unit = original.clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::s2();
            let file_label = unit
                .as_ref()
                .map_or("No source files", |u| u.path.as_str())
                .to_owned();
            // The row keeps room for the language badge, the divider and three icon buttons.
            let reserved = neoflash_ui::metrics::field_m()
                + 3. * (neoflash_ui::metrics::control() + space::s2())
                + 2. * space::s3();
            let select_w = (ui.available_width() - reserved).clamp(
                neoflash_ui::metrics::field_s(),
                neoflash_ui::metrics::field_l() + neoflash_ui::metrics::field_s(),
            );
            nf::Select::new("project_source_file", &file_label)
                .icon(neoflash_ui::icons::i16::FILE_CODE)
                .width(select_w)
                .show(ui, |ui| {
                    for source in &self.project.sources {
                        let current = Some(&source.id) == target_id.as_ref();
                        if ui
                            .add(nf::MenuItem::new(&source.path).checked(current))
                            .clicked()
                        {
                            let loc = crate::panels::actions_sidebar::ScriptLocation::Source(
                                source.id.clone(),
                            );
                            self.selected_source = Some(source.id.clone());
                            self.script_target = ScriptTarget::Source;
                            self.actions_ui.active_pinned =
                                self.actions_ui.pinned_tabs.contains(&loc).then_some(loc);
                        }
                    }
                });
            let proj_lang = self.project.effective_script_language();
            if let Some(unit) = &unit {
                let skipped = self.project.script_disposition_for(
                    proj_lang,
                    self.project.also_compile_compatible_scripts,
                    unit.language,
                ) == neoflash_core::ScriptDisposition::SkippedForeign;
                ui.add_space(space::s1());
                Self::script_language_badge(ui, unit.language, skipped);
            }
            ui.add_space(space::s1());
            nf::divider(ui, true);
            let new_enabled = matches!(
                proj_lang,
                ScriptLanguage::ActionScript3 | ScriptLanguage::ActionScript2
            );
            let new_disabled_reason = (!new_enabled).then(|| {
                format!(
                    "The project script language is {}",
                    proj_lang.display_label()
                )
            });
            let new = ui.add_enabled_semantic(
                new_enabled,
                nf::IconButton::new(neoflash_ui::icons::i16::NEW_FILE, "New class"),
            );
            crate::ui_icons::hover_tooltip(new.clone(), "", None, new_disabled_reason.as_deref());
            if new_enabled {
                nf::popup_below(&new, neoflash_ui::metrics::menu_width(), |ui| {
                    let (label, language) = if proj_lang == ScriptLanguage::ActionScript2 {
                        ("New AS2 class", ScriptLanguage::ActionScript2)
                    } else {
                        ("New AS3 class", ScriptLanguage::ActionScript3)
                    };
                    if ui.add(nf::MenuItem::new(label)).clicked() {
                        self.new_source_class(language);
                        self.activate_current_actions_location();
                    }
                });
            }
            if ui
                .add(nf::IconButton::new(
                    neoflash_ui::icons::i16::OPEN,
                    "Import source file…",
                ))
                .clicked()
            {
                self.import_source_file();
                self.activate_current_actions_location();
            }
            let remove = ui.add_enabled_semantic(
                target_id.is_some(),
                nf::IconButton::new(neoflash_ui::icons::i16::TRASH, "Remove source file"),
            );
            if remove.clicked() {
                let selected = target_id.clone();
                self.transact_named("Delete source file", |editor| {
                    editor
                        .project
                        .sources
                        .retain(|source| Some(&source.id) != selected.as_ref())
                });
                if self.selected_source == selected {
                    self.selected_source = None;
                }
                self.prune_stale_pinned_script_tabs();
            }
        });
        ui.add_space(space::s1());
        let mut document_class = self.project.document_class.clone().unwrap_or_default();
        let mut symbol_class = self.symbol().class_name.clone().unwrap_or_default();
        let mut linkage_id = self.symbol().linkage_id.clone().unwrap_or_default();
        let is_root = self.symbol_id() == self.project.root;
        let is_movie_clip = self.symbol().kind == SymbolKind::MovieClip;
        nf::SectionHeader::new("Class linkage")
            .id_salt("actions-class-linkage")
            .default_open(false)
            .show(ui, |ui| {
                ui.add(
                    nf::TextField::new(&mut document_class, "Document class")
                        .prefix("Document")
                        .hint("app.Main")
                        .mono(),
                );
                if !is_root {
                    ui.add(
                        nf::TextField::new(&mut symbol_class, "Symbol class")
                            .prefix("Symbol class")
                            .hint("ui.Component")
                            .mono(),
                    );
                    if is_movie_clip {
                        ui.add(
                            nf::TextField::new(&mut linkage_id, "Export identifier")
                                .prefix("Export ID")
                                .hint("card-library-id")
                                .mono(),
                        );
                    }
                }
            });
        let document_class =
            (!document_class.trim().is_empty()).then(|| document_class.trim().to_owned());
        let symbol_class =
            (!symbol_class.trim().is_empty()).then(|| symbol_class.trim().to_owned());
        let linkage_id = (!linkage_id.trim().is_empty()).then(|| linkage_id.trim().to_owned());
        if document_class != self.project.document_class
            || symbol_class != self.symbol().class_name
            || linkage_id != self.symbol().linkage_id
        {
            self.transact_named("Class linkage", |editor| {
                editor.project.enable_program();
                editor.project.document_class = document_class;
                editor.symbol_mut().class_name = symbol_class;
                editor.symbol_mut().linkage_id = linkage_id;
            });
        }
        let (Some(index), Some(original), Some(mut unit)) = (index, original, unit) else {
            let proj_lang = self.project.effective_script_language();
            let empty_action = match proj_lang {
                ScriptLanguage::ActionScript3 => {
                    Some(("New AS3 class", ScriptLanguage::ActionScript3))
                }
                ScriptLanguage::ActionScript2 => {
                    Some(("New AS2 class", ScriptLanguage::ActionScript2))
                }
                ScriptLanguage::ActionScript1
                | ScriptLanguage::TypeScript
                | ScriptLanguage::JavaScript => None,
            };
            let raw_avail = ui.available_rect_before_wrap();
            let bound_right = ui.max_rect().right().min(ui.clip_rect().right());
            let bound_bottom = ui.max_rect().bottom().min(ui.clip_rect().bottom());
            let avail = egui::Rect::from_min_max(
                raw_avail.min,
                egui::pos2(
                    bound_right.max(raw_avail.left()),
                    bound_bottom.max(raw_avail.top()),
                ),
            );
            let bar_h = if self.actions_ui.pinned_tabs.is_empty() {
                0.0
            } else {
                neoflash_ui::metrics::script_tab() + 2.0
            };
            let empty_rect = egui::Rect::from_min_max(
                avail.min,
                egui::pos2(avail.right(), (avail.bottom() - bar_h).max(avail.top())),
            );
            let new = ui
                .scope_builder(egui::UiBuilder::new().max_rect(empty_rect), |ui| {
                    nf::EmptyState {
                        icon: neoflash_ui::icons::i16::FILE_CODE,
                        title: "No project classes",
                        body: "Add a project class to reuse code across frames and symbol instances. Files are stored inside the project.",
                        action: empty_action.map(|(label, _)| label),
                    }
                    .show(ui)
                })
                .inner;
            if bar_h > 0.0 {
                let bar = egui::Rect::from_min_max(
                    egui::pos2(avail.left(), avail.bottom() - bar_h),
                    avail.max,
                );
                let p = neoflash_ui::palette(ui.ctx());
                ui.painter().hline(
                    bar.x_range(),
                    bar.top() + 0.5,
                    egui::Stroke::new(1.0_f32, p.border_subtle),
                );
                self.render_actions_script_tabs_bar(ui, bar, "");
                ui.advance_cursor_after_rect(avail);
            }
            if new && let Some((_, language)) = empty_action {
                self.new_source_class(language);
                self.activate_current_actions_location();
            }
            return;
        };
        nf::SectionHeader::new("File properties")
            .id_salt("actions-file-properties")
            .default_open(false)
            .show(ui, |ui| {
                ui.add(
                    nf::TextField::new(&mut unit.path, "Path")
                        .prefix("Path")
                        .mono(),
                );
            });
        let tab = unit.path.clone();
        let response = self.script_workspace(ui, &mut unit.source, unit.language, "", &tab);
        if response.has_focus() && self.playing {
            self.set_playing(false);
        }
        if unit != original {
            self.transact_named("Edit script", |editor| editor.project.sources[index] = unit);
        }
    }

    pub(super) fn new_source_class(&mut self, language: ScriptLanguage) {
        let mut index = 1;
        while self
            .project
            .sources
            .iter()
            .any(|s| s.path == format!("Class{index}.as"))
        {
            index += 1;
        }
        let name = format!("Class{index}");
        let indent = " ".repeat(usize::from(self.workspace.code_editor.tab_size.clamp(1, 8)));
        let indent2 = indent.repeat(2);
        let source = if language == ScriptLanguage::ActionScript2 {
            format!("class {name} {{\n{indent}public function {name}() {{\n{indent}}}\n}}\n")
        } else {
            format!(
                "package {{\n{indent}public class {name} {{\n{indent2}public function {name}() {{\n{indent2}}}\n{indent}}}\n}}\n"
            )
        };
        let unit = SourceUnit {
            id: new_id("source"),
            path: format!("{name}.as"),
            language,
            source,
        };
        self.selected_source = Some(unit.id.clone());
        self.transact_named("New class", |editor| {
            editor.project.enable_program();
            editor.project.sources.push(unit);
        });
    }

    pub(super) fn import_source_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Project source", &["as", "js", "ts"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(source) if source.len() <= 1_048_576 => {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                if self
                    .project
                    .sources
                    .iter()
                    .any(|s| s.path.eq_ignore_ascii_case(&name))
                {
                    self.message(
                        "A source file with this name already exists. Rename it before importing.",
                    );
                } else {
                    let language = match path.extension().and_then(|s| s.to_str()) {
                        Some(ext)
                            if ext.eq_ignore_ascii_case("js")
                                || ext.eq_ignore_ascii_case("mjs")
                                || ext.eq_ignore_ascii_case("cjs")
                                || ext.eq_ignore_ascii_case("jsx") =>
                        {
                            ScriptLanguage::JavaScript
                        }
                        Some(ext)
                            if ext.eq_ignore_ascii_case("ts")
                                || ext.eq_ignore_ascii_case("mts")
                                || ext.eq_ignore_ascii_case("cts")
                                || ext.eq_ignore_ascii_case("tsx") =>
                        {
                            ScriptLanguage::TypeScript
                        }
                        _ => {
                            let proj = self.project.effective_script_language();
                            if proj.is_actionscript() {
                                proj
                            } else {
                                ScriptLanguage::ActionScript3
                            }
                        }
                    };
                    let unit = SourceUnit {
                        id: new_id("source"),
                        path: name,
                        language,
                        source,
                    };
                    self.selected_source = Some(unit.id.clone());
                    self.transact(|editor| {
                        editor.project.enable_program();
                        editor.project.sources.push(unit);
                    });
                }
            }
            Ok(_) => self.message("A source file is limited to 1 MiB."),
            Err(error) => self.message(format!("Could not read source: {error}")),
        }
    }

    /// Commit a new project-level script language (`R-102`) in a single `"Script language"` Undo step.
    pub fn set_project_script_language(&mut self, language: ScriptLanguage) {
        if self.project.script_language == Some(language) {
            return;
        }
        self.transact_named("Script language", |editor| {
            editor.project.script_language = Some(language);
            editor.project.upgrade_format_for_features();
        });
    }

    /// Commit the compatible-script compilation toggle (`R-102`) in a single `"Compatible script policy"` Undo step.
    pub fn set_also_compile_compatible_scripts(&mut self, enabled: bool) {
        if self.project.also_compile_compatible_scripts == enabled {
            return;
        }
        self.transact_named("Compatible script policy", |editor| {
            editor.project.also_compile_compatible_scripts = enabled;
            editor.project.upgrade_format_for_features();
        });
    }

    /// Whether a `SourceUnit` at `path` can be retagged to `target_lang` via `Mark as <language>`.
    /// Files whose language is fixed by extension (`.js`/`.ts`, or `.as` outside ActionScript)
    /// omit the button (`R-102` item 7).
    pub(crate) fn can_retag_source_unit_to(path: &str, target_lang: ScriptLanguage) -> bool {
        let Some(ext) = std::path::Path::new(path)
            .extension()
            .and_then(|s| s.to_str())
        else {
            return true;
        };
        if ext.eq_ignore_ascii_case("js")
            || ext.eq_ignore_ascii_case("mjs")
            || ext.eq_ignore_ascii_case("cjs")
            || ext.eq_ignore_ascii_case("jsx")
            || ext.eq_ignore_ascii_case("ts")
            || ext.eq_ignore_ascii_case("mts")
            || ext.eq_ignore_ascii_case("cts")
            || ext.eq_ignore_ascii_case("tsx")
        {
            return false;
        }
        if ext.eq_ignore_ascii_case("as") {
            return target_lang.is_actionscript();
        }
        true
    }

    /// Retag the open script (`Frame`, `Object`, or eligible `SourceUnit`) to the project's
    /// script language in one Undo step without modifying its source text (`R-102` item 7).
    pub fn mark_current_script_as_project_language(&mut self) -> bool {
        use crate::panels::actions_sidebar::ScriptLocation;
        let proj_lang = self.project.effective_script_language();
        let active_loc = self.effective_actions_location();
        match active_loc {
            Some(ScriptLocation::Source(selected_id)) => {
                let Some(unit) = self.project.sources.iter().find(|s| s.id == selected_id) else {
                    return false;
                };
                if unit.language == proj_lang
                    || !Self::can_retag_source_unit_to(&unit.path, proj_lang)
                {
                    return false;
                }
                if self.actions_ui.active_pinned.is_none() {
                    self.selected_source = Some(selected_id.clone());
                }
                self.finish_edits();
                self.transact_named("Mark script language", |editor| {
                    editor.project.script_language.get_or_insert(proj_lang);
                    if let Some(u) = editor
                        .project
                        .sources
                        .iter_mut()
                        .find(|s| s.id == selected_id)
                    {
                        u.language = proj_lang;
                    }
                    editor.project.upgrade_format_for_features();
                });
                self.code_feedback = None;
                self.check_project_sources(false);
                true
            }
            Some(ScriptLocation::Frame {
                symbol,
                layer,
                frame,
            }) => {
                let Some(sym) = self.project.symbol(&symbol) else {
                    return false;
                };
                let layer_index = layer
                    .as_deref()
                    .and_then(|id| sym.timeline.layers.iter().position(|l| l.id == id))
                    .or_else(|| sym.timeline.action_layer_for_frame(frame))
                    .unwrap_or(0);
                let Some(layer_id) = sym.timeline.layers.get(layer_index).map(|l| l.id.clone())
                else {
                    return false;
                };
                let Some(existing) = sym
                    .timeline
                    .frame_script_for_layer(layer_index, frame)
                    .filter(|fs| !fs.script.source.is_empty())
                    .map(|fs| fs.script.clone())
                else {
                    return false;
                };
                if existing.language == proj_lang {
                    return false;
                }
                self.finish_edits();
                self.transact_named("Mark script language", |editor| {
                    editor.project.script_language.get_or_insert(proj_lang);
                    if let Some(sym_mut) = editor.project.symbol_mut(&symbol)
                        && let Some(index) = sym_mut
                            .timeline
                            .frame_script_index_for_layer(layer_index, frame)
                    {
                        let fs = &mut sym_mut.timeline.scripts[index];
                        fs.layer = Some(layer_id.clone());
                        fs.script.language = proj_lang;
                    }
                    editor.project.upgrade_format_for_features();
                });
                self.code_feedback = None;
                let updated = Script {
                    language: proj_lang,
                    source: existing.source,
                };
                self.check_action_source(&updated, false);
                true
            }
            Some(ScriptLocation::Object {
                symbol,
                element,
                event,
                ..
            }) => {
                if self.actions_ui.active_pinned.is_none() && !self.selected_editable() {
                    return false;
                }
                let Some(existing) = self
                    .project
                    .symbol(&symbol)
                    .and_then(|sym| {
                        sym.timeline
                            .layers
                            .iter()
                            .flat_map(|l| &l.keyframes)
                            .flat_map(|k| &k.elements)
                            .find(|e| e.id == element)
                    })
                    .and_then(|e| {
                        e.scripts
                            .iter()
                            .find(|os| os.event == event && !os.script.source.is_empty())
                    })
                    .map(|os| os.script.clone())
                else {
                    return false;
                };
                if existing.language == proj_lang {
                    return false;
                }
                self.finish_edits();
                if self.actions_ui.active_pinned.is_none() {
                    self.select_single_id(element.clone());
                }
                self.transact_named("Mark script language", |editor| {
                    editor.project.script_language.get_or_insert(proj_lang);
                    if let Some(sym_mut) = editor.project.symbol_mut(&symbol) {
                        for l in &mut sym_mut.timeline.layers {
                            for k in &mut l.keyframes {
                                if let Some(el) = k.elements.iter_mut().find(|e| e.id == element)
                                    && let Some(os) =
                                        el.scripts.iter_mut().find(|os| os.event == event)
                                {
                                    os.script.language = proj_lang;
                                }
                            }
                        }
                    }
                    editor.project.upgrade_format_for_features();
                });
                self.code_feedback = None;
                let updated = Script {
                    language: proj_lang,
                    source: existing.source,
                };
                self.check_action_source(&updated, false);
                true
            }
            None => {
                if self.script_target == ScriptTarget::Source
                    && let Some(first_id) = self.project.sources.first().map(|s| s.id.clone())
                {
                    self.selected_source = Some(first_id);
                    return self.mark_current_script_as_project_language();
                }
                false
            }
        }
    }

    /// Vertically-centred `NF2/Badge` in the Actions / Project files toolbar (`R-102` item 6).
    pub(crate) fn script_language_badge(
        ui: &mut egui::Ui,
        language: ScriptLanguage,
        skipped: bool,
    ) -> egui::Response {
        let badge_type = if skipped {
            nf::BadgeType::Warning
        } else {
            nf::BadgeType::Neutral
        };
        let resp = ui
            .horizontal(|ui| {
                ui.set_min_height(neoflash_ui::metrics::control());
                ui.add(nf::Badge::new(language.display_label(), badge_type))
            })
            .inner;
        crate::ui_icons::hover_tooltip(resp, "Set in Document settings", Some("⌘J"), None)
    }
}
