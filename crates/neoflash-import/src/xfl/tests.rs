use super::*;

#[test]
fn marked_xfl_strength_is_direct_and_unmarked_neoflash_export_stays_compatible() {
    let legacy =
        Document::parse(r#"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" xflVersion="2.2"/>"#)
            .unwrap();
    let adobe = Document::parse(
            r#"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" xflVersion="2.2" creatorInfo="Adobe Flash Professional CS5"/>"#,
        )
        .unwrap();
    let neoflash = Document::parse(
            r#"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" xflVersion="2.2" creatorInfo="NeoFlash" versionInfo="NeoFlash XFL export 1"/>"#,
        )
        .unwrap();

    assert!(!uses_direct_filter_strength(legacy.root_element()));
    assert!(uses_direct_filter_strength(adobe.root_element()));
    assert!(uses_direct_filter_strength(neoflash.root_element()));
    assert_eq!(normalize_filter_strength(150.0, false), 1.5);
    assert_eq!(normalize_filter_strength(1.2, true), 1.2);
    assert_eq!(normalize_filter_strength(30_000.0, false), 255.0);
}

#[test]
fn xfl_disabled_instance_filters_remain_in_the_authored_stack() {
    let mut files = BTreeMap::new();
    files.insert(
            "LIBRARY/Child.xml".into(),
            r#"<DOMSymbolItem xmlns="http://ns.adobe.com/xfl/2008/" name="Child">
  <timeline><DOMTimeline name="Child"><layers><DOMLayer name="Layer 1"><frames>
    <DOMFrame index="0"><elements><DOMRectangleObject objectWidth="10" objectHeight="10"/></elements></DOMFrame>
  </frames></DOMLayer></layers></DOMTimeline></timeline>
</DOMSymbolItem>"#
                .into(),
        );
    files.insert(
            "DOMDocument.xml".into(),
            r#"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" xflVersion="2.97" creatorInfo="NeoFlash">
  <symbols><Include href="Child.xml"/></symbols>
  <timelines><DOMTimeline name="Scene 1"><layers><DOMLayer name="Artwork"><frames>
    <DOMFrame index="0"><elements>
      <DOMSymbolInstance libraryItemName="Child" name="filtered">
        <filters>
          <BlurFilter blurX="12" blurY="8" quality="3" isEnabled="false"/>
          <AdjustColorFilter brightness="10" contrast="-20" saturation="30" hue="40" isEnabled="false"/>
        </filters>
      </DOMSymbolInstance>
    </elements></DOMFrame>
  </frames></DOMLayer></layers></DOMTimeline></timelines>
</DOMDocument>"#
                .into(),
        );
    let mut report = ImportReport {
        source: "xfl-disabled-filters".into(),
        format: crate::ImportFormat::XflDirectory,
        version: None,
        can_import: false,
        statistics: crate::Statistics::default(),
        diagnostics: vec![],
        binary_fla: None,
        swf: None,
        swc: None,
    };

    let project = convert(&files, &BTreeMap::new(), &mut report).expect("XFL converts");
    project.validate().expect("disabled filters validate");
    let root = project.symbol(&project.root).expect("root symbol");
    let element = &root.timeline.layers[0].keyframes[0].elements[0];
    assert_eq!(element.filters.len(), 2);
    assert!(element.filters.iter().all(|filter| !filter.is_enabled()));
    assert_eq!(element.filters[0].quality_level(), 3);
    assert!(matches!(element.filters[1], FilterSpec::ColorMatrix { .. }));
    assert!(
        !report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "xfl_filters_approximated" && diagnostic.message.contains("omitted")
        }),
        "disabled filters are authored state, not omitted content: {:?}",
        report.diagnostics
    );
}

#[test]
fn xfl_morph_hint_parser_bounds_retained_records_and_reports_approximation() {
    let items = (0..=ShapeHint::MAX_HINTS)
        .map(|index| {
            format!(r#"<MorphHint startU="0" startV="0" endU="1" endV="1" key="{index}"/>"#)
        })
        .collect::<String>();
    let xml = format!("<DOMFrame><morphHintsList>{items}</morphHintsList></DOMFrame>");
    let document = Document::parse(&xml).expect("bounded hint fixture parses");

    let (hints, had_unconverted) = parse_xfl_frame_morph_hints(document.root_element());
    assert_eq!(hints.len(), ShapeHint::MAX_HINTS);
    assert!(
        had_unconverted,
        "the excess hint must not be dropped silently"
    );
}

#[test]
fn xfl_morph_hints_list_populates_keyframe_shape_hints() {
    let mut files = BTreeMap::new();
    files.insert(
        "DOMDocument.xml".into(),
        r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" xflVersion="2.2">
  <timelines>
    <DOMTimeline name="Scene 1">
      <layers>
        <DOMLayer name="Morph">
          <frames>
            <DOMFrame index="0" duration="10" tweenType="shape">
              <MorphShape>
                <morphHintsList>
                  <MorphHint startPoint="#0.00, #0.00" endPoint="#7.D0, #0.00" key="0"/>
                  <MorphHint startPoint="100, 0" endPoint="100, 100" key="1"/>
                </morphHintsList>
              </MorphShape>
              <elements>
                <DOMRectangleObject objectWidth="100" objectHeight="100"/>
              </elements>
            </DOMFrame>
            <DOMFrame index="10" duration="1">
              <elements>
                <DOMRectangleObject objectWidth="100" objectHeight="100"/>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timelines>
</DOMDocument>"##
            .into(),
    );
    let mut report = ImportReport {
        source: "xfl-morph-hints".into(),
        format: crate::ImportFormat::XflDirectory,
        version: Some("2.2".into()),
        can_import: false,
        statistics: crate::Statistics::default(),
        diagnostics: vec![],
        binary_fla: None,
        swf: None,
        swc: None,
    };
    let project = convert(&files, &BTreeMap::new(), &mut report).expect("XFL converts");
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.severity == crate::Severity::Error),
        "{:?}",
        report.diagnostics
    );
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.code == "xfl_morph_hints_ignored"),
        "valid morph hints should not emit xfl_morph_hints_ignored: {:?}",
        report.diagnostics
    );
    project.validate().expect("project validates");
    assert_eq!(project.format_version, 7);

    let root = project.symbol(&project.root).unwrap();
    let k0 = &root.timeline.layers[0].keyframes[0];
    assert_eq!(k0.tween_kind, TweenKind::Shape);
    assert_eq!(k0.shape_hints.len(), 2);
    assert_eq!(k0.shape_hints[0].label, 'a');
    assert!((k0.shape_hints[0].start_uv[0] - 0.0).abs() < 1e-4);
    assert!((k0.shape_hints[0].start_uv[1] - 0.0).abs() < 1e-4);
    assert!((k0.shape_hints[0].end_uv[0] - 1.0).abs() < 1e-4);
    assert!((k0.shape_hints[0].end_uv[1] - 0.0).abs() < 1e-4);

    assert_eq!(k0.shape_hints[1].label, 'b');
    assert!((k0.shape_hints[1].start_uv[0] - 1.0).abs() < 1e-4);
    assert!((k0.shape_hints[1].start_uv[1] - 0.0).abs() < 1e-4);
    assert!((k0.shape_hints[1].end_uv[0] - 1.0).abs() < 1e-4);
    assert!((k0.shape_hints[1].end_uv[1] - 1.0).abs() < 1e-4);
}
