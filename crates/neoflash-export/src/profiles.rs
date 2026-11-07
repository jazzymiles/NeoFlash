//! Document publish profiles, custom HTML wrapper templates, and network/startup simulation (`T-116`).
//!
//! Reuses [`crate::size_report`] and [`crate::LoadSummary`] so publish profile inspection never
//! duplicates size calculation logic.

use crate::{
    ExportError, LoadSummary, PublishOptions, RuntimeFigures, assemble, html_escape, prepare,
    size_report_from, static_runtime_js_bytes, write_package,
};
use neoflash_core::{ElementKind, Project, ResourceUnit, new_id};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// Alias for [`PublishOptions`] used in publish profiles.
pub type ExportOptions = PublishOptions;

/// Resource path used to persist named publish profiles portably inside a [`Project`].
pub const PUBLISH_PROFILES_RESOURCE_PATH: &str = "publish/profiles.json";

/// Network bandwidth and round-trip latency presets for progressive startup simulation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPreset {
    /// Fast 3G mobile connection (~1.6 Mbps, 150 ms RTT).
    #[default]
    Fast3G,
    /// Slow 3G mobile connection (~400 kbps, 400 ms RTT).
    Slow3G,
    /// Broadband cable connection (~5 Mbps, 28 ms RTT).
    Cable5Mbps,
    /// User-defined bandwidth in kilobits per second and round-trip latency in milliseconds.
    Custom {
        bandwidth_kbps: u32,
        latency_ms: u32,
    },
}

impl NetworkPreset {
    pub const ALL_PRESETS: [Self; 3] = [Self::Fast3G, Self::Slow3G, Self::Cable5Mbps];

    /// Effective download bandwidth in kilobits per second (1 kbps = 1000 bits/s = 125 bytes/s).
    pub fn bandwidth_kbps(self) -> u32 {
        match self {
            Self::Fast3G => 1_600,
            Self::Slow3G => 400,
            Self::Cable5Mbps => 5_000,
            Self::Custom { bandwidth_kbps, .. } => bandwidth_kbps.max(1),
        }
    }

    /// Round-trip latency in milliseconds before payload bytes begin arriving.
    pub fn latency_ms(self) -> u32 {
        match self {
            Self::Fast3G => 150,
            Self::Slow3G => 400,
            Self::Cable5Mbps => 28,
            Self::Custom { latency_ms, .. } => latency_ms,
        }
    }

    /// Throughput in bytes per millisecond (`bandwidth_kbps * 1000 / 8 / 1000 = bandwidth_kbps * 0.125`).
    pub fn bytes_per_ms(self) -> f64 {
        (self.bandwidth_kbps() as f64) * 0.125
    }

    /// Download time in milliseconds for a payload of `bytes` including one RTT connection/request latency.
    pub fn transfer_time_ms(self, bytes: usize) -> f64 {
        if bytes == 0 {
            return 0.0;
        }
        (self.latency_ms() as f64) + (bytes as f64) / self.bytes_per_ms()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Fast3G => "Fast 3G (1.6 Mbps / 150 ms)",
            Self::Slow3G => "Slow 3G (400 kbps / 400 ms)",
            Self::Cable5Mbps => "Cable (5 Mbps / 28 ms)",
            Self::Custom { .. } => "Custom",
        }
    }
}

/// Network and delayed-asset configuration attached to a [`PublishProfile`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkSimulationProfile {
    pub preset: NetworkPreset,
    /// Resource paths or symbol names/IDs simulated as delayed / on-demand assets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delayed_assets: Vec<String>,
    /// Additional synthetic latency in milliseconds applied to delayed assets.
    #[serde(default)]
    pub extra_asset_delay_ms: u32,
}

/// Named, serializable document publish profile (`T-116`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub options: ExportOptions,
    /// Optional custom HTML wrapper template with `$TITLE`, `$WIDTH`, `$HEIGHT`, `$BG`, `$FPS`,
    /// `$SCRIPT_BUNDLE`, `$RUNTIME_SCRIPT`, and `$PROJECT_JSON` tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html_template: Option<String>,
    #[serde(default)]
    pub network_profile: NetworkSimulationProfile,
}

impl Default for PublishProfile {
    fn default() -> Self {
        Self {
            id: "profile-default".to_string(),
            name: "Default".to_string(),
            options: ExportOptions::default(),
            html_template: None,
            network_profile: NetworkSimulationProfile::default(),
        }
    }
}

impl PublishProfile {
    pub fn new(name: impl Into<String>, options: ExportOptions) -> Self {
        Self {
            id: new_id("profile"),
            name: name.into(),
            options,
            html_template: None,
            network_profile: NetworkSimulationProfile::default(),
        }
    }

    /// Validate options and any custom HTML template before exporting or saving.
    pub fn validate(&self) -> Result<Vec<String>, ExportError> {
        if self.name.trim().is_empty() {
            return Err(ExportError::Options(
                "Publish profile name cannot be empty.".into(),
            ));
        }
        self.options.validated().map_err(ExportError::Options)?;
        if let Some(template) = &self.html_template {
            validate_custom_html_wrapper(template)
        } else {
            Ok(Vec::new())
        }
    }
}

/// Duplicate an existing [`PublishProfile`] with a fresh ID and a distinct name.
pub fn duplicate_profile(profile: &PublishProfile, new_name: Option<&str>) -> PublishProfile {
    let name = match new_name {
        Some(n) if !n.trim().is_empty() => n.trim().to_owned(),
        _ => format!("{} Copy", profile.name),
    };
    PublishProfile {
        id: new_id("profile"),
        name,
        options: profile.options.clone(),
        html_template: profile.html_template.clone(),
        network_profile: profile.network_profile.clone(),
    }
}

/// Export a [`PublishProfile`] to a portable JSON string (`.nfprofile`).
pub fn export_profile_json(profile: &PublishProfile) -> Result<String, ExportError> {
    profile.validate()?;
    Ok(serde_json::to_string_pretty(profile)?)
}

/// Import and validate a [`PublishProfile`] from a JSON string (`.nfprofile`).
pub fn import_profile_json(json_str: &str) -> Result<PublishProfile, ExportError> {
    let mut profile: PublishProfile = serde_json::from_str(json_str)?;
    if profile.id.trim().is_empty() {
        profile.id = new_id("profile");
    }
    profile.options = profile.options.validated().map_err(ExportError::Options)?;
    profile.validate()?;
    Ok(profile)
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct StoredProfileSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_profile_id: Option<String>,
    profiles: Vec<PublishProfile>,
}

/// Store named publish profiles inside a [`Project`]'s portable resources (`publish/profiles.json`).
pub fn store_profiles_in_project(
    project: &mut Project,
    profiles: &[PublishProfile],
    active_profile_id: Option<&str>,
) -> Result<(), ExportError> {
    for profile in profiles {
        profile.validate()?;
    }
    if profiles.is_empty() {
        project
            .resources
            .retain(|r| !r.path.eq_ignore_ascii_case(PUBLISH_PROFILES_RESOURCE_PATH));
        return Ok(());
    }
    let payload = StoredProfileSet {
        active_profile_id: active_profile_id.map(str::to_owned),
        profiles: profiles.to_vec(),
    };
    let bytes = serde_json::to_vec_pretty(&payload)?;
    let existing_id = project
        .resources
        .iter()
        .find(|r| r.path.eq_ignore_ascii_case(PUBLISH_PROFILES_RESOURCE_PATH))
        .map(|r| r.id.clone())
        .unwrap_or_else(|| new_id("resource"));
    let unit = ResourceUnit::from_bytes(
        existing_id,
        PUBLISH_PROFILES_RESOURCE_PATH,
        "application/json",
        &bytes,
    )
    .map_err(|e| ExportError::InvalidProject(e.to_string()))?;
    if let Some(slot) = project
        .resources
        .iter_mut()
        .find(|r| r.path.eq_ignore_ascii_case(PUBLISH_PROFILES_RESOURCE_PATH))
    {
        *slot = unit;
    } else {
        project.resources.push(unit);
    }
    project.format_version = project.format_version.max(3);
    Ok(())
}

/// Load stored [`PublishProfile`]s and the active profile ID from a [`Project`].
/// Returns `[PublishProfile::default()]` when the document has no saved profiles.
pub fn load_profiles_from_project(
    project: &Project,
) -> Result<(Vec<PublishProfile>, Option<String>), ExportError> {
    let Some(unit) = project
        .resources
        .iter()
        .find(|r| r.path.eq_ignore_ascii_case(PUBLISH_PROFILES_RESOURCE_PATH))
    else {
        let default_profile = PublishProfile::default();
        let id = default_profile.id.clone();
        return Ok((vec![default_profile], Some(id)));
    };
    let bytes = unit
        .decoded_bytes()
        .map_err(|e| ExportError::InvalidProject(e.to_string()))?;
    let stored: StoredProfileSet = serde_json::from_slice(&bytes)?;
    if stored.profiles.is_empty() {
        let default_profile = PublishProfile::default();
        let id = default_profile.id.clone();
        return Ok((vec![default_profile], Some(id)));
    }
    Ok((stored.profiles, stored.active_profile_id))
}

/// Validate a custom HTML wrapper template.
///
/// Requires `$RUNTIME_SCRIPT` or an explicit `runtime.js` reference so published output never
/// silently omits the player boot script. Returns non-blocking warnings when optional mount
/// points (such as `id="stage"` or `$SCRIPT_BUNDLE`) are absent.
pub fn validate_custom_html_wrapper(template: &str) -> Result<Vec<String>, ExportError> {
    if template.trim().is_empty() {
        return Err(ExportError::Options(
            "Custom HTML wrapper template cannot be empty.".into(),
        ));
    }
    let has_runtime_hook = template.contains("$RUNTIME_SCRIPT") || template.contains("runtime.js");
    if !has_runtime_hook {
        return Err(ExportError::Options(
            "Custom HTML wrapper must include the $RUNTIME_SCRIPT token or reference runtime.js."
                .into(),
        ));
    }
    let mut warnings = Vec::new();
    if !template.contains("id=\"stage\"") && !template.contains("id='stage'") {
        warnings.push(
            "Custom HTML wrapper does not contain a <canvas id=\"stage\"> element; runtime.js requires #stage to mount the display surface.".to_string(),
        );
    }
    if !template.contains("$SCRIPT_BUNDLE")
        && !template.contains("$RUNTIME_SCRIPT")
        && !template.contains("bundle.js")
    {
        warnings.push(
            "Custom HTML wrapper references runtime.js directly without $SCRIPT_BUNDLE or bundle.js.".to_string(),
        );
    }
    Ok(warnings)
}

/// Render a custom HTML wrapper template by substituting supported publish tokens:
/// `$TITLE`, `$WIDTH`, `$HEIGHT`, `$BG`, `$FPS`, `$SCRIPT_BUNDLE`, `$RUNTIME_SCRIPT`,
/// `$PROJECT_JSON`, and `$FONT_CSS`.
pub fn render_custom_html_wrapper(
    template: &str,
    project: &Project,
    script_srcs: &[String],
    project_json: &str,
    font_css: &str,
) -> Result<String, ExportError> {
    validate_custom_html_wrapper(template)?;

    let title = html_escape(&project.name);
    let width = project.width.to_string();
    let height = project.height.to_string();
    let bg = format!(
        "#{:02x}{:02x}{:02x}",
        project.background[0], project.background[1], project.background[2]
    );
    let fps = project.fps.to_string();

    let all_script_tags: Vec<String> = script_srcs
        .iter()
        .map(|src| format!("<script defer src=\"{src}\"></script>"))
        .collect();

    let (bundle_tags, runtime_tag) = if template.contains("$SCRIPT_BUNDLE") {
        if let Some((last, rest)) = all_script_tags.split_last() {
            (rest.join("\n  "), last.clone())
        } else {
            (
                "<script defer src=\"bundle.js\"></script>".to_string(),
                "<script defer src=\"runtime.js\"></script>".to_string(),
            )
        }
    } else if all_script_tags.is_empty() {
        (
            "<script defer src=\"bundle.js\"></script>".to_string(),
            "<script defer src=\"runtime.js\"></script>".to_string(),
        )
    } else {
        (
            all_script_tags[..all_script_tags.len().saturating_sub(1)].join("\n  "),
            all_script_tags.join("\n  "),
        )
    };

    let out = template
        .replace("$TITLE", &title)
        .replace("$WIDTH", &width)
        .replace("$HEIGHT", &height)
        .replace("$BG", &bg)
        .replace("$FPS", &fps)
        .replace("$SCRIPT_BUNDLE", &bundle_tags)
        .replace("$RUNTIME_SCRIPT", &runtime_tag)
        .replace("$PROJECT_JSON", project_json)
        .replace("$FONT_CSS", font_css);

    Ok(out)
}

/// Export a [`Project`] using a [`PublishProfile`], applying its [`PublishOptions`] and optional
/// custom HTML wrapper template.
pub fn export_project_with_profile(
    project: &Project,
    output_dir: &Path,
    runtime_dir: &Path,
    profile: &PublishProfile,
) -> Result<PathBuf, ExportError> {
    profile.validate()?;
    let options = profile.options.validated().map_err(ExportError::Options)?;
    let prepared = prepare(project, runtime_dir, options.fonts, options.audio)?;
    let mut package = assemble(project, &prepared, &options.size)?;

    if let Some(template) = &profile.html_template {
        let project_json = serde_json::to_string(&prepared.published)?;
        let custom_html = render_custom_html_wrapper(
            template,
            project,
            &package.script_srcs,
            &project_json,
            &prepared.fonts.css_rules,
        )?;
        if let Some((_, bytes)) = package
            .files
            .iter_mut()
            .find(|(name, _)| name == "index.html")
        {
            *bytes = custom_html.into_bytes();
        }
    }

    write_package(output_dir, &package.files, &project.name, "project")
}

/// Per-asset (resource or library symbol) readiness under network simulation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DelayedAssetSimStatus {
    pub id_or_path: String,
    /// `"resource"` or `"symbol"`.
    pub kind: String,
    pub category: String,
    pub bytes: usize,
    /// One-based frame where this asset is first placed or triggered on the main timeline.
    pub first_used_frame: Option<u32>,
    /// Playback time (ms) when `first_used_frame` is reached.
    pub needed_at_ms: Option<f64>,
    /// Simulated download completion time (ms) from startup.
    pub ready_time_ms: f64,
    /// Whether this asset was marked as delayed in [`NetworkSimulationProfile::delayed_assets`].
    pub delayed: bool,
    /// True when `ready_time_ms > needed_at_ms`.
    pub arrives_late: bool,
}

/// Per-frame streaming and playback readiness entry (`T-116`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameBandwidthStatus {
    /// One-based flattened main-timeline frame number.
    pub frame: u32,
    /// Serialized bytes introduced at this frame (keyframes, frame scripts, and first-used assets).
    pub frame_bytes: usize,
    /// Cumulative frame payload bytes up to and including `frame`.
    pub cumulative_bytes: usize,
    /// Wall-clock playback time (ms) when the playhead enters this frame (`(frame - 1) * 1000 / fps`).
    pub playback_time_ms: f64,
    /// Simulated cumulative download time (ms) required before this frame is ready.
    pub download_time_ms: f64,
    /// True when `download_time_ms > playback_time_ms`, indicating a streaming buffer underrun.
    pub stalls: bool,
}

/// Complete network speed and progressive startup inspection report (`T-116`).
///
/// Reuses [`crate::size_report`] (`size_report_text` and `load_summary`) rather than creating a
/// separate statistics schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetworkSimulationReport {
    pub profile_id: String,
    pub profile_name: String,
    pub preset: NetworkPreset,
    pub effective_bandwidth_kbps: u32,
    pub latency_ms: u32,
    /// Exact load summary from [`crate::size_report`].
    pub load_summary: LoadSummary,
    /// Exact formatted text report from [`crate::size_report`].
    pub size_report_text: String,
    /// Initial HTML + JS runtime + WASM bootstrap payload in bytes.
    pub initial_html_js_wasm_bytes: usize,
    /// Download time in milliseconds for the initial HTML/JS/WASM bootstrap before frame 1.
    pub initial_html_js_wasm_ms: f64,
    /// Total download time in milliseconds for the entire published package and assets.
    pub total_download_ms: f64,
    /// Per-resource and per-symbol availability and delayed-loading status.
    pub delayed_assets: Vec<DelayedAssetSimStatus>,
    /// Frame-by-frame progressive streaming readiness.
    pub frames: Vec<FrameBandwidthStatus>,
    /// One-based frame numbers where `stalls == true` ("buffer underrun frames").
    pub buffer_underrun_frames: Vec<u32>,
    /// Recommended preload buffer duration in milliseconds to play from start to end without stalling.
    pub recommended_preload_ms: f64,
    /// Recommended number of frames worth of preloading at the project's frame rate.
    pub recommended_preload_frames: u32,
}

/// Compute exact size report and [`LoadSummary`] for `project` under `options`, reusing the
/// shared size-report engine.
pub fn size_report(
    project: &Project,
    options: &ExportOptions,
) -> Result<(String, LoadSummary), ExportError> {
    let options = options.validated().map_err(ExportError::Options)?;
    let issues = neoflash_core::validate(project);
    if !issues.is_empty() {
        return Err(ExportError::InvalidProject(format!("{issues:?}")));
    }
    let program = neoflash_script::compile_project(project).map_err(|errors| {
        ExportError::Scripts(
            errors
                .into_iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?;
    let compiled_scripts = crate::compile_scripts(project, &program)?;
    let runtime_js_bytes = if matches!(options.size, crate::SizeLevel::Shared(_)) {
        0
    } else {
        static_runtime_js_bytes()
    };
    let figures = RuntimeFigures {
        wasm_bytes: 0,
        wasm_inline: options.size.wasm_inline(),
        runtime_js_bytes,
    };
    size_report_from(project, &program, &compiled_scripts, Some(figures))
}

/// Simulate network download speed, delayed assets, and frame-by-frame progressive startup
/// readiness for `project` using `profile` (`T-116`).
pub fn simulate_network_startup(
    project: &Project,
    profile: &PublishProfile,
) -> Result<NetworkSimulationReport, ExportError> {
    let (size_report_text, load_summary) = size_report(project, &profile.options)?;
    let preset = profile.network_profile.preset;
    let bytes_per_ms = preset.bytes_per_ms();
    let base_latency_ms = preset.latency_ms() as f64;
    let fps = project.fps.max(1) as f64;
    let frame_duration_ms = 1000.0 / fps;

    // Initial bootstrap: runtime JS + WASM (if any) + compiled scripts + HTML shell (~3 KB).
    let html_shell_bytes = 3_072usize;
    let initial_html_js_wasm_bytes = load_summary.runtime_js_bytes
        + load_summary.wasm_bytes
        + load_summary.scripts_compiled_bytes
        + html_shell_bytes;
    let initial_html_js_wasm_ms = if initial_html_js_wasm_bytes == 0 {
        0.0
    } else {
        base_latency_ms + (initial_html_js_wasm_bytes as f64) / bytes_per_ms
    };

    // Flatten scenes to inspect frame-by-frame placements and first-used frames.
    let (flattened, _) = neoflash_core::flatten_scenes_with_ranges(project);
    let root_symbol = flattened.symbol(&flattened.root);
    let total_frames = root_symbol.map_or(1, |s| s.timeline.frame_count.max(1));

    // Track first-used 0-based frame for each resource ID and symbol ID.
    let mut resource_first_frame: BTreeMap<String, u32> = BTreeMap::new();
    let mut symbol_first_frame: BTreeMap<String, u32> = BTreeMap::new();
    let mut per_frame_keyframe_bytes: Vec<usize> = vec![0; total_frames as usize];

    if let Some(root) = root_symbol {
        for layer in &root.timeline.layers {
            for key in &layer.keyframes {
                let f = key.frame.min(total_frames.saturating_sub(1));
                let key_bytes = serde_json::to_string(key).map_or(0, |s| s.len());
                per_frame_keyframe_bytes[f as usize] += key_bytes;

                if let Some(sound) = &key.sound {
                    resource_first_frame
                        .entry(sound.resource.clone())
                        .and_modify(|prev| *prev = (*prev).min(f))
                        .or_insert(f);
                }
                for bf in &key.bitmap_fills {
                    resource_first_frame
                        .entry(bf.resource_id.clone())
                        .and_modify(|prev| *prev = (*prev).min(f))
                        .or_insert(f);
                }
                for elem in &key.elements {
                    match &elem.kind {
                        ElementKind::Bitmap { resource_id, .. } => {
                            resource_first_frame
                                .entry(resource_id.clone())
                                .and_modify(|prev| *prev = (*prev).min(f))
                                .or_insert(f);
                        }
                        ElementKind::Instance { symbol_id, .. } => {
                            symbol_first_frame
                                .entry(symbol_id.clone())
                                .and_modify(|prev| *prev = (*prev).min(f))
                                .or_insert(f);
                        }
                        _ => {}
                    }
                }
            }
        }
        for fs in &root.timeline.scripts {
            let f = fs.frame.min(total_frames.saturating_sub(1));
            let fs_bytes = serde_json::to_string(fs).map_or(0, |s| s.len());
            per_frame_keyframe_bytes[f as usize] += fs_bytes;
        }
    }

    let delayed_set: BTreeSet<String> = profile
        .network_profile
        .delayed_assets
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .collect();
    let extra_delay = profile.network_profile.extra_asset_delay_ms as f64;

    let mut delayed_assets = Vec::new();
    let mut per_frame_asset_bytes: Vec<usize> = vec![0; total_frames as usize];
    let mut cumulative_asset_stream_ms = initial_html_js_wasm_ms;

    // 1. Evaluate resources
    for (res, item) in project.resources.iter().zip(load_summary.resources.iter()) {
        let first_f0 = resource_first_frame.get(&res.id).copied();
        let is_delayed = delayed_set.contains(&res.id.to_ascii_lowercase())
            || delayed_set.contains(&res.path.to_ascii_lowercase());
        let transfer_bytes = if matches!(profile.options.size, crate::SizeLevel::Full) {
            item.b64_bytes
        } else {
            item.raw_bytes
        };
        if let Some(f0) = first_f0 {
            per_frame_asset_bytes[f0 as usize] += transfer_bytes;
        } else {
            per_frame_asset_bytes[0] += transfer_bytes;
        }
        let asset_dl_ms = (transfer_bytes as f64) / bytes_per_ms;
        let ready_time_ms = if is_delayed {
            let trigger_ms = first_f0.map_or(0.0, |f0| (f0 as f64) * frame_duration_ms);
            initial_html_js_wasm_ms.max(trigger_ms) + base_latency_ms + extra_delay + asset_dl_ms
        } else {
            cumulative_asset_stream_ms += asset_dl_ms;
            cumulative_asset_stream_ms
        };
        let needed_at_ms = first_f0.map(|f0| (f0 as f64) * frame_duration_ms);
        let arrives_late = needed_at_ms.is_some_and(|needed| ready_time_ms > needed + 1e-6);
        delayed_assets.push(DelayedAssetSimStatus {
            id_or_path: res.path.clone(),
            kind: "resource".to_string(),
            category: item.category.clone(),
            bytes: transfer_bytes,
            first_used_frame: first_f0.map(|f0| f0 + 1),
            needed_at_ms,
            ready_time_ms,
            delayed: is_delayed,
            arrives_late,
        });
    }

    // 2. Evaluate non-scene library symbols
    let scene_ids: BTreeSet<&str> = if project.scenes.is_empty() {
        BTreeSet::from([project.root.as_str()])
    } else {
        project.scenes.iter().map(String::as_str).collect()
    };
    for sym in &project.symbols {
        if scene_ids.contains(sym.id.as_str()) {
            continue;
        }
        let sym_bytes = serde_json::to_string(sym).map_or(0, |s| s.len());
        let first_f0 = symbol_first_frame.get(&sym.id).copied();
        let is_delayed = delayed_set.contains(&sym.id.to_ascii_lowercase())
            || delayed_set.contains(&sym.name.to_ascii_lowercase());
        if let Some(f0) = first_f0 {
            per_frame_asset_bytes[f0 as usize] += sym_bytes;
        } else {
            per_frame_asset_bytes[0] += sym_bytes;
        }
        let sym_dl_ms = (sym_bytes as f64) / bytes_per_ms;
        let ready_time_ms = if is_delayed {
            let trigger_ms = first_f0.map_or(0.0, |f0| (f0 as f64) * frame_duration_ms);
            initial_html_js_wasm_ms.max(trigger_ms) + base_latency_ms + extra_delay + sym_dl_ms
        } else {
            cumulative_asset_stream_ms += sym_dl_ms;
            cumulative_asset_stream_ms
        };
        let needed_at_ms = first_f0.map(|f0| (f0 as f64) * frame_duration_ms);
        let arrives_late = needed_at_ms.is_some_and(|needed| ready_time_ms > needed + 1e-6);
        delayed_assets.push(DelayedAssetSimStatus {
            id_or_path: sym.name.clone(),
            kind: "symbol".to_string(),
            category: format!("{:?}", sym.kind),
            bytes: sym_bytes,
            first_used_frame: first_f0.map(|f0| f0 + 1),
            needed_at_ms,
            ready_time_ms,
            delayed: is_delayed,
            arrives_late,
        });
    }

    // 3. Compute frame-by-frame progressive streaming readiness
    let mut frames = Vec::with_capacity(total_frames as usize);
    let mut buffer_underrun_frames = Vec::new();
    let mut cumulative_bytes = 0usize;
    let mut max_preload_deficit_ms = 0.0_f64;

    for f0 in 0..total_frames {
        let idx = f0 as usize;
        let frame_bytes = per_frame_keyframe_bytes[idx] + per_frame_asset_bytes[idx];
        cumulative_bytes += frame_bytes;
        let playback_time_ms = (f0 as f64) * frame_duration_ms;
        let mut download_time_ms =
            initial_html_js_wasm_ms + (cumulative_bytes as f64) / bytes_per_ms;
        // Include any delayed asset needed on or before this frame
        for asset in &delayed_assets {
            if asset.delayed && asset.first_used_frame == Some(f0 + 1) {
                download_time_ms = download_time_ms.max(asset.ready_time_ms);
            }
        }
        let deficit = download_time_ms - playback_time_ms;
        let stalls = deficit > 1e-6;
        if stalls {
            buffer_underrun_frames.push(f0 + 1);
            if deficit > max_preload_deficit_ms {
                max_preload_deficit_ms = deficit;
            }
        }
        frames.push(FrameBandwidthStatus {
            frame: f0 + 1,
            frame_bytes,
            cumulative_bytes,
            playback_time_ms,
            download_time_ms,
            stalls,
        });
    }

    let total_download_ms = frames
        .last()
        .map_or(initial_html_js_wasm_ms, |last| last.download_time_ms);
    let recommended_preload_frames = if max_preload_deficit_ms <= 0.0 {
        0
    } else {
        (max_preload_deficit_ms / frame_duration_ms).ceil() as u32
    };

    Ok(NetworkSimulationReport {
        profile_id: profile.id.clone(),
        profile_name: profile.name.clone(),
        preset,
        effective_bandwidth_kbps: preset.bandwidth_kbps(),
        latency_ms: preset.latency_ms(),
        load_summary,
        size_report_text,
        initial_html_js_wasm_bytes,
        initial_html_js_wasm_ms,
        total_download_ms,
        delayed_assets,
        frames,
        buffer_underrun_frames,
        recommended_preload_ms: max_preload_deficit_ms,
        recommended_preload_frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AudioExportPolicy, FontExportPolicy, SizeLevel};
    use neoflash_core::{Element, Keyframe, Layer, LayerProperties};

    #[test]
    fn duplicate_and_json_roundtrip_preserve_profile_settings() {
        let mut original = PublishProfile::new(
            "Web Banner Compact",
            ExportOptions {
                fonts: FontExportPolicy::Subset,
                size: SizeLevel::Compact,
                audio: AudioExportPolicy::Mp3_96,
            },
        );
        original.html_template = Some(
            "<!doctype html><html><head><title>$TITLE</title>$SCRIPT_BUNDLE\n$RUNTIME_SCRIPT</head><body style=\"background:$BG\"><canvas id=\"stage\" width=\"$WIDTH\" height=\"$HEIGHT\"></canvas></body></html>"
                .into(),
        );
        original.network_profile = NetworkSimulationProfile {
            preset: NetworkPreset::Slow3G,
            delayed_assets: vec!["audio/music.mp3".into()],
            extra_asset_delay_ms: 250,
        };

        let copy = duplicate_profile(&original, None);
        assert_ne!(copy.id, original.id);
        assert_eq!(copy.name, "Web Banner Compact Copy");
        assert_eq!(copy.options, original.options);
        assert_eq!(copy.html_template, original.html_template);
        assert_eq!(copy.network_profile, original.network_profile);

        let json = export_profile_json(&original).expect("export profile json");
        let imported = import_profile_json(&json).expect("import profile json");
        assert_eq!(imported, original);

        // Storing and loading profiles inside a Project
        let mut project = Project::blank();
        store_profiles_in_project(
            &mut project,
            &[original.clone(), copy.clone()],
            Some(&copy.id),
        )
        .expect("store profiles");
        let (loaded, active) = load_profiles_from_project(&project).expect("load profiles");
        assert_eq!(loaded.len(), 2);
        assert_eq!(active.as_deref(), Some(copy.id.as_str()));
        assert_eq!(loaded[0], original);
    }

    #[test]
    fn custom_html_wrapper_substitutes_tokens_and_validates_runtime_hooks() {
        let mut project = Project::blank();
        project.name = "Arcade <Hero>".into();
        project.width = 800;
        project.height = 600;
        project.fps = 60;
        project.background = [0x12, 0x34, 0x56, 0xff];

        let bad_template = "<!doctype html><html><head><title>$TITLE</title></head><body><canvas id=\"stage\"></canvas></body></html>";
        let err = render_custom_html_wrapper(
            bad_template,
            &project,
            &[
                "neoflash_web.js".into(),
                "bundle.js".into(),
                "runtime.js".into(),
            ],
            "{}",
            "",
        )
        .unwrap_err();
        assert!(err.to_string().contains("$RUNTIME_SCRIPT"));

        let good_template = "<!doctype html><html><head><title>$TITLE ($FPS fps)</title>$SCRIPT_BUNDLE\n$RUNTIME_SCRIPT</head><body data-bg=\"$BG\"><canvas id=\"stage\" width=\"$WIDTH\" height=\"$HEIGHT\"></canvas><script type=\"application/json\" id=\"nf-proj\">$PROJECT_JSON</script></body></html>";
        let rendered = render_custom_html_wrapper(
            good_template,
            &project,
            &[
                "neoflash_web.js".into(),
                "bundle.js".into(),
                "runtime.js".into(),
            ],
            "{\"ok\":true}",
            "",
        )
        .expect("valid template renders");
        assert!(rendered.contains("Arcade &lt;Hero&gt; (60 fps)"));
        assert!(rendered.contains("width=\"800\" height=\"600\""));
        assert!(rendered.contains("data-bg=\"#123456\""));
        assert!(rendered.contains("<script defer src=\"neoflash_web.js\"></script>"));
        assert!(rendered.contains("<script defer src=\"bundle.js\"></script>"));
        assert!(rendered.contains("<script defer src=\"runtime.js\"></script>"));
        assert!(rendered.contains("{\"ok\":true}"));
    }

    #[test]
    fn network_simulation_reuses_size_report_and_detects_underrun_and_delayed_assets() {
        let mut project = Project::blank();
        project.format_version = 6;
        project.fps = 30;
        let mut img_buf = Vec::new();
        let rgba = image::RgbaImage::from_fn(64, 64, |x, y| {
            image::Rgba([(x * 4) as u8, (y * 4) as u8, ((x + y) * 2) as u8, 255])
        });
        rgba.write_to(
            &mut std::io::Cursor::new(&mut img_buf),
            image::ImageFormat::Png,
        )
        .unwrap();
        let res =
            ResourceUnit::from_bytes("res-heavy", "images/heavy_bg.png", "image/png", &img_buf)
                .unwrap();
        project.resources.push(res);

        let root_id = project.root.clone();
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 10;
        root.timeline.layers = vec![Layer {
            id: "layer-main".into(),
            name: "Layer 1".into(),
            visible: true,
            locked: false,
            properties: LayerProperties::default(),
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    elements: vec![],
                    ..Keyframe::default()
                },
                Keyframe {
                    frame: 4,
                    elements: vec![Element {
                        id: "el-bmp".into(),
                        name: "HeavyBG".into(),
                        kind: ElementKind::Bitmap {
                            resource_id: "res-heavy".into(),
                            width: 64,
                            height: 64,
                            smoothing: true,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
            ],
        }];

        let mut profile = PublishProfile::default();
        profile.network_profile = NetworkSimulationProfile {
            preset: NetworkPreset::Slow3G,
            delayed_assets: vec!["images/heavy_bg.png".into()],
            extra_asset_delay_ms: 300,
        };

        let sim = simulate_network_startup(&project, &profile).expect("simulation succeeds");
        assert!(sim.size_report_text.contains("NeoFlash Size Report"));
        assert_eq!(sim.load_summary.bitmap_count, 1);
        assert_eq!(sim.frames.len(), 10);
        assert!(sim.initial_html_js_wasm_ms > 400.0);
        assert!(sim.buffer_underrun_frames.contains(&1));
        assert!(sim.buffer_underrun_frames.contains(&5));
        assert!(sim.recommended_preload_ms > 0.0);
        assert!(sim.recommended_preload_frames >= 1);

        let delayed = sim
            .delayed_assets
            .iter()
            .find(|a| a.id_or_path == "images/heavy_bg.png")
            .expect("heavy_bg entry present");
        assert!(delayed.delayed);
        assert_eq!(delayed.first_used_frame, Some(5));
        assert!(delayed.arrives_late);

        // Verify export_project_with_profile applies custom HTML template end-to-end
        let tmp = std::env::temp_dir().join(format!(
            "neoflash-profile-export-{}-{}",
            std::process::id(),
            new_id("tmp")
        ));
        let rt_dir = tmp.join("rt");
        let out_dir = tmp.join("out");
        std::fs::create_dir_all(&rt_dir).unwrap();
        std::fs::write(rt_dir.join("neoflash_web.js"), "var NeoFlashWasm;").unwrap();
        std::fs::write(rt_dir.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
        profile.html_template = Some(
            "<!doctype html><html><head><title>Custom:$TITLE</title>$SCRIPT_BUNDLE\n$RUNTIME_SCRIPT</head><body data-fps=\"$FPS\"><canvas id=\"stage\" width=\"$WIDTH\" height=\"$HEIGHT\"></canvas></body></html>"
                .into(),
        );
        let index_path =
            export_project_with_profile(&project, &out_dir, &rt_dir, &profile).unwrap();
        let exported_html = std::fs::read_to_string(index_path).unwrap();
        assert!(exported_html.contains("<title>Custom:Untitled</title>"));
        assert!(exported_html.contains("data-fps=\"30\""));
        assert!(exported_html.contains("<script defer src=\"runtime.js\"></script>"));
        let _ = std::fs::remove_dir_all(tmp);
    }
}
