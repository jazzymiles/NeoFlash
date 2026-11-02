use neoflash_core::{ElementKind, FillRule, PathCommand, evaluate};
use neoflash_import::{Severity, import_project, inspect};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Source(PathBuf);
impl Source {
    fn new(shape: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "neoflash-xfl-shape-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("DOMDocument.xml"), format!(r#"<DOMDocument width="120" height="120" frameRate="24"><timelines><DOMTimeline><layers><DOMLayer><frames><DOMFrame index="0"><elements>{shape}</elements></DOMFrame></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#)).unwrap();
        Self(path)
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const SQUARE: &str = "!425 -425|425 425!425 425|-425 425!-425 425|-425 -425!-425 -425|425 -425";
const SELECTED_SQUARE: &str =
    "!425 425S6|-425 425!-425 425|-425 -425!-425 -425|425 -425!425 -425|425 425";
const STROKE: &str = r#"<strokes><StrokeStyle index="1"><SolidStroke scaleMode="normal"><fill><SolidColor/></fill></SolidStroke></StrokeStyle></strokes>"#;

fn shape(edges: &str, strokes: &str) -> String {
    format!(
        r##"<DOMShape><fills><FillStyle index="1"><SolidColor color="#FF0000"/></FillStyle></fills>{strokes}<edges>{edges}</edges></DOMShape>"##
    )
}
fn edge(commands: &str, attrs: &str) -> String {
    format!(r#"<Edge {attrs} edges="{commands}"/>"#)
}
fn no_stroke(commands: &str) -> String {
    shape(&edge(commands, r#"fillStyle1="1""#), "")
}
fn reject(xml: &str, code: &str) {
    let source = Source::new(xml);
    let report = inspect(&source.0).unwrap();
    assert!(!report.can_import, "unexpected accepted shape: {xml}");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.code == code),
        "wanted {code}; got {:?}",
        report.diagnostics
    );
    assert!(
        import_project(&source.0).is_err(),
        "a blocked shape must not become a partial project"
    );
}

#[test]
fn preserves_original_square_geometry_default_stroke_and_selection_metadata() {
    for commands in [SQUARE, SELECTED_SQUARE] {
        let source = Source::new(&shape(
            &edge(commands, r#"fillStyle1="1" strokeStyle="1""#),
            STROKE,
        ));
        let report = inspect(&source.0).unwrap();
        assert!(report.can_import, "{:?}", report.diagnostics);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "xfl_shape_selection"),
            commands == SELECTED_SQUARE
        );
        let project = import_project(&source.0).unwrap();
        let items = evaluate(&project, &project.root, 0);
        assert_eq!(items.len(), 1);
        let ElementKind::Path {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
        } = &items[0].kind
        else {
            panic!("not a vector path")
        };
        assert_eq!(*fill, [255, 0, 0, 255]);
        assert_eq!(*stroke, [0, 0, 0, 255]);
        assert_eq!(*stroke_width, 1.);
        assert_eq!(*fill_rule, FillRule::NonZero);
        assert_eq!(commands.len(), 5);
        assert_eq!(commands.last(), Some(&PathCommand::Close));
        let bounds = neoflash_core::path_bounds(commands).unwrap().unwrap();
        assert_eq!(
            (bounds.x_min, bounds.y_min, bounds.x_max, bounds.y_max),
            (-21.25, -21.25, 21.25, 21.25)
        );
    }
}

#[test]
fn stitches_exact_endpoints_across_shuffled_edges_and_normalizes_fill_side() {
    let edges = [
        edge("!0 0|400 0", r#"fillStyle1="1""#),
        edge("!400 400|0 400", r#"fillStyle1="1""#),
        edge("!400 400/400 0", r#"fillStyle0="1""#),
        edge("!0 400|0 0", r#"fillStyle1="1""#),
    ]
    .join("");
    let source = Source::new(&shape(&edges, ""));
    let project = import_project(&source.0).unwrap();
    let items = evaluate(&project, &project.root, 0);
    let ElementKind::Path {
        commands,
        stroke_width,
        ..
    } = &items[0].kind
    else {
        panic!("not path")
    };
    assert_eq!(
        commands,
        &vec![
            PathCommand::MoveTo { x: 0., y: 0. },
            PathCommand::LineTo { x: 20., y: 0. },
            PathCommand::LineTo { x: 20., y: 20. },
            PathCommand::LineTo { x: 0., y: 20. },
            PathCommand::Close
        ]
    );
    assert_eq!(*stroke_width, 0.);
}

#[test]
fn convex_triangle_is_general_and_shape_matrix_is_retained() {
    let xml = no_stroke("!-200 -100|200 -100|0 300|-200 -100").replace(
        "<DOMShape>",
        r#"<DOMShape name="triangle"><matrix><Matrix tx="35" ty="20"/></matrix>"#,
    );
    let source = Source::new(&xml);
    let project = import_project(&source.0).unwrap();
    let items = evaluate(&project, &project.root, 0);
    assert_eq!(items.len(), 1);
    assert_eq!((items[0].transform[4], items[0].transform[5]), (35., 20.));
    let ElementKind::Path { commands, .. } = &items[0].kind else {
        panic!("not path")
    };
    assert_eq!(commands.len(), 4);
    assert_eq!(commands[0], PathCommand::MoveTo { x: -10., y: -5. });
}

#[test]
fn imports_hairline_non_round_and_partial_strokes_and_refuses_invalid_strokes() {
    for change in [
        STROKE.replace("normal", "none"),
        STROKE.replace(" scaleMode=\"normal\"", ""),
        STROKE.replace("scaleMode=\"normal\"", "scaleMode=\"normal\" weight=\"0\""),
        STROKE.replace(
            "scaleMode=\"normal\"",
            "scaleMode=\"normal\" caps=\"square\"",
        ),
        STROKE.replace(
            "scaleMode=\"normal\"",
            "scaleMode=\"normal\" joints=\"miter\"",
        ),
        STROKE.replace(
            "scaleMode=\"normal\"",
            "scaleMode=\"normal\" pixelHinting=\"true\"",
        ),
        STROKE.replace(
            "scaleMode=\"normal\"",
            "scaleMode=\"normal\" sharpCorners=\"true\"",
        ),
    ] {
        let source = Source::new(&shape(
            &edge(SQUARE, r#"fillStyle1="1" strokeStyle="1""#),
            &change,
        ));
        let report = inspect(&source.0).unwrap();
        assert!(report.can_import, "{:?}", report.diagnostics);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Warning && d.code == "xfl_stroke_rendering"),
            "expected xfl_stroke_rendering warning for {change}; got {:?}",
            report.diagnostics
        );
        let project = import_project(&source.0).unwrap();
        project.validate().unwrap();
        let items = evaluate(&project, &project.root, 0);
        assert_eq!(items.len(), 2);
        assert!(matches!(
            &items[1].kind,
            ElementKind::PaintedPath { stroke_width, .. } if *stroke_width == 1.0
        ));
    }

    // Partial stroke coverage (only two sides of the filled square are stroked).
    let partial_edges = [
        edge("!0 0|400 0", r#"fillStyle1="1" strokeStyle="1""#),
        edge("!400 0|400 400|0 400|0 0", r#"fillStyle1="1""#),
    ]
    .join("");
    let partial_source = Source::new(&shape(&partial_edges, STROKE));
    let partial_project = import_project(&partial_source.0).unwrap();
    partial_project.validate().unwrap();
    assert_eq!(
        evaluate(&partial_project, &partial_project.root, 0).len(),
        2
    );

    // Invalid stroke parameters and dangling stroke style references are still refused.
    for bad in [
        STROKE.replace("scaleMode=\"normal\"", "scaleMode=\"normal\" weight=\"-1\""),
        STROKE.replace("scaleMode=\"normal\"", "scaleMode=\"normal\" caps=\"butt\""),
        STROKE.replace(
            "scaleMode=\"normal\"",
            "scaleMode=\"normal\" joints=\"sharp\"",
        ),
    ] {
        reject(
            &shape(&edge(SQUARE, r#"fillStyle1="1" strokeStyle="1""#), &bad),
            "xfl_shape_stroke",
        );
    }
    reject(
        &shape(&edge(SQUARE, r#"fillStyle1="1" strokeStyle="2""#), STROKE),
        "xfl_shape_stroke_topology",
    );
    reject(
        &shape(&edge(SQUARE, r#"fillStyle1="1" strokeStyle="1""#), ""),
        "xfl_shape_stroke_topology",
    );
}

#[test]
fn refuses_unknown_missing_paints_and_style_indices_and_allows_unused_declared_styles() {
    for attrs in ["", r#"fillStyle1="2""#, r#"fillStyle0="1" fillStyle1="1""#] {
        reject(
            &shape(&edge(SQUARE, attrs), ""),
            "xfl_shape_style_reference",
        );
    }
    for attrs in [
        r#"fillStyle1="-1""#,
        r#"fillStyle1="1.0""#,
        r#"fillStyle1="4294967296""#,
    ] {
        reject(&shape(&edge(SQUARE, attrs), ""), "xfl_shape_style_index");
    }
    reject(
        &no_stroke(SQUARE).replace("index=\"1\"", "index=\"0\""),
        "xfl_shape_style_index",
    );
    reject(
        &no_stroke(SQUARE).replace("<SolidColor color=\"#FF0000\"/>", "<LinearGradient/>"),
        "xfl_shape_paint",
    );
    reject(
        &shape(
            &edge(SQUARE, r#"fillStyle1="1" strokeStyle="1""#),
            &STROKE.replace(
                "</strokes>",
                "<StrokeStyle index=\"2\"><SolidStroke/></StrokeStyle></strokes>",
            ),
        ),
        "xfl_shape_stroke",
    );

    // Unused declared fill styles in <fills> are harmless in authored XFL documents.
    let unused_fill = Source::new(&no_stroke(SQUARE).replace(
        "</fills>",
        "<FillStyle index=\"2\"><SolidColor color=\"#00FF00\"/></FillStyle></fills>",
    ));
    let project = import_project(&unused_fill.0).unwrap();
    project.validate().unwrap();
    assert_eq!(evaluate(&project, &project.root, 0).len(), 1);
}

#[test]
fn parses_curves_decimal_and_hex_twips_and_rejects_malformed_edge_syntax() {
    // Closed quadratic curves ([ and ]), decimal fractions, 24.8 signed hex twips, and cubics hints.
    let curved = Source::new(
        r##"<DOMShape><fills><FillStyle index="1"><SolidColor color="#FF0000"/></FillStyle></fills><edges><Edge fillStyle1="1" edges="!0.5 -0.5[100 0 #C8.00 #64.00]#FFFFFF.80 200 0.5 -0.5"/><Edge cubics="!0 0(;0,0 100,0 200,100)q0 0 200 100;"/></edges></DOMShape>"##,
    );
    let report = inspect(&curved.0).unwrap();
    assert!(report.can_import, "{:?}", report.diagnostics);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning && d.code == "xfl_cubic_editing_metadata")
    );
    let project = import_project(&curved.0).unwrap();
    project.validate().unwrap();
    let items = evaluate(&project, &project.root, 0);
    assert_eq!(items.len(), 1);
    let ElementKind::PaintedPath { commands, .. } = &items[0].kind else {
        panic!("expected PaintedPath")
    };
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, PathCommand::QuadraticTo { .. }))
    );

    for (commands, code) in [
        ("!2147483648 0|100 0", "xfl_shape_coordinate"),
        ("!#1234567.00 0|100 0", "xfl_shape_coordinate"),
        ("!#A.123 0|100 0", "xfl_shape_coordinate"),
        ("!1e2 0|100 0", "xfl_shape_coordinate"),
        ("!0 0S8|100 0", "xfl_shape_selection"),
        ("!0 0S12|100 0", "xfl_shape_selection"),
        ("|100 0", "xfl_shape_edges"),
        ("!0 0!10 10|20 20", "xfl_shape_edges"),
        ("!0 0|100 0!0 0", "xfl_shape_edges"),
        ("!0 0|100 0Z", "xfl_shape_edge_syntax"),
        ("!0 0|100", "xfl_shape_coordinate"),
    ] {
        reject(&no_stroke(commands), code);
    }
}

#[test]
fn rejects_open_branching_and_duplicate_fill_contours_and_imports_islands_and_holes() {
    for commands in [
        "!0 0|100 0|100 100|0 100",
        "!0 0[100 0 100 100",
        "!0 0|100 0|100 100|0 0!0 0|200 0",
        "!0 0|100 0|100 100|0 0!0 0|100 0",
        "!0 0|0 0",
    ] {
        reject(&no_stroke(commands), "xfl_shape_topology");
    }

    // Zero-length segments inside an otherwise valid closed fill contour are ignored, and
    // disjoint islands + interior holes (with interior bridge seams) assemble cleanly.
    let donut_edges = [
        edge("!0 0|0 0|400 0|400 400|0 400|0 0", r#"fillStyle1="1""#),
        edge(
            "!100 100|100 300|300 300|300 100|100 100",
            r#"fillStyle1="1""#,
        ),
        edge("!0 0|100 100", r#"fillStyle0="1" fillStyle1="1""#),
        edge("!500 0|600 0|500 100|500 0", r#"fillStyle1="1""#),
    ]
    .join("");
    let donut_source = Source::new(&shape(&donut_edges, ""));
    let project = import_project(&donut_source.0).unwrap();
    project.validate().unwrap();
    let items = evaluate(&project, &project.root, 0);
    assert_eq!(items.len(), 1);
    let ElementKind::PaintedPath { commands, .. } = &items[0].kind else {
        panic!("expected PaintedPath")
    };
    let closes = commands
        .iter()
        .filter(|c| matches!(c, PathCommand::Close))
        .count();
    assert_eq!(closes, 3);
}

#[test]
fn imports_concave_self_intersecting_star_and_collinear_contours() {
    for commands in [
        "!0 0|100 0|50 50|100 100|0 100|0 0",
        "!0 0|100 100|0 100|100 0|0 0",
        "!0 -100|59 81|-95 -31|95 -31|-59 81|0 -100",
        "!0 0|50 0|100 0|100 100|0 100|0 0",
    ] {
        let source = Source::new(&no_stroke(commands));
        let report = inspect(&source.0).unwrap();
        assert!(report.can_import, "{:?}", report.diagnostics);
        let project = import_project(&source.0).unwrap();
        project.validate().unwrap();
        let items = evaluate(&project, &project.root, 0);
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0].kind, ElementKind::PaintedPath { .. }));
    }
}

#[test]
fn validates_xml_semantics_and_resource_limits() {
    reject(
        &no_stroke(SQUARE).replace("<DOMShape>", "<DOMShape arbitrary=\"1\">"),
        "unsupported_attribute",
    );
    let floating =
        Source::new(&no_stroke(SQUARE).replace("<DOMShape>", "<DOMShape isFloating=\"true\">"));
    let floating_report = inspect(&floating.0).unwrap();
    assert!(
        floating_report.can_import,
        "{:?}",
        floating_report.diagnostics
    );
    assert!(
        floating_report
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning && d.code == "xfl_shape_object_mode")
    );
    reject(
        &no_stroke(SQUARE).replace("color=\"#FF0000\"", "color=\"#FF0000\" alpha=\"2\""),
        "invalid_alpha",
    );
    reject(
        &no_stroke(SQUARE).replace("<edges>", "<edges><Unknown/>"),
        "unsupported_element",
    );
    let half = format!("!0 0{}", "|20 20".repeat(8_200));
    reject(
        &shape(
            &format!(
                "{}{}",
                edge(&half, r#"fillStyle1="1""#),
                edge(&half, r#"fillStyle1="1""#),
            ),
            "",
        ),
        "xfl_shape_limit",
    );
    reject(
        &no_stroke(&format!("{}{}", " ".repeat(65_536), SQUARE)),
        "xfl_shape_limit",
    );
}
