use neoflash_core::Project;
use neoflash_export::{
    AepOptions, AtlasFormat, AtlasOptions, AudioExportOptions, AudioExportPolicy, AudioFormat,
    BannerOptions, FontExportPolicy, HtmlPublishConfig, HtmlRenderMode, HtmlScaleMode,
    HtmlStageAlign, ImageExportFormat, ImageExportOptions, ImageExportRange, LottieOptions,
    NetworkPreset, NetworkStartupEstimate, PublishOptions, PublishProfile, SizeLevel, SvgOptions,
    VideoFormat, VideoOptions, XcodeExportOptions, XcodeScriptBundle, export_profile_json,
    export_project_with_profile, export_xcode_project, import_profile_json,
    load_profiles_from_project, simulate_network_startup,
};
use std::{env, fs, path::PathBuf};

/// Structured output returned by [`run_with_args`] for CLI execution and integration tests.
#[derive(Clone, Debug, Default)]
pub struct CliRunOutput {
    pub exported_index: Option<PathBuf>,
    pub exported_profile_path: Option<PathBuf>,
    pub exported_xcodeproj: Option<PathBuf>,
    pub active_profile: Option<PublishProfile>,
    pub network_estimate: Option<NetworkStartupEstimate>,
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    run_with_args(env::args().skip(1)).map(|_| ())
}

fn run_xcode_subcommand<I>(mut args: I) -> Result<CliRunOutput, Box<dyn std::error::Error>>
where
    I: Iterator<Item = String>,
{
    let mut source: Option<PathBuf> = None;
    let mut demo = false;
    let mut output = PathBuf::from("target/artifacts/xcode");
    let mut options = XcodeExportOptions::default();
    let mut scripts_json_path: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => demo = true,
            "--output" | "--out" | "-o" => {
                output = PathBuf::from(args.next().ok_or("--output requires a directory")?);
            }
            "--bundle-id-prefix" => {
                options.bundle_identifier_prefix = args
                    .next()
                    .ok_or("--bundle-id-prefix requires a reverse-DNS prefix")?;
            }
            "--overwrite-app-shell" => {
                options.overwrite_app_shell = true;
            }
            "--jpeg-quality" => {
                options.jpeg_quality = args
                    .next()
                    .ok_or("--jpeg-quality requires an integer 1..=100")?
                    .parse()?;
            }
            "--scripts-json" => {
                scripts_json_path = Some(PathBuf::from(
                    args.next()
                        .ok_or("--scripts-json requires an XcodeScriptBundle JSON path")?,
                ));
            }
            "--help" | "-h" => {
                println!(
                    "neoflash-export xcode (<project.nflash> | --demo) [--output DIR] [--bundle-id-prefix PREFIX] [--overwrite-app-shell] [--jpeg-quality 1..100] [--scripts-json FILE.json]  [Experimental]"
                );
                return Ok(CliRunOutput::default());
            }
            _ if arg.starts_with('-') => {
                return Err(format!("Unknown xcode option: {arg}").into());
            }
            _ => {
                if source.replace(PathBuf::from(arg)).is_some() {
                    return Err("Supply one source project".into());
                }
            }
        }
    }

    if demo && source.is_some() {
        return Err("Choose a project or --demo".into());
    }
    if options.base_dir.is_none() {
        options.base_dir = source
            .as_ref()
            .and_then(|p| p.parent())
            .filter(|p| !p.as_os_str().is_empty())
            .map(std::path::Path::to_path_buf);
    }
    let project = if demo {
        Project::demo()
    } else {
        let path = source.ok_or("Supply a project or --demo")?;
        let is_interchange = path.is_dir()
            || path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case("DOMDocument.xml"))
            || path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("fla")
                        || ext.eq_ignore_ascii_case("xfl")
                        || ext.eq_ignore_ascii_case("swf")
                });
        if is_interchange {
            neoflash_import::import_project(&path)?
                .with_resolved_script_language()
                .into_owned()
        } else {
            use std::io::Read as _;
            const MAX_NFLASH_CLI_BYTES: u64 = 512 * 1024 * 1024;
            let meta = fs::metadata(&path)?;
            if meta.len() > MAX_NFLASH_CLI_BYTES {
                return Err(format!(
                    "Project '{}' ({} bytes) exceeds 512 MiB limit",
                    path.display(),
                    meta.len()
                )
                .into());
            }
            let mut raw = Vec::with_capacity(meta.len() as usize);
            fs::File::open(&path)?
                .take(MAX_NFLASH_CLI_BYTES + 1)
                .read_to_end(&mut raw)?;
            if raw.len() as u64 > MAX_NFLASH_CLI_BYTES {
                return Err(format!(
                    "Project '{}' exceeded 512 MiB limit while reading",
                    path.display()
                )
                .into());
            }
            Project::from_nflash_bytes(&raw)?
                .with_resolved_script_language()
                .into_owned()
        }
    };
    let scripts: Option<XcodeScriptBundle> = match scripts_json_path {
        Some(path) => Some(serde_json::from_str(&fs::read_to_string(path)?)?),
        None => None,
    };
    let report = export_xcode_project(&project, &output, &options, scripts.as_ref())?;
    println!(
        "Xcode project export [Experimental]: {} (scheme {}, {} symbols, {} resources)",
        report.xcodeproj_path.display(),
        report.scheme_name,
        report.symbol_count,
        report.resource_files.len()
    );
    for item in &report.preflight_items {
        eprintln!("Note [{} @ {}]: {}", item.code, item.location, item.message);
    }
    Ok(CliRunOutput {
        exported_xcodeproj: Some(report.xcodeproj_path),
        ..CliRunOutput::default()
    })
}

pub fn run_with_args<I, S>(raw_args: I) -> Result<CliRunOutput, Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = raw_args.into_iter().map(Into::into).peekable();
    if args.peek().is_some_and(|first| first == "xcode") {
        let _ = args.next();
        return run_xcode_subcommand(args);
    }
    let mut source = None;
    let mut demo = false;
    let mut clean = false;
    let mut preview = false;
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
    let mut render_mode: Option<HtmlRenderMode> = None;
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
            "--preview" => preview = true,
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
            "--render-mode" => {
                let raw = args
                    .next()
                    .ok_or("--render-mode requires 'canvas' or 'dom'")?;
                render_mode = Some(HtmlRenderMode::parse(&raw)?);
            }
            "--pack-sprite-sheet" | "--sprite-sheets" => pack_sprite_sheets = true,
            "--assets-folder" => assets_folder = true,
            "--help" | "-h" => {
                println!(
                    "neoflash-export <project.nflash> | --demo [--output DIR] [--runtime DIR] [--size-report]\n  [--size-level full|balanced|compact|shared] [--runtime-url URL] [--size-estimate]\n  [--export-profile FILE.json] [--import-profile FILE.json] [--profile NAME_OR_ID]\n  [--simulate-network fast-3g|slow-3g|cable|offline-local|<kbps>/<rtt_ms>]\n  [--lottie FILE.json] [--video FILE.mp4|.webm|.mov|.gif|.png] [--audio FILE.wav|.mp3|.ogg|.aac] [--audio-format wav|mp3|ogg|aac] [--symbol ID_OR_NAME] [--pretty]\nneoflash-export xcode (<project.nflash> | --demo) [--output DIR] [--bundle-id-prefix PREFIX] [--overwrite-app-shell] [--jpeg-quality 1..100] [--scripts-json FILE.json]  [Experimental]\nneoflash-export --shared-runtime DIR [--runtime DIR]"
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
    let source_path_for_guards = source.clone();
    let source_base_dir = source
        .as_ref()
        .and_then(|p| p.parent())
        .filter(|p| !p.as_os_str().is_empty())
        .map(std::path::Path::to_path_buf);
    let mut project = if demo {
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
            report.output_path.display(),
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
    if let Some(rm) = render_mode {
        active_profile.render_mode = rm;
        // Canvas is also the config default, which otherwise inherits a saved DOM mode.
        if rm == HtmlRenderMode::Canvas && project.html_render_mode() != rm {
            project.set_html_render_mode(rm);
        }
    }

    let mut cli_out = CliRunOutput {
        active_profile: Some(active_profile.clone()),
        ..CliRunOutput::default()
    };

    if let Some(ref profile_out_path) = export_profile_path {
        if let Some(ref src_path) = source_path_for_guards
            && cli_paths_refer_to_same_file(src_path, profile_out_path)
        {
            return Err(format!(
                "Refusing to overwrite source project '{}' with --export-profile '{}'",
                src_path.display(),
                profile_out_path.display()
            )
            .into());
        }
        let json = export_profile_json(&active_profile)?;
        write_profile_json_atomically(profile_out_path, json.as_bytes())?;
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
    let host_override = clean
        || preview
        || scale_mode.is_some()
        || stage_align.is_some()
        || allow_fullscreen.is_some()
        || render_mode.is_some();
    let index = if host_override || pack_sprite_sheets || assets_folder {
        let mut cfg = if clean {
            HtmlPublishConfig::clean_publish_default()
        } else if preview {
            HtmlPublishConfig::preview_host_default()
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
        if let Some(rm) = render_mode {
            cfg.render_mode = rm;
        } else if active_profile.render_mode != HtmlRenderMode::Canvas {
            cfg.render_mode = active_profile.render_mode;
        }
        if pack_sprite_sheets {
            cfg.sprite_sheets = true;
        }
        if assets_folder {
            cfg.assets_folder = true;
        }
        let wrapper = active_profile
            .html_template
            .as_deref()
            .filter(|_| !host_override);
        neoflash_export::export_project_with_config_and_wrapper(
            &project, &output, &runtime, &options, &cfg, wrapper,
        )?
    } else if import_profile_path.is_some()
        || profile_selector.is_some()
        || active_profile.html_template.is_some()
        || active_profile.render_mode != HtmlRenderMode::Canvas
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

fn cli_paths_refer_to_same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    if a == b {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if let (Ok(ma), Ok(mb)) = (fs::metadata(a), fs::metadata(b))
            && ma.dev() == mb.dev()
            && ma.ino() == mb.ino()
        {
            return true;
        }
    }
    #[cfg(windows)]
    {
        if let (Some(ia), Some(ib)) = (windows_file_identity(a), windows_file_identity(b))
            && ia == ib
        {
            return true;
        }
    }
    if let (Ok(ca), Ok(cb)) = (fs::canonicalize(a), fs::canonicalize(b))
        && ca == cb
    {
        return true;
    }
    let resolve_parent_and_name = |p: &std::path::Path| -> Option<PathBuf> {
        let name = p.file_name()?;
        let parent = p.parent().filter(|par| !par.as_os_str().is_empty());
        let canon_parent = match parent {
            Some(par) => fs::canonicalize(par).ok()?,
            None => env::current_dir().ok()?.canonicalize().ok()?,
        };
        Some(canon_parent.join(name))
    };
    if let (Some(ra), Some(rb)) = (resolve_parent_and_name(a), resolve_parent_and_name(b)) {
        #[cfg(windows)]
        {
            return ra
                .to_string_lossy()
                .eq_ignore_ascii_case(&rb.to_string_lossy());
        }
        #[cfg(not(windows))]
        {
            return ra == rb;
        }
    }
    false
}

#[cfg(windows)]
fn windows_file_identity(path: &std::path::Path) -> Option<(u64, u64)> {
    use std::ffi::c_void;
    use std::mem::MaybeUninit;
    use std::os::windows::io::AsRawHandle as _;

    #[repr(C)]
    struct FileTime {
        low_date_time: u32,
        high_date_time: u32,
    }

    #[repr(C)]
    struct ByHandleFileInformation {
        file_attributes: u32,
        creation_time: FileTime,
        last_access_time: FileTime,
        last_write_time: FileTime,
        volume_serial_number: u32,
        file_size_high: u32,
        file_size_low: u32,
        number_of_links: u32,
        file_index_high: u32,
        file_index_low: u32,
    }

    unsafe extern "system" {
        fn GetFileInformationByHandle(
            file: *mut c_void,
            information: *mut ByHandleFileInformation,
        ) -> i32;
    }

    let file = fs::File::open(path).ok()?;
    let mut info = MaybeUninit::<ByHandleFileInformation>::uninit();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
        return None;
    }
    let info = unsafe { info.assume_init() };
    let idx = (u64::from(info.file_index_high) << 32) | u64::from(info.file_index_low);
    (idx != 0).then_some((u64::from(info.volume_serial_number), idx))
}

fn write_profile_json_atomically(
    dst: &std::path::Path,
    bytes: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;
    static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let parent = dst
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    fs::create_dir_all(parent)?;
    let name = dst
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("profile.json");
    let seq = TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let staging = parent.join(format!(
        ".{name}.neoflash-profile-tmp-{}-{seq}",
        std::process::id()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging)?;
    let write_res = (|| -> std::io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()
    })();
    drop(file);
    if let Err(err) = write_res {
        let _ = fs::remove_file(&staging);
        return Err(err.into());
    }
    if let Err(err) = fs::rename(&staging, dst) {
        let _ = fs::remove_file(&staging);
        return Err(err.into());
    }
    Ok(())
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

        // Asset layout changes the package without taking ownership of the profile's HTML.
        custom_profile.options.audio = AudioExportPolicy::Original;
        custom_profile.html_template = Some(
            "<title>Profile:$TITLE</title>$RUNTIME_SCRIPT<canvas id='stage'></canvas><script type='application/json' id='published'>$PROJECT_JSON</script>".into(),
        );
        fs::write(
            &exported_profile_json_path,
            export_profile_json(&custom_profile).unwrap(),
        )
        .unwrap();
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/animation/midnight-express.nflash");
        let assets = tmp.join("profile-assets");
        run_with_args([
            source.to_str().unwrap(),
            "--import-profile",
            exported_profile_json_path.to_str().unwrap(),
            "--size-level",
            "shared",
            "--runtime-url",
            "../player",
            "--assets-folder",
            "--pack-sprite-sheet",
            "--runtime",
            rt_dir.to_str().unwrap(),
            "--output",
            assets.to_str().unwrap(),
        ])
        .unwrap();
        let html = fs::read_to_string(assets.join("index.html")).unwrap();
        assert!(html.contains("<title>Profile:"));
        assert!(html.contains("src=\"../player/runtime.js\""));
        assert!(html.contains("src=\"bundle.js\""));
        let (_, block) = html.split_once("id='published'>").unwrap();
        let (payload, _) = block.split_once("</script>").unwrap();
        let embedded: serde_json::Value = serde_json::from_str(payload).unwrap();
        let raw = fs::read_to_string(assets.join("bundle.js")).unwrap();
        let json = raw
            .strip_prefix("globalThis.NeoFlashBundle = ")
            .unwrap()
            .strip_suffix(";\n")
            .unwrap();
        let bundle: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(embedded, bundle["project"]);
        assert_eq!(
            bundle["sprite_sheet"]["image_url"],
            "assets/sprite_sheet.png"
        );
        assert!(assets.join("assets/sprite_sheet.png").is_file());
        let audio_url = bundle["asset_urls"]["audio/whistle.wav"].as_str().unwrap();
        assert!(audio_url.starts_with("assets/") && assets.join(audio_url).is_file());

        project.set_html_render_mode(HtmlRenderMode::Dom);
        fs::write(&proj_path, project.to_json().unwrap()).unwrap();
        let canvas_out = tmp.join("canvas");
        run_with_args([
            proj_path.to_str().unwrap(),
            "--render-mode",
            "canvas",
            "--size-level",
            "full",
            "--runtime",
            rt_dir.to_str().unwrap(),
            "--output",
            canvas_out.to_str().unwrap(),
        ])
        .unwrap();
        let html = fs::read_to_string(canvas_out.join("index.html")).unwrap();
        assert!(html.contains("<canvas id=\"stage\""));
        assert!(!html.contains("<title>Profile:"));
        assert!(!html.contains("<neoflash-stage"));

        let _ = fs::remove_dir_all(tmp);
    }

    #[test]
    fn cli_xcode_subcommand_exports_valid_xcodeproj() {
        let tmp = env::temp_dir().join(format!(
            "neoflash-cli-xcode-{}-{}",
            std::process::id(),
            new_id("cli-xcode")
        ));
        let out_dir = tmp.join("XcodeOut");
        let res = run_with_args([
            "xcode",
            "--demo",
            "--output",
            out_dir.to_str().unwrap(),
            "--bundle-id-prefix",
            "org.neoflash.test",
            "--jpeg-quality",
            "90",
        ])
        .expect("CLI xcode subcommand");
        let xcodeproj = res.exported_xcodeproj.expect("exported .xcodeproj path");
        assert!(xcodeproj.join("project.pbxproj").is_file());
        assert!(
            out_dir
                .join("Generated/NeoFlashKit/NFDisplayList.swift")
                .is_file()
        );
        assert!(
            out_dir
                .join("Generated/Scenes/GeneratedScenes.swift")
                .is_file()
        );
        assert!(out_dir.join("App/ContentView.swift").is_file());
        assert!(out_dir.join("App/AppSwiftHost.swift").is_file());
        let _ = fs::remove_dir_all(tmp);
    }

    #[test]
    fn cli_export_profile_rejects_overwriting_source_project_and_aliases() {
        let tmp = env::temp_dir().join(format!(
            "neoflash-cli-profile-guard-{}-{}",
            std::process::id(),
            new_id("cli-guard")
        ));
        fs::create_dir_all(&tmp).unwrap();
        let proj_path = tmp.join("source.nflash");
        let original_bytes = Project::demo().to_json().unwrap().into_bytes();
        fs::write(&proj_path, &original_bytes).unwrap();

        let err = run_with_args([
            proj_path.to_str().unwrap(),
            "--export-profile",
            proj_path.to_str().unwrap(),
        ])
        .expect_err("identical input and --export-profile must be rejected");
        assert!(err.to_string().contains("Refusing to overwrite"));
        assert_eq!(fs::read(&proj_path).unwrap(), original_bytes);

        #[cfg(unix)]
        {
            let link_path = tmp.join("alias-source.nflash");
            std::os::unix::fs::symlink(&proj_path, &link_path).unwrap();
            let alias_err = run_with_args([
                proj_path.to_str().unwrap(),
                "--export-profile",
                link_path.to_str().unwrap(),
            ])
            .expect_err("symlink alias to source project must be rejected");
            assert!(alias_err.to_string().contains("Refusing to overwrite"));
            assert_eq!(fs::read(&proj_path).unwrap(), original_bytes);
        }

        let valid_profile_path = tmp.join("publish-profile.json");
        run_with_args([
            proj_path.to_str().unwrap(),
            "--export-profile",
            valid_profile_path.to_str().unwrap(),
        ])
        .expect("distinct JSON profile path succeeds");
        assert!(valid_profile_path.is_file());
        let saved_profile_bytes = fs::read(&valid_profile_path).unwrap();
        assert_eq!(fs::read(&proj_path).unwrap(), original_bytes);

        // [P2-01]: A failed write (e.g. directory collision) preserves existing profile & source bytes.
        let dir_collision = tmp.join("dir-target.json");
        fs::create_dir_all(&dir_collision).unwrap();
        assert!(
            run_with_args([
                proj_path.to_str().unwrap(),
                "--export-profile",
                dir_collision.to_str().unwrap(),
            ])
            .is_err()
        );
        assert_eq!(fs::read(&valid_profile_path).unwrap(), saved_profile_bytes);
        assert_eq!(fs::read(&proj_path).unwrap(), original_bytes);
        let _ = fs::remove_dir_all(tmp);
    }
}
