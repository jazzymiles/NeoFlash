use super::*;

pub(super) fn platform_path_to_uri(
    file: &str,
    span: Span,
    raw: &str,
) -> Result<String, JsflDiagnostic> {
    if raw.is_empty() || raw.chars().any(char::is_control) {
        return Err(JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            "Invalid platform path: empty or contains control characters.",
        ));
    }
    let normalized = raw.replace('\\', "/");
    if normalized.starts_with("file:///") {
        Ok(normalized)
    } else if normalized.starts_with('/') {
        Ok(format!("file://{normalized}"))
    } else {
        Ok(format!("file:///{normalized}"))
    }
}

pub(super) fn uri_to_platform_path(
    file: &str,
    span: Span,
    uri: &str,
) -> Result<String, JsflDiagnostic> {
    if uri.is_empty() || uri.chars().any(char::is_control) {
        return Err(JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            "Invalid file URI: empty or contains control characters.",
        ));
    }
    let Some(rest) = uri.strip_prefix("file://") else {
        return Err(JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            format!("Unsupported URI scheme in '{uri}': only 'file:///' URIs are permitted."),
        ));
    };
    if !rest.starts_with('/') {
        return Err(JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            format!("Remote host file URI '{uri}' is forbidden."),
        ));
    }
    Ok(rest.to_owned())
}

pub(super) fn resolve_scoped_flfile_path(
    caps: &FlFileCapabilities,
    fs: &dyn JsflFileSystem,
    file: &str,
    span: Span,
    uri: &str,
) -> Result<PathBuf, JsflDiagnostic> {
    let Some(root) = &caps.allowed_root else {
        return Err(JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            "FLfile access denied: no allowed_root configured in FlFileCapabilities.",
        ));
    };
    let raw_path = uri_to_platform_path(file, span, uri)?;
    let path = Path::new(&raw_path);
    for comp in path.components() {
        if matches!(comp, Component::ParentDir) {
            return Err(JsflDiagnostic::new(
                file,
                span.line,
                span.column,
                format!("Path traversal ('..') is forbidden in FLfile URI '{uri}'."),
            ));
        }
    }
    let canonical_root = fs.canonicalize(root).map_err(|e| {
        JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            format!("Failed to canonicalize FLfile allowed_root: {e}"),
        )
    })?;

    // `file:///` always carries an absolute URI path. `Path::is_absolute()` is not
    // portable here: on Windows, a rooted path without a drive prefix (such as
    // `/sub`) is not considered absolute and `PathBuf::join` would discard the
    // sandbox root. Interpret the URI syntax before applying host path semantics.
    let joined = if raw_path.starts_with('/') {
        if path.starts_with(&canonical_root) || path.starts_with(root) {
            path.to_path_buf()
        } else {
            let rel = raw_path.trim_start_matches('/');
            canonical_root.join(rel)
        }
    } else {
        canonical_root.join(path)
    };

    // Walk up to the nearest existing ancestor to canonicalize and detect symlink escapes.
    let mut existing = joined.as_path();
    let mut tail_parts: Vec<&std::ffi::OsStr> = Vec::new();
    while !fs.exists(existing) && existing != canonical_root.as_path() && existing != root.as_path()
    {
        if let Some(name) = existing.file_name() {
            tail_parts.push(name);
        }
        let Some(parent) = existing.parent() else {
            break;
        };
        existing = parent;
    }
    let canonical_existing = fs.canonicalize(existing).map_err(|e| {
        JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            format!("Failed to resolve FLfile path '{uri}': {e}"),
        )
    })?;
    if !canonical_existing.starts_with(&canonical_root) {
        return Err(JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            format!("FLfile path '{uri}' escapes allowed_root via symlink or outside path."),
        ));
    }
    let mut resolved = canonical_existing;
    for part in tail_parts.into_iter().rev() {
        resolved.push(part);
    }
    let final_canon = fs.canonicalize(&resolved).map_err(|e| {
        JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            format!("Failed to canonicalize target '{uri}': {e}"),
        )
    })?;
    if !final_canon.starts_with(&canonical_root) {
        return Err(JsflDiagnostic::new(
            file,
            span.line,
            span.column,
            format!("FLfile target '{uri}' escapes allowed_root via symlink."),
        ));
    }
    Ok(final_canon)
}

impl<'a> JsflVm<'a> {
    pub(super) fn call_fl_method(
        &mut self,
        method: &str,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        match method {
            "getDocumentDOM" => Ok(JsflValue::Doc(DocHandle(0))),
            "trace" => {
                let msg = args
                    .iter()
                    .map(JsflValue::to_display_string)
                    .collect::<Vec<_>>()
                    .join(" ");
                self.push_trace(msg, span)?;
                Ok(JsflValue::Undefined)
            }
            "runScript" => {
                let uri = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                if uri.trim().is_empty() {
                    return Err(self.err(span, "fl.runScript requires a non-empty fileURI."));
                }
                if self.script_recursion >= self.sandbox.limits.max_script_recursion {
                    return Err(self.err(
                        span,
                        format!(
                            "JSFL nested fl.runScript exceeded script recursion limit ({}).",
                            self.sandbox.limits.max_script_recursion
                        ),
                    ));
                }
                let script_source = if let Some(src) = self.sandbox.virtual_scripts.get(&uri) {
                    src.clone()
                } else {
                    if !self.sandbox.flfile_caps.allow_read {
                        return Err(self.err(
                            span,
                            format!(
                                "fl.runScript('{uri}') denied: read capability is not enabled."
                            ),
                        ));
                    }
                    let path = resolve_scoped_flfile_path(
                        &self.sandbox.flfile_caps,
                        self.sandbox.fs.as_ref(),
                        &self.current_file,
                        span,
                        &uri,
                    )?;
                    self.sandbox.fs.read_file(&path).map_err(|e| {
                        self.err(span, format!("fl.runScript failed to read '{uri}': {e}"))
                    })?
                };
                if script_source.len() > self.sandbox.limits.max_source_bytes {
                    return Err(self.err(
                        span,
                        format!(
                            "fl.runScript('{uri}') exceeds maximum source size ({} bytes).",
                            self.sandbox.limits.max_source_bytes
                        ),
                    ));
                }
                let prev_file = self.current_file.clone();
                self.current_file = uri.clone();
                self.script_recursion += 1;
                let nested_tokens = tokenize(&uri, &script_source)?;
                let mut nested_parser = Parser::new(&uri, nested_tokens);
                let nested_stmts = nested_parser.parse_program()?;
                let mut res = self.run_program(&nested_stmts, span);
                if res.is_ok()
                    && let Some(func_arg) = args.get(1)
                {
                    let func_name = func_arg.to_display_string();
                    if !func_name.is_empty() && func_name != "undefined" && func_name != "null" {
                        let extra_args = if args.len() > 2 { &args[2..] } else { &[] };
                        res = self.call_global_or_user(&func_name, extra_args, span);
                    }
                }
                self.script_recursion -= 1;
                self.current_file = prev_file;
                res
            }
            _ => Err(self.err(
                span,
                format!("Unsupported Flash authoring method 'fl.{method}'."),
            )),
        }
    }

    pub(super) fn call_flfile_method(
        &mut self,
        method: &str,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        let first_str = args
            .first()
            .map(JsflValue::to_display_string)
            .unwrap_or_default();
        match method {
            "platformPathToURI" => {
                let uri = platform_path_to_uri(&self.current_file, span, &first_str)?;
                Ok(JsflValue::String(uri))
            }
            "uriToPlatformPath" => {
                let path = uri_to_platform_path(&self.current_file, span, &first_str)?;
                Ok(JsflValue::String(path))
            }
            "exists" => {
                if !self.sandbox.flfile_caps.allow_read {
                    return Err(self.err(
                        span,
                        "FLfile.exists denied: read capability is not enabled.",
                    ));
                }
                let path = resolve_scoped_flfile_path(
                    &self.sandbox.flfile_caps,
                    self.sandbox.fs.as_ref(),
                    &self.current_file,
                    span,
                    &first_str,
                )?;
                Ok(JsflValue::Bool(self.sandbox.fs.exists(&path)))
            }
            "read" => {
                if !self.sandbox.flfile_caps.allow_read {
                    return Err(
                        self.err(span, "FLfile.read denied: read capability is not enabled.")
                    );
                }
                let path = resolve_scoped_flfile_path(
                    &self.sandbox.flfile_caps,
                    self.sandbox.fs.as_ref(),
                    &self.current_file,
                    span,
                    &first_str,
                )?;
                let content = self.sandbox.fs.read_file(&path).map_err(|e| {
                    self.err(span, format!("FLfile.read('{first_str}') failed: {e}"))
                })?;
                Ok(JsflValue::String(content))
            }
            "write" => {
                if !self.sandbox.flfile_caps.allow_write {
                    return Err(self.err(
                        span,
                        "FLfile.write denied: write capability is not enabled.",
                    ));
                }
                let text = args
                    .get(1)
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let mode = args
                    .get(2)
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let path = resolve_scoped_flfile_path(
                    &self.sandbox.flfile_caps,
                    self.sandbox.fs.as_ref(),
                    &self.current_file,
                    span,
                    &first_str,
                )?;
                if mode.eq_ignore_ascii_case("append") && self.sandbox.fs.exists(&path) {
                    self.sandbox.fs.append_file(&path, &text).map_err(|e| {
                        self.err(span, format!("FLfile.write('{first_str}') failed: {e}"))
                    })?;
                } else {
                    self.sandbox.fs.write_file(&path, &text).map_err(|e| {
                        self.err(span, format!("FLfile.write('{first_str}') failed: {e}"))
                    })?;
                }
                Ok(JsflValue::Bool(true))
            }
            "createFolder" => {
                if !self.sandbox.flfile_caps.allow_write {
                    return Err(self.err(
                        span,
                        "FLfile.createFolder denied: write capability is not enabled.",
                    ));
                }
                let path = resolve_scoped_flfile_path(
                    &self.sandbox.flfile_caps,
                    self.sandbox.fs.as_ref(),
                    &self.current_file,
                    span,
                    &first_str,
                )?;
                self.sandbox.fs.create_folder(&path).map_err(|e| {
                    self.err(
                        span,
                        format!("FLfile.createFolder('{first_str}') failed: {e}"),
                    )
                })?;
                Ok(JsflValue::Bool(true))
            }
            "remove" => {
                if !self.sandbox.flfile_caps.allow_delete {
                    return Err(self.err(
                        span,
                        "FLfile.remove denied: delete capability is not enabled.",
                    ));
                }
                let path = resolve_scoped_flfile_path(
                    &self.sandbox.flfile_caps,
                    self.sandbox.fs.as_ref(),
                    &self.current_file,
                    span,
                    &first_str,
                )?;
                let removed = self.sandbox.fs.remove(&path).map_err(|e| {
                    self.err(span, format!("FLfile.remove('{first_str}') failed: {e}"))
                })?;
                Ok(JsflValue::Bool(removed))
            }
            "copy" => {
                if !self.sandbox.flfile_caps.allow_read {
                    return Err(
                        self.err(span, "FLfile.copy denied: read capability is not enabled.")
                    );
                }
                if !self.sandbox.flfile_caps.allow_write {
                    return Err(
                        self.err(span, "FLfile.copy denied: write capability is not enabled.")
                    );
                }
                let dst_str = args
                    .get(1)
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let src_path = resolve_scoped_flfile_path(
                    &self.sandbox.flfile_caps,
                    self.sandbox.fs.as_ref(),
                    &self.current_file,
                    span,
                    &first_str,
                )?;
                let dst_path = resolve_scoped_flfile_path(
                    &self.sandbox.flfile_caps,
                    self.sandbox.fs.as_ref(),
                    &self.current_file,
                    span,
                    &dst_str,
                )?;
                if !self.sandbox.fs.exists(&src_path)
                    || self.sandbox.fs.is_folder(&src_path)
                    || self.sandbox.fs.exists(&dst_path)
                {
                    return Ok(JsflValue::Bool(false));
                }
                let content = self.sandbox.fs.read_file(&src_path).map_err(|e| {
                    self.err(span, format!("FLfile.copy('{first_str}') failed: {e}"))
                })?;
                self.sandbox
                    .fs
                    .write_file(&dst_path, &content)
                    .map_err(|e| self.err(span, format!("FLfile.copy('{dst_str}') failed: {e}")))?;
                Ok(JsflValue::Bool(true))
            }
            "listFolder" => {
                if !self.sandbox.flfile_caps.allow_read {
                    return Err(self.err(
                        span,
                        "FLfile.listFolder denied: read capability is not enabled.",
                    ));
                }
                let files_only = args.get(1).is_some_and(JsflValue::is_truthy);
                let path = resolve_scoped_flfile_path(
                    &self.sandbox.flfile_caps,
                    self.sandbox.fs.as_ref(),
                    &self.current_file,
                    span,
                    &first_str,
                )?;
                let names = self
                    .sandbox
                    .fs
                    .list_folder(&path, files_only)
                    .map_err(|e| {
                        self.err(
                            span,
                            format!("FLfile.listFolder('{first_str}') failed: {e}"),
                        )
                    })?;
                Ok(JsflValue::Array(
                    names.into_iter().map(JsflValue::String).collect(),
                ))
            }
            _ => Err(self.err(
                span,
                format!("Unsupported FLfile method 'FLfile.{method}'."),
            )),
        }
    }
}
