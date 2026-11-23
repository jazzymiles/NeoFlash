//! `SwiftHost` native bridge contract extractor and validator shared between Swift 6 codegen
//! and live editor target guardrails (`warnings.rs`).

use super::hir::{ParsedClass, short_type_name};
use crate::byte_offset_to_line_col;
use crate::warnings::{CompilerWarning, DiagnosticSeverity};
use neoflash_core::ScriptLanguage;
use serde::{Deserialize, Serialize};

/// A single parameter in a `SwiftHost` static proxy method signature.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwiftProxyParam {
    pub name: String,
    pub swift_type: String,
    pub as_type: String,
}

/// A static method declared on `SwiftHost` that bridges into `@MainActor protocol SwiftHostProtocol`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwiftProxyMethod {
    pub name: String,
    pub params: Vec<SwiftProxyParam>,
    pub return_swift_type: Option<String>,
}

pub(crate) const PORTABLE_TYPES_LIST: &str =
    "`int`, `uint`, `Number`, `Boolean`, `String`, `Array`, `Vector.<T>`, `Object`";

/// Map a portable ActionScript data type annotation to its native Swift 6 equivalent.
/// Returns `None` if `raw` is untyped (`*` / empty) or non-portable (`MovieClip`, `Sprite`, etc.).
pub(crate) fn map_portable_as_type_to_swift(raw: &str) -> Option<(&'static str, &'static str)> {
    let trimmed = raw.trim();
    if let Some(elem) = trimmed
        .strip_prefix("Vector.<")
        .or_else(|| trimmed.strip_prefix("__AS3__.vec.Vector.<"))
        .and_then(|s| s.strip_suffix('>'))
    {
        return match short_type_name(elem.trim()) {
            "Number" => Some(("[Double]", "Vector.<Number>")),
            "int" => Some(("[Int]", "Vector.<int>")),
            "uint" => Some(("[UInt]", "Vector.<uint>")),
            "Boolean" => Some(("[Bool]", "Vector.<Boolean>")),
            "String" => Some(("[String]", "Vector.<String>")),
            "*" | "Object" | "Array" => Some(("[NFValue]", "Array")),
            _ => None,
        };
    }
    match short_type_name(trimmed) {
        "int" => Some(("Int", "int")),
        "uint" => Some(("UInt", "uint")),
        "Number" => Some(("Double", "Number")),
        "Boolean" => Some(("Bool", "Boolean")),
        "String" => Some(("String", "String")),
        "Array" => Some(("[NFValue]", "Array")),
        "Object" => Some(("[String: NFValue]", "Object")),
        _ => None,
    }
}

/// Check whether `unit_path` or `parsed` represents the `SwiftHost` proxy contract class.
pub(crate) fn is_swift_host_class(unit_path: Option<&str>, parsed: &ParsedClass) -> bool {
    if parsed.short_name == "SwiftHost" {
        return true;
    }
    if let Some(p) = unit_path {
        let norm = p.replace('\\', "/");
        if norm == "SwiftHost.as" || norm.ends_with("/SwiftHost.as") {
            return true;
        }
    }
    false
}

/// Extract `SwiftProxyMethod` declarations and collect structured `CompilerWarning`s for any
/// contract violations in `SwiftHost.as`.
pub(crate) fn inspect_swift_host_class(
    parsed: &ParsedClass,
    language: ScriptLanguage,
    path: Option<&str>,
) -> (Vec<SwiftProxyMethod>, Vec<CompilerWarning>) {
    let mut methods = Vec::new();
    let mut warnings = Vec::new();
    let src = &parsed.source;
    let path_owned = path.map(str::to_owned);

    for m in &parsed.methods {
        let Some(fname) = m.function.name.as_deref() else {
            continue;
        };
        // Ignore an empty default constructor `public function SwiftHost() {}` if present
        if fname == parsed.short_name
            && m.accessor.is_none()
            && !m.is_static
            && m.function.body.is_empty()
        {
            continue;
        }

        if !m.is_static || m.accessor.is_some() {
            let span = sanitize_span(src, m.name_span);
            let (line, column) = byte_offset_to_line_col(src, span.0);
            warnings.push(CompilerWarning {
                code: "swift-host-contract".into(),
                severity: DiagnosticSeverity::Warning,
                language,
                path: path_owned.clone(),
                line,
                column,
                span,
                message: format!(
                    "SwiftHost method '{fname}' must be declared 'static'. SwiftHost proxies cross into native Swift (@MainActor protocol SwiftHostProtocol) and only support static methods accepting portable data types ({PORTABLE_TYPES_LIST})."
                ),
                quick_fixes: Vec::new(),
            });
            continue;
        }

        let mut proxy_params = Vec::new();
        for (idx, (pname, pty_opt)) in m.function.params.iter().enumerate() {
            let p_span = m.param_type_spans.get(idx).copied().unwrap_or(m.name_span);
            let span = sanitize_span(src, p_span);
            let (line, column) = byte_offset_to_line_col(src, span.0);

            match pty_opt.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                None | Some("*") => {
                    warnings.push(CompilerWarning {
                        code: "swift-host-contract".into(),
                        severity: DiagnosticSeverity::Warning,
                        language,
                        path: path_owned.clone(),
                        line,
                        column,
                        span,
                        message: format!(
                            "Parameter '{pname}' of SwiftHost.{fname}() is untyped. SwiftHost proxies cross into native Swift and only accept portable data types ({PORTABLE_TYPES_LIST})."
                        ),
                        quick_fixes: Vec::new(),
                    });
                    proxy_params.push(SwiftProxyParam {
                        name: pname.clone(),
                        swift_type: "NFValue".into(),
                        as_type: "*".into(),
                    });
                }
                Some(raw_ty) => {
                    if let Some((swift_ty, canonical_as)) = map_portable_as_type_to_swift(raw_ty) {
                        proxy_params.push(SwiftProxyParam {
                            name: pname.clone(),
                            swift_type: swift_ty.to_owned(),
                            as_type: canonical_as.to_owned(),
                        });
                    } else {
                        warnings.push(CompilerWarning {
                            code: "swift-host-contract".into(),
                            severity: DiagnosticSeverity::Warning,
                            language,
                            path: path_owned.clone(),
                            line,
                            column,
                            span,
                            message: format!(
                                "Parameter '{pname}' of SwiftHost.{fname}() uses non-portable type '{raw_ty}'. SwiftHost proxies cross into native Swift and only accept portable data types ({PORTABLE_TYPES_LIST})."
                            ),
                            quick_fixes: Vec::new(),
                        });
                        proxy_params.push(SwiftProxyParam {
                            name: pname.clone(),
                            swift_type: "NFValue".into(),
                            as_type: raw_ty.to_owned(),
                        });
                    }
                }
            }
        }

        if let Some(rest) = &m.function.rest {
            let span = sanitize_span(src, m.name_span);
            let (line, column) = byte_offset_to_line_col(src, span.0);
            warnings.push(CompilerWarning {
                code: "swift-host-contract".into(),
                severity: DiagnosticSeverity::Warning,
                language,
                path: path_owned.clone(),
                line,
                column,
                span,
                message: format!(
                    "Rest parameter '...{rest}' on SwiftHost.{fname}() is not portable across SwiftHostProtocol; pass an explicit 'Array' parameter instead ({PORTABLE_TYPES_LIST})."
                ),
                quick_fixes: Vec::new(),
            });
        }

        let return_swift_type = match m.function.returns.as_deref().map(str::trim) {
            None | Some("") => {
                let span = sanitize_span(src, m.name_span);
                let (line, column) = byte_offset_to_line_col(src, span.0);
                warnings.push(CompilerWarning {
                    code: "swift-host-contract".into(),
                    severity: DiagnosticSeverity::Warning,
                    language,
                    path: path_owned.clone(),
                    line,
                    column,
                    span,
                    message: format!(
                        "SwiftHost.{fname}() is missing an explicit return type annotation. Declare ':void' / ':Void' or a portable return type ({PORTABLE_TYPES_LIST})."
                    ),
                    quick_fixes: Vec::new(),
                });
                None
            }
            Some("void" | "Void") => None,
            Some("*") => {
                let r_span = m.return_type_span.unwrap_or(m.name_span);
                let span = sanitize_span(src, r_span);
                let (line, column) = byte_offset_to_line_col(src, span.0);
                warnings.push(CompilerWarning {
                    code: "swift-host-contract".into(),
                    severity: DiagnosticSeverity::Warning,
                    language,
                    path: path_owned.clone(),
                    line,
                    column,
                    span,
                    message: format!(
                        "SwiftHost.{fname}() has untyped '*' return type. SwiftHost proxies cross into native Swift and only accept ':void' or portable data types ({PORTABLE_TYPES_LIST})."
                    ),
                    quick_fixes: Vec::new(),
                });
                Some("NFValue".into())
            }
            Some(raw_ret) => {
                if let Some((swift_ty, _)) = map_portable_as_type_to_swift(raw_ret) {
                    Some(swift_ty.to_owned())
                } else {
                    let r_span = m.return_type_span.unwrap_or(m.name_span);
                    let span = sanitize_span(src, r_span);
                    let (line, column) = byte_offset_to_line_col(src, span.0);
                    warnings.push(CompilerWarning {
                        code: "swift-host-contract".into(),
                        severity: DiagnosticSeverity::Warning,
                        language,
                        path: path_owned.clone(),
                        line,
                        column,
                        span,
                        message: format!(
                            "SwiftHost.{fname}() returns non-portable type '{raw_ret}'. SwiftHost proxies cross into native Swift and only accept ':void' or portable data types ({PORTABLE_TYPES_LIST})."
                        ),
                        quick_fixes: Vec::new(),
                    });
                    Some("NFValue".into())
                }
            }
        };

        methods.push(SwiftProxyMethod {
            name: fname.to_owned(),
            params: proxy_params,
            return_swift_type,
        });
    }

    (methods, warnings)
}

fn sanitize_span(src: &str, raw: (usize, usize)) -> (usize, usize) {
    let start = raw.0.min(src.len());
    let end = raw.1.min(src.len()).max(start);
    let safe_start = if src.is_char_boundary(start) {
        start
    } else {
        0
    };
    let safe_end = if src.is_char_boundary(end) {
        end
    } else {
        safe_start
    };
    (safe_start, safe_end)
}
