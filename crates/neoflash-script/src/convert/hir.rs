//! Stage-aware and type-aware High-Level Intermediate Representation (HIR) and AST analyzer
//! shared across the AS3, AS2, JavaScript, and Swift 6 deterministic code generators.

use crate::Diagnostic;
use crate::actionscript::{
    CompileContext,
    ast::{Case, CatchClause, Expr, Function, LegacyTrigger, PropertyKey, Stmt},
    lexer::{Kind, Lexer, Token, TokenCursor},
    parser::Frontend,
};
use crate::byte_offset_to_line_col;
use neoflash_core::{
    AuthoredPlayback, ElementKind, Project, ResourceUnit, Script, ScriptLanguage, SourceUnit,
    SymbolKind,
};
use std::collections::{BTreeMap, BTreeSet};

/// Static/inferred type tracked across variables, parameters, return types, and Stage instances.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HirType {
    Int,
    UInt,
    Float,
    Bool,
    String,
    Array,
    Vector(Box<HirType>),
    Dictionary,
    Sprite,
    MovieClip,
    Button,
    TextField,
    Object,
    Void,
    Function,
    Class(String),
    Unknown,
}

impl HirType {
    pub(crate) fn from_annotation(raw: &str) -> Self {
        let trimmed = raw.trim();
        if let Some(elem) = trimmed
            .strip_prefix("Vector.<")
            .and_then(|s| s.strip_suffix('>'))
        {
            return Self::Vector(Box::new(Self::from_annotation(elem)));
        }
        match trimmed {
            "" | "*" => Self::Unknown,
            "int" => Self::Int,
            "uint" => Self::UInt,
            "Number" => Self::Float,
            "Boolean" => Self::Bool,
            "String" => Self::String,
            "Array" => Self::Array,
            "Dictionary" | "flash.utils.Dictionary" => Self::Dictionary,
            "Sprite" | "flash.display.Sprite" => Self::Sprite,
            "MovieClip" | "flash.display.MovieClip" => Self::MovieClip,
            "Button" | "SimpleButton" | "flash.display.SimpleButton" => Self::Button,
            "TextField" | "flash.text.TextField" => Self::TextField,
            "Object" => Self::Object,
            "void" | "Void" => Self::Void,
            "Function" => Self::Function,
            other => Self::Class(other.to_owned()),
        }
    }

    pub(crate) fn to_as3_annotation(&self) -> String {
        match self {
            Self::Int => "int".into(),
            Self::UInt => "uint".into(),
            Self::Float => "Number".into(),
            Self::Bool => "Boolean".into(),
            Self::String => "String".into(),
            Self::Array => "Array".into(),
            Self::Vector(elem) => format!("Vector.<{}>", elem.to_as3_annotation()),
            Self::Dictionary => "Dictionary".into(),
            Self::Sprite => "Sprite".into(),
            Self::MovieClip => "MovieClip".into(),
            Self::Button => "SimpleButton".into(),
            Self::TextField => "TextField".into(),
            Self::Object => "Object".into(),
            Self::Void => "void".into(),
            Self::Function => "Function".into(),
            Self::Class(name) => short_type_name(name).to_owned(),
            Self::Unknown => "*".into(),
        }
    }

    pub(crate) fn to_as2_annotation(&self) -> String {
        match self {
            Self::Int | Self::UInt | Self::Float => "Number".into(),
            Self::Bool => "Boolean".into(),
            Self::String => "String".into(),
            Self::Array | Self::Vector(_) => "Array".into(),
            Self::Dictionary | Self::Object | Self::Unknown => "Object".into(),
            Self::Sprite | Self::MovieClip => "MovieClip".into(),
            Self::Button => "Button".into(),
            Self::TextField => "TextField".into(),
            Self::Void => "Void".into(),
            Self::Function => "Function".into(),
            Self::Class(name) => {
                let short = short_type_name(name);
                match short {
                    "DisplayObject"
                    | "DisplayObjectContainer"
                    | "InteractiveObject"
                    | "Shape"
                    | "Sprite"
                    | "MovieClip" => "MovieClip".into(),
                    "SimpleButton" | "Button" => "Button".into(),
                    "Event" | "MouseEvent" | "KeyboardEvent" | "TimerEvent" | "Stage"
                    | "Dictionary" | "ByteArray" | "Stage3D" | "Context3D" => "Object".into(),
                    _ => name.clone(),
                }
            }
        }
    }

    pub(crate) fn to_swift_type(&self) -> String {
        match self {
            Self::Int => "Int".into(),
            Self::UInt => "UInt".into(),
            Self::Float => "Double".into(),
            Self::Bool => "Bool".into(),
            Self::String => "String".into(),
            Self::Array | Self::Vector(_) | Self::Dictionary => "NFValue".into(),
            Self::Sprite => "NFSprite".into(),
            Self::MovieClip => "NFMovieClip".into(),
            Self::Button => "NFButton".into(),
            Self::TextField => "NFTextField".into(),
            Self::Void => "Void".into(),
            Self::Object | Self::Unknown | Self::Function => "NFValue".into(),
            Self::Class(name) => match short_type_name(name) {
                "Event" | "MouseEvent" | "KeyboardEvent" | "TimerEvent" => "NFEvent".into(),
                "Point" => "NFPoint".into(),
                "Rectangle" => "NFRectangle".into(),
                "Matrix" => "NFMatrix".into(),
                "ColorTransform" => "NFColorTransform".into(),
                "Sound" => "NFSound".into(),
                "Stage" => "NFStage".into(),
                "DisplayObject" | "DisplayObjectContainer" | "InteractiveObject" => {
                    "NFDisplayObject".into()
                }
                short => sanitize_swift_type_ident(short),
            },
        }
    }

    pub(crate) fn is_integer(&self) -> bool {
        matches!(self, Self::Int | Self::UInt)
    }

    pub(crate) fn is_nullable_object(&self) -> bool {
        !matches!(
            self,
            Self::Int | Self::UInt | Self::Float | Self::Bool | Self::Void
        )
    }
}

pub(crate) fn short_type_name(name: &str) -> &str {
    if name.starts_with("Vector.<") {
        return name;
    }
    name.rsplit('.').next().unwrap_or(name)
}

pub(crate) fn sanitize_swift_type_ident(raw: &str) -> String {
    let mut out = String::new();
    let mut capitalize_next = true;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            if out.is_empty() && ch.is_ascii_digit() {
                out.push('_');
            }
            if capitalize_next {
                out.push(ch.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                out.push(ch);
            }
        } else {
            capitalize_next = true;
        }
    }
    if out.is_empty() {
        "NFSymbol".into()
    } else {
        out
    }
}

pub(crate) fn sanitize_as_class_ident(raw: &str) -> String {
    let mut out = String::new();
    let mut capitalize_next = true;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            if out.is_empty() && ch.is_ascii_digit() {
                out.push_str("Sym");
            }
            if ch == '_' {
                capitalize_next = true;
                continue;
            }
            if capitalize_next {
                out.push(ch.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                out.push(ch);
            }
        } else {
            capitalize_next = true;
        }
    }
    if out.is_empty() {
        "LinkedClip".into()
    } else {
        out
    }
}

/// Information about a named child element placed on a Symbol's timeline.
#[derive(Clone, Debug)]
pub(crate) struct StageChildInfo {
    pub(crate) hir_type: HirType,
    #[allow(dead_code)]
    pub(crate) symbol_kind: Option<SymbolKind>,
}

/// Linkage entry for `attachMovie("linkage", ...)` resolution.
#[derive(Clone, Debug)]
pub(crate) struct LinkageSymbolInfo {
    #[allow(dead_code)]
    pub(crate) symbol_id: String,
    pub(crate) symbol_kind: SymbolKind,
    pub(crate) class_name: String,
}

/// Project-wide Stage and class metadata index.
#[derive(Clone, Debug, Default)]
pub(crate) struct ProjectStageIndex {
    /// Per-symbol ID -> (instance name -> child info).
    pub(crate) symbol_children: BTreeMap<String, BTreeMap<String, StageChildInfo>>,
    /// Linkage ID -> symbol linkage info.
    pub(crate) linkages: BTreeMap<String, LinkageSymbolInfo>,
    /// Known project class short names -> qualified class names.
    pub(crate) project_classes: BTreeMap<String, String>,
    /// Per-qualified-class -> set of instance method names.
    pub(crate) class_instance_methods: BTreeMap<String, BTreeSet<String>>,
    /// Per-qualified-class -> instance field types.
    pub(crate) class_fields: BTreeMap<String, BTreeMap<String, HirType>>,
    /// Base class map (qualified child -> qualified or short base).
    pub(crate) class_bases: BTreeMap<String, String>,
}

impl ProjectStageIndex {
    pub(crate) fn build(project: &Project) -> Self {
        let mut index = Self::default();

        for sym in &project.symbols {
            if let Some(linkage) = &sym.linkage_id
                && !linkage.trim().is_empty()
            {
                let cls = sym
                    .class_name
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| sanitize_as_class_ident(linkage));
                index.linkages.insert(
                    linkage.clone(),
                    LinkageSymbolInfo {
                        symbol_id: sym.id.clone(),
                        symbol_kind: sym.kind,
                        class_name: cls,
                    },
                );
            }

            let mut children = BTreeMap::new();
            for layer in &sym.timeline.layers {
                for kf in &layer.keyframes {
                    for el in &kf.elements {
                        if el.name.is_empty() {
                            continue;
                        }
                        let info = match &el.kind {
                            ElementKind::Text { .. } | ElementKind::TextBox { .. } => {
                                StageChildInfo {
                                    hir_type: HirType::TextField,
                                    symbol_kind: None,
                                }
                            }
                            ElementKind::Instance { symbol_id, .. } => {
                                let eff_kind = el.effective_symbol_kind(project);
                                let linked_sym = project.symbol(symbol_id);
                                let hir_type = if let Some(cls) = linked_sym
                                    .and_then(|s| s.class_name.as_ref())
                                    .filter(|c| !c.trim().is_empty())
                                {
                                    HirType::Class(cls.clone())
                                } else if eff_kind == Some(SymbolKind::Button) {
                                    HirType::Button
                                } else {
                                    HirType::MovieClip
                                };
                                StageChildInfo {
                                    hir_type,
                                    symbol_kind: eff_kind,
                                }
                            }
                            _ => StageChildInfo {
                                hir_type: HirType::Sprite,
                                symbol_kind: None,
                            },
                        };
                        children.entry(el.name.clone()).or_insert(info);
                    }
                }
            }
            index.symbol_children.insert(sym.id.clone(), children);
        }

        let includes = AuthoredPlayback::build_project_include_map(project);
        for unit in &project.sources {
            if !unit.language.is_actionscript() {
                continue;
            }
            if let Ok(parsed) = parse_class_unit(unit, &includes) {
                let short = short_type_name(&parsed.qualified_name).to_owned();
                index
                    .project_classes
                    .insert(short, parsed.qualified_name.clone());
                if let Some(base) = &parsed.base_class {
                    index
                        .class_bases
                        .insert(parsed.qualified_name.clone(), base.clone());
                }
                let mut methods = BTreeSet::new();
                let mut fields = BTreeMap::new();
                for m in &parsed.methods {
                    if !m.is_static
                        && m.accessor.is_none()
                        && let Some(name) = &m.function.name
                        && name != &parsed.short_name
                    {
                        methods.insert(name.clone());
                    }
                }
                for f in &parsed.fields {
                    if !f.is_static {
                        let ty =
                            f.ty.as_deref()
                                .map_or(HirType::Unknown, HirType::from_annotation);
                        fields.insert(f.name.clone(), ty);
                    }
                }
                index
                    .class_instance_methods
                    .insert(parsed.qualified_name.clone(), methods);
                index.class_fields.insert(parsed.qualified_name, fields);
            }
        }

        index
    }

    pub(crate) fn child_type(&self, symbol_id: &str, name: &str) -> Option<&StageChildInfo> {
        self.symbol_children.get(symbol_id)?.get(name)
    }
}

/// Access modifier on a class member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemberAccess {
    Public,
    Private,
    Protected,
    Internal,
}

impl MemberAccess {
    pub(crate) fn as3_keyword(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
            Self::Protected => "protected",
            Self::Internal => "internal",
        }
    }

    pub(crate) fn as2_keyword(self) -> &'static str {
        match self {
            Self::Private => "private",
            // AS2 has only public and private; protected/internal lower cleanly to public.
            Self::Public | Self::Protected | Self::Internal => "public",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ParsedField {
    pub(crate) offset: usize,
    pub(crate) name: String,
    pub(crate) ty: Option<String>,
    pub(crate) value: Option<Expr>,
    pub(crate) is_const: bool,
    pub(crate) is_static: bool,
    pub(crate) access: MemberAccess,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AccessorKind {
    Get,
    Set,
}

#[derive(Clone, Debug)]
pub(crate) struct ParsedMethod {
    pub(crate) function: Function,
    pub(crate) is_static: bool,
    pub(crate) is_override: bool,
    pub(crate) access: MemberAccess,
    pub(crate) accessor: Option<AccessorKind>,
    pub(crate) param_type_spans: Vec<(usize, usize)>,
    pub(crate) return_type_span: Option<(usize, usize)>,
    pub(crate) name_span: (usize, usize),
}

#[derive(Clone, Debug)]
pub(crate) struct ParsedClass {
    pub(crate) source: String,
    pub(crate) language: ScriptLanguage,
    pub(crate) package: String,
    pub(crate) short_name: String,
    pub(crate) qualified_name: String,
    pub(crate) base_class: Option<String>,
    pub(crate) interfaces: Vec<String>,
    pub(crate) imports: Vec<String>,
    pub(crate) is_interface: bool,
    pub(crate) is_dynamic: bool,
    pub(crate) is_public: bool,
    pub(crate) fields: Vec<ParsedField>,
    pub(crate) methods: Vec<ParsedMethod>,
}

#[derive(Clone, Debug)]
pub(crate) struct ParsedScript {
    pub(crate) expanded_source: String,
    pub(crate) language: ScriptLanguage,
    pub(crate) imports: Vec<String>,
    pub(crate) statements: Vec<Stmt>,
}

/// Expand `#include` / `include "..."` directives if present in an ActionScript source.
pub(crate) fn expand_includes_if_needed(
    script: &Script,
    base_path: &str,
    includes: &BTreeMap<String, ResourceUnit>,
) -> Result<Script, String> {
    let has_inc = match script.language {
        ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
            script.source.contains("#include")
        }
        ScriptLanguage::ActionScript3 => {
            script.source.contains("include ")
                || script.source.contains("include\"")
                || script.source.contains("include'")
                || script.source.contains("#include")
        }
        _ => false,
    };
    if !has_inc {
        return Ok(script.clone());
    }
    match AuthoredPlayback::expand_script_includes(script, base_path, includes) {
        Ok((expanded, _, _)) => Ok(expanded),
        Err((failing, offset, path, _, msg)) => {
            let (line, col) = byte_offset_to_line_col(&failing.source, offset);
            if path.is_empty() {
                Err(format!("{line}:{col}: {msg}"))
            } else {
                Err(format!("{path}:{line}:{col}: {msg}"))
            }
        }
    }
}

/// Build a permissive `CompileContext` that registers every type identifier referenced in `source`
/// so `Frontend` parses all valid AS1/AS2/AS3 scripts and classes into AST nodes regardless of imports.
pub(crate) fn build_permissive_context(source: &str, _lang: ScriptLanguage) -> CompileContext {
    let mut ctx = CompileContext {
        project: true,
        legacy_case: false,
        ..Default::default()
    };
    for intrinsic in [
        "Date",
        "RegExp",
        "JSON",
        "Vector",
        "XML",
        "XMLList",
        "Namespace",
        "QName",
        "XMLNode",
    ] {
        ctx.intrinsic_types.insert(intrinsic.into());
        ctx.types.insert(intrinsic.into(), intrinsic.into());
    }
    for builtin in [
        "Object",
        "Array",
        "Number",
        "String",
        "Boolean",
        "int",
        "uint",
        "Function",
        "Class",
        "Error",
        "TypeError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
        "ArgumentError",
        "SecurityError",
        "EvalError",
        "URIError",
        "VerifyError",
        "EOFError",
        "IOError",
        "MemoryError",
        "MovieClip",
        "Sprite",
        "Shape",
        "SimpleButton",
        "Button",
        "DisplayObject",
        "DisplayObjectContainer",
        "InteractiveObject",
        "Bitmap",
        "BitmapData",
        "Graphics",
        "Stage",
        "StageScaleMode",
        "StageAlign",
        "StageDisplayState",
        "StageQuality",
        "Stage3D",
        "Context3D",
        "TextField",
        "TextFormat",
        "Event",
        "MouseEvent",
        "KeyboardEvent",
        "TimerEvent",
        "FocusEvent",
        "TextEvent",
        "ProgressEvent",
        "IOErrorEvent",
        "HTTPStatusEvent",
        "NetStatusEvent",
        "EventDispatcher",
        "IEventDispatcher",
        "Keyboard",
        "Key",
        "Mouse",
        "Timer",
        "Dictionary",
        "ByteArray",
        "Endian",
        "Point",
        "Rectangle",
        "Matrix",
        "ColorTransform",
        "Transform",
        "Sound",
        "SoundChannel",
        "SoundTransform",
        "SoundMixer",
        "Loader",
        "LoaderInfo",
        "URLRequest",
        "URLLoader",
        "URLVariables",
        "SharedObject",
        "LoadVars",
        "Color",
        "Selection",
        "SwiftHost",
    ] {
        ctx.types.insert(builtin.into(), builtin.into());
    }

    // Scan tokens to register any user/qualified type names used after `:`, `<`, `new`, `is`, `as`, `extends`, `implements`.
    let scan_script = Script {
        language: ScriptLanguage::ActionScript3,
        source: source.to_owned(),
    };
    let mut lexer = Lexer {
        script: &scan_script,
        offset: 0,
    };
    let mut tokens = Vec::new();
    while let Ok(tok) = lexer.next_token() {
        let is_end = tok.kind == Kind::End;
        tokens.push(tok);
        if is_end {
            break;
        }
    }

    let mut i = 0usize;
    while i < tokens.len() {
        let trigger = match &tokens[i].kind {
            Kind::Punct(":" | "<") => true,
            Kind::Ident(w) => matches!(
                w.as_str(),
                "new"
                    | "is"
                    | "as"
                    | "instanceof"
                    | "extends"
                    | "implements"
                    | "class"
                    | "interface"
            ),
            _ => false,
        };
        if trigger {
            let mut j = i + 1;
            if let Some(Kind::Ident(first)) = tokens.get(j).map(|t| &t.kind) {
                let mut qname = first.clone();
                j += 1;
                while let (Some(Kind::Punct(".")), Some(Kind::Ident(seg))) = (
                    tokens.get(j).map(|t| &t.kind),
                    tokens.get(j + 1).map(|t| &t.kind),
                ) {
                    qname.push('.');
                    qname.push_str(seg);
                    j += 2;
                }
                if !matches!(
                    qname.as_str(),
                    "void" | "Void" | "true" | "false" | "null" | "undefined" | "this" | "super"
                ) {
                    let short = short_type_name(&qname).to_owned();
                    ctx.types
                        .entry(qname.clone())
                        .or_insert_with(|| qname.clone());
                    ctx.types.entry(short.clone()).or_insert(short);
                }
            }
        }
        i += 1;
    }

    ctx
}

fn blank_preserve_newlines(buf: &mut [u8], start: usize, end: usize) {
    for b in &mut buf[start..end] {
        if *b != b'\n' && *b != b'\r' {
            *b = b' ';
        }
    }
}

/// Parse a timeline or object ActionScript (`AS1` / `AS2` / `AS3`) into `ParsedScript`.
pub(crate) fn parse_timeline_script(
    script: &Script,
    base_path: &str,
    includes: &BTreeMap<String, ResourceUnit>,
) -> Result<ParsedScript, String> {
    let expanded = expand_includes_if_needed(script, base_path, includes)?;
    let parse_lang = match expanded.language {
        ScriptLanguage::ActionScript1 => ScriptLanguage::ActionScript2,
        other => other,
    };

    // Tokenize once to extract top-level `import ...;` directives and normalize statement-level `const` -> `var  `.
    let lex_probe = Script {
        language: parse_lang,
        source: expanded.source.clone(),
    };
    let mut lexer = Lexer {
        script: &lex_probe,
        offset: 0,
    };
    let mut raw_tokens: Vec<Token> = Vec::new();
    while let Ok(tok) = lexer.next_token() {
        let end = tok.kind == Kind::End;
        raw_tokens.push(tok);
        if end {
            break;
        }
    }

    let mut cleaned_bytes = expanded.source.as_bytes().to_vec();
    let mut imports = Vec::new();
    let mut brace_depth = 0usize;
    let mut idx = 0usize;
    while idx < raw_tokens.len() {
        match &raw_tokens[idx].kind {
            Kind::Punct("{") => brace_depth += 1,
            Kind::Punct("}") => brace_depth = brace_depth.saturating_sub(1),
            Kind::Ident(w) if w == "import" && brace_depth == 0 => {
                let start_off = raw_tokens[idx].offset;
                let mut j = idx + 1;
                let mut imp = String::new();
                while j < raw_tokens.len() {
                    match &raw_tokens[j].kind {
                        Kind::Ident(seg) => {
                            imp.push_str(seg);
                            j += 1;
                        }
                        Kind::Punct(".") => {
                            imp.push('.');
                            j += 1;
                        }
                        Kind::Punct("*") => {
                            imp.push('*');
                            j += 1;
                        }
                        _ => break,
                    }
                }
                let mut end_off = raw_tokens
                    .get(j.saturating_sub(1))
                    .map_or(raw_tokens[idx].end, |t| t.end);
                if raw_tokens
                    .get(j)
                    .is_some_and(|t| t.kind == Kind::Punct(";"))
                {
                    end_off = raw_tokens[j].end;
                    j += 1;
                }
                if !imp.is_empty() && !imports.contains(&imp) {
                    imports.push(imp);
                }
                blank_preserve_newlines(&mut cleaned_bytes, start_off, end_off);
                idx = j;
                continue;
            }
            Kind::Ident(w)
                if w == "const"
                    && raw_tokens
                        .get(idx + 1)
                        .is_some_and(|t| matches!(t.kind, Kind::Ident(_))) =>
            {
                let start_off = raw_tokens[idx].offset;
                if start_off + 5 <= cleaned_bytes.len()
                    && &cleaned_bytes[start_off..start_off + 5] == b"const"
                {
                    cleaned_bytes[start_off..start_off + 5].copy_from_slice(b"var  ");
                }
            }
            _ => {}
        }
        idx += 1;
    }

    let cleaned_source = String::from_utf8(cleaned_bytes)
        .map_err(|e| format!("Invalid UTF-8 in script source: {e}"))?;
    let parse_script = Script {
        language: parse_lang,
        source: cleaned_source,
    };
    let mut ctx = build_permissive_context(&parse_script.source, parse_lang);
    ctx.legacy_case = false;

    let mut frontend = Frontend {
        script: &parse_script,
        tokens: TokenCursor::default(),
        pos: 0,
        depth: 0,
        controls: Vec::new(),
        function_depth: 0,
        next_function: 1,
        context: ctx,
        no_in: false,
        with_depth: 0,
        in_initclip: false,
    };
    let statements = frontend
        .statements(false)
        .map_err(|diag: Diagnostic| format!("{}:{}: {}", diag.line, diag.column, diag.message))?;

    Ok(ParsedScript {
        expanded_source: expanded.source,
        language: expanded.language,
        imports,
        statements,
    })
}

fn parse_qualified_ident(p: &mut Frontend<'_>) -> Result<String, Diagnostic> {
    let mut name = p.ident()?;
    while p.eat(".") {
        if p.eat("*") {
            name.push_str(".*");
            break;
        }
        name.push('.');
        name.push_str(&p.ident()?);
    }
    Ok(name)
}

fn parse_imports_into(p: &mut Frontend<'_>, out: &mut Vec<String>) -> Result<(), Diagnostic> {
    while p.eat("import") {
        let imp = parse_qualified_ident(p)?;
        if !out.contains(&imp) {
            out.push(imp);
        }
        let _ = p.semicolon();
    }
    Ok(())
}

/// Parse an AS2 or AS3 class/interface `SourceUnit` into `ParsedClass`, including exact source spans
/// for method names, parameter types, and return types (used by both converters and `SwiftHost` lint).
pub(crate) fn parse_class_unit(
    unit: &SourceUnit,
    includes: &BTreeMap<String, ResourceUnit>,
) -> Result<ParsedClass, String> {
    let raw_script = Script {
        language: if unit.language == ScriptLanguage::ActionScript1 {
            ScriptLanguage::ActionScript2
        } else {
            unit.language
        },
        source: unit.source.clone(),
    };
    let expanded = expand_includes_if_needed(&raw_script, &unit.path, includes)?;
    let parse_lang = expanded.language;

    // Normalize `const` inside method bodies (brace_depth >= class_body_depth + 1) so `Frontend` parses local `const` cleanly.
    let mut norm_bytes = expanded.source.as_bytes().to_vec();
    {
        let probe = Script {
            language: ScriptLanguage::ActionScript3,
            source: expanded.source.clone(),
        };
        let mut lx = Lexer {
            script: &probe,
            offset: 0,
        };
        let mut toks = Vec::new();
        while let Ok(t) = lx.next_token() {
            let end = t.kind == Kind::End;
            toks.push(t);
            if end {
                break;
            }
        }
        let has_pkg = toks
            .first()
            .is_some_and(|t| matches!(&t.kind, Kind::Ident(s) if s == "package"));
        let method_brace_min = if has_pkg { 3usize } else { 2usize };
        let mut bdepth = 0usize;
        for i in 0..toks.len() {
            match &toks[i].kind {
                Kind::Punct("{") => bdepth += 1,
                Kind::Punct("}") => bdepth = bdepth.saturating_sub(1),
                Kind::Ident(s)
                    if s == "const"
                        && bdepth >= method_brace_min
                        && toks
                            .get(i + 1)
                            .is_some_and(|nt| matches!(nt.kind, Kind::Ident(_))) =>
                {
                    let off = toks[i].offset;
                    if off + 5 <= norm_bytes.len() && &norm_bytes[off..off + 5] == b"const" {
                        norm_bytes[off..off + 5].copy_from_slice(b"var  ");
                    }
                }
                _ => {}
            }
        }
    }
    let norm_source =
        String::from_utf8(norm_bytes).map_err(|e| format!("Invalid UTF-8 in source unit: {e}"))?;
    let parse_script = Script {
        language: parse_lang,
        source: norm_source,
    };
    let ctx = build_permissive_context(&parse_script.source, parse_lang);
    let mut p = Frontend {
        script: &parse_script,
        tokens: TokenCursor::default(),
        pos: 0,
        depth: 0,
        controls: Vec::new(),
        function_depth: 0,
        next_function: 1,
        context: ctx,
        no_in: false,
        with_depth: 0,
        in_initclip: false,
    };

    let fmt_err = |d: Diagnostic| -> String {
        format!("{}:{}:{}: {}", unit.path, d.line, d.column, d.message)
    };

    let mut imports = Vec::new();
    parse_imports_into(&mut p, &mut imports).map_err(fmt_err)?;

    let packaged = p.eat("package");
    let mut package = if packaged {
        let pkg = if p.is("{") {
            String::new()
        } else {
            parse_qualified_ident(&mut p).map_err(fmt_err)?
        };
        p.expect("{").map_err(fmt_err)?;
        pkg
    } else {
        String::new()
    };

    parse_imports_into(&mut p, &mut imports).map_err(fmt_err)?;

    let mut is_public = parse_lang == ScriptLanguage::ActionScript2;
    let mut is_dynamic = false;
    loop {
        if p.eat("public") {
            is_public = true;
        } else if p.eat("internal") {
            is_public = false;
        } else if p.eat("dynamic") {
            is_dynamic = true;
        } else if p.eat("final") || p.eat("intrinsic") {
            // Accepted and ignored or handled by emitters.
        } else if p.is("use")
            && p.peek(p.pos + 1)
                .is_some_and(|t| matches!(&t.kind, Kind::Ident(s) if s == "namespace"))
        {
            p.pos += 2;
            let _ = parse_qualified_ident(&mut p);
            let _ = p.semicolon();
            parse_imports_into(&mut p, &mut imports).map_err(fmt_err)?;
        } else {
            break;
        }
    }

    let is_interface = p.eat("interface");
    if !is_interface {
        p.expect("class").map_err(fmt_err)?;
    }

    let raw_class_name = parse_qualified_ident(&mut p).map_err(fmt_err)?;
    let (short_name, qualified_name) =
        if let Some((pkg_prefix, short)) = raw_class_name.rsplit_once('.') {
            if package.is_empty() {
                package = pkg_prefix.to_owned();
            }
            (short.to_owned(), raw_class_name)
        } else if package.is_empty() {
            (raw_class_name.clone(), raw_class_name)
        } else {
            let q = format!("{package}.{raw_class_name}");
            (raw_class_name, q)
        };
    p.context.class_name = Some(qualified_name.clone());

    let mut interfaces = Vec::new();
    let base_class = if p.eat("extends") {
        if is_interface {
            loop {
                interfaces.push(parse_qualified_ident(&mut p).map_err(fmt_err)?);
                if !p.eat(",") {
                    break;
                }
            }
            None
        } else {
            Some(parse_qualified_ident(&mut p).map_err(fmt_err)?)
        }
    } else {
        None
    };

    if p.eat("implements") {
        loop {
            interfaces.push(parse_qualified_ident(&mut p).map_err(fmt_err)?);
            if !p.eat(",") {
                break;
            }
        }
    }

    p.expect("{").map_err(fmt_err)?;

    let mut fields = Vec::new();
    let mut methods = Vec::new();

    while !p.is("}") && p.token().kind != Kind::End {
        if p.eat(";") {
            continue;
        }
        if p.is("import") {
            parse_imports_into(&mut p, &mut imports).map_err(fmt_err)?;
            continue;
        }
        if p.is("use")
            && p.peek(p.pos + 1)
                .is_some_and(|t| matches!(&t.kind, Kind::Ident(s) if s == "namespace"))
        {
            p.pos += 2;
            let _ = parse_qualified_ident(&mut p);
            let _ = p.semicolon();
            continue;
        }

        let member_offset = p.token().offset;
        let mut is_static = false;
        let mut is_override = false;
        let mut is_native = false;
        let mut access = if parse_lang == ScriptLanguage::ActionScript2 || is_interface {
            MemberAccess::Public
        } else {
            MemberAccess::Internal
        };

        loop {
            match p.token_text().as_str() {
                "public" => {
                    p.pos += 1;
                    access = MemberAccess::Public;
                }
                "private" => {
                    p.pos += 1;
                    access = MemberAccess::Private;
                }
                "protected" => {
                    p.pos += 1;
                    access = MemberAccess::Protected;
                }
                "internal" => {
                    p.pos += 1;
                    access = MemberAccess::Internal;
                }
                "static" => {
                    p.pos += 1;
                    is_static = true;
                }
                "override" => {
                    p.pos += 1;
                    is_override = true;
                }
                "final" => {
                    p.pos += 1;
                }
                "native" => {
                    p.pos += 1;
                    is_native = true;
                }
                _ => break,
            }
        }

        if p.is("var") || p.is("const") {
            let is_const = p.eat("const");
            if !is_const {
                p.expect("var").map_err(fmt_err)?;
            }
            let Stmt::Var(vars) = p.variables().map_err(fmt_err)? else {
                unreachable!()
            };
            p.semicolon().map_err(fmt_err)?;
            for (name, ty, value) in vars {
                fields.push(ParsedField {
                    offset: member_offset,
                    name,
                    ty,
                    value,
                    is_const,
                    is_static,
                    access,
                });
            }
        } else if p.is("function") {
            let fn_pos_before = p.pos;
            let (param_spans, ret_span, name_span) = scan_method_signature_spans(&p, fn_pos_before);
            let (func, accessor_raw) = p
                .project_function(true, true, is_interface, is_native)
                .map_err(fmt_err)?;
            let accessor = match accessor_raw.as_deref() {
                Some("get") => Some(AccessorKind::Get),
                Some("set") => Some(AccessorKind::Set),
                _ => None,
            };
            methods.push(ParsedMethod {
                function: func,
                is_static,
                is_override,
                access,
                accessor,
                param_type_spans: param_spans,
                return_type_span: ret_span,
                name_span,
            });
        } else {
            return Err(fmt_err(
                p.error("Expected field or method declaration in class body."),
            ));
        }
    }

    p.expect("}").map_err(fmt_err)?;
    if packaged {
        let _ = p.eat("}");
    }

    Ok(ParsedClass {
        source: expanded.source,
        language: unit.language,
        package,
        short_name,
        qualified_name,
        base_class,
        interfaces,
        imports,
        is_interface,
        is_dynamic,
        is_public,
        fields,
        methods,
    })
}

type MethodSigSpans = (Vec<(usize, usize)>, Option<(usize, usize)>, (usize, usize));

/// Scan token spans for `(param_type_spans, return_type_span, method_name_span)` starting at `function`.
fn scan_method_signature_spans(p: &Frontend<'_>, fn_pos: usize) -> MethodSigSpans {
    let mut cur = fn_pos + 1; // after `function`
    if let Some(tok) = p.peek(cur)
        && matches!(&tok.kind, Kind::Ident(s) if s == "get" || s == "set")
        && p.peek(cur + 1)
            .is_some_and(|nt| matches!(nt.kind, Kind::Ident(_)))
    {
        cur += 1;
    }
    let name_span = p.peek(cur).map_or((0, 0), |tok| (tok.offset, tok.end));
    if p.peek(cur)
        .is_some_and(|tok| matches!(tok.kind, Kind::Ident(_)))
    {
        cur += 1;
    }
    let mut param_spans = Vec::new();
    if p.peek(cur).is_some_and(|t| t.kind == Kind::Punct("(")) {
        cur += 1;
        let mut paren_depth = 1usize;
        let mut cur_param_start = p.peek(cur).map_or(0, |t| t.offset);
        let mut cur_param_end = cur_param_start;
        let mut cur_type_span: Option<(usize, usize)> = None;
        while let Some(tok) = p.peek(cur) {
            if tok.kind == Kind::End {
                break;
            }
            if tok.kind == Kind::Punct("(") {
                paren_depth += 1;
            } else if tok.kind == Kind::Punct(")") {
                paren_depth -= 1;
                if paren_depth == 0 {
                    if cur_param_end > cur_param_start {
                        param_spans.push(cur_type_span.unwrap_or((cur_param_start, cur_param_end)));
                    }
                    cur += 1;
                    break;
                }
            } else if paren_depth == 1 && tok.kind == Kind::Punct(",") {
                param_spans.push(cur_type_span.unwrap_or((cur_param_start, cur_param_end)));
                cur += 1;
                cur_param_start = p.peek(cur).map_or(0, |t| t.offset);
                cur_param_end = cur_param_start;
                cur_type_span = None;
                continue;
            } else if paren_depth == 1 && tok.kind == Kind::Punct(":") {
                let ty_start = p.peek(cur + 1).map_or(tok.offset, |t| t.offset);
                let mut j = cur + 1;
                let mut ty_end = ty_start;
                while let Some(tt) = p.peek(j) {
                    if matches!(tt.kind, Kind::Punct("," | ")" | "=")) || tt.kind == Kind::End {
                        break;
                    }
                    ty_end = tt.end;
                    j += 1;
                }
                cur_type_span = Some((ty_start, ty_end));
            }
            cur_param_end = tok.end;
            cur += 1;
        }
    }

    let ret_span = if p.peek(cur).is_some_and(|t| t.kind == Kind::Punct(":")) {
        let ty_start = p.peek(cur + 1).map_or(0, |t| t.offset);
        let mut j = cur + 1;
        let mut ty_end = ty_start;
        while let Some(tt) = p.peek(j) {
            if matches!(tt.kind, Kind::Punct("{" | ";")) || tt.kind == Kind::End {
                break;
            }
            ty_end = tt.end;
            j += 1;
        }
        Some((ty_start, ty_end))
    } else {
        None
    };

    (param_spans, ret_span, name_span)
}

/// Lexical type environment for tracking local/timeline/class variable & function types.
#[derive(Clone, Debug, Default)]
pub(crate) struct TypeEnv {
    scopes: Vec<BTreeMap<String, HirType>>,
    pub(crate) functions: BTreeMap<String, HirType>,
    pub(crate) symbol_id: Option<String>,
    pub(crate) class_name: Option<String>,
}

impl TypeEnv {
    pub(crate) fn new(symbol_id: Option<String>, class_name: Option<String>) -> Self {
        Self {
            scopes: vec![BTreeMap::new()],
            functions: BTreeMap::new(),
            symbol_id,
            class_name,
        }
    }

    pub(crate) fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    pub(crate) fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    pub(crate) fn define(&mut self, name: impl Into<String>, ty: HirType) {
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name.into(), ty);
        }
    }

    pub(crate) fn lookup(&self, name: &str) -> Option<&HirType> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty);
            }
        }
        None
    }

    pub(crate) fn infer_expr(&self, expr: &Expr, stage: &ProjectStageIndex) -> HirType {
        match expr {
            Expr::Literal(raw) => {
                if raw == "true" || raw == "false" {
                    HirType::Bool
                } else if raw.starts_with('"') || raw.starts_with('\'') {
                    HirType::String
                } else if raw == "null" || raw == "undefined" {
                    HirType::Unknown
                } else if raw.starts_with("0x") || raw.starts_with("0X") {
                    HirType::Int
                } else if raw.contains('.') || raw.contains('e') || raw.contains('E') {
                    HirType::Float
                } else if raw.parse::<i64>().is_ok() {
                    HirType::Int
                } else {
                    HirType::Float
                }
            }
            Expr::Name(name) => {
                if let Some(ty) = self.lookup(name) {
                    return ty.clone();
                }
                if let Some(sym_id) = &self.symbol_id
                    && let Some(child) = stage.child_type(sym_id, name)
                {
                    return child.hir_type.clone();
                }
                if let Some(cls) = &self.class_name
                    && let Some(fields) = stage.class_fields.get(cls)
                    && let Some(ty) = fields.get(name)
                {
                    return ty.clone();
                }
                HirType::Unknown
            }
            Expr::Member(base, prop) => {
                if let Expr::Name(cls) = base.as_ref() {
                    if (cls == "Key" && map_as2_key_constant(prop).is_some())
                        || (cls == "Keyboard" && map_as3_keyboard_constant(prop).is_some())
                    {
                        return HirType::UInt;
                    }
                    if cls == "Math" {
                        return HirType::Float;
                    }
                }
                if matches!(base.as_ref(), Expr::Name(n) if n == "this") {
                    if let Some(sym_id) = &self.symbol_id
                        && let Some(child) = stage.child_type(sym_id, prop)
                    {
                        return child.hir_type.clone();
                    }
                    if let Some(cls) = &self.class_name
                        && let Some(fields) = stage.class_fields.get(cls)
                        && let Some(ty) = fields.get(prop)
                    {
                        return ty.clone();
                    }
                    if let Some(ty) = self.lookup(prop) {
                        return ty.clone();
                    }
                }
                match prop.as_str() {
                    "_x" | "_y" | "x" | "y" | "_width" | "_height" | "width" | "height"
                    | "_rotation" | "rotation" | "_alpha" | "alpha" | "_xscale" | "_yscale"
                    | "scaleX" | "scaleY" | "stageWidth" | "stageHeight" | "mouseX" | "mouseY"
                    | "_xmouse" | "_ymouse" => HirType::Float,
                    "_visible" | "visible" | "enabled" => HirType::Bool,
                    "_name" | "name" | "text" | "htmlText" => HirType::String,
                    "_currentframe" | "_totalframes" | "currentFrame" | "totalFrames"
                    | "length" | "numChildren" => HirType::Int,
                    "keyCode" | "charCode" => HirType::UInt,
                    _ => HirType::Unknown,
                }
            }
            Expr::Array(_) => HirType::Array,
            Expr::Object(_) => HirType::Object,
            Expr::NewVector(ty, _, _) => HirType::from_annotation(ty),
            Expr::NewBuiltin(ty, _, _) | Expr::NewClass(ty, _, _) | Expr::NewValue(ty, _, _) => {
                HirType::from_annotation(ty)
            }
            Expr::Unary(op, inner) => match op.as_str() {
                "!" | "not" => HirType::Bool,
                "~" => HirType::Int,
                "typeof" => HirType::String,
                "+" | "-" => self.infer_expr(inner, stage),
                _ => HirType::Unknown,
            },
            Expr::Binary(op, lhs, rhs) => match op.as_str() {
                "==" | "!=" | "===" | "!==" | "<" | "<=" | ">" | ">=" | "&&" | "||" | "and"
                | "or" | "eq" | "ne" | "<>" | "in" => HirType::Bool,
                "&" | "|" | "^" | "<<" | ">>" => HirType::Int,
                ">>>" => HirType::UInt,
                "/" => HirType::Float,
                "+" => {
                    let lt = self.infer_expr(lhs, stage);
                    let rt = self.infer_expr(rhs, stage);
                    if lt == HirType::String || rt == HirType::String {
                        HirType::String
                    } else if lt == HirType::Float || rt == HirType::Float {
                        HirType::Float
                    } else if lt.is_integer() && rt.is_integer() {
                        HirType::Int
                    } else {
                        HirType::Float
                    }
                }
                "-" | "*" | "%" => {
                    let lt = self.infer_expr(lhs, stage);
                    let rt = self.infer_expr(rhs, stage);
                    if lt == HirType::Float || rt == HirType::Float {
                        HirType::Float
                    } else if lt.is_integer() && rt.is_integer() {
                        HirType::Int
                    } else {
                        HirType::Float
                    }
                }
                _ => HirType::Unknown,
            },
            Expr::Conditional(_, yes, no) => {
                let yt = self.infer_expr(yes, stage);
                let nt = self.infer_expr(no, stage);
                if yt == nt { yt } else { HirType::Unknown }
            }
            Expr::Call(callee, _, _) => match callee.as_ref() {
                Expr::Name(fn_name) => match fn_name.as_str() {
                    "int" => HirType::Int,
                    "uint" => HirType::UInt,
                    "Number" | "parseFloat" => HirType::Float,
                    "String" => HirType::String,
                    "Boolean" | "isNaN" | "isFinite" => HirType::Bool,
                    other => self
                        .functions
                        .get(other)
                        .cloned()
                        .unwrap_or(HirType::Unknown),
                },
                Expr::Member(base, method) => match (base.as_ref(), method.as_str()) {
                    (Expr::Name(n), "getCode" | "getAscii") if n == "Key" => HirType::UInt,
                    (Expr::Name(n), "isDown") if n == "Key" => HirType::Bool,
                    (Expr::Name(n), _) if n == "Math" => HirType::Float,
                    (_, "createEmptyMovieClip") => HirType::Sprite,
                    _ => HirType::Unknown,
                },
                _ => HirType::Unknown,
            },
            Expr::TypeTest(op, _, ty_name, _) => {
                if op == "as" {
                    HirType::from_annotation(ty_name)
                } else {
                    HirType::Bool
                }
            }
            _ => HirType::Unknown,
        }
    }
}

// =========================================================================
// Shared Property, Scale/Alpha, Keyboard & Drawing Normalization Helpers
// =========================================================================

/// Map classic AS1/AS2 underscore MovieClip/TextField properties (1:1 scale) to AS3/JS/Swift names.
pub(crate) fn map_as2_direct_prop_to_modern(prop: &str) -> Option<&'static str> {
    match prop {
        "_x" => Some("x"),
        "_y" => Some("y"),
        "_visible" => Some("visible"),
        "_rotation" => Some("rotation"),
        "_width" => Some("width"),
        "_height" => Some("height"),
        "_name" => Some("name"),
        "_currentframe" => Some("currentFrame"),
        "_totalframes" => Some("totalFrames"),
        "_parent" => Some("parent"),
        "_xmouse" => Some("mouseX"),
        "_ymouse" => Some("mouseY"),
        _ => None,
    }
}

/// Map AS3/JS DisplayObject properties (1:1 scale) back to AS2 underscore properties.
pub(crate) fn map_modern_direct_prop_to_as2(prop: &str) -> Option<&'static str> {
    match prop {
        "x" => Some("_x"),
        "y" => Some("_y"),
        "visible" => Some("_visible"),
        "rotation" => Some("_rotation"),
        "width" => Some("_width"),
        "height" => Some("_height"),
        "name" => Some("_name"),
        "currentFrame" => Some("_currentframe"),
        "totalFrames" => Some("_totalframes"),
        "parent" => Some("_parent"),
        "mouseX" => Some("_xmouse"),
        "mouseY" => Some("_ymouse"),
        _ => None,
    }
}

/// Map AS1/AS2 `0..100` percentage properties to AS3/JS/Swift `0..1` properties.
pub(crate) fn map_as2_percent_prop_to_modern(prop: &str) -> Option<&'static str> {
    match prop {
        "_alpha" => Some("alpha"),
        "_xscale" => Some("scaleX"),
        "_yscale" => Some("scaleY"),
        _ => None,
    }
}

/// Map AS3/JS `0..1` properties back to AS2 `0..100` percentage properties.
pub(crate) fn map_modern_unit_prop_to_as2(prop: &str) -> Option<&'static str> {
    match prop {
        "alpha" => Some("_alpha"),
        "scaleX" => Some("_xscale"),
        "scaleY" => Some("_yscale"),
        _ => None,
    }
}

/// Extract a constant numeric value from a literal or signed literal expression.
pub(crate) fn extract_numeric_literal(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Literal(raw) => {
            if raw.starts_with("0x") || raw.starts_with("0X") {
                u64::from_str_radix(&raw[2..], 16).ok().map(|v| v as f64)
            } else {
                raw.parse::<f64>().ok().filter(|v| v.is_finite())
            }
        }
        Expr::Unary(op, inner) if op == "-" => extract_numeric_literal(inner).map(|v| -v),
        Expr::Unary(op, inner) if op == "+" => extract_numeric_literal(inner),
        _ => None,
    }
}

pub(crate) fn format_clean_number(val: f64, force_decimal: bool) -> String {
    let rounded = (val * 1e9).round() / 1e9;
    if rounded == 0.0 {
        return if force_decimal {
            "0.0".into()
        } else {
            "0".into()
        };
    }
    if (rounded - rounded.trunc()).abs() < 1e-9 && rounded.abs() < 1e15 {
        if force_decimal {
            format!("{:.1}", rounded)
        } else {
            format!("{}", rounded as i64)
        }
    } else {
        let s = format!("{rounded}");
        s
    }
}

/// Fold an AS1/AS2 `0..100` numeric literal into an AS3/JS/Swift `0..1` literal (`100` -> `"1"`, `50` -> `"0.5"`).
pub(crate) fn fold_100_to_01(expr: &Expr, force_decimal: bool) -> Option<String> {
    let v = extract_numeric_literal(expr)?;
    Some(format_clean_number(v / 100.0, force_decimal))
}

/// Fold an AS3 `0..1` numeric literal into an AS2 `0..100` literal (`1.0` -> `"100"`, `0.5` -> `"50"`).
pub(crate) fn fold_01_to_100(expr: &Expr) -> Option<String> {
    let v = extract_numeric_literal(expr)?;
    Some(format_clean_number(v * 100.0, false))
}

/// Map AS2 `Key.*` constant names to AS3 `Keyboard.*` constant names and numeric keyCodes.
pub(crate) fn map_as2_key_constant(name: &str) -> Option<(&'static str, u32)> {
    match name {
        "LEFT" => Some(("LEFT", 37)),
        "UP" => Some(("UP", 38)),
        "RIGHT" => Some(("RIGHT", 39)),
        "DOWN" => Some(("DOWN", 40)),
        "SPACE" => Some(("SPACE", 32)),
        "ENTER" => Some(("ENTER", 13)),
        "ESCAPE" => Some(("ESCAPE", 27)),
        "SHIFT" => Some(("SHIFT", 16)),
        "CONTROL" => Some(("CONTROL", 17)),
        "BACKSPACE" => Some(("BACKSPACE", 8)),
        "DELETEKEY" | "DELETE" => Some(("DELETE", 46)),
        "TAB" => Some(("TAB", 9)),
        "HOME" => Some(("HOME", 36)),
        "END" => Some(("END", 35)),
        "PGUP" | "PAGE_UP" => Some(("PAGE_UP", 33)),
        "PGDN" | "PAGE_DOWN" => Some(("PAGE_DOWN", 34)),
        "INSERT" => Some(("INSERT", 45)),
        "CAPSLOCK" | "CAPS_LOCK" => Some(("CAPS_LOCK", 20)),
        _ => None,
    }
}

/// Map AS3 `Keyboard.*` constant names to AS2 `Key.*` constant names and numeric keyCodes.
pub(crate) fn map_as3_keyboard_constant(name: &str) -> Option<(&'static str, u32)> {
    match name {
        "LEFT" => Some(("LEFT", 37)),
        "UP" => Some(("UP", 38)),
        "RIGHT" => Some(("RIGHT", 39)),
        "DOWN" => Some(("DOWN", 40)),
        "SPACE" => Some(("SPACE", 32)),
        "ENTER" => Some(("ENTER", 13)),
        "ESCAPE" => Some(("ESCAPE", 27)),
        "SHIFT" => Some(("SHIFT", 16)),
        "CONTROL" => Some(("CONTROL", 17)),
        "BACKSPACE" => Some(("BACKSPACE", 8)),
        "DELETE" => Some(("DELETEKEY", 46)),
        "TAB" => Some(("TAB", 9)),
        "HOME" => Some(("HOME", 36)),
        "END" => Some(("END", 35)),
        "PAGE_UP" => Some(("PGUP", 33)),
        "PAGE_DOWN" => Some(("PGDN", 34)),
        "INSERT" => Some(("INSERT", 45)),
        "CAPS_LOCK" => Some(("CAPSLOCK", 20)),
        _ => None,
    }
}

/// Map classic `on(keyPress "<token>")` string to keyCode / AS3 `Keyboard.*`.
pub(crate) fn key_press_spec_to_code(spec: &str) -> Option<(&'static str, u32)> {
    match spec {
        "<Left>" => Some(("Keyboard.LEFT", 37)),
        "<Right>" => Some(("Keyboard.RIGHT", 39)),
        "<Up>" => Some(("Keyboard.UP", 38)),
        "<Down>" => Some(("Keyboard.DOWN", 40)),
        "<Enter>" => Some(("Keyboard.ENTER", 13)),
        "<Space>" => Some(("Keyboard.SPACE", 32)),
        "<Tab>" => Some(("Keyboard.TAB", 9)),
        "<Escape>" => Some(("Keyboard.ESCAPE", 27)),
        "<Backspace>" => Some(("Keyboard.BACKSPACE", 8)),
        "<Delete>" => Some(("Keyboard.DELETE", 46)),
        "<Home>" => Some(("Keyboard.HOME", 36)),
        "<End>" => Some(("Keyboard.END", 35)),
        "<PageUp>" => Some(("Keyboard.PAGE_UP", 33)),
        "<PageDown>" => Some(("Keyboard.PAGE_DOWN", 34)),
        "<Insert>" => Some(("Keyboard.INSERT", 45)),
        s if s.chars().count() == 1 => {
            let ch = s.chars().next().unwrap().to_ascii_uppercase() as u32;
            Some(("", ch))
        }
        _ => None,
    }
}

/// True when `expr` is a simple atomic identifier, literal, or member access needing no outer `(...)`.
pub(crate) fn is_atomic_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Literal(_)
            | Expr::Name(_)
            | Expr::Member(_, _)
            | Expr::Index(_, _)
            | Expr::Call(_, _, _)
    )
}

/// Decode a quoted string literal `Expr::Literal("\"foo\"")` into `"foo"`.
pub(crate) fn decode_string_literal(expr: &Expr) -> Option<String> {
    let Expr::Literal(raw) = expr else {
        return None;
    };
    if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
        serde_json::from_str::<String>(raw)
            .ok()
            .or_else(|| Some(raw[1..raw.len() - 1].to_owned()))
    } else if raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2 {
        Some(raw[1..raw.len() - 1].replace("\\'", "'"))
    } else {
        None
    }
}

/// Check if an AST statement list contains any `Key.isDown(...)` calls.
pub(crate) fn stmts_use_key_is_down(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_uses_key_is_down)
}

fn stmt_uses_key_is_down(stmt: &Stmt) -> bool {
    match stmt.unlocated() {
        Stmt::Function(f) | Stmt::LegacyEvent(_, f) => stmts_use_key_is_down(&f.body),
        Stmt::Try(body, catches, fin) => {
            stmts_use_key_is_down(body)
                || catches.iter().any(|c| stmts_use_key_is_down(&c.body))
                || fin.as_ref().is_some_and(|f| stmts_use_key_is_down(f))
        }
        other => {
            crate::actionscript::scope::expressions(other)
                .into_iter()
                .any(expr_uses_key_is_down)
                || crate::actionscript::scope::statements(other)
                    .into_iter()
                    .any(stmt_uses_key_is_down)
        }
    }
}

pub(crate) fn expr_uses_key_is_down(expr: &Expr) -> bool {
    if let Expr::Call(callee, _, _) = expr
        && let Expr::Member(base, method) = callee.as_ref()
        && matches!(base.as_ref(), Expr::Name(n) if n == "Key")
        && method == "isDown"
    {
        return true;
    }
    if let Expr::Function(f) = expr
        && stmts_use_key_is_down(&f.body)
    {
        return true;
    }
    crate::actionscript::scope::children(expr)
        .into_iter()
        .any(expr_uses_key_is_down)
}

/// Rewrite `this` and `_parent` inside an AST when lifting a MovieClip `onClipEvent` or `ObjectScript`
/// onto its parent timeline's `FrameScript`.
pub(crate) fn rewrite_lifted_clip_scope_stmts(stmts: &[Stmt], instance_name: &str) -> Vec<Stmt> {
    stmts
        .iter()
        .map(|s| rewrite_lifted_clip_scope_stmt(s, instance_name))
        .collect()
}

fn rewrite_lifted_clip_scope_stmt(stmt: &Stmt, inst: &str) -> Stmt {
    match stmt {
        Stmt::Located(off, inner) => {
            Stmt::Located(*off, Box::new(rewrite_lifted_clip_scope_stmt(inner, inst)))
        }
        Stmt::Empty => Stmt::Empty,
        Stmt::Expr(e) => Stmt::Expr(rewrite_lifted_clip_scope_expr(e, inst)),
        Stmt::DefaultXmlNamespace(e) => {
            Stmt::DefaultXmlNamespace(rewrite_lifted_clip_scope_expr(e, inst))
        }
        Stmt::Var(vars) => Stmt::Var(
            vars.iter()
                .map(|(n, ty, val)| {
                    (
                        n.clone(),
                        ty.clone(),
                        val.as_ref()
                            .map(|e| rewrite_lifted_clip_scope_expr(e, inst)),
                    )
                })
                .collect(),
        ),
        Stmt::Block(body) => Stmt::Block(rewrite_lifted_clip_scope_stmts(body, inst)),
        Stmt::If(cond, yes, no) => Stmt::If(
            rewrite_lifted_clip_scope_expr(cond, inst),
            Box::new(rewrite_lifted_clip_scope_stmt(yes, inst)),
            no.as_ref()
                .map(|n| Box::new(rewrite_lifted_clip_scope_stmt(n, inst))),
        ),
        Stmt::While(cond, body) => Stmt::While(
            rewrite_lifted_clip_scope_expr(cond, inst),
            Box::new(rewrite_lifted_clip_scope_stmt(body, inst)),
        ),
        Stmt::With(t, body, id) => Stmt::With(
            rewrite_lifted_clip_scope_expr(t, inst),
            Box::new(rewrite_lifted_clip_scope_stmt(body, inst)),
            *id,
        ),
        Stmt::TellTarget(t, body, id) => Stmt::TellTarget(
            rewrite_lifted_clip_scope_expr(t, inst),
            Box::new(rewrite_lifted_clip_scope_stmt(body, inst)),
            *id,
        ),
        Stmt::IfFrameLoaded(args, body) => Stmt::IfFrameLoaded(
            args.iter()
                .map(|a| rewrite_lifted_clip_scope_expr(a, inst))
                .collect(),
            Box::new(rewrite_lifted_clip_scope_stmt(body, inst)),
        ),
        Stmt::InitClip(ord, body, id) => {
            Stmt::InitClip(*ord, rewrite_lifted_clip_scope_stmts(body, inst), *id)
        }
        Stmt::LegacyEvent(trigs, f) => Stmt::LegacyEvent(
            trigs.clone(),
            Box::new(Function {
                body: rewrite_lifted_clip_scope_stmts(&f.body, inst),
                ..(**f).clone()
            }),
        ),
        Stmt::For(init, cond, upd, body) => Stmt::For(
            Box::new(rewrite_lifted_clip_scope_stmt(init, inst)),
            cond.as_ref()
                .map(|e| rewrite_lifted_clip_scope_expr(e, inst)),
            upd.as_ref()
                .map(|e| rewrite_lifted_clip_scope_expr(e, inst)),
            Box::new(rewrite_lifted_clip_scope_stmt(body, inst)),
        ),
        Stmt::ForIn(init, coll, each, body) => Stmt::ForIn(
            Box::new(rewrite_lifted_clip_scope_stmt(init, inst)),
            rewrite_lifted_clip_scope_expr(coll, inst),
            *each,
            Box::new(rewrite_lifted_clip_scope_stmt(body, inst)),
        ),
        Stmt::DoWhile(body, cond) => Stmt::DoWhile(
            Box::new(rewrite_lifted_clip_scope_stmt(body, inst)),
            rewrite_lifted_clip_scope_expr(cond, inst),
        ),
        Stmt::Switch(disc, cases) => Stmt::Switch(
            rewrite_lifted_clip_scope_expr(disc, inst),
            cases
                .iter()
                .map(|c| Case {
                    test: c
                        .test
                        .as_ref()
                        .map(|e| rewrite_lifted_clip_scope_expr(e, inst)),
                    body: rewrite_lifted_clip_scope_stmts(&c.body, inst),
                })
                .collect(),
        ),
        Stmt::Label(lbl, body) => Stmt::Label(
            lbl.clone(),
            Box::new(rewrite_lifted_clip_scope_stmt(body, inst)),
        ),
        Stmt::Break(l) => Stmt::Break(l.clone()),
        Stmt::Continue(l) => Stmt::Continue(l.clone()),
        Stmt::Throw(e) => Stmt::Throw(rewrite_lifted_clip_scope_expr(e, inst)),
        Stmt::Try(body, catches, fin) => Stmt::Try(
            rewrite_lifted_clip_scope_stmts(body, inst),
            catches
                .iter()
                .map(|c| CatchClause {
                    body: rewrite_lifted_clip_scope_stmts(&c.body, inst),
                    ..c.clone()
                })
                .collect(),
            fin.as_ref()
                .map(|f| rewrite_lifted_clip_scope_stmts(f, inst)),
        ),
        Stmt::Function(f) => Stmt::Function(Box::new(Function {
            body: rewrite_lifted_clip_scope_stmts(&f.body, inst),
            ..(**f).clone()
        })),
        Stmt::Return(val, off) => Stmt::Return(
            val.as_ref()
                .map(|e| rewrite_lifted_clip_scope_expr(e, inst)),
            *off,
        ),
    }
}

/// A display-list name can contain spaces and punctuation. Keep its exact key
/// when accessing the child from converted ActionScript or JavaScript.
fn lifted_instance(name: &str) -> Expr {
    let parent = Box::new(Expr::Name("this".into()));
    let mut chars = name.chars();
    if name != "this"
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    {
        Expr::Member(parent, name.to_owned())
    } else {
        Expr::Index(
            parent,
            Box::new(Expr::Literal(serde_json::Value::from(name).to_string())),
        )
    }
}

fn rewrite_lifted_clip_scope_expr(expr: &Expr, inst: &str) -> Expr {
    match expr {
        Expr::Name(n) if n == "this" => lifted_instance(inst),
        Expr::Name(n) if n == "_parent" => Expr::Name("this".into()),
        Expr::Member(base, prop)
            if matches!(base.as_ref(), Expr::Name(n) if n == "this") && prop == "_parent" =>
        {
            Expr::Name("this".into())
        }
        Expr::Name(n)
            if matches!(
                n.as_str(),
                "_x" | "_y"
                    | "_xscale"
                    | "_yscale"
                    | "_alpha"
                    | "_visible"
                    | "_rotation"
                    | "_width"
                    | "_height"
                    | "_name"
                    | "_currentframe"
                    | "_totalframes"
                    | "x"
                    | "y"
                    | "scaleX"
                    | "scaleY"
                    | "alpha"
                    | "visible"
                    | "rotation"
            ) =>
        {
            Expr::Member(Box::new(lifted_instance(inst)), n.clone())
        }
        Expr::Call(callee, args, off) => {
            let new_args: Vec<Expr> = args
                .iter()
                .map(|a| rewrite_lifted_clip_scope_expr(a, inst))
                .collect();
            if let Expr::Name(fn_name) = callee.as_ref()
                && matches!(
                    fn_name.as_str(),
                    "play" | "stop" | "gotoAndPlay" | "gotoAndStop" | "nextFrame" | "prevFrame"
                )
            {
                return Expr::Call(
                    Box::new(Expr::Member(
                        Box::new(lifted_instance(inst)),
                        fn_name.clone(),
                    )),
                    new_args,
                    *off,
                );
            }
            Expr::Call(
                Box::new(rewrite_lifted_clip_scope_expr(callee, inst)),
                new_args,
                *off,
            )
        }
        Expr::Member(base, prop) => Expr::Member(
            Box::new(rewrite_lifted_clip_scope_expr(base, inst)),
            prop.clone(),
        ),
        Expr::Index(base, idx) => Expr::Index(
            Box::new(rewrite_lifted_clip_scope_expr(base, inst)),
            Box::new(rewrite_lifted_clip_scope_expr(idx, inst)),
        ),
        Expr::Unary(op, inner) => Expr::Unary(
            op.clone(),
            Box::new(rewrite_lifted_clip_scope_expr(inner, inst)),
        ),
        Expr::Binary(op, l, r) => Expr::Binary(
            op.clone(),
            Box::new(rewrite_lifted_clip_scope_expr(l, inst)),
            Box::new(rewrite_lifted_clip_scope_expr(r, inst)),
        ),
        Expr::Assign(op, l, r) => Expr::Assign(
            op.clone(),
            Box::new(rewrite_lifted_clip_scope_expr(l, inst)),
            Box::new(rewrite_lifted_clip_scope_expr(r, inst)),
        ),
        Expr::Update(op, inner, pre) => Expr::Update(
            op.clone(),
            Box::new(rewrite_lifted_clip_scope_expr(inner, inst)),
            *pre,
        ),
        Expr::Conditional(c, y, n) => Expr::Conditional(
            Box::new(rewrite_lifted_clip_scope_expr(c, inst)),
            Box::new(rewrite_lifted_clip_scope_expr(y, inst)),
            Box::new(rewrite_lifted_clip_scope_expr(n, inst)),
        ),
        Expr::Array(items) => Expr::Array(
            items
                .iter()
                .map(|it| it.as_ref().map(|e| rewrite_lifted_clip_scope_expr(e, inst)))
                .collect(),
        ),
        Expr::Object(props) => Expr::Object(
            props
                .iter()
                .map(|(PropertyKey { source, name }, val)| {
                    (
                        PropertyKey {
                            source: source.clone(),
                            name: name.clone(),
                        },
                        rewrite_lifted_clip_scope_expr(val, inst),
                    )
                })
                .collect(),
        ),
        Expr::Function(f) => Expr::Function(Box::new(Function {
            body: rewrite_lifted_clip_scope_stmts(&f.body, inst),
            ..(**f).clone()
        })),
        Expr::InstanceOf(a, b) => Expr::InstanceOf(
            Box::new(rewrite_lifted_clip_scope_expr(a, inst)),
            Box::new(rewrite_lifted_clip_scope_expr(b, inst)),
        ),
        Expr::TypeTest(op, e, ty, off) => Expr::TypeTest(
            op.clone(),
            Box::new(rewrite_lifted_clip_scope_expr(e, inst)),
            ty.clone(),
            *off,
        ),
        Expr::Delete(e, off) => {
            Expr::Delete(Box::new(rewrite_lifted_clip_scope_expr(e, inst)), *off)
        }
        other => other.clone(),
    }
}

/// Represent a lifted object event ready to be emitted on the parent timeline's `FrameScript`.
#[derive(Clone, Debug)]
pub(crate) struct LiftedObjectHandler {
    pub(crate) instance_name: Option<String>,
    /// True when `this` inside `body` refers to the parent timeline (e.g. Button `on(release)` / `Click`).
    pub(crate) parent_is_this: bool,
    pub(crate) trigger: LiftedTriggerKind,
    pub(crate) body: Vec<Stmt>,
}

impl LiftedObjectHandler {
    pub(crate) fn target(&self) -> Expr {
        self.instance_name
            .as_deref()
            .map(lifted_instance)
            .unwrap_or_else(|| Expr::Name("this".into()))
    }

    pub(crate) fn scoped_body(&self) -> Vec<Stmt> {
        if !self.parent_is_this
            && let Some(name) = &self.instance_name
        {
            rewrite_lifted_clip_scope_stmts(&self.body, name)
        } else {
            self.body.clone()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LiftedTriggerKind {
    Load,
    Unload,
    StageMouseDown,
    StageMouseMove,
    StageMouseUp,
    NeedsBinding(&'static str),
    EnterFrame,
    MouseClick,
    MouseDown,
    MouseUp,
    RollOver,
    RollOut,
    KeyPress(String),
    KeyDown,
    KeyUp,
}

impl LiftedTriggerKind {
    pub(crate) fn from_legacy(trig: &LegacyTrigger) -> Option<Self> {
        if trig.clip_event {
            match trig.event {
                "load" => Some(Self::Load),
                "enterFrame" => Some(Self::EnterFrame),
                "unload" => Some(Self::Unload),
                "mouseDown" => Some(Self::StageMouseDown),
                "mouseMove" => Some(Self::StageMouseMove),
                "mouseUp" => Some(Self::StageMouseUp),
                "keyDown" => Some(Self::KeyDown),
                "keyUp" => Some(Self::KeyUp),
                _ => Some(Self::NeedsBinding(trig.event)),
            }
        } else {
            match trig.event {
                "release" => Some(Self::MouseClick),
                "press" => Some(Self::MouseDown),
                "releaseOutside" => Some(Self::MouseUp),
                "rollOver" | "dragOver" => Some(Self::RollOver),
                "rollOut" | "dragOut" => Some(Self::RollOut),
                "keyPress" => trig.key_press.clone().map(Self::KeyPress),
                _ => Some(Self::NeedsBinding(trig.event)),
            }
        }
    }
}
