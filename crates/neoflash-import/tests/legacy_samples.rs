//! Opt-in checks against local, user-supplied FLA files. No artwork, private
//! filenames or source scripts are embedded in this test. Set
//! NEOFLASH_LEGACY_SAMPLE_DIR to point at a local binary FLA corpus directory.
use neoflash_core::*;
use neoflash_import::*;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn sample_root() -> PathBuf {
    std::env::var_os("NEOFLASH_LEGACY_SAMPLE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/old flash files")
        })
}

fn collect_fla_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_fla_files(&path, out);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("fla"))
        {
            out.push(path);
        }
    }
}

fn find_sample_by_profile(
    files: &[PathBuf],
    dimensions: (u32, u32, u32),
    timeline_streams: usize,
) -> PathBuf {
    for path in files {
        let Ok(doc) = read_binary_contents(path) else {
            continue;
        };
        if doc.contents.stop.is_some() {
            continue;
        }
        let Some(settings) = &doc.contents.settings else {
            continue;
        };
        let Some([width_twips, height_twips]) = settings.size_twips() else {
            continue;
        };
        let dims = (
            u32::try_from(width_twips / 20).unwrap_or(0),
            u32::try_from(height_twips / 20).unwrap_or(0),
            settings.frame_rate().round() as u32,
        );
        let stream_count = doc
            .inspection
            .streams
            .iter()
            .filter(|s| matches!(s.role, BinaryStreamRole::Scene | BinaryStreamRole::Symbol))
            .count();
        if dims == dimensions && stream_count == timeline_streams {
            return path.clone();
        }
    }
    panic!(
        "Required local binary FLA fixture matching {:?} with {} timeline streams was not found under sample directory",
        dimensions, timeline_streams
    );
}

fn string(bytes: &[u8], value: &TimelineString) -> String {
    let bytes = &bytes[value.data.start..value.data.end];
    if value.utf16 {
        assert_eq!(bytes.len() % 2, 0);
        String::from_utf16(
            &bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .unwrap()
    } else {
        assert!(bytes.is_ascii());
        String::from_utf8(bytes.to_vec()).unwrap()
    }
}

fn object(stream: &BinaryTimelineStream, id: u32) -> &TimelineObject {
    stream
        .objects
        .iter()
        .find(|object| object.object_id == id)
        .unwrap()
}

fn authored_scripts(project: &Project) -> Vec<&Script> {
    project
        .symbols
        .iter()
        .flat_map(|symbol| {
            symbol
                .timeline
                .scripts
                .iter()
                .map(|script| &script.script)
                .chain(
                    symbol
                        .timeline
                        .layers
                        .iter()
                        .flat_map(|layer| layer.keyframes.iter())
                        .flat_map(|key| &key.elements)
                        .flat_map(|element| &element.scripts)
                        .map(|script| &script.script),
                )
        })
        .collect()
}

fn source_scripts(document: &BinaryTimelineDocument) -> Vec<(String, bool, usize)> {
    document
        .streams
        .iter()
        .flat_map(|stream| {
            stream.objects.iter().filter_map(move |object| {
                let action = match &object.payload {
                    Some(TimelinePayload::Frame(frame)) => frame.actions.as_ref(),
                    Some(TimelinePayload::Instance(instance)) => instance.actions.as_ref(),
                    _ => None,
                }?;
                if action.presence != TimelineActionPresence::Source {
                    return None;
                }
                assert!(action.complete);
                let source = action.source.as_ref().unwrap();
                Some((
                    string(&stream.original_bytes, source),
                    source.utf16,
                    source.data.end - source.data.start,
                ))
            })
        })
        .collect()
}

fn check_sample(
    path: &Path,
    dimensions: (u32, u32, u32),
    expected: (usize, usize, usize),
) -> (BinaryTimelineDocument, Project) {
    assert!(path.is_file(), "Local binary FLA sample file is required");
    let original = fs::read(path).unwrap();
    let document = read_binary_timeline(path).unwrap();
    assert_eq!(document.original_bytes, original);
    assert!(document.contents.stop.is_none());
    assert_eq!(
        document.contents.bytes_consumed,
        document.contents.original_bytes.len()
    );
    assert_eq!(document.streams.len(), expected.0);
    for stream in &document.streams {
        assert!(stream.objects.iter().all(|object| object.complete));
        let inspected = document
            .inspection
            .streams
            .iter()
            .find(|entry| entry.index == stream.index)
            .unwrap();
        assert!(inspected.structure_complete);
        assert_eq!(inspected.bytes_consumed as u64, inspected.byte_length);
    }
    let report = inspect(path).unwrap();
    assert!(report.can_import, "{:#?}", report.diagnostics);
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    );
    let project = import_project(path).unwrap();
    assert_eq!((project.width, project.height, project.fps), dimensions);
    project.validate().unwrap();

    // Compare every original symbol, ordered layer, keyframe boundary and label.
    // Synthetic groups may add symbols; they must never replace an original one.
    let mut source_layers = 0;
    let mut source_frames = 0;
    for descriptor in document
        .contents
        .descriptors
        .iter()
        .filter(|entry| entry.class == ContentsDescriptorClass::DocumentPage)
    {
        let stream_index = descriptor.stream_index.unwrap();
        let stream = document
            .streams
            .iter()
            .find(|stream| stream.index == stream_index)
            .unwrap();
        let symbol = project
            .symbol(&format!("fla-symbol-{stream_index}"))
            .unwrap();
        assert_eq!(
            symbol.name,
            string(&document.contents.original_bytes, &descriptor.name)
        );
        let page = object(stream, stream.root.unwrap());
        assert_eq!(symbol.timeline.layers.len(), page.children.len());
        let mut total_frames = 1;
        for (layer, id) in symbol.timeline.layers.iter().zip(&page.children) {
            source_layers += 1;
            let source = object(stream, *id);
            let Some(TimelinePayload::Layer(fields)) = &source.payload else {
                panic!()
            };
            assert_eq!(
                layer.name,
                string(&stream.original_bytes, fields.name.as_ref().unwrap())
            );
            assert_eq!(layer.keyframes.len(), source.children.len());
            let mut cursor = 0;
            for (key, frame) in layer.keyframes.iter().zip(&source.children) {
                source_frames += 1;
                let Some(TimelinePayload::Frame(frame)) = &object(stream, *frame).payload else {
                    panic!()
                };
                assert_eq!(key.frame, cursor);
                assert_eq!(
                    key.label,
                    frame
                        .label
                        .as_ref()
                        .map(|label| string(&stream.original_bytes, label))
                        .unwrap_or_default()
                );
                assert_eq!(key.tween.is_some(), frame.key_mode.unwrap_or(0) & 255 == 1);
                if let Some(Easing::CubicBezier { x1, y1, x2, y2 }) = key.tween {
                    let curve = &frame.custom_ease[5];
                    assert_eq!(
                        [x1, y1, x2, y2],
                        [curve[0][4], curve[0][5], curve[1][0], curve[1][1]]
                    );
                }
                cursor += u32::from(frame.duration.unwrap());
            }
            total_frames = total_frames.max(cursor);
        }
        assert_eq!(symbol.timeline.frame_count, total_frames);
    }
    assert_eq!((source_layers, source_frames), (expected.1, expected.2));
    let mut imported: Vec<_> = authored_scripts(&project)
        .into_iter()
        .map(|script| script.source.clone())
        .collect();
    let mut source: Vec<_> = source_scripts(&document)
        .into_iter()
        .map(|entry| entry.0)
        .collect();
    imported.sort();
    source.sort();
    assert_eq!(imported, source);

    let original_resource = project
        .resources
        .iter()
        .find(|resource| resource.media_type == "application/x-neoflash-fla-source")
        .unwrap();
    assert_eq!(original_resource.decoded_bytes().unwrap(), original);
    let roundtrip = Project::from_json(&project.to_json().unwrap()).unwrap();
    assert_eq!(roundtrip, project);
    for (before, after) in project.resources.iter().zip(&roundtrip.resources) {
        assert_eq!(
            before.decoded_bytes().unwrap(),
            after.decoded_bytes().unwrap()
        );
    }
    assert_eq!(fs::read(path).unwrap(), original);
    (document, project)
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-8 * expected.abs().max(1.),
        "{actual} != {expected}"
    );
}

fn expected_easing(t: f64, easing: Easing) -> f64 {
    match easing {
        Easing::Flash { acceleration } => {
            let amount = acceleration / 100.;
            if amount < 0. {
                t * (1. + amount) - t * t * amount
            } else {
                t * (1. - amount) + (1. - (1. - t).powi(2)) * amount
            }
        }
        Easing::CubicBezier { x1, y1, x2, y2 } => {
            let coordinate = |u: f64, p: f64, q: f64| {
                3. * (1. - u).powi(2) * u * p + 3. * (1. - u) * u * u * q + u.powi(3)
            };
            let (mut low, mut high) = (0., 1.);
            for _ in 0..52 {
                let middle = (low + high) / 2.;
                if coordinate(middle, x1, x2) < t {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            coordinate((low + high) / 2., y1, y2)
        }
        _ => panic!("Unexpected easing in this fixture"),
    }
}

fn check_tween_and_mask_animation(project: &Project) {
    let mut tweens = 0;
    let mut bezier = 0;
    let mut moving = 0;
    for layer in project
        .symbols
        .iter()
        .flat_map(|symbol| &symbol.timeline.layers)
    {
        for keys in layer.keyframes.windows(2) {
            let [first, next] = keys else { unreachable!() };
            let Some(easing) = first.tween else { continue };
            tweens += 1;
            bezier += usize::from(matches!(easing, Easing::CubicBezier { .. }));
            assert_eq!(first.elements.len(), 1);
            assert_eq!(next.elements.len(), 1);
            let (start, end) = (&first.elements[0], &next.elements[0]);
            assert_eq!(start.id, end.id);
            assert!(matches!(start.kind, ElementKind::Instance { .. }));
            let frame = first.frame + (next.frame - first.frame) / 2;
            assert!(frame > first.frame && frame < next.frame);
            let sampled = sample_layer(layer, frame);
            assert_eq!(sampled.len(), 1);
            assert_eq!(sampled[0].id, start.id);
            let ratio = expected_easing(
                f64::from(frame - first.frame) / f64::from(next.frame - first.frame),
                easing,
            );
            for (a, b, actual) in [
                (start.transform.x, end.transform.x, sampled[0].transform.x),
                (start.transform.y, end.transform.y, sampled[0].transform.y),
                (
                    start.transform.scale_x,
                    end.transform.scale_x,
                    sampled[0].transform.scale_x,
                ),
                (
                    start.transform.scale_y,
                    end.transform.scale_y,
                    sampled[0].transform.scale_y,
                ),
                (
                    start.transform.rotation,
                    end.transform.rotation,
                    sampled[0].transform.rotation,
                ),
            ] {
                close(actual, a + (b - a) * ratio);
            }
            for channel in 0..4 {
                let a = start.transform.color_transform.multipliers[channel];
                let b = end.transform.color_transform.multipliers[channel];
                close(
                    sampled[0].transform.color_transform.multipliers[channel],
                    a + (b - a) * ratio,
                );
            }
            moving += usize::from(start.transform != end.transform);
        }
    }
    assert_eq!((tweens, bezier), (17, 4));
    assert!(moving > 0);
    assert_eq!(
        project
            .symbols
            .iter()
            .map(|symbol| symbol.timeline.frame_count)
            .max(),
        Some(381)
    );
    assert_eq!(
        project
            .symbols
            .iter()
            .flat_map(|symbol| &symbol.timeline.layers)
            .filter(|layer| layer.properties.role == LayerRole::Mask)
            .count(),
        2
    );

    let button = project
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Button)
        .unwrap();
    assert_eq!(button.timeline.frame_count, 4);
    for frame in 0..3 {
        assert!(evaluate(project, &button.id, frame).is_empty());
    }
    assert!(!evaluate(project, &button.id, 3).is_empty());
    let root = project.symbol(&project.root).unwrap();
    let placement = root.timeline.layers.iter().flat_map(|layer| &layer.keyframes)
        .flat_map(|key| &key.elements).find(|element| matches!(&element.kind, ElementKind::Instance { symbol_id, .. } if symbol_id == &button.id)).unwrap();
    let player = Player::new(project);
    let snapshot = player.runtime_snapshot(project);
    let instance = snapshot
        .objects
        .iter()
        .find(|object| object.display.path == [placement.id.clone()])
        .unwrap();
    assert!(!instance.button_hits.is_empty());
    assert!(!snapshot.draws.iter().any(|draw| {
        draw.item
            .instance_path
            .starts_with(std::slice::from_ref(&placement.id))
    }));
}

#[test]
#[ignore = "requires local user-supplied binary FLA corpus in NEOFLASH_LEGACY_SAMPLE_DIR"]
fn local_legacy_samples_preserve_structure_animation_scripts_and_source() {
    let mut corpus_files = Vec::new();
    collect_fla_files(&sample_root(), &mut corpus_files);
    assert!(
        !corpus_files.is_empty(),
        "No local .fla files found in NEOFLASH_LEGACY_SAMPLE_DIR"
    );

    let as1_path = find_sample_by_profile(&corpus_files, (550, 550, 12), 14);
    let (as1_source, as1_sample) = check_sample(&as1_path, (550, 550, 12), (14, 37, 37));
    let scripts = source_scripts(&as1_source);
    assert_eq!(scripts.len(), 1);
    assert_eq!(
        (scripts[0].0.len(), scripts[0].1, scripts[0].2),
        (531, false, 531)
    );
    assert!(
        authored_scripts(&as1_sample)
            .iter()
            .all(|script| script.language == ScriptLanguage::ActionScript1)
    );
    assert_eq!(
        as1_sample
            .symbols
            .iter()
            .flat_map(|symbol| &symbol.timeline.layers)
            .flat_map(|layer| &layer.keyframes)
            .flat_map(|key| &key.elements)
            .filter(|element| matches!(element.kind, ElementKind::TextBox { .. }))
            .count(),
        20
    );

    let as2_path = find_sample_by_profile(&corpus_files, (300, 600, 35), 15);
    let (as2_source, as2_sample) = check_sample(&as2_path, (300, 600, 35), (15, 32, 71));
    let scripts = source_scripts(&as2_source);
    assert_eq!(scripts.len(), 2);
    let lengths: BTreeMap<_, _> = scripts
        .iter()
        .map(|(text, utf16, bytes)| {
            assert!(*utf16);
            (text.len(), *bytes)
        })
        .collect();
    assert_eq!(lengths, BTreeMap::from([(37, 74), (75, 150)]));
    assert!(
        authored_scripts(&as2_sample)
            .iter()
            .all(|script| script.language == ScriptLanguage::ActionScript2)
    );
    assert_eq!(as2_sample.library_folders.len(), 2);
    let images: Vec<_> = as2_sample
        .resources
        .iter()
        .filter(|resource| resource.media_type == "image/png")
        .collect();
    assert_eq!(images.len(), 20);
    for resource in images {
        let bytes = resource.decoded_bytes().unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).unwrap();
        assert!(image.width() > 0 && image.height() > 0);
    }
    let provenance = as2_sample
        .resources
        .iter()
        .find(|resource| resource.path == "legacy/provenance.json")
        .unwrap();
    let provenance: serde_json::Value =
        serde_json::from_slice(&provenance.decoded_bytes().unwrap()).unwrap();
    assert_eq!(provenance["publish_hidden_layers"], true);
    let targets: std::collections::BTreeSet<_> = as2_sample
        .symbols
        .iter()
        .flat_map(|symbol| &symbol.timeline.layers)
        .flat_map(|layer| {
            std::iter::once(layer.id.as_str()).chain(
                layer
                    .keyframes
                    .iter()
                    .flat_map(|key| key.elements.iter().map(|e| e.id.as_str())),
            )
        })
        .collect();
    for mapping in provenance["placements"].as_array().unwrap() {
        if let Some(target) = mapping["target"].as_str() {
            assert!(
                targets.contains(target),
                "Provenance points to absent target {target}"
            );
        }
    }

    assert_eq!(
        provenance["placements"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["editor_visible"] == false)
            .count(),
        11
    );
    check_tween_and_mask_animation(&as2_sample);
}
