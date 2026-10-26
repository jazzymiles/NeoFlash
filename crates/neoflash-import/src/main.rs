use std::{env, fs::OpenOptions, io::Write, path::Path, process::ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.as_slice() {
        [command,path] if command=="inspect"=>{
            let report=neoflash_import::inspect(Path::new(path))?;
            println!("{}",serde_json::to_string_pretty(&report)?);
        },
        [command,path,destination] if command=="import"=>{
            let project=neoflash_import::import_project(Path::new(path))?;
            let json=project.to_json()?;
            // Creating a new output never overwrites a source or an existing project.
            let mut file=OpenOptions::new().create_new(true).write(true).open(destination)?;
            file.write_all(json.as_bytes())?;file.sync_all()?;
            println!("Imported {} symbols into {destination}",project.symbols.len());
        },
        _=>return Err("Usage: neoflash-import inspect PATH | neoflash-import import PATH NEW_OUTPUT.neoflash.json".into()),
    }
    Ok(())
}
