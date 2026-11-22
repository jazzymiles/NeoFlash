//! Developer-owned `App/` shell generation and non-destructive `AppSwiftHost.swift` stub appending (`T-455`).

use super::{XcodeSwiftProxyMethod, sanitize_swift_identifier};
use std::collections::BTreeSet;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ScannedSwiftHost {
    pub declared_methods: BTreeSet<String>,
    pub closing_brace_offset: Option<usize>,
}

pub(super) fn format_swift_method_signature(method: &XcodeSwiftProxyMethod) -> String {
    let method_name = sanitize_swift_identifier(&method.name, "hostMethod");
    let params = method
        .params
        .iter()
        .enumerate()
        .map(|(idx, p)| {
            let fallback = format!("arg{idx}");
            let pname = sanitize_swift_identifier(&p.name, &fallback);
            let ptype = if p.swift_type.trim().is_empty() {
                "NFValue"
            } else {
                p.swift_type.trim()
            };
            format!("{pname}: {ptype}")
        })
        .collect::<Vec<_>>()
        .join(", ");

    let ret_part = method
        .return_swift_type
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty() && *r != "Void" && *r != "()")
        .map(|r| format!(" -> {r}"))
        .unwrap_or_default();

    format!("func {method_name}({params}){ret_part}")
}

fn default_return_statement(method_name: &str, return_type: Option<&str>) -> Option<String> {
    let raw = return_type?.trim();
    if raw.is_empty() || raw == "Void" || raw == "()" {
        return None;
    }
    if raw.ends_with('?') || raw.starts_with("Optional<") {
        return Some("return nil".to_owned());
    }
    match raw {
        "Bool" | "Swift.Bool" => return Some("return false".to_owned()),
        "String" | "Swift.String" => return Some("return \"\"".to_owned()),
        "Int" | "Int8" | "Int16" | "Int32" | "Int64" | "UInt" | "UInt8" | "UInt16" | "UInt32"
        | "UInt64" | "Double" | "Float" | "CGFloat" => return Some("return 0".to_owned()),
        "NFValue" => return Some("return .null".to_owned()),
        _ => {}
    }
    if raw.starts_with("Array<") || raw.starts_with("Set<") {
        return Some("return []".to_owned());
    }
    if raw.starts_with("Dictionary<") {
        return Some("return [:]".to_owned());
    }
    if raw.starts_with('[') && raw.ends_with(']') {
        let inner = &raw[1..raw.len() - 1];
        let mut depth = 0i32;
        let mut has_top_colon = false;
        for ch in inner.chars() {
            match ch {
                '[' | '<' | '(' => depth += 1,
                ']' | '>' | ')' => depth -= 1,
                ':' if depth == 0 => {
                    has_top_colon = true;
                    break;
                }
                _ => {}
            }
        }
        return Some(if has_top_colon {
            "return [:]".to_owned()
        } else {
            "return []".to_owned()
        });
    }
    Some(format!(
        "fatalError(\"Unimplemented SwiftHostProtocol.{method_name}\")"
    ))
}

pub(super) fn render_host_method_stub(method: &XcodeSwiftProxyMethod) -> String {
    let sig = format_swift_method_signature(method);
    let clean_name = sanitize_swift_identifier(&method.name, "hostMethod");
    let mut out = String::new();
    let _ = writeln!(out, "    {sig} {{");
    let _ = writeln!(out, "        // NeoFlash host stub: {clean_name}");
    if let Some(ret_stmt) =
        default_return_statement(&clean_name, method.return_swift_type.as_deref())
    {
        let _ = writeln!(out, "        {ret_stmt}");
    }
    let _ = writeln!(out, "    }}");
    out
}

pub(super) fn render_app_entry_swift(pascal_project_name: &str) -> String {
    format!(
        r#"import SwiftUI

@main
struct {pascal_project_name}App: App {{
    var body: some Scene {{
        WindowGroup {{
            ContentView()
        }}
    }}
}}
"#
    )
}

pub(super) fn render_content_view_swift() -> String {
    r#"import SwiftUI

struct ContentView: View {
    @State private var scene: MainScene
    @State private var host: AppSwiftHost

    init() {
        let initialHost = AppSwiftHost()
        let initialScene = MainScene()
        initialScene.host = initialHost
        _host = State(initialValue: initialHost)
        _scene = State(initialValue: initialScene)
    }

    var body: some View {
        NeoFlashStageView(scene: scene)
            .ignoresSafeArea()
    }
}
"#
    .to_owned()
}

pub(super) fn render_initial_app_swift_host(host_methods: &[XcodeSwiftProxyMethod]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "import Foundation");
    let _ = writeln!(out);
    let _ = writeln!(out, "@MainActor");
    let _ = writeln!(out, "final class AppSwiftHost: SwiftHostProtocol {{");
    let _ = writeln!(out, "    init() {{}}");
    for method in host_methods {
        let _ = writeln!(out);
        out.push_str(&render_host_method_stub(method));
    }
    let _ = writeln!(out, "}}");
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SwiftToken {
    Ident(String),
    RawIdent(String),
    LBrace(usize),
    RBrace(usize),
    LParen,
    RParen,
    LAngle,
    RAngle,
    Other,
}

fn is_swift_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_' || ch == '$' || (!ch.is_ascii() && ch.is_alphabetic())
}

fn is_swift_ident_continue(ch: char) -> bool {
    is_swift_ident_start(ch) || ch.is_ascii_digit()
}

/// Lexes Swift source while ignoring `//` line comments, nested `/* ... */` block comments,
/// standard `"..."` strings, multiline `"""..."""` strings, and raw `#"..."#` strings.
fn tokenize_swift_declarations(source: &str) -> Vec<SwiftToken> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut tokens = Vec::new();

    while i < len {
        let b = bytes[i];

        // Whitespace
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Single-line comment `//`
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // Nested block comment `/* ... */`
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            i += 2;
            let mut depth = 1usize;
            while i < len && depth > 0 {
                if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }

        // Raw string literal `#"..."#` or `#"""..."""#`
        if b == b'#' {
            let mut hashes = 0usize;
            let mut cursor = i;
            while cursor < len && bytes[cursor] == b'#' {
                hashes += 1;
                cursor += 1;
            }
            if cursor < len && bytes[cursor] == b'"' {
                i = skip_swift_string_literal(bytes, cursor, hashes);
                tokens.push(SwiftToken::Other);
                continue;
            }
        }

        // Normal or multiline string literal `"..."` or `"""..."""`
        if b == b'"' {
            i = skip_swift_string_literal(bytes, i, 0);
            tokens.push(SwiftToken::Other);
            continue;
        }

        // Backtick-escaped identifier `` `openPaywall` ``
        if b == b'`' {
            let start = i + 1;
            i += 1;
            while i < len && bytes[i] != b'`' && bytes[i] != b'\n' {
                i += 1;
            }
            let raw_name = source[start..i].trim().to_owned();
            if i < len && bytes[i] == b'`' {
                i += 1;
            }
            if !raw_name.is_empty() {
                tokens.push(SwiftToken::RawIdent(raw_name));
            }
            continue;
        }

        match b {
            b'{' => {
                tokens.push(SwiftToken::LBrace(i));
                i += 1;
            }
            b'}' => {
                tokens.push(SwiftToken::RBrace(i));
                i += 1;
            }
            b'(' => {
                tokens.push(SwiftToken::LParen);
                i += 1;
            }
            b')' => {
                tokens.push(SwiftToken::RParen);
                i += 1;
            }
            b'<' => {
                tokens.push(SwiftToken::LAngle);
                i += 1;
            }
            b'>' => {
                tokens.push(SwiftToken::RAngle);
                i += 1;
            }
            _ => {
                let ch = source[i..].chars().next().unwrap_or('\0');
                if is_swift_ident_start(ch) {
                    let start = i;
                    i += ch.len_utf8();
                    while i < len {
                        let next_ch = source[i..].chars().next().unwrap_or('\0');
                        if is_swift_ident_continue(next_ch) {
                            i += next_ch.len_utf8();
                        } else {
                            break;
                        }
                    }
                    tokens.push(SwiftToken::Ident(source[start..i].to_owned()));
                } else {
                    tokens.push(SwiftToken::Other);
                    i += ch.len_utf8().max(1);
                }
            }
        }
    }

    tokens
}

fn skip_swift_string_literal(bytes: &[u8], quote_pos: usize, hashes: usize) -> usize {
    let len = bytes.len();
    let is_multiline =
        quote_pos + 2 < len && bytes[quote_pos + 1] == b'"' && bytes[quote_pos + 2] == b'"';
    let mut i = if is_multiline {
        quote_pos + 3
    } else {
        quote_pos + 1
    };

    while i < len {
        // Escape sequence `\` followed by `hashes` `#`s
        if bytes[i] == b'\\' {
            let mut h = 0usize;
            while i + 1 + h < len && h < hashes && bytes[i + 1 + h] == b'#' {
                h += 1;
            }
            if h == hashes {
                // Skip `\` + `#`*hashes + escaped character (or `\(` interpolation opener)
                let after_escape = i + 1 + hashes;
                if after_escape < len && bytes[after_escape] == b'(' {
                    // Skip balanced parentheses inside string interpolation `\(...)`
                    i = skip_balanced_interpolation(bytes, after_escape + 1);
                    continue;
                }
                i = (after_escape + 1).min(len);
                continue;
            }
        }

        if is_multiline {
            if i + 2 + hashes < len
                && bytes[i] == b'"'
                && bytes[i + 1] == b'"'
                && bytes[i + 2] == b'"'
                && bytes[i + 3..i + 3 + hashes].iter().all(|&c| c == b'#')
            {
                return i + 3 + hashes;
            }
        } else if i + hashes < len
            && bytes[i] == b'"'
            && bytes[i + 1..i + 1 + hashes].iter().all(|&c| c == b'#')
        {
            return i + 1 + hashes;
        }

        i += 1;
    }

    len
}

fn skip_balanced_interpolation(bytes: &[u8], mut i: usize) -> usize {
    let len = bytes.len();
    let mut paren_depth = 1usize;
    while i < len && paren_depth > 0 {
        match bytes[i] {
            b'"' => {
                i = skip_swift_string_literal(bytes, i, 0);
            }
            b'(' => {
                paren_depth += 1;
                i += 1;
            }
            b')' => {
                paren_depth -= 1;
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    i
}

/// Scans `AppSwiftHost.swift` for `AppSwiftHost`'s declared `func <name>(` methods and closing `}`.
pub(super) fn scan_app_swift_host(source: &str) -> ScannedSwiftHost {
    let tokens = tokenize_swift_declarations(source);
    let mut declared_methods = BTreeSet::new();
    let mut closing_brace_offset = None;

    // Locate `class AppSwiftHost` (or `struct` / `actor` AppSwiftHost)
    let mut idx = 0usize;
    let mut host_body_start_tok = None;
    while idx + 1 < tokens.len() {
        let is_type_kw = matches!(
            &tokens[idx],
            SwiftToken::Ident(kw) if matches!(kw.as_str(), "class" | "struct" | "actor")
        );
        let is_host_name = matches!(
            &tokens[idx + 1],
            SwiftToken::Ident(name) | SwiftToken::RawIdent(name) if name == "AppSwiftHost"
        );
        if is_type_kw && is_host_name {
            let mut seek = idx + 2;
            while seek < tokens.len() {
                if matches!(tokens[seek], SwiftToken::LBrace(_)) {
                    host_body_start_tok = Some(seek);
                    break;
                }
                seek += 1;
            }
            break;
        }
        idx += 1;
    }

    let Some(open_tok_idx) = host_body_start_tok else {
        // Fallback: scan all top-level `func` declarations in the file if `AppSwiftHost` was renamed/extended.
        let mut cursor = 0usize;
        while cursor + 2 < tokens.len() {
            if matches!(&tokens[cursor], SwiftToken::Ident(kw) if kw == "func")
                && let SwiftToken::Ident(name) | SwiftToken::RawIdent(name) = &tokens[cursor + 1]
            {
                declared_methods.insert(name.clone());
            }
            cursor += 1;
        }
        return ScannedSwiftHost {
            declared_methods,
            closing_brace_offset: None,
        };
    };

    let mut depth = 1usize;
    let mut pos = open_tok_idx + 1;
    while pos < tokens.len() && depth > 0 {
        match &tokens[pos] {
            SwiftToken::LBrace(_) => {
                depth += 1;
                pos += 1;
            }
            SwiftToken::RBrace(byte_offset) => {
                depth -= 1;
                if depth == 0 {
                    closing_brace_offset = Some(*byte_offset);
                    break;
                }
                pos += 1;
            }
            SwiftToken::Ident(kw) if kw == "func" && depth == 1 => {
                if let Some(SwiftToken::Ident(fn_name) | SwiftToken::RawIdent(fn_name)) =
                    tokens.get(pos + 1)
                {
                    // Check `func name(` or `func name<T>(`
                    let mut after_name = pos + 2;
                    if matches!(tokens.get(after_name), Some(SwiftToken::LAngle)) {
                        let mut angle_depth = 1usize;
                        after_name += 1;
                        while after_name < tokens.len() && angle_depth > 0 {
                            match tokens[after_name] {
                                SwiftToken::LAngle => angle_depth += 1,
                                SwiftToken::RAngle => angle_depth -= 1,
                                _ => {}
                            }
                            after_name += 1;
                        }
                    }
                    if matches!(tokens.get(after_name), Some(SwiftToken::LParen)) {
                        declared_methods.insert(fn_name.clone());
                    }
                }
                pos += 1;
            }
            _ => {
                pos += 1;
            }
        }
    }

    // Also scan `extension AppSwiftHost { ... }` blocks in the same file so methods implemented
    // in an extension are recognized and never duplicated!
    let mut ext_idx = 0usize;
    while ext_idx + 1 < tokens.len() {
        let is_ext = matches!(&tokens[ext_idx], SwiftToken::Ident(kw) if kw == "extension");
        let is_host = matches!(
            &tokens[ext_idx + 1],
            SwiftToken::Ident(name) | SwiftToken::RawIdent(name) if name == "AppSwiftHost"
        );
        if is_ext && is_host {
            let mut seek = ext_idx + 2;
            while seek < tokens.len() && !matches!(tokens[seek], SwiftToken::LBrace(_)) {
                seek += 1;
            }
            if seek < tokens.len() {
                let mut d = 1usize;
                let mut p = seek + 1;
                while p < tokens.len() && d > 0 {
                    match &tokens[p] {
                        SwiftToken::LBrace(_) => d += 1,
                        SwiftToken::RBrace(_) => d -= 1,
                        SwiftToken::Ident(kw) if kw == "func" && d == 1 => {
                            if let Some(
                                SwiftToken::Ident(fn_name) | SwiftToken::RawIdent(fn_name),
                            ) = tokens.get(p + 1)
                            {
                                declared_methods.insert(fn_name.clone());
                            }
                        }
                        _ => {}
                    }
                    p += 1;
                }
                ext_idx = p;
                continue;
            }
        }
        ext_idx += 1;
    }

    ScannedSwiftHost {
        declared_methods,
        closing_brace_offset,
    }
}

/// Non-destructively appends only missing `SwiftHostProtocol` stubs right before `AppSwiftHost`'s
/// closing `}` without modifying a single byte of existing developer code.
pub(super) fn append_missing_host_stubs(
    existing_source: &str,
    host_methods: &[XcodeSwiftProxyMethod],
) -> (String, Vec<String>) {
    let scanned = scan_app_swift_host(existing_source);
    let mut missing_stubs = String::new();
    let mut appended_names = Vec::new();

    for method in host_methods {
        let clean_name = sanitize_swift_identifier(&method.name, "hostMethod");
        if !scanned.declared_methods.contains(&clean_name) {
            let _ = writeln!(missing_stubs);
            missing_stubs.push_str(&render_host_method_stub(method));
            appended_names.push(clean_name);
        }
    }

    if appended_names.is_empty() {
        return (existing_source.to_owned(), Vec::new());
    }

    if let Some(brace_idx) = scanned.closing_brace_offset {
        // Walk back over horizontal indentation immediately preceding `}` on the same line
        let mut insert_at = brace_idx;
        while insert_at > 0 && matches!(existing_source.as_bytes()[insert_at - 1], b' ' | b'\t') {
            insert_at -= 1;
        }
        let prefix = &existing_source[..insert_at];
        let suffix = &existing_source[insert_at..];
        let mut updated = String::with_capacity(existing_source.len() + missing_stubs.len() + 4);
        updated.push_str(prefix);
        if !prefix.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(&missing_stubs);
        updated.push_str(suffix);
        return (updated, appended_names);
    }

    // Fallback if `class AppSwiftHost` was not found in the file: append an extension at EOF.
    let mut updated = existing_source.to_owned();
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    let _ = writeln!(updated, "\nextension AppSwiftHost {{");
    updated.push_str(&missing_stubs);
    let _ = writeln!(updated, "}}");
    (updated, appended_names)
}
