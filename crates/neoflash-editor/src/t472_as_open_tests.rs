use super::*;

struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "neoflash-t472-{tag}-{}-{}",
            std::process::id(),
            new_id("dir")
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn encode_utf16(text: &str, little_endian: bool) -> Vec<u8> {
    let mut out = if little_endian {
        vec![0xff, 0xfe]
    } else {
        vec![0xfe, 0xff]
    };
    for unit in text.encode_utf16() {
        let pair = if little_endian {
            unit.to_le_bytes()
        } else {
            unit.to_be_bytes()
        };
        out.extend_from_slice(&pair);
    }
    out
}

fn encode_cp1251_subset(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_ascii() {
            out.push(ch as u8);
        } else if ('\u{0410}'..='\u{042f}').contains(&ch) {
            out.push(0xc0 + (ch as u32 - 0x0410) as u8);
        } else if ('\u{0430}'..='\u{044f}').contains(&ch) {
            out.push(0xe0 + (ch as u32 - 0x0430) as u8);
        } else if ch == 'Ё' {
            out.push(0xa8);
        } else if ch == 'ё' {
            out.push(0xb8);
        } else if ch == '№' {
            out.push(0xb9);
        } else {
            panic!("unexpected char for synthetic CP1251 encoder: {ch:?}");
        }
    }
    out
}

#[test]
fn t472_binary_applesingle_appledouble_nul_and_legacy_encodings() {
    let tmp = TempDirGuard::new("encodings");
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.sessions.clear();
    editor.new_document_session(&ctx);
    assert_eq!(editor.session_count(), 1);
    assert!(!editor.is_standalone_script_session());

    // 1. Real binary AppleSingle (`0x00051600`) must fail cleanly without mutating active session.
    let applesingle = tmp.path().join("Archive.as");
    let mut as_bin = b"\x00\x05\x16\x00\x00\x02\x00\x00Mac OS X        ".to_vec();
    as_bin.extend_from_slice(&[0u8; 32]);
    std::fs::write(&applesingle, &as_bin).unwrap();
    assert!(!editor.open_standalone_script_session(&ctx, applesingle));
    assert_eq!(editor.session_count(), 1);
    assert!(!editor.is_standalone_script_session());
    assert!(
        editor.status.contains("AppleSingle archive"),
        "expected AppleSingle error message, got: {}",
        editor.status
    );

    // 2. Real binary AppleDouble (`0x00051607`, e.g. macOS `._Intersection.as` resource fork) must fail cleanly.
    let appledouble = tmp.path().join("._Intersection.as");
    let mut ad_bin = b"\x00\x05\x16\x07\x00\x02\x00\x00Mac OS X        ".to_vec();
    ad_bin.extend_from_slice(&[0u8; 32]);
    std::fs::write(&appledouble, &ad_bin).unwrap();
    assert!(!editor.open_standalone_script_session(&ctx, appledouble));
    assert_eq!(editor.session_count(), 1);
    assert!(
        editor.status.contains("AppleDouble"),
        "expected AppleDouble error message, got: {}",
        editor.status
    );

    // 3. NUL-containing binary data misnamed `.as` and truncated UTF-16LE must fail cleanly.
    let nul_bin = tmp.path().join("FakeImage.as");
    std::fs::write(&nul_bin, b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR").unwrap();
    assert!(!editor.open_standalone_script_session(&ctx, nul_bin));
    assert!(
        editor.status.contains("NUL bytes"),
        "expected binary NUL diagnostic, got: {}",
        editor.status
    );

    let trunc_utf16 = tmp.path().join("Truncated.as");
    std::fs::write(&trunc_utf16, b"\xff\xfeA").unwrap();
    assert!(!editor.open_standalone_script_session(&ctx, trunc_utf16));
    assert!(
        editor.status.contains("UTF-16"),
        "expected UTF-16 diagnostic, got: {}",
        editor.status
    );
    assert_eq!(editor.session_count(), 1);

    // 4. UTF-8 BOM + CRLF (`\r\n`) + Cyrillic ASDoc loads cleanly with `\n` line endings and `!dirty()`.
    let utf8_bom_path = tmp.path().join("Utf8Bom.as");
    let mut utf8_bom_bytes = b"\xef\xbb\xbf".to_vec();
    utf8_bom_bytes.extend_from_slice(
        "package {\r\n\t/** Проверка UTF-8 с BOM. */\r\n\tpublic class Utf8Bom {\r\n\t\tpublic function label():String { return \"Привет\"; }\r\n\t}\r\n}\r\n"
            .as_bytes(),
    );
    std::fs::write(&utf8_bom_path, &utf8_bom_bytes).unwrap();
    assert!(editor.open_standalone_script_session(&ctx, utf8_bom_path));
    assert!(editor.is_standalone_script_session());
    assert_eq!(
        editor.standalone_script_language(),
        Some(ScriptLanguage::ActionScript3)
    );
    let src_utf8 = editor.standalone_script_source().unwrap();
    assert!(!src_utf8.starts_with('\u{feff}'));
    assert!(!src_utf8.contains('\r'));
    assert!(src_utf8.contains("Проверка UTF-8 с BOM."));
    assert!(!editor.dirty());
    editor.check_standalone_script(true);
    assert_eq!(editor.status, "Code accepted for current language support.");

    // 5. UTF-16LE BOM + CRLF and UTF-16BE BOM + classic Mac CR (`\r`) load cleanly.
    let utf16le_path = tmp.path().join("Utf16Le.as");
    std::fs::write(
        &utf16le_path,
        encode_utf16(
            "package {\r\n\t/** Документация UTF-16LE */\r\n\tpublic class Utf16Le {\r\n\t\tpublic function ok():int { return 7; }\r\n\t}\r\n}\r\n",
            true,
        ),
    )
    .unwrap();
    assert!(editor.open_standalone_script_session(&ctx, utf16le_path));
    let src_16le = editor.standalone_script_source().unwrap();
    assert!(!src_16le.contains('\r'));
    assert!(src_16le.contains("Документация UTF-16LE"));
    assert!(!editor.dirty());
    editor.check_standalone_script(true);
    assert_eq!(editor.status, "Code accepted for current language support.");

    let utf16be_path = tmp.path().join("Utf16Be.as");
    std::fs::write(
        &utf16be_path,
        encode_utf16(
            "package {\r\t/** Классический Mac CR */\r\tpublic class Utf16Be {\r\t\tpublic function ok():int { return 9; }\r\t}\r}\r",
            false,
        ),
    )
    .unwrap();
    assert!(editor.open_standalone_script_session(&ctx, utf16be_path));
    let src_16be = editor.standalone_script_source().unwrap();
    assert!(!src_16be.contains('\r'));
    assert!(src_16be.contains("Классический Mac CR"));
    assert!(!editor.dirty());
    editor.check_standalone_script(true);
    assert_eq!(editor.status, "Code accepted for current language support.");

    // 6. Legacy Windows-1251 (Cyrillic + trailing DOS 0x1A) and Windows-1252 (Western accents + quotes).
    let cp1251_path = tmp.path().join("LegacyCyrillic.as");
    let mut cp1251_bytes = encode_cp1251_subset(
        "package {\r\n\t// Проверка кодировки Кириллица №1\r\n\tpublic class LegacyCyrillic {\r\n\t\tpublic static const TITLE:String = \"Кривая Безье\";\r\n\t}\r\n}\r\n",
    );
    cp1251_bytes.push(0x1a);
    std::fs::write(&cp1251_path, &cp1251_bytes).unwrap();
    assert!(editor.open_standalone_script_session(&ctx, cp1251_path));
    let src_1251 = editor.standalone_script_source().unwrap();
    assert!(!src_1251.contains('\r'));
    assert!(src_1251.contains("Проверка кодировки Кириллица №1"));
    assert!(src_1251.contains("Кривая Безье"));
    assert!(!editor.dirty());
    editor.check_standalone_script(true);
    assert_eq!(editor.status, "Code accepted for current language support.");

    let cp1252_path = tmp.path().join("LegacyWestern.as");
    // 0x93/0x94 = “/”, 0x97 = —, 0xe9 = é, 0xef = ï, 0xa9 = © in Windows-1252
    let mut cp1252_bytes = Vec::new();
    cp1252_bytes.extend_from_slice(b"package {\r\t// Caf\xe9 na\xefve \x97 r\xe9sum\xe9 \xa9 2007\r\tpublic class LegacyWestern {\r\t\tpublic static const LABEL:String = \"\x93B\xe9zier\x94\";\r\t}\r}\r");
    std::fs::write(&cp1252_path, &cp1252_bytes).unwrap();
    assert!(editor.open_standalone_script_session(&ctx, cp1252_path));
    let src_1252 = editor.standalone_script_source().unwrap();
    assert!(!src_1252.contains('\r'));
    assert!(src_1252.contains("Café naïve — résumé © 2007"));
    assert!(src_1252.contains("“Bézier”"));
    assert!(!editor.dirty());
    editor.check_standalone_script(true);
    assert_eq!(editor.status, "Code accepted for current language support.");
}

#[test]
fn t472_synthetic_flash_geom_package_root_hydration_multi_open_and_live_tab_override() {
    let tmp = TempDirGuard::new("synthetic-pkg-root");
    let src_root = tmp.path().join("project").join("src");
    let geom_dir = src_root.join("flash").join("geom");
    let math_dir = src_root.join("flash").join("math");
    let howtodo_dir = src_root.join("howtodo");
    let view_dir = howtodo_dir.join("view");
    std::fs::create_dir_all(&geom_dir).unwrap();
    std::fs::create_dir_all(&math_dir).unwrap();
    std::fs::create_dir_all(&view_dir).unwrap();

    let iparametric_path = geom_dir.join("IParametric.as");
    std::fs::write(
        &iparametric_path,
        "package flash.geom {\r\n\tpublic interface IParametric {\r\n\t\tfunction get length():Number;\r\n\t\tfunction getPoint(time:Number, out:Point = null):Point;\r\n\t\tfunction intersectionLine(target:Line):Intersection;\r\n\t\tfunction intersectionBezier(target:Bezier):Intersection;\r\n\t}\r\n}\r\n",
    )
    .unwrap();

    let equations_path = math_dir.join("Equations.as");
    let mut eq_bytes = b"\xef\xbb\xbf".to_vec();
    eq_bytes.extend_from_slice(
        "package flash.math {\r\n\t/** Аналитическое решение линейных и квадратных уравнений. */\r\n\tpublic class Equations {\r\n\t\tpublic static const EPS:Number = 1e-9;\r\n\t\tpublic static function solveLinear(a:Number, b:Number):Array {\r\n\t\t\tif (Math.abs(a) < EPS) { return []; }\r\n\t\t\treturn [-b / a];\r\n\t\t}\r\n\t}\r\n}\r\n"
            .as_bytes(),
    );
    std::fs::write(&equations_path, &eq_bytes).unwrap();

    let line_path = geom_dir.join("Line.as");
    std::fs::write(
        &line_path,
        "package flash.geom {\r\n\timport flash.math.Equations;\r\n\tpublic class Line extends Object implements IParametric {\r\n\t\tpublic var start:Point = new Point();\r\n\t\tpublic var end:Point = new Point(10, 10);\r\n\t\tpublic function Line(s:Point = null, e:Point = null) {\r\n\t\t\tif (s) { start = s; }\r\n\t\t\tif (e) { end = e; }\r\n\t\t}\r\n\t\tpublic function get length():Number { return Point.distance(start, end); }\r\n\t\tpublic function getPoint(time:Number, out:Point = null):Point {\r\n\t\t\tvar roots:Array = Equations.solveLinear(1, -time);\r\n\t\t\tvar t:Number = roots.length > 0 ? Number(roots[0]) : time;\r\n\t\t\tvar p:Point = out ? out : new Point();\r\n\t\t\tp.x = start.x + (end.x - start.x) * t;\r\n\t\t\tp.y = start.y + (end.y - start.y) * t;\r\n\t\t\treturn p;\r\n\t\t}\r\n\t\tpublic function intersectionLine(target:Line):Intersection { return new Intersection(); }\r\n\t\tpublic function intersectionBezier(target:Bezier):Intersection { return new Intersection(); }\r\n\t}\r\n}\r\n",
    )
    .unwrap();

    let bezier_path = geom_dir.join("Bezier.as");
    let mut bez_bytes = b"\xef\xbb\xbf".to_vec();
    bez_bytes.extend_from_slice(
        "package flash.geom {\r\n\timport flash.math.Equations;\r\n\tpublic class Bezier extends Object implements IParametric {\r\n\t\tpublic var start:Point = new Point();\r\n\t\tpublic var control:Point = new Point(5, 15);\r\n\t\tpublic var end:Point = new Point(20, 0);\r\n\t\tpublic function Bezier(s:Point = null, c:Point = null, e:Point = null) {\r\n\t\t\tif (s) { start = s; }\r\n\t\t\tif (c) { control = c; }\r\n\t\t\tif (e) { end = e; }\r\n\t\t}\r\n\t\tpublic function get length():Number {\r\n\t\t\tvar r:Array = Equations.solveLinear(1, -1);\r\n\t\t\treturn r.length + Point.distance(start, end);\r\n\t\t}\r\n\t\tpublic function getPoint(time:Number, out:Point = null):Point {\r\n\t\t\tvar p:Point = out ? out : new Point();\r\n\t\t\tp.x = start.x * (1 - time) + end.x * time;\r\n\t\t\tp.y = start.y * (1 - time) + end.y * time;\r\n\t\t\treturn p;\r\n\t\t}\r\n\t\tpublic function intersectionLine(target:Line):Intersection { return new Intersection(); }\r\n\t\tpublic function intersectionBezier(target:Bezier):Intersection { return new Intersection(); }\r\n\t}\r\n}\r\n"
            .as_bytes(),
    );
    std::fs::write(&bezier_path, &bez_bytes).unwrap();

    let intersection_path = geom_dir.join("Intersection.as");
    std::fs::write(
        &intersection_path,
        "package flash.geom {\r\n\t/**\r\n\t * Результат пересечения кривой Безье и отрезка.\r\n\t * @langversion 3.0\r\n\t */\r\n\tpublic class Intersection extends Object {\r\n\t\tpublic var isCoincidence:Boolean = false;\r\n\t\tpublic var coincidenceLine:Line;\r\n\t\tpublic var coincidenceBezier:Bezier;\r\n\t\tpublic const currentTimes:Array = new Array();\r\n\t\tpublic const targetTimes:Array = new Array();\r\n\t\tpublic static function isIntersectionPossible(a:Rectangle, b:Rectangle):Boolean {\r\n\t\t\treturn a.clone().intersects(b.clone());\r\n\t\t}\r\n\t}\r\n}\r\n",
    )
    .unwrap();

    let drag_handle_path = view_dir.join("DragHandle.as");
    std::fs::write(
        &drag_handle_path,
        "package howtodo.view {\r\n\timport flash.display.Sprite;\r\n\timport flash.events.Event;\r\n\timport flash.geom.Point;\r\n\t[Event(name=\"change\", type=\"flash.events.Event\")]\r\n\tpublic class DragHandle extends Sprite {\r\n\t\tpublic static var grid:uint = 50;\r\n\t\tpublic const position:Point = new Point();\r\n\t\tpublic const changeEvent:Event = new Event(Event.CHANGE);\r\n\t}\r\n}\r\n",
    )
    .unwrap();

    let numeric_field_path = view_dir.join("NumericField.as");
    std::fs::write(
        &numeric_field_path,
        "package howtodo.view {\r\n\timport flash.text.TextField;\r\n\tpublic class NumericField extends TextField {\r\n\t\tpublic function NumericField() { super(); }\r\n\t}\r\n}\r\n",
    )
    .unwrap();

    let demo_base_path = howtodo_dir.join("CurveDemoBase.as");
    std::fs::write(
        &demo_base_path,
        "package howtodo {\r\n\timport flash.display.Sprite;\r\n\timport flash.geom.Bezier;\r\n\timport flash.geom.Intersection;\r\n\timport flash.geom.Line;\r\n\timport howtodo.view.DragHandle;\r\n\tpublic class CurveDemoBase extends Sprite {\r\n\t\tprotected var curve:Bezier = new Bezier();\r\n\t\tprotected var segment:Line = new Line();\r\n\t\tprotected var handle:DragHandle = new DragHandle();\r\n\t\tpublic function checkHit():Intersection {\r\n\t\t\treturn curve.intersectionLine(segment);\r\n\t\t}\r\n\t}\r\n}\r\n",
    )
    .unwrap();

    let smooth_demo_path = howtodo_dir.join("SmoothDemo.as");
    std::fs::write(
        &smooth_demo_path,
        "package howtodo {\r\n\timport flash.geom.Point;\r\n\tpublic class SmoothDemo extends CurveDemoBase {\r\n\t\tpublic function stepChain():Point {\r\n\t\t\tvar node:ChainNode = new ChainNode(3, 4);\r\n\t\t\tnode.advance();\r\n\t\t\treturn node;\r\n\t\t}\r\n\t}\r\n}\r\nimport flash.geom.Point;\r\ninternal class ChainNode extends Point {\r\n\tpublic var vx:Number = 1;\r\n\tpublic function ChainNode(px:Number = 0, py:Number = 0) {\r\n\t\tsuper(px, py);\r\n\t}\r\n\tpublic function advance():void {\r\n\t\tx += vx;\r\n\t}\r\n}\r\n",
    )
    .unwrap();

    let main_demo_path = src_root.join("MainDemo.as");
    std::fs::write(
        &main_demo_path,
        "package {\r\n\timport flash.display.Sprite;\r\n\timport howtodo.*;\r\n\timport howtodo.view.*;\r\n\t[SWF(backgroundColor=\"0xFFFFFF\")]\r\n\tpublic class MainDemo extends Sprite {\r\n\t\tpublic function MainDemo() {\r\n\t\t\tvar demo:SmoothDemo = new SmoothDemo();\r\n\t\t\taddChild(demo);\r\n\t\t\tvar input:NumericField = new NumericField();\r\n\t\t\tinput.text = \"grid: \" + DragHandle.grid;\r\n\t\t\taddChild(input);\r\n\t\t}\r\n\t}\r\n}\r\n",
    )
    .unwrap();

    // Unreferenced broken file in `src/` must NEVER poison demand-driven class hydration!
    std::fs::write(
        src_root.join("UnreferencedBroken.as"),
        "package { public class UnreferencedBroken { @@@ syntax error } }\n",
    )
    .unwrap();

    let all_clean_paths = vec![
        iparametric_path.clone(),
        equations_path.clone(),
        line_path.clone(),
        bezier_path.clone(),
        intersection_path.clone(),
        drag_handle_path.clone(),
        demo_base_path.clone(),
        smooth_demo_path.clone(),
        main_demo_path.clone(),
    ];

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.sessions.clear();

    // Multi-select `File ▸ Open…` (`open_picked_paths`) opens all 9 `.as` files in deduplicated tabs.
    let opened = editor.open_picked_paths(all_clean_paths.clone(), &ctx);
    assert_eq!(opened, 9);
    assert_eq!(editor.session_count(), 9);
    assert_eq!(editor.status, "Opened 9 ActionScript files.");

    // Re-opening the same batch (plus a duplicate entry) keeps `session_count() == 9`.
    let mut dup_batch = all_clean_paths.clone();
    dup_batch.push(intersection_path.clone());
    let _ = editor.open_picked_paths(dup_batch, &ctx);
    assert_eq!(editor.session_count(), 9);

    let canon_src_root = std::fs::canonicalize(&src_root).unwrap();
    for idx in 0..editor.session_count() {
        editor.switch_session_tab(&ctx, idx);
        assert!(editor.is_standalone_script_session());
        assert_eq!(
            editor.standalone_script_language(),
            Some(ScriptLanguage::ActionScript3)
        );
        assert!(
            !editor.dirty(),
            "tab {idx} ({}) must open clean",
            editor.project.name
        );
        let inferred_root = editor
            .active_project_include_base_dir()
            .expect("inferred src root");
        assert_eq!(
            std::fs::canonicalize(&inferred_root).unwrap(),
            canon_src_root,
            "tab {} must infer enclosing src/ package root",
            editor.project.name
        );

        editor.check_standalone_script(true);
        editor.refresh_compiler_diagnostics();
        let errors_only: Vec<_> = editor
            .compiler_errors
            .items
            .iter()
            .filter(|item| item.is_error)
            .collect();
        assert!(
            errors_only.is_empty(),
            "expected 0 compiler errors on {} (status: {}), got: {:?}",
            editor.project.name,
            editor.status,
            errors_only
        );
        for item in &editor.compiler_errors.items {
            assert!(
                item.location_label.starts_with(&editor.project.name),
                "sibling diagnostic leaked into {}: {:?}",
                editor.project.name,
                item
            );
        }
        if editor.project.name != "MainDemo.as" {
            assert_eq!(
                editor.status, "Code accepted for current language support.",
                "tab {} failed Check: {}",
                editor.project.name, editor.status
            );
        }
    }

    // Verify live unsaved cross-tab hydration: add a new method in `Equations.as` tab and call it from `Intersection.as` tab!
    let eq_tab = editor.session_for_open_path(&equations_path).unwrap();
    editor.switch_session_tab(&ctx, eq_tab);
    editor.transact_named("Add solveCubic", |ed| {
        ed.project.sources[0].source = ed.project.sources[0].source.replace(
            "\t}\n}",
            "\t\tpublic static function solveCubic(a:Number):Number { return a * 3; }\n\t}\n}",
        );
    });
    editor.invalidate_include_dependents_across_sessions();

    let inter_tab = editor.session_for_open_path(&intersection_path).unwrap();
    editor.switch_session_tab(&ctx, inter_tab);
    editor.transact_named("Call unsaved solveCubic", |ed| {
        ed.project.sources[0].source = ed.project.sources[0]
            .source
            .replace(
                "package flash.geom {",
                "package flash.geom {\n\timport flash.math.Equations;",
            )
            .replace(
                "return a.clone().intersects(b.clone());",
                "return Equations.solveCubic(1) > 0 && a.clone().intersects(b.clone());",
            );
    });
    editor.check_standalone_script(true);
    assert_eq!(
        editor.status, "Code accepted for current language support.",
        "live unsaved sibling tab edit must hydrate into Intersection.as: {}",
        editor.status
    );
}

#[test]
fn t472_stage_drag_and_drop_routes_single_and_multiple_as_files_to_standalone_tabs() {
    let tmp = TempDirGuard::new("stage-drop-as");
    let first_as = tmp.path().join("First.as");
    let second_as = tmp.path().join("Second.as");
    std::fs::write(&first_as, "package {\n\tpublic class First {}\n}\n").unwrap();
    std::fs::write(&second_as, "package {\n\tpublic class Second {}\n}\n").unwrap();

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    assert_eq!(editor.session_count(), 1);
    assert!(!editor.is_standalone_script_session());

    // 1. Direct `stage_file_drop` with multiple `.as` files opens standalone tabs without graphic-import errors.
    let drop_raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        dropped_files: vec![
            egui::DroppedFile {
                path: Some(first_as.clone()),
                ..Default::default()
            },
            egui::DroppedFile {
                path: Some(second_as.clone()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let _ = ctx.run(drop_raw, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let area = egui::Rect::from_min_size(egui::pos2(50.0, 50.0), egui::vec2(800.0, 600.0));
            editor.stage_file_drop(
                ui,
                area,
                egui::pos2(100.0, 100.0),
                crate::stage_mapping::StageMapping::identity(),
            );
        });
    });
    assert_eq!(
        editor.session_count(),
        3,
        "dropping 2 .as files onto Stage must open 2 standalone tabs alongside the authoring tab"
    );
    assert!(editor.is_standalone_script_session());
    assert!(!editor.status.starts_with("Import failed"));

    // 2. Switch back to authoring tab 0 and exercise mixed `.as` + `.svg` window drop interception.
    editor.switch_session_tab(&ctx, 0);
    assert!(!editor.is_standalone_script_session());
    let third_as = tmp.path().join("Third.as");
    let icon_svg = tmp.path().join("icon.svg");
    std::fs::write(&third_as, "package {\n\tpublic class Third {}\n}\n").unwrap();
    std::fs::write(
        &icon_svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\"><rect width=\"24\" height=\"24\" fill=\"#ff0000\"/></svg>",
    )
    .unwrap();

    let mixed_raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        events: vec![egui::Event::PointerMoved(egui::pos2(400.0, 400.0))],
        dropped_files: vec![
            egui::DroppedFile {
                path: Some(third_as),
                ..Default::default()
            },
            egui::DroppedFile {
                path: Some(icon_svg.clone()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let mut remaining_after_window_hook = Vec::new();
    let _ = ctx.run(mixed_raw, |ctx| {
        assert!(editor.handle_window_document_or_as_file_drop(ctx));
        remaining_after_window_hook = ctx.input(|i| i.raw.dropped_files.clone());
    });
    assert_eq!(editor.session_count(), 4);
    assert_eq!(remaining_after_window_hook.len(), 1);
    assert_eq!(
        remaining_after_window_hook[0].path.as_deref(),
        Some(icon_svg.as_path()),
        "mixed drop must open .as tabs while leaving non-.as Stage assets intact"
    );
}

fn find_local_bezier_src_dir() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest
        .parent()
        .and_then(Path::parent)
        .unwrap_or(&manifest);
    let direct = workspace_root.join("examples/old flash files/bezier/src");
    if direct.is_dir() {
        return Some(direct);
    }
    let dot_git = workspace_root.join(".git");
    if dot_git.is_file()
        && let Ok(contents) = std::fs::read_to_string(&dot_git)
        && let Some(gitdir_raw) = contents.lines().find_map(|l| l.strip_prefix("gitdir:"))
    {
        let gitdir = PathBuf::from(gitdir_raw.trim());
        if let Some(main_repo) = gitdir
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
        {
            let shared = main_repo.join("examples/old flash files/bezier/src");
            if shared.is_dir() {
                return Some(shared);
            }
        }
    }
    None
}

#[test]
fn t472_local_bezier_src_all_29_files_open_highlight_format_and_check_cleanly() {
    // PRIVACY RULE: `examples/old flash files/bezier/src` is local-only and git-ignored.
    // Skip gracefully when absent; verify all 29 files live when present on disk.
    let Some(bezier_src_buf) = find_local_bezier_src_dir() else {
        return;
    };
    let bezier_src = bezier_src_buf.as_path();

    fn collect_as_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| Some(e.ok()?.path())).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                collect_as_files(&path, out);
            } else if path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("as"))
            {
                out.push(path);
            }
        }
    }

    let mut files = Vec::new();
    collect_as_files(bezier_src, &mut files);
    assert_eq!(
        files.len(),
        29,
        "expected all 29 .as files in local bezier/src corpus"
    );

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.sessions.clear();

    let opened = editor.open_picked_paths(files.clone(), &ctx);
    assert_eq!(opened, 29);
    assert_eq!(editor.session_count(), 29);

    // Re-selecting all 29 files in `File ▸ Open…` must deduplicate to the existing 29 tabs.
    assert_eq!(editor.open_picked_paths(files.clone(), &ctx), 29);
    assert_eq!(editor.session_count(), 29);

    let canon_bezier_src = std::fs::canonicalize(bezier_src).unwrap();
    let mut zero_compile_error_files = 0usize;
    let mut zero_diagnostic_files = 0usize;

    for idx in 0..editor.session_count() {
        editor.switch_session_tab(&ctx, idx);
        let tab_name = editor.project.name.clone();
        assert!(editor.is_standalone_script_session());
        assert_eq!(
            editor.standalone_script_language(),
            Some(ScriptLanguage::ActionScript3),
            "{tab_name} must auto-detect ActionScript 3.0"
        );
        assert!(!editor.dirty(), "{tab_name} must open clean");

        let source = editor.standalone_script_source().unwrap().to_owned();
        assert!(!source.is_empty(), "{tab_name} must not be empty");
        assert!(
            !source.starts_with('\u{feff}'),
            "{tab_name} must strip UTF-8 BOM"
        );
        assert!(
            !source.contains('\r'),
            "{tab_name} must normalize CRLF/CR to LF"
        );

        // Every file in `bezier/src` (including `flash/geom/Intersection.as` & `flash/math/Equations.as`)
        // must infer `.../bezier/src` as its enclosing package root.
        let base_dir = editor
            .active_project_include_base_dir()
            .expect("base_dir for bezier/src tab");
        assert_eq!(
            std::fs::canonicalize(&base_dir).unwrap(),
            canon_bezier_src,
            "{tab_name} inferred wrong package root: {}",
            base_dir.display()
        );

        // Render one full UI frame of the standalone script editor (syntax highlighting, gutter, status bar).
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1440.0, 900.0),
                )),
                ..Default::default()
            },
            |ctx| {
                editor.document_bar(ctx);
                editor.standalone_script_editor_central(ctx);
            },
        );

        // Verify auto-formatting succeeds without panicking or dropping non-empty code.
        let formatted = crate::code_tools::auto_format_with_tab_size(&source, 4)
            .unwrap_or_else(|| source.clone());
        assert!(
            formatted.contains("package"),
            "{tab_name} formatted output lost package declaration"
        );

        // Check syntax & compiler diagnostics: zero unresolved-import, wildcard, metadata, or sibling-file errors!
        editor.check_standalone_script(true);
        editor.refresh_compiler_diagnostics();
        let status = editor.status.clone();
        for forbidden in [
            "Unresolved or inaccessible import",
            "Wildcard import",
            "Unknown base class",
            "is not a resolved project interface",
            "Expected 'class'",
            "A source unit must contain exactly one class",
        ] {
            assert!(
                !status.contains(forbidden),
                "spurious compiler error on {tab_name}: {status}"
            );
        }
        for item in &editor.compiler_errors.items {
            assert!(
                item.location_label.starts_with(&tab_name),
                "external sibling diagnostic leaked into {tab_name}: {:?}",
                item
            );
        }
        if editor
            .compiler_errors
            .items
            .iter()
            .all(|item| !item.is_error)
        {
            zero_compile_error_files += 1;
        }
        if status == "Code accepted for current language support." {
            zero_diagnostic_files += 1;
        }
        assert!(
            !editor.dirty(),
            "{tab_name} must remain clean after Check & UI render"
        );
    }

    assert!(
        zero_compile_error_files >= 18,
        "expected at least 18 bezier/src classes to have 0 compiler errors, got {zero_compile_error_files}"
    );
    assert!(
        zero_diagnostic_files >= 15,
        "expected at least 15 bezier/src classes to have 0 errors and 0 warnings, got {zero_diagnostic_files}"
    );
}

#[test]
fn t472_a01_a02_a08_multibyte_escaped_sibling_stubs_hydration_cache_and_c1_fallback() {
    let dir = TempDirGuard::new("a01-a02-a08");
    let pkg_dir = dir.path().join("pkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();

    // [A-01] Sibling `.as` with backslash + multi-byte UTF-8 scalars at class/signature depth AND body depth!
    let sibling_src = concat!(
        "package pkg {\n",
        "    public class Sibling {\n",
        "        public static const DASH:String = \"\\—\";\n",
        "        public static const WIN_PATH:String = \"C:\\Папка\\файл\";\n",
        "        public function greet(tag:String = \"\\🎉\"):String {\n",
        "            var localPath:String = \"D:\\Тест\\—\\🎉\";\n",
        "            return localPath + tag;\n",
        "        }\n",
        "    }\n",
        "}\n"
    );
    let stubbed = crate::app::standalone_script::stub_external_sibling_class_source(
        ScriptLanguage::ActionScript3,
        sibling_src,
    );
    assert!(stubbed.contains("\"\\—\""));
    assert!(stubbed.contains("\"C:\\Папка\\файл\""));
    assert!(stubbed.contains("{ throw null; }"));

    let sibling_path = pkg_dir.join("Sibling.as");
    let main_path = pkg_dir.join("Main.as");
    std::fs::write(&sibling_path, sibling_src).unwrap();
    std::fs::write(
        &main_path,
        concat!(
            "package pkg {\n",
            "    public class Main {\n",
            "        public function run():String {\n",
            "            var s:Sibling = new Sibling();\n",
            "            return Sibling.DASH + s.greet(\"ok\");\n",
            "        }\n",
            "    }\n",
            "}\n"
        ),
    )
    .unwrap();

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.sessions.clear();
    assert!(editor.try_open_path(main_path.clone(), &ctx));

    // [A-02] Repeated per-frame hydration calls must hit `standalone_hydration_cache` with 0 extra disk runs,
    // and `invalidate_include_dependents_across_sessions()` must invalidate cleanly!
    editor.standalone_hydration_cache.borrow_mut().take();
    editor.standalone_hydration_disk_runs.set(0);

    let (p1, fp1) = editor.hydrated_project_and_fingerprint();
    assert_eq!(editor.standalone_hydration_disk_runs.get(), 1);
    assert!(p1.sources.len() >= 2, "Sibling.as must be hydrated");

    for _ in 0..8 {
        let (pn, fpn) = editor.hydrated_project_and_fingerprint();
        assert_eq!(fpn, fp1);
        assert_eq!(pn.sources.len(), p1.sources.len());
    }
    editor.refresh_compiler_diagnostics();
    assert_eq!(
        editor.standalone_hydration_disk_runs.get(),
        1,
        "repeated per-frame hydration calls must hit cache with 0 extra disk runs"
    );

    // Disk reload / include invalidation forces exactly 1 fresh disk hydration:
    editor.invalidate_include_dependents_across_sessions();
    let (_, fp2) = editor.hydrated_project_and_fingerprint();
    assert_eq!(fp2, fp1);
    assert_eq!(editor.standalone_hydration_disk_runs.get(), 2);

    // [A-08] Mixed legacy C1 bytes (`0x98` + unassigned CP1252 `0x81`, `0x8D`, `0x8F`, `0x90`, `0x9D`)
    // must decode cleanly without error when passing the binary/NUL guard!
    let mut mixed_c1 = b"// Legacy comment: ".to_vec();
    mixed_c1.extend_from_slice(&[0x98, 0x81, 0x8D, 0x8F, 0x90, 0x9D]);
    mixed_c1.extend_from_slice(b"\r\nvar ok:Number = 1;\r\n");
    let decoded = crate::app::standalone_script::decode_actionscript_source_bytes(
        Path::new("legacy_mixed.as"),
        &mixed_c1,
    )
    .expect("mixed legacy C1 comment bytes must decode without error");
    assert!(decoded.contains("var ok:Number = 1;\n"));
    assert!(!decoded.contains('\r'));
}
