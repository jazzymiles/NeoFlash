//! Multiple scenes published as one main timeline. Flash concatenates scene
//! timelines when it writes a SWF and keeps scene names and frame offsets as
//! metadata; this module does the same for the HTML5 runtime.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// One scene inside the published main timeline. `start` is the zero-based
/// first frame of the scene in the concatenated timeline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneRange {
    pub name: String,
    pub start: u32,
    pub frames: u32,
}

impl SceneRange {
    pub fn contains(&self, frame: u32) -> bool {
        frame >= self.start && frame - self.start < self.frames
    }
    /// Exclusive zero-based end frame.
    pub fn end(&self) -> u32 {
        self.start.saturating_add(self.frames)
    }
}

/// Scene metadata as exposed by `MovieClip.scenes`/`currentScene`: label
/// frames are one-based and relative to the scene, as in Flash Player.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneMetadata {
    pub name: String,
    pub num_frames: u32,
    pub labels: Vec<FrameLabel>,
}

/// Frame argument of `gotoAndPlay`/`gotoAndStop`: a one-based frame number or
/// a label (numeric strings are interpreted by the language rules).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GotoFrame {
    Number(f64),
    Label(String),
}

/// A goto request as the browser adapter forwards it. Without `scene`, AS3/JS
/// frame numbers are relative to the current scene and AS2 frame numbers are
/// absolute main-timeline frames, matching the published SWF semantics.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GotoRequest {
    pub frame: GotoFrame,
    #[serde(default)]
    pub scene: Option<String>,
    #[serde(default)]
    pub as2: bool,
}

/// Scene ranges of the authored document in play order. A document without
/// `scenes` has one unnamed scene and returns an empty list.
pub fn scene_ranges(project: &Project) -> Vec<SceneRange> {
    let mut start = 0u32;
    project
        .scenes
        .iter()
        .filter_map(|id| project.symbol(id))
        .map(|symbol| {
            let range = SceneRange {
                name: symbol.name.clone(),
                start,
                frames: symbol.timeline.frame_count,
            };
            start = start.saturating_add(symbol.timeline.frame_count);
            range
        })
        .collect()
}

/// Concatenate all scenes into the root timeline. Projects with at most one
/// scene are returned unchanged.
pub fn flatten_scenes(project: &Project) -> Project {
    flatten_scenes_with_ranges(project).0
}

/// [`flatten_scenes`] plus the scene ranges inside the flattened root
/// timeline. The result is idempotent: a flattened project has no `scenes`.
pub fn flatten_scenes_with_ranges(project: &Project) -> (Project, Vec<SceneRange>) {
    let ranges = scene_ranges(project);
    if project.scenes.len() <= 1 {
        return (project.clone(), ranges);
    }
    let mut used = identities(project);
    let mut taken = HashSet::new();
    let mut layers = vec![];
    let mut scripts = vec![];
    let mut offset = 0u32;
    for (index, id) in project.scenes.iter().enumerate() {
        let Some(symbol) = project.symbol(id) else {
            continue;
        };
        let count = symbol.timeline.frame_count;
        // Layer IDs are document-unique in a valid project; imported scenes
        // can still repeat one. Rename the later layer and its references.
        let mut renamed: HashMap<&str, Id> = HashMap::new();
        for layer in &symbol.timeline.layers {
            if taken.insert(layer.id.clone()) {
                continue;
            }
            let mut suffix = index + 1;
            let fresh = loop {
                let candidate = format!("{}~scene{suffix}", layer.id);
                if !used.contains(&candidate) && !taken.contains(&candidate) {
                    break candidate;
                }
                suffix += 1;
            };
            used.insert(fresh.clone());
            taken.insert(fresh.clone());
            renamed.insert(layer.id.as_str(), fresh);
        }
        for source in &symbol.timeline.layers {
            let mut layer = source.clone();
            if let Some(fresh) = renamed.get(source.id.as_str()) {
                layer.id = fresh.clone();
            }
            let properties = &mut layer.properties;
            for reference in [
                &mut properties.parent,
                &mut properties.mask,
                &mut properties.motion_guide,
            ] {
                if let Some(target) = reference.as_mut()
                    && let Some(fresh) = renamed.get(target.as_str())
                {
                    *target = fresh.clone();
                }
            }
            let end = properties.end_frame.unwrap_or(count).min(count);
            properties.end_frame = Some(offset.saturating_add(end));
            for key in &mut layer.keyframes {
                key.frame = key.frame.saturating_add(offset);
            }
            layers.push(layer);
        }
        scripts.extend(symbol.timeline.scripts.iter().map(|script| FrameScript {
            frame: script.frame.saturating_add(offset),
            script: script.script.clone(),
        }));
        offset = offset.saturating_add(count);
    }
    let mut flat = project.clone();
    let merged: HashSet<&str> = project
        .scenes
        .iter()
        .map(String::as_str)
        .filter(|id| *id != project.root)
        .collect();
    flat.symbols
        .retain(|symbol| !merged.contains(symbol.id.as_str()));
    if let Some(root) = flat.symbol_mut(&project.root) {
        root.timeline = Timeline {
            frame_count: offset.max(1),
            layers,
            scripts,
        };
    }
    flat.scenes.clear();
    (flat, ranges)
}

fn identities(project: &Project) -> HashSet<Id> {
    let mut used = HashSet::new();
    for symbol in &project.symbols {
        used.insert(symbol.id.clone());
        for layer in &symbol.timeline.layers {
            used.insert(layer.id.clone());
            for element in layer.keyframes.iter().flat_map(|key| &key.elements) {
                used.insert(element.id.clone());
            }
        }
    }
    used
}

/// Ensure host-supplied scene ranges describe the whole root timeline.
pub(crate) fn validate_ranges(ranges: &[SceneRange], frame_count: u32) -> Result<(), String> {
    let mut start = 0u32;
    for range in ranges {
        if range.start != start || range.frames == 0 {
            return Err(format!(
                "Scene '{}' must start at frame {} and contain at least one frame.",
                range.name,
                start + 1
            ));
        }
        start = range.end();
    }
    if !ranges.is_empty() && start != frame_count {
        return Err(format!(
            "Scenes cover {start} frames, but the main timeline has {frame_count}."
        ));
    }
    Ok(())
}

/// Labels in authored order (layer order, then keyframe order) with
/// zero-based frames of the timeline that owns them.
pub(crate) fn timeline_labels(timeline: &Timeline) -> Vec<(&str, u32)> {
    timeline
        .layers
        .iter()
        .flat_map(|layer| &layer.keyframes)
        .filter(|key| !key.label.is_empty())
        .map(|key| (key.label.as_str(), key.frame))
        .collect()
}

/// Scene metadata with labels scoped to each scene and sorted by frame. When
/// several layers label one frame, the first authored layer stays first.
pub(crate) fn scene_metadata(scenes: &[SceneRange], labels: &[(&str, u32)]) -> Vec<SceneMetadata> {
    scenes
        .iter()
        .map(|scene| {
            let mut scoped: Vec<_> = labels
                .iter()
                .filter(|(_, frame)| scene.contains(*frame))
                .map(|(name, frame)| {
                    FrameLabel::new((*name).into(), (frame - scene.start + 1) as i32)
                })
                .collect();
            scoped.sort_by_key(|label| label.frame);
            SceneMetadata {
                name: scene.name.clone(),
                num_frames: scene.frames,
                labels: scoped,
            }
        })
        .collect()
}

fn scene_suffix(scene: &SceneRange) -> String {
    if scene.name.is_empty() {
        String::new()
    } else {
        format!(" in scene '{}'", scene.name)
    }
}

/// AVM1 parses numeric strings before consulting labels.
fn as2_number(text: &str) -> Option<f64> {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    match unsigned {
        "Infinity" => Some(if text.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        }),
        "NaN" => Some(f64::NAN),
        _ if !unsigned.is_empty()
            && unsigned.starts_with(|c: char| c.is_ascii_digit() || c == '.')
            && unsigned
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')) =>
        {
            text.parse().ok()
        }
        _ => None,
    }
}

/// Resolve a goto request against a timeline split into `scenes` (one
/// unnamed scene for ordinary clips). Returns the zero-based target frame,
/// `Ok(None)` for an AS2 label that does not exist (AVM1 ignores it), or an
/// actionable error. Unknown scenes are always errors.
pub fn resolve_goto(
    scenes: &[SceneRange],
    labels: &[(&str, u32)],
    current: u32,
    request: &GotoRequest,
) -> Result<Option<u32>, String> {
    let as2 = request.as2;
    let whole = SceneRange {
        name: String::new(),
        start: 0,
        frames: scenes.last().map_or(1, SceneRange::end),
    };
    let target = match &request.scene {
        Some(name) => scenes
            .iter()
            .find(|scene| {
                if as2 {
                    scene.name.eq_ignore_ascii_case(name)
                } else {
                    scene.name == *name
                }
            })
            .ok_or_else(|| {
                let names: Vec<_> = scenes
                    .iter()
                    .filter(|scene| !scene.name.is_empty())
                    .map(|scene| format!("'{}'", scene.name))
                    .collect();
                if names.is_empty() {
                    format!("Scene '{name}' was not found; this timeline has no named scenes.")
                } else {
                    format!(
                        "Scene '{name}' was not found. Available scenes: {}.",
                        names.join(", ")
                    )
                }
            })?,
        // AS2 numbers and labels address the published main timeline.
        None if as2 => &whole,
        None => scenes
            .iter()
            .find(|scene| scene.contains(current))
            .unwrap_or(&whole),
    };
    let number = match &request.frame {
        GotoFrame::Number(value) => Some(*value),
        GotoFrame::Label(text) if as2 => as2_number(text),
        GotoFrame::Label(_) => None,
    };
    let in_range = |value: f64| {
        (value.fract() == 0. && value >= 1. && value <= target.frames as f64)
            .then(|| target.start + value as u32 - 1)
    };
    if let Some(value) = number {
        return in_range(value).map(Some).ok_or_else(|| {
            if as2 {
                format!(
                    "AS2 numeric goto currently supports integer frames 1 through {}{}; legacy clamping, wrapping and other numeric coercions are not supported.",
                    target.frames,
                    scene_suffix(target)
                )
            } else {
                format!(
                    "Frame must be between 1 and {}{}.",
                    target.frames,
                    scene_suffix(target)
                )
            }
        });
    }
    let GotoFrame::Label(label) = &request.frame else {
        unreachable!("numbers returned above");
    };
    if as2 {
        return Ok(labels
            .iter()
            .filter(|(name, frame)| {
                !label.is_empty() && name.eq_ignore_ascii_case(label) && target.contains(*frame)
            })
            .map(|(_, frame)| *frame)
            .min());
    }
    let find = |scope: &SceneRange| {
        labels
            .iter()
            .find(|(name, frame)| *name == label && scope.contains(*frame))
            .map(|(_, frame)| *frame)
    };
    if let Some(frame) = find(target) {
        return Ok(Some(frame));
    }
    // Without an explicit scene, a label elsewhere in the main timeline still
    // resolves, as published SWF label tables are timeline-wide.
    if request.scene.is_none()
        && let Some(frame) = find(&whole)
    {
        return Ok(Some(frame));
    }
    if label.bytes().all(|b| b.is_ascii_digit()) && !label.is_empty() {
        return in_range(label.parse().unwrap_or(f64::NAN))
            .map(Some)
            .ok_or_else(|| {
                format!(
                    "Frame must be between 1 and {}{}.",
                    target.frames,
                    scene_suffix(target)
                )
            });
    }
    Err(format!(
        "Frame label '{label}' does not exist{}.",
        scene_suffix(target)
    ))
}

/// First frame of the scene `step` scenes away from the one containing
/// `current`; `None` past the first or last scene.
pub fn scene_step(scenes: &[SceneRange], current: u32, step: i32) -> Option<u32> {
    let index = scenes.iter().position(|scene| scene.contains(current))?;
    let target = index.checked_add_signed(step as isize)?;
    scenes.get(target).map(|scene| scene.start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(frame: u32, label: &str) -> Keyframe {
        Keyframe {
            frame,
            label: label.into(),
            ..Default::default()
        }
    }

    fn layer(id: &str, keys: Vec<Keyframe>) -> Layer {
        Layer {
            id: id.into(),
            name: id.into(),
            visible: true,
            locked: false,
            keyframes: keys,
            properties: Default::default(),
        }
    }

    fn script(frame: u32, source: &str) -> FrameScript {
        FrameScript {
            frame,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: source.into(),
            },
        }
    }

    fn scene(id: &str, name: &str, frame_count: u32, layers: Vec<Layer>) -> Symbol {
        Symbol {
            id: id.into(),
            name: name.into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count,
                layers,
                scripts: vec![],
            },
            class_name: None,
            linkage_id: None,
        }
    }

    fn two_scenes() -> Project {
        let mut project = Project::blank();
        project.format_version = 6;
        let root = project.root.clone();
        let mut first = scene(
            &root,
            "Intro",
            5,
            vec![
                layer("intro-art", vec![key(0, "start"), key(3, "")]),
                layer("intro-labels", vec![key(0, ""), key(4, "outro")]),
            ],
        );
        first.timeline.scripts = vec![script(0, "trace('intro')"), script(4, "trace('intro end')")];
        let mut second = scene(
            "scene-2",
            "Main",
            3,
            vec![layer("main-art", vec![key(1, "middle")])],
        );
        second.timeline.layers[0].properties.end_frame = Some(2);
        second.timeline.scripts = vec![script(0, "trace('main')")];
        project.symbols = vec![first, second];
        project.scenes = vec![root, "scene-2".into()];
        project.validate().unwrap();
        project
    }

    #[test]
    fn two_scenes_flatten_into_one_main_timeline() {
        let project = two_scenes();
        let (flat, ranges) = flatten_scenes_with_ranges(&project);
        assert_eq!(
            ranges,
            vec![
                SceneRange {
                    name: "Intro".into(),
                    start: 0,
                    frames: 5
                },
                SceneRange {
                    name: "Main".into(),
                    start: 5,
                    frames: 3
                },
            ]
        );
        assert!(flat.scenes.is_empty());
        assert_eq!(
            flat.symbols.len(),
            1,
            "merged scene symbols leave the library"
        );
        let root = flat.symbol(&flat.root).unwrap();
        assert_eq!(root.name, "Intro");
        assert_eq!(root.timeline.frame_count, 8);
        let ids: Vec<_> = root.timeline.layers.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["intro-art", "intro-labels", "main-art"]);
        let ends: Vec<_> = root
            .timeline
            .layers
            .iter()
            .map(|l| l.properties.end_frame)
            .collect();
        assert_eq!(ends, [Some(5), Some(5), Some(7)]);
        let main = &root.timeline.layers[2];
        assert_eq!(main.keyframes[0].frame, 6);
        assert_eq!(main.keyframes[0].label, "middle");
        assert!(main.key_at(5).is_none(), "empty before its first key");
        assert!(
            main.key_at(4).is_none(),
            "nothing from scene 2 shows in scene 1"
        );
        assert!(main.key_at(6).is_some());
        assert!(main.key_at(7).is_none(), "authored exposure end is kept");
        assert!(
            root.timeline.layers[0].key_at(5).is_none(),
            "scene 1 ends at its last frame"
        );
        let frames: Vec<_> = root.timeline.scripts.iter().map(|s| s.frame).collect();
        assert_eq!(frames, [0, 4, 5]);
        assert_eq!(root.timeline.scripts[2].script.source, "trace('main')");
        assert!(
            crate::evaluate(&flat, &flat.root, 6).is_empty(),
            "label-only keys draw nothing"
        );
        flat.validate().unwrap();
        assert_eq!(flatten_scenes(&flat), flat, "flattening is idempotent");
    }

    #[test]
    fn single_scene_projects_are_unchanged() {
        let project = Project::blank();
        assert_eq!(flatten_scenes(&project), project);
        assert!(scene_ranges(&project).is_empty());
        let mut one = Project::blank();
        one.format_version = 6;
        one.scenes = vec![one.root.clone()];
        let (flat, ranges) = flatten_scenes_with_ranges(&one);
        assert_eq!(flat, one);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "Scene 1");
    }

    #[test]
    fn colliding_layer_ids_are_renamed_with_their_references() {
        let mut project = two_scenes();
        let second = project.symbol_mut("scene-2").unwrap();
        second.timeline.layers[0].id = "intro-art".into();
        second
            .timeline
            .layers
            .push(layer("intro-labels", vec![key(0, "")]));
        second.timeline.layers[1].properties.parent = Some("intro-art".into());
        second.timeline.layers[1].properties.mask = Some("intro-art".into());
        let flat = flatten_scenes(&project);
        let layers = &flat.symbol(&flat.root).unwrap().timeline.layers;
        let ids: Vec<_> = layers.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "intro-art",
                "intro-labels",
                "intro-art~scene2",
                "intro-labels~scene2"
            ]
        );
        assert_eq!(
            layers[3].properties.parent.as_deref(),
            Some("intro-art~scene2")
        );
        assert_eq!(
            layers[3].properties.mask.as_deref(),
            Some("intro-art~scene2")
        );
        assert_eq!(layers[0].properties.parent, None);
    }

    #[test]
    fn goto_resolves_scene_names_labels_and_language_rules() {
        let project = two_scenes();
        let (flat, ranges) = flatten_scenes_with_ranges(&project);
        let labels = timeline_labels(&flat.symbol(&flat.root).unwrap().timeline);
        let request = |frame: GotoFrame, scene: Option<&str>, as2| GotoRequest {
            frame,
            scene: scene.map(Into::into),
            as2,
        };
        let go = |frame, scene, as2, current| {
            resolve_goto(&ranges, &labels, current, &request(frame, scene, as2))
        };
        use GotoFrame::{Label, Number};
        assert_eq!(go(Number(1.), Some("Main"), false, 0), Ok(Some(5)));
        assert_eq!(
            go(Number(2.), None, false, 6),
            Ok(Some(6)),
            "AS3 numbers are scene-relative"
        );
        assert_eq!(
            go(Number(2.), None, true, 6),
            Ok(Some(1)),
            "AS2 numbers are absolute"
        );
        assert_eq!(
            go(Label("Main".into()), Some("Main"), false, 0).unwrap_err(),
            "Frame label 'Main' does not exist in scene 'Main'."
        );
        assert_eq!(
            go(Label("middle".into()), Some("Main"), false, 0),
            Ok(Some(6))
        );
        assert_eq!(
            go(Label("middle".into()), None, false, 0),
            Ok(Some(6)),
            "timeline-wide fallback"
        );
        assert_eq!(
            go(Label("2".into()), Some("main"), true, 0),
            Ok(Some(6)),
            "AS2 scene names fold ASCII case"
        );
        assert_eq!(
            go(Label("MIDDLE".into()), Some("Main"), true, 0),
            Ok(Some(6))
        );
        assert_eq!(
            go(Label("start".into()), Some("Main"), true, 0),
            Ok(None),
            "AS2 missing label is a no-op"
        );
        assert_eq!(
            go(Number(4.), Some("Main"), false, 0).unwrap_err(),
            "Frame must be between 1 and 3 in scene 'Main'."
        );
        let missing = go(Number(1.), Some("Outro"), false, 0).unwrap_err();
        assert_eq!(
            missing,
            "Scene 'Outro' was not found. Available scenes: 'Intro', 'Main'."
        );
        assert!(
            go(Number(1.), Some("Outro"), true, 0).is_err(),
            "AS2 unknown scenes fail too"
        );
        assert_eq!(scene_step(&ranges, 2, 1), Some(5));
        assert_eq!(scene_step(&ranges, 6, 1), None);
        assert_eq!(scene_step(&ranges, 6, -1), Some(0));
        assert_eq!(scene_step(&ranges, 0, -1), None);
        let metadata = scene_metadata(&ranges, &labels);
        assert_eq!(
            metadata[1].labels,
            vec![FrameLabel::new("middle".into(), 2)]
        );
        assert_eq!(metadata[0].labels.len(), 2);
    }
}
