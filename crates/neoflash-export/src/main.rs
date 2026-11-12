use neoflash_core::Project;
use neoflash_export::{
    AepOptions, AtlasFormat, AtlasOptions, AudioExportOptions, AudioExportPolicy, AudioFormat,
    BannerOptions, FontExportPolicy, HtmlPublishConfig, HtmlScaleMode, HtmlStageAlign,
    ImageExportFormat, ImageExportOptions, ImageExportRange, LottieOptions, NetworkPreset,
    NetworkStartupEstimate, PublishOptions, PublishProfile, SizeLevel, SvgOptions, VideoFormat,
    VideoOptions, export_profile_json, export_project_with_profile, import_profile_json,
    load_profiles_from_project, simulate_network_startup,
};
use std::{env, fs, path::PathBuf};

/// Structured output returned by [`run_with_args`] for CLI execution and integration tests.
#[derive(Clone, Debug, Default)]
pub struct CliRunOutput {
    pub exported_index: Option<PathBuf>,
    pub exported_profile_path: Option<PathBuf>,
    pub active_profile: Option<PublishProfile>,
    pub network_estimate: Option<NetworkStartupEstimate>,
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    run_with_args(env::args().skip(1)).map(|_| ())
}

pub fn run_with_args<I, S>(raw_args: I) -> Result<CliRunOutput, Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = raw_args.into_iter().map(Into::into);
    let mut source = None;
    let mut demo = false;
    let mut clean = false;
    let mut size_report = false;
    let mut size_estimate = false;
    let mut has_output = false;
    let mut has_size_level = false;
    let mut level = "full".to_owned();
    let mut runtime_url = None;
    let mut shared_runtime = None;
    let mut desktop_output = None;
    let mut lottie_output = None;
    let mut video_output = None;
    let mut audio_output = None;
    let mut svg_output = None;
    let mut atlas_output = None;
    let mut atlas_format_override: Option<AtlasFormat> = None;
    let mut atlas_extrude: u32 = 0;
    let mut banner_output: Option<PathBuf> = None;
    let mut banner_max_gzip: Option<usize> = None;
    let mut aep_output: Option<PathBuf> = None;
    let mut sequence_output: Option<PathBuf> = None;
    let mut audio_format = None;
    let mut lottie_symbol = None;
    let mut lottie_pretty = false;
    let mut scale_mode: Option<HtmlScaleMode> = None;
    let mut stage_align: Option<HtmlStageAlign> = None;
    let mut allow_fullscreen: Option<bool> = None;
    let mut pack_sprite_sheets = false;
    let mut assets_folder = false;
    let mut export_profile_path: Option<PathBuf> = None;
    let mut import_profile_path: Option<PathBuf> = None;
    let mut profile_selector: Option<String> = None;
    let mut simulate_network_preset: Option<NetworkPreset> = None;
    let mut output = PathBuf::from("target/artifacts/html5");
    let mut runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/web-runtime");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => demo = true,
            "--clean" => clean = true,
            "--size-report" => size_report = true,
            "--size-estimate" => size_estimate = true,
            "--pretty" => lottie_pretty = true,
            "--export-profile" => {
                export_profile_path = Some(PathBuf::from(
                    args.next()
                        .ok_or("--export-profile requires an output .json file path")?,
                ));
            }
            "--import-profile" => {
                import_profile_path = Some(PathBuf::from(
                    args.next()
                        .ok_or("--import-profile requires an input .json file path")?,
                ));
            }
            "--profile" => {
                profile_selector = Some(
                    args.next()
                        .ok_or("--profile requires a profile name or ID")?,
                );
            }
            "--simulate-network" => {
                let raw = args.next().ok_or(
                    "--simulate-network requires a preset (fast-3g|slow-3g|cable|offline-local|<kbps>/<rtt_ms>)",
                )?;
                simulate_network_preset = Some(NetworkPreset::parse(&raw)?);
            }
            "--desktop" => {
                desktop_output = Some(PathBuf::from(
                    args.next()
                        .ok_or("--desktop requires an output directory")?,
                ))
            }
            "--lottie" => {
                lottie_output = Some(PathBuf::from(
                    args.next()
                        .ok_or("--lottie requires an output .json file")?,
                ))
            }
            "--video" => {
                video_output = Some(PathBuf::from(
                    args.next().ok_or("--video requires an output file path")?,
                ))
            }
            "--audio" => {
                audio_output = Some(PathBuf::from(
                    args.next()
                        .ok_or("--audio requires an output .wav/.mp3/.ogg/.aac file")?,
                ))
            }
            "--svg" => {
                svg_output = Some(PathBuf::from(
                    args.next().ok_or("--svg requires an output .svg file")?,
                ))
            }
            "--atlas" | "--sprite-sheet" => {
                atlas_output = Some(PathBuf::from(
                    args.next()
                        .ok_or("--atlas requires an output .json, .xml, or .css file")?,
                ))
            }
            "--atlas-format" => {
                let raw = args
                    .next()
                    .ok_or("--atlas-format requires json-hash|json-array|starling|easeljs|css")?;
                atlas_format_override = Some(match raw.to_ascii_lowercase().as_str() {
                    "json" | "json-hash" | "json_hash" | "hash" => AtlasFormat::JsonHash,
                    "json-array" | "json_array" | "array" => AtlasFormat::JsonArray,
                    "xml" | "starling" | "starling-xml" => AtlasFormat::StarlingXml,
                    "easel" | "easeljs" | "createjs" => AtlasFormat::EaselJs,
                    "css" => AtlasFormat::Css,
                    other => return Err(format!("Unsupported --atlas-format: {other}").into()),
                });
            }
            "--atlas-extrude" => {
                atlas_extrude = args
                    .next()
                    .ok_or("--atlas-extrude requires a pixel count")?
                    .parse()?;
            }
            "--banner" => {
                banner_output = Some(PathBuf::from(
                    args.next()
                        .ok_or("--banner requires an output .html file")?,
                ));
            }
            "--banner-max-gzip" => {
                banner_max_gzip = Some(
                    args.next()
                        .ok_or("--banner-max-gzip requires a byte limit")?
                        .parse()?,
                );
            }
            "--aep" | "--after-effects" => {
                aep_output = Some(PathBuf::from(
                    args.next()
                        .ok_or("--aep requires an output .aep, .aepx, or .jsx file")?,
                ));
            }
            "--sequence" => {
                sequence_output = Some(PathBuf::from(
                    args.next()
                        .ok_or("--sequence requires an output base file path")?,
                ));
            }
            "--audio-format" => {
                let fmt = args
                    .next()
                    .ok_or("--audio-format requires wav|mp3|ogg|aac")?;
                audio_format = Some(AudioFormat::parse(&fmt)?);
            }
            "--symbol" => {
                lottie_symbol = Some(args.next().ok_or("--symbol requires a symbol ID or name")?)
            }
            "--output" => {
                has_output = true;
                output = PathBuf::from(args.next().ok_or("--output requires a directory")?)
            }
            "--runtime" => {
                runtime = PathBuf::from(args.next().ok_or("--runtime requires a directory")?)
            }
            "--size-level" => {
                has_size_level = true;
                level = args.next().ok_or("--size-level requires a level")?;
            }
            "--runtime-url" => {
                runtime_url = Some(args.next().ok_or("--runtime-url requires a URL")?)
            }
            "--shared-runtime" => {
                shared_runtime = Some(PathBuf::from(
                    args.next().ok_or("--shared-runtime requires a directory")?,
                ))
            }
            "--scale-mode" => {
                let raw = args.next().ok_or("--scale-mode requires a mode")?;
                scale_mode = Some(match raw.to_ascii_lowercase().as_str() {
                    "fit" | "fit_keep_ratio" | "showall" | "show_all" => {
                        HtmlScaleMode::FitKeepRatio
                    }
                    "no-scale" | "no_scale" | "noscale" | "100%" => HtmlScaleMode::NoScale,
                    "exact-fit" | "exact_fit" | "exactfit" | "stretch" => HtmlScaleMode::ExactFit,
                    "fill-crop" | "fill_crop" | "noborder" | "no_border" => HtmlScaleMode::FillCrop,
                    other => return Err(format!("Unsupported --scale-mode: {other}").into()),
                });
            }
            "--stage-align" => {
                let raw = args.next().ok_or("--stage-align requires an alignment")?;
                stage_align = Some(match raw.to_ascii_uppercase().as_str() {
                    "" | "C" | "CENTER" => HtmlStageAlign::Center,
                    "T" | "TOP" => HtmlStageAlign::Top,
                    "B" | "BOTTOM" => HtmlStageAlign::Bottom,
                    "L" | "LEFT" => HtmlStageAlign::Left,
                    "R" | "RIGHT" => HtmlStageAlign::Right,
                    "TL" | "LT" | "TOP_LEFT" | "TOP-LEFT" => HtmlStageAlign::TopLeft,
                    "TR" | "RT" | "TOP_RIGHT" | "TOP-RIGHT" => HtmlStageAlign::TopRight,
                    "BL" | "LB" | "BOTTOM_LEFT" | "BOTTOM-LEFT" => HtmlStageAlign::BottomLeft,
                    "BR" | "RB" | "BOTTOM_RIGHT" | "BOTTOM-RIGHT" => HtmlStageAlign::BottomRight,
                    other => return Err(format!("Unsupported --stage-align: {other}").into()),
                });
            }
            "--no-fullscreen" => allow_fullscreen = Some(false),
            "--allow-fullscreen" => allow_fullscreen = Some(true),
            "--pack-sprite-sheet" | "--sprite-sheets" => pack_sprite_sheets = true,
            "--assets-folder" => assets_folder = true,
            "--help" | "-h" => {
                println!(
                    "neoflash-export <project.nflash> | --demo [--output DIR] [--runtime DIR] [--size-report]\n  [--size-level full|balanced|compact|shared] [--runtime-url URL] [--size-estimate]\n  [--export-profile FILE.json] [--import-profile FILE.json] [--profile NAME_OR_ID]\n  [--simulate-network fast-3g|slow-3g|cable|offline-local|<kbps>/<rtt_ms>]\n  [--lottie FILE.json] [--video FILE.mp4|.webm|.mov|.gif|.png] [--audio FILE.wav|.mp3|.ogg|.aac] [--audio-format wav|mp3|ogg|aac] [--symbol ID_OR_NAME] [--pretty]\nneoflash-export --shared-runtime DIR [--runtime DIR]"
                );
                return Ok(CliRunOutput::default());
            }
            _ if arg.starts_with('-') => return Err(format!("Unknown option: {arg}").into()),
            _ => {
                if source.replace(PathBuf::from(arg)).is_some() {
                    return Err("Supply one source project".into());
                }
            }
        }
    }
    if let Some(dir) = shared_runtime {
        let folder = neoflash_export::export_shared_runtime(&dir, &runtime)?;
        println!("Shared HTML5 runtime: {}", folder.display());
        return Ok(CliRunOutput::default());
    }
    if demo && source.is_some() {
        return Err("Choose a project or --demo".into());
    }
    let source_base_dir = source
        .as_ref()
        .and_then(|p| p.parent())
        .filter(|p| !p.as_os_str().is_empty())
        .map(std::path::Path::to_path_buf);
    let project = if demo {
        Project::demo()
    } else {
        let path = source.ok_or("Supply a project or --demo")?;
        if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("fla"))
        {
            neoflash_import::import_project(&path)?
        } else {
            serde_json::from_str(&fs::read_to_string(&path)?)?
        }
    };
    if let Some(desktop_dir) = desktop_output {
        let options = PublishOptions {
            fonts: FontExportPolicy::Embed,
            size: SizeLevel::Full,
            audio: AudioExportPolicy::Original,
        };
        let app_dir = neoflash_export::export_desktop_app(
            &project,
            &desktop_dir,
            &runtime,
            &options,
            &HtmlPublishConfig::clean_publish_default(),
        )?;
        println!("Desktop app export: {}", app_dir.display());
        return Ok(CliRunOutput::default());
    }
    if let Some(svg_path) = svg_output {
        let report = neoflash_export::export_project_svg(
            &project,
            &svg_path,
            &SvgOptions {
                symbol_id: lottie_symbol.clone(),
                ..SvgOptions::default()
            },
        )?;
        println!(
            "SVG export: {} ({} elements, {} defs, {}x{})",
            svg_path.display(),
            report.element_count,
            report.defs_count,
            report.width,
            report.height
        );
        return Ok(CliRunOutput::default());
    }
    if let Some(banner_path) = banner_output {
        let mut opts = BannerOptions::default();
        if let Some(max_gz) = banner_max_gzip {
            opts.max_gzip_bytes = Some(max_gz);
        }
        let report = neoflash_export::export_native_banner_html(&project, &banner_path, &opts)?;
        println!(
            "Native HTML5 banner: {} ({} bytes raw, {} bytes gzip, {} frames)",
            report.html_path.display(),
            report.raw_bytes,
            report.gzip_bytes,
            report.frame_count
        );
        return Ok(CliRunOutput::default());
    }
    if let Some(aep_path) = aep_output {
        let report = neoflash_export::export_project_aep(
            &project,
            &aep_path,
            &AepOptions {
                symbol: lottie_symbol.clone(),
                base_dir: source_base_dir.clone(),
                ..AepOptions::default()
            },
        )?;
        let total_layers = report.vector_shape_layer_count
            + report.text_layer_count
            + report.precomp_layer_count
            + report.footage_layer_count;
        println!(
            "After Effects export: {} ({} comps, {} layers, {} expressions, {} markers, {} footage)",
            report.aep_path.display(),
            report.composition_count,
            total_layers,
            report.expression_count,
            report.marker_count,
            report.footage_files.len()
        );
        for diag in &report.diagnostics {
            eprintln!("Note [{} @ {}]: {}", diag.code, diag.location, diag.message);
        }
        return Ok(CliRunOutput::default());
    }
    if let Some(seq_path) = sequence_output {
        let ext = seq_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("png")
            .to_ascii_lowercase();
        let format = match ext.as_str() {
            "jpg" | "jpeg" => ImageExportFormat::Jpeg,
            "webp" => ImageExportFormat::Webp,
            "gif" => ImageExportFormat::Gif,
            "bmp" => ImageExportFormat::Bmp,
            "svg" => ImageExportFormat::Svg,
            "ai" => ImageExportFormat::Ai,
            "pdf" => ImageExportFormat::Pdf,
            "psd" => ImageExportFormat::Psd,
            _ => ImageExportFormat::Png,
        };
        let mut opts = ImageExportOptions::for_project(&project, 0);
        opts.format = format;
        opts.range = if let Some(sym) = lottie_symbol.clone() {
            let fc = project
                .symbol(&sym)
                .map_or(1, |s| s.timeline.frame_count.max(1));
            opts.symbol_id = Some(sym);
            ImageExportRange::Frames {
                start: 0,
                end: fc.saturating_sub(1),
            }
        } else {
            opts.symbol_id = None;
            ImageExportRange::WholeMovie
        };
        let report = neoflash_export::export_project_image(&project, &seq_path, &opts)?;
        println!(
            "Sequence export: {} file(s) ({} bytes, {}x{}, {})",
            report.files.len(),
            report.total_bytes,
            report.width,
            report.height,
            report.format.label()
        );
        return Ok(CliRunOutput::default());
    }
    if let Some(atlas_path) = atlas_output {
        let ext = atlas_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("json")
            .to_ascii_lowercase();
        let format = atlas_format_override.unwrap_or(match ext.as_str() {
            "xml" => AtlasFormat::StarlingXml,
            "css" => AtlasFormat::Css,
            _ => AtlasFormat::JsonHash,
        });
        let report = neoflash_export::export_project_atlas(
            &project,
            &atlas_path,
            &AtlasOptions {
                symbol_id: lottie_symbol.clone(),
                format,
                extrude: atlas_extrude,
                ..AtlasOptions::default()
            },
        )?;
        println!(
            "Atlas export: {} + {} ({}x{}, {} frames, {} unique)",
            report.image_path.display(),
            report.metadata_path.display(),
            report.sheet_width,
            report.sheet_height,
            report.frame_count,
            report.unique_sprites
        );
        return Ok(CliRunOutput::default());
    }
    if let Some(lottie_path) = lottie_output {
        let report = neoflash_export::export_project_lottie(
            &project,
            &lottie_path,
            &LottieOptions {
                symbol: lottie_symbol,
                pretty: lottie_pretty,
                ..LottieOptions::default()
            },
        )?;
        println!(
            "Lottie export: {} ({} bytes, {} layers, {} frames)",
            lottie_path.display(),
            report.bytes,
            report.layer_count,
            report.frame_count
        );
        for warning in &report.warnings {
            eprintln!("Note: {warning}");
        }
        return Ok(CliRunOutput::default());
    }
    if let Some(video_path) = video_output {
        let format = VideoFormat::from_path(&video_path).unwrap_or(VideoFormat::Mp4);
        let report = neoflash_export::export_project_video(
            &project,
            &video_path,
            &VideoOptions {
                format,
                codec: format.default_codec(),
                symbol: lottie_symbol,
                ..VideoOptions::default()
            },
        )?;
        println!(
            "Video export: {} ({} bytes, {} frames, {}x{})",
            report.output_path.display(),
            report.bytes,
            report.frame_count,
            report.width,
            report.height
        );
        for warning in &report.warnings {
            eprintln!("Note: {warning}");
        }
        return Ok(CliRunOutput::default());
    }
    if let Some(audio_path) = audio_output {
        let inferred_format = audio_format.unwrap_or_else(|| {
            audio_path
                .extension()
                .and_then(|e| e.to_str())
                .and_then(|e| AudioFormat::parse(e).ok())
                .unwrap_or(AudioFormat::Mp3)
        });
        let report = neoflash_export::export_project_audio(
            &project,
            &audio_path,
            &AudioExportOptions {
                format: inferred_format,
                ..AudioExportOptions::default()
            },
        )?;
        println!(
            "Audio export: {} ({} bytes, {:.1} s, {} Hz, {} ch, {})",
            audio_path.display(),
            report.bytes,
            report.duration_seconds,
            report.sample_rate,
            report.channels,
            report.format.label()
        );
        return Ok(CliRunOutput::default());
    }
    if size_estimate {
        let url = runtime_url
            .as_deref()
            .unwrap_or("https://example.com/neoflash-runtime");
        let estimates = neoflash_export::estimate_publish_sizes(
            &project,
            &runtime,
            FontExportPolicy::Embed,
            url,
        )?;
        println!("{}", serde_json::to_string_pretty(&estimates)?);
        return Ok(CliRunOutput::default());
    }
    if size_report && !has_output {
        print!("{}", neoflash_export::format_size_report(&project)?);
        return Ok(CliRunOutput::default());
    }

    // Resolve active PublishProfile from --import-profile, stored project profiles, or CLI flags.
    let mut loaded_stored_profile = false;
    let mut active_profile = if let Some(ref import_path) = import_profile_path {
        let raw_json = fs::read_to_string(import_path)?;
        loaded_stored_profile = true;
        import_profile_json(&raw_json)?
    } else {
        let (stored_profiles, active_id) = load_profiles_from_project(&project)?;
        if let Some(ref selector) = profile_selector {
            loaded_stored_profile = true;
            stored_profiles
                .into_iter()
                .find(|p| p.id == *selector || p.name.eq_ignore_ascii_case(selector))
                .ok_or_else(|| format!("Publish profile '{selector}' not found in project"))?
        } else if let Some(ref act_id) = active_id {
            if let Some(found) = stored_profiles
                .iter()
                .find(|p| p.id == *act_id)
                .cloned()
                .or_else(|| stored_profiles.into_iter().next())
            {
                loaded_stored_profile = true;
                found
            } else {
                PublishProfile::default()
            }
        } else if let Some(first) = stored_profiles.into_iter().next() {
            loaded_stored_profile = true;
            first
        } else {
            PublishProfile::default()
        }
    };

    if has_size_level || !loaded_stored_profile {
        active_profile.options.size = SizeLevel::parse(&level, runtime_url.as_deref())?;
    }
    if let Some(preset) = simulate_network_preset {
        active_profile.network_profile.preset = preset;
    }

    let mut cli_out = CliRunOutput {
        active_profile: Some(active_profile.clone()),
        ..CliRunOutput::default()
    };

    if let Some(ref profile_out_path) = export_profile_path {
        let json = export_profile_json(&active_profile)?;
        if let Some(parent) = profile_out_path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(profile_out_path, &json)?;
        println!("Publish profile JSON: {}", profile_out_path.display());
        cli_out.exported_profile_path = Some(profile_out_path.clone());
        if !has_output && simulate_network_preset.is_none() {
            return Ok(cli_out);
        }
    }

    if let Some(preset) = simulate_network_preset {
        active_profile.network_profile.preset = preset;
        let sim_report = simulate_network_startup(&project, &active_profile)?;
        println!("Network startup simulation: {}", sim_report.summary_line());
        cli_out.network_estimate = Some(sim_report);
        if !has_output && !runtime.join("neoflash_web.js").is_file() {
            return Ok(cli_out);
        }
    }

    let options = active_profile.options.clone();
    let has_custom_html_cfg = clean
        || scale_mode.is_some()
        || stage_align.is_some()
        || allow_fullscreen.is_some()
        || pack_sprite_sheets
        || assets_folder;
    let index = if has_custom_html_cfg {
        let mut cfg = if clean {
            HtmlPublishConfig::clean_publish_default()
        } else {
            HtmlPublishConfig::default()
        };
        if let Some(sm) = scale_mode {
            cfg.scale_mode = sm;
        }
        if let Some(sa) = stage_align {
            cfg.stage_align = sa;
        }
        if let Some(fs_ok) = allow_fullscreen {
            cfg.allow_fullscreen = fs_ok;
        }
        if pack_sprite_sheets {
            cfg.sprite_sheets = true;
        }
        if assets_folder {
            cfg.assets_folder = true;
        }
        neoflash_export::export_project_with_config(&project, &output, &runtime, &options, &cfg)?
    } else if import_profile_path.is_some()
        || profile_selector.is_some()
        || active_profile.html_template.is_some()
    {
        export_project_with_profile(&project, &output, &runtime, &active_profile)?
    } else {
        neoflash_export::export_project_with_options(&project, &output, &runtime, &options)?
    };
    if let Some(ref estimate) = cli_out.network_estimate {
        let sim_path = output.join("network-simulation.json");
        fs::write(&sim_path, serde_json::to_string_pretty(estimate)?)?;
    }
    println!("HTML5 export: {}", index.display());
    cli_out.exported_index = Some(index);
    if size_report {
        let report = neoflash_export::format_size_report(&project)?;
        print!("{report}");
    }
    for warning in neoflash_export::export_warnings(&project) {
        eprintln!("Note: {warning}");
    }
    Ok(cli_out)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::new_id;
    use neoflash_export::{NetworkSimulationProfile, store_profiles_in_project};

    #[test]
    fn cli_export_import_profile_and_simulate_network_alongside_package() {
        let tmp = env::temp_dir().join(format!(
            "neoflash-cli-profile-sim-{}-{}",
            std::process::id(),
            new_id("cli")
        ));
        let rt_dir = tmp.join("runtime");
        let out_dir = tmp.join("dist");
        fs::create_dir_all(&rt_dir).unwrap();
        fs::write(rt_dir.join("neoflash_web.js"), "var NeoFlashWasm;").unwrap();
        fs::write(rt_dir.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();

        let mut project = Project::demo();
        let mut custom_profile = PublishProfile::new(
            "Studio Compact",
            PublishOptions {
                fonts: FontExportPolicy::Subset,
                size: SizeLevel::Compact,
                audio: AudioExportPolicy::Mp3_96,
            },
        );
        custom_profile.html_template = Some(
            "<!doctype html><html><head><title>Profile:$TITLE</title>$SCRIPT_BUNDLE\n$RUNTIME_SCRIPT</head><body><canvas id=\"stage\" width=\"$WIDTH\" height=\"$HEIGHT\"></canvas></body></html>"
                .into(),
        );
        custom_profile.network_profile = NetworkSimulationProfile {
            preset: NetworkPreset::Slow3G,
            delayed_assets: Vec::new(),
            extra_asset_delay_ms: 0,
        };
        store_profiles_in_project(
            &mut project,
            std::slice::from_ref(&custom_profile),
            Some(&custom_profile.id),
        )
        .unwrap();

        let proj_path = tmp.join("project.nflash");
        fs::write(&proj_path, serde_json::to_string_pretty(&project).unwrap()).unwrap();

        // 1. Export active profile JSON via --export-profile
        let exported_profile_json_path = tmp.join("exported-profile.json");
        let export_res = run_with_args([
            proj_path.to_str().unwrap(),
            "--export-profile",
            exported_profile_json_path.to_str().unwrap(),
        ])
        .expect("CLI --export-profile");
        assert_eq!(
            export_res.exported_profile_path.as_deref(),
            Some(exported_profile_json_path.as_path())
        );
        let roundtrip_profile =
            import_profile_json(&fs::read_to_string(&exported_profile_json_path).unwrap())
                .expect("valid exported profile JSON");
        assert_eq!(roundtrip_profile.name, "Studio Compact");
        assert_eq!(roundtrip_profile.options.size, SizeLevel::Compact);

        // 2. Import profile JSON via --import-profile and simulate network startup (--simulate-network)
        // alongside exported HTML5 package
        let blank_proj_path = tmp.join("blank.nflash");
        fs::write(
            &blank_proj_path,
            serde_json::to_string_pretty(&Project::demo()).unwrap(),
        )
        .unwrap();

        for (preset_flag, expected_preset) in [
            ("slow-3g", NetworkPreset::Slow3G),
            ("fast-3g", NetworkPreset::Fast3G),
            ("cable", NetworkPreset::Cable5Mbps),
            ("offline-local", NetworkPreset::OfflineLocal),
            (
                "2400/60",
                NetworkPreset::Custom {
                    bandwidth_kbps: 2400,
                    latency_ms: 60,
                },
            ),
        ] {
            let sub_out = out_dir.join(preset_flag.replace('/', "_"));
            let run_out = run_with_args([
                blank_proj_path.to_str().unwrap(),
                "--import-profile",
                exported_profile_json_path.to_str().unwrap(),
                "--simulate-network",
                preset_flag,
                "--output",
                sub_out.to_str().unwrap(),
                "--runtime",
                rt_dir.to_str().unwrap(),
            ])
            .expect("CLI --import-profile and --simulate-network");

            let index_path = run_out.exported_index.expect("exported index.html");
            assert!(index_path.is_file());
            let html = fs::read_to_string(&index_path).unwrap();
            assert!(
                html.contains("<title>Profile:"),
                "imported profile HTML template must be applied"
            );

            let estimate = run_out
                .network_estimate
                .expect("NetworkStartupEstimate returned");
            assert_eq!(estimate.preset, expected_preset);
            assert!(estimate.initial_html_js_wasm_bytes > 0);
            assert!(sub_out.join("network-simulation.json").is_file());
        }

        let _ = fs::remove_dir_all(tmp);
    }
}
