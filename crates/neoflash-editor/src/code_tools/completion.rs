use super::*;

/// Category of a resolved code completion item.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompletionKind {
    LocalVariable,
    Parameter,
    Field,
    Method,
    Class,
    Interface,
    Function,
    ModuleExport,
    Builtin,
}

/// A resolved code hint / completion candidate from local scope, current class, imported project
/// classes/modules, or language built-ins.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompletionItem {
    pub label: String,
    pub insert: String,
    pub caret: usize,
    pub signature: String,
    pub kind: CompletionKind,
    pub import: Option<String>,
}

/// Infer local variable and parameter types (`var name: Type`, `let name: Type`,
/// `const name = new ClassName()`, literals, AS1/AS2 suffixes, comment hints, and `this`/`super`).
#[allow(dead_code)]
pub(crate) fn infer_local_variable_types(
    source: &str,
    language: neoflash_core::ScriptLanguage,
) -> std::collections::BTreeMap<String, String> {
    let mut types = std::collections::BTreeMap::new();

    // 1. Check AS1/AS2 comment type hints (`// MovieClip hero;` or `// TextField label;`)
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(comment) = trimmed.strip_prefix("//") {
            let parts: Vec<&str> = comment
                .trim()
                .trim_end_matches(';')
                .split_whitespace()
                .collect();
            if parts.len() == 2 && is_ident(parts[0]) && is_ident(parts[1]) {
                types.insert(parts[1].to_string(), short_type_name(parts[0]).to_string());
            }
        }
    }

    // 2. Detect enclosing `class ClassName [extends SuperName]` for `this` and `super`
    let tokens = neoflash_script::warnings::scan_word_tokens(source);
    for (idx, (word, _, _)) in tokens.iter().enumerate() {
        if word == "class"
            && let Some((cls_name, _, _)) = tokens.get(idx + 1)
            && is_ident(cls_name)
        {
            types
                .entry("this".to_string())
                .or_insert_with(|| cls_name.clone());
            if let Some((ext_kw, _, _)) = tokens.get(idx + 2)
                && ext_kw == "extends"
                && let Some((base_name, _, _)) = tokens.get(idx + 3)
            {
                types
                    .entry("super".to_string())
                    .or_insert_with(|| base_name.clone());
            }
        }
    }

    // 3. Scan variable declarations (`var`, `let`, `const`) and function parameters
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }

        // Function parameter annotations: `function foo(a: Type, b: OtherType)`
        if let Some(fn_idx) = trimmed.find("function") {
            let after_fn = &trimmed[fn_idx + "function".len()..];
            if let Some(open_p) = after_fn.find('(')
                && let Some(close_p) = after_fn[open_p + 1..].find(')')
            {
                let params_str = &after_fn[open_p + 1..open_p + 1 + close_p];
                for raw_param in params_str.split(',') {
                    let p = raw_param.trim().trim_start_matches("...");
                    if let Some((p_name, p_rest)) = p.split_once(':') {
                        let name = p_name.trim().trim_end_matches('?');
                        let ty_raw = p_rest.split('=').next().unwrap_or(p_rest).trim();
                        if is_ident(name) && !ty_raw.is_empty() {
                            types.insert(name.to_string(), normalize_type_label(ty_raw));
                        }
                    }
                }
            }
        }

        // Variable declarations (`var`, `let`, `const`, including `public var`, `private var`, `export const`)
        for kw in ["var ", "let ", "const "] {
            let mut search_from = 0usize;
            while let Some(rel_idx) = trimmed[search_from..].find(kw) {
                let abs_idx = search_from + rel_idx;
                let valid_boundary = abs_idx == 0
                    || !trimmed[..abs_idx]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_alphanumeric() || c == '_');
                search_from = abs_idx + kw.len();
                if !valid_boundary {
                    continue;
                }
                let rest = trimmed[search_from..].trim_start();
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                    .collect();
                if name.is_empty() {
                    continue;
                }
                let after_name = rest[name.len()..].trim_start();
                let after_name = after_name
                    .strip_prefix('?')
                    .unwrap_or(after_name)
                    .trim_start();

                if let Some(after_colon) = after_name.strip_prefix(':') {
                    let ty_part = after_colon
                        .split(['=', ';', ','])
                        .next()
                        .unwrap_or("")
                        .trim();
                    if !ty_part.is_empty() {
                        types.insert(name.clone(), normalize_type_label(ty_part));
                        continue;
                    }
                }

                if let Some(after_eq) = after_name.strip_prefix('=') {
                    let rhs = after_eq.split(';').next().unwrap_or(after_eq).trim();
                    if let Some(inferred) = infer_rhs_type(rhs, language) {
                        types.insert(name.clone(), inferred);
                        continue;
                    }
                }

                if let Some(suffix_ty) = as2_suffix_type(&name) {
                    types.insert(name, suffix_ty.to_string());
                }
            }
        }
    }

    // 4. Also check any identifier in the script for AS1/AS2 suffix conventions (`hero_mc`, `title_txt`, etc.)
    for (word, _, _) in &tokens {
        if !types.contains_key(word)
            && let Some(suffix_ty) = as2_suffix_type(word)
        {
            types.insert(word.clone(), suffix_ty.to_string());
        }
    }

    types
}

fn infer_rhs_type(rhs: &str, language: neoflash_core::ScriptLanguage) -> Option<String> {
    let trimmed = rhs.trim();
    if let Some(after_new) = trimmed.strip_prefix("new ") {
        let ctor: String = after_new
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.' || *c == '$')
            .collect();
        if !ctor.is_empty() {
            return Some(short_type_name(&ctor).to_string());
        }
    }
    if trimmed.starts_with('"') || trimmed.starts_with('\'') || trimmed.starts_with('`') {
        return Some(
            if language == neoflash_core::ScriptLanguage::TypeScript {
                "string"
            } else {
                "String"
            }
            .to_string(),
        );
    }
    if trimmed.starts_with("true") || trimmed.starts_with("false") {
        return Some(
            if language == neoflash_core::ScriptLanguage::TypeScript {
                "boolean"
            } else {
                "Boolean"
            }
            .to_string(),
        );
    }
    if trimmed.starts_with('[') {
        return Some("Array".to_string());
    }
    if trimmed
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || (c == '-' && trimmed.len() > 1))
    {
        return Some(
            if language == neoflash_core::ScriptLanguage::TypeScript {
                "number"
            } else {
                "Number"
            }
            .to_string(),
        );
    }
    None
}

fn as2_suffix_type(ident: &str) -> Option<&'static str> {
    const SUFFIXES: &[(&str, &str)] = &[
        ("_mc", "MovieClip"),
        ("_btn", "Button"),
        ("_txt", "TextField"),
        ("_fmt", "TextFormat"),
        ("_sound", "Sound"),
        ("_color", "Color"),
        ("_array", "Array"),
        ("_str", "String"),
        ("_date", "Date"),
        ("_xml", "XML"),
        ("_xmlsocket", "XMLSocket"),
        ("_lv", "LoadVars"),
        ("_lc", "LocalConnection"),
        ("_nc", "NetConnection"),
        ("_ns", "NetStream"),
        ("_so", "SharedObject"),
        ("_cam", "Camera"),
        ("_mic", "Microphone"),
        ("_video", "Video"),
        ("_pj", "PrintJob"),
        ("_cm", "ContextMenu"),
        ("_cmi", "ContextMenuItem"),
        ("_style", "StyleSheet"),
    ];
    SUFFIXES
        .iter()
        .find_map(|&(sfx, ty)| ident.ends_with(sfx).then_some(ty))
}

fn normalize_type_label(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.ends_with("[]") {
        return "Array".to_string();
    }
    if let Some(before_generic) = trimmed.split('<').next() {
        return short_type_name(before_generic.trim()).to_string();
    }
    short_type_name(trimmed).to_string()
}

fn short_type_name(qualified: &str) -> &str {
    qualified.rsplit('.').next().unwrap_or(qualified).trim()
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_alphabetic() || first == '_' || first == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// Metadata extracted from a class, interface, or module in the current file or a project `SourceUnit`.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub(crate) struct ExtractedTypeInfo {
    pub name: String,
    pub qualified_name: String,
    pub super_class: Option<String>,
    pub is_interface: bool,
    pub members: Vec<CompletionItem>,
}

/// Extract classes, interfaces, and module exports (fields, methods, getters/setters, exported functions/consts)
/// from a script or project `SourceUnit`.
#[allow(dead_code)]
pub(crate) fn extract_types_and_members(
    source: &str,
    path: Option<&str>,
) -> std::collections::BTreeMap<String, ExtractedTypeInfo> {
    let mut map: std::collections::BTreeMap<String, ExtractedTypeInfo> =
        std::collections::BTreeMap::new();

    // Detect package name if AS3 `package foo.bar {`
    let mut package_prefix = String::new();
    for line in source.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("package ") {
            let pkg: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                .collect();
            if !pkg.is_empty() {
                package_prefix = pkg;
            }
            break;
        }
    }

    let mut current_type: Option<ExtractedTypeInfo> = None;
    let mut module_exports: Vec<CompletionItem> = Vec::new();

    for line in source.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
            continue;
        }

        // Check for `class Name` or `interface Name`
        if let Some((kind_is_iface, decl_after)) = find_class_or_interface_header(t) {
            if let Some(prev) = current_type.take() {
                map.insert(prev.name.clone(), prev);
            }
            let raw_name: String = decl_after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                .collect();
            if !raw_name.is_empty() {
                let short = short_type_name(&raw_name).to_string();
                let qualified = if raw_name.contains('.') {
                    raw_name.clone()
                } else if !package_prefix.is_empty() {
                    format!("{package_prefix}.{short}")
                } else {
                    short.clone()
                };
                let after_name = decl_after[raw_name.len()..].trim_start();
                let super_class = after_name
                    .strip_prefix("extends ")
                    .map(|ext| {
                        ext.trim_start()
                            .chars()
                            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                            .collect::<String>()
                    })
                    .filter(|s| !s.is_empty())
                    .map(|s| short_type_name(&s).to_string());

                current_type = Some(ExtractedTypeInfo {
                    name: short,
                    qualified_name: qualified,
                    super_class,
                    is_interface: kind_is_iface,
                    members: Vec::new(),
                });
                continue;
            }
        }

        // Inside a class/interface, or at module top-level:
        if let Some(member) = parse_member_line(t, current_type.as_ref().map(|c| c.name.as_str())) {
            if let Some(cur) = &mut current_type {
                if !cur.members.iter().any(|m| m.label == member.label) {
                    cur.members.push(member);
                }
            } else if t.starts_with("export ") {
                let mut exp_item = member;
                exp_item.kind = CompletionKind::ModuleExport;
                module_exports.push(exp_item);
            }
        }
    }

    if let Some(prev) = current_type.take() {
        map.insert(prev.name.clone(), prev);
    }

    if !module_exports.is_empty()
        && let Some(p) = path
    {
        let mod_stem = std::path::Path::new(p)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(p)
            .to_string();
        map.entry(mod_stem.clone())
            .or_insert_with(|| ExtractedTypeInfo {
                name: mod_stem,
                qualified_name: p.to_string(),
                super_class: None,
                is_interface: false,
                members: module_exports,
            });
    }

    map
}

fn find_class_or_interface_header(trimmed: &str) -> Option<(bool, &str)> {
    let mut s = trimmed;
    for kw in [
        "export ",
        "default ",
        "public ",
        "internal ",
        "dynamic ",
        "final ",
        "abstract ",
    ] {
        while let Some(rest) = s.strip_prefix(kw) {
            s = rest.trim_start();
        }
    }
    if let Some(rest) = s.strip_prefix("class ") {
        return Some((false, rest.trim_start()));
    }
    if let Some(rest) = s.strip_prefix("interface ") {
        return Some((true, rest.trim_start()));
    }
    None
}

fn parse_member_line(trimmed: &str, enclosing_class: Option<&str>) -> Option<CompletionItem> {
    let mut s = trimmed;
    let is_exported = s.starts_with("export ");
    for kw in [
        "export ",
        "public ",
        "private ",
        "protected ",
        "internal ",
        "static ",
        "override ",
        "final ",
        "readonly ",
        "async ",
    ] {
        while let Some(rest) = s.strip_prefix(kw) {
            s = rest.trim_start();
        }
    }

    // Getter / setter: `get foo()` or `function get foo()`
    if let Some(rest) = s
        .strip_prefix("function get ")
        .or_else(|| s.strip_prefix("function set "))
        .or_else(|| s.strip_prefix("get "))
        .or_else(|| s.strip_prefix("set "))
    {
        let name: String = rest
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
            .collect();
        if is_ident(&name) && rest[name.len()..].trim_start().starts_with('(') {
            return Some(CompletionItem {
                caret: name.chars().count(),
                label: name.clone(),
                insert: name.clone(),
                signature: format!("{}.{name}", enclosing_class.unwrap_or("property")),
                kind: CompletionKind::Field,
                import: None,
            });
        }
    }

    // Function or AS method: `function move(dx: Number): Void`
    if let Some(rest) = s.strip_prefix("function ") {
        let name: String = rest
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
            .collect();
        if is_ident(&name) {
            if enclosing_class == Some(name.as_str()) || name == "constructor" {
                return None;
            }
            let after_name = rest[name.len()..].trim_start();
            let sig_params = after_name
                .find(')')
                .map(|idx| &after_name[..=idx])
                .unwrap_or("()");
            let has_args = sig_params != "()";
            let insert = format!("{name}()");
            let caret = if has_args {
                name.chars().count() + 1
            } else {
                insert.chars().count()
            };
            return Some(CompletionItem {
                label: name.clone(),
                insert,
                caret,
                signature: format!("{name}{sig_params}"),
                kind: if enclosing_class.is_some() {
                    CompletionKind::Method
                } else {
                    CompletionKind::Function
                },
                import: None,
            });
        }
    }

    // Variable / field declaration: `var speed: Number`, `let count = 0`, `const MAX = 10`
    for kw in ["var ", "let ", "const "] {
        if let Some(rest) = s.strip_prefix(kw) {
            let name: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                .collect();
            if is_ident(&name) {
                let after = rest[name.len()..].trim_start();
                let ty_str = after
                    .strip_prefix(':')
                    .and_then(|r| r.split(['=', ';']).next())
                    .map(str::trim)
                    .unwrap_or("any");
                return Some(CompletionItem {
                    caret: name.chars().count(),
                    label: name.clone(),
                    insert: name.clone(),
                    signature: format!("{name}: {ty_str}"),
                    kind: if enclosing_class.is_some() {
                        CompletionKind::Field
                    } else if is_exported {
                        CompletionKind::ModuleExport
                    } else {
                        CompletionKind::LocalVariable
                    },
                    import: None,
                });
            }
        }
    }

    // TypeScript / ES6 class method or property without `function`/`var` keyword inside a class/interface
    if enclosing_class.is_some() {
        let name: String = s
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
            .collect();
        if is_ident(&name)
            && !matches!(
                name.as_str(),
                "if" | "for"
                    | "while"
                    | "switch"
                    | "return"
                    | "throw"
                    | "try"
                    | "catch"
                    | "new"
                    | "constructor"
                    | "super"
            )
        {
            let after = s[name.len()..].trim_start();
            let after = after.strip_prefix('?').unwrap_or(after).trim_start();
            if after.starts_with('(') {
                let sig_params = after.find(')').map(|idx| &after[..=idx]).unwrap_or("()");
                let has_args = sig_params != "()";
                let insert = format!("{name}()");
                let caret = if has_args {
                    name.chars().count() + 1
                } else {
                    insert.chars().count()
                };
                return Some(CompletionItem {
                    label: name.clone(),
                    insert,
                    caret,
                    signature: format!("{name}{sig_params}"),
                    kind: CompletionKind::Method,
                    import: None,
                });
            } else if let Some(after_colon) = after.strip_prefix(':') {
                let ty_str = after_colon
                    .split(['=', ';'])
                    .next()
                    .map(str::trim)
                    .unwrap_or("any");
                return Some(CompletionItem {
                    caret: name.chars().count(),
                    label: name.clone(),
                    insert: name.clone(),
                    signature: format!("{name}: {ty_str}"),
                    kind: CompletionKind::Field,
                    import: None,
                });
            }
        }
    }

    None
}

/// Resolve code completion candidates at `caret` (char index) in `source`, combining:
/// - Local variable and parameter declarations (`var name: Type`, `let name: Type`, `const name = new ClassName()`)
/// - Class fields and methods of the current file and imported/project `SourceUnit` classes (including inheritance)
/// - Exported members of imported JS/TS modules (`import * as ns from "./mod"`, `import { fn } from "./mod"`)
/// - AS1/AS2 variable suffixes (`_mc`, `_txt`, `_btn`, `_sound`) and `// ClassName varName;` comment hints
/// - Built-in language APIs from `script_api`.
#[allow(dead_code)]
pub(crate) fn complete_at_caret(
    language: neoflash_core::ScriptLanguage,
    source: &str,
    caret: usize,
    project_sources: &[neoflash_core::SourceUnit],
) -> Vec<CompletionItem> {
    let byte_pos = char_to_byte(source, caret.min(source.chars().count()));
    let before = &source[..byte_pos];

    // Collect all class/interface/module definitions from the current file and project_sources
    let mut type_catalog = extract_types_and_members(source, None);
    for unit in project_sources {
        for (k, v) in extract_types_and_members(&unit.source, Some(&unit.path)) {
            type_catalog.entry(k).or_insert(v);
        }
    }

    let mut local_types = infer_local_variable_types(source, language);

    // Also track JS/TS namespace imports (`import * as math from "./math"`)
    for line in source.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("import * as ")
            && let Some((ns_name, from_part)) = rest.split_once(" from ")
        {
            let ns = ns_name.trim();
            let spec = from_part
                .trim()
                .trim_end_matches(';')
                .trim_matches(['"', '\'']);
            let mod_stem = std::path::Path::new(spec)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(spec);
            if is_ident(ns) {
                local_types.insert(ns.to_string(), mod_stem.to_string());
            }
        }
    }

    // Check if we are completing a member access `receiver.prefix`
    let ident_len: usize = before
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .map(char::len_utf8)
        .sum();
    let prefix = &before[before.len() - ident_len..];
    let before_prefix = &before[..before.len() - ident_len];

    if let Some(before_dot) = before_prefix.strip_suffix('.') {
        let receiver: String = before_dot
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
            .collect::<String>()
            .chars()
            .rev()
            .collect();

        if !receiver.is_empty() {
            let mut results: Vec<CompletionItem> = Vec::new();
            let resolved_type = local_types.get(&receiver).cloned().or_else(|| {
                type_catalog
                    .contains_key(&receiver)
                    .then(|| receiver.clone())
            });

            if let Some(ty_name) = resolved_type {
                // Walk class hierarchy in `type_catalog`
                let mut cur_ty = Some(ty_name.clone());
                let mut visited = std::collections::BTreeSet::new();
                while let Some(ct) = cur_ty {
                    if !visited.insert(ct.clone()) {
                        break;
                    }
                    if let Some(info) = type_catalog.get(&ct) {
                        for m in &info.members {
                            if m.label
                                .to_ascii_lowercase()
                                .starts_with(&prefix.to_ascii_lowercase())
                                && !results.iter().any(|r| r.label == m.label)
                            {
                                results.push(m.clone());
                            }
                        }
                        cur_ty = info.super_class.clone();
                    } else {
                        break;
                    }
                }

                // Also include built-in members if `ty_name` is a known built-in class (e.g. MovieClip, TextField, Array, String)
                if let Some(api) = crate::script_api::api(language) {
                    let lower_prefix = prefix.to_ascii_lowercase();
                    for m in &api.members {
                        let owner_short = crate::script_api::short_name(&m.owner);
                        if (owner_short == ty_name || m.owner == ty_name)
                            && m.name.to_ascii_lowercase().starts_with(&lower_prefix)
                        {
                            let label = m.label();
                            if !results.iter().any(|r| r.label == label) {
                                let (insert, caret) = m.insertion();
                                results.push(CompletionItem {
                                    label,
                                    insert,
                                    caret,
                                    signature: format!("{}.{}", owner_short, m.name),
                                    kind: CompletionKind::Builtin,
                                    import: None,
                                });
                            }
                        }
                    }
                }
            }
            return results;
        }
    }

    // Unqualified completion at caret: local variables + current class members + project classes + language built-ins
    let mut items: Vec<CompletionItem> = Vec::new();
    let lower_prefix = prefix.to_ascii_lowercase();

    for (var_name, var_ty) in &local_types {
        if var_name == "this" || var_name == "super" {
            continue;
        }
        if var_name.to_ascii_lowercase().starts_with(&lower_prefix)
            && !items.iter().any(|i| i.label == *var_name)
        {
            items.push(CompletionItem {
                caret: var_name.chars().count(),
                label: var_name.clone(),
                insert: var_name.clone(),
                signature: format!("{var_name}: {var_ty}"),
                kind: CompletionKind::LocalVariable,
                import: None,
            });
        }
    }

    // Current class members (accessible directly in AS2/AS3)
    if let Some(this_ty) = local_types.get("this")
        && let Some(info) = type_catalog.get(this_ty)
    {
        for m in &info.members {
            if m.label.to_ascii_lowercase().starts_with(&lower_prefix)
                && !items.iter().any(|i| i.label == m.label)
            {
                items.push(m.clone());
            }
        }
    }

    // Project classes & interfaces
    for (cls_name, info) in &type_catalog {
        if cls_name.to_ascii_lowercase().starts_with(&lower_prefix)
            && !items.iter().any(|i| i.label == *cls_name)
        {
            let needs_import = language == neoflash_core::ScriptLanguage::ActionScript3
                && info.qualified_name.contains('.')
                && crate::script_api::missing_import(source, &info.qualified_name).is_some();
            items.push(CompletionItem {
                caret: cls_name.chars().count(),
                label: cls_name.clone(),
                insert: cls_name.clone(),
                signature: info.qualified_name.clone(),
                kind: if info.is_interface {
                    CompletionKind::Interface
                } else {
                    CompletionKind::Class
                },
                import: needs_import.then(|| info.qualified_name.clone()),
            });
        }
    }

    // Language built-ins
    if let Some(api) = crate::script_api::api(language) {
        for m in &api.members {
            if m.owner.is_empty() && m.name.to_ascii_lowercase().starts_with(&lower_prefix) {
                let label = m.label();
                if !items.iter().any(|i| i.label == label) {
                    let (insert, caret) = m.insertion();
                    items.push(CompletionItem {
                        label,
                        insert,
                        caret,
                        signature: m.ty.clone(),
                        kind: CompletionKind::Builtin,
                        import: None,
                    });
                }
            }
        }
    }

    items
}

/// Collect compiler diagnostics and lint warnings along with their actionable `QuickFix` items for `source`.
#[allow(dead_code)]
pub(crate) fn diagnostics_and_quick_fixes(
    language: neoflash_core::ScriptLanguage,
    source: &str,
) -> Vec<(String, Vec<neoflash_script::QuickFix>)> {
    let script = neoflash_core::Script {
        language,
        source: source.to_string(),
    };
    let mut out = Vec::new();
    if let Err(diags) = neoflash_script::compile(&script) {
        for d in diags {
            let fixes = d.quick_fixes(source);
            out.push((d.message, fixes));
        }
    }
    for w in neoflash_script::check_warnings(&script) {
        out.push((w.message, w.quick_fixes));
    }
    out
}
