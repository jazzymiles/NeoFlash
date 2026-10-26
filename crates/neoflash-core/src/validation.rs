use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("Invalid project JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid project: {0}")]
    Invalid(String),
}

impl Project {
    pub fn validate(&self) -> Result<(), ProjectError> {
        let issues = validate(self);
        if issues.is_empty() {
            crate::model::reserve_ids(self);
            Ok(())
        } else {
            Err(ProjectError::Invalid(
                issues
                    .into_iter()
                    .map(|i| format!("{}: {}", i.path, i.message))
                    .collect::<Vec<_>>()
                    .join("; "),
            ))
        }
    }

    pub fn to_json(&self) -> Result<String, ProjectError> {
        self.validate()?;
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(json: &str) -> Result<Self, ProjectError> {
        if json.len() > 32 * 1024 * 1024 {
            return Err(ProjectError::Invalid(
                "document exceeds 32 MiB; external assets are required".into(),
            ));
        }
        let project: Self = serde_json::from_str(json)?;
        project.validate()?;
        Ok(project)
    }
}

pub fn validate(project: &Project) -> Vec<ValidationIssue> {
    let mut issues = vec![];
    let mut add = |path: String, message: String| issues.push(ValidationIssue { path, message });
    if project.format_version != 1 {
        add(
            "format_version".into(),
            format!(
                "unsupported version {}; supported version is 1",
                project.format_version
            ),
        );
    }
    if project.width == 0 || project.height == 0 || project.width > 65535 || project.height > 65535
    {
        add("stage".into(), "dimensions must be in 1..=65535".into());
    }
    if project.fps == 0 || project.fps > 1000 {
        add("fps".into(), "frame rate must be in 1..=1000".into());
    }
    if project.symbols.len() > 10000 {
        add("symbols".into(), "symbol count exceeds 10000".into());
    }
    if project.symbol(&project.root).is_none() {
        add("root".into(), "root symbol does not exist".into());
    }
    let mut identities: HashMap<&str, String> = HashMap::new();
    let mut symbol_ids = HashSet::new();
    for symbol in &project.symbols {
        symbol_ids.insert(symbol.id.as_str());
    }
    let mut elements_count = 0usize;
    for (si, symbol) in project.symbols.iter().enumerate() {
        let sp = format!("symbols[{si}]");
        register(
            &mut identities,
            &symbol.id,
            &format!("symbol:{si}"),
            &format!("{sp}.id"),
            &mut add,
        );
        if symbol.timeline.frame_count == 0 || symbol.timeline.frame_count > 1_000_000 {
            add(
                format!("{sp}.timeline.frame_count"),
                "frame count must be in 1..=1000000".into(),
            );
        }
        for script in &symbol.timeline.scripts {
            if script.frame >= symbol.timeline.frame_count {
                add(
                    format!("{sp}.timeline.scripts"),
                    "script frame is outside the timeline".into(),
                );
            }
        }
        for (li, layer) in symbol.timeline.layers.iter().enumerate() {
            let lp = format!("{sp}.timeline.layers[{li}]");
            register(
                &mut identities,
                &layer.id,
                &format!("layer:{si}:{li}"),
                &format!("{lp}.id"),
                &mut add,
            );
            let mut keys = HashSet::new();
            for (ki, key) in layer.keyframes.iter().enumerate() {
                let kp = format!("{lp}.keyframes[{ki}]");
                if key.frame >= symbol.timeline.frame_count {
                    add(
                        format!("{kp}.frame"),
                        "keyframe is outside the timeline".into(),
                    );
                }
                if !keys.insert(key.frame) {
                    add(format!("{kp}.frame"), "duplicate frame in a layer".into());
                }
                if let Some(Easing::CubicBezier { x1, y1, x2, y2 }) = key.tween
                    && (![x1, y1, x2, y2].iter().all(|v| v.is_finite())
                        || !(0.0..=1.0).contains(&x1)
                        || !(0.0..=1.0).contains(&x2)
                        || y1.abs() > 100.
                        || y2.abs() > 100.)
                {
                    add(
                        format!("{kp}.tween"),
                        "Bézier controls must be finite, x in [0,1], |y| <= 100".into(),
                    );
                }
                let mut placed = HashSet::new();
                for (ei, element) in key.elements.iter().enumerate() {
                    elements_count += 1;
                    let ep = format!("{kp}.elements[{ei}]");
                    register(
                        &mut identities,
                        &element.id,
                        &format!("element:{si}:{li}"),
                        &format!("{ep}.id"),
                        &mut add,
                    );
                    if !placed.insert(&element.id) {
                        add(
                            format!("{ep}.id"),
                            "duplicate element in one keyframe".into(),
                        );
                    }
                    let t = element.transform;
                    if ![t.x, t.y, t.scale_x, t.scale_y, t.rotation, t.alpha]
                        .iter()
                        .all(|v| v.is_finite())
                        || t.x.abs() > 1e12
                        || t.y.abs() > 1e12
                        || t.scale_x.abs() > 1e4
                        || t.scale_y.abs() > 1e4
                        || t.rotation.abs() > 1e9
                        || !(0.0..=1.0).contains(&t.alpha)
                    {
                        add(
                            format!("{ep}.transform"),
                            "invalid or out-of-range transform".into(),
                        );
                    }
                    match &element.kind {
                        ElementKind::Rectangle {
                            width,
                            height,
                            stroke_width,
                            ..
                        }
                        | ElementKind::Ellipse {
                            width,
                            height,
                            stroke_width,
                            ..
                        } => {
                            if ![width, height, stroke_width]
                                .iter()
                                .all(|v| v.is_finite() && **v >= 0. && **v <= 1e9)
                            {
                                add(
                                    format!("{ep}.kind"),
                                    "shape dimensions/stroke must be finite and in 0..=1e9".into(),
                                );
                            }
                        }
                        ElementKind::Text { font_size, .. } => {
                            if !font_size.is_finite() || *font_size <= 0. || *font_size > 10000. {
                                add(
                                    format!("{ep}.kind.font_size"),
                                    "font size must be finite and in (0,10000]".into(),
                                );
                            }
                        }
                        ElementKind::Instance {
                            symbol_id,
                            playback,
                        } => {
                            if !symbol_ids.contains(symbol_id.as_str()) {
                                add(
                                    format!("{ep}.kind.symbol_id"),
                                    format!("missing symbol {symbol_id}"),
                                );
                            }
                            if let InstancePlayback::SingleFrame(frame) = playback
                                && project
                                    .symbol(symbol_id)
                                    .is_some_and(|s| *frame >= s.timeline.frame_count)
                            {
                                add(
                                    format!("{ep}.kind.playback"),
                                    "single-frame index is outside the referenced symbol".into(),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    if elements_count > 100000 {
        add(
            "symbols".into(),
            "document exceeds 100000 authored element records".into(),
        );
    }
    // Cycles are rejected even in unused library symbols, ensuring safe nesting later.
    let graph: HashMap<&str, Vec<&str>> = project
        .symbols
        .iter()
        .map(|s| {
            let refs = s
                .timeline
                .layers
                .iter()
                .flat_map(|l| &l.keyframes)
                .flat_map(|k| &k.elements)
                .filter_map(|e| match &e.kind {
                    ElementKind::Instance { symbol_id, .. } => Some(symbol_id.as_str()),
                    _ => None,
                })
                .collect();
            (s.id.as_str(), refs)
        })
        .collect();
    let mut active = HashSet::new();
    let mut depths = HashMap::new();
    let mut valid_graph = true;
    for symbol in &project.symbols {
        match depth(&symbol.id, &graph, &mut active, &mut depths) {
            Ok(d) if d > crate::evaluate::MAX_DEPTH => {
                valid_graph = false;
                add(
                    format!("symbol:{}", symbol.id),
                    "symbol nesting exceeds 64 levels".into(),
                );
            }
            Err(id) => {
                valid_graph = false;
                add(format!("symbol:{id}"), "cyclic symbol reference".into());
                break;
            }
            _ => {}
        }
    }
    if valid_graph {
        let mut counts = HashMap::new();
        for symbol in &project.symbols {
            if expanded_node_bound(project, &symbol.id, &mut counts) > crate::evaluate::MAX_NODES {
                add(
                    format!("symbol:{}", symbol.id),
                    "expanded scene may exceed 100000 nodes; reduce nested instance duplication"
                        .into(),
                );
            }
        }
    }
    issues
}

/// Conservative maximum across all keyframes, including independently timed children.
fn expanded_node_bound(project: &Project, id: &str, memo: &mut HashMap<Id, usize>) -> usize {
    if let Some(count) = memo.get(id) {
        return *count;
    }
    let Some(symbol) = project.symbol(id) else {
        return 1;
    };
    let mut count = 1;
    let cap = crate::evaluate::MAX_NODES + 1;
    for layer in &symbol.timeline.layers {
        let mut max_key = 0;
        for key in &layer.keyframes {
            let mut key_count = 0usize;
            for element in &key.elements {
                let nodes = match &element.kind {
                    ElementKind::Instance { symbol_id, .. } => {
                        expanded_node_bound(project, symbol_id, memo)
                    }
                    _ => 1,
                };
                key_count = key_count.saturating_add(nodes).min(cap);
            }
            max_key = max_key.max(key_count);
        }
        count = (count + max_key).min(cap);
    }
    memo.insert(id.into(), count);
    count
}

fn register<'a>(
    used: &mut HashMap<&'a str, String>,
    id: &'a str,
    owner: &str,
    path: &str,
    add: &mut impl FnMut(String, String),
) {
    if id.is_empty() || id.len() > 256 || id.contains('\0') {
        add(
            path.into(),
            "identity must be nonempty, <=256 bytes, without NUL".into(),
        );
    }
    if let Some(previous) = used.get(id) {
        if previous != owner {
            add(
                path.into(),
                format!("identity {id} is already owned by {previous}"),
            );
        }
    } else {
        used.insert(id, owner.into());
    }
}

fn depth<'a>(
    id: &'a str,
    graph: &HashMap<&'a str, Vec<&'a str>>,
    active: &mut HashSet<&'a str>,
    memo: &mut HashMap<&'a str, usize>,
) -> Result<usize, String> {
    if let Some(depth) = memo.get(id) {
        return Ok(*depth);
    }
    if !active.insert(id) {
        return Err(id.into());
    }
    // Hard recursion ceiling also protects validation of malicious inputs.
    if active.len() > crate::evaluate::MAX_DEPTH {
        active.remove(id);
        return Ok(crate::evaluate::MAX_DEPTH + 1);
    }
    let mut longest = 1;
    if let Some(children) = graph.get(id) {
        for child in children {
            longest = longest.max(1 + depth(child, graph, active, memo)?);
        }
    }
    active.remove(id);
    memo.insert(id, longest);
    Ok(longest)
}
