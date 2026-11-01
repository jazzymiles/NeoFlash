use neoflash_core::Project;
use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    process::ExitCode,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn load_or_import_project(path: &Path) -> Result<Project, Box<dyn std::error::Error>> {
    if path.is_file()
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("nflash") || e.eq_ignore_ascii_case("json"))
    {
        let json = fs::read_to_string(path)?;
        return Ok(Project::from_json(&json)?);
    }
    Ok(neoflash_import::import_project(path)?)
}

fn is_fla_destination(dest: &str) -> bool {
    Path::new(dest)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("fla"))
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.as_slice() {
        [command, path] if command == "inspect" => {
            let report = neoflash_import::inspect(Path::new(path))?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        [command, path, destination] if command == "import" => {
            let project = neoflash_import::import_project(Path::new(path))?;
            let bytes = if is_fla_destination(destination) {
                neoflash_import::export_fla_bytes(&project)?
            } else {
                project.to_json()?.into_bytes()
            };
            // Creating a new output never overwrites a source or an existing project.
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(destination)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            println!(
                "Imported {} symbols into {destination}",
                project.symbols.len()
            );
        }
        [command, path, destination] if command == "export-fla" => {
            let project = load_or_import_project(Path::new(path))?;
            let bytes = neoflash_import::export_fla_bytes(&project)?;
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(destination)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            println!(
                "Exported {} symbols into {destination}",
                project.symbols.len()
            );
        }
        _ => {
            return Err(
                "Usage: neoflash-import inspect PATH | neoflash-import import PATH NEW_OUTPUT.{nflash|json|fla} | neoflash-import export-fla INPUT NEW_OUTPUT.fla"
                    .into(),
            );
        }
    }
    Ok(())
}
