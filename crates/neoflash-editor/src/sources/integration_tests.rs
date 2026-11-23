use super::*;
use neoflash_core::{Element, FrameScript, ObjectEvent, ObjectScript, Project};

fn render(editor: &mut Editor, ctx: &egui::Context, quick_fix: bool) -> egui::FullOutput {
    let modifiers = egui::Modifiers::COMMAND;
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400., 540.),
            )),
            modifiers: if quick_fix {
                modifiers
            } else {
                egui::Modifiers::NONE
            },
            events: if quick_fix {
                vec![egui::Event::Key {
                    key: egui::Key::Period,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }]
            } else {
                Vec::new()
            },
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| editor.actions_contents(ui));
        },
    )
}

fn source(editor: &Editor, target: ScriptTarget) -> String {
    match target {
        ScriptTarget::Frame => editor.symbol().timeline.scripts[0].script.source.clone(),
        ScriptTarget::Object => editor.selected_element().unwrap().scripts[0]
            .script
            .source
            .clone(),
        ScriptTarget::Source => editor.project.sources[0].source.clone(),
    }
}

#[test]
fn actions_diagnostic_quick_fix_family_uses_current_source_and_one_undo() {
    for target in [
        ScriptTarget::Frame,
        ScriptTarget::Object,
        ScriptTarget::Source,
    ] {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut project = Project::blank();
        let text = if target == ScriptTarget::Source {
            "class Helper {\n public function run():Boolean { return not false; }\n}"
        } else {
            "// é: character positions\ntrace(not false);"
        };
        let script = Script {
            language: ScriptLanguage::ActionScript2,
            source: text.into(),
        };
        let mut element = Element {
            id: "tooling-object".into(),
            ..Element::default()
        };
        element.scripts.push(ObjectScript {
            event: ObjectEvent::EnterFrame,
            script: script.clone(),
        });
        let element_id = element.id.clone();
        project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(element);
        project.symbols[0].timeline.scripts.push(FrameScript {
            layer: None,
            frame: 0,
            script: script.clone(),
        });
        project.sources.push(SourceUnit {
            id: "helper-source".into(),
            path: "Helper.as".into(),
            language: ScriptLanguage::ActionScript2,
            source: if target == ScriptTarget::Source {
                text.into()
            } else {
                "class Helper {}".into()
            },
        });
        project.enable_program();
        let mut editor = Editor::from_project(&ctx, project);
        editor.script_target = target;
        editor.script_event = ObjectEvent::EnterFrame;
        editor.selected_source = Some("helper-source".into());
        editor.select_single_id(element_id);
        let original = editor.project.clone();
        let _ = render(&mut editor, &ctx, false);
        if target == ScriptTarget::Source {
            editor.check_project_sources(false);
        } else {
            editor.check_action_source(&script, false);
        }
        let feedback = editor.current_feedback(&script).unwrap();
        assert!(feedback.success, "{:?}", feedback.diagnostics);
        assert!(
            feedback
                .diagnostics
                .iter()
                .any(|d| d.severity == DiagnosticSeverity::Warning
                    && d.code.as_deref() == Some("deprecated-construct")
                    && d.line == 2)
        );
        let output = render(&mut editor, &ctx, false);
        let labels: Vec<_> = output
            .platform_output
            .accesskit_update
            .unwrap()
            .nodes
            .into_iter()
            .filter_map(|(_, node)| node.label().map(str::to_owned))
            .collect();
        assert!(
            labels
                .iter()
                .any(|label| label.starts_with("Line 1, Col 1") && label.contains("1 warning"))
                && !labels.iter().any(|label| label.starts_with("Warning:")),
            "{labels:?}"
        );
        jump_to(&ctx, editor.actions_editor_id(), text, 2, 1);
        let _ = render(&mut editor, &ctx, true);
        assert_eq!(
            source(&editor, target),
            text.replace("not false", "! false")
        );
        assert!(
            editor.actions_ui.edited.is_some(),
            "command edits schedule live diagnostics"
        );
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, original);
        assert!(!editor.history.can_undo());
        // An in-bounds edit from the old snapshot must not be applied to new text.
        let changed = text.replace("not false", "true");
        editor.transact(|editor| match target {
            ScriptTarget::Frame => {
                editor.symbol_mut().timeline.scripts[0].script.source = changed.clone()
            }
            ScriptTarget::Object => {
                editor.selected_element_mut().unwrap().scripts[0]
                    .script
                    .source = changed.clone()
            }
            ScriptTarget::Source => editor.project.sources[0].source = changed.clone(),
        });
        jump_to(&ctx, editor.actions_editor_id(), &changed, 2, 1);
        let _ = render(&mut editor, &ctx, true);
        assert_eq!(source(&editor, target), changed);
        assert!(editor.output.last().unwrap().contains("No quick fix"));
        editor.undo();
        assert_eq!(editor.project, original);
        assert!(!editor.history.can_undo());
        if target == ScriptTarget::Frame {
            let ambiguous = "// two suggestions on the same line\ntrace(not false and true);";
            editor.transact(|editor| {
                editor.symbol_mut().timeline.scripts[0].script.source = ambiguous.into()
            });
            jump_to(&ctx, editor.actions_editor_id(), ambiguous, 2, 1);
            let _ = render(&mut editor, &ctx, true);
            assert_eq!(source(&editor, target), ambiguous);
            assert!(
                ctx.data(
                    |d| d.get_temp::<diagnostics_ui::DiagnosticUi>(diagnostics_ui::state_id(
                        editor.actions_editor_id()
                    ))
                )
                .unwrap()
                .is_open()
            );
            editor.undo();
            assert_eq!(editor.project, original);
            assert!(!editor.history.can_undo());
        }
    }
}

#[test]
fn project_diagnostics_navigate_to_their_source_without_foreign_markers() {
    let ctx = egui::Context::default();
    let mut project = Project::blank();
    project.sources = vec![
        SourceUnit {
            id: "good".into(),
            path: "good.ts".into(),
            language: ScriptLanguage::TypeScript,
            source: "export const count: number = 1;".into(),
        },
        SourceUnit {
            id: "bad".into(),
            path: "bad.ts".into(),
            language: ScriptLanguage::TypeScript,
            source: "// é\nexport const count: number = \"bad\";".into(),
        },
    ];
    project.enable_program();
    let mut editor = Editor::from_project(&ctx, project);
    editor.script_target = ScriptTarget::Source;
    editor.selected_source = Some("good".into());
    editor.check_project_sources(false);
    let diagnostic = editor
        .code_feedback
        .as_ref()
        .unwrap()
        .diagnostics
        .iter()
        .find(|d| d.severity == DiagnosticSeverity::Error && d.path.as_deref() == Some("bad.ts"))
        .expect("project errors carry their source path")
        .clone();
    assert!(!editor.diagnostic_is_current(&diagnostic));
    editor.jump_to_diagnostic(&ctx, "not the error's source", &diagnostic);
    assert_eq!(editor.selected_source.as_deref(), Some("bad"));
    assert!(editor.diagnostic_is_current(&diagnostic));
    let state = egui::text_edit::TextEditState::load(&ctx, editor.actions_editor_id()).unwrap();
    let caret = state.cursor.char_range().unwrap().primary.index;
    assert_eq!(
        nf::code::line_col(&editor.project.sources[1].source, caret),
        (diagnostic.line, diagnostic.column)
    );
    assert!(!editor.history.can_undo());
    editor.project.validate().unwrap();
}

#[test]
fn actions_flow_feedback_is_advisory_scoped_and_tracks_dependencies() {
    let ctx = egui::Context::default();
    let text = "function consume(point:Point):void { trace(point.x); }\nvar wrong:Rectangle = new Rectangle(0,0,10,10);\nconsume(wrong);";
    let script = Script {
        language: ScriptLanguage::ActionScript3,
        source: text.into(),
    };
    let mut project = Project::blank();
    project.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: script.clone(),
    });
    let mut editor = Editor::from_project(&ctx, project);
    let original = editor.project.clone();
    // Exercise the same debounce used by the visible Actions editor, including its delayed run.
    for (time, changed) in [(1., true), (1.7, false)] {
        let _ = ctx.run(
            egui::RawInput {
                time: Some(time),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut response = ui.label("analysis");
                    if changed {
                        response.mark_changed();
                    }
                    editor.live_check(
                        ctx,
                        editor.actions_editor_id(),
                        &response,
                        text,
                        script.language,
                    );
                });
            },
        );
    }
    let feedback = editor.current_feedback(&script).unwrap();
    assert!(
        feedback.success,
        "advisory checking must not redefine compiler support: {:?}",
        feedback.diagnostics
    );
    let diagnostic = feedback
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_deref() == Some("flow-argument-type"))
        .expect("Actions live checks consume semantic analysis");
    assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
    assert_eq!(diagnostic.line, 3);
    assert!(diagnostic.path.is_none());
    assert!(
        diagnostic.fixes.is_empty(),
        "type-flow advice must not rewrite intentional coercions"
    );
    assert_eq!(editor.project, original);
    assert!(!editor.history.can_undo());

    let context = editor.hint_context(script.language).unwrap();
    let reassigned = "var value = new Point(); value = new Rectangle(); value.";
    let hints = context
        .hints(reassigned, reassigned.chars().count(), true)
        .expect("incomplete member access keeps preceding flow facts");
    assert!(hints.items.iter().any(|hint| hint.label == "bottomRight"));
    assert!(!hints.items.iter().any(|hint| hint.label == "length"));
    let joined = "var value = new Point(); if (condition) { value = new Rectangle(); } value.";
    let hints = context.hints(joined, joined.chars().count(), true);
    assert!(
        hints.is_none_or(|hints| hints
            .items
            .iter()
            .all(|hint| hint.label != "bottomRight" && hint.label != "length")),
        "a branch join cannot retain one branch's concrete type"
    );

    for (language, source) in [
        (
            ScriptLanguage::ActionScript2,
            "import flash.geom.Point; import flash.geom.Rectangle; function consume(point:Point):Void {} var wrong:Rectangle = new Rectangle(); consume(wrong);",
        ),
        (
            ScriptLanguage::ActionScript3,
            "function consume(value:Number):Number { return '4'; } consume('3');",
        ),
        (
            ScriptLanguage::ActionScript3,
            "function consume(point:Point):void {} var dynamicValue:* = {}; consume(dynamicValue);",
        ),
    ] {
        let current = Script {
            language,
            source: source.into(),
        };
        editor.check_action_source(&current, false);
        let feedback = editor.current_feedback(&current).unwrap();
        assert!(feedback.success, "{}: {:?}", source, feedback.diagnostics);
        assert!(
            !feedback.diagnostics.iter().any(|d| d
                .code
                .as_deref()
                .is_some_and(|code| code.starts_with("flow-"))),
            "intentional coercion/dynamic flow: {:?}",
            feedback.diagnostics
        );
    }

    let included = Script {
        language: ScriptLanguage::ActionScript2,
        source: "#include \"logic.as\"\ntrace(value);".into(),
    };
    editor.project.resources.push(
        neoflash_core::ResourceUnit::from_bytes(
            "logic",
            "logic.as",
            "text/plain",
            b"var value = 1;",
        )
        .unwrap(),
    );
    editor.check_action_source(&included, false);
    assert!(editor.current_feedback(&included).unwrap().success);
    editor.project.resources[0] = neoflash_core::ResourceUnit::from_bytes(
        "logic",
        "logic.as",
        "text/plain",
        b"var value = ;",
    )
    .unwrap();
    assert!(
        editor.current_feedback(&included).is_none(),
        "include edits invalidate cached acceptance"
    );
    editor.check_action_source(&included, false);
    assert!(!editor.current_feedback(&included).unwrap().success);

    editor.project.resources.clear();
    let class_text = "package { import flash.geom.Point; import flash.geom.Rectangle; public class Helper { public function consume(point:Point):void {} public function run():void { consume(new Rectangle()); } } }";
    editor.project.sources.push(SourceUnit {
        id: "helper-flow".into(),
        path: "Helper.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: class_text.into(),
    });
    editor.project.enable_program();
    editor.script_target = ScriptTarget::Source;
    editor.selected_source = Some("helper-flow".into());
    editor.check_project_sources(false);
    let diagnostic = editor
        .code_feedback
        .as_ref()
        .unwrap()
        .diagnostics
        .iter()
        .find(|d| d.code.as_deref() == Some("flow-argument-type"))
        .expect("project source semantic diagnostics are collected");
    assert_eq!(diagnostic.path.as_deref(), Some("Helper.as"));
    assert!(editor.diagnostic_is_current(diagnostic));

    // Unrelated timeline instances are not implicit fields of a helper class.
    let stage_element = Element {
        id: "named-flow-instance".into(),
        name: "stageOnly".into(),
        ..Element::default()
    };
    editor.project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(stage_element);
    let context = editor.hint_context(ScriptLanguage::ActionScript3).unwrap();
    let hints = context
        .hints(class_text, class_text.chars().count(), true)
        .unwrap();
    assert!(!hints.items.iter().any(|hint| hint.label == "stageOnly"));
    let before = editor.code_fingerprint(&script);
    editor.project.symbols[0].timeline.layers[0].keyframes[0].elements[0].name = "renamed".into();
    assert_ne!(before, editor.code_fingerprint(&script));
    let before = editor.code_fingerprint(&script);
    editor.project.symbols[0].timeline.layers[0].keyframes[0].elements[0]
        .scripts
        .push(neoflash_core::ObjectScript {
            event: neoflash_core::ObjectEvent::Load,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "#initclip\nvar initialized = true;\n#endinitclip".into(),
            },
        });
    assert_ne!(before, editor.code_fingerprint(&script));
}

#[test]
fn conditional_and_data_breakpoints_watch_evaluation_and_live_mutation() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    assert!(!editor.has_breakpoint("Main.as", 12));
    assert!(editor.toggle_breakpoint("Main.as", 12));
    assert!(editor.has_breakpoint("Main.as", 12));
    assert!(!editor.toggle_breakpoint("Main.as", 12));
    assert!(!editor.has_breakpoint("Main.as", 12));

    editor.set_paused_debugger_scopes(vec![DebuggerScope {
        name: "Local".into(),
        variables: vec![
            DebuggerVariable::scalar("score", "40", "Number"),
            DebuggerVariable::object(
                "mc",
                "MovieClip",
                vec![
                    DebuggerVariable::scalar("x", "120.5", "Number"),
                    DebuggerVariable::scalar("visible", "true", "Boolean"),
                ],
            ),
            DebuggerVariable::object(
                "player",
                "Object",
                vec![DebuggerVariable::object(
                    "stats",
                    "Object",
                    vec![
                        DebuggerVariable::scalar("hp", "25", "Number"),
                        DebuggerVariable::scalar("maxHp", "100", "Number"),
                    ],
                )],
            ),
            DebuggerVariable {
                name: "items".into(),
                value: "[Array(2)]".into(),
                kind: "Array".into(),
                children: vec![
                    DebuggerVariable::scalar("0", "potion", "String"),
                    DebuggerVariable::scalar("1", "shield", "String"),
                ],
            },
        ],
    }]);
    assert_eq!(editor.paused_debugger_scopes().len(), 1);

    // Expression evaluation: identifiers, dotted paths, array indexing, arithmetic, comparisons, booleans
    assert_eq!(editor.evaluate_debugger_expression("score").unwrap(), "40");
    assert_eq!(
        editor.evaluate_debugger_expression("mc.x").unwrap(),
        "120.5"
    );
    assert_eq!(
        editor
            .evaluate_debugger_expression("player.stats.hp")
            .unwrap(),
        "25"
    );
    assert_eq!(
        editor.evaluate_debugger_expression("items[0]").unwrap(),
        "potion"
    );
    assert_eq!(
        editor.evaluate_debugger_expression("items[1]").unwrap(),
        "shield"
    );
    assert_eq!(
        editor.evaluate_debugger_expression("score + 10").unwrap(),
        "50"
    );
    assert_eq!(
        editor
            .evaluate_debugger_expression("player.stats.maxHp - player.stats.hp * 2")
            .unwrap(),
        "50"
    );
    assert_eq!(
        editor
            .evaluate_debugger_expression("player.stats.hp <= 0")
            .unwrap(),
        "false"
    );
    assert_eq!(
        editor
            .evaluate_debugger_expression("player.stats.hp <= 25 && mc.visible")
            .unwrap(),
        "true"
    );
    assert!(
        editor
            .evaluate_debugger_expression("unknownVar.prop")
            .is_err()
    );

    // Watches
    let w0 = editor.add_debugger_watch("player.stats.hp");
    let w1 = editor.add_debugger_watch("score + 10");
    let w2 = editor.add_debugger_watch("missingSymbol");
    let watches = editor.debugger_watches();
    assert_eq!(watches[w0].last_value, "25");
    assert!(!watches[w0].is_error);
    assert_eq!(watches[w1].last_value, "50");
    assert!(!watches[w1].is_error);
    assert!(watches[w2].is_error);

    // Conditional breakpoint
    editor.set_breakpoint_spec(
        "Main.as",
        BreakpointSpec::new(20).with_condition("player.stats.hp <= 0"),
    );
    assert!(!editor.should_pause_at_breakpoint("Main.as", 20));
    assert_eq!(editor.breakpoint_spec("Main.as", 20).unwrap().hit_count, 0);

    // Data / property-change breakpoint
    editor.set_breakpoint_spec(
        "Main.as",
        BreakpointSpec::new(30).with_watch_property("player.stats.hp"),
    );
    // Unchanged from baseline -> does not pause
    assert!(!editor.should_pause_at_breakpoint("Main.as", 30));

    // Deep object inspection & live in-pause variable editing
    let hp_var = editor
        .inspect_debugger_variable_path(0, "player.stats.hp")
        .expect("nested variable found");
    assert_eq!(hp_var.value, "25");

    editor
        .set_debugger_variable_value(0, "player.stats.hp", "0")
        .expect("live mutation succeeds");
    assert_eq!(
        editor
            .evaluate_debugger_expression("player.stats.hp")
            .unwrap(),
        "0"
    );
    // Watch automatically refreshes on variable mutation
    assert_eq!(editor.debugger_watches()[w0].last_value, "0");

    // Now both the conditional breakpoint (hp <= 0) and the data breakpoint (hp changed 25 -> 0) pause!
    assert!(editor.should_pause_at_breakpoint("Main.as", 20));
    assert_eq!(editor.breakpoint_spec("Main.as", 20).unwrap().hit_count, 1);
    assert!(editor.should_pause_at_breakpoint("Main.as", 30));
    assert_eq!(editor.breakpoint_spec("Main.as", 30).unwrap().hit_count, 1);
    // Second check without further hp change does not pause data breakpoint
    assert!(!editor.should_pause_at_breakpoint("Main.as", 30));

    assert!(editor.remove_debugger_watch(w2));
    assert_eq!(editor.debugger_watches().len(), 2);
}

#[test]
fn code_folding_and_bounded_script_assist_across_languages() {
    let src = "\
function onEnterFrame() {
    if (ready) {
        trace(\"{not a brace}\");
        step();
    }
}
";
    let regions = compute_fold_regions(src);
    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0].start_line, 1);
    assert_eq!(regions[0].end_line, 6);
    assert_eq!(regions[1].start_line, 2);
    assert_eq!(regions[1].end_line, 5);

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    assert_eq!(editor.visible_source_lines(src).len(), 6);
    assert!(editor.toggle_fold_line(2));
    let visible_inner_folded = editor.visible_source_lines(src);
    // Lines 3, 4, 5 are hidden inside the fold starting on line 2
    assert_eq!(visible_inner_folded.len(), 3);
    assert_eq!(visible_inner_folded[0].0, 1);
    assert_eq!(visible_inner_folded[1].0, 2);
    assert!(visible_inner_folded[1].1.contains('…'));
    assert_eq!(visible_inner_folded[2].0, 6);

    // Unfold line 2
    assert!(!editor.toggle_fold_line(2));
    assert_eq!(editor.visible_source_lines(src).len(), 6);

    // Bounded Script Assist across AS2, AS3, JS, TS
    let stop_as3 = format_script_assist_snippet(
        &ScriptAssistTemplate::StopTimeline,
        ScriptLanguage::ActionScript3,
    );
    assert_eq!(stop_as3, "stop();\n");
    let stop_js = format_script_assist_snippet(
        &ScriptAssistTemplate::StopTimeline,
        ScriptLanguage::JavaScript,
    );
    assert_eq!(stop_js, "this.stop();\n");

    let rel_as2 = format_script_assist_snippet(
        &ScriptAssistTemplate::OnReleaseHandler {
            body: "play();".into(),
        },
        ScriptLanguage::ActionScript2,
    );
    assert!(rel_as2.contains("on (release)"));

    let rel_as3 = format_script_assist_snippet(
        &ScriptAssistTemplate::OnReleaseHandler {
            body: "play();".into(),
        },
        ScriptLanguage::ActionScript3,
    );
    assert!(rel_as3.contains("MouseEvent.CLICK"));

    let ef_ts = format_script_assist_snippet(
        &ScriptAssistTemplate::EnterFrameLoop {
            body: "this.x += 1;".into(),
        },
        ScriptLanguage::TypeScript,
    );
    assert!(ef_ts.contains("\"enterFrame\""));
    assert!(ef_ts.contains(": void =>"));

    let var_as2 = format_script_assist_snippet(
        &ScriptAssistTemplate::VariableDeclaration {
            name: "score".into(),
            var_type: "Number".into(),
            initial_value: "0".into(),
        },
        ScriptLanguage::ActionScript2,
    );
    assert_eq!(var_as2, "var score:Number = 0;\n");

    let var_ts = format_script_assist_snippet(
        &ScriptAssistTemplate::VariableDeclaration {
            name: "score".into(),
            var_type: "number".into(),
            initial_value: "0".into(),
        },
        ScriptLanguage::TypeScript,
    );
    assert_eq!(var_ts, "let score: number = 0;\n");

    // Inserting via Editor method updates the frame script and supports Undo
    editor.script_target = ScriptTarget::Frame;
    assert_eq!(
        editor.default_script_language_for_project(),
        ScriptLanguage::ActionScript3
    );
    assert!(
        editor.insert_script_assist_snippet(ScriptAssistTemplate::GotoAndPlayFrame { frame: 5 })
    );
    assert_eq!(
        editor.symbol().timeline.scripts[0].script.language,
        ScriptLanguage::ActionScript3
    );
    assert_eq!(
        editor.symbol().timeline.scripts[0].script.source,
        "gotoAndPlay(5);\n"
    );
    assert!(editor.history.can_undo());

    // Project with AS2 scripts or explicit project script_language honors that language when inserting new scripts
    let mut as2_proj = Project::blank();
    as2_proj.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "stop();".into(),
        },
    });
    let mut as2_editor = Editor::from_project(&ctx, as2_proj);
    assert_eq!(
        as2_editor.default_script_language_for_project(),
        ScriptLanguage::ActionScript2
    );
    as2_editor.set_project_script_language(ScriptLanguage::ActionScript1);
    assert_eq!(
        as2_editor.default_script_language_for_project(),
        ScriptLanguage::ActionScript1
    );
}
