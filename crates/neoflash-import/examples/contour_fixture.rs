//! Original synthetic binary shape records -> adapter -> native/web Project.
//! This is not a binary FLA document converter and never reads user artwork.
use neoflash_core::*;
use neoflash_import::*;
use std::{fs::OpenOptions, io::Write};

#[derive(Clone, Copy)]
struct Segment {
    start: [i32; 2],
    control: Option<[i32; 2]>,
    end: [i32; 2],
}
fn rectangle(size: i32) -> Vec<Segment> {
    let points = [[0, 0], [size, 0], [size, size], [0, size]];
    (0..4)
        .map(|i| Segment {
            start: points[i],
            control: None,
            end: points[(i + 1) % 4],
        })
        .collect()
}
fn converted(
    segments: &[Segment],
    matrix: [i32; 6],
) -> Result<ElementKind, Box<dyn std::error::Error>> {
    let mut bytes = vec![2];
    for word in matrix {
        bytes.extend(word.to_le_bytes());
    }
    bytes.push(4);
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend([31, 97, 211, 255, 0, 0]);
    bytes.extend(0u16.to_le_bytes());
    let mut previous = [0; 2];
    for segment in segments {
        let start = segment.start.map(|v| v * 5120);
        let end = segment.end.map(|v| v * 5120);
        bytes.push(if segment.control.is_some() {
            0x6a
        } else {
            0x62
        });
        for value in [0u16, 0, 1] {
            bytes.extend(value.to_le_bytes());
        }
        for v in [start[0] - previous[0], start[1] - previous[1]] {
            bytes.extend(v.to_le_bytes());
        }
        if let Some(control) = segment.control.map(|p| p.map(|v| v * 5120)) {
            for v in [control[0] - start[0], control[1] - start[1]] {
                bytes.extend(v.to_le_bytes());
            }
        }
        for v in [end[0] - start[0], end[1] - start[1]] {
            bytes.extend(v.to_le_bytes());
        }
        previous = end;
    }
    bytes.push(0);
    Ok(adapt_binary_solid_shape(
        &decode_binary_shape(&bytes)?,
        BinaryContourContext {
            contents_revision: 0x14,
        },
    )?
    .path)
}
fn element(name: &str, kind: ElementKind, x: f64, y: f64) -> Element {
    Element {
        id: new_id("fixture"),
        name: name.into(),
        kind,
        transform: Transform {
            x,
            y,
            ..Default::default()
        },
        scripts: vec![],
        accessibility: None,
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .ok_or("provide a new output Project JSON path")?;
    let mut project = Project::blank();
    project.name = "Original binary contour adapter fixtures".into();
    project.width = 640;
    project.height = 260;
    project.background = [255; 4];
    project.symbols[0].timeline.frame_count = 1;
    let mut holes = rectangle(120);
    for mut edge in rectangle(50) {
        std::mem::swap(&mut edge.start, &mut edge.end);
        edge.start = edge.start.map(|v| v + 35);
        edge.end = edge.end.map(|v| v + 35);
        holes.push(edge);
    }
    let lens = [
        Segment {
            start: [0, 50],
            control: Some([60, -30]),
            end: [120, 50],
        },
        Segment {
            start: [120, 50],
            control: Some([60, 130]),
            end: [0, 50],
        },
    ];
    let elements = &mut project.symbols[0].timeline.layers[0].keyframes[0].elements;
    for (name, kind, x, y) in [
        (
            "Directed hole",
            converted(&holes, [65536, 0, 0, 65536, 0, 0])?,
            35.,
            85.,
        ),
        (
            "Quadratic lens",
            converted(&lens, [65536, 0, 0, 65536, 0, 0])?,
            230.,
            80.,
        ),
        (
            "Affine fill",
            converted(&rectangle(100), [-65536, 0, 32768, 65536, 2000, 0])?,
            440.,
            80.,
        ),
    ] {
        elements.push(element(name, kind, x, y));
    }
    for (text, x, y, size) in [
        ("Original F3 shape adapter fixtures", 28., 18., 22.),
        ("Directed contours", 35., 55., 14.),
        ("Quadratic lens", 230., 55., 14.),
        ("Reflected + sheared", 440., 55., 14.),
    ] {
        elements.push(element(
            text,
            ElementKind::Text {
                text: text.into(),
                font_family: "Arial".into(),
                font_size: size,
                color: [40, 48, 65, 255],
            },
            x,
            y,
        ));
    }
    let json = project.to_json()?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?
        .write_all(json.as_bytes())?;
    println!("Wrote original synthetic adapter fixture: 3 Path elements; no private source data.");
    Ok(())
}
