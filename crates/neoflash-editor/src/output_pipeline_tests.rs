//! Regression and performance tests for the structured `Output` event pipeline (`T-310`).
//!
//! Covers:
//! - Bounded `OutputPipeline` ring retention (`MAX_OUTPUT_EVENTS = 8_192`), 25,000-event high-volume
//!   bursts, O(1) oldest-entry eviction, and strictly monotonic `revision()` across every mutation;
//! - O(1) `(instance_id, document_id, revision, filter)` cache hits across unchanged frames, targeted
//!   cache invalidation on append / coalesce / interior edit / eviction / filter / tab switch / clear,
//!   and virtualized `nf::LogLine` rendering (`OutputVirtualStats`);
//! - Structured `severity` (`Info` / `Warning` / `Error` / `Trace`) + search token composition
//!   (`error:`, `warn:`, `source:compiler`, `debugger:`, plain substring, programmatic `OutputFilter`)
//!   with `Copy output` copying ONLY currently visible (filtered) lines;
//! - Zero duplicate messages across compiler (`Actions` check), runtime (`[preview]`), debugger
//!   (`poll_debugger`), legacy import (`try_import_legacy`), and publish/export warnings;
//! - Click-to-jump navigation from located `nf::LogLine` rows to timeline symbol/frame scripts,
//!   object event handlers, 80-line `.ts` `SourceUnit` spans (including viewport scroll), and
//!   cross-document tabs;
//! - Strict per-document isolation (`DocumentSessions`);
//! - Active `Debugger` session resilience across `Clear output` and high-volume ring-buffer eviction;
//! - Notification (`Show`), Updater, and redacted AI producer hooks.

use crate::ai_connections::AiConnectionError;
use crate::app::{Editor, ScriptTarget};
use crate::debugger_protocol::{
    AuthoredBreakpoint, AuthoredLocation, DebugCommand, DebugFrame, DebugLocal, DebugOutputKind,
    DebugOutputRecord, DebugProtocol, DebugSnapshot, DebugSource, DebugState,
};
use crate::debugger_sources::ScriptOrigin;
use crate::output_pipeline::{
    MAX_OUTPUT_EVENT_BYTES, MAX_OUTPUT_EVENTS, OutputEvent, OutputFilter, OutputLocation,
    OutputPipeline, OutputSeverity, OutputSource,
};
use crate::panels::timeline::output::OUTPUT_VIRTUALIZATION_THRESHOLD;
use crate::updater::{UpdateState, UpdaterConfig};
use crate::workspace::{PanelId, UpdateChannel};
use eframe::egui::{self, Event, PointerButton, Pos2, Rect, Vec2};
use neoflash_core::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

struct TempDir(PathBuf);

impl TempDir {
    fn new(prefix: &str) -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "neoflash-t310-{prefix}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("create temp test dir");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn click_events(pos: Pos2) -> Vec<Event> {
    vec![
        Event::PointerMoved(pos),
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

fn run_output_frame(
    editor: &mut Editor,
    ctx: &egui::Context,
    events: Vec<Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(920., 320.))),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.dock_panel_contents(ui, PanelId::Output);
            });
        },
    )
}

fn run_actions_and_output_frame(
    editor: &mut Editor,
    ctx: &egui::Context,
    events: Vec<Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200., 680.))),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::SidePanel::right("t310-actions-side")
                .exact_width(560.)
                .show(ctx, |ui| {
                    editor.dock_panel_contents(ui, PanelId::Actions);
                });
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.dock_panel_contents(ui, PanelId::Output);
            });
        },
    )
}

fn painted_texts(out: &egui::FullOutput) -> Vec<String> {
    out.shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
            _ => None,
        })
        .collect()
}

fn find_accesskit_button_center(out: &egui::FullOutput, label_substr: &str) -> Option<Pos2> {
    let nodes = &out.platform_output.accesskit_update.as_ref()?.nodes;
    nodes.iter().find_map(|(_, node)| {
        let label = node.label()?;
        if !label.contains(label_substr) {
            return None;
        }
        let b = node.bounds()?;
        Some(Pos2::new(
            ((b.x0 + b.x1) * 0.5) as f32,
            ((b.y0 + b.y1) * 0.5) as f32,
        ))
    })
}

fn find_painted_text_center(out: &egui::FullOutput, needle: &str) -> Option<Pos2> {
    out.shapes.iter().find_map(|clipped| match &clipped.shape {
        egui::Shape::Text(t) if t.galley.job.text.contains(needle) => {
            let rect = Rect::from_min_size(t.pos, t.galley.size());
            Some(rect.center())
        }
        _ => None,
    })
}

fn set_output_filter_text(editor: &Editor, ctx: &egui::Context, filter: &str) {
    let filter_id = egui::Id::new(("neoflash-output-filter", &editor.clipboard.session));
    ctx.data_mut(|d| d.insert_temp(filter_id, filter.to_owned()));
}

#[test]
fn t310_high_volume_25k_burst_ring_eviction_and_monotonic_revision() {
    let mut pipeline = OutputPipeline::new();
    pipeline.set_document_id("session-burst");
    let initial_rev = pipeline.revision();

    let total_burst = 25_000usize;
    let mut prev_rev = initial_rev;
    for i in 0..total_burst {
        pipeline.push_event(OutputEvent::new(
            OutputSeverity::Trace,
            OutputSource::Trace,
            format!("[trace] burst-item-{i:05}"),
        ));
        let rev = pipeline.revision();
        assert!(
            rev > prev_rev,
            "revision must strictly advance on every push and ring eviction at i={i}"
        );
        prev_rev = rev;
    }

    assert_eq!(
        pipeline.len(),
        MAX_OUTPUT_EVENTS,
        "ring buffer must cap retained events at MAX_OUTPUT_EVENTS"
    );
    assert_eq!(
        pipeline.evicted_count(),
        (total_burst - MAX_OUTPUT_EVENTS) as u64
    );

    let first_expected = total_burst - MAX_OUTPUT_EVENTS;
    assert_eq!(
        pipeline.events().first().unwrap().message,
        format!("[trace] burst-item-{first_expected:05}"),
        "oldest overflowed events must be evicted in FIFO order"
    );
    assert_eq!(
        pipeline.events().last().unwrap().message,
        format!("[trace] burst-item-{:05}", total_burst - 1),
        "newest events must be preserved"
    );
    assert!(
        pipeline
            .events()
            .windows(2)
            .all(|pair| pair[0].id < pair[1].id),
        "event IDs must remain strictly monotonic across ring compaction"
    );
    assert!(
        pipeline
            .events()
            .iter()
            .all(|ev| ev.document == "session-burst" && ev.timestamp_ms > 0),
        "every event must carry document session identity and UTC timestamp"
    );

    // Coalescing consecutive identical events bumps revision without growing len().
    let rev_before_coalesce = pipeline.revision();
    pipeline.push_coalesced(OutputEvent::new(
        OutputSeverity::Trace,
        OutputSource::Trace,
        format!("[trace] burst-item-{:05}", total_burst - 1),
    ));
    assert_eq!(pipeline.len(), MAX_OUTPUT_EVENTS);
    assert_eq!(pipeline.events().last().unwrap().repeat_count, 2);
    assert!(pipeline.revision() > rev_before_coalesce);

    // Interior IndexMut mutation bumps revision and resets repeat_count.
    let rev_before_mut = pipeline.revision();
    pipeline[MAX_OUTPUT_EVENTS - 1] = "[trace] edited-tail".into();
    assert!(pipeline.revision() > rev_before_mut);
    assert_eq!(pipeline.events().last().unwrap().repeat_count, 1);

    // Clear bumps revision and resets retained buffer cleanly.
    let rev_before_clear = pipeline.revision();
    pipeline.clear();
    assert!(pipeline.is_empty());
    assert!(pipeline.revision() > rev_before_clear);
}

#[test]
fn t322_output_message_budget_includes_unicode_truncation_marker() {
    let ascii = OutputEvent::info(OutputSource::System, "a".repeat(MAX_OUTPUT_EVENT_BYTES + 1));
    assert_eq!(ascii.message.len(), MAX_OUTPUT_EVENT_BYTES);
    assert!(ascii.message.ends_with('…'));

    let unicode = OutputEvent::info(
        OutputSource::System,
        "é".repeat(MAX_OUTPUT_EVENT_BYTES / 2 + 16),
    );
    assert!(unicode.message.len() <= MAX_OUTPUT_EVENT_BYTES);
    assert!(unicode.message.ends_with('…'));
    assert!(unicode.message.is_char_boundary(unicode.message.len()));
}

#[test]
fn t322_document_identity_sync_updates_auto_stamped_locations_only() {
    let mut pipeline = OutputPipeline::new();
    let placeholder = pipeline.document_id().to_owned();
    pipeline.push_event(
        OutputEvent::info(OutputSource::System, "owned")
            .with_location(OutputLocation::for_file("owned.nflash")),
    );
    pipeline.push_event(
        OutputEvent::info(OutputSource::System, "foreign")
            .with_document("foreign-event")
            .with_location(OutputLocation::for_file("foreign.nflash").with_document("foreign")),
    );
    assert_eq!(
        pipeline.events()[0]
            .location
            .as_ref()
            .and_then(|location| location.document_id.as_deref()),
        Some(placeholder.as_str())
    );

    let revision = pipeline.revision();
    pipeline.set_document_id("document-real");

    assert!(pipeline.revision() > revision);
    assert_eq!(pipeline.events()[0].document, "document-real");
    assert_eq!(
        pipeline.events()[0]
            .location
            .as_ref()
            .and_then(|location| location.document_id.as_deref()),
        Some("document-real")
    );
    assert_eq!(pipeline.events()[1].document, "foreign-event");
    assert_eq!(
        pipeline.events()[1]
            .location
            .as_ref()
            .and_then(|location| location.document_id.as_deref()),
        Some("foreign")
    );

    let unchanged_revision = pipeline.revision();
    pipeline.set_document_id("document-real");
    assert_eq!(pipeline.revision(), unchanged_revision);
}

#[test]
fn t310_o1_cache_hit_and_invalidation_across_mutations_and_virtualization() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::blank());

    for i in 0..2_000 {
        editor.push_output_event(OutputEvent::new(
            OutputSeverity::Trace,
            OutputSource::Trace,
            format!("[trace] row-{i:04}"),
        ));
    }

    let _ = run_output_frame(&mut editor, &ctx, vec![]);
    let gen_initial = editor
        .last_output_cache_generation(&ctx)
        .expect("initial Output render populates revision-keyed cache");
    let stats = editor
        .last_output_virtual_stats(&ctx)
        .expect("virtualization stats recorded");
    assert_eq!(stats.total_lines, 2_000);
    assert!(
        stats.drawn_rows <= OUTPUT_VIRTUALIZATION_THRESHOLD,
        "virtualized viewport must paint only visible slice + overscan, got {}",
        stats.drawn_rows
    );

    // 5 consecutive unchanged frames: cache generation MUST NOT change (O(1) hit, zero rebuilds).
    for _ in 0..5 {
        let _ = run_output_frame(&mut editor, &ctx, vec![]);
        assert_eq!(
            editor.last_output_cache_generation(&ctx),
            Some(gen_initial),
            "unchanged Output pipeline + filter must hit O(1) cache without rebuilding"
        );
    }

    // 1. Append invalidates cache once.
    editor.push_output_event(OutputEvent::warning(
        OutputSource::Compiler,
        "Warning: implicit variable declaration",
    ));
    let _ = run_output_frame(&mut editor, &ctx, vec![]);
    let gen_after_append = editor.last_output_cache_generation(&ctx).unwrap();
    assert!(gen_after_append > gen_initial);

    // 2. Consecutive coalesce (`repeat_count` -> 2) invalidates cache and renders `(×2)`.
    editor.push_output_coalesced(OutputEvent::warning(
        OutputSource::Compiler,
        "Warning: implicit variable declaration",
    ));
    let out_coalesced = run_output_frame(&mut editor, &ctx, vec![]);
    let gen_after_coalesce = editor.last_output_cache_generation(&ctx).unwrap();
    assert!(gen_after_coalesce > gen_after_append);
    assert!(
        painted_texts(&out_coalesced)
            .iter()
            .any(|t| t.contains("Warning: implicit variable declaration (×2)")),
        "coalesced event must render repeat suffix (×2)"
    );

    // 3. Interior replacement (`IndexMut`) invalidates cache.
    editor.output[2_000] = "Warning: edited warning text".into();
    let _ = run_output_frame(&mut editor, &ctx, vec![]);
    let gen_after_interior = editor.last_output_cache_generation(&ctx).unwrap();
    assert!(gen_after_interior > gen_after_coalesce);

    // 4. Filter query change invalidates cache; steady filter hits cache again.
    set_output_filter_text(&editor, &ctx, "warn:");
    let _ = run_output_frame(&mut editor, &ctx, vec![]);
    let gen_after_filter = editor.last_output_cache_generation(&ctx).unwrap();
    assert!(gen_after_filter > gen_after_interior);
    let _ = run_output_frame(&mut editor, &ctx, vec![]);
    assert_eq!(
        editor.last_output_cache_generation(&ctx),
        Some(gen_after_filter)
    );

    // 5. Ring eviction at capacity invalidates cache even when `len()` stays `MAX_OUTPUT_EVENTS`.
    set_output_filter_text(&editor, &ctx, "");
    while editor.output.len() < MAX_OUTPUT_EVENTS {
        editor.push_output_event(OutputEvent::trace("[trace] fill-cap"));
    }
    let _ = run_output_frame(&mut editor, &ctx, vec![]);
    let gen_at_cap = editor.last_output_cache_generation(&ctx).unwrap();
    assert_eq!(editor.output.len(), MAX_OUTPUT_EVENTS);

    editor.push_output_event(OutputEvent::trace("[trace] overflow-cap-by-one"));
    assert_eq!(editor.output.len(), MAX_OUTPUT_EVENTS);
    let _ = run_output_frame(&mut editor, &ctx, vec![]);
    let gen_after_evict = editor.last_output_cache_generation(&ctx).unwrap();
    assert!(
        gen_after_evict > gen_at_cap,
        "ring eviction at constant len() must still invalidate cache via monotonic revision"
    );
}

#[test]
fn t310_severity_and_search_work_together_and_copy_copies_only_visible_lines() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::blank());

    editor.push_output_event(OutputEvent::info(
        OutputSource::Publish,
        "HTML5 exported: /tmp/demo/index.html",
    ));
    editor.push_output_event(OutputEvent::warning(
        OutputSource::Import,
        "DOMDocument.xml: Font 'Avenir' approximated",
    ));
    editor.push_output_event(
        OutputEvent::error(
            OutputSource::Compiler,
            "Line 4:10: Undefined identifier 'heroClip'",
        )
        .with_location(OutputLocation::for_frame(
            "sym-1",
            Some("Hero".into()),
            Some(0),
            0,
        )),
    );
    editor.push_output_event(OutputEvent::error(
        OutputSource::Debugger,
        "Runtime error (Scene 1 · Frame 1 · line 7): Null reference",
    ));
    editor.push_output_event(OutputEvent::trace("[trace] player spawned"));

    // Programmatic structured filter (`OutputFilter`) combines severity, source, document, and text.
    let compiler_errors = editor.output.filter_events(
        &OutputFilter::new()
            .with_severity(OutputSeverity::Error)
            .with_source(OutputSource::Compiler)
            .with_document(editor.clipboard.session.clone())
            .with_search("heroclip"),
    );
    assert_eq!(compiler_errors.len(), 1);
    assert!(compiler_errors[0].message.contains("heroClip"));

    // NF2 Output dock composition check: Search + Copy + Clear header, no extra chrome controls,
    // and real NF2 `Severity` colors painted on `LogLine` rows (`danger_default` & `warning_default`).
    let unfiltered_out = run_output_frame(&mut editor, &ctx, vec![]);
    assert!(
        find_accesskit_button_center(&unfiltered_out, "Copy output").is_some(),
        "Copy output button preserved"
    );
    assert!(
        find_accesskit_button_center(&unfiltered_out, "Clear output").is_some(),
        "Clear output button preserved"
    );
    let palette = neoflash_ui::palette(&ctx);
    let has_colored_error = unfiltered_out.shapes.iter().any(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.job.text.contains("Undefined identifier") => t
            .galley
            .job
            .sections
            .iter()
            .any(|sec| sec.format.color == palette.danger_default),
        _ => false,
    });
    assert!(
        has_colored_error,
        "Error severity must map to nf::code::Severity::Error in nf::LogLine"
    );

    // Search query combining severity + source + text (`"error: source:compiler hero"`), then click Copy.
    set_output_filter_text(&editor, &ctx, "error: source:compiler hero");
    let filtered_out = run_output_frame(&mut editor, &ctx, vec![]);
    let visible = painted_texts(&filtered_out);
    assert!(
        visible
            .iter()
            .any(|t| t.contains("Undefined identifier 'heroClip'"))
    );
    assert!(!visible.iter().any(|t| t.contains("Null reference")));
    assert!(!visible.iter().any(|t| t.contains("HTML5 exported")));

    let copy_pos =
        find_accesskit_button_center(&filtered_out, "Copy output").expect("Copy output enabled");
    let copy_out = run_output_frame(&mut editor, &ctx, click_events(copy_pos));
    let copied = copy_out
        .platform_output
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .expect("Copy output must emit CopyText command");
    assert_eq!(
        copied, "Line 4:10: Undefined identifier 'heroClip'",
        "Copy output must copy ONLY the currently visible (filtered) lines"
    );

    // Also verify `warn:` token + substring filtering.
    set_output_filter_text(&editor, &ctx, "warn: avenir");
    let warn_out = run_output_frame(&mut editor, &ctx, vec![]);
    let warn_copy_pos = find_accesskit_button_center(&warn_out, "Copy output").unwrap();
    let warn_copy_out = run_output_frame(&mut editor, &ctx, click_events(warn_copy_pos));
    let warn_copied = warn_copy_out
        .platform_output
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(warn_copied, "DOMDocument.xml: Font 'Avenir' approximated");
}

#[test]
fn t310_zero_duplicate_messages_across_compiler_runtime_debugger_import_and_export() {
    let temp = TempDir::new("dedup");
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());

    // 1. Compiler (`Actions` check): checking identical faulty code twice coalesces without duplicate rows.
    let faulty_script = Script {
        language: ScriptLanguage::ActionScript2,
        source: "var broken = ;".into(),
    };
    let root_id = editor.project.root.clone();
    editor
        .project
        .symbol_mut(&root_id)
        .unwrap()
        .timeline
        .scripts
        .push(FrameScript {
            layer: None,
            frame: 0,
            script: faulty_script.clone(),
        });
    editor.script_target = ScriptTarget::Frame;
    editor.frame = 0;
    editor.check_action_source(&faulty_script, true);
    let count_after_first_check = editor
        .output
        .events()
        .iter()
        .filter(|e| e.source == OutputSource::Compiler)
        .count();
    assert!(count_after_first_check >= 1);
    editor.check_action_source(&faulty_script, true);
    let count_after_second_check = editor
        .output
        .events()
        .iter()
        .filter(|e| e.source == OutputSource::Compiler)
        .count();
    assert_eq!(
        count_after_second_check, count_after_first_check,
        "repeated compiler check on identical error must not append duplicate Output events"
    );

    // 2. Runtime (`[preview]` diagnostic): repeated authored preview warnings deduplicate via `push_output_unique`.
    editor.output.clear();
    let root_id = editor.project.root.clone();
    editor
        .project
        .symbol_mut(&root_id)
        .unwrap()
        .timeline
        .frame_count = 3;
    editor
        .project
        .symbol_mut(&root_id)
        .unwrap()
        .timeline
        .scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "nonexistentNativeBridgeCall();".into(),
        },
    }];
    editor.playing = true;
    editor.sync_authored_preview();
    editor.tick_authored_preview();
    editor.playing = false;
    editor.reset_authored_preview();
    editor.playing = true;
    editor.sync_authored_preview();
    editor.tick_authored_preview();
    editor.playing = false;
    let preview_diag_count = editor
        .output
        .events()
        .iter()
        .filter(|e| {
            e.source == OutputSource::Runtime
                && e.severity == OutputSeverity::Warning
                && e.message.starts_with("[preview]")
        })
        .count();
    assert!(
        preview_diag_count <= 1,
        "identical [preview] runtime diagnostics must never duplicate across ticks: {:?}",
        editor.output.events()
    );

    // 3. Debugger (`poll_debugger`): polling repeatedly and receiving consecutive identical runtime errors.
    editor.output.clear();
    let snapshot = DebugSnapshot {
        epoch: 1,
        state: DebugState::Running,
        reason: String::new(),
        frames: vec![],
        selected_frame: None,
        locals: vec![],
        breakpoints: vec![],
        messages: vec![],
        structured_messages: vec![],
        output_events: vec![],
        structured_output: vec![
            DebugOutputRecord {
                seq: 1,
                kind: DebugOutputKind::Error,
                formatted: "Runtime error (Scene 1 · Frame 1 · line 1): Boom".into(),
                location: Some(AuthoredLocation {
                    source: "Scene 1 · Frame 1".into(),
                    line: 1,
                    column: 1,
                }),
            },
            DebugOutputRecord {
                seq: 2,
                kind: DebugOutputKind::Error,
                formatted: "Runtime error (Scene 1 · Frame 1 · line 1): Boom".into(),
                location: Some(AuthoredLocation {
                    source: "Scene 1 · Frame 1".into(),
                    line: 1,
                    column: 1,
                }),
            },
        ],
        command_pending: false,
    };
    let shared = Arc::new(Mutex::new(snapshot));
    let (tx, _rx) = mpsc::sync_channel(8);
    editor
        .debugger
        .install_test_session(editor.project.clone(), shared, tx, vec![]);
    editor.poll_debugger(&ctx);
    editor.poll_debugger(&ctx);
    let boom_events: Vec<_> = editor
        .output
        .events()
        .iter()
        .filter(|e| e.message.contains("Boom"))
        .collect();
    assert_eq!(
        boom_events.len(),
        1,
        "consecutive identical debugger errors must coalesce into a single event across polls"
    );
    assert_eq!(boom_events[0].repeat_count, 2);
    assert_eq!(boom_events[0].severity, OutputSeverity::Error);
    assert_eq!(boom_events[0].source, OutputSource::Debugger);
    editor.debugger.stop();

    // 4. Legacy import (`try_import_legacy`): structured Import events carry file location and never duplicate.
    let xfl_dir = temp.0.join("sample-xfl");
    std::fs::create_dir_all(&xfl_dir).unwrap();
    let dom_xml = xfl_dir.join("DOMDocument.xml");
    std::fs::write(
        &dom_xml,
        r#"<DOMDocument platform="Windows" width="120" height="80" frameRate="24"><timelines><DOMTimeline name="Scene 1"><layers><DOMLayer name="Artwork"><frames><DOMFrame index="0" duration="1"><elements><DOMRectangleObject objectWidth="10" objectHeight="10"/></elements></DOMFrame></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#,
    )
    .unwrap();
    assert!(editor.try_open_path(dom_xml.clone(), &ctx));
    let import_events: Vec<_> = editor
        .output
        .events()
        .iter()
        .filter(|e| e.source == OutputSource::Import)
        .collect();
    assert!(!import_events.is_empty());
    let mut unique_msgs = std::collections::HashSet::new();
    for ev in &import_events {
        assert!(
            unique_msgs.insert(ev.message.clone()),
            "import diagnostics must have zero duplicate messages: {:?}",
            import_events
        );
        assert_eq!(
            ev.location.as_ref().and_then(|l| l.file_path.as_ref()),
            Some(&dom_xml)
        );
    }

    // 5. Export warnings (`emit_export_warnings` via `publish_html5_with_settings`): publishing twice does not duplicate warnings.
    editor.workspace.publish.warnings_as_errors = false;
    editor.add_element(
        ElementKind::TextBox {
            text: "Hello".into(),
            width: 120.,
            height: 40.,
            multiline: false,
            word_wrap: false,
            format: TextFormat {
                font: "NeoFlashMissingExportFont_T310".into(),
                ..Default::default()
            },
        },
        10.,
        10.,
    );
    editor.output.clear();
    let runtime_dir = temp.0.join("runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::write(runtime_dir.join("neoflash_web.js"), "var NeoFlashWasm;").unwrap();
    std::fs::write(runtime_dir.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let publish_out = temp.0.join("html5-out");
    let prefs = editor.workspace.publish.clone();
    assert!(editor.publish_html5_with_settings(&publish_out, &runtime_dir, &prefs));
    assert!(editor.publish_html5_with_settings(&publish_out, &runtime_dir, &prefs));
    let missing_font_warnings = editor
        .output
        .events()
        .iter()
        .filter(|e| {
            e.source == OutputSource::Publish
                && e.severity == OutputSeverity::Warning
                && e.message.contains("NeoFlashMissingExportFont_T310")
        })
        .count();
    assert_eq!(
        missing_font_warnings, 1,
        "identical export warnings must not duplicate across consecutive publishes"
    );
}

#[test]
fn t310_click_output_row_jumps_to_symbol_frame_object_and_source_line_span() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut project = Project::blank();
    project.format_version = 8;

    // Add a non-root MovieClip symbol with a multi-line frame script on frame 3 (0-based index 2)
    // and an object Click handler on frame 0.
    let clip_id = "sym-enemy".to_owned();
    let btn_elem_id = "elem-trigger".to_owned();
    let frame_script_src =
        "// line 1\n// line 2\nlet brokenCall = unknownEnemyHelper(42);\n// line 4\n".to_owned();
    let object_script_src = "on(release) {\n    missingButtonTarget();\n}\n".to_owned();
    project.symbols.push(Symbol {
        id: clip_id.clone(),
        name: "EnemyClip".into(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 5,
            layers: vec![Layer {
                id: "layer-enemy-0".into(),
                name: "Actions".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: btn_elem_id.clone(),
                        name: "trigger_btn".into(),
                        transform: Transform::default(),
                        kind: ElementKind::Rectangle {
                            width: 40.,
                            height: 20.,
                            fill: [200, 60, 60, 255],
                            stroke: [0, 0, 0, 255],
                            stroke_width: 1.,
                        },
                        scripts: vec![ObjectScript {
                            event: ObjectEvent::Click,
                            script: Script {
                                language: ScriptLanguage::ActionScript2,
                                source: object_script_src.clone(),
                            },
                        }],
                        ..Element::default()
                    }],
                    ..Default::default()
                }],
            }],
            scripts: vec![FrameScript {
                layer: None,
                frame: 2,
                script: Script {
                    language: ScriptLanguage::TypeScript,
                    source: frame_script_src.clone(),
                },
            }],
            guides: vec![],
        },
        linkage_id: None,
        class_name: None,
        folder: String::new(),
    });

    // Add an 85-line `.ts` SourceUnit so jumping to line 72 must scroll `actions_source_scroll`.
    let mut long_source = String::new();
    for line_no in 1..=85 {
        if line_no == 72 {
            long_source.push_str("    const badToken = undefinedSymbolAtLine72;\n");
        } else {
            long_source.push_str(&format!("// padding line {line_no:02}\n"));
        }
    }
    project.sources.push(SourceUnit {
        id: "src-game-controller".into(),
        path: "scripts/GameController.ts".into(),
        language: ScriptLanguage::TypeScript,
        source: long_source.clone(),
    });

    let mut editor = Editor::from_project(&ctx, project);

    // Emit 3 located errors into Output:
    // (A) 85-line SourceUnit error spanning columns 22..45 on line 72
    editor.push_output_event(
        OutputEvent::error(
            OutputSource::Compiler,
            "GameController.ts:72:22: Cannot find name 'undefinedSymbolAtLine72'",
        )
        .with_location(
            OutputLocation::for_source("src-game-controller", "scripts/GameController.ts")
                .with_span(72, 22, 72, 45),
        ),
    );
    // (B) Non-root symbol `EnemyClip` frame 2 script error on line 3 col 18..36
    editor.push_output_event(
        OutputEvent::error(
            OutputSource::Compiler,
            "EnemyClip · Frame 3:3:18: Unknown helper 'unknownEnemyHelper'",
        )
        .with_location(
            OutputLocation::for_frame(&clip_id, Some("EnemyClip".into()), Some(0), 2)
                .with_span(3, 18, 3, 36),
        ),
    );
    // (C) Object Click script error on line 2 col 5
    editor.push_output_event(
        OutputEvent::error(
            OutputSource::Debugger,
            "EnemyClip · trigger_btn:2:5: missingButtonTarget is not defined",
        )
        .with_location(
            OutputLocation::for_object(
                &clip_id,
                Some("EnemyClip".into()),
                Some(0),
                0,
                &btn_elem_id,
                ObjectEvent::Click,
            )
            .with_line_col(2, 5),
        ),
    );

    // Render Actions + Output together and click row (A): `GameController.ts:72:22`.
    let out_initial = run_actions_and_output_frame(&mut editor, &ctx, vec![]);
    let row_a_pos = find_painted_text_center(&out_initial, "undefinedSymbolAtLine72")
        .expect("located SourceUnit Output row painted");
    let _ = run_actions_and_output_frame(&mut editor, &ctx, click_events(row_a_pos));
    // Run one follow-up layout pass so `pending-scroll-to-caret` executes inside Actions `ScrollArea`.
    let _ = run_actions_and_output_frame(&mut editor, &ctx, vec![]);

    assert!(editor.script_target == ScriptTarget::Source);
    assert_eq!(
        editor.selected_source.as_deref(),
        Some("src-game-controller")
    );
    let range_a = egui::text_edit::TextEditState::load(&ctx, editor.actions_editor_id())
        .and_then(|s| s.cursor.char_range())
        .expect("clicking Output row selects exact diagnostic span in Actions");
    let expected_start_a = neoflash_ui::components::code::char_index(&long_source, 72, 22);
    let expected_end_a = neoflash_ui::components::code::char_index(&long_source, 72, 45);
    assert_eq!(range_a.primary.index, expected_start_a);
    assert_eq!(range_a.secondary.index, expected_end_a);

    let scroll_offset_y: f32 = ctx
        .data(|d| d.get_temp(editor.actions_editor_id().with("last-scroll-offset-y")))
        .expect("Actions ScrollArea offset recorded");
    assert!(
        scroll_offset_y > 200.0,
        "jumping to line 72 of 85 must scroll Actions viewport down, got offset.y={scroll_offset_y}"
    );

    // Click row (B): non-root symbol `EnemyClip` frame 2, line 3.
    let out_b = run_actions_and_output_frame(&mut editor, &ctx, vec![]);
    let row_b_pos =
        find_painted_text_center(&out_b, "unknownEnemyHelper").expect("frame script row painted");
    let _ = run_actions_and_output_frame(&mut editor, &ctx, click_events(row_b_pos));
    assert_eq!(editor.symbol_id(), clip_id);
    assert_eq!(editor.frame, 2);
    assert!(editor.script_target == ScriptTarget::Frame);
    let range_b = egui::text_edit::TextEditState::load(&ctx, editor.actions_editor_id())
        .and_then(|s| s.cursor.char_range())
        .unwrap();
    assert_eq!(
        range_b.primary.index,
        neoflash_ui::components::code::char_index(&frame_script_src, 3, 18)
    );

    // Click row (C): object handler script on `btn_elem_id`.
    let out_c = run_actions_and_output_frame(&mut editor, &ctx, vec![]);
    let row_c_pos =
        find_painted_text_center(&out_c, "missingButtonTarget").expect("object script row painted");
    let _ = run_actions_and_output_frame(&mut editor, &ctx, click_events(row_c_pos));
    assert_eq!(editor.symbol_id(), clip_id);
    assert_eq!(editor.frame, 0);
    assert!(editor.script_target == ScriptTarget::Object);
    assert_eq!(editor.script_event, ObjectEvent::Click);
    let selected_elements: Vec<_> = editor
        .selection
        .targets
        .iter()
        .map(|t| t.element.clone())
        .collect();
    assert_eq!(selected_elements, vec![btn_elem_id]);
}

#[test]
fn t310_strict_document_session_isolation_and_cross_tab_jump() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.project.name = "Doc Alpha".into();
    let doc0_id = editor.clipboard.session.clone();
    editor.push_output_event(OutputEvent::info(OutputSource::System, "alpha-only-event"));

    editor.new_document_session(&ctx);
    editor.project.name = "Doc Beta".into();
    let doc1_id = editor.clipboard.session.clone();
    assert_ne!(doc0_id, doc1_id);
    assert!(
        editor.output.is_empty(),
        "newly opened document session must start with an empty isolated Output pipeline"
    );
    editor.push_output_event(OutputEvent::warning(
        OutputSource::Compiler,
        "beta-only-warning",
    ));

    editor.new_document_session(&ctx);
    editor.project.name = "Doc Gamma".into();
    let doc2_id = editor.clipboard.session.clone();
    editor.push_output_event(OutputEvent::error(
        OutputSource::Debugger,
        "gamma-only-error",
    ));

    // Switch across all three tabs and verify zero mixing both in data and in painted Output UI.
    assert!(editor.switch_session_tab(&ctx, 0));
    assert_eq!(editor.output.document_id(), doc0_id);
    assert_eq!(editor.output, vec!["alpha-only-event".to_owned()]);
    let out0 = run_output_frame(&mut editor, &ctx, vec![]);
    let texts0 = painted_texts(&out0);
    assert!(texts0.iter().any(|t| t.contains("alpha-only-event")));
    assert!(!texts0.iter().any(|t| t.contains("beta-only-warning")));
    assert!(!texts0.iter().any(|t| t.contains("gamma-only-error")));

    assert!(editor.switch_session_tab(&ctx, 1));
    assert_eq!(editor.output.document_id(), doc1_id);
    assert_eq!(editor.output, vec!["beta-only-warning".to_owned()]);
    let out1 = run_output_frame(&mut editor, &ctx, vec![]);
    let texts1 = painted_texts(&out1);
    assert!(texts1.iter().any(|t| t.contains("beta-only-warning")));
    assert!(!texts1.iter().any(|t| t.contains("alpha-only-event")));

    assert!(editor.switch_session_tab(&ctx, 2));
    assert_eq!(editor.output.document_id(), doc2_id);
    assert_eq!(editor.output, vec!["gamma-only-error".to_owned()]);

    // Cross-document location jump switches back to tab 0 when `document_id` targets `doc0_id`.
    let cross_loc = OutputLocation {
        document_id: Some(doc0_id.clone()),
        frame_index: Some(0),
        ..Default::default()
    };
    assert!(editor.navigate_to_output_location(&ctx, &cross_loc));
    assert_eq!(editor.active_session, 0);
    assert_eq!(editor.clipboard.session, doc0_id);
}

#[test]
fn t310_clear_and_ring_eviction_never_break_active_debugger_session() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();

    let mut project = Project::blank();
    let root = project.root.clone();
    project.symbol_mut(&root).unwrap().timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "var score = 10;\ntrace(score);\nscore += 5;\n".into(),
        },
    }];
    let mut editor = Editor::from_project(&ctx, project);

    assert!(editor.toggle_breakpoint("Scene 1 · Frame 1", 2));
    let bp = AuthoredBreakpoint {
        source: "Scene 1 · Frame 1".into(),
        line: 2,
        column: None,
    };

    let debug_sources = vec![DebugSource {
        source_url: "file:///tmp/t310-bundle.js".into(),
        code: "var score = 10;\ntrace(score);\nscore += 5;\n".into(),
        locations: vec![
            neoflash_script::RuntimeSourceLocation {
                generated_line: 1,
                generated_column: 1,
                source: "Scene 1 · Frame 1".into(),
                line: 1,
                column: 1,
            },
            neoflash_script::RuntimeSourceLocation {
                generated_line: 2,
                generated_column: 1,
                source: "Scene 1 · Frame 1".into(),
                line: 2,
                column: 1,
            },
            neoflash_script::RuntimeSourceLocation {
                generated_line: 3,
                generated_column: 1,
                source: "Scene 1 · Frame 1".into(),
                line: 3,
                column: 1,
            },
        ],
    }];
    let mut protocol = DebugProtocol::new(
        "file:///tmp/t310-index.html".into(),
        debug_sources,
        vec![bp.clone()],
    );
    protocol.start();
    let _ = protocol.drain_commands();

    let shared_snapshot = Arc::new(Mutex::new(DebugSnapshot {
        epoch: 1,
        state: DebugState::Paused,
        reason: "Paused on breakpoint".into(),
        selected_frame: Some(0),
        frames: vec![DebugFrame {
            function_name: "frame_1".into(),
            location: Some(AuthoredLocation {
                source: "Scene 1 · Frame 1".into(),
                line: 2,
                column: 1,
            }),
            generated_url: "file:///tmp/t310-bundle.js".into(),
            generated_line: 2,
            generated_column: 1,
        }],
        locals: vec![DebugLocal {
            scope: "Local".into(),
            name: "score".into(),
            kind: "number".into(),
            value: "10".into(),
            accessor: false,
        }],
        breakpoints: vec![],
        messages: vec![],
        structured_messages: vec![],
        output_events: vec![],
        structured_output: vec![DebugOutputRecord {
            seq: 1,
            kind: DebugOutputKind::Trace,
            formatted: "pre-clear-trace-10".into(),
            location: Some(AuthoredLocation {
                source: "Scene 1 · Frame 1".into(),
                line: 2,
                column: 1,
            }),
        }],
        command_pending: false,
    }));

    let (cmd_tx, cmd_rx) = mpsc::sync_channel(16);
    let origins = vec![ScriptOrigin {
        language: ScriptLanguage::ActionScript2,
        source: "var score = 10;\ntrace(score);\nscore += 5;\n".into(),
        location: "Scene 1 · Frame 1".into(),
    }];
    editor.debugger.install_test_session(
        editor.project.clone(),
        Arc::clone(&shared_snapshot),
        cmd_tx,
        origins,
    );

    editor.poll_debugger(&ctx);
    assert!(
        editor
            .output
            .events()
            .iter()
            .any(|e| e.message == "pre-clear-trace-10")
    );

    // 1. Click `Clear output` in the real Output panel UI while debugger is paused!
    let out_before_clear = run_output_frame(&mut editor, &ctx, vec![]);
    let clear_pos = find_accesskit_button_center(&out_before_clear, "Clear output")
        .expect("Clear output button enabled");
    let _ = run_output_frame(&mut editor, &ctx, click_events(clear_pos));
    assert!(
        editor.output.is_empty(),
        "Clear output empties Output buffer"
    );

    // Debugger session, breakpoints, locals, and command channel MUST remain 100% intact,
    // and polling again MUST NOT re-emit cleared `seq == 1` history!
    editor.poll_debugger(&ctx);
    assert!(
        editor.output.is_empty(),
        "polling after Clear output must not resurrect cleared debugger traces"
    );
    assert!(editor.debugger.active());
    assert!(editor.debugger.commandable());
    assert_eq!(editor.debugger.breakpoints, vec![bp]);
    let snap = editor.debugger.snapshot().unwrap();
    assert_eq!(snap.state, DebugState::Paused);
    assert_eq!(snap.locals[0].name, "score");
    assert_eq!(snap.locals[0].value, "10");
    assert!(editor.debug_command(DebugCommand::StepOver));
    assert!(matches!(cmd_rx.try_recv(), Ok(DebugCommand::StepOver)));

    // 2. Flood Output past `MAX_OUTPUT_EVENTS` (10,000 events -> 1,808 ring evictions).
    for i in 0..10_000 {
        editor.push_output_event(OutputEvent::trace(format!("[trace] flood-{i}")));
    }
    assert_eq!(editor.output.len(), MAX_OUTPUT_EVENTS);
    assert!(editor.output.evicted_count() > 0);
    assert!(editor.debugger.active());
    assert!(editor.debugger.commandable());

    // 3. Emit subsequent debugger trace (`seq: 2`) and runtime error (`seq: 3`) — must stream cleanly!
    {
        let mut locked = shared_snapshot.lock().unwrap();
        locked.state = DebugState::Running;
        locked.reason.clear();
        locked.structured_output.push(DebugOutputRecord {
            seq: 2,
            kind: DebugOutputKind::Trace,
            formatted: "post-eviction-trace-15".into(),
            location: Some(AuthoredLocation {
                source: "Scene 1 · Frame 1".into(),
                line: 2,
                column: 1,
            }),
        });
        locked.structured_output.push(DebugOutputRecord {
            seq: 3,
            kind: DebugOutputKind::Error,
            formatted: "Runtime error (Scene 1 · Frame 1 · line 3): Post-clear exception".into(),
            location: Some(AuthoredLocation {
                source: "Scene 1 · Frame 1".into(),
                line: 3,
                column: 1,
            }),
        });
    }
    editor.poll_debugger(&ctx);

    let tail_events = &editor.output.events()[editor.output.len() - 2..];
    assert_eq!(tail_events[0].message, "post-eviction-trace-15");
    assert_eq!(tail_events[0].severity, OutputSeverity::Trace);
    assert_eq!(tail_events[0].source, OutputSource::Debugger);
    assert_eq!(
        tail_events[1].message,
        "Runtime error (Scene 1 · Frame 1 · line 3): Post-clear exception"
    );
    assert_eq!(tail_events[1].severity, OutputSeverity::Error);
    assert_eq!(
        tail_events[1].location.as_ref().and_then(|l| l.line),
        Some(3)
    );

    assert!(editor.debug_command(DebugCommand::Resume));
    assert!(matches!(cmd_rx.try_recv(), Ok(DebugCommand::Resume)));
    editor.debugger.stop();
}

#[test]
fn t310_notifications_updater_and_ai_producer_hooks() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());

    // 1. Notifications preserve ToastType severity (`Warning` / `Error`) and deduplicate.
    editor.notify_with_output(
        neoflash_ui::components::ToastType::Warning,
        "Disk space is low while autosaving.",
    );
    editor.notify_with_output(
        neoflash_ui::components::ToastType::Warning,
        "Disk space is low while autosaving.",
    );
    assert_eq!(editor.output.len(), 1);
    assert_eq!(editor.output.events()[0].severity, OutputSeverity::Warning);

    // 2. Redacted AI producer hooks (`OutputSource::Ai`, secret token redaction).
    editor.push_ai_output(
        OutputSeverity::Info,
        "Bearer sk-secret-token-1234567890 connected to local endpoint",
    );
    let ai_info = editor.output.events().last().unwrap();
    assert_eq!(ai_info.source, OutputSource::Ai);
    assert!(!ai_info.message.contains("sk-secret-token-1234567890"));
    assert!(ai_info.message.contains("[REDACTED_SECRET]"));

    editor.report_ai_error(&AiConnectionError::RateLimited {
        retry_after_ms: Some(12_000),
        message: "OpenAI rate limit reached".into(),
    });
    let ai_err = editor.output.events().last().unwrap();
    assert_eq!(ai_err.source, OutputSource::Ai);
    assert_eq!(ai_err.severity, OutputSeverity::Warning);
    assert!(ai_err.message.contains("OpenAI"));

    // 3. Updater (`poll_updater` -> `OutputSource::Updater`) over a loopback fixture server.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_bg = Arc::clone(&stop);
    let release_body = serde_json::to_vec(&serde_json::json!([
        {
            "tag_name": "v99.1.0",
            "html_url": format!("http://{addr}/jazzymiles/NeoFlash/releases/tag/v99.1.0"),
            "draft": false,
            "prerelease": false,
            "assets": [
                {
                    "name": "NeoFlash-99.1.0-macos-arm64.dmg",
                    "browser_download_url": format!("http://{addr}/NeoFlash-99.1.0-macos-arm64.dmg"),
                    "size": 128
                },
                {
                    "name": "SHA256SUMS.txt",
                    "browser_download_url": format!("http://{addr}/SHA256SUMS.txt"),
                    "size": 96
                }
            ]
        }
    ]))
    .unwrap();
    let mut routes = HashMap::new();
    routes.insert(
        "/repos/jazzymiles/NeoFlash/releases?per_page=20".to_owned(),
        release_body,
    );
    let server = std::thread::spawn(move || {
        while !stop_bg.load(Ordering::Acquire) {
            if let Ok((stream, _)) = listener.accept() {
                let _ = stream.set_nonblocking(false);
                let mut reader = BufReader::new(&stream);
                let mut req = String::new();
                let _ = reader.read_line(&mut req);
                loop {
                    let mut hdr = String::new();
                    if reader.read_line(&mut hdr).is_err() || hdr.trim().is_empty() {
                        break;
                    }
                }
                let path = req.split_whitespace().nth(1).unwrap_or("/");
                if let Some(body) = routes.get(path) {
                    let mut w = &stream;
                    let _ = write!(
                        w,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = w.write_all(body);
                    let _ = w.flush();
                }
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    });

    let cfg = UpdaterConfig {
        current_version: "0.2.0".into(),
        api_base_override: Some(format!("http://{addr}")),
        ..UpdaterConfig::default()
    };
    editor.updater = crate::updater::UpdaterController::new(cfg);
    editor.updater.start_check(UpdateChannel::Stable, &ctx);

    let deadline = Instant::now() + Duration::from_secs(3);
    while editor.updater.is_checking() {
        editor.poll_updater(&ctx, false);
        if !editor.updater.is_checking() {
            break;
        }
        assert!(Instant::now() < deadline, "updater check timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
    editor.poll_updater(&ctx, false);
    stop.store(true, Ordering::Release);
    let _ = TcpStream::connect(addr);
    let _ = server.join();

    assert!(matches!(
        editor.updater.state,
        UpdateState::UpdateAvailable { .. }
    ));
    let updater_events: Vec<_> = editor
        .output
        .events()
        .iter()
        .filter(|e| e.source == OutputSource::Updater)
        .collect();
    assert_eq!(updater_events.len(), 1);
    assert_eq!(updater_events[0].severity, OutputSeverity::Info);
    assert!(updater_events[0].message.contains("99.1.0"));
}

#[test]
fn t319_output_click_to_jump_switches_scenes_nested_symbols_and_soloed_object_scripts() {
    use crate::panels::actions_sidebar::ScriptLocation;

    let ctx = egui::Context::default();
    let mut project = Project::blank();
    let scene1_id = project.root.clone();

    // Button definition for attached Object scripts.
    let btn_sym_id = "sym-jump-btn".to_owned();
    let mut btn_tl = Project::blank().symbols.remove(0).timeline;
    btn_tl.frame_count = 4;
    btn_tl.layers[0].id = new_id("layer");
    project.symbols.push(Symbol {
        id: btn_sym_id.clone(),
        name: "JumpButton".into(),
        kind: SymbolKind::Button,
        folder: String::new(),
        class_name: None,
        linkage_id: None,
        timeline: btn_tl,
    });

    // Nested MovieClip symbol with: Layer 0 (Artwork), Layer 1 (Interactive Button at frame 2), Layer 2 (Actions, empty keyframes).
    let nested_id = "sym-jump-nested".to_owned();
    let nested_art_layer_id = new_id("layer");
    let nested_btn_layer_id = new_id("layer");
    let nested_act_layer_id = new_id("layer");
    let nested_btn_el_id = "el-nested-btn".to_owned();
    project.symbols.push(Symbol {
        id: nested_id.clone(),
        name: "NestedDialog".into(),
        kind: SymbolKind::MovieClip,
        folder: String::new(),
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 6,
            guides: vec![],
            scripts: vec![FrameScript {
                layer: None,
                frame: 1,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "// nested frame 2\ntrace('nested');\n".into(),
                },
            }],
            layers: vec![
                Layer {
                    id: nested_art_layer_id.clone(),
                    name: "Art".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: new_id("rect"),
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Rectangle {
                                width: 40.0,
                                height: 40.0,
                                fill: [120, 120, 200, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Default::default()
                    }],
                },
                Layer {
                    id: nested_btn_layer_id,
                    name: "Buttons".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 2,
                        elements: vec![Element {
                            id: nested_btn_el_id.clone(),
                            name: "nested_btn".into(),
                            transform: Transform::default(),
                            kind: ElementKind::Instance {
                                symbol_id: btn_sym_id.clone(),
                                playback: InstancePlayback::SingleFrame(0),
                            },
                            scripts: vec![ObjectScript {
                                event: ObjectEvent::Click,
                                script: Script {
                                    language: ScriptLanguage::ActionScript2,
                                    source: "trace('nested press');\ngotoAndStop(1);\n".into(),
                                },
                            }],
                            ..Element::default()
                        }],
                        ..Default::default()
                    }],
                },
                Layer {
                    id: nested_act_layer_id,
                    name: "Actions".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            ..Default::default()
                        },
                        Keyframe {
                            frame: 1,
                            ..Default::default()
                        },
                    ],
                },
            ],
        },
    });

    // Configure Scene 1: Layer 0 (Artwork), Layer 1 (Button + Nested clip), Layer 2 (Actions).
    let scene1_art_layer_id = project.symbol(&scene1_id).unwrap().timeline.layers[0]
        .id
        .clone();
    let scene1_btn_el_id = "el-scene1-btn".to_owned();
    let scene1_nested_inst_id = "el-scene1-nested".to_owned();
    {
        let s1 = project.symbol_mut(&scene1_id).unwrap();
        s1.timeline.frame_count = 6;
        s1.timeline.layers[0].keyframes[0].elements.push(Element {
            id: new_id("bg"),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 100.0,
                height: 80.0,
                fill: [40, 50, 60, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        });
        s1.timeline.layers.push(Layer {
            id: new_id("layer"),
            name: "Interactive".into(),
            visible: true,
            locked: false,
            properties: LayerProperties::default(),
            keyframes: vec![Keyframe {
                frame: 3,
                elements: vec![
                    Element {
                        id: scene1_btn_el_id.clone(),
                        name: "hero_btn".into(),
                        transform: Transform::default(),
                        kind: ElementKind::Instance {
                            symbol_id: btn_sym_id,
                            playback: InstancePlayback::SingleFrame(0),
                        },
                        scripts: vec![ObjectScript {
                            event: ObjectEvent::Load,
                            script: Script {
                                language: ScriptLanguage::ActionScript2,
                                source: "// hero release\ntrace('clicked');\n".into(),
                            },
                        }],
                        ..Element::default()
                    },
                    Element {
                        id: scene1_nested_inst_id.clone(),
                        name: "dialog_mc".into(),
                        transform: Transform::default(),
                        kind: ElementKind::Instance {
                            symbol_id: nested_id.clone(),
                            playback: InstancePlayback::Independent,
                        },
                        ..Element::default()
                    },
                ],
                ..Default::default()
            }],
        });
        s1.timeline.layers.push(Layer {
            id: new_id("layer"),
            name: "Actions".into(),
            visible: true,
            locked: false,
            properties: LayerProperties::default(),
            keyframes: vec![Keyframe {
                frame: 0,
                ..Default::default()
            }],
        });
        s1.timeline.scripts.push(FrameScript {
            layer: None,
            frame: 4,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "stop();\n".into(),
            },
        });
    }

    // Scene 2: Layer 0 (Bg with element), Layer 1 (Actions with frame script at frame 2).
    let scene2_id = "scene-jump-2".to_owned();
    let scene2_bg_layer_id = new_id("layer");
    let scene2_el_id = "el-scene2-prop".to_owned();
    project.symbols.push(Symbol {
        id: scene2_id.clone(),
        name: "Scene 2".into(),
        kind: SymbolKind::MovieClip,
        folder: String::new(),
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 5,
            guides: vec![],
            scripts: vec![FrameScript {
                layer: None,
                frame: 2,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "// scene 2 script\nplay();\n".into(),
                },
            }],
            layers: vec![
                Layer {
                    id: scene2_bg_layer_id.clone(),
                    name: "Bg".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: new_id("rect"),
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Rectangle {
                                width: 50.0,
                                height: 50.0,
                                fill: [80, 90, 100, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Default::default()
                    }],
                },
                Layer {
                    id: new_id("layer"),
                    name: "Actions".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            ..Default::default()
                        },
                        Keyframe {
                            frame: 2,
                            ..Default::default()
                        },
                    ],
                },
                Layer {
                    id: new_id("layer"),
                    name: "Props".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 1,
                        elements: vec![Element {
                            id: scene2_el_id.clone(),
                            name: "banner_box".into(),
                            transform: Transform::default(),
                            kind: ElementKind::Rectangle {
                                width: 30.0,
                                height: 20.0,
                                fill: [200, 80, 80, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Default::default()
                    }],
                },
            ],
        },
    });
    project.scenes = vec![scene1_id.clone(), scene2_id.clone()];
    project.sources.push(SourceUnit {
        id: "src-jump-ctrl".into(),
        path: "src/Controller.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: "package {\n  public class Controller {}\n}\n".into(),
    });
    project.format_version = project.format_version.max(6);
    project.validate().unwrap();

    let mut editor = Editor::from_project(&ctx, project);

    // (a) Enter Edit-in-Place on `dialog_mc` inside Scene 1, then jump via OutputLocation to Scene 2 Frame 2 script.
    editor.frame = 3;
    editor.select_single_id(scene1_nested_inst_id);
    assert!(editor.enter_selected_symbol_in_place());
    assert_eq!(editor.symbol_id(), nested_id);

    let scene2_frame_loc = editor.output_location_from_script(
        &ScriptLocation::Frame {
            symbol: scene2_id.clone(),
            layer: None,
            frame: 2,
        },
        Some(2),
        Some(1),
    );
    assert_eq!(
        scene2_frame_loc.layer_index,
        Some(1),
        "output_location_from_script must resolve dedicated Actions layer (index 1), not layer 0"
    );
    assert!(editor.navigate_to_output_location(&ctx, &scene2_frame_loc));
    assert_eq!(editor.current_scene(), scene2_id);
    assert_eq!(editor.symbol_id(), scene2_id);
    assert!(!editor.editing_in_place());
    assert_eq!(editor.frame, 2);
    assert_eq!(editor.layer, 1);
    assert!(editor.script_target == ScriptTarget::Frame);
    assert!(editor.workspace.is_panel_visible(PanelId::Actions));

    // Non-script element OutputLocation in Scene 2 while Bg layer (0) is soloed -> clears solo and selects `banner_box`.
    editor.solo_layers.insert(
        scene2_id.clone(),
        std::collections::BTreeSet::from([scene2_bg_layer_id]),
    );
    let el_loc = OutputLocation {
        symbol_id: Some(scene2_id.clone()),
        frame_index: Some(1),
        layer_index: Some(2),
        element_id: Some(scene2_el_id.clone()),
        ..OutputLocation::default()
    };
    assert!(editor.navigate_to_output_location(&ctx, &el_loc));
    assert_eq!(
        editor.selection.single().map(|t| t.element.as_str()),
        Some(scene2_el_id.as_str())
    );

    // (b) Jump to nested symbol `NestedDialog` Frame 1 script -> enters symbol editing and selects Actions layer (index 2).
    let nested_frame_loc = editor.output_location_from_script(
        &ScriptLocation::Frame {
            symbol: nested_id.clone(),
            layer: None,
            frame: 1,
        },
        Some(2),
        Some(1),
    );
    assert_eq!(nested_frame_loc.layer_index, Some(2));
    assert!(editor.navigate_to_output_location(&ctx, &nested_frame_loc));
    assert_eq!(editor.symbol_id(), nested_id);
    assert_eq!(editor.frame, 1);
    assert_eq!(editor.layer, 2);
    assert!(editor.script_target == ScriptTarget::Frame);

    // (c) Jump to attached Object script on `hero_btn` in Scene 1 AND `nested_btn` in `NestedDialog` while Layer 0 is soloed!
    editor.solo_layers.insert(
        scene1_id.clone(),
        std::collections::BTreeSet::from([scene1_art_layer_id]),
    );
    let s1_obj_loc = editor.output_location_from_script(
        &ScriptLocation::Object {
            symbol: scene1_id.clone(),
            frame: 3,
            element: scene1_btn_el_id.clone(),
            event: ObjectEvent::Load,
        },
        Some(2),
        Some(1),
    );
    assert!(editor.navigate_to_output_location(&ctx, &s1_obj_loc));
    assert_eq!(editor.symbol_id(), scene1_id);
    assert_eq!(editor.frame, 3);
    assert_eq!(editor.layer, 1);
    assert_eq!(
        editor.selection.single().map(|t| t.element.as_str()),
        Some(scene1_btn_el_id.as_str())
    );
    assert!(
        editor.script_target == ScriptTarget::Object,
        "Object script jump must NOT be clobbered to Frame when another layer was soloed"
    );
    assert_eq!(editor.script_event, ObjectEvent::Load);

    editor.solo_layers.insert(
        nested_id.clone(),
        std::collections::BTreeSet::from([nested_art_layer_id]),
    );
    let nested_obj_loc = editor.output_location_from_script(
        &ScriptLocation::Object {
            symbol: nested_id.clone(),
            frame: 2,
            element: nested_btn_el_id.clone(),
            event: ObjectEvent::Click,
        },
        Some(2),
        Some(1),
    );
    assert!(editor.navigate_to_output_location(&ctx, &nested_obj_loc));
    assert_eq!(editor.symbol_id(), nested_id);
    assert_eq!(editor.frame, 2);
    assert_eq!(editor.layer, 1);
    assert_eq!(
        editor.selection.single().map(|t| t.element.as_str()),
        Some(nested_btn_el_id.as_str())
    );
    assert!(editor.script_target == ScriptTarget::Object);
    assert_eq!(editor.script_event, ObjectEvent::Click);

    // (d) Jump to external project SourceUnit `src/Controller.as`.
    let src_loc = editor.output_location_from_script(
        &ScriptLocation::Source("src-jump-ctrl".into()),
        Some(2),
        Some(3),
    );
    assert!(editor.navigate_to_output_location(&ctx, &src_loc));
    assert!(editor.script_target == ScriptTarget::Source);
    assert_eq!(editor.selected_source.as_deref(), Some("src-jump-ctrl"));
}

#[test]
fn t368_output_filter_preserves_complete_multiline_structured_trace_blocks() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::blank());

    let structured_msg = [
        "state = {",
        "  player: \"hero\",",
        "  matrix: [",
        "    [0, 1, 0],",
        "    [1, 1, 1],",
        "    [0, 0, 0]",
        "  ]",
        "}",
    ]
    .join("\n");
    editor.push_output(structured_msg.clone());
    editor.push_output("unrelated frame tick 42");

    set_output_filter_text(&editor, &ctx, "player");
    let filtered_out = run_output_frame(&mut editor, &ctx, vec![]);
    let copy_pos =
        find_accesskit_button_center(&filtered_out, "Copy output").expect("Copy output enabled");
    let copy_out = run_output_frame(&mut editor, &ctx, click_events(copy_pos));
    let copied = copy_out
        .platform_output
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .expect("Copy output must emit CopyText command");

    assert_eq!(
        copied, structured_msg,
        "filtering a structured multi-line trace event by one key must keep all nested matrix/closing lines intact"
    );
}
