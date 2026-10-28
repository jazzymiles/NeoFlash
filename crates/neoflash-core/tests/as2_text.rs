use neoflash_core::as2_text::{Request, execute};
use neoflash_core::{TextFieldState, TextFormat, layout_text};
use serde_json::{Value, json};
fn call(value: Value) -> Value {
    execute(serde_json::from_value::<Request>(value).unwrap()).unwrap()
}
#[test]
fn stylesheet_uniform_html_and_paragraph_layout_share_one_text_model() {
    let parsed = call(
        json!({"op":"parse_css","state":{"styles":{}},"source":"/* authored CSS */ p, .story { font-family: Arial; font-size: 20px; color: #2266AA; font-weight:bold; text-decoration:underline; margin-left:8; margin-right:6; text-indent:12; }"}),
    );
    assert_eq!(parsed["result"], true);
    let state = &parsed["state"];
    assert_eq!(
        call(json!({"op":"names","state":state}))["result"],
        json!([".story", "p"])
    );
    let style = call(json!({"op":"get_style","state":state,"selector":".story"}))["result"].clone();
    let transformed = call(json!({"op":"transform","style":style}));
    assert_eq!(transformed["result"]["leftMargin"], 8.);
    assert_eq!(transformed["result"]["underline"], true);
    let format = TextFormat::default();
    let html = call(
        json!({"op":"html","source":"<p class=\"story\">Rust &amp; HTML<br>second line</p>","format":format,"styles":state,"condense_white":true}),
    );
    assert_eq!(html["text"], "Rust & HTML\nsecond line\n");
    let field = TextFieldState {
        text: html["text"].as_str().unwrap().into(),
        format: serde_json::from_value(html["format"].clone()).unwrap(),
        multiline: true,
        word_wrap: true,
        ..TextFieldState::default()
    };
    let layout = layout_text(&field, |strings, format| {
        Ok(strings
            .iter()
            .map(|s| s.chars().count() as f64 * format.size / 2.)
            .collect())
    })
    .unwrap();
    assert_eq!(layout.lines[0].x, 22.);
    assert!(!layout.decorations.is_empty());
    assert!(layout.lines.len() > 2);
    let mut viewport_field = field.clone();
    viewport_field.box_size = Some(neoflash_core::TextBoxSize {
        width: 60.,
        height: 30.,
    });
    viewport_field.word_wrap = false;
    viewport_field.as2_viewport.background = true;
    viewport_field.as2_viewport.border = true;
    viewport_field.as2_viewport.scroll = 2;
    viewport_field.as2_viewport.hscroll = 15;
    let view = layout_text(&viewport_field, |strings, format| {
        Ok(strings
            .iter()
            .map(|value| value.chars().count() as f64 * format.size / 2.)
            .collect())
    })
    .unwrap();
    assert_eq!(view.viewport.scroll, 2);
    assert_eq!(view.viewport.hscroll, 15);
    assert!(view.viewport.maxscroll > 1);
    assert!(view.viewport.maxhscroll > 0);
    assert_eq!(view.surface.background, Some(0xffffff));
    assert_eq!(view.surface.border, Some(0));
    let malformed = call(json!({"op":"parse_css","state":state,"source":"p { font-size 20; }"}));
    assert_eq!(malformed["result"], false);
    assert_eq!(malformed["state"], *state);
    let mixed=serde_json::from_value::<Request>(json!({"op":"html","source":"plain <b>bold</b>","format":format,"styles":{"styles":{}},"condense_white":false})).unwrap();
    assert!(
        execute(mixed)
            .unwrap_err()
            .to_string()
            .contains("mixed formatting")
    );
    let canonical = call(json!({"op":"serialize_html","text":"A < B","format":field.format}));
    let roundtrip = call(
        json!({"op":"html","source":canonical["result"],"format":format,"styles":{"styles":{}},"condense_white":false}),
    );
    assert_eq!(roundtrip["text"], "A < B");
    assert_eq!(roundtrip["format"]["leftMargin"], 8.);
    let changed = call(json!({"op":"set_style","state":state,"selector":".story","style":null}));
    assert_eq!(
        call(json!({"op":"names","state":changed["state"]}))["result"],
        json!(["p"])
    );
    assert_eq!(call(json!({"op":"clear"}))["state"], json!({"styles":{}}));
}
