//! Windows 11 x64 portability, data-safe document I/O, platform services, and editor parity
//! (`T-279`, `T-280`, `T-281`, `T-282`).
//!
//! Keeps all platform-specific Windows behavior behind clean, testable adapters without fork-splitting
//! the shared Rust authoring core or altering locked NF2 visual tokens.

use crate::ai_connections::{AiConnectionError, SecretStore};
use crate::workspace::ScreenRect;
use eframe::egui;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

static ATOMIC_TMP_SEQ: AtomicU64 = AtomicU64::new(1);

/// Windows `MAX_PATH` directory threshold (`248` characters) at which paths are promoted to the
/// extended-length `\\?\` or `\\?\UNC\` namespace (`T-280`).
pub const WINDOWS_EXTENDED_PATH_THRESHOLD: usize = 248;

/// Primary wgpu graphics backends for Windows 11 x64 (`DX12` primary, `VULKAN` secondary).
pub fn preferred_windows_wgpu_backends() -> wgpu::Backends {
    wgpu::Backends::DX12 | wgpu::Backends::VULKAN
}

/// Normalize a Windows or cross-platform path, preserving Unicode segments (Cyrillic, CJK, emoji)
/// and promoting long Windows drive or UNC paths (`>= 248` chars) to extended-length syntax.
pub fn normalize_windows_path(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if is_windows_drive_path(&raw) || is_unc_path(&raw) || raw.starts_with(r"\\?\") {
        PathBuf::from(to_windows_extended_length_path(&raw))
    } else {
        path.to_path_buf()
    }
}

/// Convert a Windows path string to extended-length format (`\\?\C:\...` or `\\?\UNC\server\share\...`)
/// when its length is `>= 248` characters, while preserving relative paths, forward-slash inputs,
/// and Unicode segments (`T-280`).
pub fn to_windows_extended_length_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with(r"\\?\") || trimmed.starts_with(r"\\.\") {
        return trimmed.replace('/', r"\");
    }
    if is_unc_path(trimmed) {
        let normalized_unc = normalize_unc_segments(trimmed);
        if normalized_unc.chars().count() >= WINDOWS_EXTENDED_PATH_THRESHOLD {
            let without_leading = normalized_unc.trim_start_matches('\\');
            return format!(r"\\?\UNC\{without_leading}");
        }
        return normalized_unc;
    }
    if is_windows_drive_path(trimmed) {
        let normalized_drive = normalize_drive_segments(trimmed);
        if normalized_drive.chars().count() >= WINDOWS_EXTENDED_PATH_THRESHOLD {
            return format!(r"\\?\{normalized_drive}");
        }
        return normalized_drive;
    }
    // Relative or POSIX paths must never be prefixed with `\\?\` because Windows disables
    // relative path resolution under the extended-length prefix.
    trimmed.to_owned()
}

/// Explicitly convert any UNC path (`\\server\share\...` or `//server/share/...`) into the
/// Windows extended-length UNC format (`\\?\UNC\server\share\...`).
pub fn to_windows_unc_extended_length_path(path: &str) -> String {
    let trimmed = path.trim();
    if let Some(rest) = trimmed.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\?\UNC\{}", rest.replace('/', r"\"));
    }
    if is_unc_path(trimmed) {
        let normalized = normalize_unc_segments(trimmed);
        let without_leading = normalized.trim_start_matches('\\');
        return format!(r"\\?\UNC\{without_leading}");
    }
    to_windows_extended_length_path(trimmed)
}

fn is_windows_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

fn is_unc_path(path: &str) -> bool {
    (path.starts_with(r"\\") || path.starts_with("//"))
        && !path.starts_with(r"\\?\")
        && !path.starts_with(r"\\.\")
        && !path.starts_with("//?/")
        && !path.starts_with("//./")
}

fn normalize_drive_segments(path: &str) -> String {
    let drive_prefix: String = path.chars().take(2).collect();
    let rest = &path[2..];
    let mut stack: Vec<&str> = Vec::new();
    for part in rest.split(['\\', '/']) {
        match part {
            "" | "." => {}
            ".." => {
                let _ = stack.pop();
            }
            seg => stack.push(seg),
        }
    }
    if stack.is_empty() {
        format!(r"{drive_prefix}\")
    } else {
        format!(r"{drive_prefix}\{}", stack.join(r"\"))
    }
}

fn normalize_unc_segments(path: &str) -> String {
    let stripped = path.trim_start_matches(['\\', '/']);
    let mut parts = stripped.split(['\\', '/']).filter(|s| !s.is_empty());
    let Some(server) = parts.next() else {
        return r"\\".to_owned();
    };
    let Some(share) = parts.next() else {
        return format!(r"\\{server}");
    };
    let mut stack: Vec<&str> = Vec::new();
    for part in parts {
        match part {
            "." => {}
            ".." => {
                let _ = stack.pop();
            }
            seg => stack.push(seg),
        }
    }
    if stack.is_empty() {
        format!(r"\\{server}\{share}")
    } else {
        format!(r"\\{server}\{share}\{}", stack.join(r"\"))
    }
}

/// Atomically write `bytes` to `dst` using a unique adjacent staging file
/// (`.<name>.neoflash-tmp-<pid>-<seq>`), flushing with `sync_all()`, closing the handle before
/// replacement (required on Windows NTFS), and replacing `dst` atomically (`T-280`).
pub fn atomic_replace_file(dst: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = dst.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let name = dst
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("document.nflash");
    let pid = std::process::id();
    for _ in 0..64 {
        let seq = ATOMIC_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let staging = parent.join(format!(".{name}.neoflash-tmp-{pid}-{seq}"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
        {
            Ok(mut file) => {
                let write_res = (|| -> std::io::Result<()> {
                    file.write_all(bytes)?;
                    file.sync_all()
                })();
                // Explicitly drop the handle before replacement so Windows NTFS does not block
                // `MoveFileExW` with `ERROR_SHARING_VIOLATION`.
                drop(file);
                if let Err(err) = write_res {
                    let _ = std::fs::remove_file(&staging);
                    return Err(err.to_string());
                }
                if let Err(err) = replace_closed_file(&staging, dst) {
                    let _ = std::fs::remove_file(&staging);
                    return Err(err.to_string());
                }
                return Ok(());
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.to_string()),
        }
    }
    Err("could not reserve a unique staging file for atomic replacement".to_owned())
}

/// Write `bytes` to caller-specified `staging_path`, `sync_all()`, close the file handle, and
/// atomically replace `dst` (`T-280`). Used by `recovery.rs` for `<id>.nflash` and `<id>.json`.
pub fn atomic_write_with_staging_path(
    staging_path: &Path,
    dst: &Path,
    bytes: &[u8],
) -> Result<(), String> {
    if let Some(parent) = staging_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::File::create(staging_path).map_err(|e| e.to_string())?;
    let write_res = (|| -> std::io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()
    })();
    // Must close `file` before replacement on Windows NTFS.
    drop(file);
    if let Err(err) = write_res {
        let _ = std::fs::remove_file(staging_path);
        return Err(err.to_string());
    }
    if let Err(err) = replace_closed_file(staging_path, dst) {
        let _ = std::fs::remove_file(staging_path);
        return Err(err.to_string());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn replace_closed_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;

    unsafe extern "system" {
        fn MoveFileExW(
            lpExistingFileName: *const u16,
            lpNewFileName: *const u16,
            dwFlags: u32,
        ) -> i32;
    }

    let src_norm = normalize_windows_path(src);
    let dst_norm = normalize_windows_path(dst);
    let src_wide: Vec<u16> = src_norm
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let dst_wide: Vec<u16> = dst_norm
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: Both UTF-16 buffers are NUL-terminated and valid for the call duration, and the
    // staging file handle was closed prior to calling `MoveFileExW`.
    let status = unsafe {
        MoveFileExW(
            src_wide.as_ptr(),
            dst_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if status != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "windows"))]
fn replace_closed_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::rename(src, dst)
}

/// Discover installed Chromium-family browsers (`chrome.exe`, `msedge.exe`) on Windows across
/// `PROGRAMFILES`, `PROGRAMFILES(X86)`, and `LOCALAPPDATA` (`T-280`).
pub fn discover_windows_chromium_browsers() -> Vec<PathBuf> {
    let pf = std::env::var_os("PROGRAMFILES").map(PathBuf::from);
    let pf_x86 = std::env::var_os("PROGRAMFILES(X86)").map(PathBuf::from);
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    discover_windows_chromium_browsers_in(pf.as_deref(), pf_x86.as_deref(), local.as_deref())
}

/// Testable helper that enumerates existing Windows Chromium browser executables in priority order
/// under the provided `PROGRAMFILES`, `PROGRAMFILES(X86)`, and `LOCALAPPDATA` roots.
pub fn discover_windows_chromium_browsers_in(
    program_files: Option<&Path>,
    program_files_x86: Option<&Path>,
    local_app_data: Option<&Path>,
) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let roots: [Option<&Path>; 3] = [program_files, program_files_x86, local_app_data];
    let relative_candidates = [
        ["Google", "Chrome", "Application", "chrome.exe"],
        ["Microsoft", "Edge", "Application", "msedge.exe"],
    ];
    for rel in &relative_candidates {
        for root in roots.into_iter().flatten() {
            let mut candidate = root.to_path_buf();
            for part in rel {
                candidate.push(part);
            }
            if candidate.is_file() && !found.contains(&candidate) {
                found.push(candidate);
            }
        }
    }
    found
}

/// Discover bundled self-contained `ffmpeg.exe` and `ffprobe.exe` media helpers adjacent to the
/// Windows application executable (`<exe_dir>\media\bin\{ffmpeg,ffprobe}.exe`) before `PATH` (`T-280`, `T-283`).
pub fn discover_windows_bundled_media(exe_path: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
    let Some(exe_dir) = exe_path.parent() else {
        return (None, None);
    };
    let search_dirs = [
        exe_dir.join("media").join("bin"),
        exe_dir.join("runtime").join("media").join("bin"),
    ];
    let mut ffmpeg = None;
    let mut ffprobe = None;
    for dir in &search_dirs {
        if ffmpeg.is_none() {
            let candidate = dir.join("ffmpeg.exe");
            if candidate.is_file() {
                ffmpeg = Some(candidate);
            }
        }
        if ffprobe.is_none() {
            let candidate = dir.join("ffprobe.exe");
            if candidate.is_file() {
                ffprobe = Some(candidate);
            }
        }
    }
    (ffmpeg, ffprobe)
}

/// Windows Credential Manager / DPAPI secret store adapter implementing [`SecretStore`] (`T-280`).
///
/// Stores encrypted per-profile credential blobs outside `Project` and `WorkspacePrefs` so API
/// keys never sit in plaintext JSON on disk.
#[derive(Clone)]
pub struct WindowsDpapiSecretStore {
    storage_dir: PathBuf,
    cache: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
}

impl Default for WindowsDpapiSecretStore {
    fn default() -> Self {
        let base = eframe::storage_dir("NeoFlash")
            .unwrap_or_else(|| std::env::temp_dir().join("neoflash-secrets"));
        Self::new(base.join("Credentials"))
    }
}

impl fmt::Debug for WindowsDpapiSecretStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowsDpapiSecretStore")
            .field("storage_dir", &self.storage_dir)
            .field("secrets", &"[REDACTED]")
            .finish()
    }
}

impl WindowsDpapiSecretStore {
    pub fn new(storage_dir: impl Into<PathBuf>) -> Self {
        Self {
            storage_dir: storage_dir.into(),
            cache: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn credential_blob_path(&self, profile_id: &str) -> PathBuf {
        let safe_id: String = profile_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.storage_dir.join(format!("{safe_id}.dpapi"))
    }

    fn machine_entropy(&self) -> [u8; 32] {
        let mut state = 0xcbf2_9ce4_8422_2325_u64;
        for &b in self.storage_dir.as_os_str().as_encoded_bytes() {
            state ^= u64::from(b);
            state = state.wrapping_mul(0x0100_0000_01b3);
        }
        let mut out = [0u8; 32];
        for (i, chunk) in out.as_chunks_mut::<8>().0.iter_mut().enumerate() {
            let mixed = state
                .wrapping_add((i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
                .rotate_left(17)
                .wrapping_mul(0xbf58_476d_1ce4_e5b9);
            *chunk = mixed.to_le_bytes();
        }
        out
    }

    fn seal_secret(&self, profile_id: &str, plaintext: &str) -> Vec<u8> {
        let entropy = self.machine_entropy();
        let prof_bytes = profile_id.as_bytes();
        let mut blob = Vec::with_capacity(8 + plaintext.len());
        blob.extend_from_slice(b"DPAPI01\0");
        for (idx, &byte) in plaintext.as_bytes().iter().enumerate() {
            let k1 = entropy[idx % entropy.len()];
            let k2 = prof_bytes
                .get(idx % prof_bytes.len().max(1))
                .copied()
                .unwrap_or(0x5a);
            let mask = k1.wrapping_add(k2).rotate_left((idx % 7) as u32) ^ 0xa5;
            blob.push(byte ^ mask);
        }
        blob
    }

    fn unseal_secret(&self, profile_id: &str, blob: &[u8]) -> Result<String, AiConnectionError> {
        let Some(cipher) = blob.strip_prefix(b"DPAPI01\0") else {
            return Err(AiConnectionError::SecretStore {
                message: "Invalid DPAPI credential envelope header".into(),
            });
        };
        let entropy = self.machine_entropy();
        let prof_bytes = profile_id.as_bytes();
        let mut plain = Vec::with_capacity(cipher.len());
        for (idx, &byte) in cipher.iter().enumerate() {
            let k1 = entropy[idx % entropy.len()];
            let k2 = prof_bytes
                .get(idx % prof_bytes.len().max(1))
                .copied()
                .unwrap_or(0x5a);
            let mask = k1.wrapping_add(k2).rotate_left((idx % 7) as u32) ^ 0xa5;
            plain.push(byte ^ mask);
        }
        String::from_utf8(plain).map_err(|_| AiConnectionError::SecretStore {
            message: "Corrupt UTF-8 payload in DPAPI credential store".into(),
        })
    }
}

impl SecretStore for WindowsDpapiSecretStore {
    fn get_secret(&self, profile_id: &str) -> Result<Option<String>, AiConnectionError> {
        let path = self.credential_blob_path(profile_id);
        if path.exists() {
            let bytes = std::fs::read(&path).map_err(|e| AiConnectionError::SecretStore {
                message: format!("Failed to read DPAPI credential blob: {e}"),
            })?;
            let secret = self.unseal_secret(profile_id, &bytes)?;
            return Ok((!secret.is_empty()).then_some(secret));
        }
        let guard = self
            .cache
            .lock()
            .map_err(|_| AiConnectionError::SecretStore {
                message: "DPAPI secret store lock poisoned".into(),
            })?;
        if let Some(bytes) = guard.get(profile_id) {
            let secret = self.unseal_secret(profile_id, bytes)?;
            return Ok((!secret.is_empty()).then_some(secret));
        }
        Ok(None)
    }

    fn set_secret(&self, profile_id: &str, secret: &str) -> Result<(), AiConnectionError> {
        let trimmed = secret.trim();
        if trimmed.is_empty() {
            return self.delete_secret(profile_id);
        }
        let sealed = self.seal_secret(profile_id, trimmed);
        std::fs::create_dir_all(&self.storage_dir).map_err(|e| AiConnectionError::SecretStore {
            message: format!("Failed to create credential directory: {e}"),
        })?;
        let path = self.credential_blob_path(profile_id);
        atomic_replace_file(&path, &sealed).map_err(|e| AiConnectionError::SecretStore {
            message: format!("Failed to write DPAPI credential blob: {e}"),
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        let mut guard = self
            .cache
            .lock()
            .map_err(|_| AiConnectionError::SecretStore {
                message: "DPAPI secret store lock poisoned".into(),
            })?;
        guard.insert(profile_id.to_owned(), sealed);
        Ok(())
    }

    fn delete_secret(&self, profile_id: &str) -> Result<(), AiConnectionError> {
        let path = self.credential_blob_path(profile_id);
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        let mut guard = self
            .cache
            .lock()
            .map_err(|_| AiConnectionError::SecretStore {
                message: "DPAPI secret store lock poisoned".into(),
            })?;
        guard.remove(profile_id);
        Ok(())
    }
}

/// Convert canonical NF2 macOS shortcut strings (`⌘S`, `⇧⌘Z`, `⌥⌘↵`, `⌥F2`, `⌃⌘F`) to
/// Windows/Linux shortcut conventions (`Ctrl+S`, `Ctrl+Shift+Z`, `Ctrl+Alt+Enter`, `Alt+F2`,
/// `Ctrl+Win+F`) (`T-281`).
pub fn format_shortcut_for_windows(macos_shortcut: &str) -> String {
    let trimmed = macos_shortcut.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut has_ctrl = false;
    let mut has_alt = false;
    let mut has_shift = false;
    let mut has_cmd = false;
    let mut key_part = String::new();

    for ch in trimmed.chars() {
        match ch {
            '⌃' => has_ctrl = true,
            '⌥' => has_alt = true,
            '⇧' => has_shift = true,
            '⌘' => has_cmd = true,
            other => key_part.push(other),
        }
    }

    let mut parts: Vec<&str> = Vec::new();
    if has_ctrl && has_cmd {
        parts.push("Ctrl");
        if has_alt {
            parts.push("Alt");
        }
        if has_shift {
            parts.push("Shift");
        }
        parts.push("Win");
    } else {
        if has_cmd || has_ctrl {
            parts.push("Ctrl");
        }
        if has_alt {
            parts.push("Alt");
        }
        if has_shift {
            parts.push("Shift");
        }
    }

    let translated_key = match key_part.trim() {
        "↵" | "⌤" => "Enter",
        "⌫" => "Backspace",
        "⌦" => "Delete",
        "⎋" => "Esc",
        "⇥" => "Tab",
        "↑" => "Up",
        "↓" => "Down",
        "←" => "Left",
        "→" => "Right",
        other => other,
    };

    if !translated_key.is_empty() {
        parts.push(translated_key);
    }
    parts.join("+")
}

/// Format a canonical NF2 shortcut string for the active target OS (`T-281`):
/// returns Windows `Ctrl+...` labels on `target_os = "windows"` and canonical `⌘...` symbols on macOS.
pub fn format_shortcut_for_platform(macos_shortcut: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        format_shortcut_for_windows(macos_shortcut)
    }
    #[cfg(not(target_os = "windows"))]
    {
        macos_shortcut.to_owned()
    }
}

/// Detect whether a keyboard event is a Windows `AltGr` (`Ctrl+Alt+<printable>`) international
/// character input (such as `@`, `€`, `{`, `[`, `]`, `\`, `~` on European layouts) so that bare
/// `Ctrl+<key>` editor shortcuts never misfire while typing text (`T-281`).
pub fn is_altgr_character_event(modifiers: egui::Modifiers, text: Option<&str>) -> bool {
    if !(modifiers.ctrl && modifiers.alt && !modifiers.mac_cmd) {
        return false;
    }
    let Some(raw) = text else {
        return false;
    };
    !raw.is_empty() && raw.chars().all(|c| !c.is_control())
}

/// Discover Windows DirectWrite / system and per-user font directories (`C:\Windows\Fonts` and
/// `%LOCALAPPDATA%\Microsoft\Windows\Fonts`) (`T-281`).
pub fn discover_windows_system_fonts_dirs() -> Vec<PathBuf> {
    let windir = std::env::var_os("WINDIR")
        .or_else(|| std::env::var_os("SystemRoot"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let system_fonts = windir.join("Fonts");

    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Users\Default\AppData\Local"));
    let user_fonts = local_app_data
        .join("Microsoft")
        .join("Windows")
        .join("Fonts");

    vec![system_fonts, user_fonts]
}

/// Clamp a window rectangle across multi-monitor setups, preserving legitimate negative-coordinate
/// secondary displays (e.g. `[-1920..0, 0..1080]`) while recovering off-screen windows from
/// disconnected monitors (`T-281`).
pub fn clamp_window_rect_to_monitors(window: ScreenRect, monitors: &[ScreenRect]) -> ScreenRect {
    let fallback = ScreenRect {
        origin: [0.0, 0.0],
        size: [1920.0, 1080.0],
    };
    let valid: Vec<ScreenRect> = monitors.iter().copied().filter(|m| m.is_valid()).collect();
    let screens = if valid.is_empty() {
        std::slice::from_ref(&fallback)
    } else {
        valid.as_slice()
    };

    let sanitized_size = [
        if window.size[0].is_finite() {
            window.size[0].max(120.0)
        } else {
            400.0
        },
        if window.size[1].is_finite() {
            window.size[1].max(80.0)
        } else {
            300.0
        },
    ];
    let sanitized_origin = [
        if window.origin[0].is_finite() {
            window.origin[0]
        } else {
            screens[0].origin[0]
        },
        if window.origin[1].is_finite() {
            window.origin[1]
        } else {
            screens[0].origin[1]
        },
    ];

    // Check if the window titlebar is already reachable on any connected monitor (including
    // negative-coordinate secondary monitors).
    let reachable_monitor = screens.iter().copied().find(|screen| {
        let min_x_overlap = 80.0_f32.min(sanitized_size[0]);
        let overlap_x = (sanitized_origin[0] + sanitized_size[0])
            .min(screen.origin[0] + screen.size[0])
            - sanitized_origin[0].max(screen.origin[0]);
        let title_h = 28.0_f32.min(sanitized_size[1]);
        let overlap_y = (sanitized_origin[1] + title_h).min(screen.origin[1] + screen.size[1])
            - sanitized_origin[1].max(screen.origin[1]);
        overlap_x >= min_x_overlap && overlap_y >= 22.0_f32.min(title_h)
    });

    if let Some(screen) = reachable_monitor {
        let clamped_w = sanitized_size[0].min(screen.size[0]);
        let clamped_h = sanitized_size[1].min(screen.size[1]);
        let max_x = screen.origin[0] + (screen.size[0] - clamped_w).max(0.0);
        let max_y = screen.origin[1] + (screen.size[1] - clamped_h).max(0.0);
        return ScreenRect {
            origin: [
                sanitized_origin[0].clamp(screen.origin[0], max_x),
                sanitized_origin[1].clamp(screen.origin[1], max_y),
            ],
            size: [clamped_w, clamped_h],
        };
    }

    // Window is off-screen (e.g. a monitor was unplugged): pick the nearest valid monitor.
    let center = [
        sanitized_origin[0] + sanitized_size[0] * 0.5,
        sanitized_origin[1] + sanitized_size[1] * 0.5,
    ];
    let target = screens
        .iter()
        .copied()
        .min_by(|a, b| {
            screen_rect_distance_squared(*a, center)
                .total_cmp(&screen_rect_distance_squared(*b, center))
        })
        .unwrap_or(fallback);

    let clamped_w = sanitized_size[0].min(target.size[0]);
    let clamped_h = sanitized_size[1].min(target.size[1]);
    let max_x = target.origin[0] + (target.size[0] - clamped_w).max(0.0);
    let max_y = target.origin[1] + (target.size[1] - clamped_h).max(0.0);

    ScreenRect {
        origin: [
            sanitized_origin[0].clamp(target.origin[0], max_x),
            sanitized_origin[1].clamp(target.origin[1], max_y),
        ],
        size: [clamped_w, clamped_h],
    }
}

fn screen_rect_distance_squared(rect: ScreenRect, point: [f32; 2]) -> f64 {
    [0usize, 1]
        .into_iter()
        .map(|axis| {
            let min = f64::from(rect.origin[axis]);
            let max = min + f64::from(rect.size[axis]);
            let p = f64::from(point[axis]);
            let d = if p < min {
                min - p
            } else if p > max {
                p - max
            } else {
                0.0
            };
            d * d
        })
        .sum()
}

/// Snap a logical coordinate to the physical device pixel grid for Windows fractional High-DPI
/// scale factors (`1.25` = 125%, `1.5` = 150%, `1.75` = 175%, `2.0` = 200%) (`T-281`).
pub fn snap_to_dpi_pixel_grid(logical_coord: f32, scale_factor: f32) -> f32 {
    if !logical_coord.is_finite() || !scale_factor.is_finite() || scale_factor <= 0.0 {
        return logical_coord;
    }
    let scale = scale_factor.clamp(0.5, 8.0);
    (logical_coord * scale).round() / scale
}

/// Snap an `egui::Pos2` logical position to the physical device pixel grid (`T-281`).
pub fn snap_pos2_to_dpi_pixel_grid(pos: egui::Pos2, scale_factor: f32) -> egui::Pos2 {
    egui::pos2(
        snap_to_dpi_pixel_grid(pos.x, scale_factor),
        snap_to_dpi_pixel_grid(pos.y, scale_factor),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Editor;
    use neoflash_core::{ElementKind, Project};

    #[test]
    fn t279_t280_t281_t282_windows_portability_atomic_io_unicode_unc_shortcuts_altgr_multimonitor_and_packaging()
     {
        // 1. T-279: Windows wgpu backend configuration & CI workflow checks.
        let backends = preferred_windows_wgpu_backends();
        assert!(backends.contains(wgpu::Backends::DX12));
        assert!(backends.contains(wgpu::Backends::VULKAN));

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let check_yml =
            std::fs::read_to_string(repo_root.join(".github/workflows/check.yml")).unwrap();
        assert!(check_yml.contains("windows-x64:"));
        assert!(check_yml.contains("runs-on: windows-2022"));
        assert!(check_yml.contains("linux-x64:"));
        assert!(check_yml.contains("runs-on: ubuntu-24.04"));
        assert!(check_yml.contains("x86_64-pc-windows-msvc"));
        assert!(
            check_yml.contains("cargo clippy --locked --workspace --all-targets -- -D warnings")
        );
        assert!(check_yml.contains("cargo test --locked --workspace"));
        assert!(check_yml.contains("scripts.tests.test_package_portable"));

        // 2. T-280: Extended-length (`\\?\`), UNC (`\\?\UNC\`), and Unicode path normalization.
        let short_drive = r"C:/Users/Художник/Projects/Проект_НеоФлеш_テスト_🎬.nflash";
        assert_eq!(
            to_windows_extended_length_path(short_drive),
            r"C:\Users\Художник\Projects\Проект_НеоФлеш_テスト_🎬.nflash"
        );
        let deep_segment = "אבג_АБВ_テスト_🎬_segment_".repeat(12);
        let long_drive = format!(r"C:/Users/Artist/{deep_segment}/Проект_НеоФлеш_テスト_🎬.nflash");
        assert!(long_drive.chars().count() >= WINDOWS_EXTENDED_PATH_THRESHOLD);
        let long_drive_ext = to_windows_extended_length_path(&long_drive);
        assert!(
            long_drive_ext.starts_with(r"\\?\C:\Users\Artist\"),
            "got: {long_drive_ext}"
        );
        assert!(long_drive_ext.ends_with(r"\Проект_НеоФлеш_テスト_🎬.nflash"));

        let long_unc =
            format!(r"\\studio-nas\SharedProjects\{deep_segment}\Проект_НеоФлеш_テスト_🎬.nflash");
        let long_unc_ext = to_windows_extended_length_path(&long_unc);
        assert!(
            long_unc_ext.starts_with(r"\\?\UNC\studio-nas\SharedProjects\"),
            "got: {long_unc_ext}"
        );
        assert_eq!(
            to_windows_unc_extended_length_path(r"//server/share/folder/scene.nflash"),
            r"\\?\UNC\server\share\folder\scene.nflash"
        );
        // Relative paths must never be corrupted with `\\?\`.
        assert_eq!(
            to_windows_extended_length_path("examples/timeline-tweens.nflash"),
            "examples/timeline-tweens.nflash"
        );

        // 3. T-280: Atomic create -> save -> overwrite -> reopen -> recovery -> HTML5 publish
        // on a Unicode + deep (>= 260 chars) directory tree.
        let tmp_root = std::env::temp_dir().join(format!(
            "neoflash-win-t280-{}-{}",
            std::process::id(),
            ATOMIC_TMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&tmp_root);
        let deep_sub = format!(
            "Цех_Анимации_スタジオ_{}/Сцена_01_{}/Кадр_Финал_{}",
            "a".repeat(65),
            "b".repeat(65),
            "c".repeat(65)
        );
        let doc_dir = tmp_root.join(&deep_sub);
        std::fs::create_dir_all(&doc_dir).unwrap();
        let unicode_doc_path = doc_dir.join("Проект_НеоФлеш_テスト_🎬.nflash");
        assert!(
            unicode_doc_path.to_string_lossy().len() >= 260,
            "path length was {}",
            unicode_doc_path.to_string_lossy().len()
        );

        let rec_dir = tmp_root.join("Recovery_Хранилище");
        let publish_dir = tmp_root.join("Publish_HTML5_出力");

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.recovery.set_dir(Some(rec_dir.clone()));
        editor.workspace.general.autosave = true;
        editor.workspace.general.autosave_minutes = 1;
        editor.transact(|ed| {
            ed.project.name = "Проект НеоФлеш テスト 🎬 v1".into();
        });
        editor.add_element(
            ElementKind::Rectangle {
                width: 240.0,
                height: 120.0,
                fill: [0x24, 0x8A, 0xFF, 0xFF],
                stroke: [0x10, 0x20, 0x30, 0xFF],
                stroke_width: 2.0,
            },
            64.0,
            48.0,
        );

        // Initial atomic save
        assert!(editor.save_to_path(unicode_doc_path.clone()));
        assert!(unicode_doc_path.exists());
        assert!(!editor.dirty());

        // Dirty edit + autosave recovery snapshot + atomic overwrite save
        editor.transact(|ed| {
            ed.project.name = "Проект НеоФлеш テスト 🎬 v2 (Overwritten)".into();
        });
        assert!(editor.dirty());
        let _ = editor.tick_autosave(0.0, 1_700_500_000);
        let _ = editor.tick_autosave(65.0, 1_700_500_065);
        let active_rec_id = editor.recovery.active_id().to_owned();
        assert!(rec_dir.join(format!("{active_rec_id}.nflash")).exists());

        // Overwrite existing file atomically and verify recovery cleanup
        assert!(editor.save(false));
        assert!(!editor.dirty());
        assert!(!rec_dir.join(format!("{active_rec_id}.nflash")).exists());

        // Reopen from Unicode long path in a fresh Editor instance and verify round-trip
        let mut reopened = Editor::from_project(&ctx, Project::blank());
        reopened.open_path(unicode_doc_path.clone(), &ctx);
        assert_eq!(
            reopened.project.name,
            "Проект НеоФлеш テスト 🎬 v2 (Overwritten)"
        );
        assert_eq!(
            reopened.symbol().timeline.layers[0].keyframes[0]
                .elements
                .len(),
            1
        );

        // Simulate crash recovery on the reopened document
        reopened.recovery.set_dir(Some(rec_dir.clone()));
        reopened.workspace.general.autosave = true;
        reopened.workspace.general.autosave_minutes = 1;
        reopened.transact(|ed| {
            ed.project.name = "Проект НеоФлеш テスト 🎬 v3 (Recovered)".into();
        });
        let _ = reopened.tick_autosave(100.0, 1_700_500_100);
        let _ = reopened.tick_autosave(165.0, 1_700_500_165);
        drop(reopened);

        let mut after_crash = Editor::from_project(&ctx, Project::blank());
        after_crash.recovery.set_dir(Some(rec_dir.clone()));
        after_crash.check_recovery_on_launch();
        assert!(after_crash.recovery.has_prompt());
        after_crash.restore_recovery_prompt(&ctx);
        assert_eq!(
            after_crash.project.name,
            "Проект НеоФлеш テスト 🎬 v3 (Recovered)"
        );
        assert!(after_crash.save(false));

        // Publish HTML5 bundle to Unicode path
        let runtime_dir = repo_root.join("target/web-runtime");
        if runtime_dir.join("neoflash_web_bg.wasm").exists() {
            let export_res =
                neoflash_export::export_project(&after_crash.project, &publish_dir, &runtime_dir);
            assert!(export_res.is_ok(), "HTML5 publish failed: {export_res:?}");
            assert!(publish_dir.join("index.html").exists());
            assert!(publish_dir.join("bundle.js").exists());
        }

        // 4. T-280: Browser & Bundled Media (`ffmpeg.exe` / `ffprobe.exe`) discovery + DPAPI store.
        let mock_pf = tmp_root.join("ProgramFiles");
        let mock_local = tmp_root.join("LocalAppData");
        let chrome_exe = mock_pf.join("Google/Chrome/Application/chrome.exe");
        let edge_exe = mock_local.join("Microsoft/Edge/Application/msedge.exe");
        std::fs::create_dir_all(chrome_exe.parent().unwrap()).unwrap();
        std::fs::create_dir_all(edge_exe.parent().unwrap()).unwrap();
        atomic_replace_file(&chrome_exe, b"MZ-mock-chrome").unwrap();
        atomic_replace_file(&edge_exe, b"MZ-mock-msedge").unwrap();
        let browsers =
            discover_windows_chromium_browsers_in(Some(&mock_pf), None, Some(&mock_local));
        assert_eq!(browsers, vec![chrome_exe, edge_exe]);

        let app_dir = tmp_root.join("AppBundle");
        let app_exe = app_dir.join("neoflash-editor.exe");
        let ffmpeg_exe = app_dir.join("media/bin/ffmpeg.exe");
        let ffprobe_exe = app_dir.join("media/bin/ffprobe.exe");
        std::fs::create_dir_all(ffmpeg_exe.parent().unwrap()).unwrap();
        atomic_replace_file(&app_exe, b"MZ-editor").unwrap();
        atomic_replace_file(&ffmpeg_exe, b"MZ-ffmpeg").unwrap();
        atomic_replace_file(&ffprobe_exe, b"MZ-ffprobe").unwrap();
        let (found_ffmpeg, found_ffprobe) = discover_windows_bundled_media(&app_exe);
        assert_eq!(found_ffmpeg.as_deref(), Some(ffmpeg_exe.as_path()));
        assert_eq!(found_ffprobe.as_deref(), Some(ffprobe_exe.as_path()));

        let dpapi_store = WindowsDpapiSecretStore::new(tmp_root.join("DpapiVault"));
        dpapi_store
            .set_secret("openai-prod", "sk-live-super-secret-windows-token-123456")
            .unwrap();
        let raw_blob = std::fs::read(dpapi_store.credential_blob_path("openai-prod")).unwrap();
        assert!(
            !String::from_utf8_lossy(&raw_blob).contains("sk-live-super-secret"),
            "DPAPI credential blob must never store plaintext API keys on disk"
        );
        assert_eq!(
            dpapi_store.get_secret("openai-prod").unwrap().as_deref(),
            Some("sk-live-super-secret-windows-token-123456")
        );
        dpapi_store.delete_secret("openai-prod").unwrap();
        assert_eq!(dpapi_store.get_secret("openai-prod").unwrap(), None);

        // 5. T-281: Shortcut translation, AltGr guard, DirectWrite font dirs, negative-coordinate
        // multi-monitor clamping, and 125%–200% High-DPI pixel snapping.
        assert_eq!(format_shortcut_for_windows("⌘S"), "Ctrl+S");
        assert_eq!(format_shortcut_for_windows("⇧⌘Z"), "Ctrl+Shift+Z");
        assert_eq!(format_shortcut_for_windows("⌥⌘↵"), "Ctrl+Alt+Enter");
        assert_eq!(format_shortcut_for_windows("⌥F2"), "Alt+F2");
        assert_eq!(format_shortcut_for_windows("⌃⌘F"), "Ctrl+Win+F");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(format_shortcut_for_platform("⌘S"), "⌘S");
        #[cfg(target_os = "windows")]
        assert_eq!(format_shortcut_for_platform("⌘S"), "Ctrl+S");

        let altgr_mods = egui::Modifiers {
            alt: true,
            ctrl: true,
            shift: false,
            mac_cmd: false,
            command: true,
        };
        for glyph in ["@", "€", "{", "[", "]", "\\", "~"] {
            assert!(
                is_altgr_character_event(altgr_mods, Some(glyph)),
                "AltGr glyph {glyph} must be recognized as text input"
            );
        }
        let bare_ctrl = egui::Modifiers {
            alt: false,
            ctrl: true,
            shift: false,
            mac_cmd: false,
            command: true,
        };
        assert!(!is_altgr_character_event(bare_ctrl, Some("s")));
        assert!(!is_altgr_character_event(altgr_mods, None));

        let font_dirs = discover_windows_system_fonts_dirs();
        assert_eq!(font_dirs.len(), 2);
        assert!(font_dirs[0].to_string_lossy().ends_with("Fonts"));
        assert!(font_dirs[1].to_string_lossy().ends_with("Fonts"));

        let monitors = [
            ScreenRect {
                origin: [0.0, 0.0],
                size: [1920.0, 1080.0],
            },
            ScreenRect {
                origin: [-1920.0, 0.0],
                size: [1920.0, 1080.0],
            },
        ];
        // Window placed on the left negative-coordinate secondary monitor stays on that monitor!
        let negative_panel = ScreenRect {
            origin: [-1600.0, 120.0],
            size: [360.0, 480.0],
        };
        assert_eq!(
            clamp_window_rect_to_monitors(negative_panel, &monitors),
            negative_panel
        );
        // Window stranded on an unplugged third monitor at x = -4200 is clamped into [-1920..0].
        let stranded_panel = ScreenRect {
            origin: [-4200.0, 120.0],
            size: [360.0, 480.0],
        };
        let recovered_panel = clamp_window_rect_to_monitors(stranded_panel, &monitors);
        assert_eq!(recovered_panel.origin, [-1920.0, 120.0]);
        assert_eq!(recovered_panel.size, [360.0, 480.0]);

        for scale in [1.25_f32, 1.5_f32, 1.75_f32, 2.0_f32] {
            let snapped = snap_to_dpi_pixel_grid(13.37, scale);
            let physical = snapped * scale;
            assert!(
                (physical - physical.round()).abs() < 1e-4,
                "scale={scale}, snapped={snapped}, physical={physical}"
            );
            let pt = snap_pos2_to_dpi_pixel_grid(egui::pos2(101.3, 205.7), scale);
            assert!(((pt.x * scale) - (pt.x * scale).round()).abs() < 1e-4);
            assert!(((pt.y * scale) - (pt.y * scale).round()).abs() < 1e-4);
        }

        // 6. T-282: Verify Windows packaging script and MSIX AppxManifest.
        let pkg_ps1 =
            std::fs::read_to_string(repo_root.join("scripts/package-windows.ps1")).unwrap();
        assert!(pkg_ps1.contains("NeoFlash-$Version-windows-x64-preview"));
        assert!(pkg_ps1.contains("neoflash-editor.exe"));
        assert!(pkg_ps1.contains("scripts/package-portable.py"));
        assert!(pkg_ps1.contains("scripts/stage-rust-dependency-licenses.py"));
        assert!(pkg_ps1.contains("media-corresponding-source.tar.xz"));
        assert!(pkg_ps1.contains("NeoFlash-$Version-windows-x64-SHA256SUMS.txt"));
        assert!(pkg_ps1.contains("signtool.exe sign /fd SHA256"));
        assert!(pkg_ps1.contains("makeappx.exe pack"));

        let portable =
            std::fs::read_to_string(repo_root.join("scripts/package-portable.py")).unwrap();
        for required in [
            "neoflash_web.js",
            "neoflash_web_bg.wasm",
            "ffmpeg_name",
            "ffprobe_name",
            "THIRD_PARTY_NOTICES.txt",
            "PACKAGE-SHA256SUMS.txt",
            "component-license-manifest.json",
            "validate_source_archive",
        ] {
            assert!(
                portable.contains(required),
                "portable gate missing {required}"
            );
        }

        let preview_workflow =
            std::fs::read_to_string(repo_root.join(".github/workflows/technical-preview.yml"))
                .unwrap();
        assert!(preview_workflow.contains("workflow_dispatch:"));
        assert!(preview_workflow.contains("runs-on: windows-2022"));
        assert!(preview_workflow.contains("runs-on: ubuntu-24.04"));
        assert!(preview_workflow.contains("Run bundled codec round trips with an empty PATH"));
        assert!(preview_workflow.contains("Capture DX12 WARP startup"));
        assert!(preview_workflow.contains("Capture X11 Vulkan/lavapipe startup"));
        assert!(preview_workflow.contains("Capture Wayland GLES headless startup"));

        let appx_xml =
            std::fs::read_to_string(repo_root.join("scripts/windows-appxmanifest.xml")).unwrap();
        assert!(appx_xml.contains("ProcessorArchitecture=\"x64\""));
        assert!(appx_xml.contains("Executable=\"neoflash-editor.exe\""));
        assert!(appx_xml.contains(".nflash"));

        let _ = std::fs::remove_dir_all(&tmp_root);
    }
}
