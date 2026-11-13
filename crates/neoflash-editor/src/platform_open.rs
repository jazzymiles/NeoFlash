//! Cross-platform browser handoff for generated previews and local Help.

use std::io;
use std::path::Path;

/// Convert an existing local path to a standards-compliant `file:` URL.
///
/// `url` owns the platform-specific drive/UNC handling, while canonicalization
/// makes relative callers deterministic and rejects paths that were not written.
pub(crate) fn file_url(path: &Path) -> io::Result<String> {
    let canonical = std::fs::canonicalize(path)?;
    url::Url::from_file_path(&canonical)
        .map(String::from)
        .map_err(|()| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "path cannot be represented as a file URL",
            )
        })
}

pub(crate) fn file_url_with_fragment(path: &Path, fragment: &str) -> io::Result<String> {
    let canonical = std::fs::canonicalize(path)?;
    let mut url = url::Url::from_file_path(&canonical).map_err(|()| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "path cannot be represented as a file URL",
        )
    })?;
    url.set_fragment(Some(fragment));
    Ok(url.into())
}

pub(crate) fn open_url(url: &str) -> Result<(), String> {
    webbrowser::open(url).map_err(|error| error.to_string())
}

pub(crate) fn open_file(path: &Path) -> Result<(), String> {
    let url = file_url(path).map_err(|error| error.to_string())?;
    open_url(&url)
}

#[cfg(test)]
mod tests {
    use super::*;

    static FIXTURE_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

    struct Fixture(std::path::PathBuf);

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn t289_file_urls_encode_unicode_spaces_and_fragments_without_losing_path_identity() {
        let root = std::env::temp_dir().join(format!(
            "neoflash-platform-url-{}-{}",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let fixture = Fixture(root);
        let path = fixture.0.join("Пример folder").join("电影 🎬.html");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "<!doctype html>").unwrap();

        let plain = file_url(&path).unwrap();
        let parsed = url::Url::parse(&plain).unwrap();
        assert_eq!(parsed.scheme(), "file");
        // Windows canonical paths use the extended `\\?\` form, while the
        // standards-compliant file URL round-trip returns an ordinary drive
        // path. Canonicalize the decoded path again before comparing identity;
        // the two spellings address the same file.
        assert_eq!(
            std::fs::canonicalize(parsed.to_file_path().unwrap()).unwrap(),
            std::fs::canonicalize(&path).unwrap()
        );
        assert!(plain.contains("%20"));

        let routed = file_url_with_fragment(&path, "chapter-19 / β").unwrap();
        let parsed = url::Url::parse(&routed).unwrap();
        assert_eq!(parsed.fragment(), Some("chapter-19%20/%20%CE%B2"));
        assert_eq!(
            std::fs::canonicalize(parsed.to_file_path().unwrap()).unwrap(),
            std::fs::canonicalize(path).unwrap()
        );

        #[cfg(target_os = "windows")]
        assert!(plain.starts_with("file:///"), "{plain}");
        #[cfg(not(target_os = "windows"))]
        assert!(plain.starts_with("file:///"), "{plain}");
    }
}
