use super::*;

#[test]
fn insert_adds_missing_as3_import_once() {
    let command = CodeCommand::Insert {
        text: "Point".into(),
        caret: 5,
        import: Some("flash.geom.Point".into()),
    };
    let (out, sel) = apply(&command, "var p:", (6, 6)).unwrap();
    assert_eq!(out, "import flash.geom.Point;\nvar p:Point");
    assert_eq!(sel, (36, 36));
    let (again, _) = apply(&command, &out, (36, 36)).unwrap();
    assert_eq!(again.matches("import").count(), 1);
}

#[test]
fn line_comments_round_trip() {
    let text = "a();\n  b();\n  c();\nd();";
    let (commented, sel) = line_comment(text, (6, 14)).unwrap();
    assert_eq!(commented, "a();\n  // b();\n  // c();\nd();");
    let (plain, _) = uncomment(&commented, sel).unwrap();
    assert_eq!(plain, text);
    assert!(uncomment(text, (0, 0)).is_none());
}

#[test]
fn block_comment_wraps_selection_or_caret_line() {
    let (out, sel) = block_comment("x = 1;\ny = 2;", (0, 6)).unwrap();
    assert_eq!(out, "/* x = 1; */\ny = 2;");
    assert_eq!(sel, (0, 12));
    let (back, _) = uncomment(&out, sel).unwrap();
    assert_eq!(back, "x = 1;\ny = 2;");
    let (line, _) = block_comment("  stop();", (3, 3)).unwrap();
    assert_eq!(line, "  /* stop(); */");
}

#[test]
fn auto_format_reindents_without_moving_tokens() {
    let source = "function f(n:int):void {\nif (n > 0) {\ntrace(\"{ not a brace\");\n}   \nswitch (n) {\ncase 1:\nstop();\nbreak;\ndefault:\nplay();\n}\n}";
    let formatted = auto_format(source).unwrap();
    assert_eq!(
        formatted,
        "function f(n:int):void {\n    if (n > 0) {\n        trace(\"{ not a brace\");\n    }\n    switch (n) {\n        case 1:\n            stop();\n            break;\n        default:\n            play();\n    }\n}"
    );
    assert!(auto_format(&formatted).is_none(), "idempotent");
    let strip = |s: &str| s.split_whitespace().collect::<String>();
    assert_eq!(strip(&formatted), strip(source));
}

#[test]
fn auto_format_keeps_comments_and_xml_lines() {
    let source = "/* keep\n      this */\nvar x:XML = <a>\n      <b/>\n</a>;\nif (x) {\nstop();\n}";
    let formatted = auto_format(source).unwrap();
    assert!(formatted.contains("      this */"));
    assert!(formatted.contains("\n      <b/>\n"));
    assert!(formatted.ends_with("if (x) {\n    stop();\n}"));
}

#[test]
fn auto_format_preserves_multiline_token_payloads_and_exponent_operators() {
    let source = concat!(
        "/* keep \t\r\n    \r\nline \t\n*/\n",
        "var x:XML=<a>  \r\n    \r\nvalue  \n</a>;\n",
        "const template=`keep \t\r\n    \r\nline \t\nend`;\n",
        "/* first\n*/ const second = `keep  \r\nend`;\n",
        "const continued='a\\\r\n b';\n",
        "let power=2**3;\npower**=2;",
    );
    let expected = concat!(
        "/* keep \t\r\n    \r\nline \t\n*/\n",
        "var x:XML = <a>  \r\n    \r\nvalue  \n</a>;\n",
        "const template = `keep \t\r\n    \r\nline \t\nend`;\n",
        "/* first\n*/ const second = `keep  \r\nend`;\n",
        "const continued = 'a\\\r\n b';\n",
        "let power = 2 ** 3;\npower **= 2;",
    );
    assert_eq!(auto_format(source).as_deref(), Some(expected));
    assert_eq!(auto_format(expected), None);
}

#[test]
fn auto_format_preserves_incomplete_quotes_with_a_trailing_escape() {
    for quote in ['"', '\'', '`'] {
        let source = format!("var text={quote}unfinished\\");
        let expected = format!("var text = {quote}unfinished\\");
        assert_eq!(auto_format(&source).as_deref(), Some(expected.as_str()));
        assert_eq!(auto_format(&expected), None);
    }
}

#[test]
fn tab_size_controls_auto_format_line_comment_and_tab_indent_outdent() {
    let src = "function f() {\nif (ok) {\nstop();\n}\n}";
    assert_eq!(
        auto_format_with_tab_size(src, 2).unwrap(),
        "function f() {\n  if (ok) {\n    stop();\n  }\n}"
    );
    assert_eq!(
        auto_format_with_tab_size(src, 8).unwrap(),
        "function f() {\n        if (ok) {\n                stop();\n        }\n}"
    );

    // Single caret Tab / Shift+Tab with 2 and 4 spaces.
    let (indented, sel) = indent_tab("stop();", (0, 0), 2);
    assert_eq!(indented, "  stop();");
    assert_eq!(sel, (2, 2));
    let (outdented, sel) = outdent_tab(&indented, (9, 9), 2).unwrap();
    assert_eq!(outdented, "stop();");
    assert_eq!(sel, (7, 7));
    assert!(outdent_tab("stop();", (0, 0), 4).is_none());

    // Multi-line block indent & outdent preserves empty lines and selection span.
    let block = "a();\n\nb();";
    let (ind2, sel2) = indent_tab(block, (0, block.chars().count()), 4);
    assert_eq!(ind2, "    a();\n\n    b();");
    let (back, _) = outdent_tab(&ind2, sel2, 4).unwrap();
    assert_eq!(back, block);

    // Mixed leading tab and spaces in line comment uses visual tab_size columns.
    let mixed = "\ta();\n    b();";
    let (commented, _) = line_comment_with_tab_size(mixed, (0, mixed.chars().count()), 4).unwrap();
    assert_eq!(commented, "\t// a();\n    // b();");
}

#[test]
fn auto_close_and_backspace_pair_handle_brackets_quotes_escapes_and_skip_over() {
    // Opening brackets pair when not before identifier char, then closing bracket skips over.
    let (s, sel) = auto_close_char("trace", 5, '(').unwrap();
    assert_eq!(s, "trace()");
    assert_eq!(sel, (6, 6));
    let (s, sel) = auto_close_char(&s, sel.0, ')').unwrap();
    assert_eq!(s, "trace()");
    assert_eq!(sel, (7, 7));

    // Do not auto-close directly before an identifier character.
    assert!(auto_close_char("foo", 0, '(').is_none());
    assert!(auto_close_char("foo", 0, '"').is_none());

    // Quotes pair, skip over closing quote, ignore escaped quotes, and close unclosed strings.
    let (q, qsel) = auto_close_char("x = ", 4, '"').unwrap();
    assert_eq!(q, "x = \"\"");
    assert_eq!(qsel, (5, 5));
    let (q2, qsel2) = auto_close_char(&q, qsel.0, '"').unwrap();
    assert_eq!(q2, "x = \"\"");
    assert_eq!(qsel2, (6, 6));
    assert!(auto_close_char("x = \"hello", 10, '"').is_none());
    assert!(auto_close_char("x = \"hello\\", 11, '"').is_none());

    // Backspace between an empty pair deletes both delimiters; non-empty or escaped pairs do not.
    for (pair, caret) in [
        ("()", 1),
        ("[]", 1),
        ("{}", 1),
        ("\"\"", 1),
        ("''", 1),
        ("``", 1),
    ] {
        let (deleted, dsel) = auto_delete_pair(pair, caret).unwrap();
        assert_eq!(deleted, "");
        assert_eq!(dsel, (0, 0));
    }
    assert!(auto_delete_pair("(a)", 1).is_none());
    assert!(auto_delete_pair("\\\"\"", 2).is_none());
}

#[test]
fn infer_local_variable_types_resolves_annotations_constructors_suffixes_and_comment_hints() {
    let ts_src = r#"
// MovieClip legacyClip;
class Player extends Actor {
    speed: number = 10;
    move(dx: number, dy: string): void {
        let count: number = 0;
        const hero = new HeroSprite();
        const items: string[] = [];
        var label_txt;
    }
}
"#;
    let types = infer_local_variable_types(ts_src, neoflash_core::ScriptLanguage::TypeScript);
    assert_eq!(
        types.get("legacyClip").map(String::as_str),
        Some("MovieClip")
    );
    assert_eq!(types.get("this").map(String::as_str), Some("Player"));
    assert_eq!(types.get("super").map(String::as_str), Some("Actor"));
    assert_eq!(types.get("count").map(String::as_str), Some("number"));
    assert_eq!(types.get("hero").map(String::as_str), Some("HeroSprite"));
    assert_eq!(types.get("items").map(String::as_str), Some("Array"));
    assert_eq!(
        types.get("label_txt").map(String::as_str),
        Some("TextField")
    );
}

#[test]
fn complete_at_caret_resolves_current_and_imported_project_class_members() {
    let project_sources = vec![
        neoflash_core::SourceUnit {
            id: "actor".into(),
            path: "game/Actor.as".into(),
            language: neoflash_core::ScriptLanguage::ActionScript3,
            source: r#"package game {
    public class Actor {
        public var health: Number = 100;
        public function takeDamage(amount: Number): void {}
    }
}"#
            .into(),
        },
        neoflash_core::SourceUnit {
            id: "hero".into(),
            path: "game/Hero.ts".into(),
            language: neoflash_core::ScriptLanguage::TypeScript,
            source: r#"export class Hero extends Actor {
    stamina: number = 50;
    jump(force: number): void {}
}"#
            .into(),
        },
        neoflash_core::SourceUnit {
            id: "math".into(),
            path: "utils/math.ts".into(),
            language: neoflash_core::ScriptLanguage::TypeScript,
            source: r#"export const GRAVITY: number = 9.8;
export function clamp(val: number, min: number, max: number): number { return val; }"#
                .into(),
        },
    ];

    // Member completion on `player = new Hero()` resolves both `Hero` own members and inherited `Actor` members!
    let handler_src =
        "import * as math from \"./utils/math\";\nconst player = new Hero();\nplayer.";
    let completions = complete_at_caret(
        neoflash_core::ScriptLanguage::TypeScript,
        handler_src,
        handler_src.chars().count(),
        &project_sources,
    );
    let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
    assert!(
        labels.contains(&"stamina"),
        "expected Hero.stamina in {labels:?}"
    );
    assert!(labels.contains(&"jump"), "expected Hero.jump in {labels:?}");
    assert!(
        labels.contains(&"health"),
        "expected inherited Actor.health in {labels:?}"
    );
    assert!(
        labels.contains(&"takeDamage"),
        "expected inherited Actor.takeDamage in {labels:?}"
    );

    // Namespace module completion (`math.`) resolves exported members of `utils/math.ts`
    let mod_src = "import * as math from \"./utils/math\";\nmath.";
    let mod_completions = complete_at_caret(
        neoflash_core::ScriptLanguage::TypeScript,
        mod_src,
        mod_src.chars().count(),
        &project_sources,
    );
    let mod_labels: Vec<&str> = mod_completions.iter().map(|c| c.label.as_str()).collect();
    assert!(
        mod_labels.contains(&"GRAVITY"),
        "expected GRAVITY in {mod_labels:?}"
    );
    assert!(
        mod_labels.contains(&"clamp"),
        "expected clamp in {mod_labels:?}"
    );

    // Unqualified completion in AS3 offers unimported project class `Actor` with `import: Some("game.Actor")`
    let as3_src = "var a: Act";
    let as3_completions = complete_at_caret(
        neoflash_core::ScriptLanguage::ActionScript3,
        as3_src,
        as3_src.chars().count(),
        &project_sources,
    );
    let actor_item = as3_completions
        .iter()
        .find(|c| c.label == "Actor")
        .expect("Actor class completion");
    assert_eq!(actor_item.import.as_deref(), Some("game.Actor"));
}

#[test]
fn quick_fixes_apply_through_code_command_for_warnings_and_diagnostics() {
    // 1. Misspelled identifier quick fix on TypeScript diagnostic
    let bad_ts = "let score: number = 10;\nscr = 20;";
    let fixes = diagnostics_and_quick_fixes(neoflash_core::ScriptLanguage::TypeScript, bad_ts);
    let (_, typo_fixes) = fixes
        .iter()
        .find(|(msg, f)| msg.contains("Did you mean 'score'") && !f.is_empty())
        .expect("misspelled identifier quick fix");
    let (fixed_ts, _) = apply(
        &CodeCommand::ApplyQuickFix(typo_fixes[0].clone()),
        bad_ts,
        (0, 0),
    )
    .unwrap();
    assert_eq!(fixed_ts, "let score: number = 10;\nscore = 20;");

    // 2. Unused variable & deprecated AS2 operator quick fixes
    let as2_src =
        "function example():Void { var unusedCount:Number = 1; }\nif (a and b) {\n    trace(a);\n}";
    let as2_fixes =
        diagnostics_and_quick_fixes(neoflash_core::ScriptLanguage::ActionScript2, as2_src);
    let (_, unused_f) = as2_fixes
        .iter()
        .find(|(msg, _)| msg.contains("Variable 'unusedCount'"))
        .expect("unused variable warning");
    let (prefixed, _) = apply(
        &CodeCommand::ApplyQuickFix(unused_f[0].clone()),
        as2_src,
        (0, 0),
    )
    .unwrap();
    assert!(prefixed.contains("var _unusedCount:Number = 1;"));

    let (_, and_f) = as2_fixes
        .iter()
        .find(|(msg, _)| msg.contains("'and'"))
        .expect("deprecated and operator warning");
    let (modern_and, _) = apply(
        &CodeCommand::ApplyQuickFix(and_f[0].clone()),
        as2_src,
        (0, 0),
    )
    .unwrap();
    assert!(modern_and.contains("if (a && b)"));
}

#[test]
fn insert_places_as3_imports_inside_package_block_and_deduplicates() {
    let pkg_src = "package com.demo {\n    public class Main {\n        public function Main() {\n            \n        }\n    }\n}\n";
    let caret = pkg_src.find("            \n").unwrap() + 12;
    let (after_first, (new_pos, _)) = apply(
        &CodeCommand::Insert {
            text: "var mc:MovieClip = new MovieClip();".into(),
            caret: "var mc:MovieClip = new MovieClip();".chars().count(),
            import: Some("flash.display.MovieClip".into()),
        },
        pkg_src,
        (caret, caret),
    )
    .unwrap();
    assert!(
        after_first.starts_with(
            "package com.demo {\n    import flash.display.MovieClip;\n    public class Main {"
        ),
        "unexpected output:\n{after_first}"
    );
    assert!(!after_first.starts_with("import "));

    let (after_second, _) = apply(
        &CodeCommand::Insert {
            text: "\n            var ev:Event;".into(),
            caret: "\n            var ev:Event;".chars().count(),
            import: Some("flash.events.Event".into()),
        },
        &after_first,
        (new_pos, new_pos),
    )
    .unwrap();
    assert!(
            after_second.starts_with(
                "package com.demo {\n    import flash.display.MovieClip;\n    import flash.events.Event;\n    public class Main {"
            ),
            "unexpected output:\n{after_second}"
        );

    // Re-inserting an already-imported type does not duplicate the import line.
    let (after_dup, _) = apply(
        &CodeCommand::Insert {
            text: "".into(),
            caret: 0,
            import: Some("flash.display.MovieClip".into()),
        },
        &after_second,
        (new_pos, new_pos),
    )
    .unwrap();
    assert_eq!(
        after_dup.matches("import flash.display.MovieClip;").count(),
        1
    );
}

#[test]
fn all_50_classic_flash_esc_chords_match_reference_table_indentation_and_placeholders() {
    let expected_table: [(&str, &str, Option<&str>); 50] = [
        ("br", "break;", None),
        ("ca", "call ( );", None),
        ("//", "// ", None),
        ("co", "continue;", None),
        ("da", "delete ( );", None),
        ("do", "do\n{\n} while ( condition );", Some("condition")),
        (
            "dm",
            "duplicateMovieClip ( target, newname, depth );",
            Some("target"),
        ),
        ("el", "} else {", None),
        ("ei", "} else if ( condition ) {", Some("condition")),
        ("fr", "for ( init; condition; next ) {\n}", Some("init")),
        ("fi", "for ( iterator in object ) {\n}", Some("iterator")),
        ("fs", "fscommand ( command, arguments );", Some("command")),
        ("fn", "function name ( ) {\n\t\n}", Some("name")),
        ("gu", "getURL ( url, window, method );", Some("url")),
        ("gp", "gotoAndPlay ( frame );", Some("frame")),
        ("gs", "gotoAndStop ( frame );", Some("frame")),
        ("if", "if ( condition ) {\n}", Some("condition")),
        ("il", "ifFrameLoaded ( frame ) {\n\t\n}", Some("frame")),
        ("in", "#include \"path\"", Some("path")),
        ("lm", "loadMovie ( url, target, method );", Some("url")),
        ("ln", "loadMovieNum ( url, level, method );", Some("url")),
        ("lv", "loadVariables ( url, target, method );", Some("url")),
        (
            "vn",
            "loadVariablesNum ( url, level, method );",
            Some("url"),
        ),
        ("nf", "nextFrame ();", None),
        ("ns", "nextScene ();", None),
        ("on", "on ( event ) {\n\t\n}", Some("event")),
        ("oc", "onClipEvent ( event ) {\n}", Some("event")),
        ("pl", "play ( );", None),
        ("pf", "prevFrame ( );", None),
        ("ps", "prevScene ( );", None),
        ("pr", "print ( target, type );", Some("target")),
        ("pb", "printAsBitmap ( target, type );", Some("target")),
        ("bn", "printAsBitmapNum ( level, type );", Some("level")),
        ("pn", "printNum ( level, type );", Some("level")),
        ("rm", "removeMovieClip ( target );", Some("target")),
        ("rt", "return ( );", None),
        ("sv", "set ( name, value );", Some("name")),
        (
            "sp",
            "setProperty ( target, property, value );",
            Some("target"),
        ),
        (
            "dr",
            "startDrag ( target, lockcenter, l, t, r, b );",
            Some("target"),
        ),
        ("st", "stop ( );", None),
        ("ss", "stopAllSounds ( );", None),
        ("sd", "stopDrag ( );", None),
        ("tt", "tellTarget ( target ) {\n}", Some("target")),
        ("tq", "toggleHighQuality ( );", None),
        ("tr", "trace ( message );", Some("message")),
        ("um", "unloadMovie ( target );", Some("target")),
        ("un", "unloadMovieNum ( level );", Some("level")),
        ("vr", "var ;", None),
        ("wh", "while ( condition ) {\n}", Some("condition")),
        ("wt", "with ( object ) {\n}", Some("object")),
    ];

    assert_eq!(ESC_CHORDS.len(), 50);
    let mut seen_keys = std::collections::HashSet::new();
    for (idx, &(keys_str, expected_tpl, expected_placeholder)) in expected_table.iter().enumerate()
    {
        let mut kc = keys_str.chars();
        let c1 = kc.next().unwrap();
        let c2 = kc.next().unwrap();
        assert!(kc.next().is_none());
        assert!(seen_keys.insert((c1, c2)), "duplicate chord key {keys_str}");
        assert!(is_esc_chord_first_char(c1));
        assert!(is_esc_chord_first_char(c1.to_ascii_uppercase()));

        let spec = &ESC_CHORDS[idx];
        assert_eq!(spec.keys, [c1, c2], "chord index {idx}");
        assert_eq!(
            spec.template, expected_tpl,
            "template mismatch for {keys_str}"
        );

        let (expanded, (s, e)) = expand_esc_chord("", (0, 0), c1, c2).expect("chord must expand");
        assert_eq!(
            expanded, expected_tpl,
            "unindented expansion for {keys_str}"
        );

        if let Some(placeholder) = expected_placeholder {
            let s_byte = char_to_byte(&expanded, s);
            let e_byte = char_to_byte(&expanded, e);
            assert_eq!(
                &expanded[s_byte..e_byte],
                placeholder,
                "placeholder selection mismatch for {keys_str}"
            );
            // Ensure `fs` selects the parameter `command`, not `fs[command]`.
            if keys_str == "fs" {
                assert!(s > "fscommand".len());
            }
        } else {
            assert_eq!(s, e, "expected collapsed caret for {keys_str}");
            match keys_str {
                "vr" => {
                    assert_eq!(s, 4);
                    assert_eq!(&expanded[..s], "var ");
                    assert_eq!(&expanded[s..], ";");
                }
                "ca" | "da" | "rt" => {
                    let open_p = expanded.find('(').unwrap();
                    let close_p = expanded.find(')').unwrap();
                    assert!(
                        open_p < s && s < close_p,
                        "caret {s} should sit inside `( )` for {keys_str}"
                    );
                }
                _ => {
                    assert_eq!(s, expanded.chars().count(), "caret at end for {keys_str}");
                }
            }
        }

        // Verify multi-line indentation and shifted placeholder selection on an indented line!
        let prefix = "    ";
        let (ind_out, (ind_s, ind_e)) = expand_esc_chord(
            prefix,
            (4, 4),
            c1.to_ascii_uppercase(),
            c2.to_ascii_uppercase(),
        )
        .expect("case-insensitive chord expansion");
        if expected_tpl.contains('\n') {
            for (line_idx, line) in ind_out.split('\n').enumerate() {
                assert!(
                    line.starts_with(prefix),
                    "line {line_idx} of indented `{keys_str}` missing indent: {ind_out:?}"
                );
            }
        }
        if let Some(placeholder) = expected_placeholder {
            let sb = char_to_byte(&ind_out, ind_s);
            let eb = char_to_byte(&ind_out, ind_e);
            assert_eq!(
                &ind_out[sb..eb],
                placeholder,
                "indented placeholder mismatch for {keys_str}"
            );
        }
    }

    // Verify valid 1st chord character set matches exact 18 Classic Flash prefixes.
    for valid in [
        'b', 'c', '/', 'd', 'e', 'f', 'g', 'i', 'l', 'v', 'n', 'o', 'p', 'r', 's', 't', 'u', 'w',
    ] {
        assert!(is_esc_chord_first_char(valid));
        assert!(esc_chord_first_char_prompt(valid).is_some());
    }
    for invalid in ['a', 'h', 'j', 'k', 'm', 'q', 'x', 'y', 'z', '1', ';'] {
        assert!(!is_esc_chord_first_char(invalid));
        assert!(esc_chord_first_char_prompt(invalid).is_none());
    }
    assert_eq!(
        esc_chord_first_char_prompt('f').as_deref(),
        Some("ESC f_: fn (function), fr (for), fi (for..in), fs (fscommand)")
    );
    assert!(expand_esc_chord("keep", (4, 4), 'f', 'z').is_none());
}

#[test]
fn t373_auto_format_formats_screenshot_tetris_operators_delimiters_and_stays_idempotent() {
    let unformatted = "\
TFigure=new Array([0,1,0],[1,1,1]);
lFigure = new Array([0,1,0],[0,1,0],[0,1,0],[0,1,0]);
QFigure = new Array ([1,1],[1,1]);
LlFigure = new Array ([0,1],[0,1],[1,1]);
LrFigure = new Array ([1,0],[1,0],[1,1]);
ZlFigure = new Array ([0,1],[1,1],[1,0]);
ZrFigure = new Array ([1,0],[1,1],[0,1]);

AllFigures = new Array(TFigure, lFigure, QFigure, LlFigure, LrFigure,ZlFigure, ZrFigure);

function rotateFigure (figure, direction) {
    var n=figure.length;
    var m=figure[0].length;
    //trace (n,m);
    var tempFigure:Array=new Array();
    for(i=0;i<m;i++){
        tempFigure[i]=new Array();
    }
}";
    let expected = "\
TFigure = new Array([0, 1, 0], [1, 1, 1]);
lFigure = new Array([0, 1, 0], [0, 1, 0], [0, 1, 0], [0, 1, 0]);
QFigure = new Array([1, 1], [1, 1]);
LlFigure = new Array([0, 1], [0, 1], [1, 1]);
LrFigure = new Array([1, 0], [1, 0], [1, 1]);
ZlFigure = new Array([0, 1], [1, 1], [1, 0]);
ZrFigure = new Array([1, 0], [1, 1], [0, 1]);

AllFigures = new Array(TFigure, lFigure, QFigure, LlFigure, LrFigure, ZlFigure, ZrFigure);

function rotateFigure(figure, direction) {
    var n = figure.length;
    var m = figure[0].length;
    //trace (n,m);
    var tempFigure:Array = new Array();
    for (i = 0; i < m; i++) {
        tempFigure[i] = new Array();
    }
}";
    let formatted = auto_format(unformatted).expect("should format Tetris script");
    assert_eq!(formatted, expected);
    assert_eq!(
        auto_format(&formatted),
        None,
        "auto_format must be strictly idempotent"
    );
}

#[test]
fn t373_auto_format_fixes_trailing_colon_typos_and_preserves_valid_colons_strings_regexes() {
    let src = "\
#include \"defs:init.as\"
TFigure = new Array([0, 1, 0]):
var x:Number = 1e-5 + 1e+5:
var y :  int = -x + ~0 - -2:
trace (x,y):
foo ():
var obj={a:1,b : 2,nested:{k:TFigure}};
var pick=ready?TFigure:lFigure;
var re=/a+b*c:1/gi;
var str=\"a=1,b:2\";
var anon = function (a: number, b?: string) {
    return (a + 1);
};
outerLoop:
for(i=0;i<10;i++){
    if(i<>5&&i<=8||i>>>1==2){
        switch(i){
        case 1 :
            x+=2:
            break:
        default :
            x>>>=1:
        }
    }else{
        continue:
    }
}";
    let formatted = auto_format(src).expect("should format complex script");
    let expected = "\
#include \"defs:init.as\"
TFigure = new Array([0, 1, 0]);
var x:Number = 1e-5 + 1e+5;
var y: int = -x + ~0 - -2;
trace(x, y);
foo();
var obj = { a: 1, b: 2, nested: { k: TFigure } };
var pick = ready ? TFigure : lFigure;
var re = /a+b*c:1/gi;
var str = \"a=1,b:2\";
var anon = function (a: number, b?: string) {
    return (a + 1);
};
outerLoop:
for (i = 0; i < 10; i++) {
    if (i <> 5 && i <= 8 || i >>> 1 == 2) {
        switch (i) {
            case 1:
                x += 2;
                break;
            default:
                x >>>= 1;
        }
    } else {
        continue;
    }
}";
    assert_eq!(formatted, expected);
    assert_eq!(
        auto_format(&formatted),
        None,
        "auto_format must be strictly idempotent on complex constructs"
    );
    // Incomplete/broken expressions with missing RHS must remain stable under format_on_save.
    assert_eq!(auto_format("var brokenCharacter = ;"), None);
}

#[test]
fn t373_auto_format_handles_nested_case_expressions_block_scopes_member_calls_and_2d_generics() {
    let src = "\
function onFrame(e:Event):* {
    var a:Array = []:
    var g:Vector.<Vector.<int>> = new Vector.<Vector.<int>> ():
    let tsGrid: Array<Array<number> > = [];
    let o: { a: MyType, b?: Array<Array<number>> } = { a: item };
    promise.catch (errHandler);
    promise?.catch (errHandler);
    map.delete (key);
    try {
        delete (obj.prop):
    } catch(err) {
        trace (err):
    }
    outer: {
        var b:Array = [1,2]:
    }
    switch(mode){
    case (ready?1:2):
        a.push (1):
        break:
    case ready?3:4:
        a.push (2):
        break:
    case makeKey({ a:1, nested:{ k:2 } }): {
        var c:Array = [3,4]:
        break:
    }
    default:
        break:
    }
}
class Store {
    delete(key: string): boolean {
        return map.delete (key);
    }
}";
    let expected = "\
function onFrame(e:Event):* {
    var a:Array = [];
    var g:Vector.<Vector.<int>> = new Vector.<Vector.<int>>();
    let tsGrid: Array<Array<number>> = [];
    let o: { a: MyType, b?: Array<Array<number>> } = { a: item };
    promise.catch(errHandler);
    promise?.catch(errHandler);
    map.delete(key);
    try {
        delete (obj.prop);
    } catch (err) {
        trace(err);
    }
    outer: {
        var b:Array = [1, 2];
    }
    switch (mode) {
        case (ready ? 1 : 2):
            a.push(1);
            break;
        case ready ? 3 : 4:
            a.push(2);
            break;
        case makeKey({ a: 1, nested: { k: 2 } }): {
            var c:Array = [3, 4];
            break;
        }
        default:
            break;
    }
}
class Store {
    delete(key: string): boolean {
        return map.delete(key);
    }
}";
    let formatted = auto_format(src).expect("should format edge-case constructs");
    assert_eq!(formatted, expected);
    assert_eq!(
        auto_format(&formatted),
        None,
        "auto_format must remain strictly idempotent on nested cases, :* blocks, member calls, and 2D generics"
    );
}
