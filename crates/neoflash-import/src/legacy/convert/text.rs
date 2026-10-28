//! Classic static text conversion. Archived names never depend on installed
//! fonts; layout units and enum meanings follow the pinned reader reference.
use crate::ImportReport;
use crate::legacy::timeline::{
    BinaryTimelineStream, TimelineString, TimelineText, TimelineTextStyle,
};
use neoflash_core::{Element, ElementKind, TextAlign, TextBoxSize, TextFormat, Transform};

fn string(stream: &BinaryTimelineStream, value: Option<&TimelineString>) -> Result<String, String> {
    value.map_or_else(
        || Ok(String::new()),
        |value| super::read_string(&stream.original_bytes, value),
    )
}

fn format(
    stream: &BinaryTimelineStream,
    style: &TimelineTextStyle,
) -> Result<(TextFormat, f64), String> {
    if style.revision < 2 {
        return Err(
            "Text has no archived font or size; a revision-only style cannot be reconstructed."
                .into(),
        );
    }
    if style.vertical.unwrap_or(0) != 0 || style.rotation.unwrap_or(0) != 0 {
        return Err("Vertical text or rotated characters require a text-layout adapter.".into());
    }
    if style.character_position.unwrap_or(0) != 0 {
        return Err("Superscript/subscript text requires a styled-run layout adapter.".into());
    }
    let face = string(stream, style.font_face.as_ref())?;
    let family = string(stream, style.font_family.as_ref())?;
    let [red, green, blue, alpha] = style
        .color_rgba
        .ok_or("Text style has no archived color")?
        .to_le_bytes();
    let align =
        match style.alignment.unwrap_or(0) {
            0 => TextAlign::Left,
            1 => TextAlign::Right,
            2 => TextAlign::Center,
            _ => return Err(
                "Justified or unknown text alignment is not supported by the shared text layout."
                    .into(),
            ),
        };
    let twips = |value: Option<i16>| f64::from(value.unwrap_or(0)) / 20.;
    let value = TextFormat {
        font: if face.is_empty() { family } else { face },
        size: f64::from(
            style
                .font_size_twips
                .ok_or("Text style has no archived size")?,
        ) / 20.,
        color: (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue),
        bold: style.bold.unwrap_or(0) != 0,
        italic: style.italic.unwrap_or(0) != 0,
        leading: twips(style.line_spacing),
        align,
        left_margin: twips(style.left_margin),
        right_margin: twips(style.right_margin),
        indent: twips(style.indent),
        letter_spacing: twips(style.letter_spacing),
        kerning: style.auto_kern.map(|value| value != 0),
        url: string(stream, style.url_override.as_ref().or(style.url.as_ref()))?,
        target: string(stream, style.target.as_ref())?,
        ..TextFormat::default()
    };
    value.validate().map_err(|error| error.to_string())?;
    Ok((
        value,
        if style.revision < 4 {
            1.
        } else {
            f64::from(alpha) / 255.
        },
    ))
}

pub(super) fn convert_text(
    stream: &BinaryTimelineStream,
    text: &TimelineText,
    id: &str,
    name: &str,
    report: &mut ImportReport,
) -> Result<Element, String> {
    if text.flags.unwrap_or(0) & 1 != 0 {
        return Err("Dynamic/input text needs authored field-type and binding conversion; its original record is retained.".into());
    }
    if !string(stream, text.variable_name.as_ref())?.is_empty()
        || !string(stream, text.instance_name.as_ref())?.is_empty()
    {
        return Err("Named or variable-bound text needs script-addressable field conversion; its original strings are retained.".into());
    }
    if text
        .accessibility
        .as_ref()
        .is_some_and(|value| value.revision != 0)
    {
        return Err("Text accessibility metadata requires a text accessibility adapter.".into());
    }
    if text.scrollable.unwrap_or(0) != 0 {
        return Err("Authored scrollable text needs viewport-state conversion.".into());
    }
    let style = text
        .runs
        .first()
        .map(|run| &run.style)
        .or(text.default_style.as_ref())
        .ok_or("Text has no run or default style")?;
    let (format, alpha) = format(stream, style)?;
    let mut characters = String::new();
    for run in &text.runs {
        let candidate = self::format(stream, &run.style)?;
        if candidate != (format.clone(), alpha) {
            return Err("Text contains different styled runs; all original runs are retained, but uniform text cannot represent them.".into());
        }
        characters.push_str(&string(stream, Some(&run.characters))?);
    }
    let bounds = text.bounds_words.ok_or("Text has no archived bounds")?;
    let [left, right, top, bottom] = bounds.map(|value| f64::from(value) / 20.);
    let dimensions = TextBoxSize {
        width: right - left,
        height: bottom - top,
    };
    dimensions.validate().map_err(|error| error.to_string())?;
    let words = text.matrix_words.ok_or("Text has no archived matrix")?;
    let [a, b, c, d] =
        [words[0], words[1], words[2], words[3]].map(|value| f64::from(value) / 65536.);
    // The authored text rectangle has a local origin. Compose it into the
    // placement while retaining its exact width/height and both matrix axes.
    let mut transform = Transform::from_matrix([
        a,
        b,
        c,
        d,
        f64::from(words[4]) / 20. + a * left + c * top,
        f64::from(words[5]) / 20. + b * left + d * top,
    ])
    .ok_or("Text matrix is outside supported transform bounds")?;
    transform.alpha = alpha;
    let location = if name.is_empty() { id } else { name };
    if !report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "fla_text_metrics")
    {
        report.warning("fla_text_metrics", location,
            "Text, font names, style, bounds and placement are retained. Shared text layout uses host font metrics and text-box padding, so static glyph placement/antialiasing may differ from Flash.");
    }
    if text
        .runs
        .iter()
        .any(|run| run.style.font_flags.unwrap_or(0) != 0)
    {
        report.warning("fla_text_font_flags", location,
            "Additional archived font flags remain in the embedded source; named font, size, bold and italic are imported.");
    }
    let multiline = characters.contains(['\r', '\n']);
    Ok(Element {
        id: id.into(),
        name: name.into(),
        transform,
        kind: ElementKind::TextBox {
            text: characters,
            width: dimensions.width,
            height: dimensions.height,
            format,
            word_wrap: false,
            multiline,
        },
        scripts: vec![],
        accessibility: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImportFormat, Statistics, TimelineByteSpan, TimelineTextRun};

    fn fixture() -> (BinaryTimelineStream, TimelineText, ImportReport) {
        let mut stream = BinaryTimelineStream {
            index: 0,
            stream_kind: Some(1),
            root: None,
            references: vec![],
            pointers: vec![],
            objects: vec![],
            shapes: vec![],
            original_bytes: vec![],
        };
        let mut append = |text: &str| {
            let start = stream.original_bytes.len();
            stream
                .original_bytes
                .extend(text.encode_utf16().flat_map(u16::to_le_bytes));
            let span = TimelineByteSpan {
                start,
                end: stream.original_bytes.len(),
            };
            TimelineString {
                encoded: span,
                data: span,
                utf16: true,
            }
        };
        let family = append("Archived Uninstalled Font");
        let first = append("Clock ");
        let second = append("Ω");
        let style = TimelineTextStyle {
            revision: 10,
            font_family: Some(family),
            font_size_twips: Some(720),
            color_rgba: Some(0x80663311),
            bold: Some(1),
            italic: Some(1),
            alignment: Some(2),
            line_spacing: Some(40),
            auto_kern: Some(0),
            right_to_left: Some(1),
            vertical: Some(0),
            ..TimelineTextStyle::default()
        };
        let text = TimelineText {
            revision: Some(10),
            matrix_words: Some([0, 65536, -65536, 0, 3495, 4342]),
            bounds_words: Some([332, 3802, 20, 797]),
            flags: Some(0),
            runs: vec![
                TimelineTextRun {
                    characters: first,
                    style: style.clone(),
                },
                TimelineTextRun {
                    characters: second,
                    style,
                },
            ],
            ..TimelineText::default()
        };
        let report = ImportReport {
            source: "synthetic classic text".into(),
            format: ImportFormat::LegacyBinaryFla,
            version: None,
            can_import: false,
            statistics: Statistics::default(),
            diagnostics: vec![],
            binary_fla: None,
        };
        (stream, text, report)
    }

    #[test]
    fn static_text_preserves_uniform_runs_geometry_fonts_and_rejects_semantic_loss() {
        let (stream, text, mut report) = fixture();
        let element = convert_text(&stream, &text, "t", "Clock text", &mut report).unwrap();
        let ElementKind::TextBox {
            text: value,
            width,
            height,
            format,
            word_wrap,
            multiline,
        } = element.kind
        else {
            panic!()
        };
        assert_eq!(value, "Clock Ω");
        assert_eq!(width, 173.5);
        assert!((height - 38.85).abs() < 1e-10);
        assert!((element.transform.x - 173.75).abs() < 1e-10);
        assert!((element.transform.y - 233.7).abs() < 1e-10);
        assert_eq!(element.transform.rotation, 90.);
        assert_eq!(element.transform.scale_x, 1.);
        assert_eq!(element.transform.scale_y, 1.);
        assert_eq!(element.transform.alpha, 128. / 255.);
        assert_eq!(format.font, "Archived Uninstalled Font");
        assert_eq!(format.size, 36.);
        assert_eq!(format.color, 0x113366);
        assert!(format.bold && format.italic);
        assert_eq!(format.align, TextAlign::Center);
        assert_eq!(format.leading, 2.);
        assert!(!word_wrap && !multiline);
        convert_text(&stream, &text, "t2", "More text", &mut report).unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].code, "fla_text_metrics");

        for variation in 0..5 {
            let mut changed = text.clone();
            match variation {
                0 => changed.runs[1].style.bold = Some(0),
                1 => changed.runs[0].style.alignment = Some(3),
                2 => changed.runs[0].style.vertical = Some(1),
                3 => changed.variable_name = Some(changed.runs[0].characters.clone()),
                _ => changed.flags = Some(1),
            }
            assert!(convert_text(&stream, &changed, "unsupported", "Text", &mut report).is_err());
        }
    }
}
