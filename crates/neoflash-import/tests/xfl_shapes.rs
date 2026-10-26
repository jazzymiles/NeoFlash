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
fn refuses_unrepresentable_strokes_and_partial_stroke_coverage() {
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
        reject(
            &shape(&edge(SQUARE, r#"fillStyle1="1" strokeStyle="1""#), &change),
            "xfl_shape_stroke",
        );
    }
    reject(
        &shape(&edge(SQUARE, r#"fillStyle1="1""#), STROKE),
        "xfl_shape_stroke_topology",
    );
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
fn refuses_unknown_missing_or_unused_paints_and_style_indices() {
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
        &no_stroke(SQUARE).replace(
            "</fills>",
            "<FillStyle index=\"2\"><SolidColor/></FillStyle></fills>",
        ),
        "xfl_shape_fill",
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
}

#[test]
fn rejects_unsupported_edge_syntax_instead_of_skipping_tokens() {
    for (commands, code) in [
        ("!0 0[100 0 100 100", "xfl_shape_curve_unsupported"),
        ("!0 0]100 0 100 100", "xfl_shape_curve_unsupported"),
        ("!0.5 0|100 0", "xfl_shape_coordinate"),
        ("!#A.00 0|100 0", "xfl_shape_coordinate"),
        ("!2147483648 0|100 0", "xfl_shape_coordinate"),
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
fn rejects_open_branching_duplicate_and_multiple_contours() {
    for commands in [
        "!0 0|100 0|100 100|0 100",
        "!0 0|100 0|100 100|0 0!0 0|200 0",
        "!0 0|100 0|100 100|0 0!0 0|100 0",
        "!0 0|0 0|100 0|100 100|0 0",
        "!0 0|100 0|0 100|0 0!200 200|300 200|200 300|200 200",
    ] {
        reject(&no_stroke(commands), "xfl_shape_topology");
    }
}

#[test]
fn rejects_concave_self_intersecting_star_and_collinear_contours() {
    for commands in [
        "!0 0|100 0|50 50|100 100|0 100|0 0",
        "!0 0|100 100|0 100|100 0|0 0",
        "!0 -100|59 81|-95 -31|95 -31|-59 81|0 -100",
        "!0 0|50 0|100 0|100 100|0 100|0 0",
    ] {
        reject(&no_stroke(commands), "xfl_shape_convexity");
    }
}

#[test]
fn validates_xml_semantics_and_resource_limits() {
    reject(
        &no_stroke(SQUARE).replace("<DOMShape>", "<DOMShape arbitrary=\"1\">"),
        "unsupported_attribute",
    );
    reject(
        &no_stroke(SQUARE).replace("<DOMShape>", "<DOMShape isFloating=\"true\">"),
        "xfl_shape_object_mode",
    );
    reject(
        &no_stroke(SQUARE).replace("color=\"#FF0000\"", "color=\"#FF0000\" alpha=\"2\""),
        "invalid_alpha",
    );
    reject(
        &no_stroke(SQUARE).replace("<edges>", "<edges><Unknown/>"),
        "unsupported_element",
    );
    reject(
        &no_stroke(&format!("!0 0{}", "|20 20".repeat(513))),
        "xfl_shape_limit",
    );
    reject(
        &no_stroke(&format!("{}{}", " ".repeat(65_536), SQUARE)),
        "xfl_shape_limit",
    );
}
