use neoflash_core::Project;
use neoflash_export::{
    AudioExportOptions, AudioExportPolicy, AudioFormat, FontExportPolicy, LottieOptions,
    PublishOptions, SizeLevel, VideoFormat, VideoOptions,
};
use std::{env, fs, path::PathBuf};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let mut source = None;
    let mut demo = false;
    let mut size_report = false;
    let mut size_estimate = false;
    let mut has_output = false;
    let mut level = "full".to_owned();
    let mut runtime_url = None;
    let mut shared_runtime = None;
    let mut lottie_output = None;
    let mut video_output = None;
    let mut audio_output = None;
    let mut audio_format = None;
    let mut lottie_symbol = None;
    let mut lottie_pretty = false;
    let mut output = PathBuf::from("dist/html5");
    let mut runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/web-runtime");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => demo = true,
            "--size-report" => size_report = true,
            "--size-estimate" => size_estimate = true,
            "--pretty" => lottie_pretty = true,
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
            "--size-level" => level = args.next().ok_or("--size-level requires a level")?,
            "--runtime-url" => {
                runtime_url = Some(args.next().ok_or("--runtime-url requires a URL")?)
            }
            "--shared-runtime" => {
                shared_runtime = Some(PathBuf::from(
                    args.next().ok_or("--shared-runtime requires a directory")?,
                ))
            }
            "--help" | "-h" => {
                println!(
                    "neoflash-export <project.nflash> | --demo [--output DIR] [--runtime DIR] [--size-report]\n  [--size-level full|balanced|compact|shared] [--runtime-url URL] [--size-estimate]\n  [--lottie FILE.json] [--video FILE.mp4|.webm|.mov|.gif|.png] [--audio FILE.wav|.mp3|.ogg|.aac] [--audio-format wav|mp3|ogg|aac] [--symbol ID_OR_NAME] [--pretty]\nneoflash-export --shared-runtime DIR [--runtime DIR]"
                );
                return Ok(());
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
        return Ok(());
    }
    if demo && source.is_some() {
        return Err("Choose a project or --demo".into());
    }
    let project = if demo {
        Project::demo()
    } else {
        serde_json::from_str(&fs::read_to_string(
            source.ok_or("Supply a project or --demo")?,
        )?)?
    };
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
        return Ok(());
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
        return Ok(());
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
        return Ok(());
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
        return Ok(());
    }
    if size_report && !has_output {
        print!("{}", neoflash_export::format_size_report(&project)?);
        return Ok(());
    }
    let options = PublishOptions {
        fonts: FontExportPolicy::Embed,
        size: SizeLevel::parse(&level, runtime_url.as_deref())?,
        audio: AudioExportPolicy::Original,
    };
    let index =
        neoflash_export::export_project_with_options(&project, &output, &runtime, &options)?;
    println!("HTML5 export: {}", index.display());
    if size_report {
        let report = neoflash_export::format_size_report(&project)?;
        print!("{report}");
    }
    for warning in neoflash_export::export_warnings(&project) {
        eprintln!("Note: {warning}");
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
