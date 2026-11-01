//! Synthetic command bytes built here; no imported document content.
use super::*;

#[derive(Default)]
struct B(Vec<u8>);
impl B {
    fn u8(mut self, v: u8) -> Self {
        self.0.push(v);
        self
    }
    fn u16(mut self, v: u16) -> Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn u32(mut self, v: u32) -> Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn s(mut self, v: &str) -> Self {
        self.0.push(v.len() as u8);
        self.0.extend(v.as_bytes());
        self
    }
}

#[derive(Clone, Copy)]
enum V {
    S(&'static str),
    N(u32),
}

/// Historical command in archive field order; unspecified fields are empty/zero.
fn historical(revision: u8, kind: u8, set: &[(&str, V)]) -> Vec<u8> {
    let s = |name: &str| {
        set.iter()
            .find(|(n, _)| *n == name)
            .map_or("", |(_, v)| match v {
                V::S(s) => s,
                V::N(_) => panic!("{name}"),
            })
    };
    let n = |name: &str, default: u32| {
        set.iter()
            .find(|(n, _)| *n == name)
            .map_or(default, |(_, v)| match v {
                V::N(n) => *n,
                V::S(_) => panic!("{name}"),
            })
    };
    let mut b = B::default().u8(revision).u8(kind);
    for name in ["url", "window", "page"] {
        b = b.s(s(name));
    }
    b = b.u16(n("frame_number", 1) as u16);
    if revision >= 1 {
        b = b.u16(n("wait", NO_WAIT as u16 as u32) as u16);
    }
    if revision >= 2 {
        b = b.s(s("label"));
    }
    if revision >= 3 {
        b = b
            .s(s("arg0"))
            .s(s("arg1"))
            .s(s("target"))
            .u32(n("level", 1));
    }
    if revision >= 4 {
        for name in [
            "release",
            "press",
            "release_outside",
            "roll_over",
            "roll_out",
            "drag_over",
            "drag_out",
        ] {
            b = b.u32(n(name, 0));
        }
    }
    if revision >= 5 {
        b = b.u32(n("property", 0));
        for name in ["r1", "r2", "depth", "variable_name"] {
            b = b.s(s(name));
        }
    }
    if revision >= 6 {
        b = b.s(s("method")).u32(n("load_variables", 0));
    }
    if revision >= 7 {
        b = b.s(s("arg2")).s(s("arg3"));
        for name in [
            "variable_name_is_expression",
            "url_is_expression",
            "arg0_is_expression",
            "arg1_is_expression",
            "arg2_is_expression",
            "arg3_is_expression",
            "window_is_expression",
            "target_is_expression",
            "depth_is_expression",
            "start",
            "constrain_to_rectangle",
            "lock_mouse_to_center",
        ] {
            b = b.u32(n(name, 0));
        }
    }
    if revision >= 8 {
        b = b.u32(n("on_key_press", 0)).u32(n("key", 0));
    }
    if revision >= 9 {
        b = b.u32(0);
    }
    b.0
}

fn metadata(b: B, text: &str) -> B {
    b.s("").s(text).s("\r\n").u32(0).u32(0)
}

struct Decoded {
    actions: TimelineActions,
    result: std::result::Result<(), String>,
    end: usize,
    data: Vec<u8>,
}

fn decode(
    data: &[u8],
    form: TimelineActionForm,
    list_revision: Option<u32>,
    count: u32,
) -> Decoded {
    let mut shapes = 250_000;
    let mut items = 100_000;
    let mut reader = Reader {
        data,
        pos: 0,
        refs: vec![TimelineReference::Null],
        objects: vec![],
        pointers: vec![],
        counts: RecordCounts::default(),
        geometry: geometry::GeometryCounts::default(),
        retained: vec![],
        retain_geometry: true,
        retain_timeline: true,
        retained_items_remaining: &mut shapes,
        timeline_items_remaining: &mut items,
        root_class: None,
        root_schema: None,
        root: None,
        stream_kind: None,
    };
    let mut actions = TimelineActions {
        encoded: TimelineByteSpan { start: 0, end: 0 },
        complete: false,
        presence: TimelineActionPresence::Unknown,
        revision: None,
        mode: None,
        identifier: None,
        breakpoint_count: None,
        breakpoints: None,
        collapse_word_count: None,
        collapse_words: None,
        source: None,
        compiled_revision: None,
        command_count: None,
        decoded: None,
    };
    let result = reader
        .decode_actions(&mut actions, form, list_revision, count)
        .map_err(|e| e.code);
    Decoded {
        actions,
        result,
        end: reader.pos,
        data: data.to_vec(),
    }
}

fn list(commands: &[Vec<u8>]) -> Decoded {
    let data: Vec<u8> = commands.concat();
    decode(
        &data,
        TimelineActionForm::HistoricalList,
        None,
        commands.len() as u32,
    )
}

fn source(d: &Decoded) -> &str {
    &d.actions.decoded.as_ref().unwrap().source
}

#[test]
fn historical_list_translates_button_blocks_and_flash4_expressions() {
    let d = list(&[
        historical(
            8,
            20,
            &[
                ("release", V::N(1)),
                ("on_key_press", V::N(1)),
                ("key", V::N(13)),
            ],
        ),
        historical(
            8,
            10,
            &[("page", V::S("Scene 2")), ("label", V::S("intro"))],
        ),
        historical(8, 14, &[("target", V::S("/menu"))]),
        historical(8, 2, &[]),
        historical(8, 15, &[]),
        historical(8, 22, &[("arg0", V::S("/:count > 3"))]),
        historical(
            8,
            29,
            &[
                ("variable_name", V::S("count")),
                ("arg0", V::S("count + 1")),
                ("arg0_is_expression", V::N(1)),
            ],
        ),
        historical(8, 32, &[("arg0", V::S("done eq \"yes\""))]),
        historical(
            8,
            29,
            &[("variable_name", V::S("/:total")), ("arg0", V::S("a\"b"))],
        ),
        historical(8, 23, &[]),
        historical(
            8,
            30,
            &[
                ("target", V::S("/ball")),
                ("property", V::N(6)),
                ("arg0", V::S("50")),
            ],
        ),
        historical(8, 24, &[]),
        historical(8, 17, &[("url", V::S("next.swf")), ("level", V::N(2))]),
        historical(
            8,
            9,
            &[
                ("url", V::S("http://example.test/")),
                ("window", V::S("_blank")),
                ("method", V::S("POST")),
            ],
        ),
        historical(8, 21, &[]),
    ]);
    assert_eq!(d.result, Ok(()));
    assert_eq!(d.end, d.data.len());
    let decoded = d.actions.decoded.as_ref().unwrap();
    assert!(!decoded.needs_review);
    assert_eq!(decoded.commands.len(), 15);
    assert!(decoded.commands.iter().all(|c| c.complete));
    assert_eq!(
        source(&d),
        "on (release, keyPress \"<Enter>\") {
    gotoAndPlay(\"Scene 2\", \"intro\");
    with (eval(\"/menu\")) { // tellTarget (\"/menu\")
        stop();
    }
    if (/:count > 3) {
        count = count + 1;
    } else if (done eq \"yes\") {
        set(\"/:total\", \"a\\\"b\");
    } else {
        setProperty(\"/ball\", _alpha, \"50\");
    }
    loadMovieNum(\"next.swf\", 2);
    getURL(\"http://example.test/\", \"_blank\", \"POST\");
}
"
    );
}

#[test]
fn early_revisions_and_numeric_frames_use_literal_operands() {
    let d = list(&[
        historical(4, 18, &[("frame_number", V::N(12))]),
        historical(4, 8, &[("frame_number", V::N(3))]),
        historical(4, 19, &[]),
        historical(
            4,
            16,
            &[("arg0", V::S("fullscreen")), ("arg1", V::S("true"))],
        ),
        historical(0, 13, &[]),
        historical(4, 37, &[("arg0", V::S("two\rlines"))]),
    ]);
    assert_eq!(d.result, Ok(()));
    assert_eq!(
        source(&d),
        "if (_framesloaded >= 12) { // ifFrameLoaded (12)
    gotoAndStop(3);
}
fscommand(\"fullscreen\", \"true\");
stopAllSounds();
// two
// lines
"
    );
}

#[test]
fn single_historical_command_carries_events_and_wait_frames() {
    let mut wait = historical(
        9,
        10,
        &[
            ("frame_number", V::N(4)),
            ("wait", V::N(2)),
            ("press", V::N(1)),
        ],
    );
    let d = decode(&wait, TimelineActionForm::HistoricalCommand, None, 1);
    assert_eq!(d.result, Ok(()));
    assert_eq!(d.actions.presence, TimelineActionPresence::Unknown);
    assert_eq!(
        source(&d),
        "on (press) {
    if (_framesloaded >= 6) { // ifFrameLoaded (6)
        gotoAndPlay(4);
    }
}
"
    );
    wait[1] = 0;
    let d = decode(&wait, TimelineActionForm::HistoricalCommand, None, 1);
    assert_eq!(d.actions.presence, TimelineActionPresence::Empty);
    assert_eq!(source(&d), "");
}

#[test]
fn unassigned_commands_and_broken_blocks_stay_as_marked_comments() {
    let d = list(&[
        historical(4, 27, &[("arg0", V::S("x"))]),
        historical(4, 1, &[]),
        historical(5, 30, &[("target", V::S("/a")), ("property", V::N(99))]),
        historical(4, 24, &[]),
    ]);
    assert_eq!(d.result, Ok(()));
    let decoded = d.actions.decoded.as_ref().unwrap();
    assert!(decoded.needs_review);
    assert_eq!(decoded.untranslated, vec![0, 2, 3]);
    let text = source(&d);
    assert!(text.starts_with(
        "// NeoFlash: untranslated FLA action @0x0 (historical command 27 r4): unassigned historical command kind 27; frame_number=1 wait_for_extra_frames=-16000 arg0=\"x\" level=1\nplay();\n"
    ));
    assert!(text.contains("unassigned property selector"));
    assert!(text.contains("block delimiter without its opener"));
    assert_eq!(text.lines().count(), 4);
}

#[test]
fn untranslated_opener_comments_out_its_body_instead_of_running_it() {
    let d = list(&[
        historical(7, 22, &[]),
        historical(7, 25, &[("arg0", V::S("i < 3"))]),
        historical(7, 2, &[]),
        historical(7, 26, &[]),
        historical(7, 23, &[]),
        historical(7, 2, &[]),
        historical(7, 24, &[]),
        historical(7, 1, &[]),
    ]);
    let decoded = d.actions.decoded.as_ref().unwrap();
    assert_eq!(decoded.untranslated, (0..7).collect::<Vec<_>>());
    assert!(
        source(&d)
            .lines()
            .take(7)
            .all(|l| l.starts_with("// NeoFlash: untranslated"))
    );
    assert!(source(&d).ends_with("\nplay();\n"));

    let d = list(&[historical(4, 14, &[]), historical(4, 1, &[])]);
    let decoded = d.actions.decoded.as_ref().unwrap();
    assert!(decoded.needs_review);
    assert_eq!(
        source(&d),
        "with (this) { // tellTarget (\"\")
    play();
} // NeoFlash: block left open in the source was closed by the importer
"
    );
}

#[test]
fn ansi_text_is_not_decoded_with_a_guessed_code_page() {
    let mut bytes = historical(4, 38, &[("arg0", V::S("caf?"))]);
    let at = bytes.iter().position(|b| *b == b'?').unwrap();
    bytes[at] = 0xe9;
    let d = list(&[bytes]);
    assert!(d.actions.decoded.as_ref().unwrap().needs_review);
    assert!(source(&d).contains("needs an explicit source code page"));
    assert!(source(&d).contains("arg0=\"caf\\xe9\""));
}

#[test]
fn historical_counts_revisions_and_truncation_stop_exactly() {
    let one = historical(4, 2, &[]);
    let d = decode(&one, TimelineActionForm::HistoricalList, None, 7);
    assert_eq!(
        d.result.as_ref().err().map(String::as_str),
        Some("fla_record_limit")
    );
    assert_eq!(d.end, 0);
    let mut bad = one.clone();
    bad[0] = 11;
    let d = decode(&bad, TimelineActionForm::HistoricalList, None, 1);
    assert_eq!(
        d.result.as_ref().err().map(String::as_str),
        Some("fla_record_revision_unsupported")
    );
    let d = decode(
        &[&one[..], &one[..one.len() - 3]].concat(),
        TimelineActionForm::HistoricalList,
        None,
        2,
    );
    assert_eq!(
        d.result.as_ref().err().map(String::as_str),
        Some("fla_truncated_record")
    );
    let decoded = d.actions.decoded.as_ref().unwrap();
    assert_eq!(decoded.commands.len(), 2);
    assert!(decoded.commands[0].complete && !decoded.commands[1].complete);
    assert_eq!(decoded.commands[0].encoded.end, one.len());
    assert!(decoded.source.is_empty());
}

fn compiled(list_revision: u32, commands: &[Vec<u8>]) -> Decoded {
    decode(
        &commands.concat(),
        TimelineActionForm::Compiled,
        Some(list_revision),
        commands.len() as u32,
    )
}

#[test]
fn compiled_revision_zero_omits_simple_bodies() {
    let d = compiled(
        0,
        &[
            B::default().u32(11).u32(0).u32(2).u32(0).0,
            B::default().u32(0).u32(0).u32(1).s("").s("P").u32(1).0,
            B::default().u32(1).0,
            B::default().u32(12).0,
        ],
    );
    assert_eq!(d.result, Ok(()));
    assert_eq!(d.end, d.data.len());
    let decoded = d.actions.decoded.as_ref().unwrap();
    assert!(!decoded.commands[2].body_present);
    assert_eq!(
        source(&d),
        "on (release) {\n    gotoAndPlay(\"P\");\n    play();\n}\n"
    );
}

#[test]
fn compiled_revision_two_reads_metadata_and_structured_statements() {
    let simple = |kind: u32| metadata(B::default().u32(kind).u32(2), "").0;
    let d = compiled(
        2,
        &[
            metadata(
                B::default().u32(13).u32(2).u32(0x3),
                "onClipEvent (load, enterFrame) {",
            )
            .0,
            metadata(
                B::default().u32(37).u32(4).s("step").s("n").u32(0).u8(0),
                "",
            )
            .0,
            metadata(B::default().u32(15).u32(2).s("n > 0"), "").0,
            metadata(B::default().u32(40).u32(2).s("n - 1"), "").0,
            simple(16),
            metadata(B::default().u32(28).u32(2).s(" done"), "")
                .u32(0)
                .0,
            simple(18),
            simple(38),
            metadata(B::default().u32(20).u32(2).s("step(i) > 0").u8(1), "").0,
            simple(21),
            B::default()
                .u32(24)
                .u32(2)
                .s("i")
                .u32(0)
                .s("i + 1")
                .u32(1)
                .s("")
                .s("i = i + 1;")
                .s("")
                .u32(0)
                .u16(34)
                .u32(0)
                .0,
            metadata(B::default().u32(8).u32(2).s("_parent.clip").u32(1), "").0,
            metadata(B::default().u32(0).u32(2).u32(0).s("").s("5").u32(0), "").0,
            simple(9),
            metadata(
                B::default()
                    .u32(59)
                    .u32(3)
                    .s(".play()")
                    .s("clip")
                    .s("")
                    .u32(1)
                    .u32(0),
                "",
            )
            .0,
            metadata(
                B::default()
                    .u32(5)
                    .u32(2)
                    .s("page.html")
                    .s("_top")
                    .u32(0)
                    .u32(0)
                    .u32(1),
                "",
            )
            .0,
            metadata(B::default().u32(41).u32(3).s("x++"), "").0,
            simple(14),
        ],
    );
    assert_eq!(d.result, Ok(()), "{}", source(&d));
    assert_eq!(d.end, d.data.len());
    assert!(
        !d.actions.decoded.as_ref().unwrap().needs_review,
        "{}",
        source(&d)
    );
    assert_eq!(
        source(&d),
        "onClipEvent (load, enterFrame) {
    function step(n) {
        if (n > 0) {
            return n - 1;
        } else {
            //  done
        }
    }
    while (step(i) > 0);
    i = i + 1;
    with (eval(_parent.clip)) { // tellTarget (_parent.clip)
        gotoAndStop(5);
    }
    clip.play();
    getURL(\"page.html\", \"_top\", \"GET\");
    x++;
}
"
    );
}

#[test]
fn compiled_unknown_types_and_revisions_stop_without_resynchronizing() {
    let play = B::default().u32(1).0;
    let d = compiled(0, &[play.clone(), B::default().u32(35).u32(0).0]);
    assert_eq!(
        d.result.as_ref().err().map(String::as_str),
        Some("fla_action_command_unsupported")
    );
    assert_eq!(d.end, 8);
    let decoded = d.actions.decoded.as_ref().unwrap();
    assert_eq!(decoded.commands.len(), 2);
    assert!(decoded.commands[0].complete && !decoded.commands[1].complete);
    let d = compiled(0, &[B::default().u32(27).u32(3).s("x").u32(0).0]);
    assert_eq!(
        d.result.as_ref().err().map(String::as_str),
        Some("fla_action_command_revision_unsupported")
    );
    let d = compiled(0, &[B::default().u32(86).0]);
    assert_eq!(
        d.result.as_ref().err().map(String::as_str),
        Some("fla_action_command_unsupported")
    );
    let d = decode(&play, TimelineActionForm::Compiled, Some(0), 2);
    assert_eq!(
        d.result.as_ref().err().map(String::as_str),
        Some("fla_record_limit")
    );
}

#[test]
fn compiled_as2_declarations_are_kept_as_comments() {
    let d = compiled(
        3,
        &[
            B::default()
                .u32(71)
                .u32(1)
                .s("Ball")
                .s("")
                .s("")
                .u32(0)
                .u32(0)
                .0,
            B::default().u32(39).u32(1).s("speed:Number").u32(1).0,
            B::default().u32(72).u32(0).0,
            B::default().u32(2).u32(0).0,
        ],
    );
    assert_eq!(d.result, Ok(()));
    let decoded = d.actions.decoded.as_ref().unwrap();
    assert_eq!(decoded.untranslated, vec![0, 1, 2]);
    assert!(source(&d).ends_with("\nstop();\n"));
}
