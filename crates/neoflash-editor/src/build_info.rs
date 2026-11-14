//! Real build identity shared by About NeoFlash and Preferences ▸ Updates.

use serde::Deserialize;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BuildIdentity {
    pub version: String,
    pub build_number: Option<u64>,
    pub commit: String,
}

impl BuildIdentity {
    /// Resolve the running build's version, commit count, and git commit from packaged
    /// metadata (`Info.plist` / `BUILD_METADATA.json`) with compile-time git fallback.
    pub(crate) fn current() -> Self {
        let exe = std::env::current_exe().ok();
        let mut build_number = option_env!("NEOFLASH_BUILD_NUMBER")
            .and_then(|s| s.trim().parse::<u64>().ok())
            .filter(|&n| n > 0);
        let mut commit = option_env!("NEOFLASH_GIT_COMMIT")
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "unknown")
            .unwrap_or("dev")
            .to_owned();

        if let Some(exe_path) = exe.as_deref()
            && let Some((pkg_build, pkg_commit)) = read_packaged_metadata(exe_path)
        {
            if let Some(n) = pkg_build.filter(|&n| n > 0) {
                build_number = Some(n);
            }
            if let Some(c) = pkg_commit.filter(|s| !s.trim().is_empty()) {
                commit = c;
            }
        }

        Self {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            build_number,
            commit,
        }
    }

    pub(crate) fn commit_short(&self) -> &str {
        let c = self.commit.trim();
        if c.is_empty() {
            return "dev";
        }
        let end = c
            .char_indices()
            .nth(7)
            .map(|(idx, _)| idx)
            .unwrap_or(c.len());
        &c[..end]
    }

    pub(crate) fn build_label(&self) -> String {
        match self.build_number {
            Some(n) if n > 0 => format!("build {n}"),
            _ => "local build".to_owned(),
        }
    }

    /// Version + build + short commit string shown in `Preferences ▸ Updates` and tests.
    pub(crate) fn version_prefix(&self) -> String {
        format!(
            "NeoFlash {} · {} ({})",
            self.version,
            self.build_label(),
            self.commit_short()
        )
    }

    /// Subtitle displayed in `About NeoFlash` so commit/build match `Preferences ▸ Updates`.
    pub(crate) fn about_subtitle(&self) -> String {
        format!(
            "{} · commit {} · Open source · MIT OR Apache-2.0",
            match self.build_number {
                Some(n) if n > 0 => format!("Build {n}"),
                _ => "Local build".to_owned(),
            },
            self.commit_short()
        )
    }
}

fn read_packaged_metadata(exe_path: &Path) -> Option<(Option<u64>, Option<String>)> {
    let exe_dir = exe_path.parent()?;
    // Windows / Linux portable directory carries `BUILD_METADATA.json` beside the binary.
    let json_path = exe_dir.join("BUILD_METADATA.json");
    if let Ok(raw) = std::fs::read_to_string(json_path)
        && let Some(parsed) = parse_portable_build_metadata(&raw)
    {
        return Some(parsed);
    }
    // macOS `.app` places `Info.plist` at `Contents/Info.plist` (`exe_dir` = `Contents/MacOS`).
    if let Some(contents_dir) = exe_dir.parent() {
        let plist_path = contents_dir.join("Info.plist");
        if let Ok(raw) = std::fs::read_to_string(plist_path)
            && let Some(parsed) = parse_macos_info_plist(&raw)
        {
            return Some(parsed);
        }
    }
    None
}

#[derive(Deserialize)]
struct PortableBuildMetadata {
    build_number: Option<u64>,
    commit: Option<String>,
}

pub(crate) fn parse_portable_build_metadata(raw: &str) -> Option<(Option<u64>, Option<String>)> {
    let parsed: PortableBuildMetadata = serde_json::from_str(raw).ok()?;
    let commit = parsed
        .commit
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    Some((parsed.build_number.filter(|&n| n > 0), commit))
}

pub(crate) fn parse_macos_info_plist(raw: &str) -> Option<(Option<u64>, Option<String>)> {
    let build = plist_string_value(raw, "CFBundleVersion")
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|&n| n > 0);
    let commit = plist_string_value(raw, "NeoFlashGitCommit")
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    if build.is_some() || commit.is_some() {
        Some((build, commit))
    } else {
        None
    }
}

fn plist_string_value(plist: &str, key: &str) -> Option<String> {
    let needle = format!("<key>{key}</key>");
    let (_, after_key) = plist.split_once(&needle)?;
    let (_, after_start) = after_key.split_once("<string>")?;
    let (value, _) = after_start.split_once("</string>")?;
    Some(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_identity_uses_real_package_version_and_parsers() {
        let current = BuildIdentity::current();
        assert_eq!(current.version, env!("CARGO_PKG_VERSION"));
        assert!(!current.commit_short().is_empty());
        assert!(current.version_prefix().contains(&current.version));
        assert!(current.about_subtitle().contains(current.commit_short()));

        let plist = r#"<?xml version="1.0"?>
<plist version="1.0"><dict>
<key>CFBundleVersion</key><string>1482</string>
<key>NeoFlashGitCommit</key><string>abcdef1234567890</string>
</dict></plist>"#;
        assert_eq!(
            parse_macos_info_plist(plist),
            Some((Some(1482), Some("abcdef1234567890".into())))
        );

        let portable = r#"{"build_number": 901, "commit": "1234567890abcdef"}"#;
        assert_eq!(
            parse_portable_build_metadata(portable),
            Some((Some(901), Some("1234567890abcdef".into())))
        );
    }
}
