//! Inspect editable imports, render static authoring-stage frames matching Adobe Animate's
//! `t.currentFrame = f; doc.exportPNG(..., true, true)`, plan deterministic multi-frame
//! keyframe/tween-midpoint sweeps, and evaluate scripted / `MovieClip` `AuthoredPlayback`
//! alongside companion `SwfBytecodePlayer` behavioral parity.
use neoflash_core::{
    AuthoredPlayback, ElementKind, InstancePlayback, Project, SymbolKind, Timeline,
    evaluate_scene_with_frozen_clips,
};
use neoflash_export::render_project_frame_rgba;
use neoflash_import::{SwfBytecodePlayer, SwfDisplayEntry, import_project};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    path::{Path, PathBuf},
};

const MAX_SCRIPTED_SAMPLE_TICKS: u32 = 4096;
const DEFAULT_MAX_FRAMES_PER_SCENE: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameSweepMode {
    Sampled,
    KeyframesAndMidpoints,
}

impl FrameSweepMode {
    fn parse(raw: &str) -> Result<Self, Box<dyn Error>> {
        match raw {
            "sampled" => Ok(Self::Sampled),
            "keyframes-and-midpoints" => Ok(Self::KeyframesAndMidpoints),
            other => Err(format!(
                "unsupported --frame-mode {other:?}; expected sampled or keyframes-and-midpoints"
            )
            .into()),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Sampled => "sampled",
            Self::KeyframesAndMidpoints => "keyframes-and-midpoints",
        }
    }
}

fn project_has_any_scripts(project: &Project) -> bool {
    project.symbols.iter().any(|sym| {
        sym.timeline
            .scripts
            .iter()
            .any(|s| !s.script.source.trim().is_empty())
            || sym.timeline.layers.iter().any(|l| {
                l.keyframes.iter().any(|kf| {
                    kf.elements.iter().any(|el| {
                        el.scripts
                            .iter()
                            .any(|s| !s.script.source.trim().is_empty())
                    })
                })
            })
    })
}

fn project_has_multiframe_independent_clips(project: &Project) -> bool {
    let mc_multi: std::collections::HashSet<&str> = project
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::MovieClip && s.timeline.frame_count > 1)
        .map(|s| s.id.as_str())
        .collect();
    let multi_any: std::collections::HashSet<&str> = project
        .symbols
        .iter()
        .filter(|s| s.timeline.frame_count > 1)
        .map(|s| s.id.as_str())
        .collect();
    project.symbols.iter().any(|sym| {
        sym.timeline.layers.iter().any(|l| {
            l.keyframes.iter().any(|kf| {
                kf.elements.iter().any(|el| {
                    if let ElementKind::Instance {
                        symbol_id,
                        playback,
                    } = &el.kind
                    {
                        let is_mc = el
                            .instance_behavior
                            .map(|k| {
                                k == SymbolKind::MovieClip && multi_any.contains(symbol_id.as_str())
                            })
                            .unwrap_or_else(|| mc_multi.contains(symbol_id.as_str()));
                        is_mc
                            && (matches!(playback, InstancePlayback::Independent)
                                || (el.instance_behavior == Some(SymbolKind::MovieClip)
                                    && matches!(playback, InstancePlayback::Loop)))
                    } else {
                        false
                    }
                })
            })
        })
    })
}

fn diff_rgba_buffers(a: &[u8], b: &[u8]) -> (u64, u64) {
    let mut different = 0_u64;
    let mut visible = 0_u64;
    for (pa, pb) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0.iter()) {
        if pa != pb {
            different += 1;
            if pa[3] != 0 || pb[3] != 0 {
                visible += 1;
            }
        }
    }
    (different, visible)
}

fn sampled_scene_frames(last: u32) -> Vec<u32> {
    let candidates = [0, last / 2, last];
    let mut out = Vec::with_capacity(3);
    for f in candidates {
        if !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

/// Deterministically plan unique keyframe starts, tween midpoints, middle, and terminal frames
/// across `timeline`, bounded to at most `max_frames_per_scene` while preserving `0` and `last`.
fn keyframes_and_midpoints_scene_frames(
    timeline: &Timeline,
    max_frames_per_scene: usize,
) -> Vec<u32> {
    let limit = max_frames_per_scene.max(1);
    let last = timeline.frame_count.saturating_sub(1);
    if limit == 1 || last == 0 {
        return vec![0];
    }

    let mut set = BTreeSet::new();
    set.insert(0);
    set.insert(last / 2);
    set.insert(last);

    for script in &timeline.scripts {
        if !script.script.source.trim().is_empty() {
            set.insert(script.frame.min(last));
        }
    }

    for layer in &timeline.layers {
        for (idx, kf) in layer.keyframes.iter().enumerate() {
            let start = kf.frame.min(last);
            set.insert(start);
            if kf.tween.is_some() {
                let end = layer
                    .keyframes
                    .get(idx + 1)
                    .map(|next| next.frame.min(last))
                    .unwrap_or(last);
                if end > start + 1 {
                    set.insert((start + end) / 2);
                }
            }
        }
    }

    let candidates: Vec<u32> = set.into_iter().collect();
    if candidates.len() <= limit {
        return candidates;
    }

    let mut bounded = Vec::with_capacity(limit);
    let max_idx = candidates.len() - 1;
    for slot in 0..limit {
        let pick = candidates[slot * max_idx / (limit - 1)];
        if bounded.last().copied() != Some(pick) {
            bounded.push(pick);
        }
    }
    if bounded.first().copied() != Some(0) {
        bounded.insert(0, 0);
    }
    if bounded.last().copied() != Some(last) {
        if bounded.len() >= limit {
            *bounded.last_mut().unwrap() = last;
        } else {
            bounded.push(last);
        }
    }
    bounded
}

fn plan_scene_frames(
    timeline: &Timeline,
    mode: FrameSweepMode,
    max_frames_per_scene: usize,
) -> Vec<u32> {
    let last = timeline.frame_count.saturating_sub(1);
    match mode {
        FrameSweepMode::Sampled => sampled_scene_frames(last),
        FrameSweepMode::KeyframesAndMidpoints => {
            keyframes_and_midpoints_scene_frames(timeline, max_frames_per_scene)
        }
    }
}

fn active_timeline_label(timeline: &Timeline, frame: u32) -> Option<String> {
    for layer in &timeline.layers {
        if let Some(kf) = layer
            .keyframes
            .iter()
            .rfind(|kf| kf.frame <= frame && !kf.label.is_empty())
            && kf.frame == frame
        {
            return Some(kf.label.clone());
        }
    }
    None
}

fn collect_scene_labels(timeline: &Timeline, scene_idx: usize, global_offset: u32) -> Vec<Value> {
    let mut by_frame: BTreeMap<u32, String> = BTreeMap::new();
    for layer in &timeline.layers {
        for kf in &layer.keyframes {
            if !kf.label.is_empty() && kf.frame < timeline.frame_count {
                by_frame.entry(kf.frame).or_insert_with(|| kf.label.clone());
            }
        }
    }
    by_frame
        .into_iter()
        .map(|(f, lbl)| {
            json!({
                "scene": scene_idx,
                "scene_frame": f,
                "global_frame": global_offset + f,
                "label": lbl,
            })
        })
        .collect()
}

fn count_swf_display_nodes(entries: &[SwfDisplayEntry]) -> usize {
    entries
        .iter()
        .map(|e| 1 + count_swf_display_nodes(&e.children))
        .sum()
}

fn compute_behavioral_parity(
    project: &Project,
    companion_swf: Option<&Path>,
    frame_mode: FrameSweepMode,
    max_frames_per_scene: usize,
) -> Result<Value, Box<dyn Error>> {
    let has_scripts = project_has_any_scripts(project);
    let ids = if project.scenes.is_empty() {
        std::slice::from_ref(&project.root)
    } else {
        &project.scenes
    };

    let mut authored_total_frames = 0_u32;
    let mut authored_labels = Vec::new();
    let mut authored_label_names = Vec::new();
    let mut authored_display_samples = Vec::new();
    let mut authored_traces = Vec::new();
    let mut authored_diagnostics = Vec::new();

    for (scene_idx, id) in ids.iter().enumerate() {
        let timeline = &project.symbol(id).ok_or("missing scene")?.timeline;
        let scene_labels = collect_scene_labels(timeline, scene_idx, authored_total_frames);
        for item in &scene_labels {
            if let Some(lbl) = item.get("label").and_then(Value::as_str) {
                authored_label_names.push(lbl.to_owned());
            }
        }
        authored_labels.extend(scene_labels);

        let frames = plan_scene_frames(timeline, frame_mode, max_frames_per_scene);
        let max_step = timeline
            .frame_count
            .saturating_sub(1)
            .min(MAX_SCRIPTED_SAMPLE_TICKS);
        let mut playback = AuthoredPlayback::with_live_scripts(project, id, 0, true);
        authored_traces.extend(playback.drain_traces());
        authored_diagnostics.extend(playback.drain_diagnostics());

        let mut frame_set: BTreeSet<u32> = frames.iter().copied().collect();
        let mut cur_step = 0_u32;
        if frame_set.remove(&0) {
            let nodes = playback.scene(project, false, true).draws.len();
            let pf = playback.frame();
            authored_display_samples.push(json!({
                "scene": scene_idx,
                "scene_frame": 0,
                "global_frame": authored_total_frames,
                "playhead_frame": pf,
                "display_nodes": nodes,
            }));
        }
        while cur_step < max_step {
            playback.tick(project);
            authored_traces.extend(playback.drain_traces());
            authored_diagnostics.extend(playback.drain_diagnostics());
            cur_step += 1;
            if frame_set.remove(&cur_step) {
                let nodes = playback.scene(project, false, true).draws.len();
                let pf = playback.frame();
                authored_display_samples.push(json!({
                    "scene": scene_idx,
                    "scene_frame": cur_step,
                    "global_frame": authored_total_frames + cur_step,
                    "playhead_frame": pf,
                    "display_nodes": nodes,
                }));
            }
        }

        authored_total_frames = authored_total_frames.saturating_add(timeline.frame_count);
    }

    let Some(swf_path) = companion_swf else {
        return Ok(json!({
            "status": if has_scripts { "authored_scripted_only" } else { "authored_static_only" },
            "has_companion_swf": false,
            "has_scripts": has_scripts,
            "authored": {
                "scene_count": ids.len(),
                "total_frames": authored_total_frames,
                "labels": authored_labels,
                "display_samples": authored_display_samples,
                "traces": authored_traces,
                "diagnostics": authored_diagnostics,
            },
        }));
    };

    let mut player = SwfBytecodePlayer::from_path(swf_path)?;
    let swf_total_frames = player.total_frames();
    let swf_ticks = (swf_total_frames.max(authored_total_frames).max(1) as usize)
        .min(MAX_SCRIPTED_SAMPLE_TICKS as usize);
    let swf_run = player.run_frames(swf_ticks);

    let mut swf_labels = Vec::new();
    let mut swf_label_names = Vec::new();
    let mut seen_swf_labels = BTreeSet::new();
    let mut swf_nodes_by_step: BTreeMap<u32, usize> = BTreeMap::new();
    for (step_idx, state) in swf_run.frames_executed.iter().enumerate() {
        let step_u32 = step_idx as u32;
        swf_nodes_by_step
            .entry(step_u32)
            .or_insert_with(|| count_swf_display_nodes(&state.display_list));
        if let Some(lbl) = &state.label
            && seen_swf_labels.insert((state.frame, lbl.clone()))
        {
            swf_label_names.push(lbl.clone());
            swf_labels.push(json!({
                "frame": state.frame,
                "label": lbl,
            }));
        }
    }

    let last_halted_swf_nodes = swf_run
        .frames_executed
        .last()
        .filter(|st| !st.playing)
        .map(|st| count_swf_display_nodes(&st.display_list));

    let mut frame_comparisons = Vec::with_capacity(authored_display_samples.len());
    let mut display_presence_equal = true;
    let mut display_node_counts_equal = true;
    for sample in &authored_display_samples {
        let gf = sample
            .get("global_frame")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let auth_nodes = sample
            .get("display_nodes")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let swf_nodes = swf_nodes_by_step.get(&gf).copied().or_else(|| {
            (gf < swf_total_frames)
                .then_some(last_halted_swf_nodes)
                .flatten()
        });
        if let Some(sn) = swf_nodes {
            if (auth_nodes > 0) != (sn > 0) {
                display_presence_equal = false;
            }
            if auth_nodes != sn {
                display_node_counts_equal = false;
            }
        } else {
            display_presence_equal = false;
            display_node_counts_equal = false;
        }
        frame_comparisons.push(json!({
            "global_frame": gf,
            "scene": sample.get("scene"),
            "scene_frame": sample.get("scene_frame"),
            "authored_display_nodes": auth_nodes,
            "swf_display_nodes": swf_nodes,
        }));
    }

    let frame_count_equal = authored_total_frames == swf_total_frames;
    let labels_equal = authored_label_names == swf_label_names;
    let trace_equal = authored_traces == swf_run.trace_output;
    let status = if frame_count_equal
        && labels_equal
        && trace_equal
        && display_presence_equal
        && swf_run.unsupported_diagnostics.is_empty()
    {
        "parity_match"
    } else {
        "parity_difference"
    };

    Ok(json!({
        "status": status,
        "has_companion_swf": true,
        "has_scripts": has_scripts,
        "frame_count_equal": frame_count_equal,
        "labels_equal": labels_equal,
        "display_presence_equal": display_presence_equal,
        "display_node_counts_equal": display_node_counts_equal,
        "trace_equal": trace_equal,
        "authored": {
            "scene_count": ids.len(),
            "total_frames": authored_total_frames,
            "labels": authored_labels,
            "display_samples": authored_display_samples,
            "traces": authored_traces,
            "diagnostics": authored_diagnostics,
        },
        "swf": {
            "swf_version": swf_run.swf_version,
            "vm_kind": swf_run.vm_kind,
            "total_frames": swf_total_frames,
            "labels": swf_labels,
            "traces": swf_run.trace_output,
            "diagnostics": swf_run.unsupported_diagnostics,
        },
        "frame_comparisons": frame_comparisons,
    }))
}

fn authored(
    source: &Path,
    destination: &Path,
    frame_mode: FrameSweepMode,
    max_frames_per_scene: usize,
    companion_swf: Option<&Path>,
) -> Result<Value, Box<dyn Error>> {
    let project = import_project(source)?;
    project.validate()?;
    let w = project.width.max(1);
    let h = project.height.max(1);
    let has_scripts = project_has_any_scripts(&project);
    let has_independent_mcs = project_has_multiframe_independent_clips(&project);

    let mut scenes = Vec::new();
    let mut planned_scene_frames = Vec::new();
    let mut render_errors = Vec::new();
    let mut scripted_scenes = Vec::new();
    let mut all_traces = Vec::new();
    let mut all_diagnostics = Vec::new();
    let mut total_divergent_frames = 0_usize;

    let ids = if project.scenes.is_empty() {
        std::slice::from_ref(&project.root)
    } else {
        &project.scenes
    };

    for (scene, id) in ids.iter().enumerate() {
        let timeline = &project.symbol(id).ok_or("missing scene")?.timeline;
        let keys = timeline.layers.iter().flat_map(|layer| &layer.keyframes);
        let frame_script_count = timeline
            .scripts
            .iter()
            .filter(|script| !script.script.source.trim().is_empty())
            .count();
        let layout = [
            timeline.frame_count as usize,
            timeline.layers.len(),
            keys.clone().count(),
            keys.filter(|key| !key.label.is_empty()).count(),
            frame_script_count,
        ];
        scenes.push(layout);
        let _last = timeline
            .frame_count
            .checked_sub(1)
            .ok_or("empty timeline")?;
        let frames = plan_scene_frames(timeline, frame_mode, max_frames_per_scene);
        planned_scene_frames.push(frames.clone());
        let mut editable_buffers: BTreeMap<u32, Vec<u8>> = BTreeMap::new();

        for &frame in &frames {
            let path = destination.join(format!("native-s{scene}-f{frame}.png"));
            match render_project_frame_rgba(&project, Some(id), frame, w, h, true, false) {
                Ok(rgba) => {
                    let img = image::RgbaImage::from_raw(w, h, rgba.clone())
                        .ok_or("invalid RGBA dimensions")?;
                    if let Err(error) = img.save(&path) {
                        render_errors.push(
                            json!({"scene": scene, "frame": frame, "reason": error.to_string()}),
                        );
                    } else {
                        editable_buffers.insert(frame, rgba);
                    }
                }
                Err(error) => {
                    render_errors
                        .push(json!({"scene": scene, "frame": frame, "reason": error.to_string()}));
                }
            }
        }

        // Evaluate scripted / MovieClip runtime playback across the planned sweep steps.
        let mut scripted_frames = Vec::with_capacity(frames.len());
        if !has_scripts && !has_independent_mcs {
            let empty_clips = std::collections::HashMap::new();
            for &step_frame in &frames {
                let display_nodes = evaluate_scene_with_frozen_clips(
                    &project,
                    id,
                    step_frame,
                    &empty_clips,
                    &[],
                    false,
                    true,
                )
                .0
                .draws
                .len();
                let label = active_timeline_label(timeline, step_frame);
                scripted_frames.push(json!({
                    "step": step_frame,
                    "playhead_frame": step_frame,
                    "label": label,
                    "display_node_count": display_nodes,
                    "root_halted_by_script": false,
                    "advancing_clip_count": 0,
                    "diverges_from_editable_stage": false,
                    "different_rgba_pixels_vs_editable": 0,
                    "different_visible_pixels_vs_editable": 0,
                    "divergence_causes": [],
                }));
            }
        } else {
            let max_step = frames
                .iter()
                .copied()
                .max()
                .unwrap_or(0)
                .min(MAX_SCRIPTED_SAMPLE_TICKS);
            let mut playback = AuthoredPlayback::with_live_scripts(&project, id, 0, true);
            all_traces.extend(playback.drain_traces());
            all_diagnostics.extend(playback.drain_diagnostics());
            let mut cur_step = 0_u32;

            for &step_frame in &frames {
                let target_step = step_frame.min(max_step);
                while cur_step < target_step {
                    playback.tick(&project);
                    all_traces.extend(playback.drain_traces());
                    all_diagnostics.extend(playback.drain_diagnostics());
                    cur_step += 1;
                }
                let playhead_frame = playback.frame();
                let label = active_timeline_label(timeline, playhead_frame);
                let display_nodes = playback.scene(&project, false, true).draws.len();
                let halted = playback.is_root_halted_by_script();
                let clip_frames = playback.clip_frames();
                let advancing_clip_count = clip_frames.values().filter(|&&f| f > 0).count();
                let state_might_differ = playhead_frame != step_frame
                    || halted
                    || advancing_clip_count > 0
                    || has_scripts;

                let (diff_rgba, diff_vis) = if state_might_differ
                    && let Some(editable_rgba) = editable_buffers.get(&step_frame)
                    && let Ok(runtime_rgba) =
                        render_project_frame_rgba(&project, Some(id), target_step, w, h, true, true)
                {
                    diff_rgba_buffers(editable_rgba, &runtime_rgba)
                } else {
                    (0, 0)
                };

                let mut causes: Vec<&'static str> = Vec::new();
                if playhead_frame != step_frame || halted {
                    causes.push("playhead_redirected_by_script");
                }
                if advancing_clip_count > 0 {
                    causes.push("independent_movie_clip_timeline");
                }
                if diff_rgba > 0 && causes.is_empty() && has_scripts {
                    causes.push("frame_or_instance_script");
                }
                let diverges = diff_rgba > 0 || playhead_frame != step_frame;
                if diverges {
                    total_divergent_frames += 1;
                }
                scripted_frames.push(json!({
                    "step": step_frame,
                    "playhead_frame": playhead_frame,
                    "label": label,
                    "display_node_count": display_nodes,
                    "root_halted_by_script": halted,
                    "advancing_clip_count": advancing_clip_count,
                    "diverges_from_editable_stage": diverges,
                    "different_rgba_pixels_vs_editable": diff_rgba,
                    "different_visible_pixels_vs_editable": diff_vis,
                    "divergence_causes": causes,
                }));
            }
        }
        scripted_scenes.push(json!({
            "scene": scene,
            "planned_frames": frames,
            "frames": scripted_frames,
        }));
    }

    let behavioral_parity =
        compute_behavioral_parity(&project, companion_swf, frame_mode, max_frames_per_scene)?;

    let bg = project.background;
    Ok(json!({
        "evaluation_mode": "editable_stage",
        "frame_mode": frame_mode.as_str(),
        "max_frames_per_scene": max_frames_per_scene,
        "planned_scene_frames": planned_scene_frames,
        "stage": [project.width, project.height, project.fps],
        "background": format!("#{:02x}{:02x}{:02x}", bg[0], bg[1], bg[2]),
        "scenes": scenes,
        "symbols": project.symbols.len(),
        "render_errors": render_errors,
        "scripted_playback": {
            "engine": "AuthoredPlayback",
            "has_scripts": has_scripts,
            "has_independent_movie_clips": has_independent_mcs,
            "divergent_frames": total_divergent_frames,
            "traces": all_traces,
            "diagnostics": all_diagnostics,
            "scenes": scripted_scenes,
        },
        "behavioral_parity": behavioral_parity,
    }))
}

fn parse_sweep_flags(
    tail: &[String],
) -> Result<(FrameSweepMode, usize, Option<PathBuf>), Box<dyn Error>> {
    let mut mode = FrameSweepMode::Sampled;
    let mut max_frames = DEFAULT_MAX_FRAMES_PER_SCENE;
    let mut companion_swf: Option<PathBuf> = None;
    let mut i = 0;
    while i < tail.len() {
        match tail[i].as_str() {
            "--frame-mode" => {
                let val = tail.get(i + 1).ok_or("missing value for --frame-mode")?;
                mode = FrameSweepMode::parse(val)?;
                i += 2;
            }
            "--max-frames-per-scene" => {
                let val = tail
                    .get(i + 1)
                    .ok_or("missing value for --max-frames-per-scene")?;
                max_frames = val.parse::<usize>()?.max(1);
                i += 2;
            }
            "--companion-swf" => {
                let val = tail.get(i + 1).ok_or("missing value for --companion-swf")?;
                companion_swf = Some(PathBuf::from(val));
                i += 2;
            }
            other => {
                return Err(format!("unrecognized option {other:?}").into());
            }
        }
    }
    Ok((mode, max_frames, companion_swf))
}

fn plan_frames_cmd(
    source: &Path,
    frame_mode: FrameSweepMode,
    max_frames_per_scene: usize,
) -> Result<Value, Box<dyn Error>> {
    let project = import_project(source)?;
    project.validate()?;
    let ids = if project.scenes.is_empty() {
        std::slice::from_ref(&project.root)
    } else {
        &project.scenes
    };
    let mut scenes = Vec::with_capacity(ids.len());
    for (idx, id) in ids.iter().enumerate() {
        let timeline = &project.symbol(id).ok_or("missing scene")?.timeline;
        let frames = plan_scene_frames(timeline, frame_mode, max_frames_per_scene);
        scenes.push(json!({
            "scene": idx,
            "frame_count": timeline.frame_count,
            "planned_frames": frames,
        }));
    }
    Ok(json!({
        "frame_mode": frame_mode.as_str(),
        "max_frames_per_scene": max_frames_per_scene,
        "scenes": scenes,
    }))
}

fn behavioral_parity_cmd(
    source: &Path,
    companion_swf: Option<&Path>,
    frame_mode: FrameSweepMode,
    max_frames_per_scene: usize,
) -> Result<Value, Box<dyn Error>> {
    let project = import_project(source)?;
    project.validate()?;
    compute_behavioral_parity(&project, companion_swf, frame_mode, max_frames_per_scene)
}

fn compare_pixels(native: &Path, adobe: &Path) -> Result<Value, Box<dyn Error>> {
    let native = image::open(native)?.to_rgba8();
    let adobe = image::open(adobe)?.to_rgba8();
    if native.dimensions() != adobe.dimensions() {
        return Ok(json!({
            "status": "different_dimensions",
            "native": native.dimensions(),
            "adobe": adobe.dimensions()
        }));
    }
    let total_pixels = u64::from(native.width()) * u64::from(native.height());
    let mut different = 0_u64;
    let mut visible_differences = 0_u64;
    let mut abs_sum = 0_u64;
    for (a, b) in native.pixels().zip(adobe.pixels()) {
        if a != b {
            different += 1;
            if a[3] != 0 || b[3] != 0 {
                visible_differences += 1;
            }
            for c in 0..4 {
                abs_sum += u64::from(a.0[c].abs_diff(b.0[c]));
            }
        }
    }
    let rgba_mae = if total_pixels > 0 {
        ((abs_sum as f64) / ((total_pixels * 4) as f64) * 1_000_000.0).round() / 1_000_000.0
    } else {
        0.0
    };
    Ok(json!({
        "pixels": total_pixels,
        "different_rgba_pixels": different,
        "different_visible_pixels": visible_differences,
        "rgba_mae": rgba_mae,
    }))
}

fn static_swf(source: &Path, reference: &Path, output: &Path) -> Result<Value, Box<dyn Error>> {
    let project = import_project(source)?;
    project.validate()?;
    let w = project.width.max(1);
    let h = project.height.max(1);
    let rgba = render_project_frame_rgba(&project, None, 0, w, h, false, false)?;
    let img = image::RgbaImage::from_raw(w, h, rgba).ok_or("invalid RGBA dimensions")?;
    img.save(output)?;
    compare_pixels(output, reference)
}

fn playback(source: &Path, ticks: usize) -> Result<Value, Box<dyn Error>> {
    let mut player = SwfBytecodePlayer::from_path(source)?;
    let mut traces = Vec::new();
    let mut diagnostics = Vec::new();
    for _ in 0..ticks {
        let frame = player.step_frame();
        traces.extend(frame.traces);
        diagnostics.extend(frame.unsupported_diagnostics);
    }
    Ok(json!({ "traces": traces, "diagnostics": diagnostics }))
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    let result = match args.as_slice() {
        [mode, file, dir, tail @ ..] if mode == "authored" => {
            let (frame_mode, max_frames, companion_swf) = parse_sweep_flags(tail)?;
            authored(
                Path::new(file),
                Path::new(dir),
                frame_mode,
                max_frames,
                companion_swf.as_deref(),
            )?
        }
        [mode, file, tail @ ..] if mode == "plan-frames" => {
            let (frame_mode, max_frames, _) = parse_sweep_flags(tail)?;
            plan_frames_cmd(Path::new(file), frame_mode, max_frames)?
        }
        [mode, file, tail @ ..] if mode == "behavioral-parity" => {
            let (positional_swf, flag_tail) = if tail.first().is_some_and(|s| !s.starts_with("--"))
            {
                (Some(PathBuf::from(&tail[0])), &tail[1..])
            } else {
                (None, tail)
            };
            let (frame_mode, max_frames, flag_swf) = parse_sweep_flags(flag_tail)?;
            let swf = flag_swf.or(positional_swf);
            behavioral_parity_cmd(Path::new(file), swf.as_deref(), frame_mode, max_frames)?
        }
        [mode, native, adobe] if mode == "pixels" => {
            compare_pixels(Path::new(native), Path::new(adobe))?
        }
        [mode, file, reference, output] if mode == "static-swf" => {
            static_swf(Path::new(file), Path::new(reference), Path::new(output))?
        }
        [mode, file, ticks] if mode == "playback" => playback(Path::new(file), ticks.parse()?)?,
        _ => {
            return Err(
                "Expected authored INPUT OUTPUT_DIR [--frame-mode sampled|keyframes-and-midpoints] [--max-frames-per-scene N] [--companion-swf SWF] | plan-frames INPUT [...] | behavioral-parity INPUT [SWF] [...] | pixels NATIVE ADOBE | playback SWF NUM_FRAMES | static-swf SWF REFERENCE OUTPUT"
                    .into(),
            );
        }
    };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
