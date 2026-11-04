use neoflash_core::Project;
use neoflash_export::{FontExportPolicy, PublishOptions, SizeLevel};
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
    let mut output = PathBuf::from("dist/html5");
    let mut runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/web-runtime");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => demo = true,
            "--size-report" => size_report = true,
            "--size-estimate" => size_estimate = true,
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
                    "neoflash-export <project.nflash> | --demo [--output DIR] [--runtime DIR] [--size-report]\n  [--size-level full|balanced|compact|shared] [--runtime-url URL] [--size-estimate]\nneoflash-export --shared-runtime DIR [--runtime DIR]"
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
