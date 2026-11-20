use super::*;
use std::io::Read as _;
use zip::ZipArchive;

fn decode_24_8_ref(token: &str) -> i64 {
    if let Some(hex_body) = token.strip_prefix('#') {
        let (whole_s, frac_s) = hex_body.split_once('.').expect("hex fraction");
        assert!((1..=6).contains(&whole_s.len()));
        assert_eq!(frac_s.len(), 2);
        let raw_whole = u32::from_str_radix(whole_s, 16).unwrap();
        let signed_whole = if raw_whole > 0x7f_ffff {
            i64::from(raw_whole) - 0x100_0000
        } else {
            i64::from(raw_whole)
        };
        let frac = u8::from_str_radix(frac_s, 16).unwrap();
        signed_whole * 256 + i64::from(frac)
    } else {
        token.parse::<i64>().unwrap() * 256
    }
}

#[test]
fn xml_and_cdata_escaping_preserves_content_and_strips_illegal_controls() {
    assert_eq!(
        escape_xml_attr("A&B <\"quote\"> 'single'\n\x00\x07"),
        "A&amp;B &lt;&quot;quote&quot;&gt; &apos;single&apos;&#xA;"
    );
    assert_eq!(
        escape_xml_text("x < y && z > 0\x01"),
        "x &lt; y &amp;&amp; z &gt; 0"
    );
    assert_eq!(
        escape_xfl_characters("Line 1\r\nLine 2\nLine 3 & <more>"),
        "Line 1&#xD;Line 2&#xD;Line 3 &amp; &lt;more&gt;"
    );
    assert_eq!(
        cdata_wrap("trace(\"a]]>b\");\x00"),
        "<![CDATA[trace(\"a]]]]><![CDATA[>b\");]]>"
    );
}

#[test]
fn numeric_formatter_avoids_negative_zero_and_exponents() {
    assert_eq!(fmt_num(0.0), "0");
    assert_eq!(fmt_num(-0.0), "0");
    assert_eq!(fmt_num(-1.0e-12), "0");
    assert_eq!(fmt_num(24.0), "24");
    assert_eq!(fmt_num(-100.0), "-100");
    assert_eq!(fmt_num(0.5), "0.5");
    assert_eq!(fmt_num(1.0e-5), "0.00001");
    assert!(!fmt_num(1.0e-7).contains(['e', 'E']));
    assert_eq!(fmt_num(f64::NAN), "0");
    assert_eq!(fmt_num(f64::INFINITY), "0");
}

#[test]
fn layer_owned_same_frame_scripts_export_to_their_layers_and_legacy_uses_topmost_key() {
    use neoflash_core::FrameScript;

    let mut project = Project::blank();
    project.format_version = 13;
    let timeline = &mut project.symbols[0].timeline;
    timeline.frame_count = 2;
    timeline.layers[0].id = "lower-layer".into();
    timeline.layers[0].name = "Lower".into();
    timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            ..Default::default()
        },
        Keyframe {
            frame: 1,
            ..Default::default()
        },
    ];
    timeline.layers.push(Layer {
        id: "upper-layer".into(),
        name: "Upper".into(),
        visible: true,
        locked: false,
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
        properties: LayerProperties::default(),
    });
    let js = |source: &str| Script {
        language: ScriptLanguage::JavaScript,
        source: source.into(),
    };
    timeline.scripts = vec![
        FrameScript {
            layer: Some("lower-layer".into()),
            frame: 0,
            script: js("trace('owned lower');"),
        },
        FrameScript {
            layer: Some("upper-layer".into()),
            frame: 0,
            script: js("trace('owned upper');"),
        },
        FrameScript {
            layer: None,
            frame: 1,
            script: js("trace('legacy topmost');"),
        },
    ];
    project.validate().unwrap();

    let ctx = ExportContext::build(&project).unwrap();
    let xml = render_dom_document(&project, &ctx);
    let layer_block = |name: &str| {
        let start = xml
            .find(&format!("<DOMLayer name=\"{name}\""))
            .unwrap_or_else(|| panic!("missing XFL layer {name}"));
        let tail = &xml[start..];
        &tail[..tail.find("</DOMLayer>").expect("closed XFL layer")]
    };
    let lower = layer_block("Lower");
    let upper = layer_block("Upper");
    assert!(lower.contains("owned lower"));
    assert!(!lower.contains("owned upper"));
    assert!(!lower.contains("legacy topmost"));
    assert!(upper.contains("owned upper"));
    assert!(!upper.contains("owned lower"));
    assert!(upper.contains("legacy topmost"));
    assert!(!xml.contains("<DOMLayer name=\"Actions\""));

    let output = std::env::temp_dir().join(format!(
        "neoflash-xfl-owned-frame-scripts-{}.fla",
        std::process::id()
    ));
    let _ = fs::remove_file(&output);
    export_fla(&project, &output).unwrap();
    let imported = crate::import_project(&output).unwrap();
    let _ = fs::remove_file(&output);
    let imported_timeline = &imported.symbol(&imported.root).unwrap().timeline;
    let owner_name = |script: &FrameScript| {
        script.layer.as_ref().and_then(|owner| {
            imported_timeline
                .layers
                .iter()
                .find(|layer| &layer.id == owner)
                .map(|layer| layer.name.as_str())
        })
    };
    assert_eq!(imported_timeline.scripts.len(), 3);
    for (source, expected_layer) in [
        ("owned lower", "Lower"),
        ("owned upper", "Upper"),
        ("legacy topmost", "Upper"),
    ] {
        let script = imported_timeline
            .scripts
            .iter()
            .find(|script| script.script.source.contains(source))
            .unwrap_or_else(|| panic!("missing round-tripped script {source}"));
        assert_eq!(owner_name(script), Some(expected_layer));
    }
}

#[test]
fn hidden_owned_frame_script_forces_hidden_layer_publication() {
    use neoflash_core::FrameScript;

    let mut project = Project::blank();
    project.format_version = 13;
    let timeline = &mut project.symbols[0].timeline;
    timeline.layers[0].visible = false;
    let layer_id = timeline.layers[0].id.clone();
    timeline.scripts.push(FrameScript {
        layer: Some(layer_id),
        frame: 0,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "trace('hidden owner');".into(),
        },
    });
    project.validate().unwrap();

    assert!(!can_exclude_hidden_layers(&project));
    let ctx = ExportContext::build(&project).unwrap();
    assert!(ctx.include_hidden_layers);
    assert!(render_publish_settings(&project, &ctx).contains("<InvisibleLayer>1</InvisibleLayer>"));
}

#[test]
fn motion_expression_baking_uses_the_document_frame_rate() {
    use neoflash_core::{PropertyExpression, TweenProperty};

    let element = Element {
        id: "expression-shape".into(),
        transform: Transform::default(),
        kind: ElementKind::Rectangle {
            width: 10.0,
            height: 10.0,
            fill: [255, 0, 0, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        ..Element::default()
    };
    let start = Keyframe {
        frame: 0,
        elements: vec![element.clone()],
        tween: Some(Easing::Linear),
        tween_kind: TweenKind::Motion,
        property_expressions: vec![PropertyExpression {
            property: TweenProperty::X,
            expression: "value + time".into(),
            ..PropertyExpression::default()
        }],
        ..Keyframe::default()
    };
    let end = Keyframe {
        frame: 60,
        elements: vec![element],
        ..Keyframe::default()
    };
    let layer = Layer {
        id: "expression-layer".into(),
        name: "Expression".into(),
        visible: true,
        locked: false,
        keyframes: vec![start, end],
        properties: LayerProperties::default(),
    };
    let timeline = Timeline {
        frame_count: 61,
        layers: vec![layer.clone()],
        scripts: Vec::new(),
        guides: Vec::new(),
    };

    let at_30_fps = expand_motion_spans(&timeline, &layer, 61, 30.0);
    let at_60_fps = expand_motion_spans(&timeline, &layer, 61, 60.0);
    let x_at = |frames: &[Keyframe], frame| {
        frames
            .iter()
            .find(|key| key.frame == frame)
            .unwrap()
            .elements[0]
            .transform
            .x
    };
    assert!((x_at(&at_30_fps, 30) - 1.0).abs() < 1e-9);
    assert!((x_at(&at_60_fps, 30) - 0.5).abs() < 1e-9);
}

#[test]
fn twip_24_8_formatter_round_trips_signed_integers_and_subtwips() {
    assert_eq!(fmt_24_8(0), "0");
    assert_eq!(fmt_24_8(256 * 20), "20");
    assert_eq!(fmt_24_8(-256 * 35), "-35");

    for sample in [
        1_i64,
        5,
        128,
        255,
        257,
        5120 + 64,
        -1,
        -5,
        -128,
        -255,
        -257,
        -5120 - 192,
        i64::from(i32::MAX),
        i64::from(i32::MIN) + 1,
    ] {
        let encoded = fmt_24_8(sample);
        assert_eq!(
            decode_24_8_ref(&encoded),
            sample,
            "failed 24.8 round-trip for {sample} ({encoded})"
        );
    }
}

#[test]
fn cubic_subdivision_converges_to_endpoints() {
    let mut quads = Vec::new();
    cubic_to_quadratics(
        [0.0, 0.0],
        [20.0, 60.0],
        [80.0, -40.0],
        [100.0, 10.0],
        0,
        &mut quads,
    );
    assert!(!quads.is_empty());
    assert!(quads.len() <= 8);
    let last_end = quads.last().unwrap().1;
    assert!((last_end[0] - 100.0).abs() < 1e-9);
    assert!((last_end[1] - 10.0).abs() < 1e-9);
}

#[test]
fn export_fla_bytes_builds_valid_xfl_archive_for_demo_project() {
    let project = Project::demo();
    let bytes = export_fla_bytes(&project).expect("export demo project");
    assert!(bytes.starts_with(b"PK\x03\x04"));

    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("open zip");
    let mut first = archive.by_index(0).expect("first entry");
    assert_eq!(first.name(), "mimetype");
    assert_eq!(first.compression(), CompressionMethod::Stored);
    let mut mime = String::new();
    first.read_to_string(&mut mime).unwrap();
    assert_eq!(mime, "application/vnd.adobe.fla");
    drop(first);

    let mut dom_xml = String::new();
    archive
        .by_name("DOMDocument.xml")
        .expect("DOMDocument.xml present")
        .read_to_string(&mut dom_xml)
        .unwrap();
    assert!(dom_xml.contains("xflVersion=\"2.2\""));
    assert!(dom_xml.contains("creatorInfo=\"NeoFlash\""));
    assert!(dom_xml.contains("versionInfo=\"NeoFlash XFL export 1\""));

    let mut pub_xml = String::new();
    archive
        .by_name("PublishSettings.xml")
        .expect("PublishSettings.xml present")
        .read_to_string(&mut pub_xml)
        .unwrap();
    assert!(pub_xml.contains("<ActionScriptVersion>2</ActionScriptVersion>"));
}

#[test]
fn export_fla_rejects_unrepresentable_media_before_writing_output() {
    let cases: [(&str, &str, &str, &[u8]); 3] = [
        (
            "bad-image",
            "assets/bad-image.png",
            "image/png",
            b"not a PNG",
        ),
        (
            "bad-wav",
            "audio/bad.wav",
            "audio/wav",
            b"RIFF\x04\0\0\0WAVE",
        ),
        (
            "unsupported-ogg",
            "audio/music.ogg",
            "audio/ogg",
            b"OggS\0unsupported",
        ),
    ];

    for (index, (id, resource_path, media_type, bytes)) in cases.into_iter().enumerate() {
        let mut project = Project::blank();
        project.resources.push(
            ResourceUnit::from_bytes(id, resource_path, media_type, bytes)
                .expect("bounded resource"),
        );
        let output = std::env::temp_dir().join(format!(
            "neoflash-xfl-export-invalid-media-{}-{index}.fla",
            std::process::id()
        ));
        let _ = fs::remove_file(&output);

        let error = export_fla(&project, &output).expect_err("media must be rejected");
        let message = error.to_string();
        assert!(message.contains(resource_path), "{message}");
        assert!(message.contains(media_type), "{message}");
        assert!(!output.exists(), "failed export created {output:?}");
    }
}

#[test]
fn authored_media_roles_override_mislabelled_mime_or_file_extensions() {
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        1,
        1,
        image::Rgba([12, 34, 56, 255]),
    ))
    .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
    .unwrap();
    let wav = [
        b'R', b'I', b'F', b'F', 44, 0, 0, 0, b'W', b'A', b'V', b'E', b'f', b'm', b't', b' ', 16, 0,
        0, 0, 1, 0, 1, 0, 0x44, 0xAC, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0, b'd', b'a', b't', b'a',
        8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    let mut project = Project::blank();
    project.format_version = 6;
    project.resources.push(
        ResourceUnit::from_bytes(
            "bitmap-with-audio-mime",
            "assets/picture.wav",
            "audio/wav",
            &png,
        )
        .unwrap(),
    );
    project.resources.push(
        ResourceUnit::from_bytes(
            "audio-with-image-extension",
            "assets/not-really.png",
            "audio/wav",
            &wav,
        )
        .unwrap(),
    );
    let key = &mut project.symbols[0].timeline.layers[0].keyframes[0];
    key.elements.push(Element {
        id: "mislabelled-bitmap".into(),
        name: String::new(),
        transform: Transform::default(),
        kind: ElementKind::Bitmap {
            resource_id: "bitmap-with-audio-mime".into(),
            width: 1,
            height: 1,
            smoothing: true,
        },
        ..Default::default()
    });
    key.sound = Some(FrameSound::new("audio-with-image-extension"));
    project.validate().expect("core-valid authored media roles");

    let bytes = export_fla_bytes(&project).expect("authored roles classify media");
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut dom_xml = String::new();
    archive
        .by_name("DOMDocument.xml")
        .unwrap()
        .read_to_string(&mut dom_xml)
        .unwrap();
    assert!(dom_xml.contains("<DOMBitmapItem name=\"picture.png\""));
    assert!(dom_xml.contains("<DOMSoundItem name=\"not-really.wav\""));
    assert!(archive.by_name("LIBRARY/media/picture.png").is_ok());
    assert!(archive.by_name("LIBRARY/media/not-really.wav").is_ok());
}

#[test]
fn export_fla_preserves_js_ts_scripts_bold_italic_sound_trim_and_instance_filters() {
    use neoflash_core::{
        Accessibility, AccessibleRole, BlendMode, FrameScript, SourceUnit, TextFormat,
    };

    let mut project = Project::blank();
    project.format_version = 7;
    project.sources.push(SourceUnit {
        id: "src-js".into(),
        path: "scripts/app.js".into(),
        language: ScriptLanguage::JavaScript,
        source: "export const speed = 5;".into(),
    });
    project.sources.push(SourceUnit {
        id: "src-ts".into(),
        path: "scripts/helper.ts".into(),
        language: ScriptLanguage::TypeScript,
        source: "export function double(n: number): number { return n * 2; }".into(),
    });
    project.sources.push(SourceUnit {
        id: "src-as1".into(),
        path: "scripts/legacy.as".into(),
        language: ScriptLanguage::ActionScript1,
        source: "tellTarget (\"/hero\") { play(); }".into(),
    });

    let wav_bytes = [
        b'R', b'I', b'F', b'F', 44, 0, 0, 0, b'W', b'A', b'V', b'E', b'f', b'm', b't', b' ', 16, 0,
        0, 0, 1, 0, 1, 0, 0x44, 0xAC, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0, b'd', b'a', b't', b'a',
        8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    project.resources.push(
        ResourceUnit::from_bytes("snd-1", "audio/beep.wav", "audio/wav", &wav_bytes).unwrap(),
    );

    let child_sym_id = "sym-box".to_owned();
    project.symbols.push(Symbol {
        id: child_sym_id.clone(),
        name: "BoxClip".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: "layer-box".into(),
                name: "Art".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "el-rect".into(),
                        name: String::new(),
                        transform: Transform::default(),
                        kind: ElementKind::Rectangle {
                            width: 30.0,
                            height: 20.0,
                            fill: [255, 0, 0, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
            }],
            scripts: vec![],
            guides: vec![],
        },
    });

    let mut sound = FrameSound::new("snd-1");
    sound.in_point = Some(0.25);
    sound.out_point = Some(0.75);

    let fmt = TextFormat {
        font: "Arial".into(),
        size: 18.0,
        bold: true,
        italic: true,
        ..Default::default()
    };

    let expected_filters = vec![
        FilterSpec::Blur {
            blur_x: 6.0,
            blur_y: 4.0,
            quality: 2,
        },
        FilterSpec::Glow {
            color: 0x336699,
            alpha: 0.75,
            blur_x: 8.0,
            blur_y: 8.0,
            strength: 1.2,
            quality: 2,
            inner: true,
            knockout: false,
        },
        FilterSpec::DropShadow {
            distance: 5.0,
            angle: 45.0,
            color: 0x112233,
            alpha: 0.5,
            blur_x: 4.0,
            blur_y: 4.0,
            strength: 0.75,
            quality: 1,
            inner: false,
            knockout: false,
            hide_object: false,
        },
        FilterSpec::Bevel {
            distance: 3.0,
            angle: 30.0,
            highlight_color: 0xFEDCBA,
            highlight_alpha: 0.8,
            shadow_color: 0x123456,
            shadow_alpha: 0.6,
            blur_x: 5.0,
            blur_y: 7.0,
            strength: 1.05,
            quality: 3,
            placement: "full".into(),
            knockout: false,
        },
        FilterSpec::GradientGlow {
            distance: 2.0,
            angle: 15.0,
            colors: vec![0x102030, 0xA0B0C0],
            alphas: vec![0.2, 0.9],
            ratios: vec![0, 255],
            blur_x: 6.0,
            blur_y: 7.0,
            strength: 1.3,
            quality: 2,
            placement: "outer".into(),
            knockout: true,
        },
        FilterSpec::GradientBevel {
            distance: 4.0,
            angle: 60.0,
            colors: vec![0x010203, 0xF0E0D0],
            alphas: vec![0.4, 1.0],
            ratios: vec![32, 224],
            blur_x: 3.0,
            blur_y: 9.0,
            strength: 3.0,
            quality: 1,
            placement: "inner".into(),
            knockout: false,
        },
    ];

    let root_sym = project.symbol_mut(&project.root.clone()).unwrap();
    root_sym.timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::TypeScript,
            source: "const stageReady: boolean = true;".into(),
        },
    });
    root_sym.timeline.layers[0].keyframes[0].sound = Some(sound);
    root_sym.timeline.layers[0].keyframes[0].elements = vec![
        Element {
            id: "el-inst".into(),
            name: "box1".into(),
            transform: Transform::default(),
            kind: ElementKind::Instance {
                symbol_id: child_sym_id.clone(),
                playback: InstancePlayback::Independent,
            },
            visible: false,
            cache_as_bitmap: true,
            blend_mode: BlendMode::Multiply,
            merge: false,
            instance_behavior: None,
            accessibility: Some(Accessibility {
                name: "Hero Box".into(),
                description: "Clickable box".into(),
                role: AccessibleRole::Button,
                tab_order: 3,
                disabled: true,
            }),
            filters: expected_filters.clone(),
            scripts: vec![ObjectScript {
                event: ObjectEvent::Click,
                script: Script {
                    language: ScriptLanguage::JavaScript,
                    source: "this.x += 10;".into(),
                },
            }],
        },
        Element {
            id: "el-wrapped-as2".into(),
            name: "wrappedAs2".into(),
            kind: ElementKind::Instance {
                symbol_id: child_sym_id.clone(),
                playback: InstancePlayback::Independent,
            },
            scripts: vec![ObjectScript {
                event: ObjectEvent::Click,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "on (release) { trace('as2 click'); }".into(),
                },
            }],
            ..Default::default()
        },
        Element {
            id: "el-wrapped-as3".into(),
            name: "wrappedAs3".into(),
            kind: ElementKind::Instance {
                symbol_id: child_sym_id,
                playback: InstancePlayback::Independent,
            },
            scripts: vec![ObjectScript {
                event: ObjectEvent::EnterFrame,
                script: Script {
                    language: ScriptLanguage::ActionScript3,
                    source: "onClipEvent (enterFrame) { trace('as3 frame'); }".into(),
                },
            }],
            ..Default::default()
        },
        Element {
            id: "el-txt".into(),
            name: "label1".into(),
            transform: Transform::default(),
            kind: ElementKind::TextBox {
                text: "Styled Header".into(),
                width: 160.0,
                height: 32.0,
                format: fmt,
                word_wrap: false,
                multiline: false,
            },
            ..Default::default()
        },
    ];
    project.validate().unwrap();

    let temp_path = std::env::temp_dir().join(format!(
        "neoflash-xfl-export-f03-{}.fla",
        std::process::id()
    ));
    export_fla(&project, &temp_path).expect("export fla");

    let mut dom_xml = String::new();
    ZipArchive::new(fs::File::open(&temp_path).unwrap())
        .unwrap()
        .by_name("DOMDocument.xml")
        .unwrap()
        .read_to_string(&mut dom_xml)
        .unwrap();
    assert!(dom_xml.contains("inPoint44=\"11025\""));
    assert!(dom_xml.contains("outPoint44=\"33075\""));
    assert!(dom_xml.contains("isVisible=\"false\""));
    assert!(dom_xml.contains("cacheAsBitmap=\"true\""));
    assert!(dom_xml.contains("blendMode=\"multiply\""));
    assert!(dom_xml.contains("hasAccessibleData=\"true\""));
    assert!(dom_xml.contains("accName=\"Hero Box\""));
    assert!(dom_xml.contains("<BlurFilter blurX=\"6\" blurY=\"4\" quality=\"2\"/>"));
    assert!(dom_xml.contains(
        "<GlowFilter blurX=\"8\" blurY=\"8\" color=\"#336699\" alpha=\"0.75\" strength=\"1.2\""
    ));
    assert!(dom_xml.contains("<DropShadowFilter angle=\"45\" blurX=\"4\" blurY=\"4\" distance=\"5\" color=\"#112233\" alpha=\"0.5\" strength=\"0.75\""));
    assert!(dom_xml.contains("<BevelFilter angle=\"30\" blurX=\"5\" blurY=\"7\" distance=\"3\" highlightColor=\"#FEDCBA\" highlightAlpha=\"0.8\" shadowColor=\"#123456\" shadowAlpha=\"0.6\" strength=\"1.05\""));
    assert!(dom_xml.contains(
        "<GradientGlowFilter angle=\"15\" blurX=\"6\" blurY=\"7\" distance=\"2\" strength=\"1.3\""
    ));
    assert!(dom_xml.contains(
        "<GradientBevelFilter angle=\"60\" blurX=\"3\" blurY=\"9\" distance=\"4\" strength=\"3\""
    ));
    assert!(dom_xml.contains("bold=\"true\""));
    assert!(dom_xml.contains("italic=\"true\""));

    let imported = crate::import_project(&temp_path).expect("reimport exported fla");
    let _ = fs::remove_file(&temp_path);
    imported.validate().unwrap();

    assert!(
        imported
            .sources
            .iter()
            .any(|s| s.language == ScriptLanguage::JavaScript && s.source.contains("speed = 5"))
    );
    assert!(imported.sources.iter().any(
        |s| s.language == ScriptLanguage::TypeScript && s.source.contains("double(n: number)")
    ));
    assert!(
        imported
            .sources
            .iter()
            .any(|s| s.language == ScriptLanguage::ActionScript1
                && s.source == "tellTarget (\"/hero\") { play(); }")
    );
    let imp_root = imported.symbol(&imported.root).unwrap();
    assert_eq!(imp_root.timeline.scripts.len(), 1);
    assert_eq!(
        imp_root.timeline.scripts[0].script.language,
        ScriptLanguage::TypeScript
    );
    assert_eq!(
        imp_root.timeline.scripts[0].script.source,
        "const stageReady: boolean = true;"
    );

    let snd_kf = imp_root
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .find_map(|k| k.sound.as_ref())
        .expect("frame sound preserved");
    assert_eq!(snd_kf.in_point, Some(0.25));
    assert_eq!(snd_kf.out_point, Some(0.75));

    let imp_inst = imp_root
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .find(|e| e.name == "box1")
        .expect("instance preserved");
    assert!(!imp_inst.visible);
    assert!(imp_inst.cache_as_bitmap);
    assert_eq!(imp_inst.blend_mode, BlendMode::Multiply);
    let acc = imp_inst
        .accessibility
        .as_ref()
        .expect("accessibility preserved");
    assert_eq!(acc.name, "Hero Box");
    assert_eq!(acc.description, "Clickable box");
    assert_eq!(acc.tab_order, 3);
    assert!(acc.disabled);
    assert_eq!(imp_inst.filters, expected_filters);
    assert_eq!(imp_inst.scripts.len(), 1);
    assert_eq!(imp_inst.scripts[0].event, ObjectEvent::Click);
    assert_eq!(
        imp_inst.scripts[0].script.language,
        ScriptLanguage::JavaScript
    );
    assert_eq!(imp_inst.scripts[0].script.source, "this.x += 10;");

    for (name, event, language, source) in [
        (
            "wrappedAs2",
            ObjectEvent::Click,
            ScriptLanguage::ActionScript2,
            "on (release) { trace('as2 click'); }",
        ),
        (
            "wrappedAs3",
            ObjectEvent::EnterFrame,
            ScriptLanguage::ActionScript3,
            "onClipEvent (enterFrame) { trace('as3 frame'); }",
        ),
    ] {
        let script = imp_root
            .timeline
            .layers
            .iter()
            .flat_map(|layer| &layer.keyframes)
            .flat_map(|keyframe| &keyframe.elements)
            .find(|element| element.name == name)
            .and_then(|element| element.scripts.first())
            .unwrap_or_else(|| panic!("wrapped handler {name} preserved"));
        assert_eq!(script.event, event);
        assert_eq!(script.script.language, language);
        assert_eq!(script.script.source, source);
    }

    let imp_txt = imp_root
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .find(|e| e.name == "label1")
        .expect("textbox preserved");
    let ElementKind::TextBox { format: tf, .. } = &imp_txt.kind else {
        panic!("expected TextBox");
    };
    assert!(tf.bold);
    assert!(tf.italic);
}

#[test]
fn xfl_roundtrips_instance_behavior_and_frame_label_types() {
    let mut project = Project::blank();
    let mut mc_sym = project.symbols[0].clone();
    mc_sym.id = "mc-lib".into();
    mc_sym.name = "HeroClip".into();
    mc_sym.kind = SymbolKind::MovieClip;
    mc_sym.timeline.layers[0].id = "mc-lib-layer".into();
    mc_sym.timeline.frame_count = 3;

    let mut gfx_sym = mc_sym.clone();
    gfx_sym.id = "gfx-lib".into();
    gfx_sym.name = "BadgeGraphic".into();
    gfx_sym.kind = SymbolKind::Graphic;
    gfx_sym.timeline.layers[0].id = "gfx-lib-layer".into();

    project.symbols.push(mc_sym);
    project.symbols.push(gfx_sym);

    let root_id = project.root.clone();
    let root_sym = project.symbol_mut(&root_id).unwrap();
    root_sym.timeline.frame_count = 3;
    root_sym.timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            label: "author_comment".into(),
            label_type: FrameLabelType::Comment,
            elements: vec![
                Element {
                    id: "inst-gfx-ov".into(),
                    name: String::new(),
                    transform: Transform::default(),
                    kind: ElementKind::Instance {
                        symbol_id: "mc-lib".into(),
                        playback: InstancePlayback::SingleFrame(1),
                    },
                    instance_behavior: Some(SymbolKind::Graphic),
                    ..Default::default()
                },
                Element {
                    id: "inst-btn-ov".into(),
                    name: "btn1".into(),
                    transform: Transform {
                        x: 100.0,
                        ..Default::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: "mc-lib".into(),
                        playback: InstancePlayback::SingleFrame(0),
                    },
                    instance_behavior: Some(SymbolKind::Button),
                    ..Default::default()
                },
                Element {
                    id: "inst-mc-ov".into(),
                    name: "clip1".into(),
                    transform: Transform {
                        x: 200.0,
                        ..Default::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: "gfx-lib".into(),
                        playback: InstancePlayback::Independent,
                    },
                    instance_behavior: Some(SymbolKind::MovieClip),
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
        Keyframe {
            frame: 1,
            label: "deep_link".into(),
            label_type: FrameLabelType::Anchor,
            elements: vec![],
            ..Default::default()
        },
    ];
    project.upgrade_format_for_features();
    project.validate().unwrap();

    let temp_path =
        std::env::temp_dir().join(format!("neoflash-xfl-t360-t362-{}.fla", std::process::id()));
    export_fla(&project, &temp_path).expect("export fla");
    let imported = crate::import_project(&temp_path).expect("reimport exported fla");
    let _ = fs::remove_file(&temp_path);
    imported.validate().unwrap();

    let hero = imported
        .symbols
        .iter()
        .find(|s| s.name == "HeroClip")
        .unwrap();
    assert_eq!(hero.kind, SymbolKind::MovieClip);
    let badge = imported
        .symbols
        .iter()
        .find(|s| s.name == "BadgeGraphic")
        .unwrap();
    assert_eq!(badge.kind, SymbolKind::Graphic);

    let imp_root = imported.symbol(&imported.root).unwrap();
    let all_kfs: Vec<&Keyframe> = imp_root
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .collect();
    assert!(all_kfs.iter().any(|k| {
        k.frame == 0 && k.label == "author_comment" && k.label_type == FrameLabelType::Comment
    }));
    assert!(all_kfs.iter().any(|k| {
        k.frame == 1 && k.label == "deep_link" && k.label_type == FrameLabelType::Anchor
    }));

    let all_els: Vec<&Element> = all_kfs.iter().flat_map(|k| &k.elements).collect();
    let imp_btn = all_els.iter().find(|e| e.name == "btn1").unwrap();
    assert_eq!(imp_btn.instance_behavior, Some(SymbolKind::Button));
    let imp_mc = all_els.iter().find(|e| e.name == "clip1").unwrap();
    assert_eq!(imp_mc.instance_behavior, Some(SymbolKind::MovieClip));
    let imp_gfx = all_els
        .iter()
        .find(|e| e.instance_behavior == Some(SymbolKind::Graphic))
        .expect("graphic override preserved");
    assert!(matches!(
        imp_gfx.kind,
        ElementKind::Instance {
            playback: InstancePlayback::SingleFrame(1),
            ..
        }
    ));
}
