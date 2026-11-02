//! Compiler errors panel (`Panel / Compiler errors`, `PP-04`; reference `panels-more-2.png`),
//! opened by Window ▸ Compiler errors (`⌥F2`).
//!
//! Compiles project source units, timeline frame scripts and object event scripts against the
//! current `Project` snapshot (plus HTML5 publish warnings) without mutating document state.
//! Clicking `Go to source` opens the target script in Actions and moves the caret to the exact
//! 1-based `(line, column)`.
use crate::app::Editor;
use crate::panels::actions_sidebar::ScriptLocation;
use crate::theme::ScrollTheme as _;
use eframe::egui::{self, Ui};
use neoflash_core::{ElementKind, Project};
use neoflash_ui::components as c;
use neoflash_ui::icons::{self, i16};
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::typography::rich;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompilerDiagnosticItem {
    pub is_error: bool,
    pub message: String,
    pub location_label: String,
    pub script: Option<ScriptLocation>,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CompilerErrorsState {
    fingerprint: Option<u64>,
    pub(crate) items: Vec<CompilerDiagnosticItem>,
}

fn project_diagnostics_fingerprint(project: &Project) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    project.name.hash(&mut hash);
    project.document_class.hash(&mut hash);
    project.scenes.hash(&mut hash);
    for unit in &project.sources {
        unit.id.hash(&mut hash);
        unit.path.hash(&mut hash);
        std::mem::discriminant(&unit.language).hash(&mut hash);
        unit.source.hash(&mut hash);
    }
    project.resources.len().hash(&mut hash);
    for resource in &project.resources {
        resource.id.hash(&mut hash);
        resource.path.hash(&mut hash);
        resource.media_type.hash(&mut hash);
    }
    for symbol in &project.symbols {
        symbol.id.hash(&mut hash);
        symbol.name.hash(&mut hash);
        symbol.class_name.hash(&mut hash);
        symbol.linkage_id.hash(&mut hash);
        for fs in &symbol.timeline.scripts {
            fs.frame.hash(&mut hash);
            std::mem::discriminant(&fs.script.language).hash(&mut hash);
            fs.script.source.hash(&mut hash);
        }
        for layer in &symbol.timeline.layers {
            layer.id.hash(&mut hash);
            layer.name.hash(&mut hash);
            for key in &layer.keyframes {
                key.frame.hash(&mut hash);
                for el in &key.elements {
                    el.id.hash(&mut hash);
                    el.name.hash(&mut hash);
                    match &el.kind {
                        ElementKind::Text { font_family, .. } => font_family.hash(&mut hash),
                        ElementKind::TextBox { format, .. } => format.font.hash(&mut hash),
                        _ => {}
                    }
                    for os in &el.scripts {
                        std::mem::discriminant(&os.event).hash(&mut hash);
                        std::mem::discriminant(&os.script.language).hash(&mut hash);
                        os.script.source.hash(&mut hash);
                    }
                }
            }
        }
    }
    hash.finish()
}

pub(crate) fn collect_compiler_diagnostics(project: &Project) -> Vec<CompilerDiagnosticItem> {
    let mut items = Vec::new();
    let program = neoflash_script::compile_project(project);
    if let Err(errors) = &program {
        for err in errors {
            let matched = project.sources.iter().find_map(|unit| {
                let prefix = format!("{}: ", unit.path);
                err.message
                    .strip_prefix(&prefix)
                    .map(|msg| (unit, msg.to_owned()))
            });
            if let Some((unit, message)) = matched {
                items.push(CompilerDiagnosticItem {
                    is_error: true,
                    message,
                    location_label: format!("{} · Line {}", unit.path, err.line),
                    script: Some(ScriptLocation::Source(unit.id.clone())),
                    line: err.line,
                    column: err.column,
                });
            } else {
                items.push(CompilerDiagnosticItem {
                    is_error: true,
                    message: err.message.clone(),
                    location_label: format!("Project · Line {}", err.line),
                    script: project
                        .sources
                        .first()
                        .map(|u| ScriptLocation::Source(u.id.clone())),
                    line: err.line,
                    column: err.column,
                });
            }
        }
    }

    let fallback_program = program
        .is_err()
        .then(|| neoflash_script::compile_project(&Project::blank()).ok())
        .flatten();
    let active_program = program.as_ref().ok().or(fallback_program.as_ref());

    for symbol in &project.symbols {
        let actions_layer = symbol
            .timeline
            .layers
            .iter()
            .rev()
            .find(|l| l.name.eq_ignore_ascii_case("actions"))
            .map(|l| l.name.as_str());
        for fs in &symbol.timeline.scripts {
            if fs.script.source.trim().is_empty() {
                continue;
            }
            let result = match active_program {
                Some(prog) => neoflash_script::compile_with_program(&fs.script, prog),
                None => neoflash_script::compile(&fs.script),
            };
            if let Err(errors) = result {
                for err in errors {
                    let location_label = match actions_layer {
                        Some(layer_name) => format!(
                            "{} · layer {:?} · frame {} · Line {}",
                            symbol.name,
                            layer_name,
                            fs.frame + 1,
                            err.line
                        ),
                        None => format!(
                            "{} · frame {} · Line {}",
                            symbol.name,
                            fs.frame + 1,
                            err.line
                        ),
                    };
                    items.push(CompilerDiagnosticItem {
                        is_error: true,
                        message: err.message,
                        location_label,
                        script: Some(ScriptLocation::Frame {
                            symbol: symbol.id.clone(),
                            frame: fs.frame,
                        }),
                        line: err.line,
                        column: err.column,
                    });
                }
            }
        }

        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for el in &key.elements {
                    for os in &el.scripts {
                        if os.script.source.trim().is_empty() {
                            continue;
                        }
                        let result = match active_program {
                            Some(prog) => neoflash_script::compile_with_program(&os.script, prog),
                            None => neoflash_script::compile(&os.script),
                        };
                        if let Err(errors) = result {
                            let target = if el.name.is_empty() {
                                "object"
                            } else {
                                el.name.as_str()
                            };
                            for err in errors {
                                items.push(CompilerDiagnosticItem {
                                    is_error: true,
                                    message: err.message,
                                    location_label: format!(
                                        "{} · layer {:?} · frame {} · {:?} ({target}) · Line {}",
                                        symbol.name,
                                        layer.name,
                                        key.frame + 1,
                                        os.event,
                                        err.line
                                    ),
                                    script: Some(ScriptLocation::Object {
                                        symbol: symbol.id.clone(),
                                        frame: key.frame,
                                        element: el.id.clone(),
                                        event: os.event,
                                    }),
                                    line: err.line,
                                    column: err.column,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    for warning in neoflash_export::export_warnings(project) {
        items.push(CompilerDiagnosticItem {
            is_error: false,
            message: warning,
            location_label: format!("{} · HTML5 publish", project.name),
            script: None,
            line: 1,
            column: 1,
        });
    }

    items
}

impl Editor {
    pub(crate) fn refresh_compiler_diagnostics(&mut self) {
        let fp = project_diagnostics_fingerprint(&self.project);
        if self.compiler_errors.fingerprint != Some(fp) {
            self.compiler_errors.items = collect_compiler_diagnostics(&self.project);
            self.compiler_errors.fingerprint = Some(fp);
        }
    }

    pub(crate) fn script_source_for(&self, location: &ScriptLocation) -> Option<String> {
        match location {
            ScriptLocation::Frame { symbol, frame } => self
                .project
                .symbol(symbol)?
                .timeline
                .scripts
                .iter()
                .find(|fs| fs.frame == *frame)
                .map(|fs| fs.script.source.clone()),
            ScriptLocation::Object {
                symbol,
                frame,
                element,
                event,
            } => self
                .project
                .symbol(symbol)?
                .timeline
                .layers
                .iter()
                .filter_map(|layer| layer.key_at(*frame))
                .flat_map(|key| &key.elements)
                .find(|el| &el.id == element)?
                .scripts
                .iter()
                .find(|os| os.event == *event)
                .map(|os| os.script.source.clone()),
            ScriptLocation::Source(id) => self
                .project
                .sources
                .iter()
                .find(|u| &u.id == id)
                .map(|u| u.source.clone()),
        }
    }

    pub(crate) fn jump_to_compiler_diagnostic(&mut self, ctx: &egui::Context, idx: usize) -> bool {
        self.refresh_compiler_diagnostics();
        let Some(item) = self.compiler_errors.items.get(idx).cloned() else {
            return false;
        };
        let Some(loc) = item.script else {
            return false;
        };
        let source = self.script_source_for(&loc).unwrap_or_default();
        self.open_script_location(loc);
        self.open_actions();
        let editor_id = self.actions_editor_id();
        crate::sources::jump_to(ctx, editor_id, &source, item.line, item.column);
        true
    }

    pub(crate) fn compiler_errors_panel_ui(&mut self, ui: &mut Ui) {
        self.refresh_compiler_diagnostics();
        let nf = neoflash_ui::palette(ui.ctx());
        let error_count = self
            .compiler_errors
            .items
            .iter()
            .filter(|i| i.is_error)
            .count();
        let warning_count = self.compiler_errors.items.len() - error_count;
        let warnings_badge = if warning_count == 1 {
            "1 warning".to_owned()
        } else {
            format!("{warning_count} warnings")
        };
        let errors_badge = if error_count == 1 {
            "1 error".to_owned()
        } else {
            format!("{error_count} errors")
        };

        ui.horizontal(|ui| {
            ui.add(c::Badge::new(&warnings_badge, c::BadgeType::Warning));
            ui.add(c::Badge::new(&errors_badge, c::BadgeType::Neutral));
        });

        if self.compiler_errors.items.is_empty() {
            c::EmptyState {
                icon: i16::CHECK,
                title: "No compiler errors",
                body: "Project sources, timeline scripts and publish checks passed.",
                action: None,
            }
            .show(ui);
            return;
        }

        let mut jump_target = None;
        egui::ScrollArea::vertical()
            .id_salt("compiler-errors-scroll")
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = space::S2;
                for (idx, item) in self.compiler_errors.items.iter().enumerate() {
                    let (icon, color) = if item.is_error {
                        (i16::ERROR, nf.danger_default)
                    } else {
                        (i16::WARNING, nf.warning_default)
                    };
                    let can_jump = item.script.is_some();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        icons::show(ui, icon, color);
                        let text_resp = ui
                            .vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(
                                    rich(&item.message, TextStyle::Body).color(nf.text_primary),
                                );
                                ui.label(
                                    rich(&item.location_label, TextStyle::Small)
                                        .color(nf.text_secondary),
                                );
                            })
                            .response;
                        if can_jump && text_resp.interact(egui::Sense::click()).clicked() {
                            jump_target = Some(idx);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    can_jump,
                                    c::IconButton::new(i16::EXTERNAL, "Go to source"),
                                )
                                .clicked()
                            {
                                jump_target = Some(idx);
                            }
                        });
                    });
                }
            });

        if let Some(idx) = jump_target {
            let ctx = ui.ctx().clone();
            self.jump_to_compiler_diagnostic(&ctx, idx);
        }
    }
}
