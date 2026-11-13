//! Windows Technical Preview boundary for the private Test Movie debugger.
//!
//! Ordinary Test Movie uses the platform browser handoff. The full debugger
//! requires securely inheriting Chromium's fd 3/fd 4-equivalent pipe handles;
//! that transport is not shipped until it can preserve the Unix isolation
//! contract without opening a TCP debugging endpoint.

use serde_json::Value;
use std::io;
use std::path::{Path, PathBuf};

const UNAVAILABLE: &str = "The private Test Movie debugger is not available in the Windows Technical Preview yet. Use Test Movie to run the movie; debugging remains available on macOS and Linux.";

pub(crate) use crate::platform_open::file_url;

pub(crate) struct BrowserPipe;

pub(crate) fn browser_executable() -> Result<PathBuf, String> {
    Err(UNAVAILABLE.into())
}

pub(crate) fn private_directory() -> io::Result<PathBuf> {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    for _ in 0..64 {
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("neoflash-debug-{}-{sequence}", std::process::id()));
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a private debugger directory",
    ))
}

impl BrowserPipe {
    pub(crate) fn launch(_executable: &Path, _headless: bool) -> io::Result<Self> {
        Err(io::Error::new(io::ErrorKind::Unsupported, UNAVAILABLE))
    }

    pub(crate) fn send(&mut self, _message: &Value) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, UNAVAILABLE))
    }

    pub(crate) fn receive(&mut self) -> io::Result<Option<Value>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, UNAVAILABLE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t289_windows_preview_reports_the_private_debugger_boundary_without_tcp_fallback() {
        let error = browser_executable().unwrap_err();
        assert!(error.contains("Windows Technical Preview"));
        assert!(error.contains("Use Test Movie"));
        assert!(!error.contains("port"));
    }
}
