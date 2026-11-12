//! Shared ActionScript 3 AIR runtime services: virtual sandboxed filesystem,
//! AIR URI/path resolution, EncryptedLocalStore, application descriptor
//! synthesis, and a bounded relational SQL engine for `flash.data.*`.
//!
//! Browser hosts marshal inputs/outputs and dispatch AS3 events without
//! duplicating path normalization or SQL execution logic.

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const MAX_AIR_PATH_BYTES: usize = 4096;
pub const MAX_AIR_FS_ENTRIES: usize = 4096;
pub const MAX_AIR_FILE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_AIR_TOTAL_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_AIR_ELS_ITEMS: usize = 1024;
pub const MAX_AIR_SQL_ROWS: usize = 32_768;
pub const MAX_AIR_SQL_TABLES: usize = 256;
pub const MAX_AIR_SQL_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AirError {
    #[error("SecurityError: {0}")]
    Security(String),
    #[error("IOError: {0}")]
    Io(String),
    #[error("ArgumentError: {0}")]
    Argument(String),
    #[error("RangeError: {0}")]
    Range(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("SQLError({operation}, #{error_id}): {details}")]
pub struct AirSqlError {
    pub operation: String,
    pub details: String,
    pub error_id: i32,
    pub detail_id: i32,
    pub detail_args: Vec<String>,
}

impl AirSqlError {
    pub fn new(operation: &str, details: impl Into<String>, error_id: i32) -> Self {
        Self {
            operation: operation.to_owned(),
            details: details.into(),
            error_id,
            detail_id: 0,
            detail_args: Vec::new(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::json!({
            "operation": self.operation,
            "details": self.details,
            "errorId": self.error_id,
            "detailId": self.detail_id,
            "detailArguments": self.detail_args,
        })
        .to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirResourceInit {
    pub path: String,
    #[serde(default)]
    pub data_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum VfsNode {
    Dir {
        created_ms: f64,
        modified_ms: f64,
        read_only: bool,
    },
    File {
        bytes: Vec<u8>,
        created_ms: f64,
        modified_ms: f64,
        read_only: bool,
    },
}

impl VfsNode {
    fn is_dir(&self) -> bool {
        matches!(self, Self::Dir { .. })
    }

    fn read_only(&self) -> bool {
        match self {
            Self::Dir { read_only, .. } | Self::File { read_only, .. } => *read_only,
        }
    }

    fn created_ms(&self) -> f64 {
        match self {
            Self::Dir { created_ms, .. } | Self::File { created_ms, .. } => *created_ms,
        }
    }

    fn modified_ms(&self) -> f64 {
        match self {
            Self::Dir { modified_ms, .. } | Self::File { modified_ms, .. } => *modified_ms,
        }
    }

    fn byte_len(&self) -> usize {
        match self {
            Self::Dir { .. } => 0,
            Self::File { bytes, .. } => bytes.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AirPathInfo {
    pub native_path: String,
    pub url: String,
    pub name: String,
    pub extension: Option<String>,
    pub file_type: String,
    pub is_hidden: bool,
    pub parent: Option<String>,
    pub exists: bool,
    pub is_directory: bool,
    pub size: usize,
    pub created_ms: f64,
    pub modified_ms: f64,
    pub space_available: usize,
    pub writable: bool,
}

#[derive(Debug, Clone)]
pub struct AirFileSystem {
    nodes: BTreeMap<String, VfsNode>,
    total_bytes: usize,
    temp_counter: u64,
}

fn percent_decode(raw: &str) -> Result<String, AirError> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(AirError::Argument(
                    "Malformed percent-encoding in AIR URL.".into(),
                ));
            }
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            match (hi, lo) {
                (Some(h), Some(l)) => {
                    out.push(((h << 4) | l) as u8);
                    i += 3;
                }
                _ => {
                    return Err(AirError::Argument("Invalid hex escape in AIR URL.".into()));
                }
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| AirError::Argument("AIR URL is not valid UTF-8.".into()))
}

fn percent_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

pub fn normalize_air_path(input: &str) -> Result<String, AirError> {
    if input.is_empty() {
        return Err(AirError::Argument(
            "File path or URL must not be empty.".into(),
        ));
    }
    if input.len() > MAX_AIR_PATH_BYTES {
        return Err(AirError::Range("File path exceeds 4096 bytes.".into()));
    }
    if input.contains('\0') {
        return Err(AirError::Argument(
            "File path must not contain NUL bytes.".into(),
        ));
    }
    let trimmed = input.trim();
    let replaced;
    let raw_posix = if let Some(idx) = trimmed.find(':') {
        let scheme = trimmed[..idx].to_ascii_lowercase();
        let rest = &trimmed[idx + 1..];
        if idx == 1 && scheme.chars().all(|c| c.is_ascii_alphabetic()) {
            // Windows drive path such as C:\Users\demo -> /Users/demo
            replaced = format!("/{}", rest.replace('\\', "/").trim_start_matches('/'));
            replaced.as_str()
        } else {
            let decoded = percent_decode(rest.trim_start_matches('/'))?;
            let sub = decoded.replace('\\', "/");
            let base = match scheme.as_str() {
                "app" => "/app",
                "app-storage" => "/app-storage",
                "app-temp" => "/tmp",
                "cache" => "/cache",
                "user" => "/user",
                "documents" => "/user/Documents",
                "desktop" => "/user/Desktop",
                "file" => "",
                _ => {
                    return Err(AirError::Argument(format!(
                        "Unsupported AIR URL scheme '{scheme}:'."
                    )));
                }
            };
            if sub.is_empty() {
                replaced = if base.is_empty() {
                    "/".to_owned()
                } else {
                    base.to_owned()
                };
            } else if base.is_empty() {
                replaced = format!("/{}", sub.trim_start_matches('/'));
            } else {
                replaced = format!("{base}/{}", sub.trim_start_matches('/'));
            }
            replaced.as_str()
        }
    } else {
        replaced = trimmed.replace('\\', "/");
        if !replaced.starts_with('/') {
            return Err(AirError::Argument(
                "Unqualified File constructor path requires an absolute path or AIR URL scheme."
                    .into(),
            ));
        }
        replaced.as_str()
    };

    let mut stack: Vec<&str> = Vec::new();
    for seg in raw_posix.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            other => stack.push(other),
        }
    }
    if stack.is_empty() {
        Ok("/".to_owned())
    } else {
        let out = format!("/{}", stack.join("/"));
        if out.len() > MAX_AIR_PATH_BYTES {
            return Err(AirError::Range(
                "Normalized File path exceeds 4096 bytes.".into(),
            ));
        }
        Ok(out)
    }
}

pub fn resolve_air_path(base_native_path: &str, relative: &str) -> Result<String, AirError> {
    let base = normalize_air_path(base_native_path)?;
    if relative.is_empty() || relative == "." {
        return Ok(base);
    }
    if relative.len() > MAX_AIR_PATH_BYTES || relative.contains('\0') {
        return Err(AirError::Argument("Invalid relative File path.".into()));
    }
    let trimmed = relative.trim();
    if trimmed.starts_with('/') || trimmed.contains(":/") || trimmed.contains(":\\") {
        return normalize_air_path(trimmed);
    }
    let joined = if base == "/" {
        format!("/{}", trimmed.replace('\\', "/"))
    } else {
        format!("{base}/{}", trimmed.replace('\\', "/"))
    };
    normalize_air_path(&joined)
}

pub fn relative_air_path(
    from_native_path: &str,
    to_native_path: &str,
    use_dot_dot: bool,
) -> Result<Option<String>, AirError> {
    let from = normalize_air_path(from_native_path)?;
    let to = normalize_air_path(to_native_path)?;
    if from == to {
        return Ok(Some(String::new()));
    }
    let from_parts: Vec<&str> = if from == "/" {
        Vec::new()
    } else {
        from[1..].split('/').collect()
    };
    let to_parts: Vec<&str> = if to == "/" {
        Vec::new()
    } else {
        to[1..].split('/').collect()
    };
    let mut common = 0;
    while common < from_parts.len()
        && common < to_parts.len()
        && from_parts[common] == to_parts[common]
    {
        common += 1;
    }
    if common < from_parts.len() && !use_dot_dot {
        return Ok(None);
    }
    let mut rel: Vec<&str> = vec![".."; from_parts.len() - common];
    rel.extend_from_slice(&to_parts[common..]);
    Ok(Some(rel.join("/")))
}

pub fn air_url_for_native_path(native_path: &str) -> String {
    if native_path == "/app" {
        "app:/".to_owned()
    } else if let Some(rest) = native_path.strip_prefix("/app/") {
        format!("app:/{}", percent_encode_path(rest))
    } else if native_path == "/app-storage" {
        "app-storage:/".to_owned()
    } else if let Some(rest) = native_path.strip_prefix("/app-storage/") {
        format!("app-storage:/{}", percent_encode_path(rest))
    } else {
        format!("file://{}", percent_encode_path(native_path))
    }
}

fn parent_of(native_path: &str) -> Option<String> {
    if native_path == "/" {
        return None;
    }
    match native_path.rfind('/') {
        Some(0) => Some("/".to_owned()),
        Some(idx) => Some(native_path[..idx].to_owned()),
        None => None,
    }
}

fn basename_of(native_path: &str) -> &str {
    if native_path == "/" {
        ""
    } else {
        native_path.rsplit('/').next().unwrap_or(native_path)
    }
}

fn extension_of(name: &str) -> Option<String> {
    if let Some(dot) = name.rfind('.')
        && dot > 0
        && dot + 1 < name.len()
    {
        return Some(name[dot + 1..].to_owned());
    }
    None
}

fn is_under_readonly_root(path: &str) -> bool {
    path == "/app" || path.starts_with("/app/")
}

impl AirFileSystem {
    pub fn new(resources: &[AirResourceInit], now_ms: f64) -> Result<Self, AirError> {
        let mut fs = Self {
            nodes: BTreeMap::new(),
            total_bytes: 0,
            temp_counter: 0,
        };
        for (dir, ro) in [
            ("/", false),
            ("/app", true),
            ("/app-storage", false),
            ("/cache", false),
            ("/user", false),
            ("/user/Documents", false),
            ("/user/Desktop", false),
            ("/tmp", false),
        ] {
            fs.nodes.insert(
                dir.to_owned(),
                VfsNode::Dir {
                    created_ms: now_ms,
                    modified_ms: now_ms,
                    read_only: ro,
                },
            );
        }
        for res in resources {
            let clean = res.path.trim_start_matches('/').replace('\\', "/");
            if clean.is_empty() {
                continue;
            }
            let full = normalize_air_path(&format!("/app/{clean}"))?;
            if !full.starts_with("/app/") {
                continue;
            }
            let bytes = if res.data_base64.is_empty() {
                Vec::new()
            } else {
                BASE64
                    .decode(res.data_base64.trim())
                    .map_err(|_| AirError::Argument("Invalid base64 in project resource.".into()))?
            };
            if bytes.len() > MAX_AIR_FILE_BYTES
                || fs.total_bytes + bytes.len() > MAX_AIR_TOTAL_BYTES
            {
                return Err(AirError::Range(
                    "Embedded AIR resources exceed virtual filesystem budget.".into(),
                ));
            }
            // Create read-only intermediate directories under /app
            let mut ancestors = Vec::new();
            let mut cur = parent_of(&full);
            while let Some(p) = cur {
                if fs.nodes.contains_key(&p) {
                    break;
                }
                ancestors.push(p.clone());
                cur = parent_of(&p);
            }
            for dir in ancestors.into_iter().rev() {
                fs.nodes.insert(
                    dir,
                    VfsNode::Dir {
                        created_ms: now_ms,
                        modified_ms: now_ms,
                        read_only: true,
                    },
                );
            }
            fs.total_bytes += bytes.len();
            fs.nodes.insert(
                full,
                VfsNode::File {
                    bytes,
                    created_ms: now_ms,
                    modified_ms: now_ms,
                    read_only: true,
                },
            );
        }
        Ok(fs)
    }

    pub fn canonicalize(&self, path: &str) -> Result<String, AirError> {
        let norm = normalize_air_path(path)?;
        if norm == "/" || self.nodes.contains_key(&norm) {
            return Ok(norm);
        }
        // Match existing segments case-insensitively to mimic AIR canonicalize()
        let mut current = String::new();
        for seg in norm[1..].split('/') {
            let prefix = if current.is_empty() {
                "/".to_owned()
            } else {
                format!("{current}/")
            };
            let candidate = format!("{prefix}{seg}");
            if self.nodes.contains_key(&candidate) {
                current = candidate;
                continue;
            }
            let found = self
                .nodes
                .keys()
                .find(|k| {
                    if let Some(rest) = k.strip_prefix(&prefix) {
                        !rest.contains('/') && rest.eq_ignore_ascii_case(seg)
                    } else {
                        false
                    }
                })
                .cloned();
            current = found.unwrap_or(candidate);
        }
        Ok(if current.is_empty() {
            "/".to_owned()
        } else {
            current
        })
    }

    pub fn info(&self, path: &str) -> Result<AirPathInfo, AirError> {
        let native_path = normalize_air_path(path)?;
        let url = air_url_for_native_path(&native_path);
        let name = basename_of(&native_path).to_owned();
        let extension = extension_of(&name);
        let file_type = extension
            .as_ref()
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        let is_hidden = name.starts_with('.') && name.len() > 1;
        let parent = parent_of(&native_path);
        let writable = !is_under_readonly_root(&native_path);
        let space_available = if writable {
            MAX_AIR_TOTAL_BYTES.saturating_sub(self.total_bytes)
        } else {
            0
        };
        if let Some(node) = self.nodes.get(&native_path) {
            Ok(AirPathInfo {
                native_path,
                url,
                name,
                extension,
                file_type,
                is_hidden,
                parent,
                exists: true,
                is_directory: node.is_dir(),
                size: node.byte_len(),
                created_ms: node.created_ms(),
                modified_ms: node.modified_ms(),
                space_available,
                writable: writable && !node.read_only(),
            })
        } else {
            Ok(AirPathInfo {
                native_path,
                url,
                name,
                extension,
                file_type,
                is_hidden,
                parent,
                exists: false,
                is_directory: false,
                size: 0,
                created_ms: 0.0,
                modified_ms: 0.0,
                space_available,
                writable,
            })
        }
    }

    fn ensure_writable(&self, native_path: &str) -> Result<(), AirError> {
        if native_path == "/" || is_under_readonly_root(native_path) {
            return Err(AirError::Security(format!(
                "Path '{native_path}' is inside a read-only AIR sandbox root."
            )));
        }
        if let Some(node) = self.nodes.get(native_path)
            && node.read_only()
        {
            return Err(AirError::Security(format!(
                "Path '{native_path}' is read-only."
            )));
        }
        Ok(())
    }

    pub fn create_directory(&mut self, path: &str, now_ms: f64) -> Result<(), AirError> {
        let target = normalize_air_path(path)?;
        self.ensure_writable(&target)?;
        if let Some(existing) = self.nodes.get(&target) {
            return if existing.is_dir() {
                Ok(())
            } else {
                Err(AirError::Io(format!(
                    "Cannot create directory '{target}': a file already exists at that path."
                )))
            };
        }
        let mut to_create = Vec::new();
        let mut cur = Some(target);
        while let Some(p) = cur {
            if let Some(node) = self.nodes.get(&p) {
                if !node.is_dir() {
                    return Err(AirError::Io(format!(
                        "Ancestor '{p}' is a file, not a directory."
                    )));
                }
                break;
            }
            self.ensure_writable(&p)?;
            to_create.push(p.clone());
            cur = parent_of(&p);
        }
        if self.nodes.len() + to_create.len() > MAX_AIR_FS_ENTRIES {
            return Err(AirError::Range(
                "Virtual AIR filesystem entry limit (4096) exceeded.".into(),
            ));
        }
        for dir in to_create.into_iter().rev() {
            self.nodes.insert(
                dir,
                VfsNode::Dir {
                    created_ms: now_ms,
                    modified_ms: now_ms,
                    read_only: false,
                },
            );
        }
        Ok(())
    }

    pub fn create_temp(&mut self, is_directory: bool, now_ms: f64) -> Result<String, AirError> {
        if self.nodes.len() >= MAX_AIR_FS_ENTRIES {
            return Err(AirError::Range(
                "Virtual AIR filesystem entry limit (4096) exceeded.".into(),
            ));
        }
        loop {
            self.temp_counter += 1;
            let candidate = if is_directory {
                format!("/tmp/air-temp-dir-{}", self.temp_counter)
            } else {
                format!("/tmp/air-temp-{}.tmp", self.temp_counter)
            };
            if !self.nodes.contains_key(&candidate) {
                let node = if is_directory {
                    VfsNode::Dir {
                        created_ms: now_ms,
                        modified_ms: now_ms,
                        read_only: false,
                    }
                } else {
                    VfsNode::File {
                        bytes: Vec::new(),
                        created_ms: now_ms,
                        modified_ms: now_ms,
                        read_only: false,
                    }
                };
                self.nodes.insert(candidate.clone(), node);
                return Ok(candidate);
            }
        }
    }

    pub fn list_directory(&self, path: &str) -> Result<Vec<String>, AirError> {
        let dir = normalize_air_path(path)?;
        match self.nodes.get(&dir) {
            Some(VfsNode::Dir { .. }) => {}
            Some(VfsNode::File { .. }) => {
                return Err(AirError::Io(format!("'{dir}' is not a directory.")));
            }
            None => {
                return Err(AirError::Io(format!("Directory '{dir}' does not exist.")));
            }
        }
        let prefix = if dir == "/" {
            "/".to_owned()
        } else {
            format!("{dir}/")
        };
        let mut out = Vec::new();
        for key in self.nodes.keys() {
            if key == &dir {
                continue;
            }
            if let Some(rest) = key.strip_prefix(&prefix)
                && !rest.is_empty()
                && !rest.contains('/')
            {
                out.push(key.clone());
            }
        }
        Ok(out)
    }

    pub fn delete_file(&mut self, path: &str) -> Result<(), AirError> {
        let target = normalize_air_path(path)?;
        self.ensure_writable(&target)?;
        match self.nodes.get(&target) {
            Some(VfsNode::File { bytes, .. }) => {
                let len = bytes.len();
                self.nodes.remove(&target);
                self.total_bytes = self.total_bytes.saturating_sub(len);
                Ok(())
            }
            Some(VfsNode::Dir { .. }) => Err(AirError::Io(format!(
                "Cannot deleteFile on directory '{target}'."
            ))),
            None => Err(AirError::Io(format!("File '{target}' does not exist."))),
        }
    }

    pub fn delete_directory(&mut self, path: &str, delete_contents: bool) -> Result<(), AirError> {
        let target = normalize_air_path(path)?;
        if matches!(
            target.as_str(),
            "/" | "/app" | "/app-storage" | "/cache" | "/user" | "/tmp"
        ) {
            return Err(AirError::Security(format!(
                "Cannot delete protected AIR root directory '{target}'."
            )));
        }
        self.ensure_writable(&target)?;
        match self.nodes.get(&target) {
            Some(VfsNode::Dir { .. }) => {}
            Some(VfsNode::File { .. }) => {
                return Err(AirError::Io(format!(
                    "Cannot deleteDirectory on file '{target}'."
                )));
            }
            None => {
                return Err(AirError::Io(format!(
                    "Directory '{target}' does not exist."
                )));
            }
        }
        let prefix = format!("{target}/");
        let descendants: Vec<String> = self
            .nodes
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        if !descendants.is_empty() && !delete_contents {
            return Err(AirError::Io(format!("Directory '{target}' is not empty.")));
        }
        for key in descendants {
            if let Some(removed) = self.nodes.remove(&key) {
                self.total_bytes = self.total_bytes.saturating_sub(removed.byte_len());
            }
        }
        self.nodes.remove(&target);
        Ok(())
    }

    pub fn read_bytes(&self, path: &str) -> Result<Vec<u8>, AirError> {
        let target = normalize_air_path(path)?;
        match self.nodes.get(&target) {
            Some(VfsNode::File { bytes, .. }) => Ok(bytes.clone()),
            Some(VfsNode::Dir { .. }) => Err(AirError::Io(format!(
                "Cannot open directory '{target}' as a file."
            ))),
            None => Err(AirError::Io(format!("File '{target}' does not exist."))),
        }
    }

    pub fn write_bytes(&mut self, path: &str, data: &[u8], now_ms: f64) -> Result<(), AirError> {
        let target = normalize_air_path(path)?;
        self.ensure_writable(&target)?;
        if data.len() > MAX_AIR_FILE_BYTES {
            return Err(AirError::Range("AIR file exceeds 16 MiB limit.".into()));
        }
        if let Some(parent) = parent_of(&target) {
            if !self.nodes.contains_key(&parent) {
                self.create_directory(&parent, now_ms)?;
            } else if !self.nodes[&parent].is_dir() {
                return Err(AirError::Io(format!(
                    "Parent path '{parent}' is not a directory."
                )));
            }
        }
        let (old_len, created) = match self.nodes.get(&target) {
            Some(VfsNode::Dir { .. }) => {
                return Err(AirError::Io(format!(
                    "Cannot overwrite directory '{target}' with a file."
                )));
            }
            Some(VfsNode::File {
                bytes, created_ms, ..
            }) => (bytes.len(), *created_ms),
            None => {
                if self.nodes.len() >= MAX_AIR_FS_ENTRIES {
                    return Err(AirError::Range(
                        "Virtual AIR filesystem entry limit (4096) exceeded.".into(),
                    ));
                }
                (0, now_ms)
            }
        };
        let next_total = self
            .total_bytes
            .saturating_sub(old_len)
            .saturating_add(data.len());
        if next_total > MAX_AIR_TOTAL_BYTES {
            return Err(AirError::Range(
                "Virtual AIR filesystem storage quota (64 MiB) exceeded.".into(),
            ));
        }
        self.total_bytes = next_total;
        self.nodes.insert(
            target,
            VfsNode::File {
                bytes: data.to_vec(),
                created_ms: created,
                modified_ms: now_ms,
                read_only: false,
            },
        );
        Ok(())
    }

    pub fn open_stream(
        &mut self,
        path: &str,
        mode: &str,
        now_ms: f64,
    ) -> Result<Vec<u8>, AirError> {
        let target = normalize_air_path(path)?;
        match mode {
            "read" => self.read_bytes(&target),
            "write" => {
                self.write_bytes(&target, &[], now_ms)?;
                Ok(Vec::new())
            }
            "append" | "update" => {
                self.ensure_writable(&target)?;
                match self.nodes.get(&target) {
                    Some(VfsNode::File { bytes, .. }) => Ok(bytes.clone()),
                    Some(VfsNode::Dir { .. }) => Err(AirError::Io(format!(
                        "Cannot open directory '{target}' as a FileStream."
                    ))),
                    None => {
                        self.write_bytes(&target, &[], now_ms)?;
                        Ok(Vec::new())
                    }
                }
            }
            other => Err(AirError::Argument(format!(
                "Invalid FileMode '{other}'. Expected read, write, append or update."
            ))),
        }
    }

    pub fn copy_to(
        &mut self,
        src_path: &str,
        dst_path: &str,
        overwrite: bool,
        now_ms: f64,
    ) -> Result<(), AirError> {
        let src = normalize_air_path(src_path)?;
        let dst = normalize_air_path(dst_path)?;
        if src == dst {
            return if overwrite {
                Ok(())
            } else {
                Err(AirError::Io(
                    "Source and destination refer to the same File.".into(),
                ))
            };
        }
        if dst.starts_with(&format!("{src}/")) {
            return Err(AirError::Io(
                "Cannot copy a directory into one of its own descendants.".into(),
            ));
        }
        self.ensure_writable(&dst)?;
        let src_node = self
            .nodes
            .get(&src)
            .cloned()
            .ok_or_else(|| AirError::Io(format!("Source '{src}' does not exist.")))?;
        if self.nodes.contains_key(&dst) {
            if !overwrite {
                return Err(AirError::Io(format!(
                    "Destination '{dst}' already exists and overwrite is false."
                )));
            }
            if self.nodes[&dst].is_dir() {
                self.delete_directory(&dst, true)?;
            } else {
                self.delete_file(&dst)?;
            }
        }
        match src_node {
            VfsNode::File { bytes, .. } => self.write_bytes(&dst, &bytes, now_ms),
            VfsNode::Dir { .. } => {
                self.create_directory(&dst, now_ms)?;
                let prefix = format!("{src}/");
                let children: Vec<(String, VfsNode)> = self
                    .nodes
                    .iter()
                    .filter(|(k, _)| k.starts_with(&prefix))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (child_key, child_node) in children {
                    let suffix = &child_key[prefix.len()..];
                    let mapped = format!("{dst}/{suffix}");
                    match child_node {
                        VfsNode::Dir { .. } => self.create_directory(&mapped, now_ms)?,
                        VfsNode::File { bytes, .. } => self.write_bytes(&mapped, &bytes, now_ms)?,
                    }
                }
                Ok(())
            }
        }
    }

    pub fn move_to(
        &mut self,
        src_path: &str,
        dst_path: &str,
        overwrite: bool,
        now_ms: f64,
    ) -> Result<(), AirError> {
        let src = normalize_air_path(src_path)?;
        let dst = normalize_air_path(dst_path)?;
        self.ensure_writable(&src)?;
        if src == dst {
            return Ok(());
        }
        self.copy_to(&src, &dst, overwrite, now_ms)?;
        if self.nodes.get(&src).is_some_and(VfsNode::is_dir) {
            self.delete_directory(&src, true)
        } else {
            self.delete_file(&src)
        }
    }
}

// ---------------------------------------------------------------------------
// Application Descriptor & EncryptedLocalStore
// ---------------------------------------------------------------------------

fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn synthesize_app_id(project_name: &str) -> String {
    let mut slug = String::new();
    let mut prev_dot = false;
    for ch in project_name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dot = false;
        } else if !slug.is_empty() && !prev_dot {
            slug.push('-');
            prev_dot = true;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "org.neoflash.application".to_owned()
    } else {
        format!("org.neoflash.{slug}")
    }
}

pub fn synthesize_app_descriptor_xml(project_name: &str, width: u32, height: u32) -> String {
    let app_id = synthesize_app_id(project_name);
    let safe_name = if project_name.trim().is_empty() {
        "NeoFlash Application"
    } else {
        project_name.trim()
    };
    let escaped = xml_escape(safe_name);
    format!(
        "<application xmlns=\"http://ns.adobe.com/air/application/32.0\"><id>{}</id><filename>{escaped}</filename><name>{escaped}</name><versionNumber>1.0.0</versionNumber><initialWindow><title>{escaped}</title><visible>true</visible><width>{width}</width><height>{height}</height></initialWindow></application>",
        xml_escape(&app_id)
    )
}

#[derive(Debug, Clone, Default)]
pub struct AirEncryptedStore {
    items: BTreeMap<String, Vec<u8>>,
    total_bytes: usize,
}

impl AirEncryptedStore {
    pub fn set_item(&mut self, name: &str, bytes: &[u8]) -> Result<(), AirError> {
        if name.is_empty() {
            return Err(AirError::Argument(
                "EncryptedLocalStore item name must not be empty.".into(),
            ));
        }
        if name.len() > MAX_AIR_PATH_BYTES {
            return Err(AirError::Range(
                "EncryptedLocalStore item name exceeds 4096 bytes.".into(),
            ));
        }
        if bytes.len() > MAX_AIR_FILE_BYTES {
            return Err(AirError::Range(
                "EncryptedLocalStore item exceeds 16 MiB.".into(),
            ));
        }
        let old_len = self.items.get(name).map_or(0, Vec::len);
        if !self.items.contains_key(name) && self.items.len() >= MAX_AIR_ELS_ITEMS {
            return Err(AirError::Range(
                "EncryptedLocalStore item count limit (1024) exceeded.".into(),
            ));
        }
        let next_total = self
            .total_bytes
            .saturating_sub(old_len)
            .saturating_add(bytes.len());
        if next_total > MAX_AIR_TOTAL_BYTES {
            return Err(AirError::Range(
                "EncryptedLocalStore total storage limit (64 MiB) exceeded.".into(),
            ));
        }
        self.total_bytes = next_total;
        self.items.insert(name.to_owned(), bytes.to_vec());
        Ok(())
    }

    pub fn get_item(&self, name: &str) -> Result<Option<Vec<u8>>, AirError> {
        if name.is_empty() {
            return Err(AirError::Argument(
                "EncryptedLocalStore item name must not be empty.".into(),
            ));
        }
        Ok(self.items.get(name).cloned())
    }

    pub fn remove_item(&mut self, name: &str) -> Result<(), AirError> {
        if name.is_empty() {
            return Err(AirError::Argument(
                "EncryptedLocalStore item name must not be empty.".into(),
            ));
        }
        if let Some(old) = self.items.remove(name) {
            self.total_bytes = self.total_bytes.saturating_sub(old.len());
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        self.items.clear();
        self.total_bytes = 0;
    }
}

// ---------------------------------------------------------------------------
// Bounded Relational SQL Engine (`flash.data.*`)
// ---------------------------------------------------------------------------

const SQL_VFS_MAGIC: &[u8] = b"NFAIRSDB1\n";
const SQL_ENC_VFS_MAGIC: &[u8] = b"NFAIRSLE1\n";

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, &word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

pub fn sha1_bytes(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xefcdab89;
    let mut h2: u32 = 0x98badcfe;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xc3d2e1f0;

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (i, &word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(word);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5a827999u32),
                20..=39 => (b ^ c ^ d, 0x6ed9eba1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdcu32),
                _ => (b ^ c ^ d, 0xca62c1d6u32),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut k_block = [0u8; 64];
    if key.len() > 64 {
        let hashed = sha256_bytes(key);
        k_block[..32].copy_from_slice(&hashed);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Vec::with_capacity(64 + data.len());
    for &b in &k_block {
        inner.push(b ^ 0x36);
    }
    inner.extend_from_slice(data);
    let inner_hash = sha256_bytes(&inner);

    let mut outer = Vec::with_capacity(64 + 32);
    for &b in &k_block {
        outer.push(b ^ 0x5c);
    }
    outer.extend_from_slice(&inner_hash);
    sha256_bytes(&outer)
}

static SQL_CRYPTO_GLOBAL_COUNTER: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

#[inline]
fn chacha20_quarter_round(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(16);

    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(12);

    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(8);

    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(7);
}

/// Standard RFC 8439 §2.3 ChaCha20 block function (20 rounds).
pub fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut state = [0u32; 16];
    state[0] = 0x6170_7865; // "expa"
    state[1] = 0x3320_646e; // "nd 3"
    state[2] = 0x7962_2d32; // "2-by"
    state[3] = 0x6b20_6574; // "te k"
    for i in 0..8 {
        state[4 + i] =
            u32::from_le_bytes([key[i * 4], key[i * 4 + 1], key[i * 4 + 2], key[i * 4 + 3]]);
    }
    state[12] = counter;
    for i in 0..3 {
        state[13 + i] = u32::from_le_bytes([
            nonce[i * 4],
            nonce[i * 4 + 1],
            nonce[i * 4 + 2],
            nonce[i * 4 + 3],
        ]);
    }

    let mut working = state;
    for _ in 0..10 {
        // Column rounds
        chacha20_quarter_round(&mut working, 0, 4, 8, 12);
        chacha20_quarter_round(&mut working, 1, 5, 9, 13);
        chacha20_quarter_round(&mut working, 2, 6, 10, 14);
        chacha20_quarter_round(&mut working, 3, 7, 11, 15);
        // Diagonal rounds
        chacha20_quarter_round(&mut working, 0, 5, 10, 15);
        chacha20_quarter_round(&mut working, 1, 6, 11, 12);
        chacha20_quarter_round(&mut working, 2, 7, 8, 13);
        chacha20_quarter_round(&mut working, 3, 4, 9, 14);
    }

    let mut out = [0u8; 64];
    for i in 0..16 {
        let word = working[i].wrapping_add(state[i]);
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// Standard RFC 8439 §2.4 ChaCha20 stream cipher encryption/decryption.
pub fn chacha20_xor(
    key: &[u8; 32],
    nonce: &[u8; 12],
    initial_counter: u32,
    input: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    for (block_idx, chunk) in input.chunks(64).enumerate() {
        let counter = initial_counter.wrapping_add(block_idx as u32);
        let ks = chacha20_block(key, counter, nonce);
        for (i, &b) in chunk.iter().enumerate() {
            out.push(b ^ ks[i]);
        }
    }
    out
}

/// RFC 5869 HKDF-SHA256 key derivation for encryption and authentication subkeys.
fn derive_sql_subkeys(master_key: &[u8; 16], salt: &[u8; 16]) -> ([u8; 32], [u8; 32]) {
    let prk = hmac_sha256(salt, master_key);
    let enc_key = hmac_sha256(&prk, b"neoflash-air-sql-chacha20-v2-enc\x01");
    let mac_key = hmac_sha256(&prk, b"neoflash-air-sql-chacha20-v2-mac\x01");
    (enc_key, mac_key)
}

/// Generate fresh 16-byte salt and 12-byte RFC 8439 nonce material by mixing a
/// seeded xorshift64* + atomic counter CSPRNG mixer, `RandomState` OS seed, a
/// high-resolution clock, and stack address entropy without performing host
/// filesystem I/O.
fn generate_sql_salt_and_nonce(
    key: &[u8; 16],
    instance_seed: u64,
    payload_len: usize,
) -> ([u8; 16], [u8; 12]) {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let global_seq = SQL_CRYPTO_GLOBAL_COUNTER
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        .wrapping_add(1);
    let mut stack_anchor = global_seq ^ instance_seed;
    let stack_addr = &stack_anchor as *const u64 as usize as u64;

    let mut hasher_a = RandomState::new().build_hasher();
    hasher_a.write_u64(global_seq);
    hasher_a.write_u64(instance_seed);
    hasher_a.write_u64(stack_addr);
    let rand_a = hasher_a.finish();

    let mut hasher_b = RandomState::new().build_hasher();
    hasher_b.write_u64(rand_a.rotate_left(17));
    hasher_b.write_usize(payload_len);
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(dur) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        hasher_b.write_u128(dur.as_nanos());
    }
    let rand_b = hasher_b.finish();

    // Seeded xorshift64* CSPRNG mixer (4 x 64-bit words = 32 bytes).
    let mut xorshift_state = rand_a
        ^ rand_b.rotate_left(29)
        ^ global_seq.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ instance_seed.rotate_left(13)
        ^ stack_addr.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    if xorshift_state == 0 {
        xorshift_state = 0x2545_F491_4F6C_DD1D;
    }
    stack_anchor ^= xorshift_state;
    let mut os_entropy = [0u8; 32];
    for chunk in os_entropy.as_chunks_mut::<8>().0 {
        xorshift_state ^= xorshift_state >> 12;
        xorshift_state ^= xorshift_state << 25;
        xorshift_state ^= xorshift_state >> 27;
        let word = xorshift_state.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ stack_anchor;
        *chunk = word.to_le_bytes();
        xorshift_state = xorshift_state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    }

    let mut entropy_buf = Vec::with_capacity(80);
    entropy_buf.extend_from_slice(&os_entropy);
    entropy_buf.extend_from_slice(&global_seq.to_le_bytes());
    entropy_buf.extend_from_slice(&instance_seed.to_le_bytes());
    entropy_buf.extend_from_slice(&rand_a.to_le_bytes());
    entropy_buf.extend_from_slice(&rand_b.to_le_bytes());
    entropy_buf.extend_from_slice(&(payload_len as u64).to_le_bytes());

    let block = hmac_sha256(key, &entropy_buf);
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    salt.copy_from_slice(&block[..16]);
    nonce.copy_from_slice(&block[16..28]);
    (salt, nonce)
}

const SQL_ENC_V2_HEADER: &[u8; 4] = b"CV2\0";

fn encrypt_sql_payload(key: &[u8; 16], nonce_seed: u64, plaintext: &[u8]) -> Vec<u8> {
    let (salt, nonce) = generate_sql_salt_and_nonce(key, nonce_seed, plaintext.len());
    let (enc_key, mac_key) = derive_sql_subkeys(key, &salt);
    let ciphertext = chacha20_xor(&enc_key, &nonce, 1, plaintext);

    let mut mac_msg = Vec::with_capacity(4 + 16 + 12 + ciphertext.len());
    mac_msg.extend_from_slice(SQL_ENC_V2_HEADER);
    mac_msg.extend_from_slice(&salt);
    mac_msg.extend_from_slice(&nonce);
    mac_msg.extend_from_slice(&ciphertext);
    let tag = hmac_sha256(&mac_key, &mac_msg);

    let mut out = Vec::with_capacity(SQL_ENC_VFS_MAGIC.len() + 4 + 16 + 12 + ciphertext.len() + 32);
    out.extend_from_slice(SQL_ENC_VFS_MAGIC);
    out.extend_from_slice(SQL_ENC_V2_HEADER);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    out.extend_from_slice(&tag);
    out
}

fn decrypt_sql_payload(
    key: &[u8; 16],
    encrypted_body: &[u8],
    operation: &str,
) -> Result<Vec<u8>, AirSqlError> {
    if let Some(v2_body) = encrypted_body.strip_prefix(SQL_ENC_V2_HEADER)
        && v2_body.len() >= 16 + 12 + 32
    {
        let mut salt = [0u8; 16];
        salt.copy_from_slice(&v2_body[..16]);
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&v2_body[16..28]);
        let tag_start = v2_body.len() - 32;
        let ciphertext = &v2_body[28..tag_start];
        let tag = &v2_body[tag_start..];

        let (enc_key, mac_key) = derive_sql_subkeys(key, &salt);
        let mut mac_msg = Vec::with_capacity(4 + 16 + 12 + ciphertext.len());
        mac_msg.extend_from_slice(SQL_ENC_V2_HEADER);
        mac_msg.extend_from_slice(&salt);
        mac_msg.extend_from_slice(&nonce);
        mac_msg.extend_from_slice(ciphertext);
        let expected_tag = hmac_sha256(&mac_key, &mac_msg);
        let mut diff = 0u8;
        for (&a, &b) in tag.iter().zip(expected_tag.iter()) {
            diff |= a ^ b;
        }
        if diff == 0 {
            return Ok(chacha20_xor(&enc_key, &nonce, 1, ciphertext));
        }
        // A legacy payload has no explicit version byte: its 16-byte salt can begin with
        // `CV2\0` by chance. Only accept the v2 interpretation after its MAC verifies, then
        // fall through to the authenticated legacy decoder for compatibility.
    }

    if encrypted_body.len() < 48 {
        return Err(AirSqlError::new(
            operation,
            "Encrypted database header is truncated or corrupt.",
            3138,
        ));
    }
    let salt = &encrypted_body[..16];
    let tag_start = encrypted_body.len() - 32;
    let ciphertext = &encrypted_body[16..tag_start];
    let tag = &encrypted_body[tag_start..];

    let mut mac_msg = Vec::with_capacity(4 + 16 + ciphertext.len());
    mac_msg.extend_from_slice(b"mac\0");
    mac_msg.extend_from_slice(salt);
    mac_msg.extend_from_slice(ciphertext);
    let expected_tag = hmac_sha256(key, &mac_msg);
    let mut diff = 0u8;
    for (&a, &b) in tag.iter().zip(expected_tag.iter()) {
        diff |= a ^ b;
    }
    if diff != 0 {
        return Err(AirSqlError::new(
            operation,
            "File is encrypted or is not a database.",
            3138,
        ));
    }

    let mut plaintext = Vec::with_capacity(ciphertext.len());
    for (block_idx, chunk) in ciphertext.chunks(32).enumerate() {
        let mut ctr_msg = [0u8; 28];
        ctr_msg[..4].copy_from_slice(b"enc\0");
        ctr_msg[4..20].copy_from_slice(salt);
        ctr_msg[20..28].copy_from_slice(&(block_idx as u64).to_be_bytes());
        let ks = hmac_sha256(key, &ctr_msg);
        for (i, &b) in chunk.iter().enumerate() {
            plaintext.push(b ^ ks[i]);
        }
    }
    Ok(plaintext)
}

fn parse_sql_encryption_key(
    key_opt: Option<&[u8]>,
    operation: &str,
) -> Result<Option<[u8; 16]>, AirSqlError> {
    let Some(raw) = key_opt else {
        return Ok(None);
    };
    if raw.len() != 16 {
        return Err(AirSqlError::new(
            operation,
            "SQL encryptionKey must be a 16-byte ByteArray.",
            3143,
        ));
    }
    let mut arr = [0u8; 16];
    arr.copy_from_slice(raw);
    Ok(Some(arr))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "k", content = "v")]
pub enum SqlValue {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
    Blob(String), // base64
}

impl SqlValue {
    fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    fn truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(b) => *b,
            Self::Int(n) => *n != 0,
            Self::Real(f) => *f != 0.0 && !f.is_nan(),
            Self::Text(s) => {
                let t = s.trim();
                !t.is_empty() && t != "0" && !t.eq_ignore_ascii_case("false")
            }
            Self::Blob(b) => !b.is_empty(),
        }
    }

    fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(n) => Some(*n as f64),
            Self::Real(f) => Some(*f),
            Self::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Self::Text(s) => s.trim().parse::<f64>().ok(),
            Self::Null | Self::Blob(_) => None,
        }
    }

    fn as_text(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Int(n) => n.to_string(),
            Self::Real(f) => {
                if f.fract() == 0.0 && f.is_finite() {
                    format!("{f:.1}")
                } else {
                    f.to_string()
                }
            }
            Self::Text(s) => s.clone(),
            Self::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
            Self::Blob(b) => b.clone(),
        }
    }

    fn cmp_sql(&self, other: &Self, nocase: bool) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            (Self::Null, Self::Null) => Ordering::Equal,
            (Self::Null, _) => Ordering::Less,
            (_, Self::Null) => Ordering::Greater,
            (Self::Int(a), Self::Int(b)) => a.cmp(b),
            (Self::Int(a), Self::Real(b)) => (*a as f64).partial_cmp(b).unwrap_or(Ordering::Equal),
            (Self::Real(a), Self::Int(b)) => a.partial_cmp(&(*b as f64)).unwrap_or(Ordering::Equal),
            (Self::Real(a), Self::Real(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (Self::Bool(a), Self::Bool(b)) => a.cmp(b),
            (Self::Bool(a), Self::Int(b)) => i64::from(*a).cmp(b),
            (Self::Int(a), Self::Bool(b)) => a.cmp(&i64::from(*b)),
            (Self::Text(a), Self::Text(b)) => {
                if nocase {
                    a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase())
                } else {
                    a.cmp(b)
                }
            }
            (a, b) => {
                if let (Some(na), Some(nb)) = (a.as_f64(), b.as_f64())
                    && (matches!(a, Self::Int(_) | Self::Real(_) | Self::Bool(_))
                        || matches!(b, Self::Int(_) | Self::Real(_) | Self::Bool(_)))
                {
                    return na.partial_cmp(&nb).unwrap_or(Ordering::Equal);
                }
                let sa = a.as_text();
                let sb = b.as_text();
                if nocase {
                    sa.to_ascii_lowercase().cmp(&sb.to_ascii_lowercase())
                } else {
                    sa.cmp(&sb)
                }
            }
        }
    }

    fn coerce_affinity(&self, data_type: &str) -> Self {
        if self.is_null() {
            return Self::Null;
        }
        let upper = data_type.to_ascii_uppercase();
        if upper.contains("INT") {
            match self {
                Self::Int(n) => Self::Int(*n),
                Self::Real(f) if f.is_finite() && f.fract() == 0.0 => Self::Int(*f as i64),
                Self::Bool(b) => Self::Int(i64::from(*b)),
                Self::Text(s) => s
                    .trim()
                    .parse::<i64>()
                    .map_or_else(|_| self.clone(), Self::Int),
                _ => self.clone(),
            }
        } else if upper.contains("BOOL") {
            match self {
                Self::Bool(b) => Self::Bool(*b),
                Self::Int(n) => Self::Bool(*n != 0),
                _ => self.clone(),
            }
        } else if upper.contains("CHAR") || upper.contains("CLOB") || upper.contains("TEXT") {
            match self {
                Self::Text(_) | Self::Blob(_) => self.clone(),
                other => Self::Text(other.as_text()),
            }
        } else if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
            self.as_f64().map_or_else(|| self.clone(), Self::Real)
        } else {
            self.clone()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlColumnDef {
    pub name: String,
    pub data_type: String,
    pub allow_null: bool,
    pub primary_key: bool,
    pub auto_increment: bool,
    pub unique: bool,
    pub default_collation_type: String,
    pub default_value: Option<SqlValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SqlRow {
    rowid: i64,
    values: Vec<SqlValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SqlTable {
    name: String,
    sql: String,
    columns: Vec<SqlColumnDef>,
    rows: Vec<SqlRow>,
    next_rowid: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SqlIndexMeta {
    name: String,
    table: String,
    sql: String,
    unique: bool,
    columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SqlViewMeta {
    name: String,
    sql: String,
    select_sql: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SqlTriggerMeta {
    name: String,
    table: String,
    sql: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SqlDatabase {
    tables: BTreeMap<String, SqlTable>,
    indices: BTreeMap<String, SqlIndexMeta>,
    views: BTreeMap<String, SqlViewMeta>,
    triggers: BTreeMap<String, SqlTriggerMeta>,
}

#[derive(Debug, Clone)]
struct AttachedDb {
    vfs_path: Option<String>,
    read_only: bool,
    encryption_key: Option<[u8; 16]>,
    db: SqlDatabase,
}

#[derive(Debug, Clone)]
struct SavepointSnap {
    name: String,
    dbs: BTreeMap<String, SqlDatabase>,
}

#[derive(Debug, Clone)]
struct SqlConnState {
    connected: bool,
    auto_compact: bool,
    page_size: u32,
    cache_size: u32,
    column_name_style: String,
    last_insert_rowid: i64,
    total_changes: u64,
    in_transaction: bool,
    tx_base: Option<BTreeMap<String, SqlDatabase>>,
    savepoints: Vec<SavepointSnap>,
    sp_counter: u64,
    attached: BTreeMap<String, AttachedDb>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlUpdateRecord {
    pub action: String,
    pub table: String,
    pub row_id: i64,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    Ident(String), // quoted identifier
    Str(String),
    Int(i64),
    Real(f64),
    Param(String), // ":id", "@id", "$id" or "?"
    LParen,
    RParen,
    Comma,
    Semi,
    Dot,
    Star,
    Plus,
    Minus,
    Slash,
    Percent,
    Concat, // ||
    Eq,     // = or ==
    Ne,     // != or <>
    Lt,
    Le,
    Gt,
    Ge,
}

fn tokenize_sql(sql: &str) -> Result<Vec<Tok>, AirSqlError> {
    if sql.len() > MAX_AIR_SQL_BYTES {
        return Err(AirSqlError::new(
            "execute",
            "SQL text exceeds 256 KiB limit.",
            3115,
        ));
    }
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    let mut pos_param = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 >= bytes.len() {
                return Err(AirSqlError::new(
                    "execute",
                    "Unterminated SQL block comment.",
                    3115,
                ));
            }
            i += 2;
            continue;
        }
        match b {
            b'(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            b')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            b',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            b';' => {
                out.push(Tok::Semi);
                i += 1;
            }
            b'.' if !(i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit()) => {
                out.push(Tok::Dot);
                i += 1;
            }
            b'*' => {
                out.push(Tok::Star);
                i += 1;
            }
            b'+' => {
                out.push(Tok::Plus);
                i += 1;
            }
            b'-' => {
                out.push(Tok::Minus);
                i += 1;
            }
            b'/' => {
                out.push(Tok::Slash);
                i += 1;
            }
            b'%' => {
                out.push(Tok::Percent);
                i += 1;
            }
            b'|' if i + 1 < bytes.len() && bytes[i + 1] == b'|' => {
                out.push(Tok::Concat);
                i += 2;
            }
            b'=' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    i += 2;
                } else {
                    i += 1;
                }
                out.push(Tok::Eq);
            }
            b'!' if i + 1 < bytes.len() && bytes[i + 1] == b'=' => {
                out.push(Tok::Ne);
                i += 2;
            }
            b'<' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    out.push(Tok::Le);
                    i += 2;
                } else if i + 1 < bytes.len() && bytes[i + 1] == b'>' {
                    out.push(Tok::Ne);
                    i += 2;
                } else {
                    out.push(Tok::Lt);
                    i += 1;
                }
            }
            b'>' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    out.push(Tok::Ge);
                    i += 2;
                } else {
                    out.push(Tok::Gt);
                    i += 1;
                }
            }
            b'?' => {
                i += 1;
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i > start {
                    let n: usize = sql[start..i].parse().unwrap_or(1);
                    out.push(Tok::Param(n.saturating_sub(1).to_string()));
                } else {
                    out.push(Tok::Param(pos_param.to_string()));
                    pos_param += 1;
                }
            }
            b':' | b'@' | b'$' => {
                let prefix = b as char;
                i += 1;
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                if i == start {
                    return Err(AirSqlError::new(
                        "execute",
                        "Invalid SQL parameter name.",
                        3115,
                    ));
                }
                out.push(Tok::Param(format!("{prefix}{}", &sql[start..i])));
            }
            b'\'' => {
                i += 1;
                let mut s = String::new();
                let mut closed = false;
                while i < sql.len() {
                    let ch = sql[i..].chars().next().unwrap();
                    i += ch.len_utf8();
                    if ch == '\'' {
                        if i < bytes.len() && bytes[i] == b'\'' {
                            s.push('\'');
                            i += 1;
                        } else {
                            closed = true;
                            break;
                        }
                    } else {
                        s.push(ch);
                    }
                }
                if !closed {
                    return Err(AirSqlError::new(
                        "execute",
                        "Unterminated SQL string literal.",
                        3115,
                    ));
                }
                out.push(Tok::Str(s));
            }
            b'"' | b'`' | b'[' => {
                let closer = if b == b'[' { ']' } else { b as char };
                i += 1;
                let mut s = String::new();
                let mut closed = false;
                while i < sql.len() {
                    let ch = sql[i..].chars().next().unwrap();
                    i += ch.len_utf8();
                    if ch == closer {
                        if closer != ']' && i < bytes.len() && bytes[i] == closer as u8 {
                            s.push(closer);
                            i += 1;
                        } else {
                            closed = true;
                            break;
                        }
                    } else {
                        s.push(ch);
                    }
                }
                if !closed {
                    return Err(AirSqlError::new(
                        "execute",
                        "Unterminated quoted SQL identifier.",
                        3115,
                    ));
                }
                out.push(Tok::Ident(s));
            }
            _ if b.is_ascii_digit()
                || (b == b'.' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit()) =>
            {
                let start = i;
                let mut is_float = false;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'.' {
                    is_float = true;
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                    is_float = true;
                    i += 1;
                    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                        i += 1;
                    }
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let raw = &sql[start..i];
                if is_float {
                    let v = raw.parse::<f64>().map_err(|_| {
                        AirSqlError::new("execute", format!("Invalid number '{raw}'."), 3115)
                    })?;
                    out.push(Tok::Real(v));
                } else {
                    let v = raw.parse::<i64>().map_err(|_| {
                        AirSqlError::new("execute", format!("Invalid integer '{raw}'."), 3115)
                    })?;
                    out.push(Tok::Int(v));
                }
            }
            _ if b.is_ascii_alphabetic() || b == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                out.push(Tok::Word(sql[start..i].to_owned()));
            }
            _ => {
                return Err(AirSqlError::new(
                    "execute",
                    format!("Unexpected character '{}' in SQL statement.", b as char),
                    3115,
                ));
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
enum SqlExpr {
    Lit(SqlValue),
    Param(String),
    Col {
        table: Option<String>,
        name: String,
    },
    UnaryMinus(Box<SqlExpr>),
    Not(Box<SqlExpr>),
    Binary {
        op: BinOp,
        left: Box<SqlExpr>,
        right: Box<SqlExpr>,
    },
    IsNull {
        expr: Box<SqlExpr>,
        negated: bool,
    },
    InList {
        expr: Box<SqlExpr>,
        list: Vec<SqlExpr>,
        negated: bool,
    },
    Between {
        expr: Box<SqlExpr>,
        low: Box<SqlExpr>,
        high: Box<SqlExpr>,
        negated: bool,
    },
    Like {
        expr: Box<SqlExpr>,
        pattern: Box<SqlExpr>,
        negated: bool,
    },
    Call {
        name: String,
        distinct: bool,
        star: bool,
        args: Vec<SqlExpr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Concat,
}

#[derive(Debug, Clone)]
enum SelectItem {
    All(Option<String>),
    Expr {
        expr: SqlExpr,
        alias: Option<String>,
    },
}

#[derive(Debug, Clone)]
struct JoinClause {
    left_outer: bool,
    db: Option<String>,
    table: String,
    alias: Option<String>,
    on: Option<SqlExpr>,
}

#[derive(Debug, Clone)]
struct OrderItem {
    expr: SqlExpr,
    desc: bool,
}

#[derive(Debug, Clone)]
struct SelectPlan {
    distinct: bool,
    items: Vec<SelectItem>,
    from_db: Option<String>,
    from_table: Option<String>,
    from_alias: Option<String>,
    joins: Vec<JoinClause>,
    where_expr: Option<SqlExpr>,
    group_by: Vec<SqlExpr>,
    having: Option<SqlExpr>,
    order_by: Vec<OrderItem>,
    limit: Option<usize>,
    offset: usize,
}

struct SqlParser<'a> {
    toks: &'a [Tok],
    pos: usize,
}

impl<'a> SqlParser<'a> {
    fn new(toks: &'a [Tok]) -> Self {
        Self { toks, pos: 0 }
    }

    fn peek(&self) -> Option<&'a Tok> {
        self.toks.get(self.pos)
    }

    fn at_end(&self) -> bool {
        matches!(self.peek(), None | Some(Tok::Semi))
    }

    fn kw(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Tok::Word(w)) if w.eq_ignore_ascii_case(word))
    }

    fn eat_kw(&mut self, word: &str) -> bool {
        if self.kw(word) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_kw(&mut self, word: &str) -> Result<(), AirSqlError> {
        if self.eat_kw(word) {
            Ok(())
        } else {
            Err(AirSqlError::new(
                "execute",
                format!("Expected SQL keyword '{word}'."),
                3115,
            ))
        }
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek() == Some(tok) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, tok: &Tok) -> Result<(), AirSqlError> {
        if self.eat(tok) {
            Ok(())
        } else {
            Err(AirSqlError::new(
                "execute",
                format!("Unexpected SQL syntax near token #{}.", self.pos + 1),
                3115,
            ))
        }
    }

    fn ident(&mut self) -> Result<String, AirSqlError> {
        match self.peek() {
            Some(Tok::Word(w)) | Some(Tok::Ident(w)) => {
                let s = w.clone();
                self.pos += 1;
                Ok(s)
            }
            Some(Tok::Str(s)) => {
                let v = s.clone();
                self.pos += 1;
                Ok(v)
            }
            _ => Err(AirSqlError::new(
                "execute",
                "Expected SQL identifier.",
                3115,
            )),
        }
    }

    fn qualified_name(&mut self) -> Result<(Option<String>, String), AirSqlError> {
        let first = self.ident()?;
        if self.eat(&Tok::Dot) {
            let second = self.ident()?;
            Ok((Some(first), second))
        } else {
            Ok((None, first))
        }
    }

    fn expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        self.or_expr()
    }

    fn or_expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        let mut left = self.and_expr()?;
        while self.eat_kw("OR") {
            let right = self.and_expr()?;
            left = SqlExpr::Binary {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        let mut left = self.not_expr()?;
        while self.eat_kw("AND") {
            let right = self.not_expr()?;
            left = SqlExpr::Binary {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn not_expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        if self.eat_kw("NOT") {
            Ok(SqlExpr::Not(Box::new(self.not_expr()?)))
        } else {
            self.cmp_expr()
        }
    }

    fn cmp_expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        let mut left = self.add_expr()?;
        loop {
            if self.eat_kw("IS") {
                let negated = self.eat_kw("NOT");
                if self.eat_kw("NULL") {
                    left = SqlExpr::IsNull {
                        expr: Box::new(left),
                        negated,
                    };
                    continue;
                }
                let right = self.add_expr()?;
                left = SqlExpr::Binary {
                    op: if negated { BinOp::Ne } else { BinOp::Eq },
                    left: Box::new(left),
                    right: Box::new(right),
                };
                continue;
            }
            let negated_op = if self.kw("NOT") {
                let saved = self.pos;
                self.pos += 1;
                if self.kw("IN") || self.kw("LIKE") || self.kw("BETWEEN") {
                    true
                } else {
                    self.pos = saved;
                    false
                }
            } else {
                false
            };
            if self.eat_kw("IN") {
                self.expect(&Tok::LParen)?;
                let mut list = Vec::new();
                if !self.eat(&Tok::RParen) {
                    loop {
                        list.push(self.expr()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(&Tok::RParen)?;
                }
                left = SqlExpr::InList {
                    expr: Box::new(left),
                    list,
                    negated: negated_op,
                };
                continue;
            }
            if self.eat_kw("BETWEEN") {
                let low = self.add_expr()?;
                self.expect_kw("AND")?;
                let high = self.add_expr()?;
                left = SqlExpr::Between {
                    expr: Box::new(left),
                    low: Box::new(low),
                    high: Box::new(high),
                    negated: negated_op,
                };
                continue;
            }
            if self.eat_kw("LIKE") {
                let pattern = self.add_expr()?;
                left = SqlExpr::Like {
                    expr: Box::new(left),
                    pattern: Box::new(pattern),
                    negated: negated_op,
                };
                continue;
            }
            let op = match self.peek() {
                Some(Tok::Eq) => Some(BinOp::Eq),
                Some(Tok::Ne) => Some(BinOp::Ne),
                Some(Tok::Lt) => Some(BinOp::Lt),
                Some(Tok::Le) => Some(BinOp::Le),
                Some(Tok::Gt) => Some(BinOp::Gt),
                Some(Tok::Ge) => Some(BinOp::Ge),
                _ => None,
            };
            if let Some(op) = op {
                self.pos += 1;
                let right = self.add_expr()?;
                left = SqlExpr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn add_expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        let mut left = self.mul_expr()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => Some(BinOp::Add),
                Some(Tok::Minus) => Some(BinOp::Sub),
                Some(Tok::Concat) => Some(BinOp::Concat),
                _ => None,
            };
            if let Some(op) = op {
                self.pos += 1;
                let right = self.mul_expr()?;
                left = SqlExpr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn mul_expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        let mut left = self.unary_expr()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Star) => Some(BinOp::Mul),
                Some(Tok::Slash) => Some(BinOp::Div),
                Some(Tok::Percent) => Some(BinOp::Rem),
                _ => None,
            };
            if let Some(op) = op {
                self.pos += 1;
                let right = self.unary_expr()?;
                left = SqlExpr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn unary_expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        if self.eat(&Tok::Plus) {
            return self.unary_expr();
        }
        if self.eat(&Tok::Minus) {
            return Ok(SqlExpr::UnaryMinus(Box::new(self.unary_expr()?)));
        }
        self.primary_expr()
    }

    fn primary_expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        if self.eat(&Tok::LParen) {
            let inner = self.expr()?;
            self.expect(&Tok::RParen)?;
            return Ok(inner);
        }
        match self.peek().cloned() {
            Some(Tok::Int(n)) => {
                self.pos += 1;
                Ok(SqlExpr::Lit(SqlValue::Int(n)))
            }
            Some(Tok::Real(f)) => {
                self.pos += 1;
                Ok(SqlExpr::Lit(SqlValue::Real(f)))
            }
            Some(Tok::Str(s)) => {
                self.pos += 1;
                Ok(SqlExpr::Lit(SqlValue::Text(s)))
            }
            Some(Tok::Param(p)) => {
                self.pos += 1;
                Ok(SqlExpr::Param(p))
            }
            Some(Tok::Word(w)) if w.eq_ignore_ascii_case("NULL") => {
                self.pos += 1;
                Ok(SqlExpr::Lit(SqlValue::Null))
            }
            Some(Tok::Word(w)) if w.eq_ignore_ascii_case("TRUE") => {
                self.pos += 1;
                Ok(SqlExpr::Lit(SqlValue::Bool(true)))
            }
            Some(Tok::Word(w)) if w.eq_ignore_ascii_case("FALSE") => {
                self.pos += 1;
                Ok(SqlExpr::Lit(SqlValue::Bool(false)))
            }
            Some(Tok::Word(w)) | Some(Tok::Ident(w)) => {
                let is_word = matches!(self.peek(), Some(Tok::Word(_)));
                self.pos += 1;
                if is_word && self.eat(&Tok::LParen) {
                    if self.eat(&Tok::Star) {
                        self.expect(&Tok::RParen)?;
                        return Ok(SqlExpr::Call {
                            name: w.to_ascii_uppercase(),
                            distinct: false,
                            star: true,
                            args: Vec::new(),
                        });
                    }
                    let distinct = self.eat_kw("DISTINCT");
                    let mut args = Vec::new();
                    if !self.eat(&Tok::RParen) {
                        loop {
                            args.push(self.expr()?);
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                        }
                        self.expect(&Tok::RParen)?;
                    }
                    return Ok(SqlExpr::Call {
                        name: w.to_ascii_uppercase(),
                        distinct,
                        star: false,
                        args,
                    });
                }
                if self.eat(&Tok::Dot) {
                    let col = self.ident()?;
                    Ok(SqlExpr::Col {
                        table: Some(w),
                        name: col,
                    })
                } else {
                    Ok(SqlExpr::Col {
                        table: None,
                        name: w,
                    })
                }
            }
            _ => Err(AirSqlError::new(
                "execute",
                "Expected SQL expression.",
                3115,
            )),
        }
    }

    fn optional_table_alias(&mut self) -> Option<String> {
        let has_as = self.eat_kw("AS");
        match self.peek() {
            Some(Tok::Word(w)) => {
                if !has_as
                    && matches!(
                        w.to_ascii_uppercase().as_str(),
                        "WHERE"
                            | "GROUP"
                            | "HAVING"
                            | "ORDER"
                            | "LIMIT"
                            | "OFFSET"
                            | "JOIN"
                            | "INNER"
                            | "LEFT"
                            | "CROSS"
                            | "ON"
                    )
                {
                    return None;
                }
                let s = w.clone();
                self.pos += 1;
                Some(s)
            }
            Some(Tok::Ident(w)) => {
                let s = w.clone();
                self.pos += 1;
                Some(s)
            }
            _ => None,
        }
    }

    fn parse_select(&mut self) -> Result<SelectPlan, AirSqlError> {
        self.expect_kw("SELECT")?;
        let distinct = if self.eat_kw("DISTINCT") {
            true
        } else {
            self.eat_kw("ALL");
            false
        };
        let mut items = Vec::new();
        loop {
            if self.eat(&Tok::Star) {
                items.push(SelectItem::All(None));
            } else if matches!(
                (
                    self.toks.get(self.pos),
                    self.toks.get(self.pos + 1),
                    self.toks.get(self.pos + 2)
                ),
                (
                    Some(Tok::Word(_) | Tok::Ident(_)),
                    Some(Tok::Dot),
                    Some(Tok::Star)
                )
            ) {
                let tbl = self.ident()?;
                self.pos += 2;
                items.push(SelectItem::All(Some(tbl)));
            } else {
                let expr = self.expr()?;
                let alias = if self.eat_kw("AS") {
                    Some(self.ident()?)
                } else if let Some(Tok::Word(w) | Tok::Ident(w)) = self.peek() {
                    if !matches!(
                        w.to_ascii_uppercase().as_str(),
                        "FROM" | "WHERE" | "GROUP" | "HAVING" | "ORDER" | "LIMIT"
                    ) {
                        let a = w.clone();
                        self.pos += 1;
                        Some(a)
                    } else {
                        None
                    }
                } else {
                    None
                };
                items.push(SelectItem::Expr { expr, alias });
            }
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        let (mut from_db, mut from_table, mut from_alias) = (None, None, None);
        let mut joins = Vec::new();
        if self.eat_kw("FROM") {
            let (db, tbl) = self.qualified_name()?;
            from_db = db;
            from_table = Some(tbl);
            from_alias = self.optional_table_alias();
            loop {
                let mut left_outer = false;
                if self.eat_kw("LEFT") {
                    self.eat_kw("OUTER");
                    self.expect_kw("JOIN")?;
                    left_outer = true;
                } else if self.eat_kw("INNER") || self.eat_kw("CROSS") {
                    self.expect_kw("JOIN")?;
                } else if self.eat_kw("JOIN") || self.eat(&Tok::Comma) {
                    // plain JOIN / comma join
                } else {
                    break;
                }
                let (jdb, jtbl) = self.qualified_name()?;
                let jalias = self.optional_table_alias();
                let on = if self.eat_kw("ON") {
                    Some(self.expr()?)
                } else {
                    None
                };
                joins.push(JoinClause {
                    left_outer,
                    db: jdb,
                    table: jtbl,
                    alias: jalias,
                    on,
                });
            }
        }
        let where_expr = if self.eat_kw("WHERE") {
            Some(self.expr()?)
        } else {
            None
        };
        let mut group_by = Vec::new();
        if self.eat_kw("GROUP") {
            self.expect_kw("BY")?;
            loop {
                group_by.push(self.expr()?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        let having = if self.eat_kw("HAVING") {
            Some(self.expr()?)
        } else {
            None
        };
        let mut order_by = Vec::new();
        if self.eat_kw("ORDER") {
            self.expect_kw("BY")?;
            loop {
                let expr = self.expr()?;
                self.eat_kw("COLLATE");
                if matches!(self.peek(), Some(Tok::Word(w)) if w.eq_ignore_ascii_case("NOCASE") || w.eq_ignore_ascii_case("BINARY"))
                {
                    self.pos += 1;
                }
                let desc = if self.eat_kw("DESC") {
                    true
                } else {
                    self.eat_kw("ASC");
                    false
                };
                order_by.push(OrderItem { expr, desc });
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        let mut limit = None;
        let mut offset = 0usize;
        if self.eat_kw("LIMIT") {
            let lim_val = self.parse_nonneg_int()?;
            if self.eat_kw("OFFSET") {
                offset = self.parse_nonneg_int()?;
                limit = Some(lim_val);
            } else if self.eat(&Tok::Comma) {
                offset = lim_val;
                limit = Some(self.parse_nonneg_int()?);
            } else {
                limit = Some(lim_val);
            }
        }
        Ok(SelectPlan {
            distinct,
            items,
            from_db,
            from_table,
            from_alias,
            joins,
            where_expr,
            group_by,
            having,
            order_by,
            limit,
            offset,
        })
    }

    fn parse_nonneg_int(&mut self) -> Result<usize, AirSqlError> {
        match self.peek() {
            Some(Tok::Int(n)) if *n >= 0 => {
                let v = *n as usize;
                self.pos += 1;
                Ok(v)
            }
            _ => Err(AirSqlError::new(
                "execute",
                "Expected non-negative integer in LIMIT/OFFSET.",
                3115,
            )),
        }
    }

    fn parse_column_def(&mut self) -> Result<SqlColumnDef, AirSqlError> {
        let name = self.ident()?;
        let mut type_words = Vec::new();
        while let Some(Tok::Word(w)) = self.peek() {
            if matches!(
                w.to_ascii_uppercase().as_str(),
                "PRIMARY"
                    | "NOT"
                    | "NULL"
                    | "UNIQUE"
                    | "DEFAULT"
                    | "COLLATE"
                    | "AUTOINCREMENT"
                    | "CHECK"
                    | "REFERENCES"
                    | "CONSTRAINT"
            ) {
                break;
            }
            type_words.push(w.clone());
            self.pos += 1;
            if self.eat(&Tok::LParen) {
                let mut depth = 1;
                while depth > 0 && self.pos < self.toks.len() {
                    if self.eat(&Tok::LParen) {
                        depth += 1;
                    } else if self.eat(&Tok::RParen) {
                        depth -= 1;
                    } else {
                        self.pos += 1;
                    }
                }
            }
        }
        let data_type = if type_words.is_empty() {
            "TEXT".to_owned()
        } else {
            type_words.join(" ").to_ascii_uppercase()
        };
        let mut allow_null = true;
        let mut primary_key = false;
        let mut auto_increment = false;
        let mut unique = false;
        let mut default_collation_type = "BINARY".to_owned();
        let mut default_value = None;

        while !self.at_end() && !matches!(self.peek(), Some(Tok::Comma | Tok::RParen)) {
            if self.eat_kw("CONSTRAINT") {
                let _ = self.ident()?;
            } else if self.eat_kw("PRIMARY") {
                self.expect_kw("KEY")?;
                self.eat_kw("ASC");
                self.eat_kw("DESC");
                primary_key = true;
                if self.eat_kw("AUTOINCREMENT") {
                    auto_increment = true;
                }
            } else if self.eat_kw("AUTOINCREMENT") {
                auto_increment = true;
            } else if self.eat_kw("NOT") {
                self.expect_kw("NULL")?;
                allow_null = false;
            } else if self.eat_kw("NULL") {
                allow_null = true;
            } else if self.eat_kw("UNIQUE") {
                unique = true;
            } else if self.eat_kw("COLLATE") {
                let c = self.ident()?.to_ascii_uppercase();
                default_collation_type = if c == "NOCASE" {
                    "NO_CASE".to_owned()
                } else {
                    "BINARY".to_owned()
                };
            } else if self.eat_kw("DEFAULT") {
                let expr = self.unary_expr()?;
                default_value = Some(eval_const_expr(&expr)?);
            } else if self.eat_kw("CHECK") {
                self.expect(&Tok::LParen)?;
                let mut depth = 1;
                while depth > 0 && self.pos < self.toks.len() {
                    if self.eat(&Tok::LParen) {
                        depth += 1;
                    } else if self.eat(&Tok::RParen) {
                        depth -= 1;
                    } else {
                        self.pos += 1;
                    }
                }
            } else if self.eat_kw("REFERENCES") {
                let _ = self.ident()?;
                if self.eat(&Tok::LParen) {
                    while !self.eat(&Tok::RParen) && self.pos < self.toks.len() {
                        self.pos += 1;
                    }
                }
            } else {
                break;
            }
        }
        if primary_key && data_type == "INTEGER" {
            auto_increment = true;
        }
        Ok(SqlColumnDef {
            name,
            data_type,
            allow_null,
            primary_key,
            auto_increment,
            unique,
            default_collation_type,
            default_value,
        })
    }
}

fn eval_const_expr(expr: &SqlExpr) -> Result<SqlValue, AirSqlError> {
    match expr {
        SqlExpr::Lit(v) => Ok(v.clone()),
        SqlExpr::UnaryMinus(inner) => match eval_const_expr(inner)? {
            SqlValue::Int(n) => Ok(SqlValue::Int(-n)),
            SqlValue::Real(f) => Ok(SqlValue::Real(-f)),
            _ => Err(AirSqlError::new(
                "execute",
                "Invalid DEFAULT numeric literal.",
                3115,
            )),
        },
        _ => Err(AirSqlError::new(
            "execute",
            "Column DEFAULT requires a literal value in this bounded SQL profile.",
            3115,
        )),
    }
}

fn sql_like_match(value: &str, pattern: &str) -> bool {
    let v: Vec<char> = value.chars().map(|c| c.to_ascii_lowercase()).collect();
    let p: Vec<char> = pattern.chars().map(|c| c.to_ascii_lowercase()).collect();
    let (mut vi, mut pi) = (0usize, 0usize);
    let (mut star_pi, mut star_vi) = (None::<usize>, 0usize);
    while vi < v.len() {
        if pi < p.len() && (p[pi] == '_' || p[pi] == v[vi]) {
            vi += 1;
            pi += 1;
        } else if pi < p.len() && p[pi] == '%' {
            star_pi = Some(pi);
            pi += 1;
            star_vi = vi;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_vi += 1;
            vi = star_vi;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '%' {
        pi += 1;
    }
    pi == p.len()
}

#[derive(Debug, Clone)]
struct EvalColMeta {
    table_name: String,
    table_alias: String,
    col_name: String,
    nocase: bool,
}

#[derive(Debug, Clone)]
struct EvalRow {
    rowid: Option<i64>,
    values: Vec<SqlValue>,
}

fn resolve_param(params: &BTreeMap<String, SqlValue>, key: &str) -> Result<SqlValue, AirSqlError> {
    if let Some(v) = params.get(key) {
        return Ok(v.clone());
    }
    let stripped = key.trim_start_matches([':', '@', '$']);
    if let Some(v) = params.get(stripped) {
        return Ok(v.clone());
    }
    for prefix in [":", "@", "$"] {
        if let Some(v) = params.get(&format!("{prefix}{stripped}")) {
            return Ok(v.clone());
        }
    }
    Err(AirSqlError::new(
        "execute",
        format!("Missing bound value for SQL parameter '{key}'."),
        3115,
    ))
}

fn expr_contains_aggregate(expr: &SqlExpr) -> bool {
    match expr {
        SqlExpr::Call { name, args, .. } => {
            matches!(
                name.as_str(),
                "COUNT" | "SUM" | "AVG" | "MIN" | "MAX" | "TOTAL"
            ) || args.iter().any(expr_contains_aggregate)
        }
        SqlExpr::UnaryMinus(e) | SqlExpr::Not(e) | SqlExpr::IsNull { expr: e, .. } => {
            expr_contains_aggregate(e)
        }
        SqlExpr::Binary { left, right, .. } => {
            expr_contains_aggregate(left) || expr_contains_aggregate(right)
        }
        SqlExpr::InList { expr, list, .. } => {
            expr_contains_aggregate(expr) || list.iter().any(expr_contains_aggregate)
        }
        SqlExpr::Between {
            expr, low, high, ..
        } => {
            expr_contains_aggregate(expr)
                || expr_contains_aggregate(low)
                || expr_contains_aggregate(high)
        }
        SqlExpr::Like { expr, pattern, .. } => {
            expr_contains_aggregate(expr) || expr_contains_aggregate(pattern)
        }
        SqlExpr::Lit(_) | SqlExpr::Param(_) | SqlExpr::Col { .. } => false,
    }
}

type ProjectedRowCtx = (Vec<SqlValue>, Option<EvalRow>, Option<Vec<EvalRow>>);
type OrderKeyedRow = (Vec<(SqlValue, bool)>, Vec<SqlValue>);

fn default_expr_column_name(expr: &SqlExpr, long_style: bool) -> String {
    match expr {
        SqlExpr::Col { table, name } => {
            if long_style && let Some(t) = table {
                return format!("{t}_{name}");
            }
            name.clone()
        }
        SqlExpr::Call {
            name, star: true, ..
        } => format!("{name}(*)"),
        SqlExpr::Call { name, args, .. } => {
            let inner: Vec<String> = args
                .iter()
                .map(|a| default_expr_column_name(a, false))
                .collect();
            format!("{name}({})", inner.join(","))
        }
        _ => "column".to_owned(),
    }
}

fn eval_sql_expr(
    expr: &SqlExpr,
    cols: &[EvalColMeta],
    row: Option<&EvalRow>,
    group: Option<&[EvalRow]>,
    params: &BTreeMap<String, SqlValue>,
    extra_aliases: Option<&[(&str, SqlValue)]>,
) -> Result<SqlValue, AirSqlError> {
    let active_row = row.or_else(|| group.and_then(|g| g.first()));
    match expr {
        SqlExpr::Lit(v) => Ok(v.clone()),
        SqlExpr::Param(p) => resolve_param(params, p),
        SqlExpr::Col { table, name } => {
            if table.is_none()
                && let Some(aliases) = extra_aliases
                && let Some((_, val)) = aliases.iter().find(|(a, _)| a.eq_ignore_ascii_case(name))
            {
                return Ok(val.clone());
            }
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "rowid" | "_rowid_" | "oid"
            ) && let Some(r) = active_row
                && let Some(id) = r.rowid
            {
                let has_explicit = cols.iter().any(|c| c.col_name.eq_ignore_ascii_case(name));
                if !has_explicit {
                    return Ok(SqlValue::Int(id));
                }
            }
            let mut matched: Option<(usize, &EvalColMeta)> = None;
            for (idx, c) in cols.iter().enumerate() {
                let table_ok = match table {
                    Some(t) => {
                        c.table_alias.eq_ignore_ascii_case(t)
                            || c.table_name.eq_ignore_ascii_case(t)
                    }
                    None => true,
                };
                if table_ok && c.col_name.eq_ignore_ascii_case(name) {
                    matched = Some((idx, c));
                    break;
                }
            }
            let (idx, _) = matched.ok_or_else(|| {
                AirSqlError::new("execute", format!("No such column '{name}'."), 3115)
            })?;
            Ok(active_row
                .and_then(|r| r.values.get(idx).cloned())
                .unwrap_or(SqlValue::Null))
        }
        SqlExpr::UnaryMinus(inner) => {
            let v = eval_sql_expr(inner, cols, row, group, params, extra_aliases)?;
            match v {
                SqlValue::Null => Ok(SqlValue::Null),
                SqlValue::Int(n) => Ok(SqlValue::Int(-n)),
                SqlValue::Real(f) => Ok(SqlValue::Real(-f)),
                other => Ok(SqlValue::Real(-other.as_f64().unwrap_or(0.0))),
            }
        }
        SqlExpr::Not(inner) => {
            let v = eval_sql_expr(inner, cols, row, group, params, extra_aliases)?;
            Ok(if v.is_null() {
                SqlValue::Null
            } else {
                SqlValue::Bool(!v.truthy())
            })
        }
        SqlExpr::IsNull { expr, negated } => {
            let v = eval_sql_expr(expr, cols, row, group, params, extra_aliases)?;
            let is_n = v.is_null();
            Ok(SqlValue::Bool(if *negated { !is_n } else { is_n }))
        }
        SqlExpr::InList {
            expr,
            list,
            negated,
        } => {
            let lhs = eval_sql_expr(expr, cols, row, group, params, extra_aliases)?;
            if lhs.is_null() {
                return Ok(SqlValue::Null);
            }
            let mut found = false;
            for item in list {
                let rhs = eval_sql_expr(item, cols, row, group, params, extra_aliases)?;
                if !rhs.is_null() && lhs.cmp_sql(&rhs, false) == std::cmp::Ordering::Equal {
                    found = true;
                    break;
                }
            }
            Ok(SqlValue::Bool(if *negated { !found } else { found }))
        }
        SqlExpr::Between {
            expr,
            low,
            high,
            negated,
        } => {
            let v = eval_sql_expr(expr, cols, row, group, params, extra_aliases)?;
            let lo = eval_sql_expr(low, cols, row, group, params, extra_aliases)?;
            let hi = eval_sql_expr(high, cols, row, group, params, extra_aliases)?;
            if v.is_null() || lo.is_null() || hi.is_null() {
                return Ok(SqlValue::Null);
            }
            let inside = v.cmp_sql(&lo, false) != std::cmp::Ordering::Less
                && v.cmp_sql(&hi, false) != std::cmp::Ordering::Greater;
            Ok(SqlValue::Bool(if *negated { !inside } else { inside }))
        }
        SqlExpr::Like {
            expr,
            pattern,
            negated,
        } => {
            let v = eval_sql_expr(expr, cols, row, group, params, extra_aliases)?;
            let p = eval_sql_expr(pattern, cols, row, group, params, extra_aliases)?;
            if v.is_null() || p.is_null() {
                return Ok(SqlValue::Null);
            }
            let m = sql_like_match(&v.as_text(), &p.as_text());
            Ok(SqlValue::Bool(if *negated { !m } else { m }))
        }
        SqlExpr::Binary { op, left, right } => match op {
            BinOp::And => {
                let l = eval_sql_expr(left, cols, row, group, params, extra_aliases)?;
                if !l.is_null() && !l.truthy() {
                    return Ok(SqlValue::Bool(false));
                }
                let r = eval_sql_expr(right, cols, row, group, params, extra_aliases)?;
                if !r.is_null() && !r.truthy() {
                    return Ok(SqlValue::Bool(false));
                }
                if l.is_null() || r.is_null() {
                    Ok(SqlValue::Null)
                } else {
                    Ok(SqlValue::Bool(true))
                }
            }
            BinOp::Or => {
                let l = eval_sql_expr(left, cols, row, group, params, extra_aliases)?;
                if l.truthy() {
                    return Ok(SqlValue::Bool(true));
                }
                let r = eval_sql_expr(right, cols, row, group, params, extra_aliases)?;
                if r.truthy() {
                    return Ok(SqlValue::Bool(true));
                }
                if l.is_null() || r.is_null() {
                    Ok(SqlValue::Null)
                } else {
                    Ok(SqlValue::Bool(false))
                }
            }
            _ => {
                let l = eval_sql_expr(left, cols, row, group, params, extra_aliases)?;
                let r = eval_sql_expr(right, cols, row, group, params, extra_aliases)?;
                if l.is_null() || r.is_null() {
                    return Ok(SqlValue::Null);
                }
                let nocase = col_uses_nocase(left, cols) || col_uses_nocase(right, cols);
                match op {
                    BinOp::Eq => Ok(SqlValue::Bool(
                        l.cmp_sql(&r, nocase) == std::cmp::Ordering::Equal,
                    )),
                    BinOp::Ne => Ok(SqlValue::Bool(
                        l.cmp_sql(&r, nocase) != std::cmp::Ordering::Equal,
                    )),
                    BinOp::Lt => Ok(SqlValue::Bool(
                        l.cmp_sql(&r, nocase) == std::cmp::Ordering::Less,
                    )),
                    BinOp::Le => Ok(SqlValue::Bool(
                        l.cmp_sql(&r, nocase) != std::cmp::Ordering::Greater,
                    )),
                    BinOp::Gt => Ok(SqlValue::Bool(
                        l.cmp_sql(&r, nocase) == std::cmp::Ordering::Greater,
                    )),
                    BinOp::Ge => Ok(SqlValue::Bool(
                        l.cmp_sql(&r, nocase) != std::cmp::Ordering::Less,
                    )),
                    BinOp::Concat => Ok(SqlValue::Text(format!("{}{}", l.as_text(), r.as_text()))),
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                        if let (SqlValue::Int(a), SqlValue::Int(b)) = (&l, &r) {
                            return match op {
                                BinOp::Add => Ok(SqlValue::Int(a.saturating_add(*b))),
                                BinOp::Sub => Ok(SqlValue::Int(a.saturating_sub(*b))),
                                BinOp::Mul => Ok(SqlValue::Int(a.saturating_mul(*b))),
                                BinOp::Div => Ok(if *b == 0 {
                                    SqlValue::Null
                                } else {
                                    SqlValue::Int(a / b)
                                }),
                                BinOp::Rem => Ok(if *b == 0 {
                                    SqlValue::Null
                                } else {
                                    SqlValue::Int(a % b)
                                }),
                                _ => unreachable!(),
                            };
                        }
                        let a = l.as_f64().unwrap_or(0.0);
                        let b = r.as_f64().unwrap_or(0.0);
                        match op {
                            BinOp::Add => Ok(SqlValue::Real(a + b)),
                            BinOp::Sub => Ok(SqlValue::Real(a - b)),
                            BinOp::Mul => Ok(SqlValue::Real(a * b)),
                            BinOp::Div => Ok(if b == 0.0 {
                                SqlValue::Null
                            } else {
                                SqlValue::Real(a / b)
                            }),
                            BinOp::Rem => Ok(if b == 0.0 {
                                SqlValue::Null
                            } else {
                                SqlValue::Real(a % b)
                            }),
                            _ => unreachable!(),
                        }
                    }
                    BinOp::And | BinOp::Or => unreachable!(),
                }
            }
        },
        SqlExpr::Call {
            name,
            distinct,
            star,
            args,
        } => {
            if matches!(
                name.as_str(),
                "COUNT" | "SUM" | "AVG" | "MIN" | "MAX" | "TOTAL"
            ) {
                let rows_slice: &[EvalRow] = if let Some(g) = group {
                    g
                } else if let Some(r) = row {
                    std::slice::from_ref(r)
                } else {
                    &[]
                };
                if name == "COUNT" && *star {
                    return Ok(SqlValue::Int(rows_slice.len() as i64));
                }
                let arg = args.first().ok_or_else(|| {
                    AirSqlError::new("execute", format!("{name}() requires an argument."), 3115)
                })?;
                let mut vals = Vec::new();
                for r in rows_slice {
                    let v = eval_sql_expr(arg, cols, Some(r), None, params, None)?;
                    if !v.is_null() {
                        if *distinct
                            && vals.iter().any(|old: &SqlValue| {
                                old.cmp_sql(&v, false) == std::cmp::Ordering::Equal
                            })
                        {
                            continue;
                        }
                        vals.push(v);
                    }
                }
                return match name.as_str() {
                    "COUNT" => Ok(SqlValue::Int(vals.len() as i64)),
                    "SUM" | "TOTAL" => {
                        if vals.is_empty() {
                            return Ok(if name == "TOTAL" {
                                SqlValue::Real(0.0)
                            } else {
                                SqlValue::Null
                            });
                        }
                        if name == "SUM" && vals.iter().all(|v| matches!(v, SqlValue::Int(_))) {
                            let s: i64 = vals
                                .iter()
                                .map(|v| match v {
                                    SqlValue::Int(n) => *n,
                                    _ => 0,
                                })
                                .sum();
                            Ok(SqlValue::Int(s))
                        } else {
                            let s: f64 = vals.iter().filter_map(SqlValue::as_f64).sum();
                            Ok(SqlValue::Real(s))
                        }
                    }
                    "AVG" => {
                        if vals.is_empty() {
                            Ok(SqlValue::Null)
                        } else {
                            let s: f64 = vals.iter().filter_map(SqlValue::as_f64).sum();
                            Ok(SqlValue::Real(s / (vals.len() as f64)))
                        }
                    }
                    "MIN" => Ok(vals
                        .into_iter()
                        .min_by(|a, b| a.cmp_sql(b, false))
                        .unwrap_or(SqlValue::Null)),
                    "MAX" => Ok(vals
                        .into_iter()
                        .max_by(|a, b| a.cmp_sql(b, false))
                        .unwrap_or(SqlValue::Null)),
                    _ => unreachable!(),
                };
            }
            let mut evaluated = Vec::with_capacity(args.len());
            for a in args {
                evaluated.push(eval_sql_expr(a, cols, row, group, params, extra_aliases)?);
            }
            match name.as_str() {
                "COALESCE" | "IFNULL" => Ok(evaluated
                    .into_iter()
                    .find(|v| !v.is_null())
                    .unwrap_or(SqlValue::Null)),
                "NULLIF" => {
                    if evaluated.len() == 2
                        && evaluated[0].cmp_sql(&evaluated[1], false) == std::cmp::Ordering::Equal
                    {
                        Ok(SqlValue::Null)
                    } else {
                        Ok(evaluated.into_iter().next().unwrap_or(SqlValue::Null))
                    }
                }
                "UPPER" => Ok(match evaluated.first() {
                    None | Some(SqlValue::Null) => SqlValue::Null,
                    Some(v) => SqlValue::Text(v.as_text().to_uppercase()),
                }),
                "LOWER" => Ok(match evaluated.first() {
                    None | Some(SqlValue::Null) => SqlValue::Null,
                    Some(v) => SqlValue::Text(v.as_text().to_lowercase()),
                }),
                "TRIM" => Ok(match evaluated.first() {
                    None | Some(SqlValue::Null) => SqlValue::Null,
                    Some(v) => SqlValue::Text(v.as_text().trim().to_owned()),
                }),
                "LENGTH" => Ok(match evaluated.first() {
                    None | Some(SqlValue::Null) => SqlValue::Null,
                    Some(SqlValue::Blob(b)) => {
                        let decoded = BASE64.decode(b).unwrap_or_default();
                        SqlValue::Int(decoded.len() as i64)
                    }
                    Some(v) => SqlValue::Int(v.as_text().chars().count() as i64),
                }),
                "ABS" => Ok(match evaluated.first() {
                    None | Some(SqlValue::Null) => SqlValue::Null,
                    Some(SqlValue::Int(n)) => SqlValue::Int(n.abs()),
                    Some(v) => SqlValue::Real(v.as_f64().unwrap_or(0.0).abs()),
                }),
                "ROUND" => match evaluated.first() {
                    None | Some(SqlValue::Null) => Ok(SqlValue::Null),
                    Some(v) => {
                        let f = v.as_f64().unwrap_or(0.0);
                        let places =
                            evaluated.get(1).and_then(SqlValue::as_f64).unwrap_or(0.0) as i32;
                        let factor = 10_f64.powi(places.clamp(0, 12));
                        Ok(SqlValue::Real((f * factor).round() / factor))
                    }
                },
                "SUBSTR" | "SUBSTRING" => match evaluated.first() {
                    None | Some(SqlValue::Null) => Ok(SqlValue::Null),
                    Some(v) => {
                        let chars: Vec<char> = v.as_text().chars().collect();
                        let start_1 =
                            evaluated.get(1).and_then(SqlValue::as_f64).unwrap_or(1.0) as isize;
                        let idx = if start_1 > 0 {
                            (start_1 as usize).saturating_sub(1).min(chars.len())
                        } else if start_1 < 0 {
                            chars.len().saturating_sub((-start_1) as usize)
                        } else {
                            0
                        };
                        let take = evaluated
                            .get(2)
                            .and_then(SqlValue::as_f64)
                            .map_or(chars.len(), |n| n.max(0.0) as usize);
                        let s: String = chars.into_iter().skip(idx).take(take).collect();
                        Ok(SqlValue::Text(s))
                    }
                },
                "TYPEOF" => Ok(SqlValue::Text(
                    match evaluated.first().unwrap_or(&SqlValue::Null) {
                        SqlValue::Null => "null",
                        SqlValue::Int(_) | SqlValue::Bool(_) => "integer",
                        SqlValue::Real(_) => "real",
                        SqlValue::Text(_) => "text",
                        SqlValue::Blob(_) => "blob",
                    }
                    .to_owned(),
                )),
                other => Err(AirSqlError::new(
                    "execute",
                    format!("Unsupported SQL function '{other}'."),
                    3115,
                )),
            }
        }
    }
}

fn col_uses_nocase(expr: &SqlExpr, cols: &[EvalColMeta]) -> bool {
    if let SqlExpr::Col { table, name } = expr {
        cols.iter().any(|c| {
            let t_ok = table.as_ref().is_none_or(|t| {
                c.table_alias.eq_ignore_ascii_case(t) || c.table_name.eq_ignore_ascii_case(t)
            });
            t_ok && c.col_name.eq_ignore_ascii_case(name) && c.nocase
        })
    } else {
        false
    }
}

pub struct AirRuntime {
    pub app_id: String,
    pub descriptor_xml: String,
    pub fs: AirFileSystem,
    pub els: AirEncryptedStore,
    pub html: AirHtmlEngine,
    pub net: AirNetworkKernel,
    pub xml_sig: AirXmlSigEngine,
    conns: BTreeMap<u32, SqlConnState>,
    next_conn_id: u32,
    enc_nonce_counter: u64,
}

impl AirRuntime {
    pub fn new(
        project_name: &str,
        width: u32,
        height: u32,
        resources: &[AirResourceInit],
        now_ms: f64,
    ) -> Result<Self, AirError> {
        Ok(Self {
            app_id: synthesize_app_id(project_name),
            descriptor_xml: synthesize_app_descriptor_xml(project_name, width, height),
            fs: AirFileSystem::new(resources, now_ms)?,
            els: AirEncryptedStore::default(),
            html: AirHtmlEngine::default(),
            net: AirNetworkKernel::default(),
            xml_sig: AirXmlSigEngine::default(),
            conns: BTreeMap::new(),
            next_conn_id: 1,
            enc_nonce_counter: 1,
        })
    }

    fn encode_db_payload(
        &mut self,
        db: &SqlDatabase,
        encryption_key: Option<&[u8; 16]>,
        operation: &str,
    ) -> Result<Vec<u8>, AirSqlError> {
        let json_bytes =
            serde_json::to_vec(db).map_err(|e| AirSqlError::new(operation, e.to_string(), 3125))?;
        if let Some(key) = encryption_key {
            let seed = self.enc_nonce_counter;
            self.enc_nonce_counter = self.enc_nonce_counter.wrapping_add(1);
            Ok(encrypt_sql_payload(key, seed, &json_bytes))
        } else {
            let mut payload = SQL_VFS_MAGIC.to_vec();
            payload.extend(json_bytes);
            Ok(payload)
        }
    }

    fn load_or_create_db(
        &mut self,
        reference_path: Option<&str>,
        mode: &str,
        encryption_key: Option<[u8; 16]>,
        operation: &str,
        now_ms: f64,
    ) -> Result<AttachedDb, AirSqlError> {
        let mode_lc = mode.to_ascii_lowercase();
        if !matches!(mode_lc.as_str(), "create" | "read" | "update") {
            return Err(AirSqlError::new(
                operation,
                format!("Invalid SQLMode '{mode}'."),
                3112,
            ));
        }
        let read_only = mode_lc == "read";
        let Some(raw_path) = reference_path.filter(|s| !s.is_empty() && *s != ":memory:") else {
            return Ok(AttachedDb {
                vfs_path: None,
                read_only,
                encryption_key,
                db: SqlDatabase::default(),
            });
        };
        let vfs_path = normalize_air_path(raw_path)
            .map_err(|e| AirSqlError::new(operation, e.to_string(), 3125))?;
        let info = self
            .fs
            .info(&vfs_path)
            .map_err(|e| AirSqlError::new(operation, e.to_string(), 3125))?;
        if info.exists {
            if info.is_directory {
                return Err(AirSqlError::new(
                    operation,
                    format!("Database path '{vfs_path}' is a directory."),
                    3125,
                ));
            }
            if !read_only && !info.writable {
                return Err(AirSqlError::new(
                    operation,
                    format!("Database '{vfs_path}' is read-only; open with SQLMode.READ."),
                    3132,
                ));
            }
            let bytes = self
                .fs
                .read_bytes(&vfs_path)
                .map_err(|e| AirSqlError::new(operation, e.to_string(), 3125))?;
            let db = if bytes.is_empty() {
                let empty_db = SqlDatabase::default();
                if !read_only {
                    let payload =
                        self.encode_db_payload(&empty_db, encryption_key.as_ref(), operation)?;
                    self.fs
                        .write_bytes(&vfs_path, &payload, now_ms)
                        .map_err(|e| AirSqlError::new(operation, e.to_string(), 3125))?;
                }
                empty_db
            } else if let Some(enc_body) = bytes.strip_prefix(SQL_ENC_VFS_MAGIC) {
                let Some(key) = encryption_key.as_ref() else {
                    return Err(AirSqlError::new(
                        operation,
                        "File is encrypted or is not a database.",
                        3138,
                    ));
                };
                let json_bytes = decrypt_sql_payload(key, enc_body, operation)?;
                serde_json::from_slice::<SqlDatabase>(&json_bytes).map_err(|_| {
                    AirSqlError::new(
                        operation,
                        "File is not a valid NeoFlash AIR SQL database.",
                        3138,
                    )
                })?
            } else if let Some(json_bytes) = bytes.strip_prefix(SQL_VFS_MAGIC) {
                if encryption_key.is_some() {
                    return Err(AirSqlError::new(
                        operation,
                        "Cannot open an unencrypted database with an encryption key.",
                        3138,
                    ));
                }
                serde_json::from_slice::<SqlDatabase>(json_bytes).map_err(|_| {
                    AirSqlError::new(
                        operation,
                        "File is not a valid NeoFlash AIR SQL database.",
                        3138,
                    )
                })?
            } else {
                return Err(AirSqlError::new(
                    operation,
                    "File is encrypted or is not a database.",
                    3138,
                ));
            };
            Ok(AttachedDb {
                vfs_path: Some(vfs_path),
                read_only,
                encryption_key,
                db,
            })
        } else {
            if mode_lc != "create" {
                return Err(AirSqlError::new(
                    operation,
                    format!("Database file '{vfs_path}' does not exist."),
                    3125,
                ));
            }
            let db = SqlDatabase::default();
            let payload = self.encode_db_payload(&db, encryption_key.as_ref(), operation)?;
            self.fs
                .write_bytes(&vfs_path, &payload, now_ms)
                .map_err(|e| AirSqlError::new(operation, e.to_string(), 3125))?;
            Ok(AttachedDb {
                vfs_path: Some(vfs_path),
                read_only: false,
                encryption_key,
                db,
            })
        }
    }

    fn persist_conn(
        &mut self,
        conn_id: u32,
        operation: &str,
        now_ms: f64,
    ) -> Result<(), AirSqlError> {
        let Some(conn) = self.conns.get(&conn_id) else {
            return Ok(());
        };
        if conn.in_transaction {
            return Ok(());
        }
        let targets: Vec<(String, SqlDatabase, Option<[u8; 16]>)> = conn
            .attached
            .values()
            .filter_map(|att| {
                if att.read_only {
                    return None;
                }
                let path = att.vfs_path.as_ref()?.clone();
                Some((path, att.db.clone(), att.encryption_key))
            })
            .collect();
        for (path, db, key) in targets {
            let payload = self.encode_db_payload(&db, key.as_ref(), operation)?;
            self.fs
                .write_bytes(&path, &payload, now_ms)
                .map_err(|e| AirSqlError::new(operation, e.to_string(), 3125))?;
        }
        Ok(())
    }

    fn conn(&self, conn_id: u32, operation: &str) -> Result<&SqlConnState, AirSqlError> {
        self.conns
            .get(&conn_id)
            .filter(|c| c.connected)
            .ok_or_else(|| AirSqlError::new(operation, "SQLConnection is not open.", 3104))
    }

    fn conn_mut(
        &mut self,
        conn_id: u32,
        operation: &str,
    ) -> Result<&mut SqlConnState, AirSqlError> {
        self.conns
            .get_mut(&conn_id)
            .filter(|c| c.connected)
            .ok_or_else(|| AirSqlError::new(operation, "SQLConnection is not open.", 3104))
    }

    pub fn sql_open(
        &mut self,
        reference_path: Option<&str>,
        open_mode: &str,
        auto_compact: bool,
        page_size: u32,
        now_ms: f64,
    ) -> Result<u32, AirSqlError> {
        self.sql_open_with_key(
            reference_path,
            open_mode,
            auto_compact,
            page_size,
            None,
            now_ms,
        )
    }

    pub fn sql_open_with_key(
        &mut self,
        reference_path: Option<&str>,
        open_mode: &str,
        auto_compact: bool,
        page_size: u32,
        encryption_key: Option<&[u8]>,
        now_ms: f64,
    ) -> Result<u32, AirSqlError> {
        if !matches!(page_size, 512 | 1024 | 2048 | 4096 | 8192 | 16384 | 32768) {
            return Err(AirSqlError::new(
                "open",
                format!(
                    "Invalid SQL pageSize {page_size}; must be a power of two from 512 to 32768."
                ),
                3113,
            ));
        }
        let parsed_key = parse_sql_encryption_key(encryption_key, "open")?;
        let main_db =
            self.load_or_create_db(reference_path, open_mode, parsed_key, "open", now_ms)?;
        let id = self.next_conn_id;
        self.next_conn_id += 1;
        let mut attached = BTreeMap::new();
        attached.insert("main".to_owned(), main_db);
        self.conns.insert(
            id,
            SqlConnState {
                connected: true,
                auto_compact,
                page_size,
                cache_size: 2000,
                column_name_style: "default".to_owned(),
                last_insert_rowid: 0,
                total_changes: 0,
                in_transaction: false,
                tx_base: None,
                savepoints: Vec::new(),
                sp_counter: 0,
                attached,
            },
        );
        Ok(id)
    }

    pub fn sql_reencrypt(
        &mut self,
        conn_id: u32,
        new_key_bytes: &[u8],
        now_ms: f64,
    ) -> Result<(), AirSqlError> {
        let Some(new_key) = parse_sql_encryption_key(Some(new_key_bytes), "reencrypt")? else {
            return Err(AirSqlError::new(
                "reencrypt",
                "newEncryptionKey must be a 16-byte ByteArray.",
                3143,
            ));
        };
        {
            let conn = self.conn_mut(conn_id, "reencrypt")?;
            if conn.in_transaction {
                return Err(AirSqlError::new(
                    "reencrypt",
                    "Cannot reencrypt database while a transaction is active.",
                    3115,
                ));
            }
            let main_att = conn.attached.get_mut("main").ok_or_else(|| {
                AirSqlError::new("reencrypt", "Main database is not attached.", 3104)
            })?;
            if main_att.read_only {
                return Err(AirSqlError::new(
                    "reencrypt",
                    "Cannot reencrypt a read-only database.",
                    3132,
                ));
            }
            if main_att.encryption_key.is_none() {
                return Err(AirSqlError::new(
                    "reencrypt",
                    "Cannot reencrypt a database that was not created with encryption.",
                    3143,
                ));
            }
            main_att.encryption_key = Some(new_key);
        }
        self.persist_conn(conn_id, "reencrypt", now_ms)
    }

    pub fn sql_close(&mut self, conn_id: u32) -> Result<(), AirSqlError> {
        let conn = self.conn_mut(conn_id, "close")?;
        conn.connected = false;
        conn.in_transaction = false;
        conn.tx_base = None;
        conn.savepoints.clear();
        self.conns.remove(&conn_id);
        Ok(())
    }

    pub fn sql_attach(
        &mut self,
        conn_id: u32,
        name: &str,
        reference_path: Option<&str>,
        now_ms: f64,
    ) -> Result<(), AirSqlError> {
        self.sql_attach_with_key(conn_id, name, reference_path, None, now_ms)
    }

    pub fn sql_attach_with_key(
        &mut self,
        conn_id: u32,
        name: &str,
        reference_path: Option<&str>,
        encryption_key: Option<&[u8]>,
        now_ms: f64,
    ) -> Result<(), AirSqlError> {
        let clean = name.trim();
        if clean.is_empty() {
            return Err(AirSqlError::new(
                "attach",
                "Database name must not be empty.",
                3115,
            ));
        }
        let key = clean.to_ascii_lowercase();
        if self.conn(conn_id, "attach")?.attached.contains_key(&key) {
            return Err(AirSqlError::new(
                "attach",
                format!("Database '{clean}' is already attached."),
                3115,
            ));
        }
        let parsed_key = parse_sql_encryption_key(encryption_key, "attach")?;
        let att = self.load_or_create_db(reference_path, "create", parsed_key, "attach", now_ms)?;
        self.conn_mut(conn_id, "attach")?.attached.insert(key, att);
        Ok(())
    }

    pub fn sql_detach(&mut self, conn_id: u32, name: &str) -> Result<(), AirSqlError> {
        let key = name.trim().to_ascii_lowercase();
        if key == "main" || key == "temp" {
            return Err(AirSqlError::new(
                "detach",
                format!("Cannot detach built-in database '{name}'."),
                3115,
            ));
        }
        let conn = self.conn_mut(conn_id, "detach")?;
        if conn.in_transaction {
            return Err(AirSqlError::new(
                "detach",
                "Cannot detach a database while a transaction is active.",
                3115,
            ));
        }
        if conn.attached.remove(&key).is_none() {
            return Err(AirSqlError::new(
                "detach",
                format!("No such attached database '{name}'."),
                3115,
            ));
        }
        Ok(())
    }

    pub fn sql_begin(&mut self, conn_id: u32, option: Option<&str>) -> Result<(), AirSqlError> {
        if let Some(opt) = option.filter(|s| !s.is_empty())
            && !matches!(
                opt.to_ascii_lowercase().as_str(),
                "deferred" | "immediate" | "exclusive"
            )
        {
            return Err(AirSqlError::new(
                "begin",
                format!("Invalid SQLTransactionLockType '{opt}'."),
                3115,
            ));
        }
        let conn = self.conn_mut(conn_id, "begin")?;
        if conn.in_transaction {
            return Err(AirSqlError::new(
                "begin",
                "Cannot start a transaction within an active transaction.",
                3106,
            ));
        }
        let snap: BTreeMap<String, SqlDatabase> = conn
            .attached
            .iter()
            .map(|(k, v)| (k.clone(), v.db.clone()))
            .collect();
        conn.in_transaction = true;
        conn.tx_base = Some(snap);
        conn.savepoints.clear();
        Ok(())
    }

    pub fn sql_commit(&mut self, conn_id: u32, now_ms: f64) -> Result<(), AirSqlError> {
        {
            let conn = self.conn_mut(conn_id, "commit")?;
            if !conn.in_transaction {
                return Err(AirSqlError::new(
                    "commit",
                    "Cannot commit when no transaction is active.",
                    3107,
                ));
            }
            conn.in_transaction = false;
            conn.tx_base = None;
            conn.savepoints.clear();
        }
        self.persist_conn(conn_id, "commit", now_ms)
    }

    pub fn sql_rollback(&mut self, conn_id: u32) -> Result<(), AirSqlError> {
        let conn = self.conn_mut(conn_id, "rollback")?;
        if !conn.in_transaction {
            return Err(AirSqlError::new(
                "rollback",
                "Cannot roll back when no transaction is active.",
                3107,
            ));
        }
        if let Some(base) = conn.tx_base.take() {
            for (k, db) in base {
                if let Some(att) = conn.attached.get_mut(&k) {
                    att.db = db;
                }
            }
        }
        conn.in_transaction = false;
        conn.savepoints.clear();
        Ok(())
    }

    pub fn sql_set_savepoint(
        &mut self,
        conn_id: u32,
        name: Option<&str>,
    ) -> Result<String, AirSqlError> {
        let conn = self.conn_mut(conn_id, "setSavepoint")?;
        let sp_name = match name.map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => s.to_owned(),
            None => {
                conn.sp_counter += 1;
                format!("nf_sp_{}", conn.sp_counter)
            }
        };
        let snap: BTreeMap<String, SqlDatabase> = conn
            .attached
            .iter()
            .map(|(k, v)| (k.clone(), v.db.clone()))
            .collect();
        if !conn.in_transaction {
            conn.in_transaction = true;
            conn.tx_base = Some(snap.clone());
        }
        conn.savepoints.push(SavepointSnap {
            name: sp_name.clone(),
            dbs: snap,
        });
        Ok(sp_name)
    }

    pub fn sql_release_savepoint(
        &mut self,
        conn_id: u32,
        name: Option<&str>,
        now_ms: f64,
    ) -> Result<(), AirSqlError> {
        {
            let conn = self.conn_mut(conn_id, "releaseSavepoint")?;
            let idx = match name.map(str::trim).filter(|s| !s.is_empty()) {
                Some(target) => conn
                    .savepoints
                    .iter()
                    .rposition(|s| s.name.eq_ignore_ascii_case(target)),
                None => conn.savepoints.len().checked_sub(1),
            }
            .ok_or_else(|| {
                AirSqlError::new(
                    "releaseSavepoint",
                    "No matching savepoint to release.",
                    3115,
                )
            })?;
            conn.savepoints.truncate(idx);
            if conn.savepoints.is_empty() && conn.tx_base.is_some() {
                // Savepoint-started implicit transaction remains active only if explicit begin ran;
                // SQLite RELEASE of outermost implicit savepoint commits.
            }
        }
        self.persist_conn(conn_id, "releaseSavepoint", now_ms)
    }

    pub fn sql_rollback_to_savepoint(
        &mut self,
        conn_id: u32,
        name: Option<&str>,
    ) -> Result<(), AirSqlError> {
        let conn = self.conn_mut(conn_id, "rollbackToSavepoint")?;
        let idx = match name.map(str::trim).filter(|s| !s.is_empty()) {
            Some(target) => conn
                .savepoints
                .iter()
                .rposition(|s| s.name.eq_ignore_ascii_case(target)),
            None => conn.savepoints.len().checked_sub(1),
        }
        .ok_or_else(|| {
            AirSqlError::new(
                "rollbackToSavepoint",
                "No matching savepoint to roll back to.",
                3115,
            )
        })?;
        let snap = conn.savepoints[idx].dbs.clone();
        conn.savepoints.truncate(idx + 1);
        for (k, db) in snap {
            if let Some(att) = conn.attached.get_mut(&k) {
                att.db = db;
            }
        }
        Ok(())
    }

    pub fn sql_compact(&mut self, conn_id: u32, now_ms: f64) -> Result<(), AirSqlError> {
        let conn = self.conn(conn_id, "compact")?;
        if conn.in_transaction {
            return Err(AirSqlError::new(
                "compact",
                "Cannot compact database while a transaction is active.",
                3115,
            ));
        }
        self.persist_conn(conn_id, "compact", now_ms)
    }

    pub fn sql_analyze(
        &self,
        conn_id: u32,
        _resource_name: Option<&str>,
    ) -> Result<(), AirSqlError> {
        let _ = self.conn(conn_id, "analyze")?;
        Ok(())
    }

    pub fn sql_deanalyze(&self, conn_id: u32) -> Result<(), AirSqlError> {
        let _ = self.conn(conn_id, "deanalyze")?;
        Ok(())
    }

    pub fn sql_state_json(&self, conn_id: u32) -> Result<String, AirSqlError> {
        let conn = self.conn(conn_id, "open")?;
        Ok(serde_json::json!({
            "connected": conn.connected,
            "inTransaction": conn.in_transaction,
            "lastInsertRowID": conn.last_insert_rowid,
            "totalChanges": conn.total_changes,
            "autoCompact": conn.auto_compact,
            "pageSize": conn.page_size,
            "cacheSize": conn.cache_size,
            "columnNameStyle": conn.column_name_style,
        })
        .to_string())
    }

    pub fn sql_set_config(
        &mut self,
        conn_id: u32,
        cache_size: Option<u32>,
        column_name_style: Option<&str>,
    ) -> Result<(), AirSqlError> {
        let conn = self.conn_mut(conn_id, "execute")?;
        if let Some(cs) = cache_size {
            conn.cache_size = cs.max(1);
        }
        if let Some(style) = column_name_style {
            if !matches!(style, "default" | "long" | "short") {
                return Err(AirSqlError::new(
                    "execute",
                    format!("Invalid SQLColumnNameStyle '{style}'."),
                    3115,
                ));
            }
            conn.column_name_style = style.to_owned();
        }
        Ok(())
    }

    pub fn sql_load_schema_json(
        &self,
        conn_id: u32,
        schema_type: Option<&str>,
        name: Option<&str>,
        database: &str,
        include_columns: bool,
    ) -> Result<String, AirSqlError> {
        let conn = self.conn(conn_id, "schema")?;
        let db_key = if database.trim().is_empty() {
            "main".to_owned()
        } else {
            database.trim().to_ascii_lowercase()
        };
        let att = conn.attached.get(&db_key).ok_or_else(|| {
            AirSqlError::new(
                "schema",
                format!("No such attached database '{database}'."),
                3115,
            )
        })?;
        let want_tables =
            schema_type.is_none_or(|t| matches!(t, "table" | "SQLTableSchema" | "all"));
        let want_views = schema_type.is_none_or(|t| matches!(t, "view" | "SQLViewSchema" | "all"));
        let want_indices =
            schema_type.is_none_or(|t| matches!(t, "index" | "SQLIndexSchema" | "all"));
        let want_triggers =
            schema_type.is_none_or(|t| matches!(t, "trigger" | "SQLTriggerSchema" | "all"));
        let name_filter = name.map(str::trim).filter(|s| !s.is_empty());

        let mut tables = Vec::new();
        if want_tables {
            for tbl in att.db.tables.values() {
                if name_filter.is_some_and(|f| !tbl.name.eq_ignore_ascii_case(f)) {
                    continue;
                }
                let cols: Vec<serde_json::Value> = if include_columns {
                    tbl.columns
                        .iter()
                        .map(|c| {
                            serde_json::json!({
                                "name": c.name,
                                "dataType": c.data_type,
                                "allowNull": c.allow_null,
                                "primaryKey": c.primary_key,
                                "autoIncrement": c.auto_increment,
                                "defaultCollationType": c.default_collation_type,
                            })
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                tables.push(serde_json::json!({
                    "database": db_key,
                    "name": tbl.name,
                    "sql": tbl.sql,
                    "columns": cols,
                }));
            }
        }
        let mut views = Vec::new();
        if want_views {
            for v in att.db.views.values() {
                if name_filter.is_some_and(|f| !v.name.eq_ignore_ascii_case(f)) {
                    continue;
                }
                views.push(serde_json::json!({
                    "database": db_key,
                    "name": v.name,
                    "sql": v.sql,
                }));
            }
        }
        let mut indices = Vec::new();
        if want_indices {
            for idx in att.db.indices.values() {
                if name_filter.is_some_and(|f| !idx.name.eq_ignore_ascii_case(f)) {
                    continue;
                }
                indices.push(serde_json::json!({
                    "database": db_key,
                    "name": idx.name,
                    "sql": idx.sql,
                    "table": idx.table,
                }));
            }
        }
        let mut triggers = Vec::new();
        if want_triggers {
            for tr in att.db.triggers.values() {
                if name_filter.is_some_and(|f| !tr.name.eq_ignore_ascii_case(f)) {
                    continue;
                }
                triggers.push(serde_json::json!({
                    "database": db_key,
                    "name": tr.name,
                    "sql": tr.sql,
                    "table": tr.table,
                }));
            }
        }
        if name_filter.is_some()
            && tables.is_empty()
            && views.is_empty()
            && indices.is_empty()
            && triggers.is_empty()
        {
            return Err(AirSqlError::new(
                "schema",
                "No schema objects matched the requested filter.",
                3115,
            ));
        }
        Ok(serde_json::json!({
            "tables": tables,
            "views": views,
            "indices": indices,
            "triggers": triggers,
        })
        .to_string())
    }

    pub fn sql_execute_json(
        &mut self,
        conn_id: u32,
        sql_text: &str,
        params_json: &str,
        now_ms: f64,
    ) -> Result<String, AirSqlError> {
        let params: BTreeMap<String, SqlValue> = if params_json.trim().is_empty() {
            BTreeMap::new()
        } else {
            serde_json::from_str(params_json).map_err(|e| {
                AirSqlError::new("execute", format!("Invalid SQL parameters JSON: {e}"), 3115)
            })?
        };
        let toks = tokenize_sql(sql_text)?;
        let mut p = SqlParser::new(&toks);
        while p.eat(&Tok::Semi) {}
        if p.at_end() {
            return Err(AirSqlError::new(
                "execute",
                "SQLStatement.text must not be empty.",
                3115,
            ));
        }
        let head = match p.peek() {
            Some(Tok::Word(w)) => w.to_ascii_uppercase(),
            _ => {
                return Err(AirSqlError::new(
                    "execute",
                    "SQL statement must begin with a command keyword.",
                    3115,
                ));
            }
        };

        let mut columns: Vec<String> = Vec::new();
        let mut rows_out: Vec<Vec<SqlValue>> = Vec::new();
        let mut rows_affected: usize = 0;
        let mut update_events: Vec<SqlUpdateRecord> = Vec::new();
        let mut mutated = false;

        match head.as_str() {
            "BEGIN" => {
                p.pos += 1;
                let lock = if p.eat_kw("DEFERRED") {
                    Some("deferred")
                } else if p.eat_kw("IMMEDIATE") {
                    Some("immediate")
                } else if p.eat_kw("EXCLUSIVE") {
                    Some("exclusive")
                } else {
                    None
                };
                p.eat_kw("TRANSACTION");
                self.sql_begin(conn_id, lock)?;
            }
            "COMMIT" | "END" => {
                p.pos += 1;
                p.eat_kw("TRANSACTION");
                self.sql_commit(conn_id, now_ms)?;
            }
            "ROLLBACK" => {
                p.pos += 1;
                p.eat_kw("TRANSACTION");
                if p.eat_kw("TO") {
                    p.eat_kw("SAVEPOINT");
                    let sp = p.ident()?;
                    self.sql_rollback_to_savepoint(conn_id, Some(&sp))?;
                } else {
                    self.sql_rollback(conn_id)?;
                }
            }
            "SAVEPOINT" => {
                p.pos += 1;
                let sp = p.ident()?;
                self.sql_set_savepoint(conn_id, Some(&sp))?;
            }
            "RELEASE" => {
                p.pos += 1;
                p.eat_kw("SAVEPOINT");
                let sp = p.ident()?;
                self.sql_release_savepoint(conn_id, Some(&sp), now_ms)?;
            }
            "CREATE" => {
                p.pos += 1;
                let unique = p.eat_kw("UNIQUE");
                p.eat_kw("TEMP");
                p.eat_kw("TEMPORARY");
                if p.eat_kw("TABLE") {
                    let if_not_exists = if p.eat_kw("IF") {
                        p.expect_kw("NOT")?;
                        p.expect_kw("EXISTS")?;
                        true
                    } else {
                        false
                    };
                    let (db_opt, tbl_name) = p.qualified_name()?;
                    p.expect(&Tok::LParen)?;
                    let mut col_defs = Vec::new();
                    let mut table_pk_cols = Vec::new();
                    while !p.eat(&Tok::RParen) {
                        if p.eat_kw("CONSTRAINT") {
                            let _ = p.ident()?;
                        }
                        if p.eat_kw("PRIMARY") {
                            p.expect_kw("KEY")?;
                            p.expect(&Tok::LParen)?;
                            loop {
                                table_pk_cols.push(p.ident()?);
                                if !p.eat(&Tok::Comma) {
                                    break;
                                }
                            }
                            p.expect(&Tok::RParen)?;
                        } else if p.eat_kw("UNIQUE") || p.eat_kw("CHECK") || p.eat_kw("FOREIGN") {
                            let mut depth = 0usize;
                            while p.pos < p.toks.len() {
                                if p.eat(&Tok::LParen) {
                                    depth += 1;
                                } else if p.peek() == Some(&Tok::RParen) {
                                    if depth == 0 {
                                        break;
                                    }
                                    p.pos += 1;
                                    depth -= 1;
                                } else if depth == 0 && p.peek() == Some(&Tok::Comma) {
                                    break;
                                } else {
                                    p.pos += 1;
                                }
                            }
                        } else {
                            col_defs.push(p.parse_column_def()?);
                        }
                        if !p.eat(&Tok::Comma) {
                            p.expect(&Tok::RParen)?;
                            break;
                        }
                    }
                    for pk in table_pk_cols {
                        if let Some(c) = col_defs
                            .iter_mut()
                            .find(|c| c.name.eq_ignore_ascii_case(&pk))
                        {
                            c.primary_key = true;
                        }
                    }
                    if col_defs.is_empty() {
                        return Err(AirSqlError::new(
                            "execute",
                            "CREATE TABLE requires at least one column.",
                            3115,
                        ));
                    }
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let key = tbl_name.to_ascii_lowercase();
                    if att.db.tables.contains_key(&key) {
                        if !if_not_exists {
                            return Err(AirSqlError::new(
                                "execute",
                                format!("Table '{tbl_name}' already exists."),
                                3115,
                            ));
                        }
                    } else {
                        if att.db.tables.len() >= MAX_AIR_SQL_TABLES {
                            return Err(AirSqlError::new(
                                "execute",
                                "SQL database table count limit (256) exceeded.",
                                3115,
                            ));
                        }
                        att.db.tables.insert(
                            key,
                            SqlTable {
                                name: tbl_name,
                                sql: sql_text.trim().trim_end_matches(';').to_owned(),
                                columns: col_defs,
                                rows: Vec::new(),
                                next_rowid: 1,
                            },
                        );
                        mutated = true;
                    }
                } else if p.eat_kw("INDEX") {
                    let if_not_exists = if p.eat_kw("IF") {
                        p.expect_kw("NOT")?;
                        p.expect_kw("EXISTS")?;
                        true
                    } else {
                        false
                    };
                    let (db_opt, idx_name) = p.qualified_name()?;
                    p.expect_kw("ON")?;
                    let tbl_name = p.ident()?;
                    p.expect(&Tok::LParen)?;
                    let mut idx_cols = Vec::new();
                    loop {
                        idx_cols.push(p.ident()?);
                        p.eat_kw("COLLATE");
                        if matches!(p.peek(), Some(Tok::Word(w)) if w.eq_ignore_ascii_case("NOCASE") || w.eq_ignore_ascii_case("BINARY"))
                        {
                            p.pos += 1;
                        }
                        p.eat_kw("ASC");
                        p.eat_kw("DESC");
                        if !p.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    p.expect(&Tok::RParen)?;
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let tbl_key = tbl_name.to_ascii_lowercase();
                    let actual_tbl = att
                        .db
                        .tables
                        .get(&tbl_key)
                        .map(|t| t.name.clone())
                        .ok_or_else(|| {
                            AirSqlError::new(
                                "execute",
                                format!("No such table '{tbl_name}'."),
                                3115,
                            )
                        })?;
                    let key = idx_name.to_ascii_lowercase();
                    match att.db.indices.entry(key) {
                        std::collections::btree_map::Entry::Occupied(_) => {
                            if !if_not_exists {
                                return Err(AirSqlError::new(
                                    "execute",
                                    format!("Index '{idx_name}' already exists."),
                                    3115,
                                ));
                            }
                        }
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(SqlIndexMeta {
                                name: idx_name,
                                table: actual_tbl,
                                sql: sql_text.trim().trim_end_matches(';').to_owned(),
                                unique,
                                columns: idx_cols,
                            });
                            mutated = true;
                        }
                    }
                } else if p.eat_kw("VIEW") {
                    let if_not_exists = if p.eat_kw("IF") {
                        p.expect_kw("NOT")?;
                        p.expect_kw("EXISTS")?;
                        true
                    } else {
                        false
                    };
                    let (db_opt, view_name) = p.qualified_name()?;
                    p.expect_kw("AS")?;
                    let upper_src = sql_text.to_ascii_uppercase();
                    let select_sql = if let Some(as_idx) = upper_src.find(" AS ") {
                        sql_text[as_idx + 4..]
                            .trim()
                            .trim_end_matches(';')
                            .to_owned()
                    } else {
                        "SELECT 1".to_owned()
                    };
                    let _ = p.parse_select()?;
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let key = view_name.to_ascii_lowercase();
                    match att.db.views.entry(key) {
                        std::collections::btree_map::Entry::Occupied(_) => {
                            if !if_not_exists {
                                return Err(AirSqlError::new(
                                    "execute",
                                    format!("View '{view_name}' already exists."),
                                    3115,
                                ));
                            }
                        }
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(SqlViewMeta {
                                name: view_name,
                                sql: sql_text.trim().trim_end_matches(';').to_owned(),
                                select_sql,
                            });
                            mutated = true;
                        }
                    }
                } else if p.eat_kw("TRIGGER") {
                    let if_not_exists = if p.eat_kw("IF") {
                        p.expect_kw("NOT")?;
                        p.expect_kw("EXISTS")?;
                        true
                    } else {
                        false
                    };
                    let (db_opt, trig_name) = p.qualified_name()?;
                    while !p.at_end() && !p.eat_kw("ON") {
                        p.pos += 1;
                    }
                    let tbl_name = p.ident().unwrap_or_default();
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let key = trig_name.to_ascii_lowercase();
                    match att.db.triggers.entry(key) {
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(SqlTriggerMeta {
                                name: trig_name,
                                table: tbl_name,
                                sql: sql_text.trim().to_owned(),
                            });
                            mutated = true;
                        }
                        std::collections::btree_map::Entry::Occupied(_) if !if_not_exists => {
                            return Err(AirSqlError::new(
                                "execute",
                                "Trigger already exists.",
                                3115,
                            ));
                        }
                        std::collections::btree_map::Entry::Occupied(_) => {}
                    }
                } else {
                    return Err(AirSqlError::new(
                        "execute",
                        "Unsupported CREATE target; expected TABLE, INDEX, VIEW or TRIGGER.",
                        3115,
                    ));
                }
            }
            "DROP" => {
                p.pos += 1;
                let kind = p.ident()?.to_ascii_uppercase();
                let if_exists = if p.eat_kw("IF") {
                    p.expect_kw("EXISTS")?;
                    true
                } else {
                    false
                };
                let (db_opt, obj_name) = p.qualified_name()?;
                let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                let key = obj_name.to_ascii_lowercase();
                let removed = match kind.as_str() {
                    "TABLE" => {
                        let existed = att.db.tables.remove(&key);
                        if let Some(t) = &existed {
                            att.db
                                .indices
                                .retain(|_, idx| !idx.table.eq_ignore_ascii_case(&t.name));
                            att.db
                                .triggers
                                .retain(|_, tr| !tr.table.eq_ignore_ascii_case(&t.name));
                        }
                        existed.is_some()
                    }
                    "INDEX" => att.db.indices.remove(&key).is_some(),
                    "VIEW" => att.db.views.remove(&key).is_some(),
                    "TRIGGER" => att.db.triggers.remove(&key).is_some(),
                    _ => {
                        return Err(AirSqlError::new(
                            "execute",
                            format!("Unsupported DROP {kind}."),
                            3115,
                        ));
                    }
                };
                if !removed && !if_exists {
                    return Err(AirSqlError::new(
                        "execute",
                        format!("No such {} '{obj_name}'.", kind.to_ascii_lowercase()),
                        3115,
                    ));
                }
                mutated = removed;
            }
            "ALTER" => {
                p.pos += 1;
                p.expect_kw("TABLE")?;
                let (db_opt, tbl_name) = p.qualified_name()?;
                if p.eat_kw("RENAME") {
                    p.expect_kw("TO")?;
                    let new_name = p.ident()?;
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let old_key = tbl_name.to_ascii_lowercase();
                    let new_key = new_name.to_ascii_lowercase();
                    if att.db.tables.contains_key(&new_key) {
                        return Err(AirSqlError::new(
                            "execute",
                            format!("Table '{new_name}' already exists."),
                            3115,
                        ));
                    }
                    let mut tbl = att.db.tables.remove(&old_key).ok_or_else(|| {
                        AirSqlError::new("execute", format!("No such table '{tbl_name}'."), 3115)
                    })?;
                    tbl.name = new_name;
                    att.db.tables.insert(new_key, tbl);
                    mutated = true;
                } else if p.eat_kw("ADD") {
                    p.eat_kw("COLUMN");
                    let col = p.parse_column_def()?;
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let tbl = att
                        .db
                        .tables
                        .get_mut(&tbl_name.to_ascii_lowercase())
                        .ok_or_else(|| {
                            AirSqlError::new(
                                "execute",
                                format!("No such table '{tbl_name}'."),
                                3115,
                            )
                        })?;
                    if tbl
                        .columns
                        .iter()
                        .any(|c| c.name.eq_ignore_ascii_case(&col.name))
                    {
                        return Err(AirSqlError::new(
                            "execute",
                            format!("Duplicate column name '{}'.", col.name),
                            3115,
                        ));
                    }
                    let def_val = col.default_value.clone().unwrap_or(SqlValue::Null);
                    for row in &mut tbl.rows {
                        row.values.push(def_val.clone());
                    }
                    tbl.columns.push(col);
                    mutated = true;
                } else {
                    return Err(AirSqlError::new(
                        "execute",
                        "Unsupported ALTER TABLE action.",
                        3115,
                    ));
                }
            }
            "INSERT" | "REPLACE" => {
                let mut conflict = if head == "REPLACE" {
                    "replace"
                } else {
                    "abort"
                };
                p.pos += 1;
                if head == "INSERT" && p.eat_kw("OR") {
                    if p.eat_kw("REPLACE") {
                        conflict = "replace";
                    } else if p.eat_kw("IGNORE") {
                        conflict = "ignore";
                    } else {
                        let _ = p.ident()?;
                    }
                }
                p.expect_kw("INTO")?;
                let (db_opt, tbl_name) = p.qualified_name()?;
                let mut target_cols = Vec::new();
                if p.eat(&Tok::LParen) {
                    loop {
                        target_cols.push(p.ident()?);
                        if !p.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    p.expect(&Tok::RParen)?;
                }
                let mut tuples: Vec<Vec<SqlExpr>> = Vec::new();
                if p.eat_kw("DEFAULT") {
                    p.expect_kw("VALUES")?;
                    tuples.push(Vec::new());
                } else {
                    p.expect_kw("VALUES")?;
                    loop {
                        p.expect(&Tok::LParen)?;
                        let mut tuple = Vec::new();
                        if !p.eat(&Tok::RParen) {
                            loop {
                                tuple.push(p.expr()?);
                                if !p.eat(&Tok::Comma) {
                                    break;
                                }
                            }
                            p.expect(&Tok::RParen)?;
                        }
                        tuples.push(tuple);
                        if !p.eat(&Tok::Comma) {
                            break;
                        }
                    }
                }

                let mut last_rowid = self.conn(conn_id, "execute")?.last_insert_rowid;
                {
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let tbl = att
                        .db
                        .tables
                        .get_mut(&tbl_name.to_ascii_lowercase())
                        .ok_or_else(|| {
                            AirSqlError::new(
                                "execute",
                                format!("No such table '{tbl_name}'."),
                                3115,
                            )
                        })?;
                    let col_indices: Vec<usize> = if target_cols.is_empty() {
                        (0..tbl.columns.len()).collect()
                    } else {
                        let mut idxs = Vec::with_capacity(target_cols.len());
                        for tc in &target_cols {
                            let idx = tbl
                                .columns
                                .iter()
                                .position(|c| c.name.eq_ignore_ascii_case(tc))
                                .ok_or_else(|| {
                                    AirSqlError::new(
                                        "execute",
                                        format!("Table '{}' has no column named '{tc}'.", tbl.name),
                                        3115,
                                    )
                                })?;
                            idxs.push(idx);
                        }
                        idxs
                    };

                    for tuple in tuples {
                        if !tuple.is_empty() && tuple.len() != col_indices.len() {
                            return Err(AirSqlError::new(
                                "execute",
                                format!(
                                    "{} values for {} columns.",
                                    tuple.len(),
                                    col_indices.len()
                                ),
                                3115,
                            ));
                        }
                        if tbl.rows.len() >= MAX_AIR_SQL_ROWS {
                            return Err(AirSqlError::new(
                                "execute",
                                "SQL table row limit (32768) exceeded.",
                                3115,
                            ));
                        }
                        let mut row_vals: Vec<SqlValue> = tbl
                            .columns
                            .iter()
                            .map(|c| c.default_value.clone().unwrap_or(SqlValue::Null))
                            .collect();
                        for (pos, expr) in tuple.iter().enumerate() {
                            let v = eval_sql_expr(expr, &[], None, None, &params, None)?;
                            let cidx = col_indices[pos];
                            row_vals[cidx] = v.coerce_affinity(&tbl.columns[cidx].data_type);
                        }
                        // Integer PK / rowid assignment
                        let mut assigned_rowid = tbl.next_rowid;
                        for (cidx, col) in tbl.columns.iter().enumerate() {
                            if col.primary_key && col.auto_increment {
                                match &row_vals[cidx] {
                                    SqlValue::Null => {
                                        row_vals[cidx] = SqlValue::Int(assigned_rowid);
                                    }
                                    SqlValue::Int(n) => {
                                        assigned_rowid = *n;
                                    }
                                    _ => {}
                                }
                            }
                            if !col.allow_null && row_vals[cidx].is_null() && !col.auto_increment {
                                return Err(AirSqlError::new(
                                    "execute",
                                    format!(
                                        "NOT NULL constraint failed: {}.{}",
                                        tbl.name, col.name
                                    ),
                                    3131,
                                ));
                            }
                        }
                        // Check PK / UNIQUE constraint conflicts
                        let mut conflict_rows = Vec::new();
                        for (r_idx, existing) in tbl.rows.iter().enumerate() {
                            let mut clashes = existing.rowid == assigned_rowid;
                            if !clashes {
                                for (cidx, col) in tbl.columns.iter().enumerate() {
                                    if (col.primary_key || col.unique) && !row_vals[cidx].is_null()
                                    {
                                        let nocase = col.default_collation_type == "NO_CASE";
                                        if existing.values[cidx].cmp_sql(&row_vals[cidx], nocase)
                                            == std::cmp::Ordering::Equal
                                        {
                                            clashes = true;
                                            break;
                                        }
                                    }
                                }
                            }
                            if clashes {
                                conflict_rows.push(r_idx);
                            }
                        }
                        if !conflict_rows.is_empty() {
                            match conflict {
                                "ignore" => continue,
                                "replace" => {
                                    for r_idx in conflict_rows.into_iter().rev() {
                                        tbl.rows.remove(r_idx);
                                    }
                                }
                                _ => {
                                    return Err(AirSqlError::new(
                                        "execute",
                                        format!(
                                            "UNIQUE constraint failed on table '{}'.",
                                            tbl.name
                                        ),
                                        3131,
                                    ));
                                }
                            }
                        }
                        if assigned_rowid >= tbl.next_rowid {
                            tbl.next_rowid = assigned_rowid + 1;
                        }
                        tbl.rows.push(SqlRow {
                            rowid: assigned_rowid,
                            values: row_vals,
                        });
                        last_rowid = assigned_rowid;
                        rows_affected += 1;
                        update_events.push(SqlUpdateRecord {
                            action: "insert".to_owned(),
                            table: tbl.name.clone(),
                            row_id: assigned_rowid,
                        });
                    }
                }
                if rows_affected > 0 {
                    let conn = self.conn_mut(conn_id, "execute")?;
                    conn.last_insert_rowid = last_rowid;
                    conn.total_changes += rows_affected as u64;
                    mutated = true;
                }
            }
            "UPDATE" => {
                p.pos += 1;
                let (db_opt, tbl_name) = p.qualified_name()?;
                p.expect_kw("SET")?;
                let mut assignments: Vec<(String, SqlExpr)> = Vec::new();
                loop {
                    let col = p.ident()?;
                    p.expect(&Tok::Eq)?;
                    let val_expr = p.expr()?;
                    assignments.push((col, val_expr));
                    if !p.eat(&Tok::Comma) {
                        break;
                    }
                }
                let where_expr = if p.eat_kw("WHERE") {
                    Some(p.expr()?)
                } else {
                    None
                };
                {
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let tbl = att
                        .db
                        .tables
                        .get_mut(&tbl_name.to_ascii_lowercase())
                        .ok_or_else(|| {
                            AirSqlError::new(
                                "execute",
                                format!("No such table '{tbl_name}'."),
                                3115,
                            )
                        })?;
                    let eval_cols: Vec<EvalColMeta> = tbl
                        .columns
                        .iter()
                        .map(|c| EvalColMeta {
                            table_name: tbl.name.clone(),
                            table_alias: tbl.name.clone(),
                            col_name: c.name.clone(),
                            nocase: c.default_collation_type == "NO_CASE",
                        })
                        .collect();
                    let mut target_idxs = Vec::with_capacity(assignments.len());
                    for (col_name, _) in &assignments {
                        let cidx = tbl
                            .columns
                            .iter()
                            .position(|c| c.name.eq_ignore_ascii_case(col_name))
                            .ok_or_else(|| {
                                AirSqlError::new(
                                    "execute",
                                    format!("No such column '{col_name}'."),
                                    3115,
                                )
                            })?;
                        target_idxs.push(cidx);
                    }
                    for row in &mut tbl.rows {
                        let erow = EvalRow {
                            rowid: Some(row.rowid),
                            values: row.values.clone(),
                        };
                        if let Some(w) = &where_expr
                            && !eval_sql_expr(w, &eval_cols, Some(&erow), None, &params, None)?
                                .truthy()
                        {
                            continue;
                        }
                        let mut next_vals = row.values.clone();
                        for (pos, (_, expr)) in assignments.iter().enumerate() {
                            let cidx = target_idxs[pos];
                            let v =
                                eval_sql_expr(expr, &eval_cols, Some(&erow), None, &params, None)?;
                            let coerced = v.coerce_affinity(&tbl.columns[cidx].data_type);
                            if !tbl.columns[cidx].allow_null && coerced.is_null() {
                                return Err(AirSqlError::new(
                                    "execute",
                                    format!(
                                        "NOT NULL constraint failed: {}.{}",
                                        tbl.name, tbl.columns[cidx].name
                                    ),
                                    3131,
                                ));
                            }
                            next_vals[cidx] = coerced;
                        }
                        row.values = next_vals;
                        rows_affected += 1;
                        update_events.push(SqlUpdateRecord {
                            action: "update".to_owned(),
                            table: tbl.name.clone(),
                            row_id: row.rowid,
                        });
                    }
                }
                if rows_affected > 0 {
                    self.conn_mut(conn_id, "execute")?.total_changes += rows_affected as u64;
                    mutated = true;
                }
            }
            "DELETE" => {
                p.pos += 1;
                p.expect_kw("FROM")?;
                let (db_opt, tbl_name) = p.qualified_name()?;
                let where_expr = if p.eat_kw("WHERE") {
                    Some(p.expr()?)
                } else {
                    None
                };
                {
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let tbl = att
                        .db
                        .tables
                        .get_mut(&tbl_name.to_ascii_lowercase())
                        .ok_or_else(|| {
                            AirSqlError::new(
                                "execute",
                                format!("No such table '{tbl_name}'."),
                                3115,
                            )
                        })?;
                    let eval_cols: Vec<EvalColMeta> = tbl
                        .columns
                        .iter()
                        .map(|c| EvalColMeta {
                            table_name: tbl.name.clone(),
                            table_alias: tbl.name.clone(),
                            col_name: c.name.clone(),
                            nocase: c.default_collation_type == "NO_CASE",
                        })
                        .collect();
                    let mut kept = Vec::with_capacity(tbl.rows.len());
                    for row in tbl.rows.drain(..) {
                        let erow = EvalRow {
                            rowid: Some(row.rowid),
                            values: row.values.clone(),
                        };
                        let should_del = match &where_expr {
                            Some(w) => {
                                eval_sql_expr(w, &eval_cols, Some(&erow), None, &params, None)?
                                    .truthy()
                            }
                            None => true,
                        };
                        if should_del {
                            rows_affected += 1;
                            update_events.push(SqlUpdateRecord {
                                action: "delete".to_owned(),
                                table: tbl.name.clone(),
                                row_id: row.rowid,
                            });
                        } else {
                            kept.push(row);
                        }
                    }
                    tbl.rows = kept;
                }
                if rows_affected > 0 {
                    self.conn_mut(conn_id, "execute")?.total_changes += rows_affected as u64;
                    mutated = true;
                }
            }
            "SELECT" => {
                let plan = p.parse_select()?;
                let (cols, rows) = self.eval_select_plan(conn_id, &plan, &params, 0)?;
                columns = cols;
                rows_out = rows;
            }
            other => {
                return Err(AirSqlError::new(
                    "execute",
                    format!("Unsupported SQL command '{other}'."),
                    3115,
                ));
            }
        }

        if mutated {
            self.persist_conn(conn_id, "execute", now_ms)?;
        }
        let last_rowid = self.conns.get(&conn_id).map_or(0, |c| c.last_insert_rowid);
        Ok(serde_json::json!({
            "columns": columns,
            "rows": rows_out,
            "rowsAffected": rows_affected,
            "lastInsertRowID": last_rowid,
            "updateEvents": update_events,
        })
        .to_string())
    }

    fn att_ref(&self, conn_id: u32, db_opt: Option<&str>) -> Result<&AttachedDb, AirSqlError> {
        let conn = self.conn(conn_id, "execute")?;
        let key = db_opt
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_else(|| "main".to_owned());
        conn.attached
            .get(&key)
            .ok_or_else(|| AirSqlError::new("execute", format!("No such database '{key}'."), 3115))
    }

    fn att_mut(
        &mut self,
        conn_id: u32,
        db_opt: Option<&str>,
        write: bool,
    ) -> Result<&mut AttachedDb, AirSqlError> {
        let conn = self.conn_mut(conn_id, "execute")?;
        let key = db_opt
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_else(|| "main".to_owned());
        let att = conn.attached.get_mut(&key).ok_or_else(|| {
            AirSqlError::new("execute", format!("No such database '{key}'."), 3115)
        })?;
        if write && att.read_only {
            return Err(AirSqlError::new(
                "execute",
                "Attempt to write a readonly database.",
                3132,
            ));
        }
        Ok(att)
    }

    fn load_table_or_view(
        &self,
        conn_id: u32,
        db_opt: Option<&str>,
        name: &str,
        alias_opt: Option<&str>,
        params: &BTreeMap<String, SqlValue>,
        depth: usize,
    ) -> Result<(Vec<EvalColMeta>, Vec<EvalRow>), AirSqlError> {
        if depth > 8 {
            return Err(AirSqlError::new(
                "execute",
                "Recursive SQL view depth exceeded.",
                3115,
            ));
        }
        let att = self.att_ref(conn_id, db_opt)?;
        let key = name.to_ascii_lowercase();
        if let Some(tbl) = att.db.tables.get(&key) {
            let alias = alias_opt.unwrap_or(&tbl.name).to_owned();
            let cols = tbl
                .columns
                .iter()
                .map(|c| EvalColMeta {
                    table_name: tbl.name.clone(),
                    table_alias: alias.clone(),
                    col_name: c.name.clone(),
                    nocase: c.default_collation_type == "NO_CASE",
                })
                .collect();
            let rows = tbl
                .rows
                .iter()
                .map(|r| EvalRow {
                    rowid: Some(r.rowid),
                    values: r.values.clone(),
                })
                .collect();
            return Ok((cols, rows));
        }
        if let Some(view) = att.db.views.get(&key) {
            let vtoks = tokenize_sql(&view.select_sql)?;
            let mut vp = SqlParser::new(&vtoks);
            let vplan = vp.parse_select()?;
            let (vcols, vrows) = self.eval_select_plan(conn_id, &vplan, params, depth + 1)?;
            let alias = alias_opt.unwrap_or(&view.name).to_owned();
            let cols = vcols
                .into_iter()
                .map(|cn| EvalColMeta {
                    table_name: view.name.clone(),
                    table_alias: alias.clone(),
                    col_name: cn,
                    nocase: false,
                })
                .collect();
            let rows = vrows
                .into_iter()
                .map(|vals| EvalRow {
                    rowid: None,
                    values: vals,
                })
                .collect();
            return Ok((cols, rows));
        }
        Err(AirSqlError::new(
            "execute",
            format!("No such table '{name}'."),
            3115,
        ))
    }

    fn eval_select_plan(
        &self,
        conn_id: u32,
        plan: &SelectPlan,
        params: &BTreeMap<String, SqlValue>,
        depth: usize,
    ) -> Result<(Vec<String>, Vec<Vec<SqlValue>>), AirSqlError> {
        let long_style = self.conn(conn_id, "execute")?.column_name_style == "long";
        let (mut working_cols, mut working_rows) = if let Some(from_tbl) = &plan.from_table {
            self.load_table_or_view(
                conn_id,
                plan.from_db.as_deref(),
                from_tbl,
                plan.from_alias.as_deref(),
                params,
                depth,
            )?
        } else {
            (
                Vec::new(),
                vec![EvalRow {
                    rowid: None,
                    values: Vec::new(),
                }],
            )
        };

        for join in &plan.joins {
            let (rcols, rrows) = self.load_table_or_view(
                conn_id,
                join.db.as_deref(),
                &join.table,
                join.alias.as_deref(),
                params,
                depth,
            )?;
            let r_width = rcols.len();
            let mut combined_cols = working_cols.clone();
            combined_cols.extend(rcols);
            let mut next_rows = Vec::new();
            for lrow in &working_rows {
                let mut matched_any = false;
                for rrow in &rrows {
                    let mut vals = lrow.values.clone();
                    vals.extend(rrow.values.clone());
                    let candidate = EvalRow {
                        rowid: lrow.rowid,
                        values: vals,
                    };
                    let ok = match &join.on {
                        Some(on_expr) => eval_sql_expr(
                            on_expr,
                            &combined_cols,
                            Some(&candidate),
                            None,
                            params,
                            None,
                        )?
                        .truthy(),
                        None => true,
                    };
                    if ok {
                        matched_any = true;
                        next_rows.push(candidate);
                    }
                }
                if !matched_any && join.left_outer {
                    let mut vals = lrow.values.clone();
                    vals.extend(std::iter::repeat_n(SqlValue::Null, r_width));
                    next_rows.push(EvalRow {
                        rowid: lrow.rowid,
                        values: vals,
                    });
                }
            }
            working_cols = combined_cols;
            working_rows = next_rows;
        }

        if let Some(w) = &plan.where_expr {
            let mut filtered = Vec::with_capacity(working_rows.len());
            for row in working_rows {
                if eval_sql_expr(w, &working_cols, Some(&row), None, params, None)?.truthy() {
                    filtered.push(row);
                }
            }
            working_rows = filtered;
        }

        // Expand projection items (`*`, `alias.*`, expressions)
        let mut proj_names: Vec<String> = Vec::new();
        let mut proj_exprs: Vec<SqlExpr> = Vec::new();
        for item in &plan.items {
            match item {
                SelectItem::All(tbl_filter) => {
                    for c in &working_cols {
                        if let Some(tf) = tbl_filter
                            && !c.table_alias.eq_ignore_ascii_case(tf)
                            && !c.table_name.eq_ignore_ascii_case(tf)
                        {
                            continue;
                        }
                        let out_name = if long_style {
                            format!("{}_{}", c.table_alias, c.col_name)
                        } else {
                            c.col_name.clone()
                        };
                        proj_names.push(out_name);
                        proj_exprs.push(SqlExpr::Col {
                            table: Some(c.table_alias.clone()),
                            name: c.col_name.clone(),
                        });
                    }
                }
                SelectItem::Expr { expr, alias } => {
                    let out_name = alias
                        .clone()
                        .unwrap_or_else(|| default_expr_column_name(expr, long_style));
                    proj_names.push(out_name);
                    proj_exprs.push(expr.clone());
                }
            }
        }

        let is_agg = !plan.group_by.is_empty()
            || plan.having.is_some()
            || proj_exprs.iter().any(expr_contains_aggregate);

        let mut projected_with_ctx: Vec<ProjectedRowCtx> = Vec::new();

        if is_agg {
            let groups: Vec<Vec<EvalRow>> = if plan.group_by.is_empty() {
                vec![working_rows]
            } else {
                let mut buckets: Vec<(Vec<SqlValue>, Vec<EvalRow>)> = Vec::new();
                for row in working_rows {
                    let mut key = Vec::with_capacity(plan.group_by.len());
                    for gexpr in &plan.group_by {
                        key.push(eval_sql_expr(
                            gexpr,
                            &working_cols,
                            Some(&row),
                            None,
                            params,
                            None,
                        )?);
                    }
                    if let Some((_, bucket)) = buckets.iter_mut().find(|(k, _)| {
                        k.len() == key.len()
                            && k.iter()
                                .zip(&key)
                                .all(|(a, b)| a.cmp_sql(b, false) == std::cmp::Ordering::Equal)
                    }) {
                        bucket.push(row);
                    } else {
                        buckets.push((key, vec![row]));
                    }
                }
                buckets.into_iter().map(|(_, b)| b).collect()
            };

            for grp in groups {
                let mut pvals = Vec::with_capacity(proj_exprs.len());
                for pexpr in &proj_exprs {
                    pvals.push(eval_sql_expr(
                        pexpr,
                        &working_cols,
                        grp.first(),
                        Some(&grp),
                        params,
                        None,
                    )?);
                }
                if let Some(h) = &plan.having {
                    let alias_pairs: Vec<(&str, SqlValue)> = proj_names
                        .iter()
                        .zip(&pvals)
                        .map(|(n, v)| (n.as_str(), v.clone()))
                        .collect();
                    if !eval_sql_expr(
                        h,
                        &working_cols,
                        grp.first(),
                        Some(&grp),
                        params,
                        Some(&alias_pairs),
                    )?
                    .truthy()
                    {
                        continue;
                    }
                }
                projected_with_ctx.push((pvals, None, Some(grp)));
            }
        } else {
            for row in working_rows {
                let mut pvals = Vec::with_capacity(proj_exprs.len());
                for pexpr in &proj_exprs {
                    pvals.push(eval_sql_expr(
                        pexpr,
                        &working_cols,
                        Some(&row),
                        None,
                        params,
                        None,
                    )?);
                }
                projected_with_ctx.push((pvals, Some(row), None));
            }
        }

        if plan.distinct {
            let mut unique_rows: Vec<ProjectedRowCtx> = Vec::new();
            for item in projected_with_ctx {
                if !unique_rows.iter().any(|(existing, _, _)| {
                    existing.len() == item.0.len()
                        && existing
                            .iter()
                            .zip(&item.0)
                            .all(|(a, b)| a.cmp_sql(b, false) == std::cmp::Ordering::Equal)
                }) {
                    unique_rows.push(item);
                }
            }
            projected_with_ctx = unique_rows;
        }

        if !plan.order_by.is_empty() {
            let mut keyed: Vec<OrderKeyedRow> = Vec::with_capacity(projected_with_ctx.len());
            for (pvals, srow, sgrp) in projected_with_ctx {
                let alias_pairs: Vec<(&str, SqlValue)> = proj_names
                    .iter()
                    .zip(&pvals)
                    .map(|(n, v)| (n.as_str(), v.clone()))
                    .collect();
                let mut keys = Vec::with_capacity(plan.order_by.len());
                for ob in &plan.order_by {
                    let kv = if let SqlExpr::Lit(SqlValue::Int(pos_1)) = &ob.expr {
                        let idx = (*pos_1 - 1) as usize;
                        pvals.get(idx).cloned().unwrap_or(SqlValue::Null)
                    } else {
                        eval_sql_expr(
                            &ob.expr,
                            &working_cols,
                            srow.as_ref()
                                .or_else(|| sgrp.as_ref().and_then(|g| g.first())),
                            sgrp.as_deref(),
                            params,
                            Some(&alias_pairs),
                        )?
                    };
                    keys.push((kv, ob.desc));
                }
                keyed.push((keys, pvals));
            }
            keyed.sort_by(|(ka, _), (kb, _)| {
                for ((va, desc), (vb, _)) in ka.iter().zip(kb.iter()) {
                    let ord = va.cmp_sql(vb, false);
                    if ord != std::cmp::Ordering::Equal {
                        return if *desc { ord.reverse() } else { ord };
                    }
                }
                std::cmp::Ordering::Equal
            });
            let mut all_rows: Vec<Vec<SqlValue>> = keyed.into_iter().map(|(_, p)| p).collect();
            if plan.offset > 0 {
                if plan.offset >= all_rows.len() {
                    all_rows.clear();
                } else {
                    all_rows.drain(0..plan.offset);
                }
            }
            if let Some(lim) = plan.limit {
                all_rows.truncate(lim);
            }
            return Ok((proj_names, all_rows));
        }

        let mut all_rows: Vec<Vec<SqlValue>> =
            projected_with_ctx.into_iter().map(|(p, _, _)| p).collect();
        if plan.offset > 0 {
            if plan.offset >= all_rows.len() {
                all_rows.clear();
            } else {
                all_rows.drain(0..plan.offset);
            }
        }
        if let Some(lim) = plan.limit {
            all_rows.truncate(lim);
        }
        Ok((proj_names, all_rows))
    }

    pub fn html_load_url(
        &mut self,
        session_id: u32,
        url: &str,
        is_post: bool,
        fallback_html: Option<&str>,
    ) -> Result<String, AirError> {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err(AirError::Argument("URL must not be empty.".into()));
        }
        let html_content = if trimmed.eq_ignore_ascii_case("about:blank") {
            String::new()
        } else if let Some(rest) = trimmed.strip_prefix("data:") {
            decode_data_url_to_string(rest)?
        } else if trimmed.starts_with("app:/")
            || trimmed.starts_with("app-storage:/")
            || trimmed.starts_with("file:")
            || trimmed.starts_with('/')
        {
            let vfs_path = normalize_air_path(trimmed)?;
            let bytes = self.fs.read_bytes(&vfs_path)?;
            String::from_utf8_lossy(&bytes).into_owned()
        } else if let Some(body) = fallback_html {
            body.to_owned()
        } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            let safe_title = xml_escape_text(trimmed);
            format!(
                "<!DOCTYPE html><html><head><title>{safe_title}</title></head><body><p>{safe_title}</p></body></html>"
            )
        } else {
            return Err(AirError::Io(format!(
                "Unsupported or unreachable URL '{trimmed}'."
            )));
        };
        self.html
            .load_document(session_id, &html_content, trimmed, trimmed, is_post, true)
    }
}

fn xml_escape_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn decode_data_url_to_string(rest: &str) -> Result<String, AirError> {
    let Some((meta, payload)) = rest.split_once(',') else {
        return Err(AirError::Argument("Malformed data: URL.".into()));
    };
    if meta.to_ascii_lowercase().ends_with(";base64") {
        let decoded = BASE64
            .decode(payload.trim())
            .map_err(|e| AirError::Argument(format!("Invalid base64 in data: URL: {e}")))?;
        Ok(String::from_utf8_lossy(&decoded).into_owned())
    } else {
        Ok(percent_decode_str(payload))
    }
}

fn percent_decode_str(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// AIR HTML Document & History Engine (`flash.html.*`, `flash.media.StageWebView`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AirHtmlHistoryEntry {
    pub url: String,
    pub original_url: String,
    pub title: String,
    pub is_post: bool,
    #[serde(skip)]
    html_source: String,
}

#[derive(Debug, Clone)]
struct AirHtmlDomElement {
    tag: String,
    id: String,
    text: String,
    href: String,
}

#[derive(Debug, Clone)]
struct AirHtmlSession {
    width: f64,
    height: f64,
    scroll_h: f64,
    scroll_v: f64,
    content_width: f64,
    content_height: f64,
    loaded: bool,
    location: String,
    title: String,
    html_source: String,
    plain_text: String,
    has_focusable_content: bool,
    links: Vec<String>,
    scripts: Vec<String>,
    elements: Vec<AirHtmlDomElement>,
    history: Vec<AirHtmlHistoryEntry>,
    history_position: usize,
}

#[derive(Debug, Default)]
pub struct AirHtmlEngine {
    sessions: BTreeMap<u32, AirHtmlSession>,
    next_id: u32,
}

struct ParsedHtmlDoc {
    title: String,
    plain_text: String,
    has_focusable: bool,
    links: Vec<String>,
    scripts: Vec<String>,
    elements: Vec<AirHtmlDomElement>,
    content_width: f64,
    content_height: f64,
}

fn unescape_html_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn extract_attr_value(tag_attrs: &str, attr_name: &str) -> Option<String> {
    let lower = tag_attrs.to_ascii_lowercase();
    let needle = format!("{}=", attr_name.to_ascii_lowercase());
    let mut search_from = 0usize;
    while let Some(rel) = lower[search_from..].find(&needle) {
        let pos = search_from + rel;
        let ok_prefix = pos == 0
            || tag_attrs.as_bytes()[pos - 1].is_ascii_whitespace()
            || tag_attrs.as_bytes()[pos - 1] == b'"'
            || tag_attrs.as_bytes()[pos - 1] == b'\'';
        if !ok_prefix {
            search_from = pos + needle.len();
            continue;
        }
        let after = tag_attrs[pos + needle.len()..].trim_start();
        if let Some(quoted) = after.strip_prefix('"') {
            let end = quoted.find('"').unwrap_or(quoted.len());
            return Some(unescape_html_entities(&quoted[..end]));
        } else if let Some(quoted) = after.strip_prefix('\'') {
            let end = quoted.find('\'').unwrap_or(quoted.len());
            return Some(unescape_html_entities(&quoted[..end]));
        } else {
            let end = after
                .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
                .unwrap_or(after.len());
            return Some(unescape_html_entities(&after[..end]));
        }
    }
    None
}

fn parse_css_px_dimension(style: &str, prop: &str) -> Option<f64> {
    for decl in style.split(';') {
        if let Some((k, v)) = decl.split_once(':')
            && k.trim().eq_ignore_ascii_case(prop)
        {
            let num_str = v.trim().trim_end_matches("px").trim();
            if let Ok(val) = num_str.parse::<f64>()
                && val > 0.0
            {
                return Some(val);
            }
        }
    }
    None
}

fn parse_html_document(html: &str, viewport_w: f64, viewport_h: f64) -> ParsedHtmlDoc {
    let mut title = String::new();
    let mut text_out = String::new();
    let mut has_focusable = false;
    let mut links = Vec::new();
    let mut scripts = Vec::new();
    let mut elements = Vec::new();
    let mut explicit_w: Option<f64> = None;
    let mut explicit_h: Option<f64> = None;

    let bytes = html.as_bytes();
    let mut i = 0usize;
    let mut in_title = false;
    let mut open_elem: Option<(String, String, String, usize)> = None;

    while i < bytes.len() {
        if bytes[i] == b'<' {
            if html[i..].starts_with("<!--") {
                if let Some(end_rel) = html[i + 4..].find("-->") {
                    i += 4 + end_rel + 3;
                } else {
                    break;
                }
                continue;
            }
            let Some(gt_rel) = html[i + 1..].find('>') else {
                break;
            };
            let raw_tag = html[i + 1..i + 1 + gt_rel].trim();
            i += gt_rel + 2;

            let is_closing = raw_tag.starts_with('/');
            let clean_tag = raw_tag.trim_start_matches('/').trim_end_matches('/').trim();
            let (tag_name_raw, attrs) = match clean_tag.find(char::is_whitespace) {
                Some(sp) => (&clean_tag[..sp], &clean_tag[sp..]),
                None => (clean_tag, ""),
            };
            let tag_lc = tag_name_raw.to_ascii_lowercase();

            if !is_closing && (tag_lc == "script" || tag_lc == "style") {
                let close_pat = format!("</{tag_lc}");
                let lower_rest = html[i..].to_ascii_lowercase();
                if let Some(end_rel) = lower_rest.find(&close_pat) {
                    let body = &html[i..i + end_rel];
                    if tag_lc == "script" {
                        scripts.push(body.to_owned());
                    }
                    let after_close = i + end_rel + close_pat.len();
                    if let Some(close_gt) = html[after_close..].find('>') {
                        i = after_close + close_gt + 1;
                    } else {
                        i = bytes.len();
                    }
                } else {
                    i = bytes.len();
                }
                continue;
            }

            if is_closing {
                if tag_lc == "title" {
                    in_title = false;
                }
                if let Some((open_tag, open_id, open_href, text_start)) = open_elem.take() {
                    let captured = text_out[text_start..].trim().to_owned();
                    elements.push(AirHtmlDomElement {
                        tag: open_tag,
                        id: open_id,
                        text: captured,
                        href: open_href,
                    });
                }
                if matches!(
                    tag_lc.as_str(),
                    "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "tr"
                ) && !text_out.ends_with('\n')
                {
                    text_out.push('\n');
                }
                continue;
            }

            if tag_lc == "title" {
                in_title = true;
                continue;
            }
            if tag_lc == "br" {
                text_out.push('\n');
            }
            if let Some(style_val) = extract_attr_value(attrs, "style") {
                if let Some(w) = parse_css_px_dimension(&style_val, "width") {
                    explicit_w = Some(explicit_w.map_or(w, |cur| cur.max(w)));
                }
                if let Some(h) = parse_css_px_dimension(&style_val, "height") {
                    explicit_h = Some(explicit_h.map_or(h, |cur| cur.max(h)));
                }
            }
            if matches!(tag_lc.as_str(), "input" | "button" | "textarea" | "select")
                || extract_attr_value(attrs, "tabindex").is_some()
            {
                has_focusable = true;
            }
            let href_val = extract_attr_value(attrs, "href").unwrap_or_default();
            if tag_lc == "a" && !href_val.is_empty() {
                has_focusable = true;
                links.push(href_val.clone());
            }
            if let Some(id_val) = extract_attr_value(attrs, "id")
                && !id_val.is_empty()
            {
                if raw_tag.ends_with('/')
                    || matches!(tag_lc.as_str(), "input" | "img" | "br" | "hr")
                {
                    let val = extract_attr_value(attrs, "value").unwrap_or_default();
                    elements.push(AirHtmlDomElement {
                        tag: tag_lc,
                        id: id_val,
                        text: val,
                        href: href_val,
                    });
                } else {
                    open_elem = Some((tag_lc, id_val, href_val, text_out.len()));
                }
            }
        } else {
            let next_lt = html[i..].find('<').map_or(bytes.len(), |r| i + r);
            let raw_text = unescape_html_entities(&html[i..next_lt]);
            if in_title {
                title.push_str(raw_text.trim());
            } else {
                let collapsed: Vec<&str> = raw_text.split_whitespace().collect();
                if !collapsed.is_empty() {
                    if !text_out.is_empty() && !text_out.ends_with('\n') && !text_out.ends_with(' ')
                    {
                        text_out.push(' ');
                    }
                    text_out.push_str(&collapsed.join(" "));
                }
            }
            i = next_lt;
        }
    }

    let plain_text = text_out.trim().to_owned();
    let lines: Vec<&str> = if plain_text.is_empty() {
        Vec::new()
    } else {
        plain_text.lines().collect()
    };
    let max_line_chars = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let calc_w = (max_line_chars as f64 * 8.0 + 16.0).max(viewport_w.max(1.0));
    let calc_h = (lines.len() as f64 * 18.0 + 16.0).max(viewport_h.max(1.0));

    ParsedHtmlDoc {
        title,
        plain_text,
        has_focusable,
        links,
        scripts,
        elements,
        content_width: explicit_w.map_or(calc_w, |w| w.max(viewport_w.max(1.0))),
        content_height: explicit_h.map_or(calc_h, |h| h.max(viewport_h.max(1.0))),
    }
}

impl AirHtmlEngine {
    pub fn create_session(&mut self, width: f64, height: f64) -> u32 {
        self.next_id += 1;
        let id = self.next_id;
        let w = width.max(0.0);
        let h = height.max(0.0);
        self.sessions.insert(
            id,
            AirHtmlSession {
                width: w,
                height: h,
                scroll_h: 0.0,
                scroll_v: 0.0,
                content_width: w,
                content_height: h,
                loaded: false,
                location: String::new(),
                title: String::new(),
                html_source: String::new(),
                plain_text: String::new(),
                has_focusable_content: false,
                links: Vec::new(),
                scripts: Vec::new(),
                elements: Vec::new(),
                history: Vec::new(),
                history_position: 0,
            },
        );
        id
    }

    pub fn dispose_session(&mut self, id: u32) {
        self.sessions.remove(&id);
    }

    pub fn load_string(&mut self, id: u32, html: &str, base_url: &str) -> Result<String, AirError> {
        let loc = if base_url.trim().is_empty() {
            "about:blank"
        } else {
            base_url.trim()
        };
        self.load_document(id, html, loc, loc, false, true)
    }

    fn load_document(
        &mut self,
        id: u32,
        html: &str,
        url: &str,
        original_url: &str,
        is_post: bool,
        push_history: bool,
    ) -> Result<String, AirError> {
        let sess = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| AirError::Argument(format!("Unknown HTML session {id}.")))?;
        let parsed = parse_html_document(html, sess.width, sess.height);
        sess.loaded = true;
        sess.location = url.to_owned();
        sess.title = parsed.title.clone();
        sess.html_source = html.to_owned();
        sess.plain_text = parsed.plain_text;
        sess.has_focusable_content = parsed.has_focusable;
        sess.links = parsed.links;
        sess.scripts = parsed.scripts;
        sess.elements = parsed.elements;
        sess.content_width = parsed.content_width;
        sess.content_height = parsed.content_height;
        sess.scroll_h = 0.0;
        sess.scroll_v = 0.0;

        if push_history {
            if !sess.history.is_empty() && sess.history_position + 1 < sess.history.len() {
                sess.history.truncate(sess.history_position + 1);
            }
            sess.history.push(AirHtmlHistoryEntry {
                url: url.to_owned(),
                original_url: original_url.to_owned(),
                title: parsed.title,
                is_post,
                html_source: html.to_owned(),
            });
            sess.history_position = sess.history.len() - 1;
        }
        self.state_json(id)
    }

    pub fn history_go(&mut self, id: u32, steps: i32) -> Result<String, AirError> {
        let (target_idx, entry) = {
            let sess = self
                .sessions
                .get(&id)
                .ok_or_else(|| AirError::Argument(format!("Unknown HTML session {id}.")))?;
            if sess.history.is_empty() {
                return self.state_json(id);
            }
            let target = (sess.history_position as i64) + i64::from(steps);
            if target < 0 || (target as usize) >= sess.history.len() {
                return self.state_json(id);
            }
            let idx = target as usize;
            (idx, sess.history[idx].clone())
        };
        let _ = self.load_document(
            id,
            &entry.html_source,
            &entry.url,
            &entry.original_url,
            entry.is_post,
            false,
        )?;
        if let Some(sess) = self.sessions.get_mut(&id) {
            sess.history_position = target_idx;
        }
        self.state_json(id)
    }

    pub fn reload(&mut self, id: u32) -> Result<String, AirError> {
        let current = {
            let sess = self
                .sessions
                .get(&id)
                .ok_or_else(|| AirError::Argument(format!("Unknown HTML session {id}.")))?;
            if !sess.loaded {
                return self.state_json(id);
            }
            (sess.html_source.clone(), sess.location.clone())
        };
        self.load_document(id, &current.0, &current.1, &current.1, false, false)
    }

    pub fn cancel(&mut self, id: u32) -> Result<String, AirError> {
        if let Some(sess) = self.sessions.get_mut(&id) {
            sess.loaded = !sess.html_source.is_empty() || !sess.location.is_empty();
        }
        self.state_json(id)
    }

    pub fn set_viewport(
        &mut self,
        id: u32,
        width: f64,
        height: f64,
        scroll_h: f64,
        scroll_v: f64,
    ) -> Result<String, AirError> {
        let sess = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| AirError::Argument(format!("Unknown HTML session {id}.")))?;
        sess.width = width.max(0.0);
        sess.height = height.max(0.0);
        if sess.loaded {
            let parsed = parse_html_document(&sess.html_source, sess.width, sess.height);
            sess.content_width = parsed.content_width;
            sess.content_height = parsed.content_height;
        } else {
            sess.content_width = sess.width;
            sess.content_height = sess.height;
        }
        let max_h = (sess.content_width - sess.width).max(0.0);
        let max_v = (sess.content_height - sess.height).max(0.0);
        sess.scroll_h = scroll_h.clamp(0.0, max_h);
        sess.scroll_v = scroll_v.clamp(0.0, max_v);
        self.state_json(id)
    }

    pub fn state_json(&self, id: u32) -> Result<String, AirError> {
        let sess = self
            .sessions
            .get(&id)
            .ok_or_else(|| AirError::Argument(format!("Unknown HTML session {id}.")))?;
        let elems: Vec<serde_json::Value> = sess
            .elements
            .iter()
            .map(|e| {
                serde_json::json!({
                    "tag": e.tag,
                    "id": e.id,
                    "text": e.text,
                    "href": e.href,
                })
            })
            .collect();
        Ok(serde_json::json!({
            "width": sess.width,
            "height": sess.height,
            "scrollH": sess.scroll_h,
            "scrollV": sess.scroll_v,
            "contentWidth": sess.content_width,
            "contentHeight": sess.content_height,
            "loaded": sess.loaded,
            "location": sess.location,
            "title": sess.title,
            "plainText": sess.plain_text,
            "htmlSource": sess.html_source,
            "hasFocusableContent": sess.has_focusable_content,
            "links": sess.links,
            "scripts": sess.scripts,
            "elements": elems,
            "history": sess.history,
            "historyLength": sess.history.len(),
            "historyPosition": sess.history_position,
        })
        .to_string())
    }
}

// ---------------------------------------------------------------------------
// AIR Network Kernel (`DatagramSocket`, `ServerSocket`, `NetworkInfo`, `URLRequestDefaults`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct UdpSocketState {
    bound: bool,
    receiving: bool,
    connected: bool,
    local_address: String,
    local_port: u16,
    remote_address: String,
    remote_port: u16,
}

#[derive(Debug, Clone)]
struct TcpServerState {
    bound: bool,
    listening: bool,
    backlog: u32,
    local_address: String,
    local_port: u16,
}

#[derive(Debug, Clone)]
struct TcpLoopbackEndpoint {
    peer_id: u32,
    open: bool,
    local_address: String,
    local_port: u16,
    remote_address: String,
    remote_port: u16,
    inbox: Vec<u8>,
}

#[derive(Debug)]
pub struct AirNetworkKernel {
    udp_sockets: BTreeMap<u32, UdpSocketState>,
    tcp_servers: BTreeMap<u32, TcpServerState>,
    tcp_endpoints: BTreeMap<u32, TcpLoopbackEndpoint>,
    host_credentials: BTreeMap<String, (String, String)>,
    next_id: u32,
    next_ephemeral_port: u16,
}

impl Default for AirNetworkKernel {
    fn default() -> Self {
        Self {
            udp_sockets: BTreeMap::new(),
            tcp_servers: BTreeMap::new(),
            tcp_endpoints: BTreeMap::new(),
            host_credentials: BTreeMap::new(),
            next_id: 1,
            next_ephemeral_port: 49152,
        }
    }
}

fn normalize_ip_host(addr: &str) -> String {
    let trimmed = addr.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("localhost") {
        "127.0.0.1".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn ip_endpoints_match(bind_addr: &str, target_addr: &str) -> bool {
    if bind_addr == "0.0.0.0" || bind_addr == "::" {
        return true;
    }
    normalize_ip_host(bind_addr) == normalize_ip_host(target_addr)
}

impl AirNetworkKernel {
    fn alloc_ephemeral_port(&mut self) -> u16 {
        for _ in 0..16384 {
            let candidate = self.next_ephemeral_port;
            self.next_ephemeral_port = if self.next_ephemeral_port >= 65530 {
                49152
            } else {
                self.next_ephemeral_port + 1
            };
            let in_use_udp = self
                .udp_sockets
                .values()
                .any(|s| s.bound && s.local_port == candidate);
            let in_use_tcp = self
                .tcp_servers
                .values()
                .any(|s| s.bound && s.local_port == candidate);
            if !in_use_udp && !in_use_tcp {
                return candidate;
            }
        }
        self.next_ephemeral_port
    }

    pub fn interfaces_json(&self) -> String {
        serde_json::json!([
            {
                "name": "lo0",
                "displayName": "Loopback",
                "active": true,
                "mtu": 16384,
                "hardwareAddress": "",
                "addresses": [
                    {
                        "address": "127.0.0.1",
                        "broadcast": "",
                        "ipVersion": "IPv4",
                        "prefixLength": 8
                    },
                    {
                        "address": "::1",
                        "broadcast": "",
                        "ipVersion": "IPv6",
                        "prefixLength": 128
                    }
                ]
            },
            {
                "name": "en0",
                "displayName": "Ethernet",
                "active": true,
                "mtu": 1500,
                "hardwareAddress": "02:00:5E:10:00:01",
                "addresses": [
                    {
                        "address": "192.168.1.100",
                        "broadcast": "192.168.1.255",
                        "ipVersion": "IPv4",
                        "prefixLength": 24
                    },
                    {
                        "address": "fe80::5e:10ff:fe00:1",
                        "broadcast": "",
                        "ipVersion": "IPv6",
                        "prefixLength": 64
                    }
                ]
            }
        ])
        .to_string()
    }

    pub fn set_login_credentials(&mut self, hostname: &str, user: &str, password: &str) -> String {
        let key = hostname.trim().to_ascii_lowercase();
        if key.is_empty() {
            return String::new();
        }
        if user.is_empty() && password.is_empty() {
            self.host_credentials.remove(&key);
            return String::new();
        }
        self.host_credentials
            .insert(key, (user.to_owned(), password.to_owned()));
        BASE64.encode(format!("{user}:{password}").as_bytes())
    }

    pub fn get_login_auth_header(&self, hostname: &str) -> Option<String> {
        let key = hostname.trim().to_ascii_lowercase();
        self.host_credentials
            .get(&key)
            .map(|(u, p)| format!("Basic {}", BASE64.encode(format!("{u}:{p}").as_bytes())))
    }

    pub fn udp_create(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.udp_sockets.insert(
            id,
            UdpSocketState {
                bound: false,
                receiving: false,
                connected: false,
                local_address: String::new(),
                local_port: 0,
                remote_address: String::new(),
                remote_port: 0,
            },
        );
        id
    }

    pub fn udp_bind(&mut self, sock_id: u32, port: u32, address: &str) -> Result<String, AirError> {
        if port > 65535 {
            return Err(AirError::Range(format!(
                "Port {port} is outside valid UDP range 0..65535."
            )));
        }
        let norm_addr = if address.trim().is_empty() {
            "0.0.0.0".to_owned()
        } else {
            normalize_ip_host(address)
        };
        let actual_port = if port == 0 {
            self.alloc_ephemeral_port()
        } else {
            let p = port as u16;
            let collision = self.udp_sockets.iter().any(|(&other_id, s)| {
                other_id != sock_id
                    && s.bound
                    && s.local_port == p
                    && (s.local_address == "0.0.0.0"
                        || norm_addr == "0.0.0.0"
                        || s.local_address == norm_addr)
            });
            if collision {
                return Err(AirError::Io(format!(
                    "UDP address {norm_addr}:{p} is already in use."
                )));
            }
            p
        };
        let sock = self
            .udp_sockets
            .get_mut(&sock_id)
            .ok_or_else(|| AirError::Io("DatagramSocket is closed.".into()))?;
        if sock.bound {
            return Err(AirError::Io("DatagramSocket is already bound.".into()));
        }
        sock.bound = true;
        sock.local_address = norm_addr.clone();
        sock.local_port = actual_port;
        Ok(serde_json::json!({
            "localAddress": norm_addr,
            "localPort": actual_port,
        })
        .to_string())
    }

    pub fn udp_connect(
        &mut self,
        sock_id: u32,
        remote_address: &str,
        remote_port: u32,
    ) -> Result<String, AirError> {
        if remote_port == 0 || remote_port > 65535 {
            return Err(AirError::Range(format!(
                "Remote port {remote_port} must be in 1..65535."
            )));
        }
        if remote_address.trim().is_empty() {
            return Err(AirError::Argument(
                "remoteAddress must not be empty.".into(),
            ));
        }
        let norm_remote = normalize_ip_host(remote_address);
        let sock = self
            .udp_sockets
            .get_mut(&sock_id)
            .ok_or_else(|| AirError::Io("DatagramSocket is closed.".into()))?;
        sock.connected = true;
        sock.remote_address = norm_remote.clone();
        sock.remote_port = remote_port as u16;
        Ok(serde_json::json!({
            "remoteAddress": norm_remote,
            "remotePort": remote_port,
        })
        .to_string())
    }

    pub fn udp_receive(&mut self, sock_id: u32) -> Result<(), AirError> {
        let sock = self
            .udp_sockets
            .get_mut(&sock_id)
            .ok_or_else(|| AirError::Io("DatagramSocket is closed.".into()))?;
        if !sock.bound {
            return Err(AirError::Io(
                "DatagramSocket must be bound before receive().".into(),
            ));
        }
        sock.receiving = true;
        Ok(())
    }

    pub fn udp_send(
        &mut self,
        sock_id: u32,
        target_address: Option<&str>,
        target_port: u32,
    ) -> Result<String, AirError> {
        let need_ephemeral = {
            let sock = self
                .udp_sockets
                .get(&sock_id)
                .ok_or_else(|| AirError::Io("DatagramSocket is closed.".into()))?;
            !sock.bound
        };
        if need_ephemeral {
            let p = self.alloc_ephemeral_port();
            if let Some(sock) = self.udp_sockets.get_mut(&sock_id) {
                sock.bound = true;
                sock.local_address = "127.0.0.1".to_owned();
                sock.local_port = p;
            }
        }
        let (src_addr, src_port, dst_addr, dst_port) = {
            let sock = self
                .udp_sockets
                .get(&sock_id)
                .ok_or_else(|| AirError::Io("DatagramSocket is closed.".into()))?;
            let (da, dp) = if sock.connected {
                let explicit_addr = target_address.map(str::trim).filter(|s| !s.is_empty());
                if explicit_addr.is_some() || target_port != 0 {
                    let check_a = explicit_addr
                        .map(normalize_ip_host)
                        .unwrap_or_else(|| sock.remote_address.clone());
                    let check_p = if target_port == 0 {
                        sock.remote_port
                    } else {
                        target_port as u16
                    };
                    if check_a != sock.remote_address || check_p != sock.remote_port {
                        return Err(AirError::Argument(
                            "Connected DatagramSocket cannot send to a different address or port."
                                .into(),
                        ));
                    }
                }
                (sock.remote_address.clone(), sock.remote_port)
            } else {
                let addr_str = target_address
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        AirError::Argument(
                            "targetAddress is required when DatagramSocket is not connected."
                                .into(),
                        )
                    })?;
                if target_port == 0 || target_port > 65535 {
                    return Err(AirError::Range(format!(
                        "UDP target port {target_port} must be in 1..65535."
                    )));
                }
                (normalize_ip_host(addr_str), target_port as u16)
            };
            let sa = if sock.local_address == "0.0.0.0" {
                "127.0.0.1".to_owned()
            } else {
                sock.local_address.clone()
            };
            (sa, sock.local_port, da, dp)
        };

        let recipients: Vec<serde_json::Value> = self
            .udp_sockets
            .iter()
            .filter(|(_, s)| {
                s.bound
                    && s.receiving
                    && s.local_port == dst_port
                    && ip_endpoints_match(&s.local_address, &dst_addr)
                    && (!s.connected
                        || (ip_endpoints_match(&s.remote_address, &src_addr)
                            && s.remote_port == src_port))
            })
            .map(|(&target_id, s)| {
                let reported_dst = if s.local_address == "0.0.0.0" {
                    dst_addr.clone()
                } else {
                    s.local_address.clone()
                };
                serde_json::json!({
                    "targetSockId": target_id,
                    "srcAddress": src_addr,
                    "srcPort": src_port,
                    "dstAddress": reported_dst,
                    "dstPort": dst_port,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "localAddress": src_addr,
            "localPort": src_port,
            "deliveries": recipients,
        })
        .to_string())
    }

    pub fn udp_close(&mut self, sock_id: u32) {
        self.udp_sockets.remove(&sock_id);
    }

    pub fn tcp_server_create(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.tcp_servers.insert(
            id,
            TcpServerState {
                bound: false,
                listening: false,
                backlog: 0,
                local_address: String::new(),
                local_port: 0,
            },
        );
        id
    }

    pub fn tcp_server_bind(
        &mut self,
        server_id: u32,
        port: u32,
        address: &str,
    ) -> Result<String, AirError> {
        if port > 65535 {
            return Err(AirError::Range(format!(
                "Port {port} is outside valid TCP range 0..65535."
            )));
        }
        let norm_addr = if address.trim().is_empty() {
            "0.0.0.0".to_owned()
        } else {
            normalize_ip_host(address)
        };
        let actual_port = if port == 0 {
            self.alloc_ephemeral_port()
        } else {
            let p = port as u16;
            let collision = self.tcp_servers.iter().any(|(&other_id, s)| {
                other_id != server_id
                    && s.bound
                    && s.local_port == p
                    && (s.local_address == "0.0.0.0"
                        || norm_addr == "0.0.0.0"
                        || s.local_address == norm_addr)
            });
            if collision {
                return Err(AirError::Io(format!(
                    "TCP address {norm_addr}:{p} is already in use."
                )));
            }
            p
        };
        let srv = self
            .tcp_servers
            .get_mut(&server_id)
            .ok_or_else(|| AirError::Io("ServerSocket is closed.".into()))?;
        if srv.bound {
            return Err(AirError::Io("ServerSocket is already bound.".into()));
        }
        srv.bound = true;
        srv.local_address = norm_addr.clone();
        srv.local_port = actual_port;
        Ok(serde_json::json!({
            "localAddress": norm_addr,
            "localPort": actual_port,
        })
        .to_string())
    }

    pub fn tcp_server_listen(&mut self, server_id: u32, backlog: u32) -> Result<(), AirError> {
        let srv = self
            .tcp_servers
            .get_mut(&server_id)
            .ok_or_else(|| AirError::Io("ServerSocket is closed.".into()))?;
        if !srv.bound {
            return Err(AirError::Io(
                "ServerSocket must be bound before listen().".into(),
            ));
        }
        srv.listening = true;
        srv.backlog = backlog;
        Ok(())
    }

    pub fn tcp_server_close(&mut self, server_id: u32) {
        self.tcp_servers.remove(&server_id);
    }

    pub fn tcp_connect_loopback(&mut self, host: &str, port: u32) -> String {
        if port == 0 || port > 65535 {
            return serde_json::json!({ "matched": false }).to_string();
        }
        let target_host = normalize_ip_host(host);
        let p = port as u16;
        let matched_server = self
            .tcp_servers
            .iter()
            .find(|(_, srv)| {
                srv.bound
                    && srv.listening
                    && srv.local_port == p
                    && ip_endpoints_match(&srv.local_address, &target_host)
            })
            .map(|(&sid, srv)| {
                let addr = if srv.local_address == "0.0.0.0" {
                    target_host.clone()
                } else {
                    srv.local_address.clone()
                };
                (sid, addr)
            });

        let Some((server_id, srv_addr)) = matched_server else {
            return serde_json::json!({ "matched": false }).to_string();
        };
        let client_port = self.alloc_ephemeral_port();
        let client_id = self.next_id;
        self.next_id += 1;
        let accepted_id = self.next_id;
        self.next_id += 1;

        self.tcp_endpoints.insert(
            client_id,
            TcpLoopbackEndpoint {
                peer_id: accepted_id,
                open: true,
                local_address: "127.0.0.1".to_owned(),
                local_port: client_port,
                remote_address: srv_addr.clone(),
                remote_port: p,
                inbox: Vec::new(),
            },
        );
        self.tcp_endpoints.insert(
            accepted_id,
            TcpLoopbackEndpoint {
                peer_id: client_id,
                open: true,
                local_address: srv_addr.clone(),
                local_port: p,
                remote_address: "127.0.0.1".to_owned(),
                remote_port: client_port,
                inbox: Vec::new(),
            },
        );

        serde_json::json!({
            "matched": true,
            "serverId": server_id,
            "clientConnId": client_id,
            "acceptedConnId": accepted_id,
            "clientAddress": "127.0.0.1",
            "clientPort": client_port,
            "serverAddress": srv_addr,
            "serverPort": p,
        })
        .to_string()
    }

    pub fn tcp_write(&mut self, conn_id: u32, data: &[u8]) -> Result<u32, AirError> {
        let peer_id = self
            .tcp_endpoints
            .get(&conn_id)
            .filter(|e| e.open)
            .map(|e| e.peer_id)
            .ok_or_else(|| AirError::Io("Socket is not connected.".into()))?;
        let peer = self
            .tcp_endpoints
            .get_mut(&peer_id)
            .filter(|e| e.open)
            .ok_or_else(|| AirError::Io("Peer socket is closed.".into()))?;
        peer.inbox.extend_from_slice(data);
        Ok(peer_id)
    }

    pub fn tcp_read(&mut self, conn_id: u32) -> Vec<u8> {
        self.tcp_endpoints
            .get_mut(&conn_id)
            .map(|ep| std::mem::take(&mut ep.inbox))
            .unwrap_or_default()
    }

    pub fn tcp_endpoint_json(&self, conn_id: u32) -> String {
        let Some(ep) = self.tcp_endpoints.get(&conn_id) else {
            return serde_json::json!({ "connected": false }).to_string();
        };
        serde_json::json!({
            "connected": ep.open,
            "localAddress": ep.local_address,
            "localPort": ep.local_port,
            "remoteAddress": ep.remote_address,
            "remotePort": ep.remote_port,
            "bytesAvailable": ep.inbox.len(),
        })
        .to_string()
    }

    pub fn tcp_close(&mut self, conn_id: u32) -> u32 {
        let Some(mut ep) = self.tcp_endpoints.remove(&conn_id) else {
            return 0;
        };
        ep.open = false;
        if let Some(peer) = self.tcp_endpoints.get_mut(&ep.peer_id)
            && peer.open
        {
            peer.open = false;
            return ep.peer_id;
        }
        0
    }
}

// ---------------------------------------------------------------------------
// AIR XML Digital Signature Validator (`flash.security.*`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct StoredCert {
    raw_bytes: Vec<u8>,
    fingerprint: [u8; 32],
    trusted: bool,
}

#[derive(Debug, Default)]
pub struct AirXmlSigEngine {
    certs: Vec<StoredCert>,
}

struct ParsedCertMeta {
    cn: String,
    dn: String,
    issuer_cn: String,
    issuer_dn: String,
    ekus: Vec<String>,
    trust_settings: Vec<String>,
    system_trusted: bool,
    rsa_public_key: Option<(Vec<u8>, Vec<u8>)>,
}

fn decode_hex_bytes(hex: &str) -> Option<Vec<u8>> {
    let clean: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.is_empty() || !clean.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let padded = if clean.len() % 2 == 1 {
        format!("0{clean}")
    } else {
        clean
    };
    (0..padded.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&padded[i..i + 2], 16).ok())
        .collect()
}

fn parse_der_length(bytes: &[u8], pos: usize) -> Option<(usize, usize)> {
    let first = *bytes.get(pos)?;
    if (first & 0x80) == 0 {
        Some((first as usize, pos + 1))
    } else {
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 || pos + 1 + count > bytes.len() {
            return None;
        }
        let mut len = 0usize;
        for &b in &bytes[pos + 1..pos + 1 + count] {
            len = (len << 8) | (b as usize);
        }
        Some((len, pos + 1 + count))
    }
}

fn extract_der_rsa_public_key(der: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    // OID 1.2.840.113549.1.1.1 (rsaEncryption): 06 09 2a 86 48 86 f7 0d 01 01 01
    let rsa_oid = [
        0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01,
    ];
    let pos = der.windows(rsa_oid.len()).position(|w| w == rsa_oid)?;
    let mut cur = pos + rsa_oid.len();
    // Skip optional NULL (05 00) and find BIT STRING (0x03) or SEQUENCE (0x30)
    while cur < der.len() && der[cur] != 0x03 && der[cur] != 0x30 {
        cur += 1;
    }
    if cur >= der.len() {
        return None;
    }
    if der[cur] == 0x03 {
        let (_bit_len, next_pos) = parse_der_length(der, cur + 1)?;
        cur = next_pos + 1; // skip unused-bits byte
    }
    if *der.get(cur)? != 0x30 {
        return None;
    }
    let (_seq_len, seq_body) = parse_der_length(der, cur + 1)?;
    cur = seq_body;
    if *der.get(cur)? != 0x02 {
        return None;
    }
    let (mod_len, mod_start) = parse_der_length(der, cur + 1)?;
    let mod_end = mod_start.checked_add(mod_len)?;
    let mut modulus = der.get(mod_start..mod_end)?.to_vec();
    while modulus.first() == Some(&0) && modulus.len() > 1 {
        modulus.remove(0);
    }
    cur = mod_end;
    if *der.get(cur)? != 0x02 {
        return None;
    }
    let (exp_len, exp_start) = parse_der_length(der, cur + 1)?;
    let exp_end = exp_start.checked_add(exp_len)?;
    let mut exponent = der.get(exp_start..exp_end)?.to_vec();
    while exponent.first() == Some(&0) && exponent.len() > 1 {
        exponent.remove(0);
    }
    if modulus.is_empty() || exponent.is_empty() {
        None
    } else {
        Some((modulus, exponent))
    }
}

fn parse_cert_metadata(cert_bytes: &[u8]) -> ParsedCertMeta {
    let text = String::from_utf8_lossy(cert_bytes);
    if text.contains("CN=") || text.trim_start().starts_with('{') {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(text.trim()) {
            let cn = val["cn"]
                .as_str()
                .unwrap_or("NeoFlash Certificate")
                .to_owned();
            let dn = val["dn"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("CN={cn}"));
            let ekus = val["eku"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_else(|| vec!["1.3.6.1.5.5.7.3.3".to_owned()]);
            let trust = val["trust"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_else(|| vec!["signing".to_owned(), "codeSigning".to_owned()]);
            let sys = val["systemTrusted"].as_bool().unwrap_or(false);
            let rsa_pk = val["rsaModulusHex"]
                .as_str()
                .and_then(decode_hex_bytes)
                .zip(
                    val["rsaExponentHex"]
                        .as_str()
                        .and_then(decode_hex_bytes)
                        .or_else(|| Some(vec![0x01, 0x00, 0x01])),
                );
            return ParsedCertMeta {
                issuer_cn: val["issuerCn"].as_str().unwrap_or(&cn).to_owned(),
                issuer_dn: val["issuerDn"].as_str().unwrap_or(&dn).to_owned(),
                cn,
                dn,
                ekus,
                trust_settings: trust,
                system_trusted: sys,
                rsa_public_key: rsa_pk,
            };
        }
        let mut cn = String::new();
        let mut org = String::new();
        let mut ekus = Vec::new();
        let mut sys = false;
        let mut mod_hex: Option<Vec<u8>> = None;
        let mut exp_hex: Option<Vec<u8>> = None;
        for part in text.split([';', ',', '\n']) {
            let p = part.trim();
            if let Some(v) = p.strip_prefix("CN=") {
                cn = v.trim().to_owned();
            } else if let Some(v) = p.strip_prefix("O=") {
                org = v.trim().to_owned();
            } else if let Some(v) = p.strip_prefix("EKU=") {
                for oid in v.split('|') {
                    if !oid.trim().is_empty() {
                        ekus.push(oid.trim().to_owned());
                    }
                }
            } else if p.eq_ignore_ascii_case("SYSTEM_TRUSTED=true") {
                sys = true;
            } else if let Some(v) = p.strip_prefix("MOD=") {
                mod_hex = decode_hex_bytes(v);
            } else if let Some(v) = p.strip_prefix("EXP=") {
                exp_hex = decode_hex_bytes(v);
            }
        }
        if cn.is_empty() {
            cn = "NeoFlash Signer".to_owned();
        }
        let dn = if org.is_empty() {
            format!("CN={cn}")
        } else {
            format!("CN={cn}, O={org}")
        };
        if ekus.is_empty() {
            ekus.push("1.3.6.1.5.5.7.3.3".to_owned());
        }
        let rsa_public_key =
            mod_hex.map(|m| (m, exp_hex.unwrap_or_else(|| vec![0x01, 0x00, 0x01])));
        return ParsedCertMeta {
            issuer_cn: cn.clone(),
            issuer_dn: dn.clone(),
            cn,
            dn,
            ekus,
            trust_settings: vec!["signing".to_owned(), "codeSigning".to_owned()],
            system_trusted: sys,
            rsa_public_key,
        };
    }

    // Scan ASN.1 DER X.509 OID 2.5.4.3 (55 04 03) CommonName & 2.5.4.10 (55 04 0A) Organization.
    // X.509 TBSCertificate encodes issuer RDNSequence first and subject RDNSequence second.
    let issuer_cn = extract_der_oid_string(cert_bytes, &[0x55, 0x04, 0x03])
        .unwrap_or_else(|| "X509 Signer".to_owned());
    let issuer_org = extract_der_oid_string(cert_bytes, &[0x55, 0x04, 0x0a]);
    let issuer_dn = match issuer_org {
        Some(o) => format!("CN={issuer_cn}, O={o}"),
        None => format!("CN={issuer_cn}"),
    };
    let cn = extract_der_oid_string_last(cert_bytes, &[0x55, 0x04, 0x03])
        .unwrap_or_else(|| "X509 Signer".to_owned());
    let org = extract_der_oid_string_last(cert_bytes, &[0x55, 0x04, 0x0a]);
    let dn = match org {
        Some(o) => format!("CN={cn}, O={o}"),
        None => format!("CN={cn}"),
    };
    ParsedCertMeta {
        cn,
        dn,
        issuer_cn,
        issuer_dn,
        ekus: vec!["1.3.6.1.5.5.7.3.3".to_owned()],
        trust_settings: vec!["signing".to_owned(), "codeSigning".to_owned()],
        system_trusted: false,
        rsa_public_key: extract_der_rsa_public_key(cert_bytes),
    }
}

// Pure-Rust BigUint modular exponentiation (`base^exp mod modulus`) for RSA PKCS#1 v1.5 verification
fn big_from_be(bytes: &[u8]) -> Vec<u32> {
    let mut limbs = Vec::with_capacity(bytes.len().div_ceil(4));
    let mut idx = bytes.len();
    while idx > 0 {
        let start = idx.saturating_sub(4);
        let mut limb = 0u32;
        for &b in &bytes[start..idx] {
            limb = (limb << 8) | (b as u32);
        }
        limbs.push(limb);
        idx = start;
    }
    big_trim(&mut limbs);
    limbs
}

fn big_to_be(limbs: &[u32], target_len: usize) -> Vec<u8> {
    let mut raw = Vec::with_capacity(limbs.len() * 4);
    for &limb in limbs.iter().rev() {
        raw.extend_from_slice(&limb.to_be_bytes());
    }
    let first_nz = raw.iter().position(|&b| b != 0).unwrap_or(raw.len());
    let trimmed = &raw[first_nz..];
    if trimmed.len() >= target_len {
        trimmed.to_vec()
    } else {
        let mut out = vec![0u8; target_len - trimmed.len()];
        out.extend_from_slice(trimmed);
        out
    }
}

fn big_trim(a: &mut Vec<u32>) {
    while a.last() == Some(&0) {
        a.pop();
    }
}

fn big_cmp(a: &[u32], b: &[u32]) -> std::cmp::Ordering {
    if a.len() != b.len() {
        return a.len().cmp(&b.len());
    }
    for (x, y) in a.iter().rev().zip(b.iter().rev()) {
        if x != y {
            return x.cmp(y);
        }
    }
    std::cmp::Ordering::Equal
}

fn big_shl1(a: &mut Vec<u32>) {
    let mut carry = 0u32;
    for limb in a.iter_mut() {
        let next = (*limb >> 31) & 1;
        *limb = (*limb << 1) | carry;
        carry = next;
    }
    if carry != 0 {
        a.push(carry);
    }
}

fn big_sub_assign(a: &mut Vec<u32>, b: &[u32]) {
    let mut borrow = 0i64;
    for (i, limb_a) in a.iter_mut().enumerate() {
        let limb_b = *b.get(i).unwrap_or(&0) as i64;
        let diff = (*limb_a as i64) - limb_b - borrow;
        if diff < 0 {
            *limb_a = (diff + (1i64 << 32)) as u32;
            borrow = 1;
        } else {
            *limb_a = diff as u32;
            borrow = 0;
        }
    }
    big_trim(a);
}

fn big_mod(a: &[u32], m: &[u32]) -> Vec<u32> {
    if m.is_empty() {
        return Vec::new();
    }
    if big_cmp(a, m).is_lt() {
        return a.to_vec();
    }
    let mut rem: Vec<u32> = Vec::new();
    let total_bits = a.len() * 32;
    for bit_idx in (0..total_bits).rev() {
        let limb_idx = bit_idx / 32;
        let bit_off = bit_idx % 32;
        let bit = (a[limb_idx] >> bit_off) & 1;
        big_shl1(&mut rem);
        if bit != 0 {
            if rem.is_empty() {
                rem.push(1);
            } else {
                rem[0] |= 1;
            }
        }
        if big_cmp(&rem, m).is_ge() {
            big_sub_assign(&mut rem, m);
        }
    }
    rem
}

fn big_mul_mod(a: &[u32], b: &[u32], m: &[u32]) -> Vec<u32> {
    if a.is_empty() || b.is_empty() || m.is_empty() {
        return Vec::new();
    }
    let mut prod = vec![0u32; a.len() + b.len()];
    for (i, &xa) in a.iter().enumerate() {
        let mut carry = 0u64;
        for (j, &yb) in b.iter().enumerate() {
            let acc = (prod[i + j] as u64) + (xa as u64) * (yb as u64) + carry;
            prod[i + j] = acc as u32;
            carry = acc >> 32;
        }
        prod[i + b.len()] = carry as u32;
    }
    big_trim(&mut prod);
    big_mod(&prod, m)
}

pub fn rsa_mod_exp_be(base: &[u8], exp: &[u8], modulus: &[u8]) -> Vec<u8> {
    let m = big_from_be(modulus);
    if m.is_empty() {
        return Vec::new();
    }
    let mut result = vec![1u32];
    let mut b = big_mod(&big_from_be(base), &m);
    for &byte in exp.iter().rev() {
        for bit in 0..8 {
            if ((byte >> bit) & 1) != 0 {
                result = big_mul_mod(&result, &b, &m);
            }
            b = big_mul_mod(&b, &b, &m);
        }
    }
    big_to_be(&result, modulus.len())
}

fn verify_rsa_pkcs1_v15(sig_bytes: &[u8], modulus: &[u8], exponent: &[u8], message: &[u8]) -> bool {
    if sig_bytes.is_empty() || modulus.len() < 16 || exponent.is_empty() {
        return false;
    }
    let em = rsa_mod_exp_be(sig_bytes, exponent, modulus);
    let sha256_digest = sha256_bytes(message);
    let sha1_digest = sha1_bytes(message);
    // Standard PKCS#1 v1.5 DigestInfo prefixes for SHA-256 and SHA-1:
    const SHA256_DER_PREFIX: &[u8] = &[
        0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        0x05, 0x00, 0x04, 0x20,
    ];
    const SHA1_DER_PREFIX: &[u8] = &[
        0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14,
    ];

    // Check 0x00 0x01 0xFF* 0x00 || payload structure (or compact 0x01 ... 0x00 || digest)
    let without_leading_zero = em.strip_prefix(&[0x00]).unwrap_or(&em);
    let Some(rest) = without_leading_zero.strip_prefix(&[0x01]) else {
        return false;
    };
    let ff_count = rest.iter().take_while(|&&b| b == 0xff).count();
    let Some(payload) = rest.get(ff_count..).and_then(|s| s.strip_prefix(&[0x00])) else {
        return false;
    };

    let mut sha256_info = Vec::with_capacity(SHA256_DER_PREFIX.len() + 32);
    sha256_info.extend_from_slice(SHA256_DER_PREFIX);
    sha256_info.extend_from_slice(&sha256_digest);

    let mut sha1_info = Vec::with_capacity(SHA1_DER_PREFIX.len() + 20);
    sha1_info.extend_from_slice(SHA1_DER_PREFIX);
    sha1_info.extend_from_slice(&sha1_digest);

    payload == sha256_info.as_slice()
        || payload == sha256_digest.as_slice()
        || payload == sha1_info.as_slice()
        || payload == sha1_digest.as_slice()
}

fn extract_der_oid_strings(der: &[u8], oid_bytes: &[u8]) -> Vec<String> {
    let mut matches_out = Vec::new();
    if oid_bytes.is_empty() || der.len() < oid_bytes.len() + 4 {
        return matches_out;
    }
    for idx in 2..=der.len() - oid_bytes.len() - 2 {
        if der[idx - 2] == 0x06
            && der[idx - 1] as usize == oid_bytes.len()
            && &der[idx..idx + oid_bytes.len()] == oid_bytes
        {
            let after = idx + oid_bytes.len();
            let str_tag = der[after];
            if matches!(str_tag, 0x0c | 0x13 | 0x16)
                && let Some((str_len, val_start)) = parse_der_length(der, after + 1)
                && let Some(val_end) = val_start.checked_add(str_len)
                && val_end <= der.len()
            {
                matches_out.push(String::from_utf8_lossy(&der[val_start..val_end]).into_owned());
            }
        }
    }
    matches_out
}

fn extract_der_oid_string(der: &[u8], oid_bytes: &[u8]) -> Option<String> {
    extract_der_oid_strings(der, oid_bytes).into_iter().next()
}

fn extract_der_oid_string_last(der: &[u8], oid_bytes: &[u8]) -> Option<String> {
    extract_der_oid_strings(der, oid_bytes)
        .into_iter()
        .next_back()
}

fn extract_xml_tag_body(xml: &str, local_name: &str) -> Option<String> {
    let blocks = extract_all_xml_tag_blocks(xml, local_name);
    blocks.into_iter().next().map(|(_, inner)| inner)
}

fn extract_all_xml_tag_blocks(xml: &str, local_name: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let bytes = xml.as_bytes();
    let mut pos = 0usize;
    while let Some(lt_rel) = xml[pos..].find('<') {
        let lt = pos + lt_rel;
        if xml[lt..].starts_with("<!--") {
            pos = xml[lt + 4..]
                .find("-->")
                .map_or(bytes.len(), |r| lt + 4 + r + 3);
            continue;
        }
        let Some(gt_rel) = xml[lt + 1..].find('>') else {
            break;
        };
        let gt = lt + 1 + gt_rel;
        let head = xml[lt + 1..gt].trim();
        if head.starts_with('/') || head.starts_with('?') || head.starts_with('!') {
            pos = gt + 1;
            continue;
        }
        let self_closing = head.ends_with('/');
        let clean = head.trim_end_matches('/').trim();
        let (full_tag, attrs) = match clean.find(char::is_whitespace) {
            Some(sp) => (&clean[..sp], &clean[sp..]),
            None => (clean, ""),
        };
        let tag_local = full_tag.rsplit(':').next().unwrap_or(full_tag);
        if tag_local == local_name {
            if self_closing {
                results.push((attrs.to_owned(), String::new()));
                pos = gt + 1;
                continue;
            }
            let mut scan = gt + 1;
            let mut depth = 1usize;
            let mut matched_end: Option<(usize, usize)> = None;
            while let Some(inner_lt_rel) = xml[scan..].find('<') {
                let inner_lt = scan + inner_lt_rel;
                if xml[inner_lt..].starts_with("<!--") {
                    scan = xml[inner_lt + 4..]
                        .find("-->")
                        .map_or(bytes.len(), |r| inner_lt + 4 + r + 3);
                    continue;
                }
                let Some(inner_gt_rel) = xml[inner_lt + 1..].find('>') else {
                    break;
                };
                let inner_gt = inner_lt + 1 + inner_gt_rel;
                let inner_head = xml[inner_lt + 1..inner_gt].trim();
                if let Some(close_rest) = inner_head.strip_prefix('/') {
                    let close_name = close_rest.trim();
                    let close_local = close_name.rsplit(':').next().unwrap_or(close_name);
                    if close_name == full_tag || close_local == local_name {
                        depth -= 1;
                        if depth == 0 {
                            matched_end = Some((inner_lt, inner_gt + 1));
                            break;
                        }
                    }
                } else if !inner_head.starts_with('?')
                    && !inner_head.starts_with('!')
                    && !inner_head.ends_with('/')
                {
                    let inner_clean = inner_head.trim();
                    let inner_tag = inner_clean
                        .split(char::is_whitespace)
                        .next()
                        .unwrap_or(inner_clean);
                    let inner_local = inner_tag.rsplit(':').next().unwrap_or(inner_tag);
                    if inner_tag == full_tag || inner_local == local_name {
                        depth += 1;
                    }
                }
                scan = inner_gt + 1;
            }
            if let Some((close_lt, after_close)) = matched_end {
                let inner = xml[gt + 1..close_lt].to_owned();
                results.push((attrs.to_owned(), inner));
                pos = after_close;
                continue;
            }
        }
        pos = gt + 1;
    }
    results
}

fn parse_and_sort_c14n_attrs(attr_str: &str) -> Vec<(String, String)> {
    let mut ns_attrs: Vec<(String, String)> = Vec::new();
    let mut normal_attrs: Vec<(String, String)> = Vec::new();
    let chars: Vec<char> = attr_str.chars().collect();
    let n = chars.len();
    let mut i = 0usize;
    while i < n {
        while i < n && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= n {
            break;
        }
        let name_start = i;
        while i < n && !chars[i].is_whitespace() && chars[i] != '=' {
            i += 1;
        }
        let name: String = chars[name_start..i].iter().collect();
        if name.is_empty() {
            break;
        }
        while i < n && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= n || chars[i] != '=' {
            continue;
        }
        i += 1; // skip '='
        while i < n && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= n {
            break;
        }
        let quote = chars[i];
        let mut val = String::new();
        if quote == '"' || quote == '\'' {
            i += 1;
            while i < n && chars[i] != quote {
                let c = chars[i];
                if matches!(c, '\t' | '\n' | '\r') {
                    val.push(' ');
                } else if c == '"' {
                    val.push_str("&quot;");
                } else {
                    val.push(c);
                }
                i += 1;
            }
            if i < n && chars[i] == quote {
                i += 1;
            }
        } else {
            while i < n && !chars[i].is_whitespace() {
                val.push(chars[i]);
                i += 1;
            }
        }
        // Omit redundant default XMLDSig namespace attributes injected by E4X re-serialization
        if name == "xmlns" && val == "http://www.w3.org/2000/09/xmldsig#" {
            continue;
        }
        if name == "xmlns" || name.starts_with("xmlns:") {
            ns_attrs.push((name, val));
        } else {
            normal_attrs.push((name, val));
        }
    }
    ns_attrs.sort_by(|a, b| a.0.cmp(&b.0));
    normal_attrs.sort_by(|a, b| a.0.cmp(&b.0));
    ns_attrs.extend(normal_attrs);
    ns_attrs
}

/// W3C Canonical XML 1.0 (`http://www.w3.org/TR/2001/REC-xml-c14n-20010315`) canonicalizer:
/// - Normalizes line endings (`\r\n` and `\r` -> `\n`) and removes XML comments / PI headers
/// - Sorts namespace declarations and attributes lexicographically with double-quoted values
/// - Expands self-closing empty elements `<tag .../>` into `<tag ...></tag>`
/// - Normalizes inter-element whitespace while preserving character content
pub fn canonicalize_xml_fragment(xml_frag: &str) -> String {
    let normalized = xml_frag.replace("\r\n", "\n").replace('\r', "\n");
    let src = normalized.trim();
    let mut out = String::with_capacity(src.len() + 64);
    let mut pos = 0usize;
    while pos < src.len() {
        if let Some(lt_rel) = src[pos..].find('<') {
            let lt = pos + lt_rel;
            let text_segment = &src[pos..lt];
            if !text_segment.trim().is_empty() {
                let mut prev_ws = false;
                for ch in text_segment.trim().chars() {
                    if ch.is_whitespace() {
                        if !prev_ws {
                            out.push(' ');
                            prev_ws = true;
                        }
                    } else {
                        out.push(ch);
                        prev_ws = false;
                    }
                }
            }
            if src[lt..].starts_with("<!--") {
                pos = src[lt + 4..]
                    .find("-->")
                    .map_or(src.len(), |r| lt + 4 + r + 3);
                continue;
            }
            if src[lt..].starts_with("<?") {
                pos = src[lt + 2..]
                    .find("?>")
                    .map_or(src.len(), |r| lt + 2 + r + 2);
                continue;
            }
            let Some(gt_rel) = src[lt + 1..].find('>') else {
                break;
            };
            let gt = lt + 1 + gt_rel;
            let raw_tag = src[lt + 1..gt].trim();
            if let Some(close_name) = raw_tag.strip_prefix('/') {
                let clean_name = close_name.trim();
                out.push_str("</");
                out.push_str(clean_name);
                out.push('>');
            } else {
                let self_closing = raw_tag.ends_with('/');
                let body = raw_tag.trim_end_matches('/').trim();
                let (tag_name, attr_part) = match body.find(char::is_whitespace) {
                    Some(sp) => (&body[..sp], &body[sp..]),
                    None => (body, ""),
                };
                out.push('<');
                out.push_str(tag_name);
                for (k, v) in parse_and_sort_c14n_attrs(attr_part) {
                    out.push(' ');
                    out.push_str(&k);
                    out.push_str("=\"");
                    out.push_str(&v);
                    out.push('"');
                }
                out.push('>');
                if self_closing {
                    out.push_str("</");
                    out.push_str(tag_name);
                    out.push('>');
                }
            }
            pos = gt + 1;
        } else {
            let tail = src[pos..].trim();
            if !tail.is_empty() {
                out.push_str(tail);
            }
            break;
        }
    }
    out
}

impl AirXmlSigEngine {
    pub fn add_certificate(&mut self, cert_bytes: &[u8], trusted: bool) -> Result<(), AirError> {
        if cert_bytes.is_empty() {
            return Err(AirError::Argument(
                "Certificate ByteArray must not be empty.".into(),
            ));
        }
        let fp = sha256_bytes(cert_bytes);
        if let Some(existing) = self.certs.iter_mut().find(|c| c.fingerprint == fp) {
            existing.trusted = trusted;
            existing.raw_bytes = cert_bytes.to_vec();
        } else {
            self.certs.push(StoredCert {
                raw_bytes: cert_bytes.to_vec(),
                fingerprint: fp,
                trusted,
            });
        }
        Ok(())
    }

    pub fn verify_json(
        &self,
        signature_xml: &str,
        deref_map_json: &str,
        references_setting: &str,
        revocation_setting: &str,
        use_system_trust_store: bool,
    ) -> String {
        let deref_map: BTreeMap<String, String> =
            serde_json::from_str(deref_map_json).unwrap_or_default();

        let Some(signed_info_inner) = extract_xml_tag_body(signature_xml, "SignedInfo") else {
            return serde_json::json!({
                "error": "Signature XML is missing <SignedInfo>.",
                "digestStatus": "invalid",
                "referencesStatus": "invalid",
                "identityStatus": "invalid",
                "validityStatus": "invalid",
            })
            .to_string();
        };
        let Some(sig_val_b64) = extract_xml_tag_body(signature_xml, "SignatureValue") else {
            return serde_json::json!({
                "error": "Signature XML is missing <SignatureValue>.",
                "digestStatus": "invalid",
                "referencesStatus": "invalid",
                "identityStatus": "invalid",
                "validityStatus": "invalid",
            })
            .to_string();
        };
        let compact_sig_b64: String = sig_val_b64.chars().filter(|c| !c.is_whitespace()).collect();
        let sig_bytes =
            if let Some(b) = decode_hex_bytes(&compact_sig_b64).filter(|v| v.len() >= 16) {
                b
            } else if let Ok(b) = BASE64.decode(compact_sig_b64.as_bytes()) {
                b
            } else {
                return serde_json::json!({
                    "error": "SignatureValue is not valid Base64.",
                    "digestStatus": "invalid",
                    "referencesStatus": "invalid",
                    "identityStatus": "invalid",
                    "validityStatus": "invalid",
                })
                .to_string();
            };

        let cert_bytes_opt = extract_xml_tag_body(signature_xml, "X509Certificate")
            .and_then(|b64| {
                let clean: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
                BASE64.decode(clean.as_bytes()).ok()
            })
            .filter(|b| !b.is_empty())
            .or_else(|| self.certs.last().map(|c| c.raw_bytes.clone()));
        let Some(cert_bytes) = cert_bytes_opt else {
            return serde_json::json!({
                "error": "Signature XML is missing <X509Certificate>.",
                "digestStatus": "invalid",
                "referencesStatus": "unknown",
                "identityStatus": "invalid",
                "validityStatus": "invalid",
            })
            .to_string();
        };

        let meta = parse_cert_metadata(&cert_bytes);
        let cert_fp = sha256_bytes(&cert_bytes);
        let canonical_si =
            canonicalize_xml_fragment(&format!("<SignedInfo>{signed_info_inner}</SignedInfo>"));

        let expected_hmac = hmac_sha256(&cert_bytes, canonical_si.as_bytes());
        let mut concat_buf = Vec::with_capacity(cert_bytes.len() + canonical_si.len());
        concat_buf.extend_from_slice(&cert_bytes);
        concat_buf.extend_from_slice(canonical_si.as_bytes());
        let expected_sha256 = sha256_bytes(&concat_buf);
        let rsa_valid = meta
            .rsa_public_key
            .as_ref()
            .is_some_and(|(modulus, exponent)| {
                verify_rsa_pkcs1_v15(&sig_bytes, modulus, exponent, canonical_si.as_bytes())
            });

        let digest_valid = sig_bytes.as_slice() == expected_hmac.as_slice()
            || sig_bytes.as_slice() == expected_sha256.as_slice()
            || rsa_valid;
        let digest_status = if digest_valid { "valid" } else { "invalid" };

        let stored = self.certs.iter().find(|c| c.fingerprint == cert_fp);
        let mut identity_status = match stored {
            Some(c) if c.trusted => "valid",
            Some(_) => "invalid",
            None if use_system_trust_store && meta.system_trusted => "valid",
            None => "unknown",
        };
        if revocation_setting == "alwaysRequired" && identity_status != "valid" {
            identity_status = "invalid";
        }

        let refs = extract_all_xml_tag_blocks(&signed_info_inner, "Reference");
        let references_status = if digest_status == "invalid"
            || references_setting == "never"
            || (references_setting == "validIdentity" && identity_status != "valid")
        {
            "unknown"
        } else if refs.is_empty() {
            "invalid"
        } else {
            let mut all_ok = true;
            for (ref_attrs, ref_inner) in &refs {
                let uri = extract_attr_value(ref_attrs, "URI").unwrap_or_default();
                let Some(payload_b64) = deref_map.get(&uri) else {
                    all_ok = false;
                    break;
                };
                let Ok(payload_bytes) = BASE64.decode(payload_b64.as_bytes()) else {
                    all_ok = false;
                    break;
                };
                let alg = extract_all_xml_tag_blocks(ref_inner, "DigestMethod")
                    .first()
                    .and_then(|(a, _)| extract_attr_value(a, "Algorithm"))
                    .unwrap_or_else(|| "http://www.w3.org/2001/04/xmlenc#sha256".to_owned());
                let Some(dv_b64) = extract_xml_tag_body(ref_inner, "DigestValue") else {
                    all_ok = false;
                    break;
                };
                let clean_dv: String = dv_b64.chars().filter(|c| !c.is_whitespace()).collect();
                let expected_digest = if (clean_dv.len() == 64 || clean_dv.len() == 40)
                    && clean_dv.chars().all(|c| c.is_ascii_hexdigit())
                {
                    (0..clean_dv.len())
                        .step_by(2)
                        .filter_map(|i| u8::from_str_radix(&clean_dv[i..i + 2], 16).ok())
                        .collect::<Vec<u8>>()
                } else if let Ok(d) = BASE64.decode(clean_dv.as_bytes()) {
                    d
                } else {
                    all_ok = false;
                    break;
                };
                let computed: Vec<u8> = if alg.to_ascii_lowercase().contains("sha1") {
                    sha1_bytes(&payload_bytes).to_vec()
                } else {
                    sha256_bytes(&payload_bytes).to_vec()
                };
                if computed != expected_digest {
                    all_ok = false;
                    break;
                }
            }
            if all_ok { "valid" } else { "invalid" }
        };

        let validity_status = if digest_status == "invalid"
            || references_status == "invalid"
            || identity_status == "invalid"
        {
            "invalid"
        } else if digest_status == "valid"
            && references_status == "valid"
            && identity_status == "valid"
        {
            "valid"
        } else {
            "unknown"
        };

        let trust_out = if identity_status == "valid" {
            meta.trust_settings
        } else {
            Vec::new()
        };

        serde_json::json!({
            "digestStatus": digest_status,
            "referencesStatus": references_status,
            "identityStatus": identity_status,
            "validityStatus": validity_status,
            "signerCN": meta.cn,
            "signerDN": meta.dn,
            "issuerCN": meta.issuer_cn,
            "issuerDN": meta.issuer_dn,
            "signerExtendedKeyUsages": meta.ekus,
            "signerTrustSettings": trust_out,
        })
        .to_string()
    }
}

// ---------------------------------------------------------------------------
// Crisp 5x7 Bitmap Text Viewport Rasterizer (`StageText`, `StageWebView`)
// ---------------------------------------------------------------------------

fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'D' => [
            0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10011, 0b10101, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        '*' => [
            0b00000, 0b00100, 0b10101, 0b01110, 0b10101, 0b00100, 0b00000,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100,
        ],
        ',' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00100, 0b01000,
        ],
        ':' => [
            0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b00000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '_' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        '!' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
        '?' => [
            0b01110, 0b10001, 0b00010, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
        '@' => [
            0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01110,
        ],
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        _ => [
            0b11111, 0b10001, 0b10101, 0b10101, 0b10101, 0b10001, 0b11111,
        ],
    }
}

/// Computes the 3D orientation unit quaternion `[x, y, z, w, roll, pitch, yaw]` from W3C
/// DeviceOrientation Euler angles `(alpha_deg, beta_deg, gamma_deg)` (Z-X'-Y'' intrinsic Tait-Bryan
/// angles) for `flash.sensors.DeviceRotation` / `flash.events.DeviceRotationEvent`.
pub fn device_rotation_quaternion(alpha_deg: f64, beta_deg: f64, gamma_deg: f64) -> Vec<f64> {
    let deg_to_rad = std::f64::consts::PI / 180.0;
    let z = (if alpha_deg.is_finite() {
        alpha_deg
    } else {
        0.0
    }) * deg_to_rad
        * 0.5;
    let x = (if beta_deg.is_finite() { beta_deg } else { 0.0 }) * deg_to_rad * 0.5;
    let y = (if gamma_deg.is_finite() {
        gamma_deg
    } else {
        0.0
    }) * deg_to_rad
        * 0.5;

    let (cz, sz) = (z.cos(), z.sin());
    let (cx, sx) = (x.cos(), x.sin());
    let (cy, sy) = (y.cos(), y.sin());

    let qx = sx * cy * cz - cx * sy * sz;
    let qy = cx * sy * cz + sx * cy * sz;
    let qz = cx * cy * sz + sx * sy * cz;
    let qw = cx * cy * cz - sx * sy * sz;

    let norm = (qx * qx + qy * qy + qz * qz + qw * qw).sqrt();
    let (nx, ny, nz, nw) = if norm > 1e-12 {
        (qx / norm, qy / norm, qz / norm, qw / norm)
    } else {
        (0.0, 0.0, 0.0, 1.0)
    };
    vec![nx, ny, nz, nw, gamma_deg, beta_deg, alpha_deg]
}

fn parse_selection_range(flags_align: &str) -> Option<(usize, usize)> {
    for part in flags_align.split(':') {
        if let Some(sel) = part.strip_prefix("sel=")
            && let Some((a, b)) = sel.split_once(',')
            && let (Ok(i1), Ok(i2)) = (a.parse::<usize>(), b.parse::<usize>())
        {
            let lo = i1.min(i2);
            let hi = i1.max(i2);
            if lo < hi {
                return Some((lo, hi));
            }
        }
    }
    None
}

pub fn rasterize_text_rgba(
    width: u32,
    height: u32,
    text: &str,
    fg_rgb: u32,
    bg_argb: u32,
    font_size: u32,
    flags_align: &str,
) -> Vec<u8> {
    let w = width.clamp(1, 4096) as usize;
    let h = height.clamp(1, 4096) as usize;
    let bg_a = ((bg_argb >> 24) & 0xff) as u8;
    let bg_r = ((bg_argb >> 16) & 0xff) as u8;
    let bg_g = ((bg_argb >> 8) & 0xff) as u8;
    let bg_b = (bg_argb & 0xff) as u8;

    let fg_r = ((fg_rgb >> 16) & 0xff) as u8;
    let fg_g = ((fg_rgb >> 8) & 0xff) as u8;
    let fg_b = (fg_rgb & 0xff) as u8;

    let mut rgba = vec![0u8; w * h * 4];
    for px in rgba.as_chunks_mut::<4>().0 {
        *px = [bg_r, bg_g, bg_b, bg_a];
    }

    let password = flags_align.contains("password");
    let multiline = !flags_align.contains("single");
    let align_center = flags_align.contains("center");
    let align_right = flags_align.contains("right");
    let sel_range = parse_selection_range(flags_align);

    let scale = (font_size.max(8) / 8).clamp(1, 6) as usize;
    let adv_x = 6 * scale;
    let adv_y = 9 * scale;
    let pad = 2 * scale;
    let max_cols = ((w.saturating_sub(pad * 2)) / adv_x).max(1);

    let display_text: String = if password {
        text.chars()
            .map(|c| if c == '\n' { '\n' } else { '*' })
            .collect()
    } else {
        text.to_owned()
    };

    let mut lines: Vec<Vec<(usize, char)>> = Vec::new();
    let mut global_idx = 0usize;
    for (raw_idx, raw_line) in display_text.split('\n').enumerate() {
        if raw_idx > 0 {
            global_idx += 1; // account for '\n'
        }
        let chars: Vec<(usize, char)> = raw_line
            .chars()
            .map(|ch| {
                let idx = global_idx;
                global_idx += 1;
                (idx, ch)
            })
            .collect();
        if !multiline {
            lines.push(chars);
            break;
        }
        if chars.is_empty() {
            lines.push(Vec::new());
        } else {
            for chunk in chars.chunks(max_cols) {
                lines.push(chunk.to_vec());
            }
        }
    }

    for (line_idx, line_chars) in lines.iter().enumerate() {
        let base_y = pad + line_idx * adv_y;
        if base_y >= h {
            break;
        }
        let line_px_w = line_chars.len() * adv_x;
        let base_x = if align_center && line_px_w + pad * 2 < w {
            (w - line_px_w) / 2
        } else if align_right && line_px_w + pad < w {
            w - line_px_w - pad
        } else {
            pad
        };

        for (ci, &(char_idx, ch)) in line_chars.iter().enumerate() {
            let gx = base_x + ci * adv_x;
            if gx >= w {
                break;
            }
            let selected = sel_range.is_some_and(|(lo, hi)| char_idx >= lo && char_idx < hi);
            if selected {
                for py in base_y..(base_y + 7 * scale).min(h) {
                    for px in gx..(gx + adv_x).min(w) {
                        let off = (py * w + px) * 4;
                        rgba[off] = 0x33;
                        rgba[off + 1] = 0x90;
                        rgba[off + 2] = 0xff;
                        rgba[off + 3] = 0xff;
                    }
                }
            }
            let rows = glyph_5x7(ch);
            for (ry, &bits) in rows.iter().enumerate() {
                for rx in 0..5usize {
                    if (bits & (1 << (4 - rx))) != 0 {
                        for sy in 0..scale {
                            let py = base_y + ry * scale + sy;
                            if py >= h {
                                continue;
                            }
                            for sx in 0..scale {
                                let px = gx + rx * scale + sx;
                                if px >= w {
                                    continue;
                                }
                                let off = (py * w + px) * 4;
                                if selected {
                                    rgba[off] = 0xff;
                                    rgba[off + 1] = 0xff;
                                    rgba[off + 2] = 0xff;
                                    rgba[off + 3] = 0xff;
                                } else {
                                    rgba[off] = fg_r;
                                    rgba[off + 1] = fg_g;
                                    rgba[off + 2] = fg_b;
                                    rgba[off + 3] = 0xff;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn air_path_resolution_urls_and_relative_paths_follow_air_contract() {
        assert_eq!(
            normalize_air_path("app:/data/levels/1.json").unwrap(),
            "/app/data/levels/1.json"
        );
        assert_eq!(
            normalize_air_path("app-storage:/prefs/../save.bin").unwrap(),
            "/app-storage/save.bin"
        );
        assert_eq!(
            normalize_air_path("documents:/Notes/todo.txt").unwrap(),
            "/user/Documents/Notes/todo.txt"
        );
        assert_eq!(
            normalize_air_path("file:///user/Desktop/my%20file.png").unwrap(),
            "/user/Desktop/my file.png"
        );
        assert_eq!(
            resolve_air_path("/app-storage/profiles", "slot1/data.sav").unwrap(),
            "/app-storage/profiles/slot1/data.sav"
        );
        assert_eq!(
            relative_air_path("/user/Documents", "/user/Documents/work/report.txt", false).unwrap(),
            Some("work/report.txt".to_owned())
        );
        assert_eq!(
            relative_air_path("/user/Documents", "/user/Desktop/icon.png", false).unwrap(),
            None
        );
        assert_eq!(
            relative_air_path("/user/Documents", "/user/Desktop/icon.png", true).unwrap(),
            Some("../Desktop/icon.png".to_owned())
        );
        assert_eq!(air_url_for_native_path("/app"), "app:/");
        assert_eq!(
            air_url_for_native_path("/app/assets/hero.png"),
            "app:/assets/hero.png"
        );
        assert_eq!(
            air_url_for_native_path("/app-storage/db/main.db"),
            "app-storage:/db/main.db"
        );
    }

    #[test]
    fn air_virtual_filesystem_protects_app_and_supports_tree_operations() {
        let res = vec![AirResourceInit {
            path: "config/settings.txt".into(),
            data_base64: BASE64.encode(b"hello-air"),
        }];
        let mut fs = AirFileSystem::new(&res, 1_700_000_000_000.0).unwrap();

        let app_file = fs.info("/app/config/settings.txt").unwrap();
        assert!(app_file.exists);
        assert!(!app_file.is_directory);
        assert_eq!(app_file.size, 9);
        assert_eq!(app_file.extension.as_deref(), Some("txt"));
        assert!(!app_file.writable);
        assert_eq!(
            fs.read_bytes("/app/config/settings.txt").unwrap(),
            b"hello-air"
        );

        // Writing or deleting under app:/ fails with SecurityError
        assert!(matches!(
            fs.write_bytes("/app/new.txt", b"x", 1.0),
            Err(AirError::Security(_))
        ));
        assert!(matches!(
            fs.delete_file("/app/config/settings.txt"),
            Err(AirError::Security(_))
        ));

        // Copy from app:/ into app-storage:/ works and becomes writable
        fs.copy_to("/app/config", "/app-storage/backup", false, 2000.0)
            .unwrap();
        assert_eq!(
            fs.read_bytes("/app-storage/backup/settings.txt").unwrap(),
            b"hello-air"
        );
        fs.move_to(
            "/app-storage/backup/settings.txt",
            "/app-storage/backup/renamed.txt",
            false,
            3000.0,
        )
        .unwrap();
        assert_eq!(
            fs.list_directory("/app-storage/backup").unwrap(),
            vec!["/app-storage/backup/renamed.txt".to_owned()]
        );
        // Non-empty deleteDirectory(false) fails, deleteDirectory(true) succeeds
        assert!(fs.delete_directory("/app-storage/backup", false).is_err());
        fs.delete_directory("/app-storage/backup", true).unwrap();
        assert!(!fs.info("/app-storage/backup").unwrap().exists);

        // Temp file & directory creation
        let tmp_f = fs.create_temp(false, 4000.0).unwrap();
        let tmp_d = fs.create_temp(true, 4000.0).unwrap();
        assert!(fs.info(&tmp_f).unwrap().exists);
        assert!(fs.info(&tmp_d).unwrap().is_directory);
    }

    #[test]
    fn encrypted_local_store_roundtrips_and_resets() {
        let mut els = AirEncryptedStore::default();
        els.set_item("token", &[1, 2, 3, 255]).unwrap();
        assert_eq!(els.get_item("token").unwrap(), Some(vec![1, 2, 3, 255]));
        els.remove_item("token").unwrap();
        assert_eq!(els.get_item("token").unwrap(), None);
        els.set_item("k1", b"abc").unwrap();
        els.reset();
        assert_eq!(els.get_item("k1").unwrap(), None);
    }

    #[test]
    fn sql_engine_executes_ddl_dml_joins_aggregates_transactions_savepoints_and_persistence() {
        let mut rt = AirRuntime::new("AIR Inventory", 800, 600, &[], 1000.0).unwrap();
        assert_eq!(rt.app_id, "org.neoflash.air-inventory");
        assert!(rt.descriptor_xml.contains("<width>800</width>"));

        let cid = rt
            .sql_open(Some("app-storage:/store.db"), "create", false, 1024, 1000.0)
            .unwrap();
        rt.sql_execute_json(
            cid,
            "CREATE TABLE items (id INTEGER PRIMARY KEY AUTOINCREMENT, category TEXT NOT NULL, price REAL DEFAULT 0, name TEXT COLLATE NOCASE UNIQUE)",
            "{}",
            1001.0,
        )
        .unwrap();
        rt.sql_execute_json(
            cid,
            "CREATE INDEX idx_items_cat ON items (category)",
            "{}",
            1002.0,
        )
        .unwrap();

        let p1 = serde_json::json!({
            ":cat": {"k": "Text", "v": "tools"},
            ":price": {"k": "Real", "v": 12.5},
            ":name": {"k": "Text", "v": "Hammer"}
        })
        .to_string();
        let res1: serde_json::Value = serde_json::from_str(
            &rt.sql_execute_json(
                cid,
                "INSERT INTO items (category, price, name) VALUES (:cat, :price, :name)",
                &p1,
                1003.0,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(res1["rowsAffected"], 1);
        assert_eq!(res1["lastInsertRowID"], 1);

        rt.sql_execute_json(
            cid,
            "INSERT INTO items (category, price, name) VALUES ('tools', 7.5, 'Wrench'), ('books', 20.0, 'Manual')",
            "{}",
            1004.0,
        )
        .unwrap();

        // Savepoint + rollback to savepoint inside transaction
        rt.sql_begin(cid, Some("immediate")).unwrap();
        rt.sql_set_savepoint(cid, Some("sp1")).unwrap();
        rt.sql_execute_json(
            cid,
            "DELETE FROM items WHERE category = 'books'",
            "{}",
            1005.0,
        )
        .unwrap();
        rt.sql_rollback_to_savepoint(cid, Some("sp1")).unwrap();
        rt.sql_execute_json(
            cid,
            "UPDATE items SET price = price + 2.5 WHERE name = 'hammer'",
            "{}",
            1006.0,
        )
        .unwrap();
        rt.sql_commit(cid, 1007.0).unwrap();

        // Close and reopen from virtual FS in READ mode to verify persistence!
        rt.sql_close(cid).unwrap();
        let cid2 = rt
            .sql_open(Some("/app-storage/store.db"), "read", false, 1024, 2000.0)
            .unwrap();

        let sel: serde_json::Value = serde_json::from_str(
            &rt.sql_execute_json(
                cid2,
                "SELECT category, COUNT(*) AS cnt, SUM(price) AS total FROM items GROUP BY category ORDER BY total DESC",
                "{}",
                2001.0,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            sel["columns"],
            serde_json::json!(["category", "cnt", "total"])
        );
        assert_eq!(sel["rows"].as_array().unwrap().len(), 2);
        assert_eq!(sel["rows"][0][0]["v"], "tools");
        assert_eq!(sel["rows"][0][1]["v"], 2);
        assert_eq!(sel["rows"][0][2]["v"], 22.5);

        // Schema introspection returns table + columns + index
        let schema: serde_json::Value = serde_json::from_str(
            &rt.sql_load_schema_json(cid2, None, None, "main", true)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(schema["tables"][0]["name"], "items");
        assert_eq!(schema["tables"][0]["columns"].as_array().unwrap().len(), 4);
        assert_eq!(schema["indices"][0]["name"], "idx_items_cat");

        // Writing to a READ mode connection fails with errorId 3132
        let ro_err = rt
            .sql_execute_json(cid2, "DELETE FROM items", "{}", 2002.0)
            .unwrap_err();
        assert_eq!(ro_err.error_id, 3132);
    }

    #[test]
    fn sql_encrypted_database_roundtrips_rejects_wrong_key_and_reencrypts() {
        let mut rt = AirRuntime::new("AIR Vault", 640, 480, &[], 1000.0).unwrap();
        let key1 = b"0123456789abcdef";
        let key2 = b"fedcba9876543210";
        let bad_short = b"short";

        assert_eq!(
            rt.sql_open_with_key(
                Some("app-storage:/vault.db"),
                "create",
                false,
                1024,
                Some(bad_short),
                1000.0
            )
            .unwrap_err()
            .error_id,
            3143
        );

        let cid = rt
            .sql_open_with_key(
                Some("app-storage:/vault.db"),
                "create",
                false,
                1024,
                Some(key1),
                1000.0,
            )
            .unwrap();
        rt.sql_execute_json(
            cid,
            "CREATE TABLE secrets (id INTEGER PRIMARY KEY, code TEXT)",
            "{}",
            1001.0,
        )
        .unwrap();
        rt.sql_execute_json(
            cid,
            "INSERT INTO secrets (id, code) VALUES (1, 'alpha-7')",
            "{}",
            1002.0,
        )
        .unwrap();
        rt.sql_close(cid).unwrap();

        // Raw file bytes on VFS are encrypted and do not contain plaintext
        let raw_vfs = rt.fs.read_bytes("/app-storage/vault.db").unwrap();
        assert!(raw_vfs.starts_with(SQL_ENC_VFS_MAGIC));
        assert!(!String::from_utf8_lossy(&raw_vfs).contains("alpha-7"));

        // Opening without key or with wrong key fails with errorId 3138
        assert_eq!(
            rt.sql_open(Some("app-storage:/vault.db"), "read", false, 1024, 1010.0)
                .unwrap_err()
                .error_id,
            3138
        );
        assert_eq!(
            rt.sql_open_with_key(
                Some("app-storage:/vault.db"),
                "read",
                false,
                1024,
                Some(key2),
                1010.0
            )
            .unwrap_err()
            .error_id,
            3138
        );

        // Reopen with key1 in update mode and reencrypt to key2
        let cid_re = rt
            .sql_open_with_key(
                Some("app-storage:/vault.db"),
                "update",
                false,
                1024,
                Some(key1),
                1020.0,
            )
            .unwrap();
        rt.sql_reencrypt(cid_re, key2, 1021.0).unwrap();
        rt.sql_close(cid_re).unwrap();

        // Old key1 now fails; new key2 succeeds and reads rows
        assert_eq!(
            rt.sql_open_with_key(
                Some("app-storage:/vault.db"),
                "read",
                false,
                1024,
                Some(key1),
                1030.0
            )
            .unwrap_err()
            .error_id,
            3138
        );
        let cid_ok = rt
            .sql_open_with_key(
                Some("app-storage:/vault.db"),
                "read",
                false,
                1024,
                Some(key2),
                1031.0,
            )
            .unwrap();
        let res: serde_json::Value = serde_json::from_str(
            &rt.sql_execute_json(cid_ok, "SELECT code FROM secrets", "{}", 1032.0)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(res["rows"][0][0]["v"], "alpha-7");
    }

    #[test]
    fn html_network_xmlsig_and_text_rasterizer_engines_follow_air_contracts() {
        assert_eq!(
            sha1_bytes(b"abc"),
            [
                0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71, 0x78, 0x50,
                0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d
            ]
        );
        assert_eq!(sha256_bytes(b"abc")[..4], [0xba, 0x78, 0x16, 0xbf]);

        let res = vec![AirResourceInit {
            path: "docs/index.html".into(),
            data_base64: BASE64.encode(
                b"<html><head><title>App Help</title></head><body><h1 id=\"hdr\">Welcome</h1><a href=\"app:/docs/p2.html\">Next</a></body></html>",
            ),
        }];
        let mut rt = AirRuntime::new("AIR Suite", 800, 600, &res, 1000.0).unwrap();

        // HTML session & history navigation
        let hid = rt.html.create_session(400.0, 300.0);
        let st1: serde_json::Value = serde_json::from_str(
            &rt.html_load_url(hid, "app:/docs/index.html", false, None)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(st1["title"], "App Help");
        assert_eq!(st1["hasFocusableContent"], true);
        assert_eq!(st1["historyLength"], 1);

        let st2: serde_json::Value = serde_json::from_str(
            &rt.html
                .load_string(hid, "<title>Second</title><p>Body 2</p>", "about:second")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(st2["title"], "Second");
        assert_eq!(st2["historyLength"], 2);
        assert_eq!(st2["historyPosition"], 1);

        let back: serde_json::Value =
            serde_json::from_str(&rt.html.history_go(hid, -1).unwrap()).unwrap();
        assert_eq!(back["title"], "App Help");
        assert_eq!(back["historyPosition"], 0);

        // UDP DatagramSocket + TCP ServerSocket loopback
        let u1 = rt.net.udp_create();
        let u2 = rt.net.udp_create();
        let b2: serde_json::Value =
            serde_json::from_str(&rt.net.udp_bind(u2, 0, "127.0.0.1").unwrap()).unwrap();
        let port2 = b2["localPort"].as_u64().unwrap() as u32;
        rt.net.udp_receive(u2).unwrap();
        let send_res: serde_json::Value =
            serde_json::from_str(&rt.net.udp_send(u1, Some("127.0.0.1"), port2).unwrap()).unwrap();
        assert_eq!(send_res["deliveries"].as_array().unwrap().len(), 1);
        assert_eq!(send_res["deliveries"][0]["targetSockId"], u2);

        let srv = rt.net.tcp_server_create();
        let sb: serde_json::Value =
            serde_json::from_str(&rt.net.tcp_server_bind(srv, 0, "127.0.0.1").unwrap()).unwrap();
        let srv_port = sb["localPort"].as_u64().unwrap() as u32;
        rt.net.tcp_server_listen(srv, 5).unwrap();
        let conn_info: serde_json::Value =
            serde_json::from_str(&rt.net.tcp_connect_loopback("127.0.0.1", srv_port)).unwrap();
        assert_eq!(conn_info["matched"], true);
        let client_cid = conn_info["clientConnId"].as_u64().unwrap() as u32;
        let accept_cid = conn_info["acceptedConnId"].as_u64().unwrap() as u32;
        assert_eq!(rt.net.tcp_write(client_cid, b"ping").unwrap(), accept_cid);
        assert_eq!(rt.net.tcp_read(accept_cid), b"ping");

        // XMLSignatureValidator real SHA-256 + HMAC-SHA256 verification & tamper detection
        let cert = b"CN=NeoFlash Release, O=NeoFlash";
        rt.xml_sig.add_certificate(cert, true).unwrap();
        let payload = b"<order id='42'>approved</order>";
        let digest_b64 = BASE64.encode(sha256_bytes(payload));
        let signed_info = format!(
            "<SignedInfo><CanonicalizationMethod Algorithm=\"http://www.w3.org/TR/2001/REC-xml-c14n-20010315\"/><SignatureMethod Algorithm=\"http://www.w3.org/2001/04/xmldsig-more#hmac-sha256\"/><Reference URI=\"urn:order:42\"><DigestMethod Algorithm=\"http://www.w3.org/2001/04/xmlenc#sha256\"/><DigestValue>{digest_b64}</DigestValue></Reference></SignedInfo>"
        );
        let canon = canonicalize_xml_fragment(&signed_info);
        let sig_val = BASE64.encode(hmac_sha256(cert, canon.as_bytes()));
        let cert_b64 = BASE64.encode(cert);
        let xml_sig = format!(
            "<Signature>{signed_info}<SignatureValue>{sig_val}</SignatureValue><KeyInfo><X509Data><X509Certificate>{cert_b64}</X509Certificate></X509Data></KeyInfo></Signature>"
        );
        let deref_ok = serde_json::json!({
            "urn:order:42": BASE64.encode(payload)
        })
        .to_string();
        let verified: serde_json::Value = serde_json::from_str(&rt.xml_sig.verify_json(
            &xml_sig,
            &deref_ok,
            "validIdentity",
            "bestEffort",
            false,
        ))
        .unwrap();
        assert_eq!(verified["validityStatus"], "valid");
        assert_eq!(verified["digestStatus"], "valid");
        assert_eq!(verified["referencesStatus"], "valid");
        assert_eq!(verified["identityStatus"], "valid");
        assert_eq!(verified["signerCN"], "NeoFlash Release");

        let deref_tampered = serde_json::json!({
            "urn:order:42": BASE64.encode(b"<order id='42'>tampered</order>")
        })
        .to_string();
        let tampered: serde_json::Value = serde_json::from_str(&rt.xml_sig.verify_json(
            &xml_sig,
            &deref_tampered,
            "validIdentity",
            "bestEffort",
            false,
        ))
        .unwrap();
        assert_eq!(tampered["referencesStatus"], "invalid");
        assert_eq!(tampered["validityStatus"], "invalid");

        // Tampering with <SignedInfo> without re-signing must fail digestStatus!
        let tampered_si_xml = xml_sig.replace("urn:order:42", "urn:order:99");
        let tampered_si: serde_json::Value = serde_json::from_str(&rt.xml_sig.verify_json(
            &tampered_si_xml,
            &deref_ok,
            "validIdentity",
            "bestEffort",
            false,
        ))
        .unwrap();
        assert_eq!(tampered_si["digestStatus"], "invalid");
        assert_eq!(tampered_si["validityStatus"], "invalid");

        // W3C Canonical XML 1.0 sorts attributes and expands self-closing tags
        assert_eq!(
            canonicalize_xml_fragment("<Ref z='2' a=\"1\"/>"),
            "<Ref a=\"1\" z=\"2\"></Ref>"
        );

        // DeviceRotation quaternion calculation produces normalized unit quaternion
        let q = device_rotation_quaternion(90.0, 30.0, -15.0);
        assert_eq!(q.len(), 7);
        let norm_sq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
        assert!((norm_sq - 1.0).abs() < 1e-9);
        assert_eq!(q[4], -15.0);
        assert_eq!(q[5], 30.0);
        assert_eq!(q[6], 90.0);

        // Text rasterizer produces foreground glyph pixels and selection highlight
        let rgba = rasterize_text_rgba(64, 24, "AIR", 0x112233, 0xFFFFFFFF, 16, "left:sel=0,2");
        assert_eq!(rgba.len(), 64 * 24 * 4);
        assert!(rgba.as_chunks::<4>().0.contains(&[0x11, 0x22, 0x33, 0xff]));
        assert!(rgba.as_chunks::<4>().0.contains(&[0x33, 0x90, 0xff, 0xff]));
    }

    #[test]
    fn x509_der_oid_parsing_handles_long_lengths_issuer_vs_subject_and_nested_xml_blocks() {
        // 1. False-positive OID bytes without preceding 0x06, 0x03 tag/length are ignored,
        //    and long-form DER string length (0x81, 130) is decoded properly.
        let long_cn = "A".repeat(130);
        let mut der_long = vec![
            0x04, 0x05, 0x55, 0x04, 0x03, 0x0c, 0x03, b'B', b'A',
            b'D', // Not an OID tag (0x04)
            0x06, 0x03, 0x55, 0x04, 0x03, // OID 2.5.4.3 (CommonName)
            0x0c, 0x81, 130, // UTF8String with 1-byte long-form length = 130
        ];
        der_long.extend_from_slice(long_cn.as_bytes());
        assert_eq!(
            extract_der_oid_string(&der_long, &[0x55, 0x04, 0x03]).as_deref(),
            Some(long_cn.as_str())
        );

        // 2. X.509 certificate with distinct Issuer RDNSequence (first) and Subject RDNSequence (second)
        let mut cert_der = Vec::new();
        // Issuer CN = "NeoFlash Root CA", O = "NeoFlash Trust"
        cert_der.extend_from_slice(&[0x06, 0x03, 0x55, 0x04, 0x03, 0x0c, 16]);
        cert_der.extend_from_slice(b"NeoFlash Root CA");
        cert_der.extend_from_slice(&[0x06, 0x03, 0x55, 0x04, 0x0a, 0x0c, 14]);
        cert_der.extend_from_slice(b"NeoFlash Trust");
        // Subject CN = "NeoFlash App Signer", O = "NeoFlash Labs"
        cert_der.extend_from_slice(&[0x06, 0x03, 0x55, 0x04, 0x03, 0x0c, 19]);
        cert_der.extend_from_slice(b"NeoFlash App Signer");
        cert_der.extend_from_slice(&[0x06, 0x03, 0x55, 0x04, 0x0a, 0x0c, 13]);
        cert_der.extend_from_slice(b"NeoFlash Labs");

        let meta = parse_cert_metadata(&cert_der);
        assert_eq!(meta.issuer_cn, "NeoFlash Root CA");
        assert_eq!(meta.issuer_dn, "CN=NeoFlash Root CA, O=NeoFlash Trust");
        assert_eq!(meta.cn, "NeoFlash App Signer");
        assert_eq!(meta.dn, "CN=NeoFlash App Signer, O=NeoFlash Labs");

        // 3. Nested same-named <Object> tags in extract_all_xml_tag_blocks
        let nested_xml = "<Signature><Object Id=\"outer\"><Object Id=\"inner\">payload</Object><Extra/></Object><Object Id=\"second\">two</Object></Signature>";
        let blocks = extract_all_xml_tag_blocks(nested_xml, "Object");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].0.trim(), "Id=\"outer\"");
        assert_eq!(blocks[0].1, "<Object Id=\"inner\">payload</Object><Extra/>");
        assert_eq!(blocks[1].0.trim(), "Id=\"second\"");
        assert_eq!(blocks[1].1, "two");
    }

    #[test]
    fn rfc8439_chacha20_vector_and_cross_runtime_unique_sql_encryption_nonces() {
        // RFC 8439 §2.4.2 test vector
        let mut rfc_key = [0u8; 32];
        for (i, b) in rfc_key.iter_mut().enumerate() {
            *b = i as u8;
        }
        let rfc_nonce: [u8; 12] = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x4a, 0x00, 0x00, 0x00, 0x00,
        ];
        let ladies = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let ct = chacha20_xor(&rfc_key, &rfc_nonce, 1, ladies);
        assert_eq!(
            &ct[..16],
            &[
                0x6e, 0x2e, 0x35, 0x9a, 0x25, 0x68, 0xf9, 0x80, 0x41, 0xba, 0x07, 0x28, 0xdd, 0x0d,
                0x69, 0x81,
            ]
        );
        let rt_plain = chacha20_xor(&rfc_key, &rfc_nonce, 1, &ct);
        assert_eq!(rt_plain.as_slice(), ladies);

        // Two separate AirRuntime instances encrypting identical payloads with the same key
        // and same initial nonce_seed must produce distinct salts, nonces, and ciphertexts.
        let key: [u8; 16] = *b"0123456789abcdef";
        let payload = b"{\"tables\":{\"secrets\":true}}";
        let enc_a = encrypt_sql_payload(&key, 0, payload);
        let enc_b = encrypt_sql_payload(&key, 0, payload);
        assert_ne!(enc_a, enc_b);
        let body_a = enc_a.strip_prefix(SQL_ENC_VFS_MAGIC).unwrap();
        let body_b = enc_b.strip_prefix(SQL_ENC_VFS_MAGIC).unwrap();
        assert!(body_a.starts_with(SQL_ENC_V2_HEADER));
        assert!(body_b.starts_with(SQL_ENC_V2_HEADER));
        // Salt (16B) + Nonce (12B) right after "CV2\0" must differ
        assert_ne!(&body_a[4..32], &body_b[4..32]);
        assert_eq!(decrypt_sql_payload(&key, body_a, "open").unwrap(), payload);
        assert_eq!(decrypt_sql_payload(&key, body_b, "open").unwrap(), payload);
    }

    #[test]
    fn legacy_sql_payload_with_v2_like_salt_still_decrypts() {
        let key: [u8; 16] = *b"0123456789abcdef";
        let plaintext = b"legacy collision";
        let mut salt = [0u8; 16];
        salt[..4].copy_from_slice(SQL_ENC_V2_HEADER);
        salt[4..].copy_from_slice(b"legacy-salt!");

        let mut ciphertext = Vec::with_capacity(plaintext.len());
        for (block_idx, chunk) in plaintext.chunks(32).enumerate() {
            let mut ctr_msg = [0u8; 28];
            ctr_msg[..4].copy_from_slice(b"enc\0");
            ctr_msg[4..20].copy_from_slice(&salt);
            ctr_msg[20..28].copy_from_slice(&(block_idx as u64).to_be_bytes());
            let stream = hmac_sha256(&key, &ctr_msg);
            ciphertext.extend(chunk.iter().enumerate().map(|(i, byte)| byte ^ stream[i]));
        }
        let mut mac_msg = Vec::new();
        mac_msg.extend_from_slice(b"mac\0");
        mac_msg.extend_from_slice(&salt);
        mac_msg.extend_from_slice(&ciphertext);
        let tag = hmac_sha256(&key, &mac_msg);
        let mut body = salt.to_vec();
        body.extend_from_slice(&ciphertext);
        body.extend_from_slice(&tag);

        assert!(body.starts_with(SQL_ENC_V2_HEADER));
        assert_eq!(decrypt_sql_payload(&key, &body, "open").unwrap(), plaintext);
    }
}
