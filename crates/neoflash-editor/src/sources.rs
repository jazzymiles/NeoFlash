//! Project code editing shares the existing Actions workspace and Undo history.
use crate::app::Editor;
use eframe::egui;
use neoflash_core::{Script, ScriptLanguage, SourceUnit, SymbolKind, new_id};
use std::hash::{Hash, Hasher};

pub(crate) struct CodeFeedback {
    target: egui::Id,
    fingerprint: u64,
    text: String,
    success: bool,
}

impl Editor {
    fn code_fingerprint(&self, script: &Script) -> u64 {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        script.source.hash(&mut hash);
        std::mem::discriminant(&script.language).hash(&mut hash);
        self.project.document_class.hash(&mut hash);
        for unit in &self.project.sources {
            unit.id.hash(&mut hash);
            unit.path.hash(&mut hash);
            unit.source.hash(&mut hash);
            std::mem::discriminant(&unit.language).hash(&mut hash);
        }
        for symbol in &self.project.symbols {
            symbol.id.hash(&mut hash);
            symbol.class_name.hash(&mut hash);
            symbol.linkage_id.hash(&mut hash);
        }
        hash.finish()
    }

    fn record_code_check(
        &mut self,
        script: &Script,
        result: Result<(), Vec<neoflash_script::Diagnostic>>,
    ) {
        let success = result.is_ok();
        let text = match result {
            Ok(()) => "Code accepted for current language support.".to_owned(),
            Err(errors) => errors
                .into_iter()
                .map(|error| {
                    format!(
                        "Line {}, column {}: {}",
                        error.line, error.column, error.message
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        };
        self.message(text.clone());
        self.code_feedback = Some(CodeFeedback {
            target: self.actions_editor_id(),
            fingerprint: self.code_fingerprint(script),
            text,
            success,
        });
    }

    pub(crate) fn check_action_source(&mut self, script: &Script) {
        let result = neoflash_script::compile_project(&self.project)
            .and_then(|program| neoflash_script::compile_with_program(script, &program))
            .map(|_| ());
        self.record_code_check(script, result);
    }

    pub(crate) fn show_code_feedback(&self, ui: &mut egui::Ui, script: &Script) {
        if let Some(feedback) = &self.code_feedback
            && feedback.target == self.actions_editor_id()
            && feedback.fingerprint == self.code_fingerprint(script)
        {
            egui::ScrollArea::vertical()
                .id_salt("code_feedback")
                .max_height(110.)
                .show(ui, |ui| {
                    let color = if feedback.success {
                        ui.visuals().text_color()
                    } else {
                        ui.visuals().error_fg_color
                    };
                    ui.label(egui::RichText::new(&feedback.text).color(color));
                });
        }
    }

    pub(crate) fn project_sources(&mut self, ui: &mut egui::Ui) {
        if !self
            .project
            .sources
            .iter()
            .any(|source| Some(&source.id) == self.selected_source.as_ref())
        {
            self.selected_source = self.project.sources.first().map(|source| source.id.clone());
        }
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("project_source_file")
                .selected_text(self.project.sources.iter().find(|s| Some(&s.id) == self.selected_source.as_ref()).map_or("No source files", |s| s.path.as_str()))
                .show_ui(ui, |ui| {
                    for unit in &self.project.sources {
                        ui.selectable_value(&mut self.selected_source, Some(unit.id.clone()), &unit.path);
                    }
                });
            for (label, language) in [("New AS2 class", ScriptLanguage::ActionScript2), ("New AS3 class", ScriptLanguage::ActionScript3)] {
              if ui.button(label).clicked() {
                let mut index = 1;
                while self.project.sources.iter().any(|s| s.path == format!("Class{index}.as")) {index += 1;}
                let name = format!("Class{index}");
                let source = if language == ScriptLanguage::ActionScript2 {
                    format!("class {name} {{\n    public function {name}() {{\n    }}\n}}\n")
                } else {
                    format!("package {{\n    public class {name} {{\n        public function {name}() {{\n        }}\n    }}\n}}\n")
                };
                let unit = SourceUnit {id:new_id("source"), path:format!("{name}.as"), language, source};
                self.selected_source = Some(unit.id.clone());
                self.transact(|editor| {editor.project.enable_program();editor.project.sources.push(unit);});
              }
            }
            if ui.button("Import source…").clicked()
                && let Some(path) = rfd::FileDialog::new().add_filter("Project source", &["as", "js", "ts"]).pick_file() {
                match std::fs::read_to_string(&path) {
                    Ok(source) if source.len() <= 1_048_576 => {
                        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                        if self.project.sources.iter().any(|s| s.path.eq_ignore_ascii_case(&name)) {
                            self.message("A source file with this name already exists. Rename it before importing.");
                        } else {
                            let language = match path.extension().and_then(|s|s.to_str()) {Some("js")=>ScriptLanguage::JavaScript,Some("ts")=>ScriptLanguage::TypeScript,_=>ScriptLanguage::ActionScript3};
                            let unit=SourceUnit{id:new_id("source"),path:name,language,source};
                            self.selected_source=Some(unit.id.clone());
                            self.transact(|editor|{editor.project.enable_program();editor.project.sources.push(unit);});
                        }
                    }
                    Ok(_) => self.message("A source file is limited to 1 MiB."),
                    Err(error) => self.message(format!("Could not read source: {error}")),
                }
            }
            if ui.add_enabled(self.selected_source.is_some(), egui::Button::new("Remove file")).clicked() {
                let selected=self.selected_source.clone();
                self.transact(|editor|editor.project.sources.retain(|source|Some(&source.id)!=selected.as_ref()));
                self.selected_source=None;
            }
        });
        let mut document_class = self.project.document_class.clone().unwrap_or_default();
        let mut symbol_class = self.symbol().class_name.clone().unwrap_or_default();
        let mut linkage_id = self.symbol().linkage_id.clone().unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            ui.label("Document class");
            ui.add(
                egui::TextEdit::singleline(&mut document_class)
                    .desired_width(180.)
                    .hint_text("app.Main"),
            );
            if self.symbol_id() != self.project.root {
                ui.label("Symbol class");
                ui.add(
                    egui::TextEdit::singleline(&mut symbol_class)
                        .desired_width(180.)
                        .hint_text("ui.Component"),
                );
                if self.symbol().kind == SymbolKind::MovieClip {
                    ui.label("Export identifier");
                    ui.add(
                        egui::TextEdit::singleline(&mut linkage_id)
                            .desired_width(160.)
                            .hint_text("card-library-id"),
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
            self.transact(|editor| {
                editor.project.enable_program();
                editor.project.document_class = document_class;
                editor.symbol_mut().class_name = symbol_class;
                editor.symbol_mut().linkage_id = linkage_id;
            });
        }
        let Some(index) = self
            .project
            .sources
            .iter()
            .position(|s| Some(&s.id) == self.selected_source.as_ref())
        else {
            ui.label("Add a project class to reuse code across frames and symbol instances. Files are stored inside the project.");
            return;
        };
        let original = self.project.sources[index].clone();
        let mut unit = original.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label("Path");
            ui.add(egui::TextEdit::singleline(&mut unit.path).desired_width(240.));
            egui::ComboBox::from_id_salt("source_language")
                .selected_text(format!("{:?}", unit.language))
                .show_ui(ui, |ui| {
                    for language in [
                        ScriptLanguage::ActionScript3,
                        ScriptLanguage::ActionScript2,
                        ScriptLanguage::TypeScript,
                        ScriptLanguage::JavaScript,
                    ] {
                        ui.selectable_value(&mut unit.language, language, format!("{language:?}"));
                    }
                });
            if ui.button("Check project").clicked() {
                let result = neoflash_script::compile_project(&self.project).map(|_| ());
                self.record_code_check(
                    &Script {
                        language: unit.language,
                        source: unit.source.clone(),
                    },
                    result,
                );
            }
        });
        self.show_code_feedback(
            ui,
            &Script {
                language: unit.language,
                source: unit.source.clone(),
            },
        );
        let editor_id = self.actions_editor_id();
        egui::ScrollArea::vertical()
            .id_salt(("project_source_scroll", editor_id))
            .show(ui, |ui| {
                let response = ui.add_sized(
                    [ui.available_width(), ui.available_height().max(80.)],
                    egui::TextEdit::multiline(&mut unit.source)
                        .id(editor_id)
                        .font(egui::TextStyle::Monospace)
                        .code_editor(),
                );
                if response.has_focus() && self.playing {
                    self.set_playing(false);
                }
            });
        if unit != original {
            self.transact(|editor| editor.project.sources[index] = unit);
        }
    }
}
