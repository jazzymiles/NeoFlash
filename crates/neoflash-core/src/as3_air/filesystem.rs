use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum VfsNode {
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

pub(super) fn percent_decode(raw: &str) -> Result<String, AirError> {
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

pub(super) fn percent_encode_path(path: &str) -> String {
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

pub(super) fn parent_of(native_path: &str) -> Option<String> {
    if native_path == "/" {
        return None;
    }
    match native_path.rfind('/') {
        Some(0) => Some("/".to_owned()),
        Some(idx) => Some(native_path[..idx].to_owned()),
        None => None,
    }
}

pub(super) fn basename_of(native_path: &str) -> &str {
    if native_path == "/" {
        ""
    } else {
        native_path.rsplit('/').next().unwrap_or(native_path)
    }
}

pub(super) fn extension_of(name: &str) -> Option<String> {
    if let Some(dot) = name.rfind('.')
        && dot > 0
        && dot + 1 < name.len()
    {
        return Some(name[dot + 1..].to_owned());
    }
    None
}

pub(super) fn is_under_readonly_root(path: &str) -> bool {
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
