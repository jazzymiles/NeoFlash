//! Inspect editable imports, render static authoring-stage frames matching Adobe Animate's
//! `t.currentFrame = f; doc.exportPNG(..., true, true)`, and separately evaluate scripted /
//! `MovieClip` runtime playback so corpus reports distinguish authoring composition from runtime
//! timeline advancement.
use neoflash_core::{AuthoredPlayback, ElementKind, InstancePlayback, Project, SymbolKind};
use neoflash_export::render_project_frame_rgba;
use neoflash_import::{SwfBytecodePlayer, import_project};
use serde_json::{Value, json};
use std::{collections::BTreeMap, env, error::Error, path::Path};

const MAX_SCRIPTED_SAMPLE_TICKS: u32 = 4096;

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

fn authored(source: &Path, destination: &Path) -> Result<Value, Box<dyn Error>> {
    let project = import_project(source)?;
    project.validate()?;
    let w = project.width.max(1);
    let h = project.height.max(1);
    let has_scripts = project_has_any_scripts(&project);
    let has_independent_mcs = project_has_multiframe_independent_clips(&project);

    let mut scenes = Vec::new();
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
        let last = timeline
            .frame_count
            .checked_sub(1)
            .ok_or("empty timeline")?;
        let frames = sampled_scene_frames(last);
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

        // Evaluate scripted / MovieClip runtime playback across the same sampled steps.
        let mut scripted_frames = Vec::with_capacity(frames.len());
        if !has_scripts && !has_independent_mcs {
            for &step_frame in &frames {
                scripted_frames.push(json!({
                    "step": step_frame,
                    "playhead_frame": step_frame,
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
            "frames": scripted_frames,
        }));
    }

    let bg = project.background;
    Ok(json!({
        "evaluation_mode": "editable_stage",
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
    }))
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
        [mode, file, dir] if mode == "authored" => authored(Path::new(file), Path::new(dir))?,
        [mode, native, adobe] if mode == "pixels" => {
            compare_pixels(Path::new(native), Path::new(adobe))?
        }
        [mode, file, reference, output] if mode == "static-swf" => {
            static_swf(Path::new(file), Path::new(reference), Path::new(output))?
        }
        [mode, file, ticks] if mode == "playback" => playback(Path::new(file), ticks.parse()?)?,
        _ => {
            return Err(
                "Expected authored INPUT OUTPUT_DIR or pixels NATIVE ADOBE or playback SWF NUM_FRAMES or static-swf SWF REFERENCE OUTPUT"
                    .into(),
            );
        }
    };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
