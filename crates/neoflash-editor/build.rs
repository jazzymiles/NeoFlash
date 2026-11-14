use std::path::Path;
use std::process::Command;

fn run_git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn watch_git_path(rel: &str) {
    if let Some(path_str) = run_git(&["rev-parse", "--git-path", rel]) {
        let path = Path::new(&path_str);
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=NEOFLASH_BUILD_NUMBER");
    println!("cargo:rerun-if-env-changed=NEOFLASH_GIT_COMMIT");

    watch_git_path("HEAD");
    if let Some(symref) = run_git(&["symbolic-ref", "-q", "HEAD"]) {
        watch_git_path(&symref);
    }
    watch_git_path("packed-refs");

    let commit = std::env::var("NEOFLASH_GIT_COMMIT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| run_git(&["rev-parse", "--verify", "HEAD"]))
        .unwrap_or_else(|| "dev".to_owned());

    let build_number = std::env::var("NEOFLASH_BUILD_NUMBER")
        .ok()
        .filter(|s| s.trim().parse::<u64>().is_ok_and(|n| n > 0))
        .or_else(|| {
            run_git(&["rev-list", "--count", "HEAD"])
                .filter(|s| s.trim().parse::<u64>().is_ok_and(|n| n > 0))
        })
        .unwrap_or_else(|| "0".to_owned());

    println!("cargo:rustc-env=NEOFLASH_GIT_COMMIT={}", commit.trim());
    println!(
        "cargo:rustc-env=NEOFLASH_BUILD_NUMBER={}",
        build_number.trim()
    );
}
