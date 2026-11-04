//! Static TypeScript type checker and analyzer over the Oxc AST.
//!
//! Validates primitive, literal, array (`T[]` / `Array<T>`), tuple, union (`A | B`),
//! intersection (`A & B`), object, interface, type alias, class (`extends` / `implements`),
//! and function (`(a: T) => R`) annotations before Oxc strips TypeScript syntax.

use crate::Diagnostic;
use neoflash_core::Script;
use oxc_ast::ast::*;
use oxc_span::GetSpan;
use std::collections::{BTreeMap, BTreeSet};

/// Structured representation of a TypeScript static type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TsType {
    Number,
    String,
    Boolean,
    Void,
    Null,
    Undefined,
    Any,
    Unknown,
    Never,
    LiteralString(String),
    LiteralNumber(String),
    LiteralBoolean(bool),
    Array(Box<TsType>),
    Tuple(Vec<TsType>),
    Union(Vec<TsType>),
    Intersection(Vec<TsType>),
    Object(BTreeMap<String, TsPropertySig>),
    Function(TsFunctionSig),
    Named(String, Vec<TsType>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TsPropertySig {
    pub ty: TsType,
    pub optional: bool,
    pub readonly: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TsFunctionSig {
    pub params: Vec<TsType>,
    pub min_params: usize,
    pub has_rest: bool,
    pub return_type: Box<TsType>,
}

#[derive(Clone, Debug)]
pub struct TsVarInfo {
    pub ty: TsType,
    pub is_const: bool,
    pub span_offset: usize,
    pub used: bool,
    pub has_explicit_type: bool,
}

/// Exported types and value bindings of a TypeScript/JavaScript module.
#[derive(Clone, Debug, Default)]
pub struct ModuleTypeExports {
    pub values: BTreeMap<String, TsType>,
    pub types: BTreeMap<String, TsType>,
    pub constructors: BTreeMap<String, TsFunctionSig>,
    pub default_export: Option<TsType>,
}

impl TsType {
    pub fn display_name(&self) -> String {
        match self {
            Self::Number => "number".into(),
            Self::String => "string".into(),
            Self::Boolean => "boolean".into(),
            Self::Void => "void".into(),
            Self::Null => "null".into(),
            Self::Undefined => "undefined".into(),
            Self::Any => "any".into(),
            Self::Unknown => "unknown".into(),
            Self::Never => "never".into(),
            Self::LiteralString(s) => format!("{s:?}"),
            Self::LiteralNumber(n) => n.clone(),
            Self::LiteralBoolean(b) => b.to_string(),
            Self::Array(elem) => format!("{}[]", elem.display_name()),
            Self::Tuple(items) => {
                let inner: Vec<String> = items.iter().map(Self::display_name).collect();
                format!("[{}]", inner.join(", "))
            }
            Self::Union(parts) => {
                let inner: Vec<String> = parts.iter().map(Self::display_name).collect();
                inner.join(" | ")
            }
            Self::Intersection(parts) => {
                let inner: Vec<String> = parts.iter().map(Self::display_name).collect();
                inner.join(" & ")
            }
            Self::Object(props) => {
                if props.is_empty() {
                    "object".into()
                } else {
                    let fields: Vec<String> = props
                        .iter()
                        .map(|(k, v)| {
                            let opt = if v.optional { "?" } else { "" };
                            format!("{k}{opt}: {}", v.ty.display_name())
                        })
                        .collect();
                    format!("{{ {} }}", fields.join("; "))
                }
            }
            Self::Function(sig) => {
                let params: Vec<String> = sig.params.iter().map(Self::display_name).collect();
                format!(
                    "({}) => {}",
                    params.join(", "),
                    sig.return_type.display_name()
                )
            }
            Self::Named(name, args) => {
                if args.is_empty() {
                    name.clone()
                } else {
                    let inner: Vec<String> = args.iter().map(Self::display_name).collect();
                    format!("{name}<{}>", inner.join(", "))
                }
            }
        }
    }

    /// Widen literal types for mutable bindings (`let x = 1` -> `number`).
    pub fn widen(&self) -> Self {
        match self {
            Self::LiteralNumber(_) => Self::Number,
            Self::LiteralString(_) => Self::String,
            Self::LiteralBoolean(_) => Self::Boolean,
            Self::Array(elem) => Self::Array(Box::new(elem.widen())),
            other => other.clone(),
        }
    }
}

/// Compute Levenshtein edit distance between two strings.
pub fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0usize; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = usize::from(a_chars[i - 1] != b_chars[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Find the closest identifier match within an edit distance threshold (`<= 2`).
pub fn suggest_identifier<'a>(
    typo: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<(&'a str, usize)> {
    if typo.len() < 2 {
        return None;
    }
    let max_dist = if typo.len() <= 2 { 1 } else { 2 };
    let mut best: Option<(&'a str, usize)> = None;
    for cand in candidates {
        if cand == typo || cand.starts_with("__nf") || cand.starts_with("__neoflash") {
            continue;
        }
        if cand.len().abs_diff(typo.len()) > max_dist {
            continue;
        }
        let dist = levenshtein_distance(typo, cand);
        if dist > 0 && dist <= max_dist && best.is_none_or(|(_, b_dist)| dist < b_dist) {
            best = Some((cand, dist));
        }
    }
    best
}

fn is_builtin_type_name(name: &str) -> bool {
    matches!(
        name,
        "Object"
            | "Function"
            | "Boolean"
            | "Number"
            | "String"
            | "Symbol"
            | "BigInt"
            | "Array"
            | "ReadonlyArray"
            | "Record"
            | "Partial"
            | "Required"
            | "Readonly"
            | "Pick"
            | "Omit"
            | "Promise"
            | "Date"
            | "RegExp"
            | "Error"
            | "TypeError"
            | "RangeError"
            | "SyntaxError"
            | "ReferenceError"
            | "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "ArrayBuffer"
            | "DataView"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int8Array"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Float32Array"
            | "Float64Array"
            | "HTMLElement"
            | "HTMLDivElement"
            | "HTMLSpanElement"
            | "HTMLButtonElement"
            | "HTMLInputElement"
            | "HTMLCanvasElement"
            | "HTMLImageElement"
            | "HTMLAudioElement"
            | "HTMLVideoElement"
            | "HTMLFormElement"
            | "HTMLAnchorElement"
            | "Element"
            | "Node"
            | "Document"
            | "Window"
            | "Event"
            | "MouseEvent"
            | "KeyboardEvent"
            | "PointerEvent"
            | "FocusEvent"
            | "TouchEvent"
            | "CustomEvent"
            | "EventTarget"
            | "CanvasRenderingContext2D"
            | "Audio"
            | "Image"
            | "URL"
            | "URLSearchParams"
            | "Request"
            | "Response"
            | "Headers"
            | "MovieClip"
            | "Sprite"
            | "DisplayObject"
            | "DisplayObjectContainer"
            | "InteractiveObject"
            | "SimpleButton"
            | "Button"
            | "TextField"
            | "TextFormat"
            | "Stage"
            | "Shape"
            | "Graphics"
            | "Bitmap"
            | "BitmapData"
            | "Point"
            | "Rectangle"
            | "Matrix"
            | "Vector3D"
            | "Matrix3D"
            | "ColorTransform"
            | "Sound"
            | "SoundChannel"
            | "SoundTransform"
            | "SoundMixer"
            | "Video"
            | "NetConnection"
            | "NetStream"
            | "Loader"
            | "LoaderInfo"
            | "URLRequest"
            | "URLLoader"
            | "URLVariables"
            | "LoadVars"
            | "Timer"
            | "TimerEvent"
            | "ByteArray"
            | "SharedObject"
            | "XML"
            | "XMLList"
            | "XMLNode"
            | "ExternalInterface"
            | "JSX.Element"
            | "React.ReactNode"
            | "React.JSX.Element"
    )
}

pub struct TypeChecker<'s> {
    script: &'s Script,
    prefix_len: usize,
    pub types: BTreeMap<String, TsType>,
    pub constructors: BTreeMap<String, TsFunctionSig>,
    pub class_parents: BTreeMap<String, String>,
    pub class_interfaces: BTreeMap<String, Vec<String>>,
    pub scopes: Vec<BTreeMap<String, TsVarInfo>>,
    pub return_stack: Vec<(TsType, bool)>,
    pub diagnostics: Vec<Diagnostic>,
    pub exports: ModuleTypeExports,
}

impl<'s> TypeChecker<'s> {
    pub fn new(script: &'s Script, prefix_len: usize) -> Self {
        let mut root_scope = BTreeMap::new();
        // Register standard globals so handlers and modules can call them while still
        // checking user-declared types strictly.
        for global in [
            "this",
            "window",
            "document",
            "globalThis",
            "console",
            "Math",
            "JSON",
            "Number",
            "String",
            "Boolean",
            "Object",
            "Array",
            "Date",
            "RegExp",
            "Error",
            "Promise",
            "Map",
            "Set",
            "WeakMap",
            "WeakSet",
            "Symbol",
            "BigInt",
            "Int32Array",
            "Uint8Array",
            "Float64Array",
            "ArrayBuffer",
            "Audio",
            "Image",
            "URL",
            "setTimeout",
            "clearTimeout",
            "setInterval",
            "clearInterval",
            "requestAnimationFrame",
            "cancelAnimationFrame",
            "fetch",
            "parseInt",
            "parseFloat",
            "isNaN",
            "isFinite",
            "encodeURI",
            "decodeURI",
            "encodeURIComponent",
            "decodeURIComponent",
            "trace",
            "play",
            "stop",
            "gotoAndPlay",
            "gotoAndStop",
            "nextFrame",
            "prevFrame",
            "stopAllSounds",
            "_root",
            "_parent",
            "event",
            "MovieClip",
            "Sprite",
            "TextField",
            "SimpleButton",
            "Button",
            "Shape",
            "Bitmap",
            "BitmapData",
            "Video",
            "Sound",
            "Point",
            "Rectangle",
            "Matrix",
            "Stage",
            "__nfJsx",
            "__nfFragment",
            "__nfProgram",
            "__nfDebug",
            "__nfDebugStep",
        ] {
            root_scope.insert(
                global.to_owned(),
                TsVarInfo {
                    ty: TsType::Any,
                    is_const: true,
                    span_offset: 0,
                    used: true,
                    has_explicit_type: true,
                },
            );
        }

        Self {
            script,
            prefix_len,
            types: BTreeMap::new(),
            constructors: BTreeMap::new(),
            class_parents: BTreeMap::new(),
            class_interfaces: BTreeMap::new(),
            scopes: vec![root_scope],
            return_stack: Vec::new(),
            diagnostics: Vec::new(),
            exports: ModuleTypeExports::default(),
        }
    }

    pub fn import_binding(&mut self, local_name: String, ty: TsType) {
        if let Some(scope) = self.scopes.first_mut() {
            scope.insert(
                local_name,
                TsVarInfo {
                    ty,
                    is_const: true,
                    span_offset: 0,
                    used: false,
                    has_explicit_type: true,
                },
            );
        }
    }

    pub fn import_type(&mut self, local_name: String, ty: TsType) {
        self.types.insert(local_name, ty);
    }

    pub fn import_constructor(&mut self, local_name: String, sig: TsFunctionSig) {
        self.constructors.insert(local_name, sig);
    }

    fn error(&mut self, span_start: u32, message: impl Into<String>) {
        let offset = (span_start as usize).saturating_sub(self.prefix_len);
        self.diagnostics
            .push(Diagnostic::at(self.script, offset, message));
    }

    fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    fn pop_scope(&mut self) -> BTreeMap<String, TsVarInfo> {
        self.scopes.pop().unwrap_or_default()
    }

    fn declare_var(
        &mut self,
        name: String,
        ty: TsType,
        is_const: bool,
        span_start: u32,
        has_explicit_type: bool,
    ) {
        let offset = (span_start as usize).saturating_sub(self.prefix_len);
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                name,
                TsVarInfo {
                    ty,
                    is_const,
                    span_offset: offset,
                    used: false,
                    has_explicit_type,
                },
            );
        }
    }

    fn lookup_var(&mut self, name: &str) -> Option<TsType> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                info.used = true;
                return Some(info.ty.clone());
            }
        }
        None
    }

    fn lookup_var_readonly(&self, name: &str) -> Option<&TsVarInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info);
            }
        }
        None
    }

    fn all_visible_names(&self) -> Vec<&str> {
        let mut out = Vec::new();
        for scope in &self.scopes {
            for k in scope.keys() {
                out.push(k.as_str());
            }
        }
        out
    }

    fn resolve_type(&self, ty: &TsType) -> TsType {
        match ty {
            TsType::Named(name, args) if args.is_empty() => {
                if let Some(resolved) = self.types.get(name) {
                    resolved.clone()
                } else {
                    ty.clone()
                }
            }
            TsType::Array(inner) => TsType::Array(Box::new(self.resolve_type(inner))),
            TsType::Union(parts) => {
                TsType::Union(parts.iter().map(|p| self.resolve_type(p)).collect())
            }
            TsType::Intersection(parts) => {
                TsType::Intersection(parts.iter().map(|p| self.resolve_type(p)).collect())
            }
            other => other.clone(),
        }
    }

    pub fn parse_ts_type(&mut self, ts_type: &TSType<'_>) -> TsType {
        match ts_type {
            TSType::TSNumberKeyword(_) => TsType::Number,
            TSType::TSStringKeyword(_) => TsType::String,
            TSType::TSBooleanKeyword(_) => TsType::Boolean,
            TSType::TSVoidKeyword(_) => TsType::Void,
            TSType::TSNullKeyword(_) => TsType::Null,
            TSType::TSUndefinedKeyword(_) => TsType::Undefined,
            TSType::TSAnyKeyword(_) => TsType::Any,
            TSType::TSUnknownKeyword(_) => TsType::Unknown,
            TSType::TSNeverKeyword(_) => TsType::Never,
            TSType::TSObjectKeyword(_) => TsType::Object(BTreeMap::new()),
            TSType::TSLiteralType(lit) => match &lit.literal {
                TSLiteral::StringLiteral(s) => TsType::LiteralString(s.value.to_string()),
                TSLiteral::NumericLiteral(n) => TsType::LiteralNumber(n.value.to_string()),
                TSLiteral::BooleanLiteral(b) => TsType::LiteralBoolean(b.value),
                TSLiteral::UnaryExpression(u) => {
                    if let Expression::NumericLiteral(n) = &u.argument {
                        TsType::LiteralNumber(format!("-{}", n.value))
                    } else {
                        TsType::Number
                    }
                }
                _ => TsType::Any,
            },
            TSType::TSArrayType(arr) => {
                let elem = self.parse_ts_type(&arr.element_type);
                TsType::Array(Box::new(elem))
            }
            TSType::TSTupleType(tuple) => {
                let elems = tuple
                    .element_types
                    .iter()
                    .map(|el| match el {
                        TSTupleElement::TSOptionalType(opt) => {
                            self.parse_ts_type(&opt.type_annotation)
                        }
                        TSTupleElement::TSRestType(rest) => {
                            self.parse_ts_type(&rest.type_annotation)
                        }
                        TSTupleElement::TSNamedTupleMember(named) => {
                            self.parse_ts_type(named.element_type.to_ts_type())
                        }
                        _ => self.parse_ts_type(el.to_ts_type()),
                    })
                    .collect();
                TsType::Tuple(elems)
            }
            TSType::TSUnionType(union) => {
                let parts = union.types.iter().map(|t| self.parse_ts_type(t)).collect();
                TsType::Union(parts)
            }
            TSType::TSIntersectionType(inter) => {
                let parts = inter.types.iter().map(|t| self.parse_ts_type(t)).collect();
                TsType::Intersection(parts)
            }
            TSType::TSParenthesizedType(paren) => self.parse_ts_type(&paren.type_annotation),
            TSType::TSTypeLiteral(lit) => {
                let props = self.parse_signatures(&lit.members);
                TsType::Object(props)
            }
            TSType::TSFunctionType(func) => {
                let sig =
                    self.parse_function_params_and_return(&func.params, Some(&func.return_type));
                TsType::Function(sig)
            }
            TSType::TSTypeReference(type_ref) => {
                let name = ts_type_name_to_string(&type_ref.type_name);
                let type_args: Vec<TsType> = type_ref
                    .type_arguments
                    .as_ref()
                    .map(|params| {
                        params
                            .params
                            .iter()
                            .map(|t| self.parse_ts_type(t))
                            .collect()
                    })
                    .unwrap_or_default();
                match name.as_str() {
                    "number" | "Number" => TsType::Number,
                    "string" | "String" => TsType::String,
                    "boolean" | "Boolean" => TsType::Boolean,
                    "void" => TsType::Void,
                    "any" => TsType::Any,
                    "unknown" => TsType::Unknown,
                    "never" => TsType::Never,
                    "Array" | "ReadonlyArray" => {
                        let elem = type_args.into_iter().next().unwrap_or(TsType::Any);
                        TsType::Array(Box::new(elem))
                    }
                    _ => {
                        if !self.types.contains_key(&name) && !is_builtin_type_name(&name) {
                            let candidates: Vec<&str> =
                                self.types.keys().map(String::as_str).collect();
                            let hint = suggest_identifier(&name, candidates)
                                .map(|(best, _)| format!(" Did you mean '{best}'?"))
                                .unwrap_or_default();
                            self.error(
                                type_ref.span.start,
                                format!("Cannot find type '{name}'.{hint}"),
                            );
                        }
                        TsType::Named(name, type_args)
                    }
                }
            }
            _ => TsType::Any,
        }
    }

    fn parse_signatures(&mut self, members: &[TSSignature<'_>]) -> BTreeMap<String, TsPropertySig> {
        let mut props = BTreeMap::new();
        for member in members {
            match member {
                TSSignature::TSPropertySignature(prop) => {
                    if let Some(key) = property_key_name(&prop.key) {
                        let ty = prop
                            .type_annotation
                            .as_ref()
                            .map(|ann| self.parse_ts_type(&ann.type_annotation))
                            .unwrap_or(TsType::Any);
                        props.insert(
                            key,
                            TsPropertySig {
                                ty,
                                optional: prop.optional,
                                readonly: prop.readonly,
                            },
                        );
                    }
                }
                TSSignature::TSMethodSignature(method) => {
                    if let Some(key) = property_key_name(&method.key) {
                        let sig = self.parse_function_params_and_return(
                            &method.params,
                            method.return_type.as_ref(),
                        );
                        props.insert(
                            key,
                            TsPropertySig {
                                ty: TsType::Function(sig),
                                optional: method.optional,
                                readonly: false,
                            },
                        );
                    }
                }
                _ => {}
            }
        }
        props
    }

    fn parse_function_params_and_return(
        &mut self,
        params: &FormalParameters<'_>,
        return_type: Option<&oxc_allocator::Box<'_, TSTypeAnnotation<'_>>>,
    ) -> TsFunctionSig {
        let mut param_types = Vec::new();
        let mut min_params = 0usize;
        for param in &params.items {
            let ty = param
                .pattern
                .type_annotation
                .as_ref()
                .map(|ann| self.parse_ts_type(&ann.type_annotation))
                .unwrap_or(TsType::Any);
            let is_optional = param.pattern.optional
                || matches!(param.pattern.kind, BindingPatternKind::AssignmentPattern(_));
            if !is_optional {
                min_params = param_types.len() + 1;
            }
            param_types.push(ty);
        }
        let has_rest = params.rest.is_some();
        let ret = return_type
            .map(|ann| self.parse_ts_type(&ann.type_annotation))
            .unwrap_or(TsType::Any);
        TsFunctionSig {
            params: param_types,
            min_params,
            has_rest,
            return_type: Box::new(ret),
        }
    }

    /// Check whether `source` is assignable to `target`.
    pub fn is_assignable(&self, source: &TsType, target: &TsType) -> bool {
        if source == target {
            return true;
        }
        match (source, target) {
            (TsType::Any, _) | (_, TsType::Any) => true,
            (TsType::Never, _) => true,
            (_, TsType::Never) => false,
            (_, TsType::Unknown) => true,
            (TsType::Unknown, _) => false,
            (TsType::LiteralNumber(_), TsType::Number) => true,
            (TsType::LiteralString(_), TsType::String) => true,
            (TsType::LiteralBoolean(_), TsType::Boolean) => true,
            (TsType::Undefined, TsType::Void) | (TsType::Void, TsType::Undefined) => true,
            (TsType::Union(sources), target) => {
                sources.iter().all(|s| self.is_assignable(s, target))
            }
            (source, TsType::Union(targets)) => {
                if let TsType::Named(s_name, _) = source
                    && let Some(TsType::Union(sources)) = self.types.get(s_name)
                {
                    return sources.iter().all(|s| self.is_assignable(s, target));
                }
                targets.iter().any(|t| self.is_assignable(source, t))
            }
            (TsType::Intersection(sources), target) => {
                sources.iter().any(|s| self.is_assignable(s, target))
            }
            (source, TsType::Intersection(targets)) => {
                targets.iter().all(|t| self.is_assignable(source, t))
            }
            (TsType::Array(s_elem), TsType::Array(t_elem)) => self.is_assignable(s_elem, t_elem),
            (TsType::Tuple(s_elems), TsType::Array(t_elem)) => {
                s_elems.iter().all(|s| self.is_assignable(s, t_elem))
            }
            (TsType::Tuple(s_elems), TsType::Tuple(t_elems)) => {
                s_elems.len() == t_elems.len()
                    && s_elems
                        .iter()
                        .zip(t_elems.iter())
                        .all(|(s, t)| self.is_assignable(s, t))
            }
            (TsType::Named(s_name, s_args), TsType::Named(t_name, t_args)) => {
                if s_name == t_name {
                    return s_args.len() == t_args.len()
                        && s_args
                            .iter()
                            .zip(t_args.iter())
                            .all(|(s, t)| self.is_assignable(s, t));
                }
                // Check class inheritance chain (`s_name extends ... -> t_name`)
                // or interface implementation (`s_name implements t_name`).
                let mut cur = Some(s_name.as_str());
                let mut visited = BTreeSet::new();
                while let Some(c) = cur {
                    if !visited.insert(c) {
                        break;
                    }
                    if c == t_name {
                        return true;
                    }
                    if let Some(ifaces) = self.class_interfaces.get(c)
                        && ifaces.iter().any(|i| i == t_name)
                    {
                        return true;
                    }
                    cur = self.class_parents.get(c).map(String::as_str);
                }
                // Structural comparison if either resolves to a registered interface/type alias/class.
                let s_resolved = self.types.get(s_name);
                let t_resolved = self.types.get(t_name);
                match (s_resolved, t_resolved) {
                    (Some(sr), Some(tr)) => self.is_assignable(sr, tr),
                    (Some(sr), None) => self.is_assignable(sr, target),
                    (None, Some(tr)) => self.is_assignable(source, tr),
                    (None, None) => false,
                }
            }
            (TsType::Named(s_name, _), target) => {
                if let Some(resolved) = self.types.get(s_name) {
                    self.is_assignable(resolved, target)
                } else {
                    is_builtin_type_name(s_name) && matches!(target, TsType::Object(_))
                }
            }
            (source, TsType::Named(t_name, _)) => {
                if let Some(resolved) = self.types.get(t_name) {
                    self.is_assignable(source, resolved)
                } else {
                    false
                }
            }
            (TsType::Object(s_props), TsType::Object(t_props)) => {
                for (k, t_prop) in t_props {
                    match s_props.get(k) {
                        Some(s_prop) => {
                            if !self.is_assignable(&s_prop.ty, &t_prop.ty) {
                                return false;
                            }
                        }
                        None => {
                            if !t_prop.optional
                                && !self.is_assignable(&TsType::Undefined, &t_prop.ty)
                            {
                                return false;
                            }
                        }
                    }
                }
                true
            }
            (TsType::Function(s_sig), TsType::Function(t_sig)) => {
                if s_sig.min_params > t_sig.params.len() && !t_sig.has_rest {
                    return false;
                }
                for (s_param, t_param) in s_sig.params.iter().zip(t_sig.params.iter()) {
                    if !self.is_assignable(t_param, s_param) {
                        return false;
                    }
                }
                if *t_sig.return_type == TsType::Void {
                    true
                } else {
                    self.is_assignable(&s_sig.return_type, &t_sig.return_type)
                }
            }
            _ => false,
        }
    }

    /// Pre-collect top-level type declarations (interfaces, type aliases, classes, functions)
    /// in a statement list so forward references within the same scope work naturally.
    fn collect_declarations(&mut self, stmts: &[Statement<'_>], is_module_top: bool) {
        for stmt in stmts {
            match stmt {
                Statement::TSInterfaceDeclaration(iface) => {
                    self.register_interface(iface, is_module_top);
                }
                Statement::TSTypeAliasDeclaration(alias) => {
                    self.register_type_alias(alias, is_module_top);
                }
                Statement::TSEnumDeclaration(enum_decl) => {
                    self.register_enum(enum_decl, is_module_top);
                }
                Statement::ClassDeclaration(class) => {
                    self.register_class_header(class, is_module_top);
                }
                Statement::FunctionDeclaration(func) => {
                    self.register_function_header(func, is_module_top);
                }
                Statement::ExportNamedDeclaration(export) => {
                    if let Some(decl) = &export.declaration {
                        match decl {
                            Declaration::TSInterfaceDeclaration(iface) => {
                                let name = iface.id.name.to_string();
                                self.register_interface(iface, true);
                                if let Some(ty) = self.types.get(&name).cloned() {
                                    self.exports.types.insert(name, ty);
                                }
                            }
                            Declaration::TSTypeAliasDeclaration(alias) => {
                                let name = alias.id.name.to_string();
                                self.register_type_alias(alias, true);
                                if let Some(ty) = self.types.get(&name).cloned() {
                                    self.exports.types.insert(name, ty);
                                }
                            }
                            Declaration::TSEnumDeclaration(enum_decl) => {
                                let name = enum_decl.id.name.to_string();
                                self.register_enum(enum_decl, true);
                                if let Some(ty) = self.types.get(&name).cloned() {
                                    self.exports.types.insert(name.clone(), ty);
                                }
                                if let Some(info) = self.lookup_var_readonly(&name) {
                                    self.exports.values.insert(name, info.ty.clone());
                                }
                            }
                            Declaration::ClassDeclaration(class) => {
                                self.register_class_header(class, true);
                                if let Some(id) = &class.id {
                                    let name = id.name.to_string();
                                    if let Some(ty) = self.types.get(&name).cloned() {
                                        self.exports.types.insert(name.clone(), ty);
                                        self.exports.values.insert(
                                            name.clone(),
                                            TsType::Named(name.clone(), Vec::new()),
                                        );
                                    }
                                    if let Some(ctor) = self.constructors.get(&name).cloned() {
                                        self.exports.constructors.insert(name, ctor);
                                    }
                                }
                            }
                            Declaration::FunctionDeclaration(func) => {
                                self.register_function_header(func, true);
                                if let Some(id) = &func.id {
                                    let name = id.name.to_string();
                                    if let Some(info) = self.lookup_var_readonly(&name) {
                                        self.exports.values.insert(name, info.ty.clone());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                        self.register_function_header(func, true);
                        let sig = self.parse_function_params_and_return(
                            &func.params,
                            func.return_type.as_ref(),
                        );
                        self.exports.default_export = Some(TsType::Function(sig));
                    }
                    ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                        self.register_class_header(class, true);
                        if let Some(id) = &class.id {
                            let name = id.name.to_string();
                            self.exports.default_export = Some(TsType::Named(name, Vec::new()));
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    fn register_interface(&mut self, iface: &TSInterfaceDeclaration<'_>, _export: bool) {
        let name = iface.id.name.to_string();
        // Temporarily insert placeholder for recursive interfaces.
        self.types
            .entry(name.clone())
            .or_insert_with(|| TsType::Object(BTreeMap::new()));
        let mut props = self.parse_signatures(&iface.body.body);
        for ext in &iface.extends {
            if let Expression::Identifier(ext_id) = &ext.expression
                && let Some(TsType::Object(base_props)) = self.types.get(ext_id.name.as_str())
            {
                for (k, v) in base_props.clone() {
                    props.entry(k).or_insert(v);
                }
            }
        }
        self.types.insert(name, TsType::Object(props));
    }

    fn register_type_alias(&mut self, alias: &TSTypeAliasDeclaration<'_>, _export: bool) {
        let name = alias.id.name.to_string();
        let ty = self.parse_ts_type(&alias.type_annotation);
        self.types.insert(name, ty);
    }

    fn register_enum(&mut self, enum_decl: &TSEnumDeclaration<'_>, _export: bool) {
        let name = enum_decl.id.name.to_string();
        let mut props = BTreeMap::new();
        for member in &enum_decl.body.members {
            let member_name = match &member.id {
                TSEnumMemberName::Identifier(ident) => ident.name.to_string(),
                TSEnumMemberName::String(lit) | TSEnumMemberName::ComputedString(lit) => {
                    lit.value.to_string()
                }
                _ => continue,
            };
            let member_ty = match &member.initializer {
                Some(Expression::StringLiteral(_)) => TsType::String,
                _ => TsType::Number,
            };
            props.insert(
                member_name,
                TsPropertySig {
                    ty: member_ty,
                    optional: false,
                    readonly: true,
                },
            );
        }
        self.types.insert(
            name.clone(),
            TsType::Union(vec![TsType::Number, TsType::String]),
        );
        self.declare_var(
            name,
            TsType::Object(props),
            true,
            enum_decl.span.start,
            true,
        );
    }

    fn register_class_header(&mut self, class: &Class<'_>, _export: bool) {
        let Some(id) = &class.id else {
            return;
        };
        let class_name = id.name.to_string();
        self.types
            .entry(class_name.clone())
            .or_insert_with(|| TsType::Object(BTreeMap::new()));

        let mut props = BTreeMap::new();
        if let Some(Expression::Identifier(super_id)) = &class.super_class {
            let parent_name = super_id.name.to_string();
            self.class_parents
                .insert(class_name.clone(), parent_name.clone());
            if let Some(TsType::Object(parent_props)) = self.types.get(&parent_name) {
                props.extend(parent_props.clone());
            }
        }
        let mut implemented = Vec::new();
        for imp in &class.implements {
            let iface_name = ts_type_name_to_string(&imp.expression);
            implemented.push(iface_name);
        }
        if !implemented.is_empty() {
            self.class_interfaces
                .insert(class_name.clone(), implemented);
        }

        let mut ctor_sig = self
            .class_parents
            .get(&class_name)
            .and_then(|parent| self.constructors.get(parent).cloned())
            .map(|mut sig| {
                sig.return_type = Box::new(TsType::Named(class_name.clone(), Vec::new()));
                sig
            })
            .unwrap_or_else(|| TsFunctionSig {
                params: Vec::new(),
                min_params: 0,
                has_rest: false,
                return_type: Box::new(TsType::Named(class_name.clone(), Vec::new())),
            });

        for elem in &class.body.body {
            match elem {
                ClassElement::PropertyDefinition(prop) => {
                    if let Some(key) = property_key_name(&prop.key) {
                        let ty = prop
                            .type_annotation
                            .as_ref()
                            .map(|ann| self.parse_ts_type(&ann.type_annotation))
                            .or_else(|| prop.value.as_ref().map(|v| self.infer_expr(v).widen()))
                            .unwrap_or(TsType::Any);
                        if !prop.r#static {
                            props.insert(
                                key,
                                TsPropertySig {
                                    ty,
                                    optional: prop.optional,
                                    readonly: prop.readonly,
                                },
                            );
                        }
                    }
                }
                ClassElement::MethodDefinition(method) => {
                    if method.kind == MethodDefinitionKind::Constructor {
                        let mut sig =
                            self.parse_function_params_and_return(&method.value.params, None);
                        sig.return_type = Box::new(TsType::Named(class_name.clone(), Vec::new()));
                        ctor_sig = sig;
                        // Also collect parameter properties (`constructor(public x: number)`)
                        for param in &method.value.params.items {
                            if (param.accessibility.is_some() || param.readonly)
                                && let BindingPatternKind::BindingIdentifier(ident) =
                                    &param.pattern.kind
                            {
                                let ty = param
                                    .pattern
                                    .type_annotation
                                    .as_ref()
                                    .map(|ann| self.parse_ts_type(&ann.type_annotation))
                                    .unwrap_or(TsType::Any);
                                props.insert(
                                    ident.name.to_string(),
                                    TsPropertySig {
                                        ty,
                                        optional: param.pattern.optional,
                                        readonly: param.readonly,
                                    },
                                );
                            }
                        }
                    } else if let Some(key) = property_key_name(&method.key) {
                        let sig = self.parse_function_params_and_return(
                            &method.value.params,
                            method.value.return_type.as_ref(),
                        );
                        if !method.r#static {
                            let ty = match method.kind {
                                MethodDefinitionKind::Get => (*sig.return_type).clone(),
                                MethodDefinitionKind::Set => {
                                    sig.params.first().cloned().unwrap_or(TsType::Any)
                                }
                                _ => TsType::Function(sig),
                            };
                            props.insert(
                                key,
                                TsPropertySig {
                                    ty,
                                    optional: method.optional,
                                    readonly: false,
                                },
                            );
                        }
                    }
                }
                _ => {}
            }
        }

        self.types.insert(class_name.clone(), TsType::Object(props));
        self.constructors.insert(class_name.clone(), ctor_sig);
        self.declare_var(
            class_name.clone(),
            TsType::Named(class_name, Vec::new()),
            true,
            class.span.start,
            true,
        );
    }

    fn register_function_header(&mut self, func: &Function<'_>, _export: bool) {
        let Some(id) = &func.id else {
            return;
        };
        let sig = self.parse_function_params_and_return(&func.params, func.return_type.as_ref());
        self.declare_var(
            id.name.to_string(),
            TsType::Function(sig),
            true,
            func.span.start,
            true,
        );
    }

    pub fn check_program(&mut self, program: &Program<'_>) {
        // If wrapped in `function __neoflash_handler__() { "use strict"; ... }`,
        // unwrap into its body so statements inside the handler are checked directly.
        if self.prefix_len > 0
            && program.body.len() == 1
            && let Statement::FunctionDeclaration(func) = &program.body[0]
            && func
                .id
                .as_ref()
                .is_some_and(|id| id.name == "__neoflash_handler__")
            && let Some(body) = &func.body
        {
            self.collect_declarations(&body.statements, false);
            for stmt in &body.statements {
                self.check_statement(stmt, false);
            }
            return;
        }

        self.collect_declarations(&program.body, true);
        for stmt in &program.body {
            self.check_statement(stmt, true);
        }
    }

    fn check_statement(&mut self, stmt: &Statement<'_>, is_module_top: bool) {
        match stmt {
            Statement::BlockStatement(block) => {
                self.push_scope();
                self.collect_declarations(&block.body, false);
                for inner in &block.body {
                    self.check_statement(inner, false);
                }
                self.pop_scope();
            }
            Statement::VariableDeclaration(decl) => {
                self.check_variable_declaration(decl, is_module_top);
            }
            Statement::FunctionDeclaration(func) => {
                self.check_function(func);
            }
            Statement::ClassDeclaration(class) => {
                self.check_class(class);
            }
            Statement::ReturnStatement(ret) => {
                let actual = ret
                    .argument
                    .as_ref()
                    .map(|arg| self.infer_expr(arg))
                    .unwrap_or(TsType::Void);
                if let Some((expected, saw_return)) = self.return_stack.last_mut() {
                    let expected = expected.clone();
                    if ret.argument.is_some() {
                        *saw_return = true;
                    }
                    if expected == TsType::Void {
                        if ret.argument.is_some()
                            && !matches!(actual, TsType::Void | TsType::Undefined | TsType::Any)
                        {
                            self.error(
                                ret.span.start,
                                format!(
                                    "Type '{}' is not assignable to return type 'void'.",
                                    actual.display_name()
                                ),
                            );
                        }
                    } else if ret.argument.is_none()
                        && !self.is_assignable(&TsType::Undefined, &expected)
                    {
                        self.error(
                            ret.span.start,
                            format!(
                                "A function whose declared return type is '{}' must return a value.",
                                expected.display_name()
                            ),
                        );
                    } else if !self.is_assignable(&actual, &expected) {
                        self.error(
                            ret.span.start,
                            format!(
                                "Type '{}' is not assignable to return type '{}'.",
                                actual.display_name(),
                                expected.display_name()
                            ),
                        );
                    }
                }
            }
            Statement::ExpressionStatement(expr_stmt) => {
                self.infer_expr(&expr_stmt.expression);
            }
            Statement::IfStatement(if_stmt) => {
                self.infer_expr(&if_stmt.test);
                self.check_statement(&if_stmt.consequent, false);
                if let Some(alt) = &if_stmt.alternate {
                    self.check_statement(alt, false);
                }
            }
            Statement::WhileStatement(w) => {
                self.infer_expr(&w.test);
                self.check_statement(&w.body, false);
            }
            Statement::DoWhileStatement(d) => {
                self.check_statement(&d.body, false);
                self.infer_expr(&d.test);
            }
            Statement::ForStatement(f) => {
                self.push_scope();
                if let Some(ForStatementInit::VariableDeclaration(decl)) = &f.init {
                    self.check_variable_declaration(decl, false);
                } else if let Some(init) = &f.init
                    && let Some(expr) = init.as_expression()
                {
                    self.infer_expr(expr);
                }
                if let Some(test) = &f.test {
                    self.infer_expr(test);
                }
                if let Some(update) = &f.update {
                    self.infer_expr(update);
                }
                self.check_statement(&f.body, false);
                self.pop_scope();
            }
            Statement::ForOfStatement(f) => {
                self.push_scope();
                let right_ty = self.infer_expr(&f.right);
                let elem_ty = match self.resolve_type(&right_ty) {
                    TsType::Array(elem) => *elem,
                    TsType::String => TsType::String,
                    _ => TsType::Any,
                };
                if let ForStatementLeft::VariableDeclaration(decl) = &f.left {
                    for d in &decl.declarations {
                        if let BindingPatternKind::BindingIdentifier(id) = &d.id.kind {
                            let declared =
                                d.id.type_annotation
                                    .as_ref()
                                    .map(|ann| self.parse_ts_type(&ann.type_annotation));
                            if let Some(ann_ty) = &declared
                                && !self.is_assignable(&elem_ty, ann_ty)
                            {
                                self.error(
                                    d.span.start,
                                    format!(
                                        "Type '{}' is not assignable to type '{}'.",
                                        elem_ty.display_name(),
                                        ann_ty.display_name()
                                    ),
                                );
                            }
                            self.declare_var(
                                id.name.to_string(),
                                declared.unwrap_or(elem_ty.clone()),
                                decl.kind == VariableDeclarationKind::Const,
                                d.span.start,
                                d.id.type_annotation.is_some(),
                            );
                        }
                    }
                }
                self.check_statement(&f.body, false);
                self.pop_scope();
            }
            Statement::SwitchStatement(s) => {
                self.infer_expr(&s.discriminant);
                self.push_scope();
                for case in &s.cases {
                    if let Some(test) = &case.test {
                        self.infer_expr(test);
                    }
                    for inner in &case.consequent {
                        self.check_statement(inner, false);
                    }
                }
                self.pop_scope();
            }
            Statement::TryStatement(t) => {
                self.push_scope();
                for inner in &t.block.body {
                    self.check_statement(inner, false);
                }
                self.pop_scope();
                if let Some(handler) = &t.handler {
                    self.push_scope();
                    if let Some(param) = &handler.param
                        && let BindingPatternKind::BindingIdentifier(id) = &param.pattern.kind
                    {
                        let ty = param
                            .pattern
                            .type_annotation
                            .as_ref()
                            .map(|ann| self.parse_ts_type(&ann.type_annotation))
                            .unwrap_or(TsType::Any);
                        self.declare_var(id.name.to_string(), ty, false, param.span.start, true);
                    }
                    for inner in &handler.body.body {
                        self.check_statement(inner, false);
                    }
                    self.pop_scope();
                }
                if let Some(finalizer) = &t.finalizer {
                    self.push_scope();
                    for inner in &finalizer.body {
                        self.check_statement(inner, false);
                    }
                    self.pop_scope();
                }
            }
            Statement::ThrowStatement(th) => {
                self.infer_expr(&th.argument);
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(decl) = &export.declaration {
                    match decl {
                        Declaration::VariableDeclaration(v) => {
                            self.check_variable_declaration(v, true);
                            for d in &v.declarations {
                                if let BindingPatternKind::BindingIdentifier(id) = &d.id.kind {
                                    let name = id.name.to_string();
                                    if let Some(info) = self.lookup_var_readonly(&name) {
                                        self.exports.values.insert(name, info.ty.clone());
                                    }
                                }
                            }
                        }
                        Declaration::FunctionDeclaration(f) => {
                            self.check_function(f);
                        }
                        Declaration::ClassDeclaration(c) => {
                            self.check_class(c);
                        }
                        _ => {}
                    }
                }
                for spec in &export.specifiers {
                    let local_name = spec.local.name().to_string();
                    let exported_name = spec.exported.name().to_string();
                    if let Some(ty) = self.lookup_var(&local_name) {
                        self.exports.values.insert(exported_name.clone(), ty);
                    }
                    if let Some(ty) = self.types.get(&local_name).cloned() {
                        self.exports.types.insert(exported_name, ty);
                    }
                }
            }
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                    self.check_function(f);
                }
                ExportDefaultDeclarationKind::ClassDeclaration(c) => {
                    self.check_class(c);
                }
                _ => {
                    if let Some(expr) = export.declaration.as_expression() {
                        let ty = self.infer_expr(expr);
                        self.exports.default_export = Some(ty);
                    }
                }
            },
            _ => {}
        }
    }

    fn check_variable_declaration(&mut self, decl: &VariableDeclaration<'_>, _is_top: bool) {
        let is_const = decl.kind == VariableDeclarationKind::Const;
        for d in &decl.declarations {
            let declared_ty =
                d.id.type_annotation
                    .as_ref()
                    .map(|ann| self.parse_ts_type(&ann.type_annotation));
            let init_ty = d.init.as_ref().map(|init| {
                if let (Some(target_ty), Expression::ObjectExpression(obj)) = (&declared_ty, init) {
                    self.check_object_literal_against_type(obj, target_ty);
                }
                if let (Some(target_ty), Expression::ArrayExpression(arr)) = (&declared_ty, init) {
                    self.check_array_literal_against_type(arr, target_ty);
                    if let TsType::Tuple(_) = self.resolve_type(target_ty) {
                        let elems: Vec<TsType> = arr
                            .elements
                            .iter()
                            .filter_map(|el| el.as_expression())
                            .map(|e| self.infer_expr(e).widen())
                            .collect();
                        return TsType::Tuple(elems);
                    }
                }
                self.infer_expr(init)
            });

            if let (Some(expected), Some(actual)) = (&declared_ty, &init_ty)
                && !self.is_assignable(actual, expected)
            {
                self.error(
                    d.init.as_ref().map_or(d.span.start, |e| e.span().start),
                    format!(
                        "Type '{}' is not assignable to type '{}'.",
                        actual.display_name(),
                        expected.display_name()
                    ),
                );
            }

            let final_ty = match (declared_ty, init_ty) {
                (Some(ann), _) => ann,
                (None, Some(inferred)) => {
                    if is_const {
                        inferred
                    } else {
                        inferred.widen()
                    }
                }
                (None, None) => TsType::Any,
            };

            if let BindingPatternKind::BindingIdentifier(id) = &d.id.kind {
                self.declare_var(
                    id.name.to_string(),
                    final_ty,
                    is_const,
                    id.span.start,
                    d.id.type_annotation.is_some(),
                );
            }
        }
    }

    fn check_object_literal_against_type(&mut self, obj: &ObjectExpression<'_>, target: &TsType) {
        let resolved = self.resolve_type(target);
        let TsType::Object(expected_props) = resolved else {
            return;
        };
        if expected_props.is_empty() {
            return;
        }
        let mut provided = BTreeSet::new();
        for prop in &obj.properties {
            if let ObjectPropertyKind::ObjectProperty(p) = prop
                && let Some(key) = property_key_name(&p.key)
            {
                provided.insert(key.clone());
                let val_ty = self.infer_expr(&p.value);
                if let Some(expected_prop) = expected_props.get(&key) {
                    if !self.is_assignable(&val_ty, &expected_prop.ty) {
                        self.error(
                            p.value.span().start,
                            format!(
                                "Type '{}' is not assignable to property '{}' of type '{}'.",
                                val_ty.display_name(),
                                key,
                                expected_prop.ty.display_name()
                            ),
                        );
                    }
                } else {
                    let candidates: Vec<&str> = expected_props.keys().map(String::as_str).collect();
                    let hint = suggest_identifier(&key, candidates)
                        .map(|(best, _)| format!(" Did you mean '{best}'?"))
                        .unwrap_or_default();
                    self.error(
                        p.span.start,
                        format!(
                            "Object literal may only specify known properties, and '{}' does not exist on type '{}'.{hint}",
                            key,
                            target.display_name()
                        ),
                    );
                }
            }
        }
        for (req_key, req_prop) in &expected_props {
            if !req_prop.optional
                && !self.is_assignable(&TsType::Undefined, &req_prop.ty)
                && !provided.contains(req_key)
            {
                self.error(
                    obj.span.start,
                    format!(
                        "Property '{}' is missing in object literal for type '{}'.",
                        req_key,
                        target.display_name()
                    ),
                );
            }
        }
    }

    fn check_array_literal_against_type(&mut self, arr: &ArrayExpression<'_>, target: &TsType) {
        let resolved = self.resolve_type(target);
        if let TsType::Array(elem_target) = resolved {
            for el in &arr.elements {
                if let Some(expr) = el.as_expression() {
                    let actual = self.infer_expr(expr);
                    if !self.is_assignable(&actual, &elem_target) {
                        self.error(
                            expr.span().start,
                            format!(
                                "Type '{}' is not assignable to array element type '{}'.",
                                actual.display_name(),
                                elem_target.display_name()
                            ),
                        );
                    }
                }
            }
        }
    }

    fn check_function(&mut self, func: &Function<'_>) {
        let Some(body) = &func.body else {
            return;
        };
        let sig = self.parse_function_params_and_return(&func.params, func.return_type.as_ref());
        self.push_scope();
        for param in &func.params.items {
            match &param.pattern.kind {
                BindingPatternKind::BindingIdentifier(id) => {
                    let ty = param
                        .pattern
                        .type_annotation
                        .as_ref()
                        .map(|ann| self.parse_ts_type(&ann.type_annotation))
                        .unwrap_or(TsType::Any);
                    self.declare_var(
                        id.name.to_string(),
                        ty,
                        false,
                        param.span.start,
                        param.pattern.type_annotation.is_some(),
                    );
                }
                BindingPatternKind::AssignmentPattern(assign) => {
                    let default_ty = self.infer_expr(&assign.right).widen();
                    if let BindingPatternKind::BindingIdentifier(id) = &assign.left.kind {
                        let ty = param
                            .pattern
                            .type_annotation
                            .as_ref()
                            .or(assign.left.type_annotation.as_ref())
                            .map(|ann| self.parse_ts_type(&ann.type_annotation))
                            .unwrap_or(default_ty);
                        self.declare_var(id.name.to_string(), ty, false, param.span.start, true);
                    }
                }
                _ => {}
            }
        }
        if let Some(rest) = &func.params.rest
            && let BindingPatternKind::BindingIdentifier(id) = &rest.argument.kind
        {
            let ty = rest
                .argument
                .type_annotation
                .as_ref()
                .map(|ann| self.parse_ts_type(&ann.type_annotation))
                .unwrap_or_else(|| TsType::Array(Box::new(TsType::Any)));
            self.declare_var(id.name.to_string(), ty, false, rest.span.start, true);
        }

        let ret_ty = (*sig.return_type).clone();
        let requires_return = func.return_type.is_some()
            && !matches!(
                ret_ty,
                TsType::Void | TsType::Any | TsType::Unknown | TsType::Undefined
            )
            && !self.is_assignable(&TsType::Undefined, &ret_ty);
        self.return_stack.push((ret_ty.clone(), false));
        self.collect_declarations(&body.statements, false);
        for stmt in &body.statements {
            self.check_statement(stmt, false);
        }
        let (_, saw_return) = self.return_stack.pop().unwrap();
        if requires_return && !saw_return && !body_always_throws(&body.statements) {
            self.error(
                func.span.start,
                format!(
                    "Function declared with return type '{}' must return a value.",
                    ret_ty.display_name()
                ),
            );
        }
        self.pop_scope();
    }

    fn check_class(&mut self, class: &Class<'_>) {
        let class_name = class
            .id
            .as_ref()
            .map(|id| id.name.to_string())
            .unwrap_or_default();
        // Verify `implements` contracts.
        if let Some(TsType::Object(class_props)) = self.types.get(&class_name).cloned() {
            for imp in &class.implements {
                let iface_name = ts_type_name_to_string(&imp.expression);
                if let Some(TsType::Object(iface_props)) = self.types.get(&iface_name).cloned() {
                    for (member_name, iface_prop) in &iface_props {
                        if iface_prop.optional {
                            continue;
                        }
                        match class_props.get(member_name) {
                            None => {
                                self.error(
                                    class.span.start,
                                    format!(
                                        "Class '{class_name}' incorrectly implements interface '{iface_name}': property '{member_name}' is missing."
                                    ),
                                );
                            }
                            Some(class_prop) => {
                                if !self.is_assignable(&class_prop.ty, &iface_prop.ty) {
                                    self.error(
                                        class.span.start,
                                        format!(
                                            "Class '{class_name}' incorrectly implements interface '{iface_name}': member '{member_name}' has type '{}', expected '{}'.",
                                            class_prop.ty.display_name(),
                                            iface_prop.ty.display_name()
                                        ),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // Check field initializers and method bodies with `this` bound to the class type.
        self.push_scope();
        if !class_name.is_empty() {
            self.declare_var(
                "this".into(),
                TsType::Named(class_name, Vec::new()),
                true,
                class.span.start,
                true,
            );
        }
        for elem in &class.body.body {
            match elem {
                ClassElement::PropertyDefinition(prop) => {
                    if let (Some(ann), Some(val)) = (&prop.type_annotation, &prop.value) {
                        let expected = self.parse_ts_type(&ann.type_annotation);
                        let actual = self.infer_expr(val);
                        if !self.is_assignable(&actual, &expected) {
                            self.error(
                                val.span().start,
                                format!(
                                    "Type '{}' is not assignable to property type '{}'.",
                                    actual.display_name(),
                                    expected.display_name()
                                ),
                            );
                        }
                    } else if let Some(val) = &prop.value {
                        self.infer_expr(val);
                    }
                }
                ClassElement::MethodDefinition(method) => {
                    self.check_function(&method.value);
                }
                _ => {}
            }
        }
        self.pop_scope();
    }

    pub fn infer_expr(&mut self, expr: &Expression<'_>) -> TsType {
        match expr {
            Expression::NumericLiteral(n) => TsType::LiteralNumber(n.value.to_string()),
            Expression::StringLiteral(s) => TsType::LiteralString(s.value.to_string()),
            Expression::BooleanLiteral(b) => TsType::LiteralBoolean(b.value),
            Expression::NullLiteral(_) => TsType::Null,
            Expression::TemplateLiteral(tpl) => {
                for expr in &tpl.expressions {
                    self.infer_expr(expr);
                }
                TsType::String
            }
            Expression::Identifier(id) => {
                if id.name == "undefined" {
                    return TsType::Undefined;
                }
                if id.name == "NaN" || id.name == "Infinity" {
                    return TsType::Number;
                }
                if let Some(ty) = self.lookup_var(id.name.as_str()) {
                    ty
                } else {
                    let candidates = self.all_visible_names();
                    let hint = suggest_identifier(id.name.as_str(), candidates)
                        .map(|(best, _)| format!(" Did you mean '{best}'?"))
                        .unwrap_or_default();
                    self.error(
                        id.span.start,
                        format!("Cannot find name '{}'.{hint}", id.name),
                    );
                    TsType::Any
                }
            }
            Expression::ThisExpression(_) => self.lookup_var("this").unwrap_or(TsType::Any),
            Expression::ParenthesizedExpression(p) => self.infer_expr(&p.expression),
            Expression::TSAsExpression(as_expr) => {
                self.infer_expr(&as_expr.expression);
                self.parse_ts_type(&as_expr.type_annotation)
            }
            Expression::TSTypeAssertion(assert_expr) => {
                self.infer_expr(&assert_expr.expression);
                self.parse_ts_type(&assert_expr.type_annotation)
            }
            Expression::TSNonNullExpression(non_null) => self.infer_expr(&non_null.expression),
            Expression::ArrayExpression(arr) => {
                let mut elem_types = Vec::new();
                for el in &arr.elements {
                    if let Some(e) = el.as_expression() {
                        elem_types.push(self.infer_expr(e).widen());
                    }
                }
                if elem_types.is_empty() {
                    TsType::Array(Box::new(TsType::Any))
                } else {
                    let first = elem_types[0].clone();
                    if elem_types.iter().all(|t| t == &first) {
                        TsType::Array(Box::new(first))
                    } else {
                        TsType::Array(Box::new(TsType::Union(elem_types)))
                    }
                }
            }
            Expression::ObjectExpression(obj) => {
                let mut props = BTreeMap::new();
                for prop in &obj.properties {
                    if let ObjectPropertyKind::ObjectProperty(p) = prop
                        && let Some(key) = property_key_name(&p.key)
                    {
                        let ty = self.infer_expr(&p.value).widen();
                        props.insert(
                            key,
                            TsPropertySig {
                                ty,
                                optional: false,
                                readonly: false,
                            },
                        );
                    }
                }
                TsType::Object(props)
            }
            Expression::UnaryExpression(u) => {
                let arg = self.infer_expr(&u.argument);
                match u.operator {
                    UnaryOperator::UnaryNegation
                    | UnaryOperator::UnaryPlus
                    | UnaryOperator::BitwiseNot => {
                        if !self.is_assignable(&arg, &TsType::Number) {
                            self.error(
                                u.span.start,
                                format!(
                                    "Unary '{}' requires a 'number' operand, got '{}'.",
                                    u.operator.as_str(),
                                    arg.display_name()
                                ),
                            );
                        }
                        TsType::Number
                    }
                    UnaryOperator::LogicalNot => TsType::Boolean,
                    UnaryOperator::Typeof => TsType::String,
                    UnaryOperator::Void => TsType::Undefined,
                    UnaryOperator::Delete => TsType::Boolean,
                }
            }
            Expression::UpdateExpression(up) => {
                let arg = match &up.argument {
                    SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                        self.lookup_var(id.name.as_str()).unwrap_or(TsType::Any)
                    }
                    _ => TsType::Any,
                };
                if !self.is_assignable(&arg, &TsType::Number) {
                    self.error(
                        up.span.start,
                        format!(
                            "An arithmetic update operand must be of type 'number', got '{}'.",
                            arg.display_name()
                        ),
                    );
                }
                TsType::Number
            }
            Expression::BinaryExpression(bin) => {
                let left = self.infer_expr(&bin.left);
                let right = self.infer_expr(&bin.right);
                match bin.operator {
                    BinaryOperator::Addition => {
                        let concat_allowed =
                            TsType::Union(vec![TsType::String, TsType::Number, TsType::Boolean]);
                        if left == TsType::Any || right == TsType::Any {
                            TsType::Any
                        } else if self.is_assignable(&left, &TsType::Number)
                            && self.is_assignable(&right, &TsType::Number)
                        {
                            TsType::Number
                        } else if self.is_assignable(&left, &TsType::String)
                            || self.is_assignable(&right, &TsType::String)
                        {
                            if self.is_assignable(&left, &concat_allowed)
                                && self.is_assignable(&right, &concat_allowed)
                            {
                                TsType::String
                            } else {
                                self.error(
                                    bin.span.start,
                                    format!(
                                        "Operator '+' cannot be applied to types '{}' and '{}'.",
                                        left.display_name(),
                                        right.display_name()
                                    ),
                                );
                                TsType::String
                            }
                        } else {
                            self.error(
                                bin.span.start,
                                format!(
                                    "Operator '+' cannot be applied to types '{}' and '{}'.",
                                    left.display_name(),
                                    right.display_name()
                                ),
                            );
                            TsType::Any
                        }
                    }
                    BinaryOperator::Subtraction
                    | BinaryOperator::Multiplication
                    | BinaryOperator::Division
                    | BinaryOperator::Remainder
                    | BinaryOperator::Exponential
                    | BinaryOperator::BitwiseOR
                    | BinaryOperator::BitwiseXOR
                    | BinaryOperator::BitwiseAnd
                    | BinaryOperator::ShiftLeft
                    | BinaryOperator::ShiftRight
                    | BinaryOperator::ShiftRightZeroFill => {
                        if !self.is_assignable(&left, &TsType::Number)
                            || !self.is_assignable(&right, &TsType::Number)
                        {
                            self.error(
                                bin.span.start,
                                format!(
                                    "Operator '{}' requires 'number' operands, got '{}' and '{}'.",
                                    bin.operator.as_str(),
                                    left.display_name(),
                                    right.display_name()
                                ),
                            );
                        }
                        TsType::Number
                    }
                    BinaryOperator::LessThan
                    | BinaryOperator::LessEqualThan
                    | BinaryOperator::GreaterThan
                    | BinaryOperator::GreaterEqualThan => {
                        let both_num = self.is_assignable(&left, &TsType::Number)
                            && self.is_assignable(&right, &TsType::Number);
                        let both_str = self.is_assignable(&left, &TsType::String)
                            && self.is_assignable(&right, &TsType::String);
                        if !both_num && !both_str && left != TsType::Any && right != TsType::Any {
                            self.error(
                                bin.span.start,
                                format!(
                                    "Operator '{}' cannot be applied to types '{}' and '{}'.",
                                    bin.operator.as_str(),
                                    left.display_name(),
                                    right.display_name()
                                ),
                            );
                        }
                        TsType::Boolean
                    }
                    BinaryOperator::Equality
                    | BinaryOperator::Inequality
                    | BinaryOperator::StrictEquality
                    | BinaryOperator::StrictInequality
                    | BinaryOperator::In
                    | BinaryOperator::Instanceof => TsType::Boolean,
                }
            }
            Expression::LogicalExpression(log) => {
                let left = self.infer_expr(&log.left);
                let right = self.infer_expr(&log.right);
                match log.operator {
                    LogicalOperator::And => right,
                    LogicalOperator::Or | LogicalOperator::Coalesce => {
                        if left == right {
                            left
                        } else {
                            TsType::Union(vec![left, right])
                        }
                    }
                }
            }
            Expression::ConditionalExpression(cond) => {
                self.infer_expr(&cond.test);
                let c = self.infer_expr(&cond.consequent).widen();
                let a = self.infer_expr(&cond.alternate).widen();
                if c == a { c } else { TsType::Union(vec![c, a]) }
            }
            Expression::AssignmentExpression(assign) => {
                let rhs = self.infer_expr(&assign.right);
                match &assign.left {
                    AssignmentTarget::AssignmentTargetIdentifier(id) => {
                        if let Some(info) = self.lookup_var_readonly(id.name.as_str()).cloned() {
                            if info.is_const {
                                self.error(
                                    assign.span.start,
                                    format!(
                                        "Cannot assign to '{}' because it is a constant.",
                                        id.name
                                    ),
                                );
                            }
                            if assign.operator == AssignmentOperator::Assign {
                                if !self.is_assignable(&rhs, &info.ty) {
                                    self.error(
                                        assign.right.span().start,
                                        format!(
                                            "Type '{}' is not assignable to type '{}'.",
                                            rhs.display_name(),
                                            info.ty.display_name()
                                        ),
                                    );
                                }
                            } else if assign.operator == AssignmentOperator::Addition {
                                let valid_num = self.is_assignable(&info.ty, &TsType::Number)
                                    && self.is_assignable(&rhs, &TsType::Number);
                                let valid_str = self.is_assignable(&info.ty, &TsType::String)
                                    && (self.is_assignable(&rhs, &TsType::String)
                                        || self.is_assignable(&rhs, &TsType::Number));
                                if !valid_num && !valid_str && info.ty != TsType::Any {
                                    self.error(
                                        assign.right.span().start,
                                        format!(
                                            "Type '{}' is not assignable to type '{}'.",
                                            rhs.display_name(),
                                            info.ty.display_name()
                                        ),
                                    );
                                }
                            } else if !self.is_assignable(&rhs, &TsType::Number)
                                || !self.is_assignable(&info.ty, &TsType::Number)
                            {
                                self.error(
                                    assign.right.span().start,
                                    format!(
                                        "Operator '{}' requires 'number' operands, got '{}' and '{}'.",
                                        assign.operator.as_str(),
                                        info.ty.display_name(),
                                        rhs.display_name()
                                    ),
                                );
                            }
                        } else {
                            let candidates = self.all_visible_names();
                            let hint = suggest_identifier(id.name.as_str(), candidates)
                                .map(|(best, _)| format!(" Did you mean '{best}'?"))
                                .unwrap_or_default();
                            self.error(
                                id.span.start,
                                format!("Cannot find name '{}'.{hint}", id.name),
                            );
                        }
                    }
                    AssignmentTarget::StaticMemberExpression(member) => {
                        let obj_ty = self.infer_expr(&member.object);
                        let prop_name = member.property.name.as_str();
                        if let TsType::Object(props) = self.resolve_type(&obj_ty)
                            && !props.is_empty()
                        {
                            if let Some(prop_sig) = props.get(prop_name) {
                                if prop_sig.readonly {
                                    self.error(
                                        member.property.span.start,
                                        format!(
                                            "Cannot assign to '{prop_name}' because it is a read-only property."
                                        ),
                                    );
                                } else if assign.operator == AssignmentOperator::Assign
                                    && !self.is_assignable(&rhs, &prop_sig.ty)
                                {
                                    self.error(
                                        assign.right.span().start,
                                        format!(
                                            "Type '{}' is not assignable to property '{}' of type '{}'.",
                                            rhs.display_name(),
                                            prop_name,
                                            prop_sig.ty.display_name()
                                        ),
                                    );
                                }
                            } else {
                                let candidates: Vec<&str> =
                                    props.keys().map(String::as_str).collect();
                                let hint = suggest_identifier(prop_name, candidates)
                                    .map(|(best, _)| format!(" Did you mean '{best}'?"))
                                    .unwrap_or_default();
                                self.error(
                                    member.property.span.start,
                                    format!(
                                        "Property '{prop_name}' does not exist on type '{}'.{hint}",
                                        obj_ty.display_name()
                                    ),
                                );
                            }
                        }
                    }
                    AssignmentTarget::ComputedMemberExpression(comp) => {
                        let obj_ty = self.infer_expr(&comp.object);
                        let idx_ty = self.infer_expr(&comp.expression);
                        if let TsType::Array(elem_ty) = self.resolve_type(&obj_ty) {
                            if !self.is_assignable(&idx_ty, &TsType::Number) {
                                self.error(
                                    comp.expression.span().start,
                                    format!(
                                        "Array index must be of type 'number', got '{}'.",
                                        idx_ty.display_name()
                                    ),
                                );
                            }
                            if assign.operator == AssignmentOperator::Assign
                                && !self.is_assignable(&rhs, &elem_ty)
                            {
                                self.error(
                                    assign.right.span().start,
                                    format!(
                                        "Type '{}' is not assignable to array element type '{}'.",
                                        rhs.display_name(),
                                        elem_ty.display_name()
                                    ),
                                );
                            }
                        }
                    }
                    _ => {}
                }
                rhs
            }
            Expression::StaticMemberExpression(member) => {
                let obj_ty = self.infer_expr(&member.object);
                let prop_name = member.property.name.as_str();
                match self.resolve_type(&obj_ty) {
                    TsType::Array(elem) => match prop_name {
                        "length" => TsType::Number,
                        "push" => TsType::Function(TsFunctionSig {
                            params: vec![*elem],
                            min_params: 0,
                            has_rest: true,
                            return_type: Box::new(TsType::Number),
                        }),
                        "pop" | "shift" => TsType::Function(TsFunctionSig {
                            params: Vec::new(),
                            min_params: 0,
                            has_rest: false,
                            return_type: Box::new(TsType::Union(vec![*elem, TsType::Undefined])),
                        }),
                        "join" => TsType::Function(TsFunctionSig {
                            params: vec![TsType::String],
                            min_params: 0,
                            has_rest: false,
                            return_type: Box::new(TsType::String),
                        }),
                        _ => TsType::Any,
                    },
                    TsType::String | TsType::LiteralString(_) => match prop_name {
                        "length" => TsType::Number,
                        "toUpperCase" | "toLowerCase" | "trim" | "slice" | "substring" => {
                            TsType::Function(TsFunctionSig {
                                params: Vec::new(),
                                min_params: 0,
                                has_rest: true,
                                return_type: Box::new(TsType::String),
                            })
                        }
                        _ => TsType::Any,
                    },
                    TsType::Object(props) if !props.is_empty() => {
                        if let Some(sig) = props.get(prop_name) {
                            sig.ty.clone()
                        } else {
                            let candidates: Vec<&str> = props.keys().map(String::as_str).collect();
                            let hint = suggest_identifier(prop_name, candidates)
                                .map(|(best, _)| format!(" Did you mean '{best}'?"))
                                .unwrap_or_default();
                            self.error(
                                member.property.span.start,
                                format!(
                                    "Property '{prop_name}' does not exist on type '{}'.{hint}",
                                    obj_ty.display_name()
                                ),
                            );
                            TsType::Any
                        }
                    }
                    _ => TsType::Any,
                }
            }
            Expression::ComputedMemberExpression(comp) => {
                let obj_ty = self.infer_expr(&comp.object);
                let idx_ty = self.infer_expr(&comp.expression);
                if let TsType::Array(elem) = self.resolve_type(&obj_ty) {
                    if !self.is_assignable(&idx_ty, &TsType::Number) {
                        self.error(
                            comp.expression.span().start,
                            format!(
                                "Array index must be of type 'number', got '{}'.",
                                idx_ty.display_name()
                            ),
                        );
                    }
                    *elem
                } else {
                    TsType::Any
                }
            }
            Expression::CallExpression(call) => {
                let callee_ty = self.infer_expr(&call.callee);
                let mut arg_types = Vec::with_capacity(call.arguments.len());
                for arg in &call.arguments {
                    if let Some(e) = arg.as_expression() {
                        arg_types.push((e.span().start, self.infer_expr(e)));
                    }
                }
                if let TsType::Function(sig) = self.resolve_type(&callee_ty) {
                    self.check_call_arguments(call.span.start, &sig, &arg_types);
                    (*sig.return_type).clone()
                } else {
                    TsType::Any
                }
            }
            Expression::NewExpression(new_expr) => {
                let mut arg_types = Vec::with_capacity(new_expr.arguments.len());
                for arg in &new_expr.arguments {
                    if let Some(e) = arg.as_expression() {
                        arg_types.push((e.span().start, self.infer_expr(e)));
                    }
                }
                if let Expression::Identifier(id) = &new_expr.callee {
                    let name = id.name.as_str();
                    if let Some(sig) = self.constructors.get(name).cloned() {
                        self.check_call_arguments(new_expr.span.start, &sig, &arg_types);
                        return (*sig.return_type).clone();
                    }
                    return match name {
                        "Audio" => TsType::Named("HTMLAudioElement".into(), Vec::new()),
                        "Image" => TsType::Named("HTMLImageElement".into(), Vec::new()),
                        "Array" => TsType::Array(Box::new(TsType::Any)),
                        _ => TsType::Named(name.to_owned(), Vec::new()),
                    };
                }
                TsType::Any
            }
            Expression::ArrowFunctionExpression(arrow) => {
                let mut sig = self
                    .parse_function_params_and_return(&arrow.params, arrow.return_type.as_ref());
                self.push_scope();
                for param in &arrow.params.items {
                    if let BindingPatternKind::BindingIdentifier(id) = &param.pattern.kind {
                        let ty = param
                            .pattern
                            .type_annotation
                            .as_ref()
                            .map(|ann| self.parse_ts_type(&ann.type_annotation))
                            .unwrap_or(TsType::Any);
                        self.declare_var(
                            id.name.to_string(),
                            ty,
                            false,
                            param.span.start,
                            param.pattern.type_annotation.is_some(),
                        );
                    }
                }
                if arrow.expression {
                    if let Some(Statement::ExpressionStatement(expr_stmt)) =
                        arrow.body.statements.first()
                    {
                        let body_ty = self.infer_expr(&expr_stmt.expression);
                        if arrow.return_type.is_some() {
                            if !self.is_assignable(&body_ty, &sig.return_type) {
                                self.error(
                                    expr_stmt.span.start,
                                    format!(
                                        "Type '{}' is not assignable to return type '{}'.",
                                        body_ty.display_name(),
                                        sig.return_type.display_name()
                                    ),
                                );
                            }
                        } else {
                            sig.return_type = Box::new(body_ty.widen());
                        }
                    }
                } else {
                    let ret_ty = (*sig.return_type).clone();
                    self.return_stack.push((ret_ty, false));
                    for stmt in &arrow.body.statements {
                        self.check_statement(stmt, false);
                    }
                    self.return_stack.pop();
                }
                self.pop_scope();
                TsType::Function(sig)
            }
            Expression::FunctionExpression(func) => {
                let sig =
                    self.parse_function_params_and_return(&func.params, func.return_type.as_ref());
                self.check_function(func);
                TsType::Function(sig)
            }
            Expression::JSXElement(jsx) => {
                self.check_jsx_element(jsx);
                TsType::Named("JSX.Element".into(), Vec::new())
            }
            Expression::JSXFragment(frag) => {
                for child in &frag.children {
                    self.check_jsx_child(child);
                }
                TsType::Named("JSX.Element".into(), Vec::new())
            }
            _ => TsType::Any,
        }
    }

    fn check_jsx_element(&mut self, jsx: &JSXElement<'_>) {
        for attr in &jsx.opening_element.attributes {
            if let JSXAttributeItem::Attribute(a) = attr
                && let Some(JSXAttributeValue::ExpressionContainer(container)) = &a.value
                && let Some(expr) = container.expression.as_expression()
            {
                self.infer_expr(expr);
            }
        }
        for child in &jsx.children {
            self.check_jsx_child(child);
        }
    }

    fn check_jsx_child(&mut self, child: &JSXChild<'_>) {
        match child {
            JSXChild::Element(el) => self.check_jsx_element(el),
            JSXChild::Fragment(frag) => {
                for c in &frag.children {
                    self.check_jsx_child(c);
                }
            }
            JSXChild::ExpressionContainer(container) => {
                if let Some(expr) = container.expression.as_expression() {
                    self.infer_expr(expr);
                }
            }
            _ => {}
        }
    }

    fn check_call_arguments(
        &mut self,
        call_span: u32,
        sig: &TsFunctionSig,
        args: &[(u32, TsType)],
    ) {
        let max_params = sig.params.len();
        if args.len() < sig.min_params || (!sig.has_rest && args.len() > max_params) {
            let expected_str = if sig.min_params == max_params && !sig.has_rest {
                format!("{}", sig.min_params)
            } else if sig.has_rest {
                format!("at least {}", sig.min_params)
            } else {
                format!("{}-{}", sig.min_params, max_params)
            };
            self.error(
                call_span,
                format!(
                    "Expected {expected_str} argument(s), but got {}.",
                    args.len()
                ),
            );
            return;
        }
        for ((arg_span, arg_ty), param_ty) in args.iter().zip(sig.params.iter()) {
            if !self.is_assignable(arg_ty, param_ty) {
                self.error(
                    *arg_span,
                    format!(
                        "Argument of type '{}' is not assignable to parameter of type '{}'.",
                        arg_ty.display_name(),
                        param_ty.display_name()
                    ),
                );
            }
        }
    }
}

fn body_always_throws(stmts: &[Statement<'_>]) -> bool {
    stmts
        .iter()
        .any(|s| matches!(s, Statement::ThrowStatement(_)))
}

fn ts_type_name_to_string(name: &TSTypeName<'_>) -> String {
    match name {
        TSTypeName::IdentifierReference(id) => id.name.to_string(),
        TSTypeName::QualifiedName(q) => {
            format!("{}.{}", ts_type_name_to_string(&q.left), q.right.name)
        }
    }
}

fn property_key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        PropertyKey::NumericLiteral(n) => Some(n.value.to_string()),
        _ => None,
    }
}

/// Output of running `check_typescript_program` on a parsed Oxc `Program`.
#[derive(Clone, Debug, Default)]
pub struct TsCheckOutput {
    pub diagnostics: Vec<Diagnostic>,
    pub exports: ModuleTypeExports,
}

/// Run static TypeScript type checking on a parsed Oxc `Program`, binding any imported module types/values.
pub fn check_typescript_program(
    program: &Program<'_>,
    source: &str,
    language: neoflash_core::ScriptLanguage,
    path_prefix: Option<&str>,
    imported_modules: &BTreeMap<String, ModuleTypeExports>,
) -> TsCheckOutput {
    let script = Script {
        language,
        source: source.to_string(),
    };
    let mut checker = TypeChecker::new(&script, 0);

    // Pre-populate exported type definitions from imported modules so transitive type references
    // in imported function/method signatures resolve structurally.
    for exp in imported_modules.values() {
        for (t_name, t_ty) in &exp.types {
            checker
                .types
                .entry(t_name.clone())
                .or_insert_with(|| t_ty.clone());
        }
    }

    // Pre-populate imported bindings from `import` statements in `program.body`
    for stmt in &program.body {
        if let Statement::ImportDeclaration(imp) = stmt {
            let spec_str = imp.source.value.as_str();
            let target_exports = imported_modules.get(spec_str);
            if let Some(specs) = &imp.specifiers {
                for spec in specs {
                    match spec {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            let imported_name = match &s.imported {
                                ModuleExportName::IdentifierName(id) => id.name.as_str(),
                                ModuleExportName::IdentifierReference(id) => id.name.as_str(),
                                ModuleExportName::StringLiteral(lit) => lit.value.as_str(),
                            };
                            let local_name = s.local.name.to_string();
                            if let Some(exp) = target_exports {
                                if let Some(ty) = exp.types.get(imported_name) {
                                    checker.import_type(local_name.clone(), ty.clone());
                                }
                                if let Some(ctor) = exp.constructors.get(imported_name) {
                                    checker.import_constructor(local_name.clone(), ctor.clone());
                                }
                                if let Some(val_ty) = exp.values.get(imported_name) {
                                    checker.import_binding(local_name, val_ty.clone());
                                } else if exp.types.contains_key(imported_name) {
                                    checker.import_binding(
                                        local_name.clone(),
                                        TsType::Named(imported_name.to_string(), Vec::new()),
                                    );
                                } else {
                                    checker.import_binding(local_name, TsType::Any);
                                }
                            } else {
                                checker.import_binding(local_name.clone(), TsType::Any);
                                checker.import_type(local_name, TsType::Any);
                            }
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            let local_name = s.local.name.to_string();
                            let default_ty = target_exports
                                .and_then(|exp| {
                                    exp.default_export
                                        .clone()
                                        .or_else(|| exp.values.get("default").cloned())
                                })
                                .unwrap_or(TsType::Any);
                            checker.import_binding(local_name, default_ty);
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            let local_name = s.local.name.to_string();
                            if let Some(exp) = target_exports {
                                let mut ns_props = BTreeMap::new();
                                for (k, v) in &exp.values {
                                    ns_props.insert(
                                        k.clone(),
                                        TsPropertySig {
                                            ty: v.clone(),
                                            optional: false,
                                            readonly: true,
                                        },
                                    );
                                }
                                checker.import_binding(local_name, TsType::Object(ns_props));
                            } else {
                                checker.import_binding(local_name, TsType::Any);
                            }
                        }
                    }
                }
            }
        }
    }

    checker.check_program(program);

    let mut diagnostics = checker.diagnostics;
    if let Some(prefix) = path_prefix {
        for diag in &mut diagnostics {
            if !diag.message.starts_with(prefix) {
                diag.message = format!("{prefix}: {}", diag.message);
                diag.path = Some(prefix.to_owned());
            }
        }
    }

    TsCheckOutput {
        diagnostics,
        exports: checker.exports,
    }
}
