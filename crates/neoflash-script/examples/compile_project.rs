//! Compile an immutable project plus extra handler fixtures from JSON stdin.
use neoflash_core::{Project, Script};
use serde::{Deserialize, Serialize};
use std::io::{self, Read};
#[derive(Deserialize)]
struct Input {
    project: Project,
    #[serde(default)]
    scripts: Vec<Script>,
}
#[derive(Serialize)]
struct Output {
    code: String,
    class_names: Vec<String>,
    scripts: Vec<String>,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let input: Input = serde_json::from_str(&input)?;
    let result = neoflash_script::compile_project(&input.project).and_then(|program| {
        let scripts = input
            .scripts
            .iter()
            .map(|s| neoflash_script::compile_with_program(s, &program))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Output {
            code: program.code,
            class_names: program.class_names,
            scripts,
        })
    });
    serde_json::to_writer(io::stdout(), &result)?;
    Ok(())
}
