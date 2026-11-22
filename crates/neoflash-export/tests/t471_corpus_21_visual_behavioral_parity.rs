//! T-471 (`imports-corpus:per-file-comparison`): Complete, reproducible 21 / 21 per-file
//! multi-frame visual and behavioral Flash comparison across every entry in
//! `examples/fla-corpus/manifest.json`.
//!
//! For all 21 public licensed corpus inputs (8 legacy binary `.fla` + 10 `.fla` XFL archives +
//! 3 `.xfl` directory companions):
//! 1. Imports from source via `neoflash_import::import_project` and validates `Project::validate()`.
//! 2. Enumerates every scene (`project.scene_ids()`), keyframe start frame, tween midpoint,
//!    keyframe span terminal frame, frame-script frame, and synchronized `Graphic` cycle boundary,
//!    plus multi-frame library symbol timelines (`Button` Up/Over/Down/Hit and Flump/nested clips).
//! 3. Evaluates static authoring composition via `AuthoredScene::evaluate_editable_stage_with_filters`
//!    and renders multi-frame RGBA bitmaps via `neoflash_export::render_project_frame_rgba`, checking
//!    draw counts, world-space geometry bounding boxes, non-background RGBA pixel coverage, pixel
//!    bounding boxes, perceptual hashes, and exact RGBA checksums against checked-in
//!    `examples/fla-corpus/behavioral-oracles.json`.
//! 4. Evaluates multi-frame runtime playback via `AuthoredPlayback::with_live_scripts`, checking
//!    playhead trajectories, `stop()`/`gotoAndPlay` control flow, `trace()` output, explicit
//!    `editable_vs_playback_diverges` semantics (`MovieClip` vs `Graphic`), and companion `.swf`
//!    execution via `SwfBytecodePlayer` across all 8 companion SWFs (including 1:1 `output.txt`
//!    Flash Player trace parity on all 5 trace-bearing suites and morph-ratio progression on MX).
//! 5. Verifies pinned Adobe Animate 27.0.1.5 (`MAC 27,0,1,5`) stage, scene-frame, SHA-256, and
//!    per-frame RGBA MAE bounds whenever `target/artifacts/t463/pinned-final` is available on disk,
//!    gracefully skipping only the disk-only PNG pixel diff in clean CI without `target/artifacts`.

use neoflash_core::{
    AuthoredDraw, AuthoredPlayback, AuthoredScene, ElementKind, InstancePlayback, OvalOptions,
    Project, RectangleOptions, Symbol, SymbolKind, oval_path_commands, path_bounds,
    rectangle_path_commands,
};
use neoflash_export::{render_project_frame_rgba, sha256_hex};
use neoflash_import::{SwfBytecodePlayer, SwfVmKind, import_project};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BehavioralOraclesManifest {
    schema_version: u32,
    corpus_manifest: String,
    total_cases: usize,
    adobe_reference_host: String,
    description: String,
    summary: OracleSummary,
    cases: Vec<CorpusCaseOracle>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct OracleSummary {
    cases: usize,
    formats: BTreeMap<String, usize>,
    total_scenes: usize,
    total_sampled_scene_frames: usize,
    total_sampled_library_frames: usize,
    editable_vs_playback_divergent_cases: usize,
    authored_trace_cases: usize,
    companion_swf_cases: usize,
    companion_swf_trace_oracle_cases: usize,
    pinned_adobe_cases: usize,
    pinned_adobe_frames: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CorpusCaseOracle {
    id: String,
    source: String,
    path: String,
    format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    identical_to_case: Option<String>,
    stage: StageOracle,
    scenes: Vec<SceneOracle>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    library_timeline_probes: Vec<LibraryTimelineOracle>,
    authored_playback: AuthoredPlaybackOracle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    companion_swf: Option<CompanionSwfOracle>,
    pinned_adobe_animate: PinnedAdobeOracle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct StageOracle {
    width: u32,
    height: u32,
    fps: u32,
    background: [u8; 4],
    symbol_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct FrameLabelOracle {
    frame: u32,
    label: String,
    label_type: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SceneOracle {
    index: usize,
    symbol_id: String,
    name: String,
    frame_count: u32,
    layer_count: usize,
    keyframe_starts: Vec<u32>,
    tween_midpoints: Vec<u32>,
    terminal_frames: Vec<u32>,
    sampled_frames: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    labels: Vec<FrameLabelOracle>,
    editable_frames: Vec<EditableFrameOracle>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct EditableFrameOracle {
    frame: u32,
    draw_count: usize,
    clip_count: usize,
    filter_node_count: usize,
    has_device_text: bool,
    draw_bbox: Option<[f64; 4]>,
    visible_pixels: u64,
    visible_coverage: f64,
    pixel_bbox: Option<[u32; 4]>,
    mean_visible_rgba: [f64; 4],
    perceptual_ahash64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rgba_fnv1a64: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LibraryTimelineOracle {
    symbol_id: String,
    name: String,
    kind: String,
    frame_count: u32,
    sampled_frames: Vec<u32>,
    frames: Vec<LibraryFrameProbe>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LibraryFrameProbe {
    frame: u32,
    draw_count: usize,
    clip_count: usize,
    filter_node_count: usize,
    draw_bbox: Option<[f64; 4]>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AuthoredPlaybackOracle {
    ticks_run: u32,
    editable_vs_playback_diverges: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    divergence_reasons: Vec<String>,
    final_playhead_frame: u32,
    root_halted_by_script: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    traces: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<String>,
    trajectory: Vec<PlaybackStepOracle>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PlaybackStepOracle {
    tick: u32,
    playhead_frame: u32,
    root_halted_by_script: bool,
    advancing_clip_count: usize,
    diverges_from_editable_stage: bool,
    diff_rgba_pixels_vs_editable: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CompanionSwfOracle {
    path: String,
    swf_version: u8,
    vm_kind: String,
    total_frames: u32,
    ticks_executed: usize,
    frame_display_counts: Vec<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    placed_ratios: Vec<u16>,
    final_playing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_trace_file: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    traces: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    matches_authored_playback_traces: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    unsupported_diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PinnedAdobeOracle {
    host: String,
    stage_matches: bool,
    background_matches: bool,
    frames: Vec<PinnedAdobeFrameOracle>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PinnedAdobeFrameOracle {
    scene: usize,
    frame: u32,
    adobe_sha256: String,
    max_rgba_mae: f64,
    max_different_rgba_pixels: u64,
}

struct MeasuredCase {
    oracle: CorpusCaseOracle,
    editable_rgba_by_scene_frame: BTreeMap<(usize, u32), Vec<u8>>,
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical workspace root")
}

fn corpus_dir() -> PathBuf {
    workspace_root().join("examples/fla-corpus")
}

/// Resolves `target/artifacts/t463/pinned-final` in either a primary checkout or a git worktree
/// without hardcoding host paths. Returns `None` in clean CI environments without `target/artifacts`.
fn resolve_pinned_adobe_dir() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("NEOFLASH_PINNED_ADOBE_DIR") {
        if explicit.is_empty() {
            return None;
        }
        let p = PathBuf::from(explicit);
        return p.join("inputs.json").is_file().then_some(p);
    }
    let root = workspace_root();
    let local = root.join("target/artifacts/t463/pinned-final");
    if local.join("inputs.json").is_file() {
        return Some(local);
    }
    let dot_git = root.join(".git");
    if dot_git.is_file()
        && let Ok(text) = fs::read_to_string(&dot_git)
        && let Some(gitdir_raw) = text.trim().strip_prefix("gitdir:")
    {
        let gitdir_path = {
            let p = PathBuf::from(gitdir_raw.trim());
            if p.is_absolute() { p } else { root.join(p) }
        };
        if let Ok(commondir_raw) = fs::read_to_string(gitdir_path.join("commondir")) {
            let common_git = gitdir_path.join(commondir_raw.trim());
            if let Ok(canon_common) = common_git.canonicalize()
                && let Some(main_root) = canon_common.parent()
            {
                let shared = main_root.join("target/artifacts/t463/pinned-final");
                if shared.join("inputs.json").is_file() {
                    return Some(shared);
                }
            }
        }
    }
    None
}

fn round3(v: f64) -> f64 {
    (v * 1_000.0).round() / 1_000.0
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn round6(v: f64) -> f64 {
    (v * 1_000_000.0).round() / 1_000_000.0
}

fn fnv1a64_hex(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn perceptual_ahash64_hex(rgba: &[u8], width: u32, height: u32) -> String {
    let w = width.max(1) as usize;
    let h = height.max(1) as usize;
    let mut grid = [0.0_f64; 64];
    for gy in 0..8 {
        let y0 = (gy * h) / 8;
        let y1 = (((gy + 1) * h) / 8).max(y0 + 1).min(h);
        for gx in 0..8 {
            let x0 = (gx * w) / 8;
            let x1 = (((gx + 1) * w) / 8).max(x0 + 1).min(w);
            let mut sum = 0.0_f64;
            let mut count = 0_usize;
            for y in y0..y1 {
                for x in x0..x1 {
                    let idx = (y * w + x) * 4;
                    let r = f64::from(rgba[idx]);
                    let g = f64::from(rgba[idx + 1]);
                    let b = f64::from(rgba[idx + 2]);
                    let a = f64::from(rgba[idx + 3]) / 255.0;
                    sum += (0.299 * r + 0.587 * g + 0.114 * b) * a;
                    count += 1;
                }
            }
            grid[gy * 8 + gx] = if count > 0 { sum / (count as f64) } else { 0.0 };
        }
    }
    let mean = grid.iter().sum::<f64>() / 64.0;
    let mut bits = 0_u64;
    for (i, &val) in grid.iter().enumerate() {
        if val > mean + 1e-6 {
            bits |= 1_u64 << i;
        }
    }
    format!("{bits:016x}")
}

fn ahash_hamming(a_hex: &str, b_hex: &str) -> u32 {
    let a = u64::from_str_radix(a_hex, 16).unwrap_or(0);
    let b = u64::from_str_radix(b_hex, 16).unwrap_or(0);
    (a ^ b).count_ones()
}

struct RgbaStats {
    visible_pixels: u64,
    visible_coverage: f64,
    pixel_bbox: Option<[u32; 4]>,
    mean_visible_rgba: [f64; 4],
    perceptual_ahash64: String,
}

fn compute_rgba_stats(rgba: &[u8], width: u32, height: u32) -> RgbaStats {
    let w = width.max(1);
    let h = height.max(1);
    assert_eq!(rgba.len(), (w as usize) * (h as usize) * 4);
    let mut visible_pixels = 0_u64;
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0_u32;
    let mut max_y = 0_u32;
    let mut sum = [0_u64; 4];

    for y in 0..h {
        for x in 0..w {
            let idx = ((y * w + x) as usize) * 4;
            let a = rgba[idx + 3];
            if a > 0 {
                visible_pixels += 1;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
                sum[0] += u64::from(rgba[idx]);
                sum[1] += u64::from(rgba[idx + 1]);
                sum[2] += u64::from(rgba[idx + 2]);
                sum[3] += u64::from(a);
            }
        }
    }

    let total_px = f64::from(w) * f64::from(h);
    let visible_coverage = round6((visible_pixels as f64) / total_px);
    let pixel_bbox = if visible_pixels > 0 {
        Some([min_x, min_y, max_x, max_y])
    } else {
        None
    };
    let mean_visible_rgba = if visible_pixels > 0 {
        let vp = visible_pixels as f64;
        [
            round2((sum[0] as f64) / vp),
            round2((sum[1] as f64) / vp),
            round2((sum[2] as f64) / vp),
            round2((sum[3] as f64) / vp),
        ]
    } else {
        [0.0, 0.0, 0.0, 0.0]
    };

    RgbaStats {
        visible_pixels,
        visible_coverage,
        pixel_bbox,
        mean_visible_rgba,
        perceptual_ahash64: perceptual_ahash64_hex(rgba, w, h),
    }
}

fn local_element_bounds(kind: &ElementKind) -> Option<[f64; 4]> {
    match kind {
        ElementKind::Rectangle {
            width,
            height,
            stroke,
            stroke_width,
            ..
        } => {
            let cmds = rectangle_path_commands(*width, *height, RectangleOptions::default());
            let b = path_bounds(&cmds).ok().flatten()?;
            let pad = if stroke[3] > 0 && *stroke_width > 0.0 {
                stroke_width * 0.5
            } else {
                0.0
            };
            Some([b.x_min - pad, b.y_min - pad, b.x_max + pad, b.y_max + pad])
        }
        ElementKind::Ellipse {
            width,
            height,
            stroke,
            stroke_width,
            ..
        } => {
            let cmds = oval_path_commands(*width, *height, OvalOptions::default());
            let b = path_bounds(&cmds).ok().flatten()?;
            let pad = if stroke[3] > 0 && *stroke_width > 0.0 {
                stroke_width * 0.5
            } else {
                0.0
            };
            Some([b.x_min - pad, b.y_min - pad, b.x_max + pad, b.y_max + pad])
        }
        ElementKind::Path {
            commands,
            stroke,
            stroke_width,
            ..
        } => {
            let b = path_bounds(commands).ok().flatten()?;
            let pad = if stroke[3] > 0 && *stroke_width > 0.0 {
                stroke_width * 0.5
            } else {
                0.0
            };
            Some([b.x_min - pad, b.y_min - pad, b.x_max + pad, b.y_max + pad])
        }
        ElementKind::PaintedPath {
            commands,
            stroke,
            stroke_gradient,
            stroke_width,
            ..
        } => {
            let b = path_bounds(commands).ok().flatten()?;
            let has_stroke = (stroke[3] > 0 || stroke_gradient.is_some()) && *stroke_width > 0.0;
            let pad = if has_stroke { stroke_width * 0.5 } else { 0.0 };
            Some([b.x_min - pad, b.y_min - pad, b.x_max + pad, b.y_max + pad])
        }
        ElementKind::Bitmap { width, height, .. } => {
            Some([0.0, 0.0, f64::from(*width), f64::from(*height)])
        }
        ElementKind::Video { width, height } => Some([0.0, 0.0, *width, *height]),
        ElementKind::TextBox { width, height, .. } => {
            Some([0.0, 0.0, (*width).max(1.0), (*height).max(1.0)])
        }
        ElementKind::Text {
            text, font_size, ..
        } => {
            let chars = text.chars().count().max(1) as f64;
            let sz = (*font_size).max(1.0);
            Some([0.0, -sz, chars * sz * 0.6, sz * 0.25])
        }
        ElementKind::Instance { .. } => None,
    }
}

fn compute_scene_draw_bbox(draws: &[AuthoredDraw]) -> Option<[f64; 4]> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut any = false;

    for draw in draws {
        let Some([lx0, ly0, lx1, ly1]) = local_element_bounds(&draw.item.kind) else {
            continue;
        };
        let m = draw.item.transform;
        for (lx, ly) in [(lx0, ly0), (lx1, ly0), (lx0, ly1), (lx1, ly1)] {
            let wx = m[0] * lx + m[2] * ly + m[4];
            let wy = m[1] * lx + m[3] * ly + m[5];
            if wx.is_finite() && wy.is_finite() {
                min_x = min_x.min(wx);
                min_y = min_y.min(wy);
                max_x = max_x.max(wx);
                max_y = max_y.max(wy);
                any = true;
            }
        }
    }

    any.then(|| [round3(min_x), round3(min_y), round3(max_x), round3(max_y)])
}

fn scene_has_unembedded_device_text(project: &Project, draws: &[AuthoredDraw]) -> bool {
    let has_font_resources = project.resources.iter().any(|r| {
        r.media_type.starts_with("font/")
            || r.media_type.contains("opentype")
            || r.media_type.contains("truetype")
    });
    draws.iter().any(|d| match &d.item.kind {
        ElementKind::Text { text, .. } | ElementKind::TextBox { text, .. } => {
            !text.trim().is_empty() && !has_font_resources
        }
        _ => false,
    })
}

fn enumerate_scene_frames(
    project: &Project,
    symbol: &Symbol,
) -> (Vec<u32>, Vec<u32>, Vec<u32>, Vec<u32>) {
    let fc = symbol.timeline.frame_count.max(1);
    let last = fc - 1;
    let mut kf_starts = BTreeSet::new();
    let mut tween_mids = BTreeSet::new();
    let mut terminals = BTreeSet::new();
    terminals.insert(last);

    let mut sync_graphic_boundaries = BTreeSet::new();
    for layer in &symbol.timeline.layers {
        for (idx, kf) in layer.keyframes.iter().enumerate() {
            if kf.frame < fc {
                kf_starts.insert(kf.frame);
            }
            let next_start = layer.keyframes.get(idx + 1).map_or(fc, |n| n.frame).min(fc);
            if next_start > kf.frame {
                terminals.insert(next_start - 1);
            }
            if (kf.tween.is_some() || kf.keyed_properties.is_some()) && next_start > kf.frame + 1 {
                tween_mids.insert(kf.frame + (next_start - kf.frame) / 2);
            }
            for el in &kf.elements {
                if let ElementKind::Instance {
                    symbol_id,
                    playback,
                } = &el.kind
                    && let Some(child_sym) = project.symbol(symbol_id)
                {
                    let eff_kind = el.instance_behavior.unwrap_or(child_sym.kind);
                    let child_fc = child_sym.timeline.frame_count;
                    if eff_kind == SymbolKind::Graphic
                        && child_fc > 1
                        && !matches!(playback, InstancePlayback::SingleFrame(_))
                        && kf.frame + child_fc < next_start
                    {
                        sync_graphic_boundaries.insert(kf.frame + child_fc / 2);
                        sync_graphic_boundaries.insert(kf.frame + child_fc - 1);
                        sync_graphic_boundaries.insert(kf.frame + child_fc);
                    }
                }
            }
        }
    }

    let mut sampled = BTreeSet::new();
    sampled.insert(0);
    sampled.insert(last / 2);
    sampled.insert(last);
    sampled.extend(&kf_starts);
    sampled.extend(&tween_mids);
    sampled.extend(&terminals);
    sampled.extend(&sync_graphic_boundaries);
    for fs_entry in &symbol.timeline.scripts {
        if !fs_entry.script.source.trim().is_empty() && fs_entry.frame < fc {
            sampled.insert(fs_entry.frame);
        }
    }

    (
        kf_starts.into_iter().collect(),
        tween_mids.into_iter().collect(),
        terminals.into_iter().collect(),
        sampled.into_iter().collect(),
    )
}

fn enumerate_library_probe_frames(symbol: &Symbol) -> Vec<u32> {
    let fc = symbol.timeline.frame_count.max(1);
    let last = fc - 1;
    if fc <= 6 {
        return (0..fc).collect();
    }
    let mut frames = BTreeSet::from([0, last / 4, last / 2, (last * 3) / 4, last]);
    for layer in &symbol.timeline.layers {
        for (idx, kf) in layer.keyframes.iter().enumerate() {
            if kf.frame < fc && frames.len() < 8 {
                frames.insert(kf.frame);
            }
            let next_start = layer.keyframes.get(idx + 1).map_or(fc, |n| n.frame).min(fc);
            if kf.tween.is_some() && next_start > kf.frame + 1 && frames.len() < 10 {
                frames.insert(kf.frame + (next_start - kf.frame) / 2);
            }
        }
    }
    frames.into_iter().collect()
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

fn project_max_placed_independent_clip_frames(project: &Project) -> u32 {
    let mut max_fc = 0_u32;
    for sym in &project.symbols {
        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    if let ElementKind::Instance {
                        symbol_id,
                        playback,
                    } = &el.kind
                        && let Some(target) = project.symbol(symbol_id)
                    {
                        let eff_kind = el.instance_behavior.unwrap_or(target.kind);
                        let advances_independently = eff_kind == SymbolKind::MovieClip
                            && target.timeline.frame_count > 1
                            && (matches!(playback, InstancePlayback::Independent)
                                || (el.instance_behavior == Some(SymbolKind::MovieClip)
                                    && matches!(playback, InstancePlayback::Loop)));
                        if advances_independently {
                            max_fc = max_fc.max(target.timeline.frame_count);
                        }
                    }
                }
            }
        }
    }
    max_fc
}

fn diff_rgba_pixels(a: &[u8], b: &[u8]) -> u64 {
    assert_eq!(a.len(), b.len());
    a.as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0.iter())
        .filter(|(pa, pb)| pa != pb)
        .count() as u64
}

fn compare_rgba_mae(a: &[u8], b: &[u8]) -> (u64, f64) {
    assert_eq!(a.len(), b.len());
    let total_pixels = (a.len() / 4) as u64;
    let mut diff_px = 0_u64;
    let mut abs_sum = 0_u64;
    for (pa, pb) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0.iter()) {
        if pa != pb {
            diff_px += 1;
            for c in 0..4 {
                abs_sum += u64::from(pa[c].abs_diff(pb[c]));
            }
        }
    }
    let mae = if total_pixels > 0 {
        round6((abs_sum as f64) / ((total_pixels * 4) as f64))
    } else {
        0.0
    };
    (diff_px, mae)
}

fn parse_test_toml_ticks(path: &Path) -> Option<usize> {
    let text = fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        for key in ["num_frames", "num_ticks"] {
            if let Some(rest) = trimmed.strip_prefix(key) {
                let val = rest.trim().strip_prefix('=')?.trim();
                if let Ok(n) = val.parse::<usize>() {
                    return Some(n);
                }
            }
        }
    }
    None
}

fn collect_morph_ratios(entries: &[neoflash_import::SwfDisplayEntry], out: &mut BTreeSet<u16>) {
    for entry in entries {
        if let Some(r) = entry.ratio {
            out.insert(r);
        }
        collect_morph_ratios(&entry.children, out);
    }
}

fn symbol_kind_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::MovieClip => "movie_clip",
        SymbolKind::Graphic => "graphic",
        SymbolKind::Button => "button",
    }
}

fn label_type_name(lt: neoflash_core::FrameLabelType) -> &'static str {
    match lt {
        neoflash_core::FrameLabelType::Name => "name",
        neoflash_core::FrameLabelType::Comment => "comment",
        neoflash_core::FrameLabelType::Anchor => "anchor",
    }
}

fn identical_pair_partner(case_id: &str) -> Option<String> {
    case_id
        .strip_suffix("-xfl")
        .map(|stem| format!("{stem}-fla"))
}

fn measure_all_corpus_cases() -> &'static Vec<MeasuredCase> {
    static MEASURED: OnceLock<Vec<MeasuredCase>> = OnceLock::new();
    MEASURED.get_or_init(|| {
        let corpus = corpus_dir();
        let manifest_val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(corpus.join("manifest.json")).unwrap())
                .expect("parse manifest.json");
        let cases = manifest_val["cases"].as_array().expect("manifest cases");
        assert_eq!(
            cases.len(),
            21,
            "manifest must contain all 21 public corpus cases"
        );

        let pinned_dir = resolve_pinned_adobe_dir();
        let mut out = Vec::with_capacity(cases.len());

        for case in cases {
            let case_id = case["id"].as_str().unwrap().to_owned();
            let source_kind = case["source"].as_str().unwrap().to_owned();
            let rel_path = case["path"].as_str().unwrap().to_owned();
            let format = case["expected"]["format"].as_str().unwrap().to_owned();
            let source_path = corpus.join(&rel_path);

            let project = import_project(&source_path)
                .unwrap_or_else(|e| panic!("import_project failed for {case_id}: {e}"));
            project
                .validate()
                .unwrap_or_else(|e| panic!("project.validate() failed for {case_id}: {e}"));

            let w = project.width.max(1);
            let h = project.height.max(1);
            let scene_ids = project.scene_ids();
            assert!(
                !scene_ids.is_empty(),
                "{case_id}: must have at least one scene"
            );

            let scene_id_set: HashSet<&str> = scene_ids.iter().map(String::as_str).collect();
            let mut editable_rgba_by_scene_frame: BTreeMap<(usize, u32), Vec<u8>> = BTreeMap::new();
            let mut scene_oracles = Vec::with_capacity(scene_ids.len());

            for (scene_idx, scene_id) in scene_ids.iter().enumerate() {
                let sym = project
                    .symbol(scene_id)
                    .unwrap_or_else(|| panic!("{case_id}: missing scene symbol {scene_id}"));
                let (kf_starts, tween_mids, terminals, sampled_frames) =
                    enumerate_scene_frames(&project, sym);

                let mut labels = Vec::new();
                for layer in &sym.timeline.layers {
                    for kf in &layer.keyframes {
                        if !kf.label.is_empty() {
                            labels.push(FrameLabelOracle {
                                frame: kf.frame,
                                label: kf.label.clone(),
                                label_type: label_type_name(kf.label_type).to_owned(),
                            });
                        }
                    }
                }
                labels.sort_by(|a, b| a.frame.cmp(&b.frame).then_with(|| a.label.cmp(&b.label)));

                let mut editable_frames = Vec::with_capacity(sampled_frames.len());
                for &frame in &sampled_frames {
                    let (scene_eval, filter_nodes) =
                        AuthoredScene::evaluate_editable_stage_with_filters(
                            &project, scene_id, frame, false, true,
                        );
                    for draw in &scene_eval.draws {
                        assert!(
                            draw.item.transform.iter().all(|v| v.is_finite()),
                            "{case_id} scene {scene_idx} frame {frame}: non-finite transform"
                        );
                        assert!(
                            draw.item.alpha.is_finite() && (0.0..=1.0).contains(&draw.item.alpha),
                            "{case_id} scene {scene_idx} frame {frame}: invalid alpha {}",
                            draw.item.alpha
                        );
                    }
                    let has_device_text =
                        scene_has_unembedded_device_text(&project, &scene_eval.draws);
                    let draw_bbox = compute_scene_draw_bbox(&scene_eval.draws);

                    let rgba = render_project_frame_rgba(
                        &project,
                        Some(scene_id),
                        frame,
                        w,
                        h,
                        true,
                        false,
                    )
                    .unwrap_or_else(|e| {
                        panic!("{case_id} scene {scene_idx} frame {frame} render failed: {e}")
                    });
                    let stats = compute_rgba_stats(&rgba, w, h);
                    let rgba_fnv1a64 = (!has_device_text).then(|| fnv1a64_hex(&rgba));
                    editable_rgba_by_scene_frame.insert((scene_idx, frame), rgba);

                    editable_frames.push(EditableFrameOracle {
                        frame,
                        draw_count: scene_eval.draws.len(),
                        clip_count: scene_eval.clips.len(),
                        filter_node_count: filter_nodes.len(),
                        has_device_text,
                        draw_bbox,
                        visible_pixels: stats.visible_pixels,
                        visible_coverage: stats.visible_coverage,
                        pixel_bbox: stats.pixel_bbox,
                        mean_visible_rgba: stats.mean_visible_rgba,
                        perceptual_ahash64: stats.perceptual_ahash64,
                        rgba_fnv1a64,
                    });
                }

                scene_oracles.push(SceneOracle {
                    index: scene_idx,
                    symbol_id: scene_id.clone(),
                    name: sym.name.clone(),
                    frame_count: sym.timeline.frame_count,
                    layer_count: sym.timeline.layers.len(),
                    keyframe_starts: kf_starts,
                    tween_midpoints: tween_mids,
                    terminal_frames: terminals,
                    sampled_frames,
                    labels,
                    editable_frames,
                });
            }

            // Multi-frame non-scene library symbol probes (e.g., TestButton Up/Over/Down/Hit and Flump animations).
            let mut library_timeline_probes = Vec::new();
            for sym in project
                .symbols
                .iter()
                .filter(|s| !scene_id_set.contains(s.id.as_str()) && s.timeline.frame_count > 1)
                .take(6)
            {
                let probe_frames = enumerate_library_probe_frames(sym);
                let mut frames = Vec::with_capacity(probe_frames.len());
                for &frame in &probe_frames {
                    let (eval, filter_nodes) = AuthoredScene::evaluate_editable_stage_with_filters(
                        &project, &sym.id, frame, false, true,
                    );
                    for draw in &eval.draws {
                        assert!(
                            draw.item.transform.iter().all(|v| v.is_finite()),
                            "{case_id} library symbol {} frame {frame}: non-finite transform",
                            sym.id
                        );
                    }
                    frames.push(LibraryFrameProbe {
                        frame,
                        draw_count: eval.draws.len(),
                        clip_count: eval.clips.len(),
                        filter_node_count: filter_nodes.len(),
                        draw_bbox: compute_scene_draw_bbox(&eval.draws),
                    });
                }
                library_timeline_probes.push(LibraryTimelineOracle {
                    symbol_id: sym.id.clone(),
                    name: sym.name.clone(),
                    kind: symbol_kind_label(sym.kind).to_owned(),
                    frame_count: sym.timeline.frame_count,
                    sampled_frames: probe_frames,
                    frames,
                });
            }

            // Multi-frame AuthoredPlayback trajectory on primary scene.
            let primary_scene_id = &scene_ids[0];
            let primary_fc = scene_oracles[0].frame_count.max(1);
            let has_scripts = project_has_any_scripts(&project);
            let max_mc_fc = project_max_placed_independent_clip_frames(&project);
            let ticks_run = if primary_fc > 1 {
                primary_fc + 2
            } else if has_scripts || max_mc_fc > 1 {
                max_mc_fc.clamp(4, 16)
            } else {
                3
            };

            let mut checkpoint_ticks: BTreeSet<u32> = BTreeSet::from([
                0,
                1,
                primary_fc / 2,
                primary_fc.saturating_sub(1),
                primary_fc,
                ticks_run,
            ]);
            for &f in &scene_oracles[0].sampled_frames {
                if f <= ticks_run {
                    checkpoint_ticks.insert(f);
                }
            }
            checkpoint_ticks.retain(|&t| t <= ticks_run);

            let mut playback =
                AuthoredPlayback::with_live_scripts(&project, primary_scene_id, 0, true);
            let mut pb_traces = playback.drain_traces();
            let mut pb_diags = playback.drain_diagnostics();
            let mut cur_tick = 0_u32;
            let mut trajectory = Vec::with_capacity(checkpoint_ticks.len());
            let mut reason_set: BTreeSet<String> = BTreeSet::new();

            for &tick in &checkpoint_ticks {
                while cur_tick < tick {
                    playback.tick(&project);
                    pb_traces.extend(playback.drain_traces());
                    pb_diags.extend(playback.drain_diagnostics());
                    cur_tick += 1;
                }
                let playhead_frame = playback.frame();
                let halted = playback.is_root_halted_by_script();
                let advancing_clip_count = playback
                    .clip_frames()
                    .iter()
                    .filter(|(path, cf)| !path.is_empty() && **cf > 0)
                    .count();
                let expected_linear_frame = tick % primary_fc;
                let might_differ = playhead_frame != expected_linear_frame
                    || halted
                    || advancing_clip_count > 0
                    || has_scripts;

                let diff_rgba = if might_differ {
                    let editable_frame = expected_linear_frame;
                    let editable_rgba = editable_rgba_by_scene_frame
                        .entry((0, editable_frame))
                        .or_insert_with(|| {
                            render_project_frame_rgba(
                                &project,
                                Some(primary_scene_id),
                                editable_frame,
                                w,
                                h,
                                true,
                                false,
                            )
                            .expect("render editable fallback frame")
                        });
                    let runtime_rgba = render_project_frame_rgba(
                        &project,
                        Some(primary_scene_id),
                        tick,
                        w,
                        h,
                        true,
                        true,
                    )
                    .expect("render runtime playback frame");
                    diff_rgba_pixels(editable_rgba, &runtime_rgba)
                } else {
                    0
                };

                if playhead_frame != expected_linear_frame {
                    reason_set.insert("playhead_redirected_by_script".to_owned());
                }
                if halted {
                    reason_set.insert("root_halted_by_stop_script".to_owned());
                }
                if advancing_clip_count > 0 {
                    reason_set.insert("independent_movie_clip_timeline".to_owned());
                }
                if diff_rgba > 0 {
                    reason_set.insert("runtime_rgba_differs_from_editable".to_owned());
                }
                let step_diverges = playhead_frame != expected_linear_frame
                    || halted
                    || advancing_clip_count > 0
                    || diff_rgba > 0;

                trajectory.push(PlaybackStepOracle {
                    tick,
                    playhead_frame,
                    root_halted_by_script: halted,
                    advancing_clip_count,
                    diverges_from_editable_stage: step_diverges,
                    diff_rgba_pixels_vs_editable: diff_rgba,
                });
            }

            let editable_vs_playback_diverges =
                trajectory.iter().any(|s| s.diverges_from_editable_stage) || !pb_traces.is_empty();
            if !pb_traces.is_empty() && reason_set.is_empty() {
                reason_set.insert("script_trace_side_effects".to_owned());
            }

            let authored_playback = AuthoredPlaybackOracle {
                ticks_run,
                editable_vs_playback_diverges,
                divergence_reasons: reason_set.into_iter().collect(),
                final_playhead_frame: playback.frame(),
                root_halted_by_script: playback.is_root_halted_by_script(),
                traces: pb_traces.clone(),
                diagnostics: pb_diags,
                trajectory,
            };

            // Companion SWF verification via SwfBytecodePlayer where present in examples/fla-corpus/.
            let companion_swf = if source_path.is_file() {
                let sibling_swf = {
                    let same_stem = source_path.with_extension("swf");
                    if same_stem.is_file() {
                        Some(same_stem)
                    } else if let Some(parent) = source_path.parent() {
                        let test_swf = parent.join("test.swf");
                        test_swf.is_file().then_some(test_swf)
                    } else {
                        None
                    }
                };
                sibling_swf.map(|swf_path| {
                    let rel_swf = swf_path
                        .strip_prefix(&corpus)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    let parent = swf_path.parent().unwrap();
                    let toml_ticks = parse_test_toml_ticks(&parent.join("test.toml"));
                    let output_txt = parent.join("output.txt");
                    let expected_trace_file = output_txt.is_file().then(|| {
                        output_txt
                            .strip_prefix(&corpus)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/")
                    });

                    let mut player = SwfBytecodePlayer::from_path(&swf_path).unwrap_or_else(|e| {
                        panic!("{case_id}: SwfBytecodePlayer load failed: {e}")
                    });
                    // Determine frame count via an initial run or header inspection:
                    let probe_res = SwfBytecodePlayer::from_path(&swf_path)
                        .unwrap()
                        .run_frames(1);
                    let total_frames = probe_res.total_frames;
                    let swf_version = probe_res.swf_version;
                    let vm_kind = match probe_res.vm_kind {
                        SwfVmKind::Avm1 => "avm1".to_owned(),
                        SwfVmKind::Avm2 => "avm2".to_owned(),
                    };
                    let ticks_executed =
                        toml_ticks.unwrap_or_else(|| (total_frames.clamp(1, 60)) as usize);

                    let mut frame_display_counts = Vec::with_capacity(ticks_executed);
                    let mut placed_ratios = BTreeSet::new();
                    let mut swf_traces = Vec::new();
                    let mut unsupported_diagnostics = Vec::new();
                    let mut final_playing = true;

                    for _ in 0..ticks_executed {
                        let st = player.step_frame();
                        frame_display_counts.push(st.display_list.len());
                        collect_morph_ratios(&st.display_list, &mut placed_ratios);
                        swf_traces.extend(st.traces);
                        for d in st.unsupported_diagnostics {
                            if !unsupported_diagnostics.contains(&d) {
                                unsupported_diagnostics.push(d);
                            }
                        }
                        final_playing = st.playing;
                    }

                    if output_txt.is_file() {
                        let expected_lines: Vec<String> = fs::read_to_string(&output_txt)
                            .unwrap()
                            .lines()
                            .map(str::to_owned)
                            .collect();
                        assert_eq!(
                            swf_traces, expected_lines,
                            "{case_id}: SwfBytecodePlayer traces must match upstream output.txt 1:1"
                        );
                    }

                    let matches_authored_playback_traces = expected_trace_file
                        .as_ref()
                        .map(|_| swf_traces == pb_traces);

                    CompanionSwfOracle {
                        path: rel_swf,
                        swf_version,
                        vm_kind,
                        total_frames,
                        ticks_executed,
                        frame_display_counts,
                        placed_ratios: placed_ratios.into_iter().collect(),
                        final_playing,
                        expected_trace_file,
                        traces: swf_traces,
                        matches_authored_playback_traces,
                        unsupported_diagnostics,
                    }
                })
            } else {
                None
            };

            // Pinned Adobe Animate 27.0.1.5 contract (measured live if pinned-final is on disk).
            let pinned_adobe_animate = if let Some(ref adobe_root) = pinned_dir {
                let case_dir = adobe_root.join(&case_id);
                let host = fs::read_to_string(case_dir.join("host.txt"))
                    .unwrap()
                    .trim()
                    .to_owned();
                let stage_tsv = fs::read_to_string(case_dir.join("stage.tsv")).unwrap();
                let parts: Vec<&str> = stage_tsv.trim().split('\t').collect();
                let aw: f64 = parts[0].parse().unwrap();
                let ah: f64 = parts[1].parse().unwrap();
                let afps: f64 = parts[2].parse().unwrap();
                let abg = parts[3].to_ascii_lowercase();
                let nbg = format!(
                    "#{:02x}{:02x}{:02x}",
                    project.background[0], project.background[1], project.background[2]
                );
                let scenes_tsv = fs::read_to_string(case_dir.join("scenes.tsv")).unwrap();
                let adobe_scene_fcs: Vec<u32> = scenes_tsv
                    .lines()
                    .map(|l| l.split('\t').next().unwrap().parse::<u32>().unwrap())
                    .collect();
                let native_scene_fcs: Vec<u32> =
                    scene_oracles.iter().map(|s| s.frame_count).collect();
                let stage_matches = (aw - f64::from(project.width)).abs() < 1e-6
                    && (ah - f64::from(project.height)).abs() < 1e-6
                    && (afps - f64::from(project.fps)).abs() < 1e-6
                    && adobe_scene_fcs == native_scene_fcs;
                let background_matches = abg == nbg;

                let mut frames = Vec::new();
                for (s_idx, &sfc) in adobe_scene_fcs.iter().enumerate() {
                    let last = sfc.saturating_sub(1);
                    let adobe_sample_set = BTreeSet::from([0, last / 2, last]);
                    for f in adobe_sample_set {
                        let png_path = case_dir.join(format!("adobe-s{s_idx}-f{f}.png"));
                        let raw_png = fs::read(&png_path).unwrap();
                        let adobe_sha256 = sha256_hex(&raw_png);
                        let adobe_rgba = image::load_from_memory(&raw_png)
                            .unwrap()
                            .to_rgba8()
                            .into_raw();
                        let native_rgba = editable_rgba_by_scene_frame
                            .get(&(s_idx, f))
                            .expect("sampled_frames includes {0, last/2, last}");
                        let (diff_px, mae) = compare_rgba_mae(native_rgba, &adobe_rgba);
                        // Tight upper bound (+0.05 MAE / +64 px headroom for minor float rounding, exact 0 when exact):
                        let max_rgba_mae = if mae == 0.0 { 0.0 } else { round6(mae + 0.05) };
                        let max_different_rgba_pixels = if diff_px == 0 { 0 } else { diff_px + 64 };
                        frames.push(PinnedAdobeFrameOracle {
                            scene: s_idx,
                            frame: f,
                            adobe_sha256,
                            max_rgba_mae,
                            max_different_rgba_pixels,
                        });
                    }
                }
                PinnedAdobeOracle {
                    host,
                    stage_matches,
                    background_matches,
                    frames,
                }
            } else {
                // Fallback loaded from checked-in behavioral-oracles.json when running in clean CI.
                let checked: BehavioralOraclesManifest = serde_json::from_str(
                    &fs::read_to_string(corpus.join("behavioral-oracles.json"))
                        .expect("behavioral-oracles.json must exist in clean CI"),
                )
                .expect("parse behavioral-oracles.json");
                checked
                    .cases
                    .into_iter()
                    .find(|c| c.id == case_id)
                    .expect("case in behavioral-oracles.json")
                    .pinned_adobe_animate
            };

            out.push(MeasuredCase {
                oracle: CorpusCaseOracle {
                    id: case_id.clone(),
                    source: source_kind,
                    path: rel_path,
                    format,
                    identical_to_case: identical_pair_partner(&case_id),
                    stage: StageOracle {
                        width: project.width,
                        height: project.height,
                        fps: project.fps,
                        background: project.background,
                        symbol_count: project.symbols.len(),
                    },
                    scenes: scene_oracles,
                    library_timeline_probes,
                    authored_playback,
                    companion_swf,
                    pinned_adobe_animate,
                },
                editable_rgba_by_scene_frame,
            });
        }
        out
    })
}

fn build_summary(cases: &[CorpusCaseOracle]) -> OracleSummary {
    let mut formats = BTreeMap::new();
    for c in cases {
        *formats.entry(c.format.clone()).or_insert(0) += 1;
    }
    OracleSummary {
        cases: cases.len(),
        formats,
        total_scenes: cases.iter().map(|c| c.scenes.len()).sum(),
        total_sampled_scene_frames: cases
            .iter()
            .flat_map(|c| &c.scenes)
            .map(|s| s.editable_frames.len())
            .sum(),
        total_sampled_library_frames: cases
            .iter()
            .flat_map(|c| &c.library_timeline_probes)
            .map(|p| p.frames.len())
            .sum(),
        editable_vs_playback_divergent_cases: cases
            .iter()
            .filter(|c| c.authored_playback.editable_vs_playback_diverges)
            .count(),
        authored_trace_cases: cases
            .iter()
            .filter(|c| !c.authored_playback.traces.is_empty())
            .count(),
        companion_swf_cases: cases.iter().filter(|c| c.companion_swf.is_some()).count(),
        companion_swf_trace_oracle_cases: cases
            .iter()
            .filter(|c| {
                c.companion_swf
                    .as_ref()
                    .and_then(|s| s.expected_trace_file.as_ref())
                    .is_some()
            })
            .count(),
        pinned_adobe_cases: cases.len(),
        pinned_adobe_frames: cases
            .iter()
            .map(|c| c.pinned_adobe_animate.frames.len())
            .sum(),
    }
}

fn load_checked_oracles() -> BehavioralOraclesManifest {
    let path = corpus_dir().join("behavioral-oracles.json");
    let text =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("missing {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("invalid JSON in {}: {e}", path.display()))
}

#[test]
fn t471_corpus_21_flash_semantic_invariants() {
    let measured = measure_all_corpus_cases();
    let by_id: BTreeMap<&str, &CorpusCaseOracle> = measured
        .iter()
        .map(|m| (m.oracle.id.as_str(), &m.oracle))
        .collect();

    // 1. MX shape tween interpolates monotonically across start (f0), midpoint (f14), and end (f29),
    //    and its companion DefineMorphShape-MX.swf advances through 28 interior morph ratios.
    let morph = by_id["ruffle-mx-shape-tween"];
    let mf = &morph.scenes[0].editable_frames;
    let mf0 = mf.iter().find(|f| f.frame == 0).unwrap();
    let mf14 = mf.iter().find(|f| f.frame == 14).unwrap();
    let mf29 = mf.iter().find(|f| f.frame == 29).unwrap();
    assert!(
        mf0.visible_pixels > mf14.visible_pixels && mf14.visible_pixels > mf29.visible_pixels,
        "MX shape tween visible area must interpolate across f0 ({}) -> f14 ({}) -> f29 ({})",
        mf0.visible_pixels,
        mf14.visible_pixels,
        mf29.visible_pixels
    );
    let morph_swf = morph.companion_swf.as_ref().unwrap();
    assert_eq!(morph_swf.placed_ratios.len(), 28);
    assert!(
        morph_swf.placed_ratios.windows(2).all(|w| w[0] < w[1]),
        "DefineMorphShape-MX.swf morph ratios must strictly increase"
    );

    // 2. CS4 motion tween sweeps horizontally across all 30 sampled frames (x_min: 29.5 -> 224.28 -> 437.394).
    let cs4 = by_id["flacomdoc-cs4-motion-tween"];
    assert_eq!(cs4.scenes[0].editable_frames.len(), 30);
    let x0 = cs4.scenes[0].editable_frames[0].draw_bbox.unwrap()[0];
    let x14 = cs4.scenes[0].editable_frames[14].draw_bbox.unwrap()[0];
    let x29 = cs4.scenes[0].editable_frames[29].draw_bbox.unwrap()[0];
    assert!(
        x0 < 50.0 && x14 > 200.0 && x29 > 400.0,
        "CS4 motion tween must progress horizontally across stage: {x0} -> {x14} -> {x29}"
    );

    // 3. Primitives and holds drops stage shapes during blank span (48..=59) and restores at return (60..=71).
    let prim = by_id["primitives-and-holds-fla"];
    assert!(!prim.authored_playback.editable_vs_playback_diverges);
    let pf0 = prim.scenes[0]
        .editable_frames
        .iter()
        .find(|f| f.frame == 0)
        .unwrap();
    let pf48 = prim.scenes[0]
        .editable_frames
        .iter()
        .find(|f| f.frame == 48)
        .unwrap();
    let pf59 = prim.scenes[0]
        .editable_frames
        .iter()
        .find(|f| f.frame == 59)
        .unwrap();
    let pf60 = prim.scenes[0]
        .editable_frames
        .iter()
        .find(|f| f.frame == 60)
        .unwrap();
    assert_eq!(pf0.draw_count, 5);
    assert_eq!(pf48.draw_count, 3);
    assert_eq!(pf59.draw_count, 3);
    assert_eq!(pf60.draw_count, 5);
    assert!(pf48.visible_pixels < pf0.visible_pixels);
    assert_eq!(pf60.visible_pixels, pf0.visible_pixels);

    // 4. MovieClip vs Graphic authoring/playback divergence on independent child MovieClips.
    for mc_case in [
        "nested-playback-as2-fla",
        "flump-potion-simple",
        "ruffle-avm2-nested-construction",
    ] {
        let pb = &by_id[mc_case].authored_playback;
        assert!(
            pb.editable_vs_playback_diverges,
            "{mc_case}: independent MovieClip must diverge from static editable_stage"
        );
        assert!(
            pb.divergence_reasons
                .iter()
                .any(|r| r == "independent_movie_clip_timeline"),
            "{mc_case}: expected independent_movie_clip_timeline reason"
        );
        assert!(
            pb.divergence_reasons
                .iter()
                .any(|r| r == "runtime_rgba_differs_from_editable"),
            "{mc_case}: expected runtime_rgba_differs_from_editable reason"
        );
    }

    // 5. AS3 frame scripts execute typed variables, arithmetic, trace(), and stop() at frame 24.
    let as3 = &by_id["frame-scripts-as3-fla"].authored_playback;
    assert_eq!(as3.traces, vec!["AS3: start", "AS3: middle 5", "AS3: done"]);
    assert!(as3.root_halted_by_script);
    assert_eq!(as3.final_playhead_frame, 24);

    // 6. All 5 Ruffle output.txt companion SWFs total 82 exact Flash Player trace lines with 0 unsupported diagnostics.
    let trace_swf_lines: usize = measured
        .iter()
        .filter_map(|m| m.oracle.companion_swf.as_ref())
        .filter(|s| s.expected_trace_file.is_some())
        .map(|s| s.traces.len())
        .sum();
    assert_eq!(trace_swf_lines, 82);
}

#[test]
fn t471_corpus_21_manifest_and_oracle_completeness() {
    let corpus = corpus_dir();
    let manifest_val: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(corpus.join("manifest.json")).unwrap()).unwrap();
    let manifest_cases = manifest_val["cases"].as_array().unwrap();
    let checked = load_checked_oracles();

    assert_eq!(checked.schema_version, 1);
    assert_eq!(checked.corpus_manifest, "manifest.json");
    assert_eq!(checked.total_cases, 21);
    assert_eq!(checked.cases.len(), 21);
    assert_eq!(checked.adobe_reference_host, "MAC 27,0,1,5");

    let expected_ids: Vec<&str> = manifest_cases
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    let oracle_ids: Vec<&str> = checked.cases.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        oracle_ids, expected_ids,
        "behavioral-oracles.json must cover all 21 manifest.json cases in exact order"
    );

    let recomputed_summary = build_summary(&checked.cases);
    assert_eq!(checked.summary, recomputed_summary);
    assert_eq!(checked.summary.cases, 21);
    assert_eq!(checked.summary.companion_swf_cases, 8);
    assert_eq!(checked.summary.companion_swf_trace_oracle_cases, 5);
    assert_eq!(checked.summary.pinned_adobe_cases, 21);
    assert_eq!(checked.summary.pinned_adobe_frames, 42);
}

#[test]
fn t471_corpus_21_multiframe_editable_stage_visual_and_geometry_parity() {
    let checked = load_checked_oracles();
    let measured = measure_all_corpus_cases();
    assert_eq!(measured.len(), 21);

    let by_id: BTreeMap<&str, &MeasuredCase> =
        measured.iter().map(|m| (m.oracle.id.as_str(), m)).collect();

    for (expected, actual_case) in checked.cases.iter().zip(measured.iter()) {
        let cid = &expected.id;
        let actual = &actual_case.oracle;
        assert_eq!(&actual.id, cid);
        assert_eq!(&actual.path, &expected.path);
        assert_eq!(&actual.format, &expected.format);
        assert_eq!(
            actual.stage, expected.stage,
            "{cid}: stage metadata mismatch"
        );
        assert_eq!(
            actual.scenes.len(),
            expected.scenes.len(),
            "{cid}: scene count mismatch"
        );

        for (exp_scene, act_scene) in expected.scenes.iter().zip(actual.scenes.iter()) {
            assert_eq!(
                act_scene.symbol_id, exp_scene.symbol_id,
                "{cid}: scene symbol_id"
            );
            assert_eq!(act_scene.name, exp_scene.name, "{cid}: scene name");
            assert_eq!(
                act_scene.frame_count, exp_scene.frame_count,
                "{cid}: frame_count"
            );
            assert_eq!(
                act_scene.layer_count, exp_scene.layer_count,
                "{cid}: layer_count"
            );
            assert_eq!(
                act_scene.keyframe_starts, exp_scene.keyframe_starts,
                "{cid}: keyframe_starts"
            );
            assert_eq!(
                act_scene.tween_midpoints, exp_scene.tween_midpoints,
                "{cid}: tween_midpoints"
            );
            assert_eq!(
                act_scene.terminal_frames, exp_scene.terminal_frames,
                "{cid}: terminal_frames"
            );
            assert_eq!(
                act_scene.sampled_frames, exp_scene.sampled_frames,
                "{cid}: sampled_frames"
            );
            assert_eq!(act_scene.labels, exp_scene.labels, "{cid}: frame labels");
            assert_eq!(
                act_scene.editable_frames.len(),
                exp_scene.editable_frames.len(),
                "{cid}: editable_frames len"
            );

            for (ef, af) in exp_scene
                .editable_frames
                .iter()
                .zip(act_scene.editable_frames.iter())
            {
                let f = ef.frame;
                assert_eq!(af.frame, f, "{cid}: frame index");
                assert_eq!(af.draw_count, ef.draw_count, "{cid} f{f}: draw_count");
                assert_eq!(af.clip_count, ef.clip_count, "{cid} f{f}: clip_count");
                assert_eq!(
                    af.filter_node_count, ef.filter_node_count,
                    "{cid} f{f}: filter_node_count"
                );
                assert_eq!(
                    af.has_device_text, ef.has_device_text,
                    "{cid} f{f}: has_device_text"
                );
                assert_eq!(af.draw_bbox, ef.draw_bbox, "{cid} f{f}: draw_bbox");

                if !ef.has_device_text {
                    assert_eq!(
                        af.visible_pixels, ef.visible_pixels,
                        "{cid} f{f}: exact visible_pixels on non-device-text frame"
                    );
                    assert_eq!(
                        af.pixel_bbox, ef.pixel_bbox,
                        "{cid} f{f}: exact pixel_bbox on non-device-text frame"
                    );
                    assert_eq!(
                        af.perceptual_ahash64, ef.perceptual_ahash64,
                        "{cid} f{f}: exact perceptual_ahash64 on non-device-text frame"
                    );
                    assert_eq!(
                        af.rgba_fnv1a64, ef.rgba_fnv1a64,
                        "{cid} f{f}: exact rgba_fnv1a64 on non-device-text frame"
                    );
                } else if ef.visible_pixels == 0 {
                    assert_eq!(af.visible_pixels, 0, "{cid} f{f}: expected blank frame");
                    assert_eq!(af.pixel_bbox, None);
                } else {
                    assert!(
                        af.visible_pixels > 0,
                        "{cid} f{f}: expected visible rendered pixels"
                    );
                    let ratio = (af.visible_pixels as f64) / (ef.visible_pixels as f64);
                    assert!(
                        (0.55..=1.45).contains(&ratio),
                        "{cid} f{f}: device-text visible_pixels {} outside tolerance of {}",
                        af.visible_pixels,
                        ef.visible_pixels
                    );
                    if let (Some(ab), Some(eb)) = (af.pixel_bbox, ef.pixel_bbox) {
                        for k in 0..4 {
                            let delta = ab[k].abs_diff(eb[k]);
                            assert!(
                                delta <= 36,
                                "{cid} f{f}: pixel_bbox[{k}] {} differs from {} by {delta}",
                                ab[k],
                                eb[k]
                            );
                        }
                    }
                    let hd = ahash_hamming(&af.perceptual_ahash64, &ef.perceptual_ahash64);
                    assert!(
                        hd <= 8,
                        "{cid} f{f}: perceptual_ahash64 hamming distance {hd} > 8 ({} vs {})",
                        af.perceptual_ahash64,
                        ef.perceptual_ahash64
                    );
                }
            }
        }

        // Verify multi-frame library symbol timeline probes (Button states & Flump/nested clips).
        assert_eq!(
            actual.library_timeline_probes.len(),
            expected.library_timeline_probes.len(),
            "{cid}: library_timeline_probes count"
        );
        for (ep, ap) in expected
            .library_timeline_probes
            .iter()
            .zip(actual.library_timeline_probes.iter())
        {
            assert_eq!(ap.symbol_id, ep.symbol_id, "{cid}: probe symbol_id");
            assert_eq!(ap.name, ep.name, "{cid}: probe name");
            assert_eq!(ap.kind, ep.kind, "{cid}: probe kind");
            assert_eq!(ap.frame_count, ep.frame_count, "{cid}: probe frame_count");
            assert_eq!(
                ap.sampled_frames, ep.sampled_frames,
                "{cid}: probe sampled_frames"
            );
            for (ef, af) in ep.frames.iter().zip(ap.frames.iter()) {
                assert_eq!(af.frame, ef.frame);
                assert_eq!(
                    af.draw_count, ef.draw_count,
                    "{cid} sym {} f{}",
                    ep.name, ef.frame
                );
                assert_eq!(
                    af.clip_count, ef.clip_count,
                    "{cid} sym {} f{}",
                    ep.name, ef.frame
                );
                assert_eq!(
                    af.filter_node_count, ef.filter_node_count,
                    "{cid} sym {} f{}",
                    ep.name, ef.frame
                );
                assert_eq!(
                    af.draw_bbox, ef.draw_bbox,
                    "{cid} sym {} f{}",
                    ep.name, ef.frame
                );
            }
        }

        // Verify bit-for-bit 0-pixel RGBA parity between .fla archive and .xfl directory pairs.
        if let Some(ref partner_id) = expected.identical_to_case {
            let partner = by_id
                .get(partner_id.as_str())
                .unwrap_or_else(|| panic!("missing partner case {partner_id}"));
            assert_eq!(
                actual_case.editable_rgba_by_scene_frame.len(),
                partner.editable_rgba_by_scene_frame.len(),
                "{cid} vs {partner_id}: sampled frame buffer count mismatch"
            );
            for (key, rgba) in &actual_case.editable_rgba_by_scene_frame {
                let partner_rgba = partner
                    .editable_rgba_by_scene_frame
                    .get(key)
                    .expect("matching (scene, frame) key");
                assert_eq!(
                    diff_rgba_pixels(rgba, partner_rgba),
                    0,
                    "{cid} vs {partner_id} scene {} frame {}: rendered RGBA must be bit-identical",
                    key.0,
                    key.1
                );
            }
        }
    }
}

#[test]
fn t471_corpus_21_authored_playback_and_companion_swf_behavioral_parity() {
    let checked = load_checked_oracles();
    let measured = measure_all_corpus_cases();

    for (expected, actual_case) in checked.cases.iter().zip(measured.iter()) {
        let cid = &expected.id;
        let epb = &expected.authored_playback;
        let apb = &actual_case.oracle.authored_playback;

        assert_eq!(apb.ticks_run, epb.ticks_run, "{cid}: ticks_run");
        assert_eq!(
            apb.editable_vs_playback_diverges, epb.editable_vs_playback_diverges,
            "{cid}: editable_vs_playback_diverges"
        );
        assert_eq!(
            apb.divergence_reasons, epb.divergence_reasons,
            "{cid}: divergence_reasons"
        );
        assert_eq!(
            apb.final_playhead_frame, epb.final_playhead_frame,
            "{cid}: final_playhead_frame"
        );
        assert_eq!(
            apb.root_halted_by_script, epb.root_halted_by_script,
            "{cid}: root_halted_by_script"
        );
        assert_eq!(apb.traces, epb.traces, "{cid}: AuthoredPlayback traces");
        assert_eq!(
            apb.diagnostics, epb.diagnostics,
            "{cid}: AuthoredPlayback diagnostics"
        );
        assert_eq!(
            apb.trajectory.len(),
            epb.trajectory.len(),
            "{cid}: trajectory len"
        );

        for (es, as_step) in epb.trajectory.iter().zip(apb.trajectory.iter()) {
            assert_eq!(as_step.tick, es.tick, "{cid}: step tick");
            assert_eq!(
                as_step.playhead_frame, es.playhead_frame,
                "{cid} tick {}: playhead_frame",
                es.tick
            );
            assert_eq!(
                as_step.root_halted_by_script, es.root_halted_by_script,
                "{cid} tick {}: root_halted_by_script",
                es.tick
            );
            assert_eq!(
                as_step.advancing_clip_count, es.advancing_clip_count,
                "{cid} tick {}: advancing_clip_count",
                es.tick
            );
            assert_eq!(
                as_step.diverges_from_editable_stage, es.diverges_from_editable_stage,
                "{cid} tick {}: diverges_from_editable_stage",
                es.tick
            );
            if es.diff_rgba_pixels_vs_editable == 0 {
                assert_eq!(
                    as_step.diff_rgba_pixels_vs_editable, 0,
                    "{cid} tick {}: expected zero RGBA diff vs editable stage",
                    es.tick
                );
            } else {
                assert!(
                    as_step.diff_rgba_pixels_vs_editable > 0,
                    "{cid} tick {}: expected positive RGBA diff vs editable stage",
                    es.tick
                );
            }
        }

        // Companion SWF SwfBytecodePlayer checks across all 8 companion SWF cases:
        assert_eq!(
            actual_case.oracle.companion_swf.is_some(),
            expected.companion_swf.is_some(),
            "{cid}: companion_swf presence"
        );
        if let (Some(eswf), Some(aswf)) =
            (&expected.companion_swf, &actual_case.oracle.companion_swf)
        {
            assert_eq!(aswf.path, eswf.path, "{cid}: companion SWF path");
            assert_eq!(aswf.swf_version, eswf.swf_version, "{cid}: SWF version");
            assert_eq!(aswf.vm_kind, eswf.vm_kind, "{cid}: SWF vm_kind");
            assert_eq!(
                aswf.total_frames, eswf.total_frames,
                "{cid}: SWF total_frames"
            );
            assert_eq!(
                aswf.ticks_executed, eswf.ticks_executed,
                "{cid}: SWF ticks_executed"
            );
            assert_eq!(
                aswf.frame_display_counts, eswf.frame_display_counts,
                "{cid}: SWF frame_display_counts"
            );
            assert_eq!(
                aswf.placed_ratios, eswf.placed_ratios,
                "{cid}: SWF placed_ratios"
            );
            assert_eq!(
                aswf.final_playing, eswf.final_playing,
                "{cid}: SWF final_playing"
            );
            assert_eq!(aswf.traces, eswf.traces, "{cid}: SWF traces");
            assert_eq!(
                aswf.matches_authored_playback_traces, eswf.matches_authored_playback_traces,
                "{cid}: matches_authored_playback_traces"
            );
            assert!(
                aswf.unsupported_diagnostics.is_empty(),
                "{cid}: companion SWF emitted unsupported diagnostics: {:?}",
                aswf.unsupported_diagnostics
            );
        }
    }
}

#[test]
fn t471_corpus_21_pinned_adobe_animate_reference_bounds() {
    let checked = load_checked_oracles();
    let measured = measure_all_corpus_cases();
    let pinned_dir = resolve_pinned_adobe_dir();

    for (expected, actual_case) in checked.cases.iter().zip(measured.iter()) {
        let cid = &expected.id;
        let padobe = &expected.pinned_adobe_animate;
        assert_eq!(padobe.host, "MAC 27,0,1,5", "{cid}: Adobe host");
        assert!(
            padobe.stage_matches,
            "{cid}: stage dimensions/fps/scene lengths must match Adobe"
        );
        assert!(
            padobe.background_matches,
            "{cid}: stage background must match Adobe"
        );
        assert!(
            !padobe.frames.is_empty(),
            "{cid}: must have pinned Adobe frame bounds"
        );

        if let Some(ref adobe_root) = pinned_dir {
            let case_dir = adobe_root.join(cid);
            assert!(
                actual_case.oracle.pinned_adobe_animate.stage_matches,
                "{cid}: live stage check against stage.tsv/scenes.tsv failed"
            );
            assert!(
                actual_case.oracle.pinned_adobe_animate.background_matches,
                "{cid}: live background check against stage.tsv failed"
            );

            for pf in &padobe.frames {
                let png_path = case_dir.join(format!("adobe-s{}-f{}.png", pf.scene, pf.frame));
                let raw_png = fs::read(&png_path)
                    .unwrap_or_else(|e| panic!("{cid}: missing {}: {e}", png_path.display()));
                assert_eq!(
                    sha256_hex(&raw_png),
                    pf.adobe_sha256,
                    "{cid} s{} f{}: pinned Adobe PNG SHA-256 changed",
                    pf.scene,
                    pf.frame
                );
                let adobe_rgba = image::load_from_memory(&raw_png)
                    .unwrap()
                    .to_rgba8()
                    .into_raw();
                let native_rgba = actual_case
                    .editable_rgba_by_scene_frame
                    .get(&(pf.scene, pf.frame))
                    .unwrap_or_else(|| {
                        panic!(
                            "{cid}: missing rendered editable RGBA for s{} f{}",
                            pf.scene, pf.frame
                        )
                    });
                let (diff_px, mae) = compare_rgba_mae(native_rgba, &adobe_rgba);
                assert!(
                    mae <= pf.max_rgba_mae,
                    "{cid} s{} f{}: live RGBA MAE {mae} exceeded bound {}",
                    pf.scene,
                    pf.frame,
                    pf.max_rgba_mae
                );
                assert!(
                    diff_px <= pf.max_different_rgba_pixels,
                    "{cid} s{} f{}: live different_rgba_pixels {diff_px} exceeded bound {}",
                    pf.scene,
                    pf.frame,
                    pf.max_different_rgba_pixels
                );
            }
        }
    }
}
