use neoflash_core::{FrameScript, Project, ResourceUnit, Script, ScriptLanguage, SourceUnit};

fn add_as_resource(project: &mut Project, id: &str, path: &str, bytes: &[u8]) {
    project
        .resources
        .push(ResourceUnit::from_bytes(id, path, "text/plain", bytes).unwrap());
}

#[test]
fn as1_as2_and_as3_includes_handle_bom_line_endings_comments_and_nested_relative_paths() {
    // 1. AS2 nested relative `#include "sub/part.as"` -> `#include "../common/defs.as"`,
    //    UTF-8 BOM on both root and included files, `\r\n` & `\r` endings, and block/line comments.
    let mut as2_project = Project::blank();
    as2_project.script_language = Some(ScriptLanguage::ActionScript2);
    add_as_resource(
        &mut as2_project,
        "res-defs",
        "common/defs.as",
        "\u{feff}var BASE_SCORE:Number = 100;\r\nfunction doubleScore(v:Number):Number { return v * 2; }\r"
            .as_bytes(),
    );
    add_as_resource(
        &mut as2_project,
        "res-part",
        "sub/part.as",
        "/* header */ #include '../common/defs.as' /* inline */\r\nvar PART_BONUS:Number = 25; // ok\n"
            .as_bytes(),
    );

    let program = neoflash_script::compile_project(&as2_project).expect("compile AS2 project");
    let root_script = Script {
        language: ScriptLanguage::ActionScript2,
        source: concat!(
            "\u{feff}/* multi-line\r\n",
            "   comment before directive */ #include \"sub/part.as\" /* trailing */\r\n",
            "// #include \"missing_commented.as\"\r\n",
            "/* #include \"missing_block.as\" */\r\n",
            "#include \"NetServices.as\" /* unclosed trailing comment\r\n",
            "#include \"missing_inside_multiline.as\"\r\n",
            "*/\r\n",
            "var total:Number = doubleScore(BASE_SCORE) + PART_BONUS;\n",
            "trace(total);\n",
        )
        .into(),
    };
    let js = neoflash_script::compile_with_program(&root_script, &program)
        .expect("AS2 root script with BOM, comments, CRLF, and nested includes should compile");
    assert!(js.contains("BASE_SCORE"), "{js}");
    assert!(js.contains("PART_BONUS"), "{js}");
    assert!(js.contains("NetServices"), "{js}");

    // 2. AS1 `#include` rejects AS2/AS3 type annotations with exact included file path,
    //    and compiles untyped AS1 `#include` + `#initclip` / `#endinitclip` cleanly.
    let as1_typed_err = neoflash_script::compile_with_program(
        &Script {
            language: ScriptLanguage::ActionScript1,
            source: "#include 'sub/part.as'\n".into(),
        },
        &program,
    )
    .expect_err("AS1 must reject type annotations inside included file");
    assert_eq!(as1_typed_err[0].path.as_deref(), Some("common/defs.as"));

    add_as_resource(
        &mut as2_project,
        "res-as1-part",
        "sub/as1_part.as",
        b"var AS1_BASE = 100;\nvar AS1_BONUS = 25;\n",
    );
    let program_with_as1 = neoflash_script::compile_project(&as2_project).unwrap();
    let as1_script = Script {
        language: ScriptLanguage::ActionScript1,
        source:
            "#initclip\n#include 'sub/as1_part.as'\n#endinitclip\ntrace(AS1_BASE + AS1_BONUS);\n"
                .into(),
    };
    let as1_js = neoflash_script::compile_with_program(&as1_script, &program_with_as1)
        .expect("AS1 #include should compile");
    assert!(as1_js.contains("AS1_BASE"), "{as1_js}");

    // 3. AS3 `include "file.as";`, `include "file.as"`, and `#include "file.as"` in both frame scripts
    //    and `SourceUnit` classes.
    let mut as3_project = Project::blank();
    as3_project.format_version = 2;
    as3_project.script_language = Some(ScriptLanguage::ActionScript3);
    add_as_resource(
        &mut as3_project,
        "as3-shared",
        "common/constants.as",
        "\u{feff}var GRID_W:int = 10;\r\nvar GRID_H:int = 20;\n".as_bytes(),
    );
    add_as_resource(
        &mut as3_project,
        "as3-extra",
        "common/extra.as",
        "var BONUS_CELLS:int = 5;\n".as_bytes(),
    );
    add_as_resource(
        &mut as3_project,
        "as3-piece",
        "game/pieces/rotate.as",
        "/* nested */ include \"../../common/constants.as\";\nfunction area():int { return GRID_W * GRID_H; }\n"
            .as_bytes(),
    );
    add_as_resource(
        &mut as3_project,
        "as3-member",
        "game/class_members.as",
        "public function boardCells():int { return 200; }\n".as_bytes(),
    );
    as3_project.sources.push(SourceUnit {
        id: "src-board".into(),
        path: "game/Board.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: concat!(
            "package {\n",
            "    public class Board {\n",
            "        include \"class_members.as\"\n",
            "    }\n",
            "}\n",
        )
        .into(),
    });
    as3_project.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: concat!(
                "/* frame */ include \"game/pieces/rotate.as\";\n",
                "#include 'common/extra.as'\n",
                "var b:Board = new Board();\n",
                "trace(area() + b.boardCells() + BONUS_CELLS);\n",
            )
            .into(),
        },
    });
    let as3_program =
        neoflash_script::compile_project(&as3_project).expect("AS3 project with includes compiles");
    let frame_js = neoflash_script::compile_with_program(
        &as3_project.symbols[0].timeline.scripts[0].script,
        &as3_program,
    )
    .expect("AS3 frame script with include; and #include compiles");
    assert!(frame_js.contains("GRID_W"), "{frame_js}");
    assert!(frame_js.contains("boardCells"), "{frame_js}");
}

#[test]
fn include_error_contracts_and_nested_diagnostic_origins_are_preserved() {
    let mut project = Project::blank();
    project.script_language = Some(ScriptLanguage::ActionScript2);
    add_as_resource(&mut project, "res-cycle-a", "a.as", b"#include \"b.as\"\n");
    add_as_resource(&mut project, "res-cycle-b", "b.as", b"#include \"a.as\"\n");
    add_as_resource(
        &mut project,
        "res-broken",
        "sub/broken.as",
        b"var ok:Number = 1;\nvar bad:Number = ;\n",
    );
    add_as_resource(
        &mut project,
        "res-outer",
        "outer.as",
        b"#include \"sub/broken.as\"\n",
    );

    let program = neoflash_script::compile_project(&project).expect("compile project");

    // 1. AS2 semicolon contract
    let semi_err = neoflash_script::compile_with_program(
        &Script {
            language: ScriptLanguage::ActionScript2,
            source: "#include \"a.as\";".into(),
        },
        &program,
    )
    .expect_err("AS2 #include with semicolon must fail");
    assert!(
        semi_err[0]
            .message
            .contains("Use #include \"path.as\" on its own line without a semicolon"),
        "{semi_err:?}"
    );

    // 2. AS3 malformed include contract
    let mut as3_proj = Project::blank();
    as3_proj.script_language = Some(ScriptLanguage::ActionScript3);
    let as3_prog = neoflash_script::compile_project(&as3_proj).unwrap();
    let as3_bad = neoflash_script::compile_with_program(
        &Script {
            language: ScriptLanguage::ActionScript3,
            source: "include unquoted.as;".into(),
        },
        &as3_prog,
    )
    .expect_err("AS3 unquoted include must fail");
    assert!(
        as3_bad[0]
            .message
            .contains("Use include \"path.as\"; on its own line"),
        "{as3_bad:?}"
    );

    // 3. Root escape, non-.as extension, cycle
    for (src, needle) in [
        (
            "#include \"../escape.as\"",
            "escapes the project resource root",
        ),
        ("#include \"notes.txt\"", "expects an .as project resource"),
        ("#include \"a.as\"", "Cyclic or over-deep #include"),
    ] {
        let errs = neoflash_script::compile_with_program(
            &Script {
                language: ScriptLanguage::ActionScript2,
                source: src.into(),
            },
            &program,
        )
        .expect_err("must reject invalid include");
        assert!(
            errs[0].message.contains(needle),
            "expected `{needle}` in {errs:?}"
        );
    }

    // 4. Nested syntax error maps back to exact included path (`sub/broken.as`) and line 2!
    let nested_errs = neoflash_script::compile_with_program(
        &Script {
            language: ScriptLanguage::ActionScript2,
            source: "var before:Number = 0;\n#include \"outer.as\"\nvar after:Number = 2;\n".into(),
        },
        &program,
    )
    .expect_err("syntax error in nested include must fail");
    assert_eq!(nested_errs[0].path.as_deref(), Some("sub/broken.as"));
    assert_eq!(nested_errs[0].line, 2);

    // 5. AS2 & AS3 class `#include` / `include` diagnostics map syntax, contract, and body errors
    //    to the exact included `.as` file path and line number!
    let mut as3_class_proj = Project::blank();
    as3_class_proj.format_version = 2;
    as3_class_proj.script_language = Some(ScriptLanguage::ActionScript3);
    add_as_resource(
        &mut as3_class_proj,
        "inc-bad-body",
        "pkg/bad_members.as",
        b"public var ok:int = 1;\npublic function broken():int {\n    return \"not-an-int\" + ;\n}\n",
    );
    as3_class_proj.sources.push(SourceUnit {
        id: "src-hero".into(),
        path: "pkg/Hero.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: "package pkg {\npublic class Hero {\ninclude \"bad_members.as\";\n}\n}\n".into(),
    });
    let Err(class_errs) = neoflash_script::compile_project(&as3_class_proj) else {
        panic!("broken included AS3 class body must fail");
    };
    assert_eq!(class_errs[0].path.as_deref(), Some("pkg/bad_members.as"));
    assert_eq!(class_errs[0].line, 3);

    let mut as3_type_proj = Project::blank();
    as3_type_proj.format_version = 2;
    as3_type_proj.script_language = Some(ScriptLanguage::ActionScript3);
    add_as_resource(
        &mut as3_type_proj,
        "inc-bad-override",
        "pkg/bad_override.as",
        b"// line 1\noverride public function nonexistent():void {}\n",
    );
    as3_type_proj.sources.push(SourceUnit {
        id: "src-hero-ov".into(),
        path: "pkg/HeroOv.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: "package pkg {\npublic class HeroOv {\ninclude \"bad_override.as\";\n}\n}\n".into(),
    });
    let Err(ov_errs) = neoflash_script::compile_project(&as3_type_proj) else {
        panic!("invalid override inside included AS3 class member must fail");
    };
    assert_eq!(ov_errs[0].path.as_deref(), Some("pkg/bad_override.as"));
    assert_eq!(ov_errs[0].line, 2);

    let mut as2_good_proj = Project::blank();
    as2_good_proj.script_language = Some(ScriptLanguage::ActionScript2);
    add_as_resource(
        &mut as2_good_proj,
        "inc-as2-good",
        "app/as2_good_member.as",
        b"// comment\npublic var computed:Array = [1, 2, 3];\n",
    );
    as2_good_proj.sources.push(SourceUnit {
        id: "src-as2-widget-good".into(),
        path: "app/Widget.as".into(),
        language: ScriptLanguage::ActionScript2,
        source: "class app.Widget {\n#include \"as2_good_member.as\"\n}\n".into(),
    });
    neoflash_script::compile_project(&as2_good_proj)
        .expect("non-scalar inline initializer inside AS2 included member compiles cleanly");

    let mut as2_class_proj = Project::blank();
    as2_class_proj.script_language = Some(ScriptLanguage::ActionScript2);
    add_as_resource(
        &mut as2_class_proj,
        "inc-as2-bad",
        "app/as2_bad_member.as",
        b"// comment\noverride public function invalid():Void {}\n",
    );
    as2_class_proj.sources.push(SourceUnit {
        id: "src-as2-widget".into(),
        path: "app/Widget.as".into(),
        language: ScriptLanguage::ActionScript2,
        source: "class app.Widget {\n#include \"as2_bad_member.as\"\n}\n".into(),
    });
    let Err(as2_errs) = neoflash_script::compile_project(&as2_class_proj) else {
        panic!("AS3-only override modifier inside AS2 included member must fail");
    };
    assert_eq!(as2_errs[0].path.as_deref(), Some("app/as2_bad_member.as"));
    assert_eq!(as2_errs[0].line, 2);
}
