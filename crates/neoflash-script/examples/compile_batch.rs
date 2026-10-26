//! Compile a JSON array of Script objects from stdin, writing JSON results.
//! Used by the independent JavaScript execution checks in tests/execute.mjs.
use std::io::{self, Read};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let scripts: Vec<neoflash_core::Script> = serde_json::from_str(&input)?;
    let compiled: Vec<_> = scripts.iter().map(neoflash_script::compile).collect();
    serde_json::to_writer(io::stdout(), &compiled)?;
    Ok(())
}
