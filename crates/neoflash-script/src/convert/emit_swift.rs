//! Deterministic AST-to-Swift 6 code generator (`compile_project_scripts_to_swift`).
//!
//! Compiles ActionScript 1.0 / 2.0 / 3.0 timeline scripts, stage object handlers, and class
//! source units into strict Swift 6 `@MainActor` code targeting NeoFlash's native Xcode runtime,
//! extracting the `SwiftHost` static proxy contract and enforcing ARC-safe `[weak self]` closures.

use super::emit_as2::{classify_as3_event_type, is_stage_expr};
use super::emit_as3::op_prec;
use super::emit_swift_helpers::{
    HoistedVarSpec, LinkedStageBinding, assign_swift_class_names, collect_dynamic_this_props,
    collect_hoisted_vars, compute_linked_stage_bindings, encode_swift_string_literal,
    flatten_plus_chain, format_swift_literal, inherits_nf_event_dispatcher, stmts_return_null,
};
use super::hir::{
    AccessorKind, HirType, ParsedClass, ParsedMethod, ParsedScript, ProjectStageIndex, TypeEnv,
    decode_string_literal, fold_100_to_01, is_atomic_expr, map_as2_direct_prop_to_modern,
    map_as2_key_constant, map_as2_percent_prop_to_modern, map_as3_keyboard_constant,
    parse_class_unit, parse_timeline_script, sanitize_swift_type_ident, short_type_name,
    stmts_use_key_is_down,
};
use super::js_ts_swift::{is_js_or_ts, lower_js_ts_source_units, parse_js_ts_script};
use super::swift_contract::{
    SwiftProxyMethod, SwiftProxyParam, inspect_swift_host_class, is_swift_host_class,
    map_portable_as_type_to_swift,
};
use super::{ConversionIssue, ConversionIssueKind};
use crate::actionscript::ast::{
    Case, CatchClause, Expr, Function, LegacyTrigger, PropertyKey, Stmt,
};
use crate::byte_offset_to_line_col;
use neoflash_core::{
    AuthoredPlayback, ObjectEvent, Project, ResourceUnit, Script, ScriptLanguage, Symbol,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Compiled Swift 6 representation of a single NeoFlash `Symbol` (root scene or library symbol).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwiftCompiledSymbol {
    pub symbol_id: String,
    pub swift_class_name: String,
    #[serde(default)]
    pub is_linked_source_class: bool,
    pub properties_swift: String,
    #[serde(skip)]
    pub(crate) prop_types: BTreeMap<String, HirType>,
    pub frame_methods_swift: BTreeMap<u32, String>,
    pub helper_methods_swift: String,
    pub instance_handlers_swift: Vec<(String, ObjectEvent, String)>,
}

/// Complete Swift 6 script compilation output for a `Project`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwiftCompiledProject {
    pub host_methods: Vec<SwiftProxyMethod>,
    pub symbols: Vec<SwiftCompiledSymbol>,
    pub source_classes_swift: String,
    pub issues: Vec<ConversionIssue>,
}

#[derive(Clone, Debug)]
struct SwiftCallableSig {
    params: Vec<HirType>,
    return_ty: HirType,
    return_is_optional: bool,
    can_throw: bool,
}

#[derive(Clone, Debug, Default)]
struct SwiftClassInfo {
    swift_name: String,
    ctor_params: Vec<HirType>,
    instance_fields: BTreeMap<String, HirType>,
    nullable_fields: BTreeSet<String>,
    static_fields: BTreeMap<String, HirType>,
    instance_methods: BTreeMap<String, SwiftCallableSig>,
    static_methods: BTreeMap<String, SwiftCallableSig>,
}

/// Escape Swift 6 reserved keywords with backticks when used as identifiers/members.
pub(crate) fn escape_swift_ident(raw: &str) -> String {
    if matches!(
        raw,
        "default"
            | "guard"
            | "in"
            | "repeat"
            | "where"
            | "func"
            | "let"
            | "var"
            | "class"
            | "struct"
            | "enum"
            | "protocol"
            | "extension"
            | "Self"
            | "Type"
            | "init"
            | "deinit"
            | "subscript"
            | "operator"
            | "actor"
            | "import"
            | "typealias"
            | "associatedtype"
            | "inout"
            | "internal"
            | "public"
            | "private"
            | "fileprivate"
            | "open"
            | "static"
            | "is"
            | "as"
            | "Any"
            | "fallthrough"
            | "rethrows"
            | "throws"
            | "defer"
            | "async"
            | "await"
            | "some"
            | "any"
            | "borrowing"
            | "consuming"
            | "isolated"
            | "nonisolated"
    ) {
        format!("`{raw}`")
    } else {
        raw.to_owned()
    }
}

fn swift_type_for_hir(ty: &HirType) -> String {
    match ty {
        HirType::Vector(elem) => match elem.as_ref() {
            HirType::Float => "[Double]".into(),
            HirType::Int => "[Int]".into(),
            HirType::UInt => "[UInt]".into(),
            HirType::Bool => "[Bool]".into(),
            HirType::String => "[String]".into(),
            HirType::Class(c) => format!("[{}]", swift_type_for_hir(&HirType::Class(c.clone()))),
            _ => "[NFValue]".into(),
        },
        HirType::Class(name) => match short_type_name(name) {
            "Event" | "MouseEvent" | "KeyboardEvent" | "TimerEvent" | "TouchEvent"
            | "FocusEvent" => "NFEvent".into(),
            "SoundChannel" => "NFSoundChannel".into(),
            "SoundTransform" => "NFSoundTransform".into(),
            "SoundMixer" => "NFSoundMixer".into(),
            "Error" | "TypeError" | "RangeError" | "ArgumentError" | "ReferenceError"
            | "SyntaxError" | "VerifyError" => "NFScriptError".into(),
            "DropShadowFilter" | "GlowFilter" | "BlurFilter" | "BitmapFilter" => {
                "NFFilterSpec".into()
            }
            "Namespace" => "NFNamespace".into(),
            "QName" => "NFQName".into(),
            "PixelSnapping" => "NFPixelSnapping".into(),
            "PNGEncoderOptions" => "NFPNGEncoderOptions".into(),
            "JPEGEncoderOptions" => "NFJPEGEncoderOptions".into(),
            "URLRequestMethod" => "NFURLRequestMethod".into(),
            "URLRequestHeader" => "NFURLRequestHeader".into(),
            "Vector3D" => "NFVector3D".into(),
            "DateTimeStyle" => "NFDateTimeStyle".into(),
            "ExternalInterface" => "NFExternalInterface".into(),
            _ => ty.to_swift_type(),
        },
        _ => ty.to_swift_type(),
    }
}

fn swift_property_type_and_default(ty: &HirType) -> (String, &'static str) {
    match ty {
        HirType::Int => ("Int".into(), "0"),
        HirType::UInt => ("UInt".into(), "0"),
        HirType::Float => ("Double".into(), "0.0"),
        HirType::Bool => ("Bool".into(), "false"),
        HirType::String => ("String".into(), "\"\""),
        HirType::Vector(_) => (swift_type_for_hir(ty), "[]"),
        HirType::Array => ("NFValue".into(), "[]"),
        HirType::Dictionary => ("NFValue".into(), "[:]"),
        HirType::Object | HirType::Unknown | HirType::Function => ("NFValue".into(), ".undefined"),
        other => (format!("{}?", swift_type_for_hir(other)), "nil"),
    }
}

fn swift_nonoptional_default_expr(ty: &HirType) -> &'static str {
    match ty {
        HirType::Int | HirType::UInt => "0",
        HirType::Float => "0.0",
        HirType::Bool => "false",
        HirType::String => "\"\"",
        HirType::Vector(_) | HirType::Array => "[]",
        HirType::Dictionary => "[:]",
        _ => ".undefined",
    }
}

fn stmts_can_throw_uncaught(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_can_throw_uncaught)
}

fn stmt_can_throw_uncaught(stmt: &Stmt) -> bool {
    match stmt.unlocated() {
        Stmt::Located(_, inner) | Stmt::Label(_, inner) | Stmt::IfFrameLoaded(_, inner) => {
            stmt_can_throw_uncaught(inner)
        }
        Stmt::Throw(_) => true,
        Stmt::Block(b) | Stmt::InitClip(_, b, _) => stmts_can_throw_uncaught(b),
        Stmt::If(_, yes, no) => {
            stmt_can_throw_uncaught(yes) || no.as_ref().is_some_and(|n| stmt_can_throw_uncaught(n))
        }
        Stmt::While(_, b) | Stmt::DoWhile(b, _) | Stmt::ForIn(_, _, _, b) => {
            stmt_can_throw_uncaught(b)
        }
        Stmt::For(init, _, _, b) => stmt_can_throw_uncaught(init) || stmt_can_throw_uncaught(b),
        Stmt::Switch(_, cases) => cases.iter().any(|c| stmts_can_throw_uncaught(&c.body)),
        Stmt::Try(body, catches, fin) => {
            let body_uncaught = catches.is_empty() && stmts_can_throw_uncaught(body);
            let catch_uncaught = catches.iter().any(|c| stmts_can_throw_uncaught(&c.body));
            let fin_uncaught = fin.as_ref().is_some_and(|f| stmts_can_throw_uncaught(f));
            body_uncaught || catch_uncaught || fin_uncaught
        }
        _ => false,
    }
}

#[derive(Clone, Debug, Default)]
struct LocalUsageInfo {
    var_decl_counts: BTreeMap<String, usize>,
    same_scope_redeclared: BTreeSet<String>,
    direct_mutated: BTreeSet<String>,
    mutated_this_props: BTreeSet<String>,
    member_or_method_mutated: BTreeSet<String>,
    read_names: BTreeSet<String>,
    active_scopes: Vec<BTreeSet<String>>,
}

fn root_lvalue_ident(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(n) => Some(n.as_str()),
        Expr::Member(b, _) | Expr::Index(b, _) => root_lvalue_ident(b),
        _ => None,
    }
}

fn is_swift_struct_mutating_method(method: &str) -> bool {
    matches!(
        method,
        "push"
            | "pop"
            | "shift"
            | "unshift"
            | "reverse"
            | "splice"
            | "sort"
            | "append"
            | "removeAll"
            | "offset"
            | "inflate"
            | "normalize"
            | "identity"
            | "translate"
            | "scale"
            | "rotate"
            | "concat"
            | "createGradientBox"
            | "setTo"
            | "copyFrom"
            | "invert"
            | "concatTransform"
    )
}

impl LocalUsageInfo {
    fn analyze(stmts: &[Stmt]) -> Self {
        let mut info = Self {
            active_scopes: vec![BTreeSet::new()],
            ..Self::default()
        };
        info.visit_stmts(stmts);
        info
    }

    fn push_scope(&mut self) {
        self.active_scopes.push(BTreeSet::new());
    }

    fn pop_scope(&mut self) {
        if self.active_scopes.len() > 1 {
            self.active_scopes.pop();
        }
    }

    fn record_decl(&mut self, name: &str) {
        *self.var_decl_counts.entry(name.to_owned()).or_insert(0) += 1;
        if self.active_scopes.iter().any(|s| s.contains(name)) {
            self.direct_mutated.insert(name.to_owned());
            self.same_scope_redeclared.insert(name.to_owned());
        } else if let Some(top) = self.active_scopes.last_mut() {
            top.insert(name.to_owned());
        }
    }

    fn visit_scoped_stmt(&mut self, stmt: &Stmt) {
        self.push_scope();
        match stmt.unlocated() {
            Stmt::Block(b) | Stmt::InitClip(_, b, _) => self.visit_stmts(b),
            other => self.visit_stmt(other),
        }
        self.pop_scope();
    }

    fn visit_stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.visit_stmt(s);
        }
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        match stmt.unlocated() {
            Stmt::Located(_, inner) => self.visit_stmt(inner),
            Stmt::Empty | Stmt::Break(_) | Stmt::Continue(_) | Stmt::DefaultXmlNamespace(_) => {}
            Stmt::Block(b) | Stmt::InitClip(_, b, _) => {
                self.push_scope();
                self.visit_stmts(b);
                self.pop_scope();
            }
            Stmt::Var(vars) => {
                for (name, _, val_opt) in vars {
                    if let Some(val) = val_opt {
                        self.visit_expr(val);
                    }
                    self.record_decl(name);
                }
            }
            Stmt::Function(f) | Stmt::LegacyEvent(_, f) => {
                self.push_scope();
                for (p, _) in &f.params {
                    if let Some(top) = self.active_scopes.last_mut() {
                        top.insert(p.clone());
                    }
                }
                self.visit_stmts(&f.body);
                self.pop_scope();
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.visit_expr(e),
            Stmt::Return(e_opt, _) => {
                if let Some(e) = e_opt {
                    self.visit_expr(e);
                }
            }
            Stmt::If(c, y, n) => {
                self.visit_expr(c);
                self.visit_scoped_stmt(y);
                if let Some(no_s) = n {
                    self.visit_scoped_stmt(no_s);
                }
            }
            Stmt::While(c, b) => {
                self.visit_expr(c);
                self.visit_scoped_stmt(b);
            }
            Stmt::DoWhile(b, c) => {
                self.visit_scoped_stmt(b);
                self.visit_expr(c);
            }
            Stmt::For(init, cond, upd, body) => {
                self.push_scope();
                self.visit_stmt(init);
                if let Some(c) = cond {
                    self.visit_expr(c);
                }
                if let Some(u) = upd {
                    self.visit_expr(u);
                }
                self.visit_scoped_stmt(body);
                self.pop_scope();
            }
            Stmt::ForIn(init, coll, _, body) => {
                self.visit_expr(coll);
                self.push_scope();
                match init.unlocated() {
                    Stmt::Var(vars) => {
                        for (name, _, val_opt) in vars {
                            if let Some(val) = val_opt {
                                self.visit_expr(val);
                            }
                            self.record_decl(name);
                        }
                    }
                    Stmt::Expr(Expr::Name(n)) => {
                        self.direct_mutated.insert(n.clone());
                    }
                    other => self.visit_stmt(other),
                }
                self.visit_scoped_stmt(body);
                self.pop_scope();
            }
            Stmt::Switch(disc, cases) => {
                self.visit_expr(disc);
                for c in cases {
                    if let Some(t) = &c.test {
                        self.visit_expr(t);
                    }
                    self.push_scope();
                    self.visit_stmts(&c.body);
                    self.pop_scope();
                }
            }
            Stmt::Try(b, catches, fin) => {
                self.push_scope();
                self.visit_stmts(b);
                self.pop_scope();
                for c in catches {
                    self.push_scope();
                    self.visit_stmts(&c.body);
                    self.pop_scope();
                }
                if let Some(f) = fin {
                    self.push_scope();
                    self.visit_stmts(f);
                    self.pop_scope();
                }
            }
            Stmt::Label(_, inner) | Stmt::IfFrameLoaded(_, inner) => self.visit_stmt(inner),
            Stmt::With(t, b, _) | Stmt::TellTarget(t, b, _) => {
                self.visit_expr(t);
                self.visit_scoped_stmt(b);
            }
        }
    }

    fn visit_lvalue_mutation(&mut self, target: &Expr, is_compound_or_update: bool) {
        match target {
            Expr::Name(n) => {
                self.direct_mutated.insert(n.clone());
                if is_compound_or_update {
                    self.read_names.insert(n.clone());
                }
            }
            Expr::Member(b, prop) => {
                if matches!(b.as_ref(), Expr::Name(n) if n == "this") {
                    self.mutated_this_props.insert(prop.clone());
                }
                if let Some(root) = root_lvalue_ident(b) {
                    self.member_or_method_mutated.insert(root.to_owned());
                }
                self.visit_expr(b);
            }
            Expr::Index(b, idx) => {
                if let Some(root) = root_lvalue_ident(b) {
                    self.member_or_method_mutated.insert(root.to_owned());
                }
                self.visit_expr(b);
                self.visit_expr(idx);
            }
            other => self.visit_expr(other),
        }
    }

    fn visit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Name(n) => {
                self.read_names.insert(n.clone());
            }
            Expr::Literal(_) | Expr::RegExpLiteral(_, _) | Expr::TypeValue(_, _) => {}
            Expr::Member(b, _) | Expr::Unary(_, b) | Expr::TypeTest(_, b, _, _) => {
                self.visit_expr(b);
            }
            Expr::Delete(t, _) => {
                self.visit_lvalue_mutation(t, true);
            }
            Expr::Update(_, t, _) => {
                self.visit_lvalue_mutation(t, true);
            }
            Expr::Assign(op, lhs, rhs) => {
                self.visit_lvalue_mutation(lhs, op != "=");
                self.visit_expr(rhs);
            }
            Expr::Index(b, i)
            | Expr::Binary(_, b, i)
            | Expr::InstanceOf(b, i)
            | Expr::XmlFilter(b, i, _) => {
                self.visit_expr(b);
                self.visit_expr(i);
            }
            Expr::Conditional(c, y, n) => {
                self.visit_expr(c);
                self.visit_expr(y);
                self.visit_expr(n);
            }
            Expr::Array(items) => {
                for it in items.iter().flatten() {
                    self.visit_expr(it);
                }
            }
            Expr::Object(props) => {
                for (_, v) in props {
                    self.visit_expr(v);
                }
            }
            Expr::Call(callee, args, _) => {
                if let Expr::Member(base, method) = callee.as_ref()
                    && is_swift_struct_mutating_method(method)
                    && let Some(root) = root_lvalue_ident(base)
                {
                    self.member_or_method_mutated.insert(root.to_owned());
                }
                self.visit_expr(callee);
                for a in args {
                    self.visit_expr(a);
                }
            }
            Expr::NewError(_, args, _)
            | Expr::NewBuiltin(_, args, _)
            | Expr::NewClass(_, args, _)
            | Expr::NewValue(_, args, _)
            | Expr::NewVector(_, args, _)
            | Expr::XmlLiteral(_, args) => {
                for a in args {
                    self.visit_expr(a);
                }
            }
            Expr::Function(f) => self.visit_stmts(&f.body),
            Expr::XmlSelect {
                base,
                local,
                namespace,
                ..
            } => {
                self.visit_expr(base);
                self.visit_expr(local);
                if let Some(ns) = namespace {
                    self.visit_expr(ns);
                }
            }
            Expr::XmlWithAttr {
                local, namespace, ..
            } => {
                self.visit_expr(local);
                if let Some(ns) = namespace {
                    self.visit_expr(ns);
                }
            }
        }
    }
}

fn build_swift_class_registry(
    parsed_units: &[(String, ParsedClass)],
    assigned_swift_names: &[String],
) -> BTreeMap<String, SwiftClassInfo> {
    let mut reg = BTreeMap::new();
    let mut short_counts: BTreeMap<String, usize> = BTreeMap::new();
    for (_, parsed) in parsed_units {
        *short_counts.entry(parsed.short_name.clone()).or_insert(0) += 1;
    }
    for ((_, parsed), swift_name) in parsed_units.iter().zip(assigned_swift_names) {
        let mut info = SwiftClassInfo {
            swift_name: swift_name.clone(),
            ..SwiftClassInfo::default()
        };
        for f in &parsed.fields {
            let ty =
                f.ty.as_deref()
                    .map_or(HirType::Unknown, HirType::from_annotation);
            if f.is_static {
                info.static_fields.insert(f.name.clone(), ty);
            } else {
                if is_swift_reference_hir_type(&ty)
                    && (f.value.is_none() || f.value.as_ref().is_some_and(is_null_or_undef))
                {
                    info.nullable_fields.insert(f.name.clone());
                }
                info.instance_fields.insert(f.name.clone(), ty);
            }
        }
        for m in &parsed.methods {
            let raw_name = m.function.name.as_deref().unwrap_or(&parsed.short_name);
            let is_ctor = raw_name == parsed.short_name && m.accessor.is_none() && !m.is_static;
            let param_tys: Vec<HirType> = m
                .function
                .params
                .iter()
                .map(|(_, t)| {
                    t.as_deref()
                        .map_or(HirType::Unknown, HirType::from_annotation)
                })
                .collect();
            if is_ctor {
                info.ctor_params = param_tys;
                continue;
            }
            if let Some(acc) = m.accessor {
                let prop_ty = match acc {
                    AccessorKind::Get => m
                        .function
                        .returns
                        .as_deref()
                        .map_or(HirType::Unknown, HirType::from_annotation),
                    AccessorKind::Set => param_tys.first().cloned().unwrap_or(HirType::Unknown),
                };
                if m.is_static {
                    info.static_fields
                        .entry(raw_name.to_owned())
                        .or_insert(prop_ty);
                } else {
                    info.instance_fields
                        .entry(raw_name.to_owned())
                        .or_insert(prop_ty);
                }
                continue;
            }
            let rty = m
                .function
                .returns
                .as_deref()
                .map_or(HirType::Void, HirType::from_annotation);
            let return_is_optional = !parsed.is_interface
                && !m.is_override
                && is_swift_reference_hir_type(&rty)
                && stmts_return_null(&m.function.body);
            let can_throw = !parsed.is_interface
                && !m.is_override
                && raw_name != "onResize"
                && stmts_can_throw_uncaught(&m.function.body);
            let sig = SwiftCallableSig {
                params: param_tys,
                return_ty: rty,
                return_is_optional,
                can_throw,
            };
            if m.is_static {
                info.static_methods.insert(raw_name.to_owned(), sig);
            } else {
                info.instance_methods.insert(raw_name.to_owned(), sig);
            }
        }
        if short_counts.get(&parsed.short_name).copied().unwrap_or(0) == 1 {
            reg.insert(parsed.short_name.clone(), info.clone());
        }
        reg.insert(parsed.qualified_name.clone(), info.clone());
        reg.insert(swift_name.clone(), info);
    }
    reg
}

/// Compile all ActionScript / JavaScript / TypeScript source units, timeline frame scripts, and
/// stage object scripts in `project` into Swift 6 code and extract the `SwiftHost` proxy contract.
pub fn compile_project_scripts_to_swift(project: &Project) -> SwiftCompiledProject {
    let mut stage = ProjectStageIndex::build(project);
    let includes = AuthoredPlayback::include_lookup_or_empty(project);

    let mut host_methods = Vec::new();
    let mut host_method_map: BTreeMap<String, SwiftProxyMethod> = BTreeMap::new();
    let mut issues = Vec::new();
    let mut class_blocks = Vec::new();

    // 1. Lower any JavaScript / TypeScript ES modules in `project.sources` first.
    let (js_ts_lowered, js_ts_issues) = lower_js_ts_source_units(project);
    issues.extend(js_ts_issues);

    // 1a. Parse ActionScript SourceUnits and extract `SwiftHost` contract first so calls across all
    //     scripts know the exact Swift parameter types of `SwiftHost` methods!
    let mut parsed_units: Vec<(String, ParsedClass)> = Vec::new();
    for unit in &project.sources {
        if is_js_or_ts(unit.language) {
            continue;
        }
        if !matches!(
            unit.language,
            ScriptLanguage::ActionScript1
                | ScriptLanguage::ActionScript2
                | ScriptLanguage::ActionScript3
        ) {
            continue;
        }
        match parse_class_unit(unit, &includes) {
            Ok(parsed) => {
                if is_swift_host_class(Some(&unit.path), &parsed) {
                    let (extracted, contract_warnings) =
                        inspect_swift_host_class(&parsed, unit.language, Some(&unit.path));
                    for w in contract_warnings {
                        issues.push(ConversionIssue {
                            kind: ConversionIssueKind::ManualAttention,
                            location: unit.path.clone(),
                            line: w.line,
                            column: w.column,
                            message: w.message,
                        });
                    }
                    for m in extracted {
                        host_method_map.insert(m.name.clone(), m.clone());
                        host_methods.push(m);
                    }
                } else {
                    parsed_units.push((unit.path.clone(), parsed));
                }
            }
            Err(err) => {
                if unit.source.contains("class ") || unit.source.contains("interface ") {
                    issues.push(ConversionIssue {
                        kind: ConversionIssueKind::ManualAttention,
                        location: unit.path.clone(),
                        line: 1,
                        column: 1,
                        message: format!("Failed to parse class unit '{}': {err}", unit.path),
                    });
                }
            }
        }
    }

    // Register lowered JS/TS classes into `parsed_units` & `stage` metadata so type inference sees them!
    for (path, parsed) in js_ts_lowered.classes {
        stage
            .project_classes
            .insert(parsed.short_name.clone(), parsed.qualified_name.clone());
        if let Some(base) = &parsed.base_class {
            stage
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
                fields.insert(
                    f.name.clone(),
                    f.ty.as_deref()
                        .map_or(HirType::Unknown, HirType::from_annotation),
                );
            }
        }
        stage
            .class_instance_methods
            .insert(parsed.qualified_name.clone(), methods);
        stage
            .class_fields
            .insert(parsed.qualified_name.clone(), fields);
        parsed_units.push((path, parsed));
    }

    let js_ts_declared_classes: BTreeSet<String> = parsed_units
        .iter()
        .map(|(_, p)| p.short_name.clone())
        .collect();

    // 1b. Discover implicit / ad-hoc Flash proxy calls across classes, timeline scripts & object handlers.
    let discovered_proxies = discover_implicit_host_proxies(
        project,
        &parsed_units,
        &stage,
        &includes,
        &host_method_map,
        &js_ts_lowered.type_aliases,
        &js_ts_declared_classes,
    );
    for m in discovered_proxies {
        host_method_map.insert(m.name.clone(), m.clone());
        host_methods.push(m);
    }

    let assigned_swift_names = assign_swift_class_names(&parsed_units);
    let class_registry = build_swift_class_registry(&parsed_units, &assigned_swift_names);

    // 1c. Collect module-level JS/TS constants & helper functions so classes and timeline scripts can reference them.
    let mut global_vars: BTreeMap<String, HirType> = BTreeMap::new();
    let mut global_funcs: BTreeMap<String, SwiftCallableSig> = BTreeMap::new();
    for (_, f) in &js_ts_lowered.module_fields {
        let ty =
            f.ty.as_deref()
                .map_or(HirType::Unknown, HirType::from_annotation);
        global_vars.insert(f.name.clone(), ty);
    }
    for (_, func) in &js_ts_lowered.module_functions {
        if let Some(fname) = &func.name {
            let param_tys: Vec<HirType> = func
                .params
                .iter()
                .map(|(_, t)| {
                    t.as_deref()
                        .map_or(HirType::Unknown, HirType::from_annotation)
                })
                .collect();
            let rty = func
                .returns
                .as_deref()
                .map_or(HirType::Void, HirType::from_annotation);
            let return_is_optional =
                is_swift_reference_hir_type(&rty) && stmts_return_null(&func.body);
            global_funcs.insert(
                fname.clone(),
                SwiftCallableSig {
                    params: param_tys,
                    return_ty: rty,
                    return_is_optional,
                    can_throw: stmts_can_throw_uncaught(&func.body),
                },
            );
        }
    }

    // 2. Match `project.document_class` -> `project.root` and `sym.class_name` -> `sym.id`!
    let find_matching_unit = |raw_cls: &str| -> Option<usize> {
        let trimmed = raw_cls.trim();
        if trimmed.is_empty() {
            return None;
        }
        parsed_units
            .iter()
            .position(|(_, p)| !p.is_interface && p.qualified_name == trimmed)
            .or_else(|| {
                let short = short_type_name(trimmed);
                parsed_units
                    .iter()
                    .position(|(_, p)| !p.is_interface && p.short_name == short)
            })
    };

    let mut linked_unit_for_symbol: BTreeMap<String, usize> = BTreeMap::new();
    let mut bound_symbol_for_unit: BTreeMap<usize, &Symbol> = BTreeMap::new();
    for sym in &project.symbols {
        let target_cls = if sym.id == project.root {
            project
                .document_class
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| sym.class_name.as_deref().filter(|s| !s.trim().is_empty()))
        } else {
            sym.class_name.as_deref().filter(|s| !s.trim().is_empty())
        };
        if let Some(cls_ref) = target_cls
            && let Some(u_idx) = find_matching_unit(cls_ref)
            && !bound_symbol_for_unit.contains_key(&u_idx)
        {
            linked_unit_for_symbol.insert(sym.id.clone(), u_idx);
            bound_symbol_for_unit.insert(u_idx, sym);
        }
    }

    // Compute per-symbol Swift class names matching `scene_codegen.rs` so Stage child bindings match 1:1.
    let mut used_swift_names: BTreeSet<String> = assigned_swift_names.iter().cloned().collect();
    let mut symbol_swift_names: BTreeMap<String, String> = BTreeMap::new();
    for sym in &project.symbols {
        if let Some(&u_idx) = linked_unit_for_symbol.get(&sym.id) {
            symbol_swift_names.insert(sym.id.clone(), assigned_swift_names[u_idx].clone());
            continue;
        }
        let base_cls_name = if sym.id == project.root {
            "MainTimeline".to_owned()
        } else if let Some(cls) = sym.class_name.as_deref().filter(|s| !s.trim().is_empty()) {
            sanitize_swift_type_ident(short_type_name(cls))
        } else {
            let stem = sanitize_swift_type_ident(&sym.name);
            if stem.starts_with("Sym") {
                stem
            } else {
                format!("Sym{stem}")
            }
        };
        let mut swift_class_name = base_cls_name.clone();
        let mut suffix = 2usize;
        while !used_swift_names.insert(swift_class_name.clone()) {
            swift_class_name = format!("{base_cls_name}_{suffix}");
            suffix += 1;
        }
        symbol_swift_names.insert(sym.id.clone(), swift_class_name);
    }
    let symbol_id_by_swift_class: BTreeMap<String, String> = symbol_swift_names
        .iter()
        .map(|(sym_id, cls_name)| (cls_name.clone(), sym_id.clone()))
        .collect();
    for sym in &project.symbols {
        let bindings = compute_linked_stage_bindings(project, sym, &symbol_swift_names);
        if let Some(children_map) = stage.symbol_children.get_mut(&sym.id) {
            for sb in bindings {
                if let Some(entry) = children_map.get_mut(&sb.raw_name) {
                    entry.hir_type = sb.hir_type;
                }
            }
        }
    }

    // 3. Compile each Symbol's timeline properties, frame scripts, helper functions, and object handlers FIRST
    //    so any timeline properties/helpers on a linked symbol can be injected directly into its `public class`!
    let mut compiled_symbols_by_id: BTreeMap<String, SwiftCompiledSymbol> = BTreeMap::new();
    for sym in &project.symbols {
        let swift_class_name = symbol_swift_names[&sym.id].clone();
        let linked_cls_info = linked_unit_for_symbol
            .get(&sym.id)
            .and_then(|&u_idx| class_registry.get(&assigned_swift_names[u_idx]));
        let (compiled_sym, sym_issues) = compile_symbol_to_swift(
            project,
            sym,
            swift_class_name,
            linked_cls_info,
            &stage,
            &symbol_id_by_swift_class,
            &includes,
            &host_method_map,
            &class_registry,
            &global_vars,
            &global_funcs,
            &js_ts_lowered.type_aliases,
            &js_ts_declared_classes,
        );
        issues.extend(sym_issues);
        compiled_symbols_by_id.insert(sym.id.clone(), compiled_sym);
    }

    // 4. Emit JS/TS module-level constants & helper functions at file scope
    if !js_ts_lowered.module_fields.is_empty() || !js_ts_lowered.module_functions.is_empty() {
        let mut mod_lines = Vec::new();
        for (path, f) in &js_ts_lowered.module_fields {
            let mut emitter = SwiftEmitter::new(
                path,
                "",
                true,
                &stage,
                &symbol_id_by_swift_class,
                &host_method_map,
                &class_registry,
                &global_vars,
                &global_funcs,
                None,
                None,
            );
            let fname = escape_swift_ident(&f.name);
            let ty =
                f.ty.as_deref()
                    .map_or(HirType::Unknown, HirType::from_annotation);
            let kw = if f.is_const { "let" } else { "var" };
            if let Some(val) = &f.value {
                let actual_ty = if ty == HirType::Unknown {
                    emitter.infer(val)
                } else {
                    ty
                };
                let sw_ty = swift_type_for_hir(&actual_ty);
                let vc = emitter.emit_coerced_expr(val, &actual_ty, f.offset);
                mod_lines.push(format!("@MainActor public {kw} {fname}: {sw_ty} = {vc}"));
            } else {
                let (sw_ty, def_val) = swift_property_type_and_default(&ty);
                mod_lines.push(format!(
                    "@MainActor public var {fname}: {sw_ty} = {def_val}"
                ));
            }
            issues.extend(emitter.issues);
        }
        for (path, func) in &js_ts_lowered.module_functions {
            if let Some(fname) = &func.name {
                let mut emitter = SwiftEmitter::new(
                    path,
                    "",
                    true,
                    &stage,
                    &symbol_id_by_swift_class,
                    &host_method_map,
                    &class_registry,
                    &global_vars,
                    &global_funcs,
                    None,
                    None,
                );
                let code = emitter.emit_helper_method(fname, func);
                issues.extend(emitter.issues);
                mod_lines.push(code);
            }
        }
        if !mod_lines.is_empty() {
            class_blocks.push(mod_lines.join("\n\n"));
        }
    }

    // 5. Emit non-`SwiftHost` project classes into `source_classes_swift`
    for (u_idx, (path, parsed)) in parsed_units.iter().enumerate() {
        let cls_swift_name = &assigned_swift_names[u_idx];
        type BoundClassCtx<'s> = (
            &'s Symbol,
            Vec<LinkedStageBinding>,
            String,
            String,
            BTreeMap<String, HirType>,
        );
        let mut bound_ctx: Option<BoundClassCtx<'_>> = None;
        if let Some(&bound_sym) = bound_symbol_for_unit.get(&u_idx) {
            let stage_bindings =
                compute_linked_stage_bindings(project, bound_sym, &symbol_swift_names);
            let (extra_props, extra_helpers, extra_prop_types) =
                if let Some(cs) = compiled_symbols_by_id.get_mut(&bound_sym.id) {
                    cs.is_linked_source_class = true;
                    (
                        std::mem::take(&mut cs.properties_swift),
                        std::mem::take(&mut cs.helper_methods_swift),
                        cs.prop_types.clone(),
                    )
                } else {
                    (String::new(), String::new(), BTreeMap::new())
                };
            bound_ctx = Some((
                bound_sym,
                stage_bindings,
                extra_props,
                extra_helpers,
                extra_prop_types,
            ));
        }

        let (swift_cls, cls_issues) = emit_swift_class(
            parsed,
            cls_swift_name,
            path,
            bound_ctx,
            &stage,
            &symbol_id_by_swift_class,
            &host_method_map,
            &class_registry,
            &global_vars,
            &global_funcs,
        );
        issues.extend(cls_issues);
        if !swift_cls.trim().is_empty() {
            class_blocks.push(swift_cls);
        }
    }

    let compiled_symbols: Vec<SwiftCompiledSymbol> = project
        .symbols
        .iter()
        .filter_map(|sym| compiled_symbols_by_id.remove(&sym.id))
        .collect();

    SwiftCompiledProject {
        host_methods,
        symbols: compiled_symbols,
        source_classes_swift: class_blocks.join("\n\n"),
        issues,
    }
}

trait IncludeLookupExt {
    fn include_lookup_or_empty(project: &Project) -> BTreeMap<String, ResourceUnit>;
}

impl IncludeLookupExt for AuthoredPlayback {
    fn include_lookup_or_empty(project: &Project) -> BTreeMap<String, ResourceUnit> {
        let mut map = BTreeMap::new();
        for res in &project.resources {
            let norm = crate::modules::normalize_module_path(&res.path);
            if !norm.is_empty() {
                map.insert(norm, res.clone());
            }
        }
        for unit in &project.sources {
            let norm = crate::modules::normalize_module_path(&unit.path);
            if !norm.is_empty()
                && !map.contains_key(&norm)
                && let Ok(res) = ResourceUnit::from_bytes(
                    unit.id.clone(),
                    unit.path.clone(),
                    "text/plain",
                    unit.source.as_bytes(),
                )
            {
                map.insert(norm, res);
            }
        }
        map
    }
}

#[allow(clippy::too_many_arguments)]
fn compile_symbol_to_swift(
    _project: &Project,
    sym: &Symbol,
    swift_class_name: String,
    linked_cls_info: Option<&SwiftClassInfo>,
    stage: &ProjectStageIndex,
    symbol_id_by_swift_class: &BTreeMap<String, String>,
    includes: &BTreeMap<String, ResourceUnit>,
    host_methods: &BTreeMap<String, SwiftProxyMethod>,
    class_registry: &BTreeMap<String, SwiftClassInfo>,
    global_vars: &BTreeMap<String, HirType>,
    global_funcs: &BTreeMap<String, SwiftCallableSig>,
    js_ts_type_aliases: &BTreeMap<String, String>,
    js_ts_declared_classes: &BTreeSet<String>,
) -> (SwiftCompiledSymbol, Vec<ConversionIssue>) {
    let mut issues = Vec::new();
    let mut prop_lines = Vec::new();
    let mut prop_types: BTreeMap<String, HirType> = BTreeMap::new();

    // 1. Stage child properties + linked class fields + TextField `variable` bindings
    let mut stage_child_props: BTreeSet<String> = BTreeSet::new();
    if let Some(children) = stage.symbol_children.get(&sym.id) {
        for (child_name, info) in children {
            prop_types.insert(child_name.clone(), info.hir_type.clone());
            stage_child_props.insert(child_name.clone());
        }
    }
    if let Some(cinfo) = linked_cls_info {
        for (fname, fty) in &cinfo.instance_fields {
            prop_types
                .entry(fname.clone())
                .or_insert_with(|| fty.clone());
        }
    }
    if let Some(tvars) = stage.pub_symbol_text_vars.get(&sym.id) {
        for tv in tvars {
            if !prop_types.contains_key(tv) {
                let escaped = escape_swift_ident(tv);
                prop_lines.push(format!("public var {escaped}: String = \"\""));
                prop_types.insert(tv.clone(), HirType::String);
            }
        }
    }

    // 2. Parse all timeline frame scripts first to lift ALL `var`s (including nested block `var`s
    //    and AS1/AS2 implicit globals) & top-level `function`s onto symbol scope!
    let mut parsed_frames: Vec<(u32, String, ParsedScript)> = Vec::new();
    let mut lifted_functions: BTreeMap<String, (Function, ScriptLanguage, String, String)> =
        BTreeMap::new();

    for entry in &sym.timeline.scripts {
        if entry.script.source.trim().is_empty() {
            continue;
        }
        let loc = format!("{}[frame {}]", sym.name, entry.frame + 1);
        let parsed_res: Result<ParsedScript, String> = if is_js_or_ts(entry.script.language) {
            parse_js_ts_script(
                &entry.script,
                &loc,
                js_ts_type_aliases,
                js_ts_declared_classes,
            )
            .map(|(p, sub_issues)| {
                issues.extend(sub_issues);
                p
            })
        } else if matches!(
            entry.script.language,
            ScriptLanguage::ActionScript1
                | ScriptLanguage::ActionScript2
                | ScriptLanguage::ActionScript3
        ) {
            parse_timeline_script(&entry.script, "", includes)
        } else {
            continue;
        };

        match parsed_res {
            Ok(parsed) => {
                let mut prelim_env = TypeEnv::new(Some(sym.id.clone()), None);
                for (k, v) in &prop_types {
                    prelim_env.define(k.clone(), v.clone());
                }
                for (k, v) in global_vars {
                    prelim_env.define(k.clone(), v.clone());
                }
                for stmt in &parsed.statements {
                    if let Stmt::Function(f) = stmt.unlocated()
                        && let Some(fname) = &f.name
                    {
                        lifted_functions.entry(fname.clone()).or_insert_with(|| {
                            (
                                (**f).clone(),
                                parsed.language,
                                loc.clone(),
                                parsed.expanded_source.clone(),
                            )
                        });
                    }
                }
                let mut known_names: BTreeSet<String> = prop_types.keys().cloned().collect();
                known_names.extend(global_vars.keys().cloned());
                known_names.extend(global_funcs.keys().cloned());
                known_names.extend(lifted_functions.keys().cloned());
                if let Some(cinfo) = linked_cls_info {
                    known_names.extend(cinfo.instance_methods.keys().cloned());
                }

                let is_as12 = matches!(
                    entry.script.language,
                    ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                );
                let hoisted = collect_hoisted_vars(&parsed.statements, is_as12, &known_names);
                for HoistedVarSpec {
                    name: vname,
                    ty: vty,
                    first_init: val_opt,
                    ..
                } in hoisted
                {
                    if prop_types.contains_key(&vname) || lifted_functions.contains_key(&vname) {
                        continue;
                    }
                    let inferred =
                        vty.as_deref()
                            .map(HirType::from_annotation)
                            .unwrap_or_else(|| {
                                val_opt.as_ref().map_or(HirType::Unknown, |e| {
                                    infer_prelim_expr(e, &prelim_env, stage, class_registry)
                                })
                            });
                    let escaped = escape_swift_ident(&vname);
                    let (swift_ty, def_val) = swift_property_type_and_default(&inferred);
                    prop_lines.push(format!("public var {escaped}: {swift_ty} = {def_val}"));
                    prelim_env.define(vname.clone(), inferred.clone());
                    prop_types.insert(vname, inferred);
                }

                let mut known_for_this: BTreeSet<String> = prop_types.keys().cloned().collect();
                known_for_this.extend(lifted_functions.keys().cloned());
                if let Some(cinfo) = linked_cls_info {
                    known_for_this.extend(cinfo.instance_methods.keys().cloned());
                }
                for dyn_prop in collect_dynamic_this_props(&parsed.statements, &known_for_this) {
                    if prop_types.contains_key(&dyn_prop)
                        || lifted_functions.contains_key(&dyn_prop)
                    {
                        continue;
                    }
                    let escaped = escape_swift_ident(&dyn_prop);
                    prop_lines.push(format!("public var {escaped}: NFValue = .undefinedVal"));
                    prelim_env.define(dyn_prop.clone(), HirType::Unknown);
                    prop_types.insert(dyn_prop, HirType::Unknown);
                }

                parsed_frames.push((entry.frame, loc, parsed));
            }
            Err(err) => {
                issues.push(ConversionIssue {
                    kind: ConversionIssueKind::ManualAttention,
                    location: loc,
                    line: 1,
                    column: 1,
                    message: format!("Failed to parse frame script: {err}"),
                });
            }
        }
    }

    let mut helper_names: BTreeSet<String> = lifted_functions.keys().cloned().collect();
    let mut helper_sigs: BTreeMap<String, SwiftCallableSig> = BTreeMap::new();
    if let Some(cinfo) = linked_cls_info {
        for (mname, msig) in &cinfo.instance_methods {
            helper_names.insert(mname.clone());
            helper_sigs.insert(mname.clone(), msig.clone());
        }
    }
    for (fname, (f, _, _, _)) in &lifted_functions {
        let param_tys: Vec<HirType> = f
            .params
            .iter()
            .map(|(_, t)| {
                t.as_deref()
                    .map_or(HirType::Unknown, HirType::from_annotation)
            })
            .collect();
        let rty = f
            .returns
            .as_deref()
            .map_or(HirType::Void, HirType::from_annotation);
        let return_is_optional = is_swift_reference_hir_type(&rty) && stmts_return_null(&f.body);
        let can_throw = fname != "onResize" && stmts_can_throw_uncaught(&f.body);
        helper_sigs.insert(
            fname.clone(),
            SwiftCallableSig {
                params: param_tys,
                return_ty: rty,
                return_is_optional,
                can_throw,
            },
        );
    }

    // 3. Emit lifted helper functions (`@MainActor func relayout(...)`)
    let mut helper_blocks = Vec::new();
    for (fname, (f, lang, loc, raw_src)) in &lifted_functions {
        let mut emitter = SwiftEmitter::new(
            loc,
            raw_src,
            *lang == ScriptLanguage::ActionScript3,
            stage,
            symbol_id_by_swift_class,
            host_methods,
            class_registry,
            global_vars,
            global_funcs,
            Some(sym.id.clone()),
            None,
        );
        emitter.set_instance_props(prop_types.clone());
        emitter.stage_child_props = stage_child_props.clone();
        emitter.instance_methods = helper_names.clone();
        emitter.instance_method_sigs = helper_sigs.clone();
        for (hn, sig) in &helper_sigs {
            emitter
                .env
                .functions
                .insert(hn.clone(), sig.return_ty.clone());
        }
        let code = emitter.emit_helper_method(fname, f);
        issues.extend(emitter.issues);
        helper_blocks.push(code);
    }

    // 4. Emit frame methods (`frame_0`, `frame_1`, ...)
    let mut frame_methods_swift = BTreeMap::new();
    let mut key_shim_emitted_on_symbol = false;

    for (frame_idx, loc, parsed) in &parsed_frames {
        let mut emitter = SwiftEmitter::new(
            loc,
            &parsed.expanded_source,
            parsed.language == ScriptLanguage::ActionScript3,
            stage,
            symbol_id_by_swift_class,
            host_methods,
            class_registry,
            global_vars,
            global_funcs,
            Some(sym.id.clone()),
            None,
        );
        emitter.set_instance_props(prop_types.clone());
        emitter.stage_child_props = stage_child_props.clone();
        emitter.instance_methods = helper_names.clone();
        emitter.instance_method_sigs = helper_sigs.clone();
        emitter.top_level_is_symbol_frame = true;
        emitter.in_symbol_frame_method = true;
        for (hn, sig) in &helper_sigs {
            emitter
                .env
                .functions
                .insert(hn.clone(), sig.return_ty.clone());
        }
        emitter.precollect(&parsed.statements);

        let needs_key_shim = parsed.language != ScriptLanguage::ActionScript3
            && stmts_use_key_is_down(&parsed.statements);
        emitter.use_key_down_shim = needs_key_shim;
        if needs_key_shim && !key_shim_emitted_on_symbol {
            key_shim_emitted_on_symbol = true;
            prop_lines.push("fileprivate var _keysDown: Set<UInt> = []".into());
            helper_blocks.push(
                "@MainActor fileprivate func _isKeyDown(_ code: Int) -> Bool {\n    return self._keysDown.contains(UInt(max(0, code)))\n}".into(),
            );
        }

        emitter.indent = 1;
        let mut body_parts = Vec::new();
        if needs_key_shim {
            let pad = emitter.pad();
            body_parts.push(format!(
                "{pad}self.stage?.addEventListener(\"keyDown\") {{ [weak self] (event: NFEvent) in\n\
                 {pad}    guard let self else {{ return }}\n\
                 {pad}    self._keysDown.insert(event.keyCode)\n\
                 {pad}}}\n\
                 {pad}self.stage?.addEventListener(\"keyUp\") {{ [weak self] (event: NFEvent) in\n\
                 {pad}    guard let self else {{ return }}\n\
                 {pad}    self._keysDown.remove(event.keyCode)\n\
                 {pad}}}"
            ));
        }

        let rendered = emitter.emit_function_body(&parsed.statements);
        if !rendered.trim().is_empty() {
            body_parts.push(rendered);
        }
        issues.extend(emitter.issues);

        let method_code = if body_parts.is_empty() {
            format!("@MainActor public func frame_{frame_idx}() {{}}")
        } else {
            format!(
                "@MainActor public func frame_{frame_idx}() {{\n{}\n}}",
                body_parts.join("\n")
            )
        };
        frame_methods_swift.insert(*frame_idx, method_code);
    }

    // 5. Emit stage instance `ObjectScript` handlers
    let mut instance_handlers_swift = Vec::new();
    for layer in &sym.timeline.layers {
        for kf in &layer.keyframes {
            for el in &kf.elements {
                if el.scripts.is_empty() {
                    continue;
                }
                let target_key = if el.name.is_empty() {
                    el.id.clone()
                } else {
                    el.name.clone()
                };
                for os in &el.scripts {
                    if os.script.source.trim().is_empty() {
                        continue;
                    }
                    let loc = format!("{}::{}::{:?}", sym.name, target_key, os.event);
                    let parsed_opt = if is_js_or_ts(os.script.language) {
                        parse_js_ts_script(
                            &os.script,
                            &loc,
                            js_ts_type_aliases,
                            js_ts_declared_classes,
                        )
                        .ok()
                        .map(|(p, sub_issues)| {
                            issues.extend(sub_issues);
                            p
                        })
                    } else {
                        parse_object_script_for_swift(&os.script, includes).ok()
                    };

                    if let Some(parsed) = parsed_opt {
                        let mut handler_blocks: Vec<(
                            ObjectEvent,
                            Option<&'static str>,
                            Vec<Stmt>,
                        )> = Vec::new();
                        let mut direct_stmts: Vec<Stmt> = Vec::new();

                        for stmt in &parsed.statements {
                            if let Stmt::LegacyEvent(triggers, func) = stmt.unlocated() {
                                let mapped = map_legacy_triggers_for_swift(triggers, os.event);
                                for (mapped_ev, tag) in mapped {
                                    handler_blocks.push((mapped_ev, tag, func.body.clone()));
                                }
                            } else {
                                direct_stmts.push(stmt.clone());
                            }
                        }
                        if !direct_stmts.is_empty() {
                            handler_blocks.push((os.event, None, direct_stmts));
                        }

                        for (ev, trigger_tag, body_stmts) in handler_blocks {
                            let mut emitter = SwiftEmitter::new(
                                &loc,
                                &parsed.expanded_source,
                                parsed.language == ScriptLanguage::ActionScript3,
                                stage,
                                symbol_id_by_swift_class,
                                host_methods,
                                class_registry,
                                global_vars,
                                global_funcs,
                                Some(sym.id.clone()),
                                None,
                            );
                            emitter.set_instance_props(prop_types.clone());
                            emitter.stage_child_props = stage_child_props.clone();
                            emitter.instance_methods = helper_names.clone();
                            emitter.instance_method_sigs = helper_sigs.clone();
                            for (hn, sig) in &helper_sigs {
                                emitter
                                    .env
                                    .functions
                                    .insert(hn.clone(), sig.return_ty.clone());
                            }
                            emitter.indent = 0;
                            let body = emitter.emit_function_body(&body_stmts);
                            issues.extend(emitter.issues);
                            if !body.trim().is_empty() || el.scripts.len() == 1 {
                                let payload = if let Some(tag) = trigger_tag {
                                    format!("// @nf-trigger: {tag}\n{body}")
                                } else {
                                    body
                                };
                                instance_handlers_swift.push((target_key.clone(), ev, payload));
                            }
                        }
                    }
                }
            }
        }
    }

    (
        SwiftCompiledSymbol {
            symbol_id: sym.id.clone(),
            swift_class_name,
            is_linked_source_class: linked_cls_info.is_some(),
            properties_swift: prop_lines.join("\n"),
            prop_types,
            frame_methods_swift,
            helper_methods_swift: helper_blocks.join("\n\n"),
            instance_handlers_swift,
        },
        issues,
    )
}

fn infer_prelim_expr(
    expr: &Expr,
    env: &TypeEnv,
    stage: &ProjectStageIndex,
    class_registry: &BTreeMap<String, SwiftClassInfo>,
) -> HirType {
    match expr {
        Expr::NewVector(elem, _, _) => HirType::Vector(Box::new(HirType::from_annotation(elem))),
        Expr::NewClass(ty, _, _) | Expr::NewBuiltin(ty, _, _) | Expr::NewValue(ty, _, _) => {
            HirType::from_annotation(ty)
        }
        Expr::NewError(ty, _, _) => HirType::Class(ty.clone()),
        Expr::Call(callee, _, _) => {
            if let Expr::Member(base, method) = callee.as_ref() {
                if let Expr::Name(cls) = base.as_ref() {
                    if cls == "Math" {
                        return HirType::Float;
                    }
                    if let Some(cinfo) = class_registry.get(cls)
                        && let Some(sig) = cinfo.static_methods.get(method)
                    {
                        return sig.return_ty.clone();
                    }
                }
                let bty = infer_prelim_expr(base, env, stage, class_registry);
                if let HirType::Class(cn) = bty
                    && let Some(cinfo) = class_registry
                        .get(cn.as_str())
                        .or_else(|| class_registry.get(short_type_name(&cn)))
                    && let Some(sig) = cinfo.instance_methods.get(method)
                {
                    return sig.return_ty.clone();
                }
            }
            env.infer_expr(expr, stage)
        }
        _ => env.infer_expr(expr, stage),
    }
}

struct SwiftEmitter<'a> {
    location: &'a str,
    raw_source: &'a str,
    source_is_as3: bool,
    stage: &'a ProjectStageIndex,
    symbol_id_by_swift_class: &'a BTreeMap<String, String>,
    host_methods: &'a BTreeMap<String, SwiftProxyMethod>,
    class_registry: &'a BTreeMap<String, SwiftClassInfo>,
    global_vars: &'a BTreeMap<String, HirType>,
    global_funcs: &'a BTreeMap<String, SwiftCallableSig>,
    env: TypeEnv,
    instance_props: BTreeMap<String, HirType>,
    stage_child_props: BTreeSet<String>,
    static_props: BTreeSet<String>,
    instance_methods: BTreeSet<String>,
    instance_method_sigs: BTreeMap<String, SwiftCallableSig>,
    static_methods: BTreeSet<String>,
    static_method_sigs: BTreeMap<String, SwiftCallableSig>,
    local_func_sigs: BTreeMap<String, SwiftCallableSig>,
    current_class_short: Option<String>,
    class_has_stage: bool,
    in_static_method: bool,
    top_level_is_symbol_frame: bool,
    in_symbol_frame_method: bool,
    in_nfvalue_closure: bool,
    current_fn_param_names: Vec<String>,
    current_fn_params: Vec<(String, HirType)>,
    use_key_down_shim: bool,
    in_key_handler_event: Option<String>,
    current_return_type: HirType,
    current_return_is_optional: bool,
    in_try_depth: usize,
    in_throwing_fn: bool,
    local_scopes: Vec<BTreeSet<String>>,
    nullable_local_scopes: Vec<BTreeSet<String>>,
    vec_alias_scopes: Vec<BTreeMap<String, String>>,
    local_usage_stack: Vec<LocalUsageInfo>,
    active_for_loop_vars: Vec<(usize, Vec<String>)>,
    for_update_stack: Vec<(Option<String>, Option<Expr>)>,
    pending_loop_label: Option<String>,
    key_down_handlers: BTreeMap<String, Expr>,
    key_up_handlers: BTreeMap<String, Expr>,
    resize_handlers: BTreeMap<String, Expr>,
    explicit_key_listeners: BTreeSet<String>,
    explicit_stage_listeners: BTreeSet<String>,
    issues: Vec<ConversionIssue>,
    indent: usize,
}

impl<'a> SwiftEmitter<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        location: &'a str,
        raw_source: &'a str,
        source_is_as3: bool,
        stage: &'a ProjectStageIndex,
        symbol_id_by_swift_class: &'a BTreeMap<String, String>,
        host_methods: &'a BTreeMap<String, SwiftProxyMethod>,
        class_registry: &'a BTreeMap<String, SwiftClassInfo>,
        global_vars: &'a BTreeMap<String, HirType>,
        global_funcs: &'a BTreeMap<String, SwiftCallableSig>,
        symbol_id: Option<String>,
        class_name: Option<String>,
    ) -> Self {
        let mut env = TypeEnv::new(symbol_id, class_name);
        for (k, v) in global_vars {
            env.define(k.clone(), v.clone());
        }
        for (k, sig) in global_funcs {
            env.functions.insert(k.clone(), sig.return_ty.clone());
        }
        Self {
            location,
            raw_source,
            source_is_as3,
            stage,
            symbol_id_by_swift_class,
            host_methods,
            class_registry,
            global_vars,
            global_funcs,
            env,
            instance_props: BTreeMap::new(),
            stage_child_props: BTreeSet::new(),
            static_props: BTreeSet::new(),
            instance_methods: BTreeSet::new(),
            instance_method_sigs: BTreeMap::new(),
            static_methods: BTreeSet::new(),
            static_method_sigs: BTreeMap::new(),
            local_func_sigs: BTreeMap::new(),
            current_class_short: None,
            class_has_stage: true,
            in_static_method: false,
            top_level_is_symbol_frame: false,
            in_symbol_frame_method: false,
            in_nfvalue_closure: false,
            current_fn_param_names: Vec::new(),
            current_fn_params: Vec::new(),
            use_key_down_shim: false,
            in_key_handler_event: None,
            current_return_type: HirType::Void,
            current_return_is_optional: false,
            in_try_depth: 0,
            in_throwing_fn: false,
            local_scopes: vec![BTreeSet::new()],
            nullable_local_scopes: vec![BTreeSet::new()],
            vec_alias_scopes: vec![BTreeMap::new()],
            local_usage_stack: Vec::new(),
            active_for_loop_vars: Vec::new(),
            for_update_stack: Vec::new(),
            pending_loop_label: None,
            key_down_handlers: BTreeMap::new(),
            key_up_handlers: BTreeMap::new(),
            resize_handlers: BTreeMap::new(),
            explicit_key_listeners: BTreeSet::new(),
            explicit_stage_listeners: BTreeSet::new(),
            issues: Vec::new(),
            indent: 0,
        }
    }

    fn pad(&self) -> String {
        "    ".repeat(self.indent)
    }

    fn set_instance_props(&mut self, props: BTreeMap<String, HirType>) {
        for (k, v) in &props {
            self.env.define(k.clone(), v.clone());
        }
        self.instance_props = props;
    }

    fn push_scope(&mut self) {
        self.env.push_scope();
        self.local_scopes.push(BTreeSet::new());
        self.nullable_local_scopes.push(BTreeSet::new());
        self.vec_alias_scopes.push(BTreeMap::new());
    }

    fn pop_scope(&mut self) {
        self.env.pop_scope();
        if self.local_scopes.len() > 1 {
            self.local_scopes.pop();
        }
        if self.nullable_local_scopes.len() > 1 {
            self.nullable_local_scopes.pop();
        }
        if self.vec_alias_scopes.len() > 1 {
            self.vec_alias_scopes.pop();
        }
    }

    fn lookup_vector_alias(&self, name: &str) -> Option<&String> {
        for (lset, amap) in self
            .local_scopes
            .iter()
            .zip(self.vec_alias_scopes.iter())
            .rev()
        {
            if lset.contains(name) {
                return amap.get(name);
            }
        }
        None
    }

    fn insert_vector_alias(&mut self, name: String, target_expr: String) {
        if let Some(top) = self.vec_alias_scopes.last_mut() {
            top.insert(name, target_expr);
        }
    }

    fn is_local(&self, name: &str) -> bool {
        self.local_scopes.iter().any(|s| s.contains(name))
    }

    fn mark_local(&mut self, name: &str) {
        if let Some(top) = self.local_scopes.last_mut() {
            top.insert(name.to_owned());
        }
    }

    fn is_nullable_local(&self, name: &str) -> bool {
        self.nullable_local_scopes.iter().any(|s| s.contains(name))
    }

    fn mark_nullable_local(&mut self, name: &str) {
        if let Some(top) = self.nullable_local_scopes.last_mut() {
            top.insert(name.to_owned());
        }
    }

    fn match_class_cast_call(&self, callee: &Expr, args: &[Expr]) -> Option<HirType> {
        if args.len() != 1 {
            return None;
        }
        let Expr::Name(cls) = callee else {
            return None;
        };
        if self.is_local(cls)
            || self.instance_methods.contains(cls)
            || self.static_methods.contains(cls)
            || self.global_funcs.contains_key(cls)
        {
            return None;
        }
        if matches!(
            cls.as_str(),
            "int"
                | "uint"
                | "Number"
                | "String"
                | "Boolean"
                | "XML"
                | "XMLList"
                | "Date"
                | "RegExp"
                | "Array"
                | "Object"
                | "Error"
                | "TypeError"
                | "RangeError"
                | "ArgumentError"
                | "ReferenceError"
                | "SyntaxError"
        ) {
            return None;
        }
        if let Some(cinfo) = self.lookup_class_info(cls) {
            return Some(HirType::Class(cinfo.swift_name.clone()));
        }
        if matches!(
            cls.as_str(),
            "MovieClip"
                | "Sprite"
                | "DisplayObject"
                | "DisplayObjectContainer"
                | "InteractiveObject"
                | "SimpleButton"
                | "Button"
                | "TextField"
                | "Bitmap"
                | "BitmapData"
                | "Shape"
                | "Video"
                | "Loader"
                | "Sound"
                | "SoundChannel"
                | "ByteArray"
                | "SharedObject"
                | "NetConnection"
                | "NetStream"
                | "URLLoader"
                | "Event"
                | "MouseEvent"
                | "KeyboardEvent"
                | "TimerEvent"
                | "TouchEvent"
                | "FocusEvent"
                | "TextEvent"
                | "IOErrorEvent"
                | "ProgressEvent"
                | "NetStatusEvent"
                | "XMLNode"
                | "XMLDocument"
        ) {
            return Some(HirType::from_annotation(cls));
        }
        None
    }

    fn lookup_class_info(&self, raw_cls: &str) -> Option<&SwiftClassInfo> {
        self.class_registry
            .get(raw_cls)
            .or_else(|| self.class_registry.get(short_type_name(raw_cls)))
    }

    fn is_swift_reference_class(&self, ty: &HirType) -> bool {
        is_swift_reference_hir_type(ty)
    }

    fn has_instance_self(&self) -> bool {
        !self.in_static_method
            && (self.current_class_short.is_some() || self.env.symbol_id.is_some())
    }

    fn should_emit_local_as_var(&self, raw_name: &str, decl_ty: &HirType) -> (bool, bool) {
        let Some(usage) = self.local_usage_stack.last() else {
            return (true, true);
        };
        let directly_mutated = usage.direct_mutated.contains(raw_name);
        let struct_mutated = !self.is_swift_reference_class(decl_ty)
            && !Self::is_nfvalue_type(decl_ty)
            && usage.member_or_method_mutated.contains(raw_name);
        let emit_var = directly_mutated || struct_mutated;
        let is_read = usage.read_names.contains(raw_name)
            || usage.member_or_method_mutated.contains(raw_name);
        (emit_var, is_read)
    }

    fn emit_function_body(&mut self, stmts: &[Stmt]) -> String {
        self.local_usage_stack.push(LocalUsageInfo::analyze(stmts));
        let saved_for_updates = std::mem::take(&mut self.for_update_stack);
        let saved_loop_label = self.pending_loop_label.take();
        let fn_params = std::mem::take(&mut self.current_fn_params);
        let mut pre_hoist_lines = Vec::new();
        let pad = self.pad();
        for (raw_p, param_ty) in fn_params {
            if self.should_emit_local_as_var(&raw_p, &param_ty).0 {
                let pname = escape_swift_ident(&raw_p);
                pre_hoist_lines.push(format!("{pad}var {pname} = {pname}"));
            }
        }
        if !self.in_symbol_frame_method {
            let mut known: BTreeSet<String> = self.instance_props.keys().cloned().collect();
            known.extend(self.static_props.iter().cloned());
            known.extend(self.instance_methods.iter().cloned());
            known.extend(self.static_methods.iter().cloned());
            known.extend(self.global_vars.keys().cloned());
            known.extend(self.global_funcs.keys().cloned());
            for sc in &self.local_scopes {
                known.extend(sc.iter().cloned());
            }
            let hoisted = collect_hoisted_vars(stmts, !self.source_is_as3, &known);
            for h in hoisted {
                if !h.needs_function_hoist || self.is_local(&h.name) {
                    continue;
                }
                let mut decl_ty =
                    h.ty.as_deref()
                        .map(HirType::from_annotation)
                        .unwrap_or_else(|| {
                            h.first_init
                                .as_ref()
                                .map_or(HirType::Unknown, |e| self.infer(e))
                        });
                for (ety, einit) in &h.extra_decls {
                    let other_ty = ety
                        .as_deref()
                        .map(HirType::from_annotation)
                        .or_else(|| einit.as_ref().map(|e| self.infer(e)));
                    if let Some(ot) = other_ty
                        && ot != decl_ty
                    {
                        if (decl_ty.is_integer() && ot == HirType::Float)
                            || (decl_ty == HirType::Float && ot.is_integer())
                        {
                            decl_ty = HirType::Float;
                        } else {
                            decl_ty = HirType::Unknown;
                        }
                    }
                }
                let vname = escape_swift_ident(&h.name);
                let (sw_ty, def_val) = swift_property_type_and_default(&decl_ty);
                let (emit_var, is_read) = self.should_emit_local_as_var(&h.name, &decl_ty);
                let has_nested_init =
                    h.first_init.is_some() || h.extra_decls.iter().any(|(_, e)| e.is_some());
                self.mark_local(&h.name);
                if sw_ty.ends_with('?') {
                    self.mark_nullable_local(&h.name);
                }
                self.env.define(h.name.clone(), decl_ty);
                if emit_var || has_nested_init {
                    pre_hoist_lines.push(format!("{pad}var {vname}: {sw_ty} = {def_val}"));
                    if !is_read {
                        pre_hoist_lines.push(format!("{pad}_ = {vname}"));
                    }
                } else {
                    pre_hoist_lines.push(format!("{pad}let {vname}: {sw_ty} = {def_val}"));
                    pre_hoist_lines.push(format!("{pad}_ = {vname}"));
                }
            }
        }
        let out = self.emit_stmt_list(stmts);
        self.for_update_stack = saved_for_updates;
        self.pending_loop_label = saved_loop_label;
        self.local_usage_stack.pop();
        if pre_hoist_lines.is_empty() {
            out
        } else if out.trim().is_empty() {
            pre_hoist_lines.join("\n")
        } else {
            format!("{}\n{out}", pre_hoist_lines.join("\n"))
        }
    }

    fn record_issue(
        &mut self,
        kind: ConversionIssueKind,
        offset: usize,
        message: impl Into<String>,
    ) {
        let (line, column) = byte_offset_to_line_col(self.raw_source, offset);
        self.issues.push(ConversionIssue {
            kind,
            location: self.location.to_owned(),
            line,
            column,
            message: message.into(),
        });
    }

    fn host_receiver(&self) -> &'static str {
        if self.class_has_stage {
            "self.stage?.host?"
        } else {
            "NFRuntime.activeHost?"
        }
    }

    fn proxy_swift_type_to_hir(swift_ty: &str) -> HirType {
        match swift_ty.trim() {
            "Int" => HirType::Int,
            "UInt" => HirType::UInt,
            "Double" => HirType::Float,
            "Bool" => HirType::Bool,
            "String" => HirType::String,
            "[Double]" => HirType::Vector(Box::new(HirType::Float)),
            "[Int]" => HirType::Vector(Box::new(HirType::Int)),
            "[UInt]" => HirType::Vector(Box::new(HirType::UInt)),
            "[Bool]" => HirType::Vector(Box::new(HirType::Bool)),
            "[String]" => HirType::Vector(Box::new(HirType::String)),
            "[NFValue]" => HirType::Array,
            _ => HirType::Unknown,
        }
    }

    fn emit_host_proxy_call_inner(
        &mut self,
        method: &str,
        payload_args: &[Expr],
        offset: usize,
    ) -> (String, Option<String>) {
        let m_esc = escape_swift_ident(method);
        let contract = self.host_methods.get(method).cloned();
        let mut call_args = Vec::new();
        let param_len = contract.as_ref().map_or(payload_args.len(), |c| {
            c.params.len().max(payload_args.len())
        });
        for idx in 0..param_len {
            if let Some(spec) = contract.as_ref().and_then(|c| c.params.get(idx)) {
                let label = escape_swift_ident(&spec.name);
                if let Some(arg) = payload_args.get(idx) {
                    let expected = Self::proxy_swift_type_to_hir(&spec.swift_type);
                    let coerced = if spec.swift_type == "[NFValue]" {
                        if let Expr::Array(items) = arg {
                            let parts: Vec<String> = items
                                .iter()
                                .map(|it| {
                                    it.as_ref()
                                        .map(|e| self.wrap_nfvalue(e, offset))
                                        .unwrap_or_else(|| ".undefined".into())
                                })
                                .collect();
                            format!("[{}]", parts.join(", "))
                        } else {
                            self.emit_expr_with_offset(arg, offset)
                        }
                    } else if spec.swift_type == "[String: NFValue]" {
                        self.emit_expr_with_offset(arg, offset)
                    } else {
                        self.emit_coerced_expr(arg, &expected, offset)
                    };
                    call_args.push(format!("{label}: {coerced}"));
                } else {
                    let def_expr = swift_default_for_proxy_type(&spec.swift_type);
                    call_args.push(format!("{label}: {def_expr}"));
                }
            } else if let Some(arg) = payload_args.get(idx) {
                call_args.push(self.emit_expr_with_offset(arg, offset));
            }
        }
        let recv = self.host_receiver();
        let ret_ty = contract.and_then(|c| c.return_swift_type);
        (format!("{recv}.{m_esc}({})", call_args.join(", ")), ret_ty)
    }

    fn is_nullable_prop(&self, prop_name: &str) -> bool {
        if self.stage_child_props.contains(prop_name) {
            return false;
        }
        let Some(ty) = self.instance_props.get(prop_name) else {
            return false;
        };
        matches!(
            ty,
            HirType::TextField
                | HirType::MovieClip
                | HirType::Sprite
                | HirType::Button
                | HirType::Class(_)
        )
    }

    fn expr_self_is_optional(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Name(n) if self.is_local(n) => self.is_nullable_local(n),
            Expr::Name(n) => {
                self.is_nullable_prop(n)
                    || matches!(
                        n.as_str(),
                        "stage" | "root" | "_root" | "parent" | "_parent" | "mask"
                    )
            }
            Expr::Member(b, p) if matches!(b.as_ref(), Expr::Name(n) if n == "this") => {
                self.is_nullable_prop(p)
                    || matches!(
                        p.as_str(),
                        "stage" | "root" | "_root" | "parent" | "_parent" | "mask"
                    )
            }
            Expr::Member(base, prop) => {
                if is_stage_expr(base) {
                    return !matches!(prop.as_str(), "stageWidth" | "stageHeight" | "stage3Ds");
                }
                if !self.source_is_as3 && matches!(base.as_ref(), Expr::Name(c) if c == "Stage") {
                    return !matches!(prop.as_str(), "width" | "height" | "scaleMode" | "align");
                }
                let bty = self.infer(base);
                if matches!(
                    bty,
                    HirType::MovieClip | HirType::Sprite | HirType::Button | HirType::TextField
                ) && matches!(
                    prop.as_str(),
                    "stage"
                        | "root"
                        | "_root"
                        | "parent"
                        | "_parent"
                        | "mask"
                        | "scrollRect"
                        | "scale9Grid"
                ) {
                    return true;
                }
                if matches!(bty, HirType::TextField)
                    && matches!(prop.as_str(), "restrict" | "styleSheet" | "variable")
                {
                    return true;
                }
                if let HirType::Class(cn) = &bty {
                    match (short_type_name(cn), prop.as_str()) {
                        (
                            "Event" | "MouseEvent" | "TouchEvent" | "KeyboardEvent" | "FocusEvent"
                            | "TimerEvent" | "TextEvent" | "IOErrorEvent" | "NetStatusEvent",
                            "target" | "currentTarget",
                        ) => return true,
                        (
                            "XML" | "XMLNode" | "XMLDocument" | "NFXML" | "NFXMLNode"
                            | "NFXMLDocument",
                            "firstChild" | "lastChild" | "nextSibling" | "previousSibling"
                            | "parentNode" | "nodeName" | "nodeValue",
                        ) => return true,
                        ("Bitmap", "bitmapData") => return true,
                        _ => {}
                    }
                    if let Some(cinfo) = self.lookup_class_info(cn)
                        && cinfo.nullable_fields.contains(prop)
                    {
                        return true;
                    }
                }
                false
            }
            Expr::Call(callee, args, _) => {
                if self.match_class_cast_call(callee, args).is_some() {
                    return true;
                }
                if match_host_proxy_call(callee, args).is_some() {
                    return false;
                }
                if let Expr::Name(fn_name) = callee.as_ref()
                    && !self.is_local(fn_name)
                    && let Some(sig) = self
                        .instance_method_sigs
                        .get(fn_name)
                        .or_else(|| self.global_funcs.get(fn_name))
                    && (sig.return_is_optional
                        || (sig.can_throw && self.in_try_depth == 0 && !self.in_throwing_fn))
                {
                    return true;
                }
                if let Expr::Member(base, method) = callee.as_ref() {
                    if matches!(base.as_ref(), Expr::Name(n) if n == "this")
                        && let Some(sig) = self.instance_method_sigs.get(method)
                        && (sig.return_is_optional
                            || (sig.can_throw && self.in_try_depth == 0 && !self.in_throwing_fn))
                    {
                        return true;
                    }
                    if let Expr::Name(cls) = base.as_ref() {
                        if cls == "BitmapData" && method == "loadBitmap" {
                            return true;
                        }
                        if let Some(cinfo) = self.lookup_class_info(cls)
                            && let Some(sig) = cinfo.static_methods.get(method)
                            && (sig.return_is_optional
                                || (sig.can_throw
                                    && self.in_try_depth == 0
                                    && !self.in_throwing_fn))
                        {
                            return true;
                        }
                    }
                    let bty = self.infer(base);
                    if let HirType::Class(cn) = &bty {
                        if short_type_name(cn) == "Sound" && method == "play" {
                            return true;
                        }
                        if let Some(cinfo) = self.lookup_class_info(cn)
                            && let Some(sig) = cinfo.instance_methods.get(method)
                            && (sig.return_is_optional
                                || (sig.can_throw
                                    && self.in_try_depth == 0
                                    && !self.in_throwing_fn))
                        {
                            return true;
                        }
                    }
                    if (matches!(bty, HirType::MovieClip | HirType::Sprite)
                        || matches!(&bty, HirType::Class(cn) if self.symbol_id_by_swift_class.contains_key(cn.as_str())))
                        && matches!(
                            method.as_str(),
                            "getChildByName" | "getChildAt" | "attachMovie"
                        )
                    {
                        return true;
                    }
                }
                false
            }
            Expr::Conditional(_, yes, no) => {
                let yt = self.infer(yes);
                let nt = self.infer(no);
                (is_null_or_undef(yes) && self.is_swift_reference_class(&nt))
                    || (is_null_or_undef(no) && self.is_swift_reference_class(&yt))
            }
            Expr::TypeTest(op, _, ty_name, _) if op == "as" => {
                !Self::is_nfvalue_type(&HirType::from_annotation(ty_name))
            }
            _ => false,
        }
    }

    fn expr_produces_optional(&self, expr: &Expr) -> bool {
        if let Expr::Call(callee, args, _) = expr
            && args.is_empty()
            && self.in_try_depth == 0
            && !self.in_throwing_fn
            && let Expr::Member(base, method) = callee.as_ref()
            && method == "readObject"
            && matches!(self.infer(base), HirType::Class(cn) if short_type_name(&cn) == "ByteArray")
        {
            return false;
        }
        if let Expr::Member(base, prop) = expr {
            if is_root_display_expr(base) {
                return infer_builtin_display_member_type(prop).is_some()
                    && (self.source_is_as3 || map_as2_percent_prop_to_modern(prop).is_none());
            }
            if !self.source_is_as3 && map_as2_percent_prop_to_modern(prop).is_some() {
                return false;
            }
            if self.expr_produces_nfvalue(base) {
                return false;
            }
        }
        if self.expr_self_is_optional(expr) {
            return true;
        }
        match expr {
            Expr::Member(base, _) => self.expr_produces_optional(base),
            Expr::Call(callee, _, _) => {
                if let Expr::Member(base, _) = callee.as_ref() {
                    if self.expr_produces_nfvalue(base) {
                        return false;
                    }
                    return self.expr_produces_optional(base);
                }
                false
            }
            _ => false,
        }
    }

    fn infer(&self, expr: &Expr) -> HirType {
        match expr {
            Expr::Function(_) => return HirType::Function,
            Expr::RegExpLiteral(_, _) => return HirType::Class("RegExp".into()),
            Expr::XmlLiteral(_, _) => return HirType::Class("XML".into()),
            Expr::Name(n) if matches!(n.as_str(), "NaN" | "Infinity") && !self.is_local(n) => {
                return HirType::Float;
            }
            Expr::Literal(lit) if matches!(lit.trim(), "NaN" | "Infinity") => {
                return HirType::Float;
            }
            Expr::XmlSelect { .. } | Expr::XmlWithAttr { .. } | Expr::XmlFilter(_, _, _) => {
                return HirType::Class("XMLList".into());
            }
            Expr::NewVector(elem, _, _) => {
                return HirType::Vector(Box::new(HirType::from_annotation(elem)));
            }
            Expr::NewError(ty, _, _) => return HirType::Class(ty.clone()),
            Expr::NewClass(ty, _, _) | Expr::NewBuiltin(ty, _, _) | Expr::NewValue(ty, _, _) => {
                return HirType::from_annotation(ty);
            }
            Expr::Unary(op, inner) => match op.as_str() {
                "!" | "not" => return HirType::Bool,
                "~" => return HirType::Int,
                "typeof" => return HirType::String,
                "+" => {
                    let it = self.infer(inner);
                    return if it == HirType::Int {
                        HirType::Int
                    } else {
                        HirType::Float
                    };
                }
                "-" => {
                    let it = self.infer(inner);
                    return if it == HirType::Int {
                        HirType::Int
                    } else if it == HirType::Float || it == HirType::UInt {
                        HirType::Float
                    } else {
                        HirType::Unknown
                    };
                }
                _ => {}
            },
            Expr::Binary(op, lhs, rhs) => {
                if op == "|" && matches!(rhs.as_ref(), Expr::Literal(s) if s == "0") {
                    return HirType::Int;
                }
                if op == ">>>" && matches!(rhs.as_ref(), Expr::Literal(s) if s == "0") {
                    return HirType::UInt;
                }
                match op.as_str() {
                    "==" | "!=" | "===" | "!==" | "<" | "<=" | ">" | ">=" | "and" | "or" | "eq"
                    | "ne" | "<>" | "in" => return HirType::Bool,
                    "&&" | "||" => {
                        let lt = self.infer(lhs);
                        let rt = self.infer(rhs);
                        if lt == HirType::Bool && rt == HirType::Bool {
                            return HirType::Bool;
                        }
                        if lt == rt {
                            return lt;
                        }
                        return HirType::Unknown;
                    }
                    "&" | "|" | "^" | "<<" | ">>" | ">>>" => return HirType::Int,
                    "/" => return HirType::Float,
                    "+" | "add" => {
                        let lt = self.infer(lhs);
                        let rt = self.infer(rhs);
                        if lt == HirType::String || rt == HirType::String {
                            return HirType::String;
                        }
                        if (lt == HirType::Float || lt.is_integer())
                            && (rt == HirType::Float || rt.is_integer())
                        {
                            return if lt == HirType::Float || rt == HirType::Float {
                                HirType::Float
                            } else if lt == HirType::UInt && rt == HirType::UInt {
                                HirType::UInt
                            } else {
                                HirType::Int
                            };
                        }
                        return HirType::Unknown;
                    }
                    "-" | "*" | "%" => {
                        let lt = self.infer(lhs);
                        let rt = self.infer(rhs);
                        if lt == HirType::UInt && rt == HirType::UInt {
                            return HirType::UInt;
                        }
                        if lt.is_integer() && rt.is_integer() {
                            return HirType::Int;
                        }
                        return HirType::Float;
                    }
                    _ => {}
                }
            }
            Expr::Conditional(_, yes, no) => {
                let yt = self.infer(yes);
                let nt = self.infer(no);
                if yt == nt {
                    return yt;
                }
                if is_null_or_undef(yes) && self.is_swift_reference_class(&nt) {
                    return nt;
                }
                if is_null_or_undef(no) && self.is_swift_reference_class(&yt) {
                    return yt;
                }
                if (yt == HirType::Float && nt.is_integer())
                    || (nt == HirType::Float && yt.is_integer())
                {
                    return HirType::Float;
                }
                return HirType::Unknown;
            }
            Expr::Index(base, _) => {
                let bty = self.infer(base);
                if let HirType::Vector(elem) = bty {
                    return (*elem).clone();
                }
                if let HirType::Class(cn) = &bty {
                    match short_type_name(cn) {
                        "ByteArray" => return HirType::Int,
                        "XML" | "XMLList" => return HirType::Class("XML".into()),
                        _ => {}
                    }
                }
            }
            Expr::Call(callee, args, _) => {
                if let Some(cast_ty) = self.match_class_cast_call(callee, args) {
                    return cast_ty;
                }
                if let Some((method, _)) = match_host_proxy_call(callee, args)
                    && let Some(spec) = self.host_methods.get(&method)
                {
                    return spec
                        .return_swift_type
                        .as_deref()
                        .map_or(HirType::Void, Self::proxy_swift_type_to_hir);
                }
                if let Expr::Name(fn_name) = callee.as_ref() {
                    match fn_name.as_str() {
                        "isNaN" | "isFinite" => return HirType::Bool,
                        "parseInt" | "parseFloat" => return HirType::Float,
                        "getTimer" | "setTimeout" | "setInterval" => return HirType::Int,
                        "escape" | "unescape" | "encodeURI" | "decodeURI"
                        | "encodeURIComponent" | "decodeURIComponent" => return HirType::String,
                        "XML" => return HirType::Class("XML".into()),
                        "XMLList" => return HirType::Class("XMLList".into()),
                        _ => {}
                    }
                    if !self.is_local(fn_name)
                        && let Some(sig) = self.instance_method_sigs.get(fn_name)
                    {
                        return sig.return_ty.clone();
                    }
                    if !self.is_local(fn_name)
                        && let Some(sig) = self.static_method_sigs.get(fn_name)
                    {
                        return sig.return_ty.clone();
                    }
                    if !self.is_local(fn_name)
                        && let Some(sig) = self.global_funcs.get(fn_name)
                    {
                        return sig.return_ty.clone();
                    }
                }
                if let Expr::Member(base, method) = callee.as_ref() {
                    if matches!(base.as_ref(), Expr::Name(n) if n == "this")
                        && let Some(sig) = self.instance_method_sigs.get(method)
                    {
                        return sig.return_ty.clone();
                    }
                    if let Expr::Name(cls) = base.as_ref()
                        && !self.is_local(cls)
                    {
                        match (cls.as_str(), method.as_str()) {
                            ("Math", _) => return HirType::Float,
                            ("NFRuntime", "imul") => return HirType::Int,
                            ("SharedObject", "getLocal") => {
                                return HirType::Class("SharedObject".into());
                            }
                            ("BitmapData", "loadBitmap") => {
                                return HirType::Class("BitmapData".into());
                            }
                            ("JSON", "parse") => return HirType::Object,
                            ("JSON", "stringify") => return HirType::String,
                            ("Date", "UTC" | "parse") => return HirType::Float,
                            ("String", "fromCharCode") => return HirType::String,
                            _ => {}
                        }
                        if let Some(cinfo) = self.lookup_class_info(cls)
                            && let Some(sig) = cinfo.static_methods.get(method)
                        {
                            return sig.return_ty.clone();
                        }
                    }
                    let base_ty = self.infer(base);
                    if base_ty == HirType::String {
                        return match method.as_str() {
                            "toLowerCase" | "toLocaleLowerCase" | "toUpperCase"
                            | "toLocaleUpperCase" | "substr" | "substring" | "slice" | "charAt"
                            | "replace" | "trim" | "toString" | "valueOf" => HirType::String,
                            "indexOf" | "lastIndexOf" | "search" => HirType::Int,
                            "charCodeAt" => HirType::Float,
                            "split" => HirType::Vector(Box::new(HirType::String)),
                            "match" => HirType::Object,
                            _ => HirType::Unknown,
                        };
                    }
                    if matches!(base_ty, HirType::Float | HirType::Int | HirType::UInt) {
                        return match method.as_str() {
                            "toFixed" | "toExponential" | "toPrecision" | "toString" => {
                                HirType::String
                            }
                            _ => HirType::Unknown,
                        };
                    }
                    if let HirType::Vector(elem) = &base_ty {
                        return match method.as_str() {
                            "pop" | "shift" => (**elem).clone(),
                            "push" | "unshift" | "indexOf" | "lastIndexOf" => HirType::Int,
                            "join" => HirType::String,
                            "slice" | "splice" | "concat" => base_ty,
                            _ => HirType::Unknown,
                        };
                    }
                    if Self::is_nfvalue_type(&base_ty) {
                        match method.as_str() {
                            "join" => return HirType::String,
                            "push" | "unshift" | "indexOf" | "lastIndexOf" => return HirType::Int,
                            "hasOwnProperty" => return HirType::Bool,
                            _ => {}
                        }
                    }
                    if let HirType::Class(cn) = &base_ty {
                        match (short_type_name(cn), method.as_str()) {
                            ("Sound", "play") => return HirType::Class("SoundChannel".into()),
                            ("SharedObject", "flush") => return HirType::String,
                            ("ByteArray", "readBoolean") => return HirType::Bool,
                            ("ByteArray", "readByte" | "readShort" | "readInt") => {
                                return HirType::Int;
                            }
                            (
                                "ByteArray",
                                "readUnsignedByte" | "readUnsignedShort" | "readUnsignedInt",
                            ) => return HirType::UInt,
                            ("ByteArray", "readFloat" | "readDouble") => return HirType::Float,
                            (
                                "ByteArray",
                                "readUTF" | "readUTFBytes" | "readMultiByte" | "toString",
                            ) => {
                                return HirType::String;
                            }
                            ("ByteArray", "readObject") => return HirType::Object,
                            ("BitmapData", "getPixel" | "getPixel32" | "threshold") => {
                                return HirType::UInt;
                            }
                            ("BitmapData", "clone") => return HirType::Class("BitmapData".into()),
                            ("Sound", "getVolume" | "getPan") => return HirType::Float,
                            ("Sound", "getBytesLoaded" | "getBytesTotal") => return HirType::Int,
                            ("SharedObject", "getSize") => return HirType::Int,
                            ("XML", "copy") => return HirType::Class("XML".into()),
                            ("XMLList", "copy") => return HirType::Class("XMLList".into()),
                            (
                                "XML" | "XMLList",
                                "child" | "children" | "elements" | "attribute" | "descendants"
                                | "text",
                            ) => return HirType::Class("XMLList".into()),
                            (
                                "XML" | "XMLList" | "XMLNode" | "XMLDocument",
                                "toString" | "toXMLString" | "name",
                            ) => {
                                return HirType::String;
                            }
                            ("XML" | "XMLList", "length") => return HirType::Int,
                            (
                                "XMLNode" | "XMLDocument",
                                "cloneNode" | "createElement" | "createTextNode",
                            ) => {
                                return HirType::Class("XMLNode".into());
                            }
                            ("XMLNode" | "XMLDocument", "hasChildNodes" | "load") => {
                                return HirType::Bool;
                            }
                            ("BitmapData", "getColorBoundsRect") => {
                                return HirType::Class("Rectangle".into());
                            }
                            ("BitmapData", "getPixels") => {
                                return HirType::Class("ByteArray".into());
                            }
                            ("BitmapData", "getVector") => {
                                return HirType::Vector(Box::new(HirType::UInt));
                            }
                            ("BitmapData", "hitTest") => return HirType::Bool,
                            ("Date", "getTime" | "valueOf" | "getTimezoneOffset") => {
                                return HirType::Float;
                            }
                            (
                                "Date",
                                "getFullYear" | "getMonth" | "getDate" | "getDay" | "getHours"
                                | "getMinutes" | "getSeconds" | "getMilliseconds"
                                | "getUTCFullYear" | "getUTCMonth" | "getUTCDate" | "getUTCDay"
                                | "getUTCHours" | "getUTCMinutes" | "getUTCSeconds"
                                | "getUTCMilliseconds",
                            ) => return HirType::Int,
                            (
                                "Date",
                                "toString" | "toUTCString" | "toISOString" | "toDateString"
                                | "toTimeString",
                            ) => {
                                return HirType::String;
                            }
                            ("RegExp", "test") => return HirType::Bool,
                            ("RegExp", "exec") => return HirType::Object,
                            ("Point", "clone" | "add" | "subtract") => {
                                return HirType::Class("Point".into());
                            }
                            ("Rectangle", "clone" | "intersection" | "union") => {
                                return HirType::Class("Rectangle".into());
                            }
                            (
                                "Rectangle",
                                "contains" | "containsPoint" | "containsRect" | "intersects"
                                | "isEmpty",
                            ) => {
                                return HirType::Bool;
                            }
                            _ => {}
                        }
                        if let Some(cinfo) = self.lookup_class_info(cn)
                            && let Some(sig) = cinfo.instance_methods.get(method)
                        {
                            return sig.return_ty.clone();
                        }
                    }
                    if matches!(
                        base_ty,
                        HirType::MovieClip
                            | HirType::Sprite
                            | HirType::TextField
                            | HirType::Class(_)
                    ) {
                        match method.as_str() {
                            "getChildByName" | "getChildAt" | "addChild" | "addChildAt"
                            | "removeChild" | "removeChildAt" => {
                                return HirType::Class("DisplayObject".into());
                            }
                            "contains" | "hitTestObject" | "hitTestPoint" => return HirType::Bool,
                            "getBounds" | "getRect" => return HirType::Class("Rectangle".into()),
                            "localToGlobal" | "globalToLocal" => {
                                return HirType::Class("Point".into());
                            }
                            "getTextFormat" | "getNewTextFormat" => {
                                return HirType::Class("TextFormat".into());
                            }
                            _ => {}
                        }
                    }
                }
            }
            Expr::Name(n) => {
                if n == "this" {
                    if let Some(cls) = &self.current_class_short {
                        return HirType::Class(cls.clone());
                    }
                    return HirType::MovieClip;
                }
                if matches!(n.as_str(), "_root" | "_level0" | "_global" | "arguments") {
                    return HirType::Object;
                }
                if !self.is_local(n)
                    && let Some(t) = self.instance_props.get(n)
                {
                    return t.clone();
                }
                if let Some(t) = self.env.lookup(n) {
                    return t.clone();
                }
                if let Some(t) = self.instance_props.get(n) {
                    return t.clone();
                }
                if let Some(cls) = &self.current_class_short
                    && let Some(cinfo) = self.lookup_class_info(cls)
                    && let Some(t) = cinfo.static_fields.get(n)
                {
                    return t.clone();
                }
                if let Some(t) = self.global_vars.get(n) {
                    return t.clone();
                }
                if !self.is_local(n)
                    && self.class_has_stage
                    && let Some(dt) = infer_builtin_display_member_type(n)
                {
                    return dt;
                }
            }
            Expr::Member(b, p) if matches!(b.as_ref(), Expr::Name(n) if n == "this") => {
                if let Some(t) = self.instance_props.get(p) {
                    return t.clone();
                }
                if self.class_has_stage
                    && let Some(dt) = infer_builtin_display_member_type(p)
                {
                    return dt;
                }
            }
            Expr::Member(b, p) if is_external_interface_base(b) && p == "available" => {
                return HirType::Bool;
            }
            Expr::Member(base, prop) => {
                if is_stage_expr(base) {
                    return match prop.as_str() {
                        "stageWidth" | "stageHeight" | "frameRate" => HirType::Float,
                        "scaleMode" => HirType::Class("NFScaleMode".into()),
                        "align" => HirType::Class("NFStageAlign".into()),
                        "focus" => HirType::Class("DisplayObject".into()),
                        "stage3Ds" => HirType::Array,
                        _ => HirType::Unknown,
                    };
                }
                if is_root_display_expr(base)
                    && let Some(dt) = infer_builtin_display_member_type(prop)
                {
                    return dt;
                }
                if let Expr::Name(cls) = base.as_ref()
                    && !self.is_local(cls)
                {
                    if cls == "Math" {
                        return HirType::Float;
                    }
                    if (cls == "Key" && map_as2_key_constant(prop).is_some())
                        || (cls == "Keyboard" && map_as3_keyboard_constant(prop).is_some())
                    {
                        return HirType::UInt;
                    }
                    if cls == "XML" {
                        match prop.as_str() {
                            "prettyPrinting"
                            | "ignoreWhitespace"
                            | "ignoreComments"
                            | "ignoreProcessingInstructions" => return HirType::Bool,
                            "prettyIndent" => return HirType::Int,
                            _ => {}
                        }
                    }
                    if matches!(
                        cls.as_str(),
                        "BlendMode"
                            | "TextFieldType"
                            | "StageScaleMode"
                            | "StageAlign"
                            | "Endian"
                            | "CompressionAlgorithm"
                            | "SharedObjectFlushStatus"
                            | "URLLoaderDataFormat"
                            | "TextFieldAutoSize"
                            | "AntiAliasType"
                            | "GridFitType"
                            | "PixelSnapping"
                    ) {
                        return HirType::String;
                    }
                    if cls == "ObjectEncoding" {
                        return HirType::UInt;
                    }
                    if cls == "Capabilities" {
                        return match prop.as_str() {
                            "screenResolutionX" | "screenResolutionY" | "screenDPI"
                            | "pixelAspectRatio" => HirType::Float,
                            "isDebugger" | "hasAudio" | "hasStreamingAudio"
                            | "hasStreamingVideo" | "hasEmbeddedVideo" | "hasMP3" => HirType::Bool,
                            _ => HirType::String,
                        };
                    }
                    if classify_swift_event_type(expr).is_some() {
                        return HirType::String;
                    }
                    if let Some(cinfo) = self.lookup_class_info(cls)
                        && let Some(fty) = cinfo.static_fields.get(prop)
                    {
                        return fty.clone();
                    }
                }
                let bty = self.infer(base);
                if matches!(bty, HirType::Vector(_) | HirType::String) && prop == "length" {
                    return HirType::Int;
                }
                if Self::is_nfvalue_type(&bty) && prop == "length" {
                    return HirType::Int;
                }
                if Self::is_nfvalue_type(&bty) {
                    return HirType::Unknown;
                }
                if let HirType::Class(cn) = &bty {
                    let short = short_type_name(cn);
                    match (short, prop.as_str()) {
                        (
                            "Error" | "TypeError" | "RangeError" | "ArgumentError"
                            | "ReferenceError" | "SyntaxError",
                            "message" | "name",
                        ) => return HirType::String,
                        (
                            "Error" | "TypeError" | "RangeError" | "ArgumentError"
                            | "ReferenceError" | "SyntaxError",
                            "errorID",
                        ) => return HirType::Int,
                        (
                            "Event" | "MouseEvent" | "TouchEvent" | "KeyboardEvent" | "FocusEvent"
                            | "ProgressEvent",
                            "stageX" | "stageY" | "localX" | "localY" | "pressure" | "bytesLoaded"
                            | "bytesTotal",
                        ) => return HirType::Float,
                        (
                            "Event" | "MouseEvent" | "TouchEvent" | "KeyboardEvent" | "FocusEvent",
                            "touchPointID",
                        ) => return HirType::Int,
                        (
                            "Event" | "MouseEvent" | "TouchEvent" | "KeyboardEvent" | "FocusEvent",
                            "buttonDown"
                            | "isPrimaryTouchPoint"
                            | "ctrlKey"
                            | "shiftKey"
                            | "altKey",
                        ) => return HirType::Bool,
                        (
                            "Event" | "MouseEvent" | "TouchEvent" | "KeyboardEvent" | "FocusEvent"
                            | "TimerEvent" | "TextEvent" | "IOErrorEvent" | "NetStatusEvent",
                            "type" | "text",
                        ) => return HirType::String,
                        (
                            "Event" | "MouseEvent" | "TouchEvent" | "KeyboardEvent" | "FocusEvent"
                            | "TimerEvent" | "TextEvent" | "IOErrorEvent" | "NetStatusEvent",
                            "target" | "currentTarget",
                        ) => return HirType::Class("DisplayObject".into()),
                        ("Event" | "KeyboardEvent", "keyCode" | "charCode") => {
                            return HirType::UInt;
                        }
                        ("NetStatusEvent", "info") => return HirType::Object,
                        ("SharedObject", "data") => return HirType::Object,
                        ("SharedObject", "size") => return HirType::Int,
                        ("SharedObject" | "ByteArray" | "NetConnection", "objectEncoding") => {
                            return HirType::UInt;
                        }
                        ("ByteArray", "length" | "position" | "bytesAvailable") => {
                            return HirType::Int;
                        }
                        ("ByteArray", "endian") => return HirType::String,
                        ("BitmapData", "width" | "height") => return HirType::Int,
                        ("BitmapData", "transparent") => return HirType::Bool,
                        ("BitmapData", "rect" | "rectangle") => {
                            return HirType::Class("Rectangle".into());
                        }
                        ("Bitmap", "bitmapData") => return HirType::Class("BitmapData".into()),
                        ("Bitmap" | "Video", "smoothing") => return HirType::Bool,
                        ("Video", "deblocking") => return HirType::Int,
                        ("Matrix" | "NFAffineMatrix", "a" | "b" | "c" | "d" | "tx" | "ty") => {
                            return HirType::Float;
                        }
                        (
                            "ColorTransform",
                            "redMultiplier" | "greenMultiplier" | "blueMultiplier"
                            | "alphaMultiplier" | "redOffset" | "greenOffset" | "blueOffset"
                            | "alphaOffset",
                        ) => return HirType::Float,
                        ("ColorTransform", "color") => return HirType::UInt,
                        (
                            "SoundTransform",
                            "volume" | "pan" | "leftToLeft" | "leftToRight" | "rightToLeft"
                            | "rightToRight",
                        ) => return HirType::Float,
                        (
                            "Sound" | "SoundChannel",
                            "position" | "length" | "duration" | "leftPeak" | "rightPeak",
                        ) => {
                            return HirType::Float;
                        }
                        ("SoundChannel" | "NetStream", "soundTransform") => {
                            return HirType::Class("SoundTransform".into());
                        }
                        ("Sound", "onLoad" | "onSoundComplete") => return HirType::Function,
                        ("Sound", "id3") => return HirType::Object,
                        ("Timer", "currentCount" | "repeatCount") => return HirType::Int,
                        ("Timer", "delay") => return HirType::Float,
                        ("Timer", "running") => return HirType::Bool,
                        ("NetStream", "time" | "duration" | "bufferTime" | "bufferLength") => {
                            return HirType::Float;
                        }
                        ("NetStream" | "NetConnection", "client") => return HirType::Object,
                        ("NetStream" | "NetConnection", "onStatus" | "onMetaData") => {
                            return HirType::Function;
                        }
                        ("NetConnection", "connected") => return HirType::Bool,
                        ("NetConnection", "uri") => return HirType::String,
                        ("NetStream", "videoWidth" | "videoHeight") => return HirType::Int,
                        ("NetStream", "currentFPS" | "currentFps") => return HirType::Float,
                        ("Video", "videoWidth" | "videoHeight") => return HirType::Int,
                        (
                            "Date" | "NFDate",
                            "fullYear" | "month" | "date" | "day" | "hours" | "minutes" | "seconds"
                            | "milliseconds" | "fullYearUTC" | "monthUTC" | "dateUTC" | "dayUTC"
                            | "hoursUTC" | "minutesUTC" | "secondsUTC" | "millisecondsUTC",
                        ) => return HirType::Int,
                        ("Date" | "NFDate", "time" | "timezoneOffset") => return HirType::Float,
                        (
                            "Event" | "MouseEvent" | "TouchEvent" | "KeyboardEvent" | "FocusEvent"
                            | "TextEvent" | "ErrorEvent" | "IOErrorEvent",
                            "type" | "key" | "text",
                        ) => return HirType::String,
                        ("URLLoader", "data") => return HirType::Object,
                        ("Loader", "contentLoaderInfo") => {
                            return HirType::Class("LoaderInfo".into());
                        }
                        ("Point", "x" | "y" | "length") => return HirType::Float,
                        (
                            "Rectangle",
                            "x" | "y" | "width" | "height" | "left" | "right" | "top" | "bottom",
                        ) => return HirType::Float,
                        (
                            "XML" | "XMLNode" | "XMLDocument" | "NFXML" | "NFXMLNode"
                            | "NFXMLDocument",
                            "firstChild" | "lastChild" | "nextSibling" | "previousSibling"
                            | "parentNode",
                        ) => return HirType::Class("XMLNode".into()),
                        ("TextFormat" | "NFTextFormat", "font" | "align" | "url" | "target") => {
                            return HirType::String;
                        }
                        (
                            "TextFormat" | "NFTextFormat",
                            "size" | "leading" | "letterSpacing" | "leftMargin" | "rightMargin"
                            | "indent",
                        ) => return HirType::Float,
                        ("TextFormat" | "NFTextFormat", "color") => return HirType::UInt,
                        (
                            "TextFormat" | "NFTextFormat",
                            "bold" | "italic" | "underline" | "bullet" | "kerning",
                        ) => return HirType::Bool,
                        (
                            "XMLNode" | "XMLDocument" | "NFXMLNode" | "NFXMLDocument",
                            "nodeName" | "nodeValue" | "localName",
                        ) => return HirType::String,
                        (
                            "XMLNode" | "XMLDocument" | "NFXMLNode" | "NFXMLDocument",
                            "nodeType" | "status",
                        ) => return HirType::Int,
                        (
                            "XMLNode" | "XMLDocument" | "NFXMLNode" | "NFXMLDocument",
                            "childNodes",
                        ) => return HirType::Vector(Box::new(HirType::Class("XMLNode".into()))),
                        (
                            "XMLNode" | "XMLDocument" | "NFXMLNode" | "NFXMLDocument",
                            "attributes",
                        ) => return HirType::Object,
                        (
                            "XMLNode" | "XMLDocument" | "NFXMLNode" | "NFXMLDocument",
                            "ignoreWhite" | "loaded",
                        ) => return HirType::Bool,
                        ("XMLNode" | "XMLDocument" | "NFXMLNode" | "NFXMLDocument", "onLoad") => {
                            return HirType::Function;
                        }
                        ("XML", prop_name) if !prop_name.is_empty() => {
                            return HirType::Class("XMLList".into());
                        }
                        ("XMLList", prop_name) if !prop_name.is_empty() => {
                            return HirType::Class("XMLList".into());
                        }
                        _ => {}
                    }
                    if let Some(sym_id) = self.symbol_id_by_swift_class.get(cn.as_str())
                        && let Some(children) = self.stage.symbol_children.get(sym_id)
                        && let Some(cinfo) = children.get(prop)
                    {
                        return cinfo.hir_type.clone();
                    }
                    if let Some(cinfo) = self.lookup_class_info(cn)
                        && let Some(fty) = cinfo.instance_fields.get(prop)
                    {
                        return fty.clone();
                    }
                    if let Some(dt) = infer_builtin_display_member_type(prop) {
                        return dt;
                    }
                }
                if matches!(bty, HirType::TextField) {
                    match prop.as_str() {
                        "text" | "htmlText" | "restrict" | "autoSize" | "antiAliasType"
                        | "gridFitType" | "variable" => return HirType::String,
                        "defaultTextFormat" => return HirType::Class("TextFormat".into()),
                        "styleSheet" => return HirType::Class("StyleSheet".into()),
                        "textColor" | "borderColor" | "backgroundColor" => return HirType::UInt,
                        "length"
                        | "numLines"
                        | "scrollV"
                        | "maxScrollV"
                        | "bottomScrollV"
                        | "scrollH"
                        | "maxScrollH"
                        | "scroll"
                        | "maxscroll"
                        | "hscroll"
                        | "maxhscroll"
                        | "caretIndex"
                        | "selectionBeginIndex"
                        | "selectionEndIndex" => return HirType::Int,
                        "textWidth" | "textHeight" | "sharpness" | "thickness" => {
                            return HirType::Float;
                        }
                        "condenseWhite" | "alwaysShowSelection" | "mouseWheelEnabled" | "html" => {
                            return HirType::Bool;
                        }
                        "onChanged" | "onSetFocus" | "onKillFocus" => return HirType::Function,
                        _ => {}
                    }
                }
                if matches!(
                    bty,
                    HirType::MovieClip | HirType::Sprite | HirType::Button | HirType::TextField
                ) && let Some(dt) = infer_builtin_display_member_type(prop)
                {
                    return dt;
                }
            }
            _ => {}
        }
        self.env.infer_expr(expr, self.stage)
    }

    fn precollect(&mut self, stmts: &[Stmt]) {
        if self.source_is_as3 {
            return;
        }
        for stmt in stmts {
            if let Stmt::Expr(Expr::Call(callee, args, _)) = stmt.unlocated()
                && args.len() == 1
                && let Expr::Member(base, method) = callee.as_ref()
                && method == "addListener"
                && let Expr::Name(mgr) = base.as_ref()
                && let Some(key) = listener_target_key(&args[0])
            {
                if mgr == "Key" {
                    self.explicit_key_listeners.insert(key);
                } else if mgr == "Stage" {
                    self.explicit_stage_listeners.insert(key);
                }
            }
        }
    }

    fn emit_helper_method(&mut self, raw_name: &str, f: &Function) -> String {
        let fname = escape_swift_ident(raw_name);
        let saved_top = self.top_level_is_symbol_frame;
        let saved_in_frame = self.in_symbol_frame_method;
        self.top_level_is_symbol_frame = false;
        self.in_symbol_frame_method = false;

        let is_key_fn = f.params.first().is_some_and(|(_, ty)| {
            ty.as_deref()
                .is_some_and(|t| short_type_name(t) == "KeyboardEvent")
        });
        let saved_ke = self.in_key_handler_event.clone();
        if is_key_fn {
            self.in_key_handler_event = f.params.first().map(|(n, _)| n.clone());
        }

        let ret_hir = f
            .returns
            .as_deref()
            .map_or(HirType::Void, HirType::from_annotation);
        let ret_opt = self.is_swift_reference_class(&ret_hir) && stmts_return_null(&f.body);
        let can_throw = raw_name != "onResize" && stmts_can_throw_uncaught(&f.body);
        let saved_ret = std::mem::replace(&mut self.current_return_type, ret_hir.clone());
        let saved_ret_opt = std::mem::replace(&mut self.current_return_is_optional, ret_opt);
        let saved_throw = std::mem::replace(&mut self.in_throwing_fn, can_throw);
        let saved_pnames = std::mem::take(&mut self.current_fn_param_names);
        let saved_params = std::mem::take(&mut self.current_fn_params);

        self.push_scope();
        let params_sig = self.emit_swift_params(f, f.offset, true);
        let throws_kw = if can_throw { " throws" } else { "" };
        let (discardable, ret_clause) = if ret_hir == HirType::Void {
            ("", String::new())
        } else {
            let opt_suf = if ret_opt { "?" } else { "" };
            (
                "@discardableResult ",
                format!(" -> {}{opt_suf}", swift_type_for_hir(&ret_hir)),
            )
        };

        self.indent = 1;
        let body = self.emit_function_body(&f.body);
        self.indent = 0;
        self.pop_scope();
        self.in_key_handler_event = saved_ke;
        self.top_level_is_symbol_frame = saved_top;
        self.in_symbol_frame_method = saved_in_frame;
        self.current_return_type = saved_ret;
        self.current_return_is_optional = saved_ret_opt;
        self.in_throwing_fn = saved_throw;
        self.current_fn_param_names = saved_pnames;
        self.current_fn_params = saved_params;

        if body.trim().is_empty() {
            format!(
                "{discardable}@MainActor public func {fname}({params_sig}){throws_kw}{ret_clause} {{}}"
            )
        } else {
            format!(
                "{discardable}@MainActor public func {fname}({params_sig}){throws_kw}{ret_clause} {{\n{body}\n}}"
            )
        }
    }

    fn emit_swift_params(&mut self, f: &Function, offset: usize, allow_defaults: bool) -> String {
        let mut out = Vec::new();
        self.current_fn_param_names.clear();
        self.current_fn_params.clear();
        for (i, (raw_p, pty)) in f.params.iter().enumerate() {
            let pname = escape_swift_ident(raw_p);
            self.current_fn_param_names.push(pname.clone());
            let hir_ty = pty
                .as_deref()
                .map_or(HirType::Unknown, HirType::from_annotation);
            self.current_fn_params.push((raw_p.clone(), hir_ty.clone()));
            let swift_ty = swift_type_for_hir(&hir_ty);
            self.mark_local(raw_p);
            self.env.define(raw_p.clone(), hir_ty.clone());
            if allow_defaults && let Some(Some(def)) = f.defaults.get(i) {
                let dc = self.emit_coerced_expr(def, &hir_ty, offset);
                out.push(format!("_ {pname}: {swift_ty} = {dc}"));
            } else {
                out.push(format!("_ {pname}: {swift_ty}"));
            }
        }
        if let Some(rest) = &f.rest {
            let rname = escape_swift_ident(rest);
            self.mark_local(rest);
            self.env
                .define(rest.clone(), HirType::Vector(Box::new(HirType::Unknown)));
            out.push(format!("_ {rname}: NFValue..."));
        }
        out.join(", ")
    }

    fn emit_stmt_list(&mut self, stmts: &[Stmt]) -> String {
        let mut lines = Vec::new();
        for stmt in stmts {
            let s = self.emit_stmt(stmt);
            if !s.trim().is_empty() {
                lines.push(s);
            }
        }
        lines.join("\n")
    }

    fn emit_stmt(&mut self, stmt: &Stmt) -> String {
        let offset = match stmt {
            Stmt::Located(off, _) => *off,
            _ => 0,
        };
        let pad = self.pad();
        match stmt.unlocated() {
            Stmt::Located(_, inner) => self.emit_stmt(inner),
            Stmt::Empty => String::new(),
            Stmt::Block(body) => {
                let saved_top = self.top_level_is_symbol_frame;
                self.top_level_is_symbol_frame = false;
                self.push_scope();
                self.indent += 1;
                let inner = self.emit_stmt_list(body);
                self.indent -= 1;
                self.pop_scope();
                self.top_level_is_symbol_frame = saved_top;
                format!("{pad}do {{\n{inner}\n{pad}}}")
            }
            Stmt::Var(vars) => {
                let mut out = Vec::new();
                for (raw_name, raw_ty, val_opt) in vars {
                    // Any timeline frame `var` declaration (including inside `if`/`for` blocks) was hoisted to `self.<prop>`!
                    if self.in_symbol_frame_method
                        && self.instance_props.contains_key(raw_name)
                        && !self.is_local(raw_name)
                    {
                        if let Some(val) = val_opt {
                            let target_ty = self
                                .instance_props
                                .get(raw_name)
                                .cloned()
                                .unwrap_or(HirType::Unknown);
                            let escaped = escape_swift_ident(raw_name);
                            if !self.source_is_as3
                                && let Some(clip_init) = self.try_emit_dynamic_clip_swift(
                                    &format!("self.{escaped}"),
                                    val,
                                    offset,
                                )
                            {
                                out.push(clip_init);
                                continue;
                            }
                            let rhs = self.emit_coerced_property_init(val, &target_ty, offset);
                            let self_lhs = format!("self.{escaped}");
                            if rhs != self_lhs {
                                out.push(format!("{pad}{self_lhs} = {rhs}"));
                            }
                        }
                        continue;
                    }

                    let vname = escape_swift_ident(raw_name);
                    if !self.source_is_as3
                        && let Some(val) = val_opt
                        && let Some(clip_init) =
                            self.try_emit_dynamic_clip_swift(&format!("let {vname}"), val, offset)
                    {
                        self.mark_local(raw_name);
                        self.env.define(raw_name.clone(), HirType::Sprite);
                        out.push(clip_init);
                        continue;
                    }

                    let raw_decl_ty = raw_ty
                        .as_deref()
                        .map(HirType::from_annotation)
                        .unwrap_or_else(|| {
                            val_opt.as_ref().map_or(HirType::Unknown, |e| self.infer(e))
                        });
                    let already = self.is_local(raw_name);
                    let same_scope_redecl = self
                        .local_usage_stack
                        .last()
                        .is_some_and(|u| u.same_scope_redeclared.contains(raw_name));
                    let decl_ty = if already {
                        self.env.lookup(raw_name).cloned().unwrap_or(raw_decl_ty)
                    } else if same_scope_redecl && raw_ty.is_none() {
                        HirType::Unknown
                    } else {
                        raw_decl_ty
                    };

                    if let Some(val) = val_opt {
                        if !already && matches!(decl_ty, HirType::Vector(_)) {
                            let vec_prop = match val {
                                Expr::Member(b, p)
                                    if matches!(b.as_ref(), Expr::Name(n) if n == "this")
                                        && self.instance_props.contains_key(p)
                                        && !self.is_nullable_prop(p) =>
                                {
                                    Some(p.as_str())
                                }
                                Expr::Name(p)
                                    if !self.is_local(p)
                                        && self.instance_props.contains_key(p)
                                        && !self.is_nullable_prop(p) =>
                                {
                                    Some(p.as_str())
                                }
                                _ => None,
                            };
                            if let Some(prop_name) = vec_prop {
                                let usage_mutates_elems =
                                    self.local_usage_stack.last().is_some_and(|u| {
                                        !u.direct_mutated.contains(raw_name)
                                            && !u.same_scope_redeclared.contains(raw_name)
                                            && !u.mutated_this_props.contains(prop_name)
                                            && !u.direct_mutated.contains(prop_name)
                                            && u.member_or_method_mutated.contains(raw_name)
                                    });
                                if usage_mutates_elems {
                                    self.mark_local(raw_name);
                                    self.env.define(raw_name.clone(), decl_ty.clone());
                                    self.insert_vector_alias(
                                        raw_name.clone(),
                                        format!("self.{}", escape_swift_ident(prop_name)),
                                    );
                                    continue;
                                }
                            }
                        }
                        if !already {
                            self.mark_local(raw_name);
                            self.env.define(raw_name.clone(), decl_ty.clone());
                        }
                        let is_nullable_class = !already
                            && matches!(
                                decl_ty,
                                HirType::TextField
                                    | HirType::MovieClip
                                    | HirType::Sprite
                                    | HirType::Button
                                    | HirType::Class(_)
                            )
                            && (is_null_or_undef(val) || self.expr_produces_optional(val));
                        if !already && is_nullable_class {
                            self.mark_nullable_local(raw_name);
                        }
                        let swift_ty = if is_nullable_class {
                            format!("{}?", swift_type_for_hir(&decl_ty))
                        } else {
                            swift_type_for_hir(&decl_ty)
                        };
                        let rhs =
                            if is_nullable_class || (already && self.is_nullable_local(raw_name)) {
                                self.emit_coerced_property_init(val, &decl_ty, offset)
                            } else {
                                self.emit_coerced_expr(val, &decl_ty, offset)
                            };
                        if already {
                            if vname != rhs {
                                out.push(format!("{pad}{vname} = {rhs}"));
                            }
                        } else {
                            let (emit_var, is_read) =
                                self.should_emit_local_as_var(raw_name, &decl_ty);
                            if emit_var {
                                out.push(format!("{pad}var {vname}: {swift_ty} = {rhs}"));
                            } else {
                                out.push(format!("{pad}let {vname}: {swift_ty} = {rhs}"));
                            }
                            if !is_read
                                || matches!(
                                    raw_name.as_str(),
                                    "root" | "_root" | "_level0" | "stage" | "_stage"
                                )
                            {
                                out.push(format!("{pad}_ = {vname}"));
                            }
                        }
                    } else if !already {
                        self.mark_local(raw_name);
                        self.env.define(raw_name.clone(), decl_ty.clone());
                        let (swift_ty, def_val) = swift_property_type_and_default(&decl_ty);
                        if swift_ty.ends_with('?') {
                            self.mark_nullable_local(raw_name);
                        }
                        let (emit_var, is_read) = self.should_emit_local_as_var(raw_name, &decl_ty);
                        if emit_var {
                            out.push(format!("{pad}var {vname}: {swift_ty} = {def_val}"));
                            if !is_read
                                || matches!(
                                    raw_name.as_str(),
                                    "root" | "_root" | "_level0" | "stage" | "_stage"
                                )
                            {
                                out.push(format!("{pad}_ = {vname}"));
                            }
                        } else {
                            out.push(format!("{pad}let {vname}: {swift_ty} = {def_val}"));
                            out.push(format!("{pad}_ = {vname}"));
                        }
                    }
                }
                out.join("\n")
            }
            Stmt::Function(f) => {
                // Top-level timeline functions are already emitted in `helper_methods_swift`
                if self.top_level_is_symbol_frame && f.name.is_some() {
                    return String::new();
                }
                let raw_name = f.name.as_deref().unwrap_or("_localFn");
                let fname = escape_swift_ident(raw_name);
                self.mark_local(raw_name);
                let param_tys: Vec<HirType> = f
                    .params
                    .iter()
                    .map(|(_, t)| {
                        t.as_deref()
                            .map_or(HirType::Unknown, HirType::from_annotation)
                    })
                    .collect();
                let ret_hir = f
                    .returns
                    .as_deref()
                    .map_or(HirType::Void, HirType::from_annotation);
                self.env
                    .functions
                    .insert(raw_name.to_owned(), ret_hir.clone());
                self.local_func_sigs.insert(
                    raw_name.to_owned(),
                    SwiftCallableSig {
                        params: param_tys,
                        return_ty: ret_hir.clone(),
                        return_is_optional: false,
                        can_throw: false,
                    },
                );
                let saved_top = self.top_level_is_symbol_frame;
                let saved_in_frame = self.in_symbol_frame_method;
                let saved_closure = self.in_nfvalue_closure;
                let saved_pnames = std::mem::take(&mut self.current_fn_param_names);
                let saved_params = std::mem::take(&mut self.current_fn_params);
                self.top_level_is_symbol_frame = false;
                self.in_symbol_frame_method = false;
                self.in_nfvalue_closure = false;
                let saved_ret = std::mem::replace(&mut self.current_return_type, ret_hir.clone());
                self.push_scope();
                let params = self.emit_swift_params(f, offset, true);
                let ret_str = if ret_hir == HirType::Void {
                    String::new()
                } else {
                    format!(" -> {}", swift_type_for_hir(&ret_hir))
                };
                self.indent += 1;
                let body = self.emit_function_body(&f.body);
                self.indent -= 1;
                self.pop_scope();
                self.current_return_type = saved_ret;
                self.top_level_is_symbol_frame = saved_top;
                self.in_symbol_frame_method = saved_in_frame;
                self.in_nfvalue_closure = saved_closure;
                self.current_fn_param_names = saved_pnames;
                self.current_fn_params = saved_params;
                format!("{pad}func {fname}({params}){ret_str} {{\n{body}\n{pad}}}")
            }
            Stmt::Expr(expr) => {
                if let Some(special) = self.try_emit_special_swift_stmt(expr, offset) {
                    return special;
                }
                let s = self.emit_expr_with_offset(expr, offset);
                let needs_discard =
                    !matches!(expr, Expr::Assign(..) | Expr::Update(..) | Expr::Delete(..))
                        && ((s.starts_with('(') && s.contains(" ?? "))
                            || matches!(
                                expr,
                                Expr::Binary(..)
                                    | Expr::Conditional(..)
                                    | Expr::Unary(..)
                                    | Expr::Name(_)
                                    | Expr::Literal(_)
                                    | Expr::Member(..)
                                    | Expr::Index(..)
                                    | Expr::Array(..)
                                    | Expr::Object(..)
                                    | Expr::Function(..)
                            ));
                if needs_discard {
                    return format!("{pad}_ = {s}");
                }
                format!("{pad}{s}")
            }
            Stmt::If(cond, yes, no) => {
                let c = self.emit_bool_cond(cond, offset);
                let yb = self.emit_swift_branch(yes);
                if let Some(no_stmt) = no {
                    if matches!(no_stmt.unlocated(), Stmt::If(_, _, _)) {
                        let nested = self.emit_stmt(no_stmt);
                        format!("{pad}if {c} {yb} else {}", nested.trim_start())
                    } else {
                        let nb = self.emit_swift_branch(no_stmt);
                        format!("{pad}if {c} {yb} else {nb}")
                    }
                } else {
                    format!("{pad}if {c} {yb}")
                }
            }
            Stmt::While(cond, body) => {
                let lbl_prefix = self
                    .pending_loop_label
                    .take()
                    .map(|l| format!("{}: ", escape_swift_ident(&l)))
                    .unwrap_or_default();
                let c = self.emit_bool_cond(cond, offset);
                self.for_update_stack.push((None, None));
                let b = self.emit_swift_branch(body);
                self.for_update_stack.pop();
                format!("{pad}{lbl_prefix}while {c} {b}")
            }
            Stmt::DoWhile(body, cond) => {
                let lbl_prefix = self
                    .pending_loop_label
                    .take()
                    .map(|l| format!("{}: ", escape_swift_ident(&l)))
                    .unwrap_or_default();
                self.for_update_stack.push((None, None));
                let b = self.emit_swift_branch(body);
                self.for_update_stack.pop();
                let c = self.emit_bool_cond(cond, offset);
                format!("{pad}{lbl_prefix}repeat {b} while {c}")
            }
            Stmt::For(init, cond, update, body) => {
                // Swift 6 removed C-style `for (init; cond; update)` -> lower deterministically to `while`,
                // running `update` before every `continue` targeting this loop!
                let loop_lbl = self.pending_loop_label.take();
                let lbl_prefix = loop_lbl
                    .as_ref()
                    .map(|l| format!("{}: ", escape_swift_ident(l)))
                    .unwrap_or_default();
                let saved_top = self.top_level_is_symbol_frame;
                self.top_level_is_symbol_frame = false;
                let wrap_in_do = matches!(
                    init.unlocated(),
                    Stmt::Var(vars) if vars.iter().any(|(n, _, _)| !self.is_local(n))
                );
                if wrap_in_do {
                    self.indent += 1;
                }
                let loop_pad = self.pad();
                self.push_scope();
                let mut lines = Vec::new();
                if !matches!(init.unlocated(), Stmt::Empty) {
                    let init_s = self.emit_stmt(init);
                    if !init_s.trim().is_empty() {
                        lines.push(init_s);
                    }
                }
                let for_scope_depth = self.local_scopes.len();
                let mut for_vars = Vec::new();
                match init.unlocated() {
                    Stmt::Var(vars) => {
                        for (n, _, _) in vars {
                            if self.is_local(n) && self.lookup_vector_alias(n).is_none() {
                                for_vars.push(n.clone());
                            }
                        }
                    }
                    Stmt::Expr(Expr::Assign(_, lhs, _)) => {
                        if let Expr::Name(n) = lhs.as_ref()
                            && self.is_local(n)
                            && self.lookup_vector_alias(n).is_none()
                        {
                            for_vars.push(n.clone());
                        }
                    }
                    _ => {}
                }
                let cond_s = cond
                    .as_ref()
                    .map(|c| self.emit_bool_cond(c, offset))
                    .unwrap_or_else(|| "true".into());
                self.indent += 1;
                let inner_pad = self.pad();
                self.push_scope();
                self.active_for_loop_vars.push((for_scope_depth, for_vars));
                self.for_update_stack.push((loop_lbl, update.clone()));
                let body_s = match body.unlocated() {
                    Stmt::Block(b) => self.emit_stmt_list(b),
                    other => self.emit_stmt(other),
                };
                self.for_update_stack.pop();
                self.active_for_loop_vars.pop();
                self.pop_scope();
                let mut loop_parts = Vec::new();
                if !body_s.trim().is_empty() {
                    loop_parts.push(body_s);
                }
                if let Some(upd) = update {
                    let u = self.emit_stmt(&Stmt::Expr(upd.clone()));
                    if !u.trim().is_empty() {
                        loop_parts.push(format!("{inner_pad}{}", u.trim_start()));
                    }
                }
                self.indent -= 1;
                self.pop_scope();
                self.top_level_is_symbol_frame = saved_top;

                lines.push(format!(
                    "{loop_pad}{lbl_prefix}while {cond_s} {{\n{}\n{loop_pad}}}",
                    loop_parts.join("\n")
                ));
                if wrap_in_do {
                    self.indent -= 1;
                    format!("{pad}do {{\n{}\n{pad}}}", lines.join("\n"))
                } else {
                    lines.join("\n")
                }
            }
            Stmt::ForIn(init, coll, is_each, body) => {
                let saved_top = self.top_level_is_symbol_frame;
                self.top_level_is_symbol_frame = false;
                let coll_ty = self.infer(coll);
                let coll_raw = self.emit_expr_with_offset(coll, offset);
                let is_xml_coll = matches!(
                    &coll_ty,
                    HirType::Class(cn) if matches!(short_type_name(cn), "XML" | "XMLList")
                );
                let coll_s = if self.expr_produces_optional(coll) && !is_xml_coll {
                    if Self::is_nfvalue_type(&coll_ty) {
                        format!("({coll_raw} ?? .undefinedVal)")
                    } else {
                        format!("({coll_raw} ?? [])")
                    }
                } else {
                    coll_raw
                };

                self.push_scope();
                let loop_lbl = self.pending_loop_label.take();
                let lbl_prefix = loop_lbl
                    .as_ref()
                    .map(|l| format!("{}: ", escape_swift_ident(l)))
                    .unwrap_or_default();
                self.for_update_stack.push((loop_lbl, None));
                let (raw_var_name, explicit_var_ty) = match init.unlocated() {
                    Stmt::Var(vars) if !vars.is_empty() => (
                        vars[0].0.clone(),
                        vars[0].1.as_deref().map(HirType::from_annotation),
                    ),
                    Stmt::Expr(Expr::Name(n)) => (n.clone(), None),
                    _ => ("_item".into(), None),
                };
                let var_name = escape_swift_ident(&raw_var_name);
                self.mark_local(&raw_var_name);

                let result = if *is_each {
                    let iter_coll_s = if matches!(coll_ty, HirType::Vector(_)) {
                        coll_s.clone()
                    } else {
                        format!("NFRuntime.values({coll_s})")
                    };
                    let yielded_ty = match &coll_ty {
                        HirType::Vector(elem) => (**elem).clone(),
                        HirType::Class(cn) if matches!(short_type_name(cn), "XML" | "XMLList") => {
                            HirType::Class("XML".into())
                        }
                        _ => HirType::Unknown,
                    };
                    let target_var_ty = explicit_var_ty.unwrap_or_else(|| yielded_ty.clone());
                    let needs_coercion_bind = Self::is_nfvalue_or_unknown(&yielded_ty)
                        && !Self::is_nfvalue_or_unknown(&target_var_ty);
                    self.env.define(raw_var_name.clone(), target_var_ty.clone());

                    if needs_coercion_bind {
                        let (emit_var, is_read) =
                            self.should_emit_local_as_var(&raw_var_name, &target_var_ty);
                        self.indent += 1;
                        let in_pad = self.pad();
                        let sw_ty = swift_type_for_hir(&target_var_ty);
                        let conv = match &target_var_ty {
                            HirType::Float => "Double(_nfIterItem)".to_owned(),
                            HirType::Int => "Int(_nfIterItem)".to_owned(),
                            HirType::UInt => "UInt(_nfIterItem)".to_owned(),
                            HirType::Bool => "_nfIterItem.isTruthy".to_owned(),
                            HirType::String => "String(_nfIterItem)".to_owned(),
                            _ => "_nfIterItem".to_owned(),
                        };
                        let inner_body = match body.unlocated() {
                            Stmt::Block(b) => self.emit_stmt_list(b),
                            other => self.emit_stmt(other),
                        };
                        self.indent -= 1;
                        if inner_body.trim().is_empty() || (!emit_var && !is_read) {
                            let tail = if inner_body.trim().is_empty() {
                                String::new()
                            } else {
                                format!("\n{inner_body}")
                            };
                            format!(
                                "{pad}{lbl_prefix}for _nfIterItem in {iter_coll_s} {{\n{in_pad}let {var_name}: {sw_ty} = {conv}\n{in_pad}_ = {var_name}{tail}\n{pad}}}"
                            )
                        } else {
                            let kw = if emit_var { "var" } else { "let" };
                            format!(
                                "{pad}{lbl_prefix}for _nfIterItem in {iter_coll_s} {{\n{in_pad}{kw} {var_name}: {sw_ty} = {conv}\n{inner_body}\n{pad}}}"
                            )
                        }
                    } else {
                        let b = self.emit_swift_branch(body);
                        format!("{pad}{lbl_prefix}for {var_name} in {iter_coll_s} {b}")
                    }
                } else {
                    self.env.define(raw_var_name, HirType::String);
                    let b = self.emit_swift_branch(body);
                    format!("{pad}{lbl_prefix}for {var_name} in NFRuntime.keys({coll_s}) {b}")
                };

                self.for_update_stack.pop();
                self.pop_scope();
                self.top_level_is_symbol_frame = saved_top;
                result
            }
            Stmt::Switch(disc, cases) => {
                let disc_ty = self.infer(disc);
                let d_raw = self.emit_expr_with_offset(disc, offset);
                let d = if self.expr_produces_optional(disc) {
                    format!("({d_raw} ?? {})", swift_nonoptional_default_expr(&disc_ty))
                } else {
                    d_raw
                };
                let mut lines = vec![format!("{pad}switch {d} {{")];
                let mut has_default = false;
                let total_cases = cases.len();

                for (case_idx, Case { test, body }) in cases.iter().enumerate() {
                    if let Some(t) = test {
                        let pat = self.emit_coerced_expr(t, &disc_ty, offset);
                        lines.push(format!("{pad}case {pat}:"));
                    } else {
                        has_default = true;
                        lines.push(format!("{pad}default:"));
                    }
                    self.push_scope();
                    self.indent += 1;
                    let bpad = self.pad();

                    if body.is_empty() {
                        // Empty `case A:` before another case in AS2/AS3 falls through!
                        if case_idx + 1 < total_cases {
                            lines.push(format!("{bpad}fallthrough"));
                        } else {
                            lines.push(format!("{bpad}break"));
                        }
                    } else {
                        let ends_with_break = body
                            .last()
                            .is_some_and(|s| matches!(s.unlocated(), Stmt::Break(None)));
                        let ends_with_control = body.last().is_some_and(|s| {
                            matches!(
                                s.unlocated(),
                                Stmt::Break(_)
                                    | Stmt::Continue(_)
                                    | Stmt::Return(_, _)
                                    | Stmt::Throw(_)
                            )
                        });
                        let slice_len = if ends_with_break {
                            body.len() - 1
                        } else {
                            body.len()
                        };
                        let mut emitted_any = false;
                        for s in &body[..slice_len] {
                            let rs = self.emit_stmt(s);
                            if !rs.trim().is_empty() {
                                lines.push(rs);
                                emitted_any = true;
                            }
                        }
                        if !ends_with_control && case_idx + 1 < total_cases {
                            lines.push(format!("{bpad}fallthrough"));
                        } else if !emitted_any {
                            lines.push(format!("{bpad}break"));
                        }
                    }
                    self.indent -= 1;
                    self.pop_scope();
                }
                if !has_default {
                    lines.push(format!("{pad}default:\n{pad}    break"));
                }
                lines.push(format!("{pad}}}"));
                lines.join("\n")
            }
            Stmt::Break(lbl) => match lbl {
                Some(l) => format!("{pad}break {}", escape_swift_ident(l)),
                None => format!("{pad}break"),
            },
            Stmt::Continue(lbl) => {
                let matching_upd = match lbl {
                    Some(target_lbl) => self
                        .for_update_stack
                        .iter()
                        .rev()
                        .find(|(loop_lbl, _)| loop_lbl.as_deref() == Some(target_lbl.as_str()))
                        .and_then(|(_, upd)| upd.clone()),
                    None => self
                        .for_update_stack
                        .last()
                        .and_then(|(_, upd)| upd.clone()),
                };
                let cont_stmt = match lbl {
                    Some(l) => format!("{pad}continue {}", escape_swift_ident(l)),
                    None => format!("{pad}continue"),
                };
                if let Some(upd_expr) = matching_upd {
                    let upd_s = self.emit_stmt(&Stmt::Expr(upd_expr));
                    if !upd_s.trim().is_empty() {
                        return format!("{upd_s}\n{cont_stmt}");
                    }
                }
                cont_stmt
            }
            Stmt::Return(val, _) => {
                if self.in_nfvalue_closure {
                    return match val {
                        Some(v) => format!("{pad}return {}", self.wrap_nfvalue(v, offset)),
                        None => format!("{pad}return NFValue.undefinedVal"),
                    };
                }
                match val {
                    Some(v) if self.current_return_type != HirType::Void => {
                        let rty = self.current_return_type.clone();
                        if self.current_return_is_optional {
                            format!(
                                "{pad}return {}",
                                self.emit_coerced_property_init(v, &rty, offset)
                            )
                        } else {
                            format!("{pad}return {}", self.emit_coerced_expr(v, &rty, offset))
                        }
                    }
                    Some(v) => {
                        if is_null_or_undef(v) {
                            format!("{pad}return")
                        } else {
                            let side_fx = self.emit_expr_with_offset(v, offset);
                            format!("{pad}_ = {side_fx}\n{pad}return")
                        }
                    }
                    None => format!("{pad}return"),
                }
            }
            Stmt::Throw(val) => {
                let expr_sw = self.emit_expr_with_offset(val, offset);
                if self.in_try_depth > 0 || self.in_throwing_fn {
                    format!("{pad}try NFRuntime.throwScriptError({expr_sw})")
                } else {
                    format!("{pad}NFRuntime.throwError({expr_sw})")
                }
            }
            Stmt::Try(body, catches, fin) => {
                let saved_top = self.top_level_is_symbol_frame;
                self.top_level_is_symbol_frame = false;
                let mut out = Vec::new();
                if let Some(fin_body) = fin {
                    self.push_scope();
                    self.indent += 1;
                    let fb = self.emit_stmt_list(fin_body);
                    self.indent -= 1;
                    self.pop_scope();
                    out.push(format!("{pad}defer {{\n{fb}\n{pad}}}"));
                }

                if let Some(CatchClause {
                    name, body: cbody, ..
                }) = catches.first()
                {
                    self.in_try_depth += 1;
                    self.push_scope();
                    self.indent += 1;
                    let try_pad = self.pad();
                    let b = self.emit_stmt_list(body);
                    self.indent -= 1;
                    self.pop_scope();
                    self.in_try_depth -= 1;

                    let cname = escape_swift_ident(name);
                    self.push_scope();
                    self.mark_local(name);
                    self.env
                        .define(name.clone(), HirType::Class("Error".into()));
                    self.indent += 1;
                    let catch_pad = self.pad();
                    let cb = self.emit_stmt_list(cbody);
                    self.indent -= 1;
                    self.pop_scope();

                    let do_inner = if b.trim().is_empty() {
                        format!("{try_pad}try NFRuntime.checkTryScope()")
                    } else {
                        format!("{try_pad}try NFRuntime.checkTryScope()\n{b}")
                    };
                    let catch_bind = format!(
                        "{catch_pad}let {cname} = (_nfCaught as? NFScriptError) ?? NFScriptError(String(describing: _nfCaught))\n\
                         {catch_pad}_ = {cname}"
                    );
                    let catch_inner = if cb.trim().is_empty() {
                        catch_bind
                    } else {
                        format!("{catch_bind}\n{cb}")
                    };
                    out.push(format!(
                        "{pad}do {{\n{do_inner}\n{pad}}} catch let _nfCaught {{\n{catch_inner}\n{pad}}}"
                    ));
                } else {
                    self.push_scope();
                    self.indent += 1;
                    let b = self.emit_stmt_list(body);
                    self.indent -= 1;
                    self.pop_scope();
                    out.push(format!("{pad}do {{\n{b}\n{pad}}}"));
                }
                self.top_level_is_symbol_frame = saved_top;
                out.join("\n")
            }
            Stmt::Label(lbl, inner) => {
                if matches!(
                    inner.unlocated(),
                    Stmt::For(_, _, _, _)
                        | Stmt::While(_, _)
                        | Stmt::DoWhile(_, _)
                        | Stmt::ForIn(_, _, _, _)
                ) {
                    self.pending_loop_label = Some(lbl.clone());
                    return self.emit_stmt(inner);
                }
                let s = self.emit_stmt(inner);
                format!("{pad}{}: {}", escape_swift_ident(lbl), s.trim_start())
            }
            Stmt::With(target, body, _) | Stmt::TellTarget(target, body, _) => {
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    "Dynamic 'with' / 'tellTarget' scope requires explicit receiver references in Swift 6.",
                );
                let tgt =
                    super::escape_block_comment_body(&self.emit_expr_with_offset(target, offset));
                let b = self.emit_swift_branch(body);
                format!("{pad}/* FIXME [NeoFlash Swift]: with ({tgt}) */ do {b}")
            }
            Stmt::DefaultXmlNamespace(_) => {
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    "E4X 'default xml namespace' is not supported on the native Swift target.",
                );
                format!("{pad}/* FIXME [NeoFlash Swift]: default xml namespace */")
            }
            Stmt::IfFrameLoaded(_, body) => self.emit_stmt(body),
            Stmt::InitClip(_, body, _) => self.emit_stmt_list(body),
            Stmt::LegacyEvent(_, _) => String::new(),
        }
    }

    fn emit_swift_branch(&mut self, stmt: &Stmt) -> String {
        let saved_top = self.top_level_is_symbol_frame;
        self.top_level_is_symbol_frame = false;
        self.push_scope();
        self.indent += 1;
        let inner = match stmt.unlocated() {
            Stmt::Block(b) => self.emit_stmt_list(b),
            other => self.emit_stmt(other),
        };
        self.indent -= 1;
        self.pop_scope();
        self.top_level_is_symbol_frame = saved_top;
        let pad = self.pad();
        if inner.trim().is_empty() {
            "{}".into()
        } else {
            format!("{{\n{inner}\n{pad}}}")
        }
    }

    fn try_emit_dynamic_clip_swift(
        &mut self,
        lhs_binding: &str,
        val: &Expr,
        offset: usize,
    ) -> Option<String> {
        let Expr::Call(callee, args, _) = val else {
            return None;
        };
        let pad = self.pad();
        if let Some(parent_expr) = match_clip_factory(callee, "createEmptyMovieClip")
            && !args.is_empty()
        {
            let parent_sw = self.emit_optional_chain_base(&parent_expr, offset);
            let name_sw = self.emit_coerced_expr(&args[0], &HirType::String, offset);
            let var_ref = lhs_binding.strip_prefix("let ").unwrap_or(lhs_binding);
            return Some(format!(
                "{pad}{lhs_binding} = NFMovieClip(name: {name_sw})\n\
                 {pad}{parent_sw}.addChild({var_ref})"
            ));
        }
        None
    }

    fn try_emit_special_swift_stmt(&mut self, expr: &Expr, offset: usize) -> Option<String> {
        let pad = self.pad();

        // 1. Update statements (`i++` / `++i` -> `i &+= 1` on Int, 32-bit masked on UInt, `mc._alpha++` -> `mc.alpha += 0.01`)
        if let Expr::Update(op, target, _) = expr {
            if !self.source_is_as3
                && let Some((base_sw, modern)) = self.match_as2_percent_lvalue_swift(target, offset)
            {
                let flt_op = if op == "++" { "+=" } else { "-=" };
                return Some(format!("{pad}{base_sw}.{modern} {flt_op} 0.01"));
            }
            let target_ty = self.infer(target);
            let is_int = target_ty.is_integer() && !self.lvalue_expects_nfvalue(target);
            let t = self.emit_lvalue_swift(target, offset);
            if is_int && target_ty == HirType::UInt {
                let wop = if op == "++" { "&+" } else { "&-" };
                return Some(format!("{pad}{t} = ({t} {wop} 1) & 0xFFFF_FFFF"));
            }
            let assign_op = match (op.as_str(), is_int) {
                ("++", true) => "&+=",
                ("++", false) => "+=",
                (_, true) => "&-=",
                (_, false) => "-=",
            };
            return Some(format!("{pad}{t} {assign_op} 1"));
        }

        // 2. AS3 `stage.addEventListener(...)` / `addEventListener(...)`
        if let Expr::Call(callee, args, call_off) = expr
            && let Some(listener_stmt) = self.try_emit_add_event_listener(callee, args, *call_off)
        {
            return Some(listener_stmt);
        }

        // 3. AS1/AS2 `this.onEnterFrame = ...`, `this.onKeyDown = ...`, `onResize = ...`
        if !self.source_is_as3
            && let Expr::Assign(op, lhs, rhs) = expr
            && op == "="
            && let Some((target_expr, cb_prop)) = extract_as2_callback_target(lhs)
        {
            match cb_prop {
                "onEnterFrame" => {
                    let tgt = self.emit_listener_receiver(&target_expr, offset);
                    let closure = self.emit_weak_self_closure(rhs, false, offset);
                    return Some(format!(
                        "{pad}{tgt}.addEventListener(\"enterFrame\") {closure}"
                    ));
                }
                "onRelease" | "onPress" | "onReleaseOutside" | "onRollOver" | "onRollOut"
                | "onMouseDown" | "onMouseUp" | "onMouseMove" | "onChanged" => {
                    let ev = match cb_prop {
                        "onRelease" => "click",
                        "onPress" | "onMouseDown" => "mouseDown",
                        "onMouseUp" => "mouseUp",
                        "onMouseMove" => "mouseMove",
                        "onReleaseOutside" => "releaseOutside",
                        "onRollOver" => "rollOver",
                        "onChanged" => "change",
                        _ => "rollOut",
                    };
                    let tgt = self.emit_listener_receiver(&target_expr, offset);
                    let closure = self.emit_weak_self_closure(rhs, false, offset);
                    return Some(format!("{pad}{tgt}.addEventListener(\"{ev}\") {closure}"));
                }
                "onKeyDown" | "onKeyUp" => {
                    let is_down = cb_prop == "onKeyDown";
                    let key = listener_target_key(&target_expr).unwrap_or_else(|| "this".into());
                    let has_explicit = self.explicit_key_listeners.contains(&key);
                    if is_down {
                        self.key_down_handlers.insert(key, (**rhs).clone());
                    } else {
                        self.key_up_handlers.insert(key, (**rhs).clone());
                    }
                    if has_explicit {
                        return Some(String::new());
                    }
                    let ev = if is_down { "keyDown" } else { "keyUp" };
                    let closure = self.emit_weak_self_closure(rhs, true, offset);
                    return Some(format!(
                        "{pad}self.stage?.addEventListener(\"{ev}\") {closure}"
                    ));
                }
                "onResize" => {
                    let key = listener_target_key(&target_expr).unwrap_or_else(|| "this".into());
                    let has_explicit = self.explicit_stage_listeners.contains(&key);
                    self.resize_handlers.insert(key, (**rhs).clone());
                    if has_explicit {
                        return Some(String::new());
                    }
                    let closure = self.emit_weak_self_closure(rhs, false, offset);
                    return Some(format!(
                        "{pad}self.stage?.addEventListener(\"resize\") {closure}"
                    ));
                }
                _ => {}
            }
        }

        // 4. AS2 `Key.addListener(obj)` & `Stage.addListener(obj)`
        if !self.source_is_as3
            && let Expr::Call(callee, args, _) = expr
            && args.len() == 1
            && let Expr::Member(base, method) = callee.as_ref()
            && method == "addListener"
            && let Expr::Name(mgr) = base.as_ref()
        {
            let key = listener_target_key(&args[0]).unwrap_or_else(|| "this".into());
            if mgr == "Key" {
                let mut lines = Vec::new();
                if let Some(kd_rhs) = self.key_down_handlers.get(&key).cloned() {
                    let closure = self.emit_weak_self_closure(&kd_rhs, true, offset);
                    lines.push(format!(
                        "{pad}self.stage?.addEventListener(\"keyDown\") {closure}"
                    ));
                }
                if let Some(ku_rhs) = self.key_up_handlers.get(&key).cloned() {
                    let closure = self.emit_weak_self_closure(&ku_rhs, true, offset);
                    lines.push(format!(
                        "{pad}self.stage?.addEventListener(\"keyUp\") {closure}"
                    ));
                }
                return Some(lines.join("\n"));
            }
            if mgr == "Stage"
                && let Some(rsz_rhs) = self.resize_handlers.get(&key).cloned()
            {
                let closure = self.emit_weak_self_closure(&rsz_rhs, false, offset);
                return Some(format!(
                    "{pad}self.stage?.addEventListener(\"resize\") {closure}"
                ));
            }
        }

        // 5. Statement-level Flash-to-Swift proxy calls (`SwiftHost.fn(...)`, `fscommand("fn", ...)`,
        //    `ExternalInterface.call("fn", ...)`) emit clean one-line optional-chained calls without `??`
        if let Expr::Call(callee, args, call_off) = expr {
            if let Some((method, payload_args)) = match_host_proxy_call(callee, args) {
                let (call_str, ret_ty) =
                    self.emit_host_proxy_call_inner(&method, payload_args, *call_off);
                if ret_ty.is_some() {
                    return Some(format!("{pad}_ = {call_str}"));
                }
                return Some(format!("{pad}{call_str}"));
            }
            if matches!(callee.as_ref(), Expr::Name(n) if n == "fscommand" && !self.is_local(n)) {
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    *call_off,
                    "Dynamic 'fscommand(...)' requires a string literal command name to generate a SwiftHostProtocol stub.",
                );
                let raw_args = super::escape_block_comment_body(&self.emit_args(args, *call_off));
                return Some(format!(
                    "{pad}/* FIXME [NeoFlash Swift]: dynamic fscommand({raw_args}) */"
                ));
            }
            if let Expr::Member(base, m) = callee.as_ref()
                && is_external_interface_base(base)
            {
                if m == "call" {
                    self.record_issue(
                        ConversionIssueKind::ManualAttention,
                        *call_off,
                        "Dynamic 'ExternalInterface.call(...)' requires a string literal function name to generate a SwiftHostProtocol stub.",
                    );
                    let raw_args =
                        super::escape_block_comment_body(&self.emit_args(args, *call_off));
                    return Some(format!(
                        "{pad}/* FIXME [NeoFlash Swift]: dynamic ExternalInterface.call({raw_args}) */"
                    ));
                }
                if m == "addCallback" {
                    self.record_issue(
                        ConversionIssueKind::ManualAttention,
                        *call_off,
                        "'ExternalInterface.addCallback' is inbound from host; call public methods on MainScene directly from Swift.",
                    );
                    let raw_args =
                        super::escape_block_comment_body(&self.emit_args(args, *call_off));
                    return Some(format!(
                        "{pad}/* FIXME [NeoFlash Swift]: ExternalInterface.addCallback({raw_args}) */"
                    ));
                }
            }
        }

        None
    }

    fn try_emit_add_event_listener(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        offset: usize,
    ) -> Option<String> {
        if args.len() < 2 {
            return None;
        }
        let receiver_sw = match callee {
            Expr::Name(n) if n == "addEventListener" => "self".to_owned(),
            Expr::Member(base, m) if m == "addEventListener" => {
                self.emit_listener_receiver(base, offset)
            }
            _ => return None,
        };
        let pad = self.pad();
        let ev_str = if let Some(ev) = classify_swift_event_type(&args[0]) {
            format!("\"{ev}\"")
        } else {
            self.emit_expr_with_offset(&args[0], offset)
        };
        let is_key = ev_str == "\"keyDown\"" || ev_str == "\"keyUp\"";
        let closure = self.emit_weak_self_closure(&args[1], is_key, offset);
        Some(format!(
            "{pad}{receiver_sw}.addEventListener({ev_str}) {closure}"
        ))
    }

    fn emit_listener_receiver(&mut self, expr: &Expr, offset: usize) -> String {
        if is_stage_expr(expr) {
            return "self.stage?".into();
        }
        if matches!(expr, Expr::Name(n) if n == "this") {
            return "self".into();
        }
        if let Expr::Name(n) = expr
            && !self.is_local(n)
            && self.instance_props.contains_key(n)
        {
            let esc = escape_swift_ident(n);
            if self.is_nullable_prop(n) {
                return format!("self.{esc}?");
            }
            return format!("self.{esc}");
        }
        if let Expr::Member(b, p) = expr
            && matches!(b.as_ref(), Expr::Name(n) if n == "this")
            && self.instance_props.contains_key(p)
        {
            let esc = escape_swift_ident(p);
            if self.is_nullable_prop(p) {
                return format!("self.{esc}?");
            }
            return format!("self.{esc}");
        }
        self.emit_optional_chain_base(expr, offset)
    }

    fn collect_for_loop_captures(
        &self,
        f: &Function,
        closure_usage: &LocalUsageInfo,
        body_sw: &str,
        extra_sw: &[String],
    ) -> Vec<String> {
        let mut param_set: BTreeSet<&str> = f.params.iter().map(|(n, _)| n.as_str()).collect();
        if let Some(rest) = f.rest.as_deref() {
            param_set.insert(rest);
        }
        let mut seen = BTreeSet::new();
        let mut caps = Vec::new();
        for (depth, vars) in self.active_for_loop_vars.iter().rev() {
            for v in vars {
                if !seen.insert(v.as_str()) {
                    continue;
                }
                if param_set.contains(v.as_str())
                    || closure_usage.var_decl_counts.contains_key(v)
                    || closure_usage.direct_mutated.contains(v)
                {
                    continue;
                }
                if self
                    .local_scopes
                    .get(*depth..)
                    .is_some_and(|scopes| scopes.iter().any(|s| s.contains(v)))
                {
                    continue;
                }
                if let Some(vty) = self.env.lookup(v)
                    && !self.is_swift_reference_class(vty)
                    && !Self::is_nfvalue_type(vty)
                    && closure_usage.member_or_method_mutated.contains(v)
                {
                    continue;
                }
                if !closure_usage.read_names.contains(v) {
                    continue;
                }
                let esc = escape_swift_ident(v);
                if contains_swift_ident(body_sw, &esc)
                    || extra_sw.iter().any(|s| contains_swift_ident(s, &esc))
                {
                    caps.push(esc);
                }
            }
        }
        caps
    }

    /// Emit an ARC-safe `{ [weak self] event in guard let self else { return } ... }` listener closure!
    fn emit_weak_self_closure(
        &mut self,
        rhs: &Expr,
        is_key_handler: bool,
        offset: usize,
    ) -> String {
        let pad = self.pad();
        let use_weak_self = self.has_instance_self();
        if let Expr::Function(f) = rhs {
            let closure_usage = LocalUsageInfo::analyze(&f.body);
            let raw_ev = f
                .params
                .first()
                .map(|(n, _)| n.as_str())
                .unwrap_or("event")
                .to_owned();
            let ev_param = escape_swift_ident(&raw_ev);
            let saved_ke = self.in_key_handler_event.clone();
            if is_key_handler {
                self.in_key_handler_event = Some(ev_param.clone());
            }
            let saved_top = self.top_level_is_symbol_frame;
            let saved_in_frame = self.in_symbol_frame_method;
            let saved_closure = self.in_nfvalue_closure;
            let saved_ret = std::mem::replace(&mut self.current_return_type, HirType::Void);
            self.top_level_is_symbol_frame = false;
            self.in_symbol_frame_method = false;
            self.in_nfvalue_closure = false;
            self.push_scope();
            self.mark_local(&raw_ev);
            self.env
                .define(raw_ev.clone(), HirType::Class("Event".into()));
            self.indent += 1;
            let in_pad = self.pad();
            let body = self.emit_function_body(&f.body);
            self.indent -= 1;
            self.pop_scope();
            self.top_level_is_symbol_frame = saved_top;
            self.in_symbol_frame_method = saved_in_frame;
            self.in_nfvalue_closure = saved_closure;
            self.current_return_type = saved_ret;
            self.in_key_handler_event = saved_ke;
            let loop_caps = self.collect_for_loop_captures(f, &closure_usage, &body, &[]);
            let mut caps = Vec::new();
            if use_weak_self {
                caps.push("weak self".to_owned());
            }
            caps.extend(loop_caps);
            let cap_clause = if caps.is_empty() {
                String::new()
            } else {
                format!("[{}] ", caps.join(", "))
            };
            let uses_self = contains_swift_ident(&body, "self");
            let uses_ev = contains_swift_ident(&body, &ev_param);
            let ev_discard = if uses_ev {
                String::new()
            } else {
                format!("{in_pad}_ = {ev_param}\n")
            };
            if use_weak_self {
                let self_guard = if uses_self {
                    format!("{in_pad}guard let self else {{ return }}")
                } else {
                    format!("{in_pad}guard self != nil else {{ return }}")
                };
                if body.trim().is_empty() {
                    format!(
                        "{{ {cap_clause}({ev_param}: NFEvent) in\n{ev_discard}{self_guard}\n{pad}}}"
                    )
                } else {
                    format!(
                        "{{ {cap_clause}({ev_param}: NFEvent) in\n{ev_discard}{self_guard}\n{body}\n{pad}}}"
                    )
                }
            } else if body.trim().is_empty() {
                format!("{{ {cap_clause}({ev_param}: NFEvent) in\n{in_pad}_ = {ev_param}\n{pad}}}")
            } else {
                format!("{{ {cap_clause}({ev_param}: NFEvent) in\n{ev_discard}{body}\n{pad}}}")
            }
        } else {
            let call_with_event = |emitter: &SwiftEmitter<'_>, fn_name: &str, prefix: &str| {
                let arity = emitter
                    .instance_method_sigs
                    .get(fn_name)
                    .or_else(|| emitter.global_funcs.get(fn_name))
                    .map_or(1, |sig| sig.params.len());
                let esc = escape_swift_ident(fn_name);
                if arity == 0 {
                    format!("{prefix}{esc}()")
                } else {
                    format!("{prefix}{esc}(event)")
                }
            };
            let target_call = match rhs {
                Expr::Name(fn_name) if !self.is_local(fn_name) => {
                    if self.instance_methods.contains(fn_name) {
                        call_with_event(self, fn_name, "self.")
                    } else if self.global_funcs.contains_key(fn_name) {
                        call_with_event(self, fn_name, "")
                    } else {
                        format!("{}(event)", self.emit_expr_with_offset(rhs, offset))
                    }
                }
                Expr::Member(b, m) if matches!(b.as_ref(), Expr::Name(n) if n == "this") => {
                    call_with_event(self, m, "self.")
                }
                other => format!("{}(event)", self.emit_expr_with_offset(other, offset)),
            };
            if use_weak_self {
                format!(
                    "{{ [weak self] (event: NFEvent) in\n{pad}    guard let self else {{ return }}\n{pad}    {target_call}\n{pad}}}"
                )
            } else {
                format!("{{ (event: NFEvent) in\n{pad}    {target_call}\n{pad}}}")
            }
        }
    }

    fn emit_bool_cond(&mut self, cond: &Expr, offset: usize) -> String {
        if is_stage_expr(cond) {
            return "self.stage != nil".into();
        }
        if let Expr::Binary(op, lhs, rhs) = cond
            && matches!(op.as_str(), "&&" | "||" | "and" | "or")
        {
            let sw_op = match op.as_str() {
                "and" => "&&",
                "or" => "||",
                other => other,
            };
            let lb = self.emit_bool_cond(lhs, offset);
            let rb = self.emit_bool_cond(rhs, offset);
            return format!("({lb} {sw_op} {rb})");
        }
        let ty = self.infer(cond);
        let s = self.emit_expr_with_offset(cond, offset);
        if ty == HirType::Bool {
            if self.expr_produces_optional(cond) {
                format!("({s} ?? false)")
            } else {
                s
            }
        } else if matches!(ty, HirType::Object | HirType::Unknown) {
            // Preserve boolean comparisons directly when AST is already a relational/logical expression
            if matches!(
                cond,
                Expr::Binary(op, _, _)
                    if matches!(
                        op.as_str(),
                        "==" | "!=" | "===" | "!==" | "<" | "<=" | ">" | ">=" | "&&" | "||" | "and" | "or"
                    )
            ) || matches!(cond, Expr::Unary(op, _) if op == "!" || op == "not")
            {
                s
            } else {
                format!("NFRuntime.isTruthy({s})")
            }
        } else if matches!(
            ty,
            HirType::Int | HirType::UInt | HirType::Float | HirType::String
        ) {
            format!("NFRuntime.isTruthy({s})")
        } else if ty.is_nullable_object() {
            if self.expr_produces_optional(cond) || has_top_level_optional_chain(&s) {
                format!("({s} != nil)")
            } else {
                format!("withExtendedLifetime({s}, {{ _ in true }})")
            }
        } else {
            s
        }
    }

    fn is_nfvalue_or_unknown(ty: &HirType) -> bool {
        matches!(
            ty,
            HirType::Object
                | HirType::Unknown
                | HirType::Array
                | HirType::Dictionary
                | HirType::Function
        )
    }

    fn is_nfvalue_type(ty: &HirType) -> bool {
        Self::is_nfvalue_or_unknown(ty)
    }

    fn expr_produces_nfvalue(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Array(_) | Expr::Object(_) => true,
            Expr::NewVector(_, _, _) => false,
            Expr::Index(base, _) => match self.infer(base) {
                HirType::Vector(elem) => Self::is_nfvalue_or_unknown(&elem),
                HirType::Class(cn)
                    if matches!(short_type_name(&cn), "ByteArray" | "XML" | "XMLList") =>
                {
                    false
                }
                _ => true,
            },
            Expr::Member(base, prop) => {
                if is_stage_expr(base) {
                    return false;
                }
                if matches!(base.as_ref(), Expr::Name(n) if n == "this") {
                    if let Some(ty) = self.instance_props.get(prop) {
                        return Self::is_nfvalue_type(ty);
                    }
                    return false;
                }
                if let Expr::Name(cls) = base.as_ref()
                    && matches!(
                        cls.as_str(),
                        "Math"
                            | "Key"
                            | "Keyboard"
                            | "Event"
                            | "MouseEvent"
                            | "TouchEvent"
                            | "FocusEvent"
                            | "KeyboardEvent"
                            | "BlendMode"
                            | "TextFieldType"
                            | "StageScaleMode"
                            | "StageAlign"
                            | "Stage"
                    )
                {
                    return false;
                }
                if prop == "length" {
                    return false;
                }
                self.expr_produces_nfvalue(base) || Self::is_nfvalue_type(&self.infer(expr))
            }
            Expr::Name(n) if !self.is_local(n) => {
                if let Some(ty) = self.instance_props.get(n) {
                    return Self::is_nfvalue_type(ty);
                }
                Self::is_nfvalue_type(&self.infer(expr))
            }
            _ => Self::is_nfvalue_type(&self.infer(expr)),
        }
    }

    fn lvalue_expects_nfvalue(&self, lhs: &Expr) -> bool {
        match lhs {
            Expr::Index(base, _) => match self.infer(base) {
                HirType::Vector(elem) => Self::is_nfvalue_or_unknown(&elem),
                HirType::Class(cn)
                    if matches!(short_type_name(&cn), "ByteArray" | "XML" | "XMLList") =>
                {
                    false
                }
                _ => true,
            },
            Expr::Member(base, prop) => {
                if is_stage_expr(base) {
                    return false;
                }
                if matches!(base.as_ref(), Expr::Name(n) if n == "this") {
                    if let Some(ty) = self.instance_props.get(prop) {
                        return Self::is_nfvalue_type(ty);
                    }
                    return false;
                }
                if let Expr::Name(cls) = base.as_ref()
                    && matches!(cls.as_str(), "Stage" | "XML" | "System" | "Mouse")
                {
                    return false;
                }
                self.expr_produces_nfvalue(base) || Self::is_nfvalue_type(&self.infer(lhs))
            }
            Expr::Name(n) if !self.is_local(n) => {
                if let Some(ty) = self.instance_props.get(n) {
                    return Self::is_nfvalue_type(ty);
                }
                Self::is_nfvalue_type(&self.infer(lhs))
            }
            _ => Self::is_nfvalue_type(&self.infer(lhs)),
        }
    }

    fn wrap_nfvalue(&mut self, expr: &Expr, offset: usize) -> String {
        if let Expr::Literal(lit) = expr {
            return format_swift_literal(lit, true);
        }
        if matches!(expr, Expr::Name(n) if n == "undefined") {
            return "NFValue.undefinedVal".into();
        }
        if matches!(expr, Expr::Name(n) if n == "null") {
            return "NFValue.null".into();
        }
        if matches!(expr, Expr::Name(n) if n == "NaN") {
            return "NFValue.number(.nan)".into();
        }
        if matches!(expr, Expr::Name(n) if n == "Infinity") {
            return "NFValue.number(.infinity)".into();
        }
        if let Expr::Name(fn_name) = expr
            && !self.is_local(fn_name)
            && !self.instance_props.contains_key(fn_name)
            && !self.global_vars.contains_key(fn_name)
            && let Some(sig) = self
                .global_funcs
                .get(fn_name)
                .or_else(|| self.instance_method_sigs.get(fn_name))
                .cloned()
        {
            let prefix = if self.instance_method_sigs.contains_key(fn_name) {
                "self."
            } else {
                ""
            };
            let arg_exprs: Vec<String> = sig
                .params
                .iter()
                .enumerate()
                .map(|(i, pty)| {
                    let a = format!("(_nfArgs.count > {i} ? _nfArgs[{i}] : .undefinedVal)");
                    match pty {
                        HirType::Float => format!("Double({a})"),
                        HirType::Int => format!("Int({a})"),
                        HirType::UInt => format!("UInt({a})"),
                        HirType::Bool => format!("{a}.isTruthy"),
                        HirType::String => format!("NFRuntime.toString({a})"),
                        _ => a,
                    }
                })
                .collect();
            let call = format!(
                "{prefix}{}({})",
                escape_swift_ident(fn_name),
                arg_exprs.join(", ")
            );
            let body = if sig.return_ty == HirType::Void {
                format!("{call}; return .undefinedVal")
            } else {
                format!("return NFValue.wrap({call})")
            };
            return if prefix.is_empty() || !self.has_instance_self() {
                format!("NFValue.closure {{ _nfArgs in {body} }}")
            } else {
                format!(
                    "NFValue.closure {{ [weak self] _nfArgs in guard let self else {{ return .undefinedVal }}; {body} }}"
                )
            };
        }
        if self.expr_produces_nfvalue(expr) {
            let s = self.emit_expr_with_offset(expr, offset);
            if self.expr_produces_optional(expr) {
                return format!("({s} ?? .undefinedVal)");
            }
            return s;
        }
        let ty = self.infer(expr);
        let is_opt = self.expr_produces_optional(expr);
        let inner_raw = self.emit_expr_with_offset(expr, offset);
        if inner_raw == "nil" {
            return "NFValue.null".into();
        }
        if inner_raw == "NFValue.null"
            || inner_raw == "NFValue.undefinedVal"
            || inner_raw == ".undefined"
            || inner_raw == ".undefinedVal"
            || inner_raw.starts_with("NFValue.")
            || inner_raw.starts_with("NFValue(")
        {
            return inner_raw;
        }
        if is_opt {
            let def = swift_nonoptional_default_expr(&ty);
            match ty {
                HirType::Bool => return format!("NFValue.bool({inner_raw} ?? false)"),
                HirType::Int => return format!("NFValue.int({inner_raw} ?? 0)"),
                HirType::UInt => {
                    return format!("NFValue.int(Int(clamping: ({inner_raw} ?? 0)))");
                }
                HirType::Float => return format!("NFValue.number({inner_raw} ?? 0.0)"),
                HirType::String => return format!("NFValue.string({inner_raw} ?? \"\")"),
                HirType::Vector(_) | HirType::Array => {
                    return format!("NFValue({inner_raw} ?? {def})");
                }
                HirType::MovieClip | HirType::Sprite | HirType::Button | HirType::TextField => {
                    return format!("NFValue({inner_raw})");
                }
                _ => return format!("NFValue.wrap({inner_raw})"),
            }
        }
        match ty {
            HirType::Bool => format!("NFValue.bool({inner_raw})"),
            HirType::Int => format!("NFValue.int({inner_raw})"),
            HirType::UInt => format!("NFValue.int(Int(clamping: {inner_raw}))"),
            HirType::Float => format!("NFValue.number({inner_raw})"),
            HirType::String => format!("NFValue.string({inner_raw})"),
            HirType::MovieClip | HirType::Sprite | HirType::Button | HirType::TextField => {
                format!("NFValue({inner_raw})")
            }
            HirType::Vector(_) => format!("NFValue({inner_raw})"),
            _ => format!("NFValue.wrap({inner_raw})"),
        }
    }

    fn emit_coerced_property_init(
        &mut self,
        expr: &Expr,
        prop_ty: &HirType,
        offset: usize,
    ) -> String {
        if let Expr::Conditional(cond, yes, no) = expr {
            let c = self.emit_bool_cond(cond, offset);
            let y = self.emit_coerced_property_init(yes, prop_ty, offset);
            let n = self.emit_coerced_property_init(no, prop_ty, offset);
            return format!("({c} ? {y} : {n})");
        }
        if matches!(
            prop_ty,
            HirType::TextField
                | HirType::MovieClip
                | HirType::Sprite
                | HirType::Button
                | HirType::Class(_)
        ) {
            if is_null_or_undef(expr) {
                return "nil".into();
            }
            if self.expr_produces_nfvalue(expr) {
                let raw = self.emit_subexpr(expr, 18, offset);
                let sw_ty = swift_type_for_hir(prop_ty);
                if !self.is_swift_reference_class(prop_ty) {
                    return format!("{sw_ty}({raw})");
                }
                return format!("{raw}.asObject(as: {sw_ty}.self)");
            }
            let raw = self.emit_expr_with_offset(expr, offset);
            if *prop_ty == HirType::Sprite && self.infer(expr) == HirType::MovieClip {
                return format!("({raw} as NFSprite?)");
            }
            return raw;
        }
        self.emit_coerced_expr(expr, prop_ty, offset)
    }

    fn emit_coerced_expr(&mut self, expr: &Expr, expected: &HirType, offset: usize) -> String {
        if let Expr::Conditional(cond, yes, no) = expr {
            let c = self.emit_bool_cond(cond, offset);
            let y = self.emit_coerced_expr(yes, expected, offset);
            let n = self.emit_coerced_expr(no, expected, offset);
            return format!("({c} ? {y} : {n})");
        }
        if *expected != HirType::Bool
            && let Expr::Binary(op, lhs, rhs) = expr
            && op == "||"
        {
            let cond = self.emit_bool_cond(lhs, offset);
            let y = self.emit_coerced_expr(lhs, expected, offset);
            let n = self.emit_coerced_expr(rhs, expected, offset);
            return format!("({cond} ? {y} : {n})");
        }
        if Self::is_nfvalue_type(expected) {
            return self.wrap_nfvalue(expr, offset);
        }
        if let HirType::Vector(elem_ty) = expected {
            if let Expr::Array(items) = expr {
                let parts: Vec<String> = items
                    .iter()
                    .map(|it| {
                        it.as_ref()
                            .map(|e| self.emit_coerced_expr(e, elem_ty, offset))
                            .unwrap_or_else(|| swift_nonoptional_default_expr(elem_ty).into())
                    })
                    .collect();
                return format!("[{}]", parts.join(", "));
            }
            if is_null_or_undef(expr) {
                return "[]".into();
            }
            let raw = self.emit_expr_with_offset(expr, offset);
            if self.expr_produces_optional(expr) {
                return format!("({raw} ?? [])");
            }
            return raw;
        }

        let actual = self.infer(expr);
        let from_nfvalue = self.expr_produces_nfvalue(expr);
        let is_opt = self.expr_produces_optional(expr);

        if *expected == HirType::Float {
            if let Expr::Literal(lit) = expr {
                let norm = format_swift_literal(lit, false);
                if norm.parse::<i64>().is_ok() && !norm.starts_with("0x") && !norm.starts_with("0X")
                {
                    return format!("{norm}.0");
                }
                if norm.starts_with("0x") || norm.starts_with("0X") {
                    return format!("Double({norm})");
                }
                return norm;
            }
            if is_opt {
                let raw = self.emit_expr_with_offset(expr, offset);
                if actual == HirType::Float {
                    return format!("({raw} ?? 0.0)");
                }
                let def = swift_nonoptional_default_expr(&actual);
                return format!("Double({raw} ?? {def})");
            }
            if actual.is_integer() || from_nfvalue || actual != HirType::Float {
                return self.wrap_double(expr, offset);
            }
        }
        if *expected == HirType::Int {
            if is_opt {
                let raw = self.emit_expr_with_offset(expr, offset);
                if actual == HirType::Int {
                    return format!("({raw} ?? 0)");
                }
                return format!("NFRuntime.toInt({raw})");
            }
            if actual == HirType::String {
                let raw = self.emit_expr_with_offset(expr, offset);
                return format!("NFRuntime.toInt({raw})");
            }
            if actual == HirType::Float || actual == HirType::UInt || from_nfvalue {
                return self.wrap_int(expr, offset);
            }
        }
        if *expected == HirType::UInt {
            if let Expr::Literal(lit) = expr {
                let norm = format_swift_literal(lit, false);
                if norm.parse::<u64>().is_ok() || norm.starts_with("0x") || norm.starts_with("0X") {
                    return norm;
                }
            }
            if is_opt {
                let raw = self.emit_expr_with_offset(expr, offset);
                if actual == HirType::UInt {
                    return format!("({raw} ?? 0)");
                }
                return format!("NFRuntime.toUInt({raw})");
            }
            if actual == HirType::Float || actual == HirType::Int || from_nfvalue {
                let inner = self.emit_expr_with_offset(expr, offset);
                return format!("NFRuntime.toUInt({inner})");
            }
        }
        if *expected == HirType::String {
            if is_null_or_undef(expr) {
                return "\"\"".into();
            }
            if is_opt {
                let raw = self.emit_expr_with_offset(expr, offset);
                if actual == HirType::String {
                    return format!("({raw} ?? \"\")");
                }
                return format!("NFRuntime.toString({raw})");
            }
            if from_nfvalue || actual != HirType::String {
                let inner = self.emit_expr_with_offset(expr, offset);
                return format!("NFRuntime.toString({inner})");
            }
        }
        if *expected == HirType::Bool {
            if is_opt {
                let raw = self.emit_expr_with_offset(expr, offset);
                if actual == HirType::Bool {
                    return format!("({raw} ?? false)");
                }
                return format!("NFRuntime.isTruthy({raw})");
            }
            if from_nfvalue {
                let inner = self.emit_subexpr(expr, 18, offset);
                return format!("{inner}.isTruthy");
            }
            if actual != HirType::Bool {
                let inner = self.emit_expr_with_offset(expr, offset);
                return format!("NFRuntime.isTruthy({inner})");
            }
        }
        if from_nfvalue && self.is_swift_reference_class(expected) {
            let inner = self.emit_subexpr(expr, 18, offset);
            let sw_ty = swift_type_for_hir(expected);
            let fallback = match expected {
                HirType::MovieClip => "NFMovieClip()".to_owned(),
                HirType::Sprite => "NFSprite()".to_owned(),
                HirType::Button => "NFButton()".to_owned(),
                HirType::TextField => "NFTextField()".to_owned(),
                _ => format!("{sw_ty}()"),
            };
            return format!("({inner}.asObject(as: {sw_ty}.self) ?? {fallback})");
        }
        if is_opt && self.is_swift_reference_class(expected) {
            let raw = self.emit_expr_with_offset(expr, offset);
            let sw_ty = swift_type_for_hir(expected);
            let fallback = match expected {
                HirType::MovieClip => "NFMovieClip()".to_owned(),
                HirType::Sprite => "NFSprite()".to_owned(),
                HirType::Button => "NFButton()".to_owned(),
                HirType::TextField => "NFTextField()".to_owned(),
                _ => format!("{sw_ty}()"),
            };
            if *expected == HirType::Sprite && actual == HirType::MovieClip {
                return format!("(({raw} as NFSprite?) ?? {fallback})");
            }
            return format!("({raw} ?? {fallback})");
        }
        if *expected == HirType::Sprite && actual == HirType::MovieClip {
            let raw = self.emit_expr_with_offset(expr, offset);
            return format!("({raw} as NFSprite)");
        }
        self.emit_expr_with_offset(expr, offset)
    }

    fn wrap_double(&mut self, expr: &Expr, offset: usize) -> String {
        if let Expr::Binary(op, lhs, rhs) = expr
            && op == "||"
        {
            let cond = self.emit_bool_cond(lhs, offset);
            let y = self.wrap_double(lhs, offset);
            let n = self.wrap_double(rhs, offset);
            return format!("({cond} ? {y} : {n})");
        }
        if let Expr::Call(callee, args, _) = expr
            && matches!(callee.as_ref(), Expr::Name(n) if n == "Number")
            && args.len() == 1
        {
            return format!(
                "Double({})",
                self.emit_coerced_expr(&args[0], &HirType::Float, offset)
            );
        }
        if let Expr::Literal(lit) = expr {
            let norm = format_swift_literal(lit, false);
            if norm.contains('.') || norm.contains('e') || norm.contains('E') {
                return norm;
            }
            return format!("Double({norm})");
        }
        if self.expr_produces_optional(expr) {
            let actual = self.infer(expr);
            let raw = self.emit_expr_with_offset(expr, offset);
            if actual == HirType::Float {
                return format!("({raw} ?? 0.0)");
            }
            if Self::is_nfvalue_type(&actual) {
                return format!("Double({raw} ?? .undefinedVal)");
            }
            let def = swift_nonoptional_default_expr(&actual);
            return format!("Double({raw} ?? {def})");
        }
        if self.infer(expr) == HirType::String && !self.expr_produces_optional(expr) {
            let s = self.emit_expr_with_offset(expr, offset);
            return format!("(Double({s}) ?? .nan)");
        }
        let s = self.emit_expr_with_offset(expr, offset);
        if s.starts_with("Double(") && s.ends_with(')') {
            s
        } else {
            format!("Double({s})")
        }
    }

    fn wrap_int(&mut self, expr: &Expr, offset: usize) -> String {
        if let Expr::Literal(lit) = expr {
            let norm = format_swift_literal(lit, false);
            if norm.parse::<i64>().is_ok() || norm.starts_with("0x") || norm.starts_with("0X") {
                return norm;
            }
        }
        let actual = self.infer(expr);
        if actual == HirType::String {
            let raw = self.emit_expr_with_offset(expr, offset);
            return format!("NFRuntime.toInt({raw})");
        }
        if self.expr_produces_optional(expr) {
            let raw = self.emit_expr_with_offset(expr, offset);
            if actual == HirType::Int {
                return format!("({raw} ?? 0)");
            }
            return format!("NFRuntime.toInt({raw})");
        }
        if actual == HirType::Float || actual == HirType::UInt || self.expr_produces_nfvalue(expr) {
            let s = self.emit_expr_with_offset(expr, offset);
            return format!("NFRuntime.toInt({s})");
        }
        let s = self.emit_expr_with_offset(expr, offset);
        if s.starts_with("Int(") && s.ends_with(')') {
            s
        } else {
            format!("Int({s})")
        }
    }

    fn emit_expr_with_offset(&mut self, expr: &Expr, offset: usize) -> String {
        match expr {
            Expr::Literal(raw) => format_swift_literal(raw, false),
            Expr::RegExpLiteral(pat, flags) => {
                format!(
                    "NFRegExp({}, {})",
                    encode_swift_string_literal(pat),
                    encode_swift_string_literal(flags)
                )
            }
            Expr::TypeValue(name, _) => swift_type_for_hir(&HirType::from_annotation(name)),
            Expr::Name(name) => self.emit_swift_ident(name),
            Expr::Member(base, prop) => self.emit_swift_member(base, prop, offset),
            Expr::Index(base, idx) => {
                let base_ty = self.infer(base);
                let b = if self.expr_produces_optional(base) {
                    if matches!(base_ty, HirType::Vector(_)) {
                        let b_raw = self.emit_subexpr(base, 18, offset);
                        format!("({b_raw} ?? [])")
                    } else {
                        self.emit_optional_chain_base(base, offset)
                    }
                } else {
                    self.emit_subexpr(base, 18, offset)
                };
                let i = if matches!(base_ty, HirType::Vector(_))
                    || matches!(&base_ty, HirType::Class(cn) if matches!(short_type_name(cn), "ByteArray" | "XML" | "XMLList"))
                {
                    self.wrap_int(idx, offset)
                } else {
                    self.emit_expr_with_offset(idx, offset)
                };
                format!("{b}[{i}]")
            }
            Expr::Array(items) => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|it| {
                        it.as_ref()
                            .map(|e| self.wrap_nfvalue(e, offset))
                            .unwrap_or_else(|| "NFValue.undefinedVal".into())
                    })
                    .collect();
                if parts.len() <= 24 {
                    format!("NFValue.fromArray([{}])", parts.join(", "))
                } else {
                    let mut chunks = Vec::new();
                    for ch in parts.chunks(24) {
                        chunks.push(format!("_nfArr.append(contentsOf: [{}])", ch.join(", ")));
                    }
                    format!(
                        "{{ () -> NFValue in var _nfArr: [NFValue] = []; _nfArr.reserveCapacity({}); {}; return NFValue.fromArray(_nfArr) }}()",
                        parts.len(),
                        chunks.join("; ")
                    )
                }
            }
            Expr::Object(props) => {
                if props.is_empty() {
                    return "NFValue.fromObject([])".into();
                }
                let parts: Vec<String> = props
                    .iter()
                    .map(|(PropertyKey { name, .. }, val)| {
                        let key_lit = encode_swift_string_literal(name);
                        format!("({key_lit}, {})", self.wrap_nfvalue(val, offset))
                    })
                    .collect();
                if parts.len() <= 24 {
                    format!("NFValue.fromObject([{}])", parts.join(", "))
                } else {
                    let mut chunks = Vec::new();
                    for ch in parts.chunks(24) {
                        chunks.push(format!("_nfPairs.append(contentsOf: [{}])", ch.join(", ")));
                    }
                    format!(
                        "{{ () -> NFValue in var _nfPairs: [(String, NFValue)] = []; _nfPairs.reserveCapacity({}); {}; return NFValue.fromObject(_nfPairs) }}()",
                        parts.len(),
                        chunks.join("; ")
                    )
                }
            }
            Expr::Unary(op, inner) => {
                let sw_op = if op == "not" { "!" } else { op.as_str() };
                match sw_op {
                    "~" => format!("~{}", self.wrap_int(inner, offset)),
                    "typeof" => format!(
                        "NFRuntime.typeofValue({})",
                        self.emit_expr_with_offset(inner, offset)
                    ),
                    "void" => "NFValue.undefinedVal".into(),
                    "+" => {
                        if self.infer(inner) == HirType::Int && !self.expr_produces_optional(inner)
                        {
                            self.emit_subexpr(inner, 16, offset)
                        } else {
                            self.wrap_double(inner, offset)
                        }
                    }
                    "!" => {
                        let inner_ty = self.infer(inner);
                        if (inner_ty == HirType::Bool || Self::is_nfvalue_type(&inner_ty))
                            && !self.expr_produces_optional(inner)
                        {
                            format!("!{}", self.emit_subexpr(inner, 16, offset))
                        } else {
                            format!("(!{})", self.emit_bool_cond(inner, offset))
                        }
                    }
                    "-" => {
                        let inner_ty = self.infer(inner);
                        if inner_ty == HirType::Int
                            && !self.expr_produces_optional(inner)
                            && !matches!(inner.as_ref(), Expr::Literal(_))
                        {
                            format!("(0 &- {})", self.emit_subexpr(inner, 13, offset))
                        } else if inner_ty == HirType::UInt {
                            format!("(-{})", self.wrap_double(inner, offset))
                        } else {
                            format!("-{}", self.emit_subexpr(inner, 16, offset))
                        }
                    }
                    _ => format!("{sw_op}{}", self.emit_subexpr(inner, 16, offset)),
                }
            }
            Expr::Delete(target, _) => {
                format!(
                    "NFRuntime.deleteProperty({})",
                    self.emit_expr_with_offset(target, offset)
                )
            }
            Expr::Update(op, target, _) => {
                let target_ty = self.infer(target);
                let is_int = target_ty.is_integer() && !self.lvalue_expects_nfvalue(target);
                let t = self.emit_lvalue_swift(target, offset);
                if is_int && target_ty == HirType::UInt {
                    let wop = if op == "++" { "&+" } else { "&-" };
                    return format!("(({t} {wop} 1) & 0xFFFF_FFFF)");
                }
                let delta = match (op.as_str(), is_int) {
                    ("++", true) => "&+ 1",
                    ("++", false) => "+ 1",
                    (_, true) => "&- 1",
                    (_, false) => "- 1",
                };
                format!("({t} {delta})")
            }
            Expr::Binary(op, lhs, rhs) => self.emit_swift_binary(op, lhs, rhs, offset),
            Expr::Conditional(cond, yes, no) => {
                let c = self.emit_bool_cond(cond, offset);
                let res_ty = self.infer(expr);
                let yt = self.infer(yes);
                let nt = self.infer(no);
                let display_poly = matches!(
                    yt,
                    HirType::MovieClip | HirType::Sprite | HirType::Button | HirType::TextField
                ) && matches!(
                    nt,
                    HirType::MovieClip | HirType::Sprite | HirType::Button | HirType::TextField
                ) && yt != nt;
                let eff_ty = if display_poly {
                    HirType::Class("DisplayObject".into())
                } else {
                    res_ty
                };
                let y = if is_null_or_undef(yes) && self.is_swift_reference_class(&eff_ty) {
                    "nil".to_owned()
                } else if display_poly {
                    let ys = self.emit_expr_with_offset(yes, offset);
                    let suf = if self.expr_produces_optional(yes) || self.expr_produces_optional(no)
                    {
                        "?"
                    } else {
                        ""
                    };
                    format!("({ys} as NFDisplayObject{suf})")
                } else {
                    self.emit_coerced_expr(yes, &eff_ty, offset)
                };
                let n = if is_null_or_undef(no) && self.is_swift_reference_class(&eff_ty) {
                    "nil".to_owned()
                } else if display_poly {
                    let ns = self.emit_expr_with_offset(no, offset);
                    let suf = if self.expr_produces_optional(yes) || self.expr_produces_optional(no)
                    {
                        "?"
                    } else {
                        ""
                    };
                    format!("({ns} as NFDisplayObject{suf})")
                } else {
                    self.emit_coerced_expr(no, &eff_ty, offset)
                };
                format!("({c} ? {y} : {n})")
            }
            Expr::Assign(op, lhs, rhs) => {
                // E4X attribute/child/index assignment (`first.@stock = ...`, `catalog.note = ...`, `list[0] = ...`)
                if op == "=" {
                    if let Expr::XmlSelect {
                        base,
                        local,
                        attribute,
                        descendant: false,
                        ..
                    } = lhs.as_ref()
                    {
                        let b = self.emit_optional_chain_base(base, offset);
                        let prop_lit = match local.as_ref() {
                            Expr::Name(n) => encode_swift_string_literal(n),
                            Expr::Literal(s) => format_swift_literal(s, false),
                            other => format!(
                                "NFRuntime.toString({})",
                                self.emit_expr_with_offset(other, offset)
                            ),
                        };
                        let r = self.emit_expr_with_offset(rhs, offset);
                        return if *attribute {
                            format!("{b}.setAttribute({prop_lit}, {r})")
                        } else {
                            format!("{b}.setChild({prop_lit}, {r})")
                        };
                    }
                    if let Expr::Member(base, prop) = lhs.as_ref() {
                        let bty = self.infer(base);
                        if matches!(&bty, HirType::Class(cn) if matches!(short_type_name(cn), "XML" | "XMLList"))
                            && !(matches!(base.as_ref(), Expr::Name(c) if c == "XML" || c == "XMLList"))
                        {
                            let b = self.emit_optional_chain_base(base, offset);
                            let prop_lit = encode_swift_string_literal(prop);
                            let r = self.emit_expr_with_offset(rhs, offset);
                            return format!("{b}.setChild({prop_lit}, {r})");
                        }
                    }
                    if let Expr::Index(base, idx) = lhs.as_ref() {
                        let bty = self.infer(base);
                        if matches!(&bty, HirType::Class(cn) if short_type_name(cn) == "XMLList") {
                            let b = self.emit_optional_chain_base(base, offset);
                            let i = self.wrap_int(idx, offset);
                            let r = self.emit_expr_with_offset(rhs, offset);
                            let rhs_ty = self.infer(rhs);
                            if matches!(&rhs_ty, HirType::Class(cn) if short_type_name(cn) == "XML")
                                && !self.expr_produces_optional(rhs)
                            {
                                return format!("{b}[{i}] = {r}");
                            }
                            return format!("{b}[{i}] = NFXML({r})");
                        }
                    }
                }

                if !self.source_is_as3
                    && let Some((base_sw, modern)) =
                        self.match_as2_percent_lvalue_swift(lhs, offset)
                {
                    let is_nf = matches!(lhs.as_ref(), Expr::Member(b, _) if Self::is_nfvalue_type(&self.infer(b)));
                    match op.as_str() {
                        "=" => {
                            let r = self.emit_scaled_100_to_01_swift(rhs, offset);
                            if is_nf {
                                return format!("{base_sw}.{modern} = NFValue.number({r})");
                            }
                            return format!("{base_sw}.{modern} = {r}");
                        }
                        "+=" | "-=" => {
                            let r = self.emit_scaled_100_to_01_swift(rhs, offset);
                            if is_nf {
                                return format!("{base_sw}.{modern} {op} NFValue.number({r})");
                            }
                            return format!("{base_sw}.{modern} {op} {r}");
                        }
                        _ => {}
                    }
                }
                let expects_nf = self.lvalue_expects_nfvalue(lhs);
                let l_ty = if expects_nf {
                    HirType::Unknown
                } else {
                    self.infer(lhs)
                };
                let l = self.emit_lvalue_swift(lhs, offset);
                let is_nullable_lhs = matches!(
                    lhs.as_ref(),
                    Expr::Name(n) if (self.is_local(n) && self.is_nullable_local(n)) || (!self.is_local(n) && self.is_nullable_prop(n))
                ) || matches!(
                    lhs.as_ref(),
                    Expr::Member(b, p) if matches!(b.as_ref(), Expr::Name(n) if n == "this") && self.is_nullable_prop(p)
                ) || matches!(
                    lhs.as_ref(),
                    Expr::Member(_, p) if matches!(p.as_str(), "mask" | "scrollRect" | "scale9Grid")
                );
                let r = if op == "="
                    && matches!(&l_ty, HirType::Class(cn) if matches!(cn.as_str(), "NFScaleMode" | "NFStageAlign" | "NFBlendMode" | "NFTextKind"))
                {
                    let HirType::Class(ref enum_ty) = l_ty else {
                        unreachable!()
                    };
                    let raw_r = self.emit_coerced_expr(rhs, &HirType::String, offset);
                    if raw_r.starts_with('"') && raw_r.ends_with('"') {
                        raw_r
                    } else {
                        format!("{enum_ty}({raw_r})")
                    }
                } else if op == "="
                    && matches!(&l_ty, HirType::Class(cn) if short_type_name(cn) == "XMLList")
                    && matches!(self.infer(rhs), HirType::Class(rc) if short_type_name(&rc) == "XML")
                {
                    format!("NFXMLList({})", self.emit_expr_with_offset(rhs, offset))
                } else if op == "="
                    && matches!(
                        l_ty,
                        HirType::TextField
                            | HirType::MovieClip
                            | HirType::Sprite
                            | HirType::Button
                            | HirType::Class(_)
                    )
                    && is_nullable_lhs
                {
                    self.emit_coerced_property_init(rhs, &l_ty, offset)
                } else if !expects_nf && l_ty == HirType::Unknown {
                    self.emit_expr_with_offset(rhs, offset)
                } else {
                    self.emit_coerced_expr(rhs, &l_ty, offset)
                };
                // Guard against Swift warning/error `assigning a variable/property to itself` (`x = x`)
                if op == "=" && l == r {
                    return format!("_ = {l}");
                }
                if !expects_nf && l_ty.is_integer() {
                    if l_ty == HirType::UInt {
                        match op.as_str() {
                            "+=" => return format!("{l} = ({l} &+ {r}) & 0xFFFF_FFFF"),
                            "-=" => return format!("{l} = ({l} &- {r}) & 0xFFFF_FFFF"),
                            "*=" => return format!("{l} = ({l} &* {r}) & 0xFFFF_FFFF"),
                            _ => {}
                        }
                    }
                    match op.as_str() {
                        "+=" => return format!("{l} &+= {r}"),
                        "-=" => return format!("{l} &-= {r}"),
                        "*=" => return format!("{l} &*= {r}"),
                        "%=" => {
                            let fn_mod = if l_ty == HirType::UInt {
                                "uintMod"
                            } else {
                                "intMod"
                            };
                            return format!("{l} = NFRuntime.{fn_mod}({l}, {r})");
                        }
                        "<<=" => {
                            let ri = self.wrap_int(rhs, offset);
                            let li = if l_ty == HirType::UInt {
                                format!("Int(truncatingIfNeeded: {l})")
                            } else {
                                l.clone()
                            };
                            let res = format!("NFRuntime.shl32({li}, {ri})");
                            return if l_ty == HirType::UInt {
                                format!("{l} = NFRuntime.toUInt({res})")
                            } else {
                                format!("{l} = {res}")
                            };
                        }
                        ">>=" => {
                            let ri = self.wrap_int(rhs, offset);
                            let li = if l_ty == HirType::UInt {
                                format!("Int(truncatingIfNeeded: {l})")
                            } else {
                                l.clone()
                            };
                            let res = format!("NFRuntime.shr32({li}, {ri})");
                            return if l_ty == HirType::UInt {
                                format!("{l} = NFRuntime.toUInt({res})")
                            } else {
                                format!("{l} = {res}")
                            };
                        }
                        ">>>=" => {
                            let ri = self.wrap_int(rhs, offset);
                            let li = if l_ty == HirType::UInt {
                                format!("Int(truncatingIfNeeded: {l})")
                            } else {
                                l.clone()
                            };
                            let res = format!("NFRuntime.ushr32({li}, {ri})");
                            return if l_ty == HirType::UInt {
                                format!("{l} = {res}")
                            } else {
                                format!("{l} = Int({res})")
                            };
                        }
                        _ => {}
                    }
                }
                format!("{l} {op} {r}")
            }
            Expr::Call(callee, args, call_off) => self.emit_swift_call(callee, args, *call_off),
            Expr::NewError(ty, args, call_off)
            | Expr::NewBuiltin(ty, args, call_off)
            | Expr::NewClass(ty, args, call_off)
            | Expr::NewValue(ty, args, call_off) => self.emit_swift_new(ty, args, *call_off),
            Expr::NewVector(elem_raw, args, call_off) => {
                self.emit_swift_new_vector(elem_raw, args, *call_off)
            }
            Expr::InstanceOf(lhs, rhs) => {
                let l = self.emit_expr_with_offset(lhs, offset);
                let r = self.emit_expr_with_offset(rhs, offset);
                format!("({l} is {r})")
            }
            Expr::TypeTest(op, inner, ty_name, tt_off) => {
                let target_hir = HirType::from_annotation(ty_name);
                let sw_ty = swift_type_for_hir(&target_hir);
                if self.expr_produces_nfvalue(inner) {
                    let inner_sw = self.emit_subexpr(inner, 18, *tt_off);
                    if Self::is_nfvalue_type(&target_hir) {
                        return if op == "as" {
                            inner_sw
                        } else {
                            format!("({inner_sw} != .null && {inner_sw} != .undefinedVal)")
                        };
                    }
                    if self.is_swift_reference_class(&target_hir) {
                        return if op == "as" {
                            format!("{inner_sw}.asObject(as: {sw_ty}.self)")
                        } else {
                            format!("({inner_sw}.asObject(as: {sw_ty}.self) != nil)")
                        };
                    }
                }
                let inner_sw = self.emit_expr_with_offset(inner, *tt_off);
                if op == "as" {
                    format!("({inner_sw} as? {sw_ty})")
                } else {
                    format!("({inner_sw} is {sw_ty})")
                }
            }
            Expr::Function(f) => {
                let saved_top = self.top_level_is_symbol_frame;
                let saved_in_frame = self.in_symbol_frame_method;
                let saved_closure = self.in_nfvalue_closure;
                let saved_ret = std::mem::replace(&mut self.current_return_type, HirType::Unknown);
                let saved_pnames = std::mem::take(&mut self.current_fn_param_names);
                let saved_params = std::mem::take(&mut self.current_fn_params);
                self.top_level_is_symbol_frame = false;
                self.in_symbol_frame_method = false;
                self.in_nfvalue_closure = true;
                self.push_scope();
                self.indent += 1;
                let in_pad = self.pad();

                let closure_usage = LocalUsageInfo::analyze(&f.body);
                let mut param_binds = Vec::new();
                for (i, (raw_p, pty)) in f.params.iter().enumerate() {
                    let pname = escape_swift_ident(raw_p);
                    self.current_fn_param_names.push(pname.clone());
                    let hir_ty = pty
                        .as_deref()
                        .map_or(HirType::Unknown, HirType::from_annotation);
                    let sw_ty = swift_type_for_hir(&hir_ty);
                    self.mark_local(raw_p);
                    self.env.define(raw_p.clone(), hir_ty.clone());

                    let arg_ref = format!("_nfArgs[{i}]");
                    let coerced_arg = match &hir_ty {
                        HirType::Float => format!("Double({arg_ref})"),
                        HirType::Int => format!("Int({arg_ref})"),
                        HirType::UInt => format!("UInt({arg_ref})"),
                        HirType::Bool => format!("{arg_ref}.isTruthy"),
                        HirType::String => format!("NFRuntime.toString({arg_ref})"),
                        other if self.is_swift_reference_class(other) => {
                            let fallback = match other {
                                HirType::MovieClip => "NFMovieClip()".to_owned(),
                                HirType::Sprite => "NFSprite()".to_owned(),
                                HirType::Button => "NFButton()".to_owned(),
                                HirType::TextField => "NFTextField()".to_owned(),
                                _ => format!("{sw_ty}()"),
                            };
                            format!("({arg_ref}.asObject(as: {sw_ty}.self) ?? {fallback})")
                        }
                        _ => arg_ref,
                    };
                    let def_expr = if let Some(Some(def)) = f.defaults.get(i) {
                        self.emit_coerced_expr(def, &hir_ty, offset)
                    } else if Self::is_nfvalue_type(&hir_ty) {
                        "NFValue.undefinedVal".to_owned()
                    } else {
                        swift_nonoptional_default_expr(&hir_ty).to_owned()
                    };
                    let redeclared = closure_usage
                        .var_decl_counts
                        .get(raw_p)
                        .copied()
                        .unwrap_or(0)
                        > 1;
                    let directly_mutated = closure_usage.direct_mutated.contains(raw_p);
                    let struct_mutated = !self.is_swift_reference_class(&hir_ty)
                        && !Self::is_nfvalue_type(&hir_ty)
                        && closure_usage.member_or_method_mutated.contains(raw_p);
                    let kw = if redeclared || directly_mutated || struct_mutated {
                        "var"
                    } else {
                        "let"
                    };
                    param_binds.push(format!(
                        "{in_pad}{kw} {pname}: {sw_ty} = _nfArgs.count > {i} ? {coerced_arg} : {def_expr}\n{in_pad}_ = {pname}"
                    ));
                }
                if let Some(rest) = &f.rest {
                    let rname = escape_swift_ident(rest);
                    let start = f.params.len();
                    self.mark_local(rest);
                    self.env
                        .define(rest.clone(), HirType::Vector(Box::new(HirType::Unknown)));
                    param_binds.push(format!(
                        "{in_pad}let {rname}: [NFValue] = _nfArgs.count > {start} ? Array(_nfArgs[{start}...]) : []\n{in_pad}_ = {rname}"
                    ));
                }

                let body = self.emit_function_body(&f.body);
                let ends_with_return = f
                    .body
                    .last()
                    .is_some_and(|s| matches!(s.unlocated(), Stmt::Return(_, _)));
                self.indent -= 1;
                self.pop_scope();
                self.top_level_is_symbol_frame = saved_top;
                self.in_symbol_frame_method = saved_in_frame;
                self.in_nfvalue_closure = saved_closure;
                self.current_return_type = saved_ret;
                self.current_fn_param_names = saved_pnames;
                self.current_fn_params = saved_params;

                let pad = self.pad();
                let mut closure_lines = Vec::new();
                let use_weak_self = self.has_instance_self();
                if use_weak_self {
                    let uses_self = contains_swift_ident(&body, "self")
                        || param_binds.iter().any(|p| contains_swift_ident(p, "self"));
                    if uses_self {
                        closure_lines.push(format!(
                            "{in_pad}guard let self else {{ return .undefinedVal }}"
                        ));
                    } else {
                        closure_lines.push(format!(
                            "{in_pad}guard self != nil else {{ return .undefinedVal }}"
                        ));
                    }
                }
                let loop_caps =
                    self.collect_for_loop_captures(f, &closure_usage, &body, &param_binds);
                let mut caps = Vec::new();
                if use_weak_self {
                    caps.push("weak self".to_owned());
                }
                caps.extend(loop_caps);
                let cap_clause = if caps.is_empty() {
                    String::new()
                } else {
                    format!("[{}] ", caps.join(", "))
                };
                closure_lines.extend(param_binds);
                if !body.trim().is_empty() {
                    closure_lines.push(body);
                }
                if !ends_with_return {
                    closure_lines.push(format!("{in_pad}return .undefinedVal"));
                }
                format!(
                    "NFValue.closure {{ {cap_clause}_nfArgs in\n{}\n{pad}}}",
                    closure_lines.join("\n")
                )
            }
            Expr::XmlLiteral(raw, _) => {
                format!("NFXML({})", encode_swift_string_literal(raw))
            }
            Expr::XmlSelect {
                base,
                local,
                attribute,
                descendant,
                ..
            } => {
                let b = self.emit_optional_chain_base(base, offset);
                let prop_lit = match local.as_ref() {
                    Expr::Name(n) => encode_swift_string_literal(n),
                    Expr::Literal(s) => format_swift_literal(s, false),
                    other => format!(
                        "NFRuntime.toString({})",
                        self.emit_expr_with_offset(other, offset)
                    ),
                };
                if *descendant {
                    if *attribute {
                        format!("{b}.descendants(\"@\" + {prop_lit})")
                    } else {
                        format!("{b}.descendants({prop_lit})")
                    }
                } else if *attribute {
                    format!("{b}.attribute({prop_lit})")
                } else {
                    format!("{b}.child({prop_lit})")
                }
            }
            Expr::XmlWithAttr {
                local,
                attribute,
                descendant,
                ..
            } => {
                let prop_lit = match local.as_ref() {
                    Expr::Name(n) => encode_swift_string_literal(n),
                    Expr::Literal(s) => format_swift_literal(s, false),
                    other => format!(
                        "NFRuntime.toString({})",
                        self.emit_expr_with_offset(other, offset)
                    ),
                };
                if *descendant {
                    format!("_nfXmlItem.descendants({prop_lit})")
                } else if *attribute {
                    format!("_nfXmlItem.attribute({prop_lit})")
                } else {
                    format!("_nfXmlItem.child({prop_lit})")
                }
            }
            Expr::XmlFilter(base, predicate, x_off) => {
                let b = self.emit_expr_with_offset(base, *x_off);
                let p = self.emit_bool_cond(predicate, *x_off);
                format!("NFXMLList(NFRuntime.values({b}).filter {{ _nfXmlItem in {p} }})")
            }
        }
    }

    fn emit_swift_new_vector(&mut self, elem_raw: &str, args: &[Expr], offset: usize) -> String {
        let elem_hir = HirType::from_annotation(elem_raw);
        if args.len() == 1
            && let Expr::Array(items) = &args[0]
        {
            let parts: Vec<String> = items
                .iter()
                .map(|it| {
                    it.as_ref()
                        .map(|e| self.emit_coerced_expr(e, &elem_hir, offset))
                        .unwrap_or_else(|| swift_nonoptional_default_expr(&elem_hir).into())
                })
                .collect();
            return format!("[{}]", parts.join(", "));
        }
        if let Some(len_arg) = args.first()
            && !matches!(len_arg, Expr::Literal(s) if s == "0")
        {
            let len_sw = self.wrap_int(len_arg, offset);
            let def_el = swift_nonoptional_default_expr(&elem_hir);
            return format!("Array(repeating: {def_el}, count: Swift.max(0, {len_sw}))");
        }
        "[]".into()
    }

    fn emit_swift_binary(&mut self, op: &str, lhs: &Expr, rhs: &Expr, offset: usize) -> String {
        // Special pattern: `(expr) | 0` integer truncation -> `Int(expr)`!
        if op == "|" && matches!(rhs, Expr::Literal(s) if s == "0") {
            return self.wrap_int(lhs, offset);
        }
        if op == ">>>" && matches!(rhs, Expr::Literal(s) if s == "0") {
            let inner = self.emit_coerced_expr(lhs, &HirType::UInt, offset);
            return if inner.starts_with("UInt(") {
                inner
            } else {
                format!("UInt({inner})")
            };
        }

        // Division `a / b` ALWAYS promotes to floating-point `Double(a) / Double(b)` in ActionScript!
        if op == "/" {
            let ld = self.wrap_double(lhs, offset);
            let rd = self.wrap_double(rhs, offset);
            return format!("{ld} / {rd}");
        }

        // Bitwise operators `&`, `|`, `^`, `<<`, `>>`, `>>>` coerce operands via 32-bit helpers!
        if matches!(op, "&" | "|" | "^" | "<<" | ">>" | ">>>") {
            let li = self.wrap_int(lhs, offset);
            let ri = self.wrap_int(rhs, offset);
            return match op {
                "<<" => format!("NFRuntime.shl32({li}, {ri})"),
                ">>" => format!("NFRuntime.shr32({li}, {ri})"),
                ">>>" => format!("Int(NFRuntime.ushr32({li}, {ri}))"),
                _ => format!("{li} {op} {ri}"),
            };
        }

        let lt = self.infer(lhs);
        let rt = self.infer(rhs);

        if matches!(op, "&&" | "||" | "and" | "or") {
            let is_or = op == "||" || op == "or";
            let sw_op = if is_or { "||" } else { "&&" };
            let lb = self.emit_bool_cond(lhs, offset);
            if op == "and" || op == "or" || (lt == HirType::Bool && rt == HirType::Bool) {
                let rb = self.emit_bool_cond(rhs, offset);
                return format!("{lb} {sw_op} {rb}");
            }
            if lt == rt
                && !Self::is_nfvalue_type(&lt)
                && !self.expr_produces_optional(lhs)
                && !self.expr_produces_optional(rhs)
            {
                let lv = self.emit_expr_with_offset(lhs, offset);
                let rv = self.emit_expr_with_offset(rhs, offset);
                return if is_or {
                    format!("({lb} ? {lv} : {rv})")
                } else {
                    format!("({lb} ? {rv} : {lv})")
                };
            }
            let lw = self.wrap_nfvalue(lhs, offset);
            let rw = self.wrap_nfvalue(rhs, offset);
            return if is_or {
                format!("({lb} ? {lw} : {rw})")
            } else {
                format!("({lb} ? {rw} : {lw})")
            };
        }

        // String concatenation `+` when either operand is String (flatten 3+ chains to O(N) `[...].joined()`)
        if (op == "+" || op == "add") && (lt == HirType::String || rt == HirType::String) {
            let mut chain = Vec::new();
            flatten_plus_chain(lhs, &mut chain);
            flatten_plus_chain(rhs, &mut chain);
            if chain.len() >= 3 {
                let parts: Vec<String> = chain
                    .into_iter()
                    .map(|part| {
                        let pt = self.infer(part);
                        if pt == HirType::String && !self.expr_produces_optional(part) {
                            self.emit_expr_with_offset(part, offset)
                        } else if pt == HirType::String {
                            format!("({} ?? \"\")", self.emit_expr_with_offset(part, offset))
                        } else {
                            format!(
                                "NFRuntime.toString({})",
                                self.emit_expr_with_offset(part, offset)
                            )
                        }
                    })
                    .collect();
                return format!("([{}] as [String]).joined()", parts.join(", "));
            }
            let ls = if lt == HirType::String && !self.expr_produces_optional(lhs) {
                self.emit_subexpr(lhs, 12, offset)
            } else if lt == HirType::String {
                format!("({} ?? \"\")", self.emit_expr_with_offset(lhs, offset))
            } else {
                format!(
                    "NFRuntime.toString({})",
                    self.emit_expr_with_offset(lhs, offset)
                )
            };
            let rs = if rt == HirType::String && !self.expr_produces_optional(rhs) {
                self.emit_subexpr(rhs, 12, offset)
            } else if rt == HirType::String {
                format!("({} ?? \"\")", self.emit_expr_with_offset(rhs, offset))
            } else {
                format!(
                    "NFRuntime.toString({})",
                    self.emit_expr_with_offset(rhs, offset)
                )
            };
            return format!("{ls} + {rs}");
        }

        // Null / undefined checks (`x != null`, `x === undefined`)
        if matches!(op, "==" | "!=" | "===" | "!==") {
            let is_eq = matches!(op, "==" | "===");
            let is_strict = matches!(op, "===" | "!==");
            if is_null_or_undef(rhs) && !is_null_or_undef(lhs) {
                if self.expr_produces_nfvalue(lhs) {
                    let lw = self.wrap_nfvalue(lhs, offset);
                    let rw = self.wrap_nfvalue(rhs, offset);
                    let fn_name = if is_strict {
                        "strictlyEquals"
                    } else {
                        "looselyEquals"
                    };
                    return if is_eq {
                        format!("NFRuntime.{fn_name}({lw}, {rw})")
                    } else {
                        format!("!NFRuntime.{fn_name}({lw}, {rw})")
                    };
                }
                if !self.expr_produces_optional(lhs) {
                    let const_bool = if is_eq { "false" } else { "true" };
                    if expr_may_have_side_effects(lhs) {
                        let l = self.emit_expr_with_offset(lhs, offset);
                        return format!("({{ _ = {l}; return {const_bool} }}())");
                    }
                    return const_bool.into();
                }
                let cmp = if is_eq { "==" } else { "!=" };
                let l = self.emit_subexpr(lhs, 9, offset);
                return format!("{l} {cmp} nil");
            }
            if is_null_or_undef(lhs) && !is_null_or_undef(rhs) {
                if self.expr_produces_nfvalue(rhs) {
                    let lw = self.wrap_nfvalue(lhs, offset);
                    let rw = self.wrap_nfvalue(rhs, offset);
                    let fn_name = if is_strict {
                        "strictlyEquals"
                    } else {
                        "looselyEquals"
                    };
                    return if is_eq {
                        format!("NFRuntime.{fn_name}({lw}, {rw})")
                    } else {
                        format!("!NFRuntime.{fn_name}({lw}, {rw})")
                    };
                }
                if !self.expr_produces_optional(rhs) {
                    let const_bool = if is_eq { "false" } else { "true" };
                    if expr_may_have_side_effects(rhs) {
                        let r = self.emit_expr_with_offset(rhs, offset);
                        return format!("({{ _ = {r}; return {const_bool} }}())");
                    }
                    return const_bool.into();
                }
                let cmp = if is_eq { "==" } else { "!=" };
                let r = self.emit_subexpr(rhs, 9, offset);
                return format!("{r} {cmp} nil");
            }
        }

        // Dynamic `NFValue` binary operators (`+`, `==`, `!=`, `===`, `!==`, `-`, `*`, `%`, `<`, `<=`, `>`, `>=`, `in`)
        let l_nf = self.expr_produces_nfvalue(lhs);
        let r_nf = self.expr_produces_nfvalue(rhs);
        if l_nf || r_nf {
            match op {
                "+" | "add" => {
                    let lw = self.wrap_nfvalue(lhs, offset);
                    let rw = self.wrap_nfvalue(rhs, offset);
                    return format!("NFRuntime.add({lw}, {rw})");
                }
                "==" | "eq" => {
                    let lw = self.wrap_nfvalue(lhs, offset);
                    let rw = self.wrap_nfvalue(rhs, offset);
                    return format!("NFRuntime.looselyEquals({lw}, {rw})");
                }
                "!=" | "ne" | "<>" => {
                    let lw = self.wrap_nfvalue(lhs, offset);
                    let rw = self.wrap_nfvalue(rhs, offset);
                    return format!("!NFRuntime.looselyEquals({lw}, {rw})");
                }
                "===" => {
                    let lw = self.wrap_nfvalue(lhs, offset);
                    let rw = self.wrap_nfvalue(rhs, offset);
                    return format!("NFRuntime.strictlyEquals({lw}, {rw})");
                }
                "!==" => {
                    let lw = self.wrap_nfvalue(lhs, offset);
                    let rw = self.wrap_nfvalue(rhs, offset);
                    return format!("!NFRuntime.strictlyEquals({lw}, {rw})");
                }
                "-" | "*" => {
                    let ld = self.wrap_double(lhs, offset);
                    let rd = self.wrap_double(rhs, offset);
                    return format!("{ld} {op} {rd}");
                }
                "%" => {
                    let ld = self.wrap_double(lhs, offset);
                    let rd = self.wrap_double(rhs, offset);
                    return format!("{ld}.truncatingRemainder(dividingBy: {rd})");
                }
                "<" | "<=" | ">" | ">=" => {
                    if l_nf && r_nf {
                        let lw = self.emit_subexpr(lhs, 10, offset);
                        let rw = self.emit_subexpr(rhs, 10, offset);
                        return format!("{lw} {op} {rw}");
                    }
                    let ld = self.wrap_double(lhs, offset);
                    let rd = self.wrap_double(rhs, offset);
                    return format!("{ld} {op} {rd}");
                }
                "in" => {
                    let key_sw = self.emit_coerced_expr(lhs, &HirType::String, offset);
                    let obj_sw = self.wrap_nfvalue(rhs, offset);
                    return format!("{obj_sw}.hasOwnProperty({key_sw})");
                }
                _ => {}
            }
        }

        // Reference identity comparisons (`a == b` / `a === b` on reference classes without `Equatable`)
        if matches!(op, "==" | "!=" | "===" | "!==")
            && self.is_swift_reference_class(&lt)
            && self.is_swift_reference_class(&rt)
        {
            let ref_op = if matches!(op, "==" | "===") {
                "==="
            } else {
                "!=="
            };
            let l = self.emit_subexpr(lhs, 9, offset);
            let r = self.emit_subexpr(rhs, 9, offset);
            return format!("{l} {ref_op} {r}");
        }

        // Mixed `Int`/`UInt` + `Double` arithmetic or comparison promotes integer via `Double(...)`!
        if matches!(
            op,
            "+" | "-" | "*" | "%" | "<" | "<=" | ">" | ">=" | "==" | "!=" | "===" | "!=="
        ) && !is_null_or_undef(lhs)
            && !is_null_or_undef(rhs)
        {
            let mixed_float = (lt == HirType::Float && rt.is_integer())
                || (rt == HirType::Float && lt.is_integer());
            if op == "%" && (lt == HirType::Float || rt == HirType::Float) {
                let ld = if lt == HirType::Float && !self.expr_produces_optional(lhs) {
                    self.emit_subexpr(lhs, 18, offset)
                } else {
                    self.wrap_double(lhs, offset)
                };
                let rd = if rt == HirType::Float && !self.expr_produces_optional(rhs) {
                    self.emit_expr_with_offset(rhs, offset)
                } else {
                    self.wrap_double(rhs, offset)
                };
                return format!("{ld}.truncatingRemainder(dividingBy: {rd})");
            }
            if mixed_float {
                let cmp_op = match op {
                    "===" => "==",
                    "!==" => "!=",
                    other => other,
                };
                let prec = op_prec(cmp_op);
                let l = if lt.is_integer() || self.expr_produces_optional(lhs) {
                    self.wrap_double(lhs, offset)
                } else {
                    self.emit_subexpr(lhs, prec, offset)
                };
                let r = if rt.is_integer() || self.expr_produces_optional(rhs) {
                    self.wrap_double(rhs, offset)
                } else {
                    self.emit_subexpr(rhs, prec, offset)
                };
                return format!("{l} {cmp_op} {r}");
            }
            let is_arith_op = matches!(op, "+" | "-" | "*" | "%");
            let mixed_int_uint = if is_arith_op {
                (lt == HirType::UInt && rt == HirType::Int)
                    || (lt == HirType::Int && rt == HirType::UInt)
            } else {
                (lt == HirType::UInt && rt == HirType::Int && !is_nonneg_int_literal(rhs))
                    || (lt == HirType::Int && rt == HirType::UInt && !is_nonneg_int_literal(lhs))
            };
            if mixed_int_uint {
                if op == "%" {
                    let l = self.wrap_int(lhs, offset);
                    let r = self.wrap_int(rhs, offset);
                    return format!("NFRuntime.intMod({l}, {r})");
                }
                let cmp_op = match op {
                    "===" => "==",
                    "!==" => "!=",
                    "+" => "&+",
                    "-" => "&-",
                    "*" => "&*",
                    other => other,
                };
                let prec = op_prec(match op {
                    "===" => "==",
                    "!==" => "!=",
                    other => other,
                });
                let l = if lt == HirType::UInt || self.expr_produces_optional(lhs) {
                    self.wrap_int(lhs, offset)
                } else {
                    self.emit_subexpr(lhs, prec, offset)
                };
                let r = if rt == HirType::UInt || self.expr_produces_optional(rhs) {
                    self.wrap_int(rhs, offset)
                } else {
                    self.emit_subexpr(rhs, prec, offset)
                };
                return format!("{l} {cmp_op} {r}");
            }
        }

        if lt.is_integer() && rt.is_integer() && op == "%" {
            let l = if self.expr_produces_optional(lhs) {
                format!(
                    "({} ?? {})",
                    self.emit_expr_with_offset(lhs, offset),
                    swift_nonoptional_default_expr(&lt)
                )
            } else {
                self.emit_expr_with_offset(lhs, offset)
            };
            let r = if self.expr_produces_optional(rhs) {
                format!(
                    "({} ?? {})",
                    self.emit_expr_with_offset(rhs, offset),
                    swift_nonoptional_default_expr(&rt)
                )
            } else {
                self.emit_expr_with_offset(rhs, offset)
            };
            let fn_mod = if lt == HirType::UInt && rt == HirType::UInt {
                "uintMod"
            } else {
                "intMod"
            };
            return format!("NFRuntime.{fn_mod}({l}, {r})");
        }

        let is_int_math = lt.is_integer() && rt.is_integer();
        let is_uint_math = lt == HirType::UInt && rt == HirType::UInt;
        let sw_op = match op {
            "and" => "&&",
            "or" => "||",
            "eq" | "===" => "==",
            "ne" | "<>" | "!==" => "!=",
            "+" if is_int_math => "&+",
            "-" if is_int_math => "&-",
            "*" if is_int_math => "&*",
            other => other,
        };
        let prec_key = match op {
            "and" => "&&",
            "or" => "||",
            "eq" | "===" => "==",
            "ne" | "<>" | "!==" => "!=",
            other => other,
        };
        let prec = op_prec(prec_key);
        let l = if self.expr_produces_optional(lhs) {
            format!(
                "({} ?? {})",
                self.emit_expr_with_offset(lhs, offset),
                swift_nonoptional_default_expr(&lt)
            )
        } else {
            self.emit_subexpr(lhs, prec, offset)
        };
        let r = if self.expr_produces_optional(rhs) {
            format!(
                "({} ?? {})",
                self.emit_expr_with_offset(rhs, offset),
                swift_nonoptional_default_expr(&rt)
            )
        } else {
            self.emit_subexpr(rhs, prec, offset)
        };
        if is_uint_math && matches!(op, "+" | "-" | "*") {
            format!("(({l} {sw_op} {r}) & 0xFFFF_FFFF)")
        } else {
            format!("{l} {sw_op} {r}")
        }
    }

    fn emit_swift_ident(&mut self, name: &str) -> String {
        match name {
            "this" => return "self".into(),
            "null" | "undefined" => return "nil".into(),
            "true" | "false" => return name.to_owned(),
            "NaN" => return "Double.nan".into(),
            "Infinity" => return "Double.infinity".into(),
            "_global" => return "NFRuntime.globalObject".into(),
            "_root" | "_level0" => {
                if self.class_has_stage {
                    return "NFValue(self.root)".into();
                }
                return "NFRuntime.globalObject".into();
            }
            "arguments" => {
                if self.in_nfvalue_closure {
                    return "NFValue.fromArray(_nfArgs)".into();
                }
                let wrapped: Vec<String> = self
                    .current_fn_param_names
                    .iter()
                    .map(|p| format!("NFValue.wrap({p})"))
                    .collect();
                return format!("NFValue.fromArray([{}])", wrapped.join(", "));
            }
            _ => {}
        }

        if let Some(alias) = self.lookup_vector_alias(name) {
            return alias.clone();
        }

        if self.is_local(name) {
            return escape_swift_ident(name);
        }

        if self.instance_props.contains_key(name) || self.instance_methods.contains(name) {
            return format!("self.{}", escape_swift_ident(name));
        }

        if let Some(cls) = &self.current_class_short
            && (self.static_props.contains(name) || self.static_methods.contains(name))
        {
            return format!("{cls}.{}", escape_swift_ident(name));
        }

        if self.global_vars.contains_key(name) || self.global_funcs.contains_key(name) {
            return escape_swift_ident(name);
        }

        if !self.source_is_as3 {
            if let Some(modern) = map_as2_percent_prop_to_modern(name) {
                return format!("(self.{modern} * 100.0)");
            }
            if let Some(modern) = map_as2_direct_prop_to_modern(name) {
                return format!("self.{modern}");
            }
        }

        if self.class_has_stage
            && matches!(
                name,
                "stage"
                    | "parent"
                    | "root"
                    | "graphics"
                    | "x"
                    | "y"
                    | "scaleX"
                    | "scaleY"
                    | "alpha"
                    | "visible"
                    | "rotation"
                    | "width"
                    | "height"
                    | "mouseX"
                    | "mouseY"
                    | "currentFrame"
                    | "totalFrames"
            )
        {
            return format!("self.{name}");
        }

        if let Some(cinfo) = self.lookup_class_info(name) {
            return cinfo.swift_name.clone();
        }

        if let Some(builtin_sw) = map_builtin_static_class_swift(name) {
            return builtin_sw.into();
        }

        escape_swift_ident(name)
    }

    fn emit_lvalue_swift(&mut self, lhs: &Expr, offset: usize) -> String {
        if let Expr::Name(name) = lhs {
            if name == "_global" {
                return "NFRuntime.globalObject".into();
            }
            if let Some(alias) = self.lookup_vector_alias(name) {
                return alias.clone();
            }
            if self.is_local(name) {
                return escape_swift_ident(name);
            }
            if self.instance_props.contains_key(name) {
                let esc = escape_swift_ident(name);
                return format!("self.{esc}");
            } else if let Some(cls) = &self.current_class_short
                && self.static_props.contains(name)
            {
                return format!("{cls}.{}", escape_swift_ident(name));
            }
            if self.global_vars.contains_key(name) {
                return escape_swift_ident(name);
            }
            if !self.source_is_as3
                && let Some(modern) = map_as2_direct_prop_to_modern(name)
            {
                return format!("self.{modern}");
            }
            if self.class_has_stage
                && matches!(
                    name.as_str(),
                    "x" | "y"
                        | "scaleX"
                        | "scaleY"
                        | "alpha"
                        | "visible"
                        | "rotation"
                        | "width"
                        | "height"
                        | "name"
                        | "blendMode"
                        | "filters"
                )
            {
                return format!("self.{name}");
            }
            return escape_swift_ident(name);
        }

        if let Expr::Member(base, prop) = lhs {
            if prop == "self" {
                let b = self.emit_lvalue_chain_base(base, offset);
                return format!("{b}[\"self\"]");
            }
            if is_root_display_expr(base) && infer_builtin_display_member_type(prop).is_none() {
                let prop_lit = encode_swift_string_literal(prop);
                return format!("self.root?[{prop_lit}]");
            }
            if is_stage_expr(base) {
                return format!("self.stage?.{}", escape_swift_ident(prop));
            }
            if !self.source_is_as3
                && let Expr::Name(cls) = base.as_ref()
                && cls == "Stage"
            {
                return format!("self.stage?.{}", escape_swift_ident(prop));
            }
            if let Expr::Name(cls) = base.as_ref()
                && !self.is_local(cls)
                && let Some(builtin_sw) = map_builtin_static_class_swift(cls)
            {
                return format!("{builtin_sw}.{}", escape_swift_ident(prop));
            }
            let b = self.emit_lvalue_chain_base(base, offset);
            let mapped = if !self.source_is_as3 {
                map_as2_direct_prop_to_modern(prop)
                    .map(str::to_owned)
                    .unwrap_or_else(|| escape_swift_ident(prop))
            } else {
                escape_swift_ident(prop)
            };
            return format!("{b}.{mapped}");
        }

        if let Expr::Index(base, idx) = lhs {
            if is_root_display_expr(base) {
                let i = self.emit_coerced_expr(idx, &HirType::String, offset);
                return format!("self.root?[{i}]");
            }
            let base_ty = self.infer(base);
            let b = self.emit_lvalue_chain_base(base, offset);
            let i = if matches!(base_ty, HirType::Vector(_))
                || matches!(&base_ty, HirType::Class(cn) if matches!(short_type_name(cn), "ByteArray" | "XML" | "XMLList"))
            {
                self.wrap_int(idx, offset)
            } else {
                self.emit_expr_with_offset(idx, offset)
            };
            return format!("{b}[{i}]");
        }

        self.emit_expr_with_offset(lhs, offset)
    }

    fn emit_lvalue_chain_base(&mut self, base: &Expr, offset: usize) -> String {
        if let Expr::Member(rb, prop) = base
            && is_root_display_expr(rb)
            && infer_builtin_display_member_type(prop).is_none()
        {
            let prop_lit = encode_swift_string_literal(prop);
            return format!("(self.root?[{prop_lit}] ?? .undefinedVal)");
        }
        if let Expr::Index(rb, idx) = base
            && is_root_display_expr(rb)
        {
            let i = self.emit_coerced_expr(idx, &HirType::String, offset);
            return format!("(self.root?[{i}] ?? .undefinedVal)");
        }
        self.emit_optional_chain_base(base, offset)
    }

    fn emit_optional_chain_base(&mut self, base: &Expr, offset: usize) -> String {
        if is_stage_expr(base) {
            return "self.stage?".into();
        }
        if is_root_display_expr(base) {
            return "self.root?".into();
        }
        if let Expr::Name(n) = base {
            if let Some(alias) = self.lookup_vector_alias(n) {
                return alias.clone();
            }
            if self.is_local(n) {
                let esc = escape_swift_ident(n);
                return if self.is_nullable_local(n) {
                    format!("{esc}?")
                } else {
                    esc
                };
            }
            if self.instance_props.contains_key(n) {
                let esc = escape_swift_ident(n);
                if self.is_nullable_prop(n) {
                    return format!("self.{esc}?");
                }
                return format!("self.{esc}");
            }
        }
        if let Expr::Member(b, p) = base
            && matches!(b.as_ref(), Expr::Name(n) if n == "this")
            && self.instance_props.contains_key(p)
        {
            let esc = escape_swift_ident(p);
            if self.is_nullable_prop(p) {
                return format!("self.{esc}?");
            }
            return format!("self.{esc}");
        }
        let s = self.emit_subexpr(base, 18, offset);
        if self.expr_produces_nfvalue(base)
            || s.starts_with("NFValue(")
            || s.starts_with("NFValue.")
        {
            if has_top_level_optional_chain(&s) {
                return format!("({s} ?? .undefinedVal)");
            }
            return s;
        }
        let needs_qmark = if has_top_level_optional_chain(&s) {
            self.expr_self_is_optional(base)
        } else {
            self.expr_produces_optional(base)
        };
        if needs_qmark && !s.ends_with('?') {
            return format!("{s}?");
        }
        s
    }

    fn match_as2_percent_lvalue_swift(
        &mut self,
        expr: &Expr,
        offset: usize,
    ) -> Option<(String, &'static str)> {
        match expr {
            Expr::Member(base, prop) => {
                let modern = map_as2_percent_prop_to_modern(prop)?;
                let b = self.emit_lvalue_chain_base(base, offset);
                Some((b, modern))
            }
            Expr::Name(prop) if !self.is_local(prop) => {
                let modern = map_as2_percent_prop_to_modern(prop)?;
                Some(("self".into(), modern))
            }
            _ => None,
        }
    }

    fn emit_scaled_100_to_01_swift(&mut self, expr: &Expr, offset: usize) -> String {
        if let Some(folded) = fold_100_to_01(expr, true) {
            return folded;
        }
        let d = self.wrap_double(expr, offset);
        if is_atomic_expr(expr) {
            format!("{d} / 100.0")
        } else {
            format!("({d}) / 100.0")
        }
    }

    fn emit_swift_member(&mut self, base: &Expr, prop: &str, offset: usize) -> String {
        if is_external_interface_base(base) && prop == "available" {
            return "true".into();
        }

        if let Expr::Name(cls) = base
            && !self.is_local(cls)
        {
            if cls == "Key"
                && let Some((_, code)) = map_as2_key_constant(prop)
            {
                return format!("UInt({code})");
            }
            if cls == "Keyboard"
                && let Some((_, code)) = map_as3_keyboard_constant(prop)
            {
                return format!("UInt({code})");
            }
            if let Some(ev) = classify_swift_event_type(&Expr::Member(
                Box::new(Expr::Name(cls.clone())),
                prop.to_owned(),
            )) {
                return format!("\"{ev}\"");
            }
            if cls == "StageScaleMode" {
                return match prop {
                    "NO_SCALE" => "\"noScale\"".into(),
                    "SHOW_ALL" => "\"showAll\"".into(),
                    "EXACT_FIT" => "\"exactFit\"".into(),
                    "NO_BORDER" => "\"noBorder\"".into(),
                    _ => format!("\"{prop}\""),
                };
            }
            if cls == "StageAlign" {
                return match prop {
                    "TOP_LEFT" => "\"TL\"".into(),
                    "TOP" => "\"T\"".into(),
                    "TOP_RIGHT" => "\"TR\"".into(),
                    "LEFT" => "\"L\"".into(),
                    "RIGHT" => "\"R\"".into(),
                    "BOTTOM_LEFT" => "\"BL\"".into(),
                    "BOTTOM" => "\"B\"".into(),
                    "BOTTOM_RIGHT" => "\"BR\"".into(),
                    _ => format!("\"{prop}\""),
                };
            }
            if cls == "BlendMode" {
                let bm = match prop {
                    "NORMAL" => "normal",
                    "LAYER" => "layer",
                    "MULTIPLY" => "multiply",
                    "SCREEN" => "screen",
                    "LIGHTEN" => "lighten",
                    "DARKEN" => "darken",
                    "DIFFERENCE" => "difference",
                    "ADD" => "add",
                    "SUBTRACT" => "subtract",
                    "INVERT" => "invert",
                    "ALPHA" => "alpha",
                    "ERASE" => "erase",
                    "OVERLAY" => "overlay",
                    "HARDLIGHT" => "hardlight",
                    _ => "normal",
                };
                return format!("\"{bm}\"");
            }
            if cls == "TextFieldType" {
                return match prop {
                    "INPUT" => "\"input\"".into(),
                    _ => "\"dynamic\"".into(),
                };
            }
            if cls == "SoundMixer" {
                return format!("NFSoundMixer.{}", escape_swift_ident(prop));
            }
            if cls == "Mouse" && matches!(prop, "lock" | "unlock") {
                return "NFValue.bool(true)".into();
            }
            if !self.source_is_as3 && cls == "Stage" {
                return match prop {
                    "width" => "(self.stage?.stageWidth ?? 0.0)".into(),
                    "height" => "(self.stage?.stageHeight ?? 0.0)".into(),
                    "scaleMode" => "(self.stage?.scaleMode ?? \"showAll\")".into(),
                    "align" => "(self.stage?.align ?? \"\")".into(),
                    _ => format!("self.stage?.{}", escape_swift_ident(prop)),
                };
            }
            if cls == "Math" {
                match prop {
                    "PI" => return "Double.pi".into(),
                    "E" => return "Foundation.M_E".into(),
                    "LN2" => return "Foundation.M_LN2".into(),
                    "LN10" => return "Foundation.M_LN10".into(),
                    "LOG2E" => return "Foundation.M_LOG2E".into(),
                    "LOG10E" => return "Foundation.M_LOG10E".into(),
                    "SQRT1_2" => return "Foundation.M_SQRT1_2".into(),
                    "SQRT2" => return "2.0.squareRoot()".into(),
                    _ => {}
                }
            }
            if let Some(cinfo) = self.lookup_class_info(cls) {
                return format!("{}.{}", cinfo.swift_name, escape_swift_ident(prop));
            }
            if let Some(builtin_sw) = map_builtin_static_class_swift(cls) {
                return format!("{builtin_sw}.{}", escape_swift_ident(prop));
            }
            if cls == "Number" {
                return format!("Double.{}", escape_swift_ident(prop));
            }
            if cls == "int" {
                return format!("Int.{}", escape_swift_ident(prop));
            }
            if cls == "uint" {
                return format!("UInt.{}", escape_swift_ident(prop));
            }
        }

        if is_stage_expr(base) {
            if prop == "stage3Ds" {
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    "Stage3D is not supported on the native Swift 2D scene target.",
                );
                return "/* FIXME [NeoFlash Swift]: stage.stage3Ds */ []".into();
            }
            return match prop {
                "stageWidth" | "stageHeight" => format!("(self.stage?.{prop} ?? 0.0)"),
                _ => format!("self.stage?.{}", escape_swift_ident(prop)),
            };
        }

        if is_root_display_expr(base) && infer_builtin_display_member_type(prop).is_none() {
            let prop_lit = encode_swift_string_literal(prop);
            return format!("(self.root?[{prop_lit}] ?? .undefinedVal)");
        }

        let b = self.emit_optional_chain_base(base, offset);
        if prop == "self" {
            return format!("{b}[\"self\"]");
        }
        if prop == "description" && self.expr_produces_nfvalue(base) {
            return format!("{b}[\"description\"]");
        }
        if !self.source_is_as3 {
            if let Some(modern) = map_as2_percent_prop_to_modern(prop) {
                if self.expr_produces_optional(base) {
                    return format!("((({b}.{modern}) ?? 0.0) * 100.0)");
                }
                return format!("(({b}.{modern}) * 100.0)");
            }
            if let Some(modern) = map_as2_direct_prop_to_modern(prop) {
                return format!("{b}.{modern}");
            }
        }
        format!("{b}.{}", escape_swift_ident(prop))
    }

    fn emit_typed_call_args(
        &mut self,
        args: &[Expr],
        expected_params: &[HirType],
        offset: usize,
    ) -> String {
        args.iter()
            .enumerate()
            .map(|(idx, arg)| {
                if let Some(pty) = expected_params.get(idx) {
                    self.emit_coerced_expr(arg, pty, offset)
                } else {
                    self.emit_expr_with_offset(arg, offset)
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn wrap_throwing_call(&self, call_sw: String, can_throw: bool) -> String {
        if !can_throw {
            return call_sw;
        }
        if self.in_try_depth > 0 || self.in_throwing_fn {
            format!("try {call_sw}")
        } else {
            format!("(try? {call_sw})")
        }
    }

    fn emit_swift_call(&mut self, callee: &Expr, args: &[Expr], offset: usize) -> String {
        // 0. `super(...)` constructor chaining -> `super.init(...)`
        if matches!(callee, Expr::Name(n) if n == "super") {
            return format!("super.init({})", self.emit_args(args, offset));
        }

        // 0b. ActionScript `ClassName(expr)` type cast (`TacticalUnit(list[i])`, `MovieClip(parent)`)
        if let Some(cast_ty) = self.match_class_cast_call(callee, args) {
            if is_null_or_undef(&args[0]) {
                return "nil".into();
            }
            let sw_ty = swift_type_for_hir(&cast_ty);
            if self.expr_produces_nfvalue(&args[0]) {
                let inner = self.emit_subexpr(&args[0], 18, offset);
                return format!("{inner}.asObject(as: {sw_ty}.self)");
            }
            let inner = self.emit_expr_with_offset(&args[0], offset);
            return format!("({inner} as? {sw_ty})");
        }

        // 1. `SwiftHost.foo(a, b)`, `fscommand("foo", a)`, `ExternalInterface.call("foo", a)` -> `self.stage?.host?.foo(...)`!
        if let Some((method, payload_args)) = match_host_proxy_call(callee, args) {
            let (call_str, ret_ty) = self.emit_host_proxy_call_inner(&method, payload_args, offset);
            if let Some(rt) = ret_ty {
                let def_expr = swift_default_for_proxy_type(&rt);
                return format!("({call_str} ?? {def_expr})");
            }
            return call_str;
        }
        if matches!(callee, Expr::Name(n) if n == "fscommand" && !self.is_local(n)) {
            self.record_issue(
                ConversionIssueKind::ManualAttention,
                offset,
                "Dynamic 'fscommand(...)' requires a string literal command name to generate a SwiftHostProtocol stub.",
            );
            return format!(
                "/* FIXME [NeoFlash Swift]: fscommand({}) */ NFValue.undefined",
                super::escape_block_comment_body(&self.emit_args(args, offset))
            );
        }
        if let Expr::Member(base, m) = callee
            && is_external_interface_base(base)
            && matches!(m.as_str(), "call" | "addCallback")
        {
            self.record_issue(
                ConversionIssueKind::ManualAttention,
                offset,
                format!("'ExternalInterface.{m}(...)' requires a static method contract or direct MainScene call in Swift."),
            );
            return format!(
                "/* FIXME [NeoFlash Swift]: ExternalInterface.{m}({}) */ NFValue.undefined",
                super::escape_block_comment_body(&self.emit_args(args, offset))
            );
        }

        // 2. Type casts, math/number builtins, `stopAllSounds`, and own methods
        if let Expr::Name(fn_name) = callee {
            match fn_name.as_str() {
                "int" if args.len() == 1 => {
                    return self.wrap_int(&args[0], offset);
                }
                "uint" if args.len() == 1 => {
                    return self.emit_coerced_expr(&args[0], &HirType::UInt, offset);
                }
                "Number" if args.len() == 1 => {
                    return self.wrap_double(&args[0], offset);
                }
                "String" if args.len() == 1 => {
                    return format!(
                        "NFRuntime.toString({})",
                        self.emit_expr_with_offset(&args[0], offset)
                    );
                }
                "Boolean" if args.len() == 1 => {
                    return self.emit_bool_cond(&args[0], offset);
                }
                "XML" if args.len() <= 1 && !self.is_local(fn_name) => {
                    let a = args
                        .first()
                        .map(|x| self.emit_expr_with_offset(x, offset))
                        .unwrap_or_default();
                    return format!("NFXML({a})");
                }
                "XMLList" if args.len() <= 1 && !self.is_local(fn_name) => {
                    let a = args
                        .first()
                        .map(|x| self.emit_expr_with_offset(x, offset))
                        .unwrap_or_default();
                    return format!("NFXMLList({a})");
                }
                "getTimer" if args.is_empty() && !self.is_local(fn_name) => {
                    return "NFRuntime.getTimer()".into();
                }
                "setTimeout" | "setInterval" if args.len() >= 2 && !self.is_local(fn_name) => {
                    let cb = self.wrap_nfvalue(&args[0], offset);
                    let delay = self.wrap_double(&args[1], offset);
                    if args.len() > 2 {
                        let extra: Vec<String> = args[2..]
                            .iter()
                            .map(|a| self.wrap_nfvalue(a, offset))
                            .collect();
                        return format!(
                            "NFRuntime.{fn_name}({cb}, {delay}, [{}])",
                            extra.join(", ")
                        );
                    }
                    return format!("NFRuntime.{fn_name}({cb}, {delay})");
                }
                "clearTimeout" | "clearInterval" if args.len() == 1 && !self.is_local(fn_name) => {
                    let id = self.wrap_int(&args[0], offset);
                    return format!("NFRuntime.{fn_name}({id})");
                }
                "escape" | "unescape" | "encodeURI" | "decodeURI" | "encodeURIComponent"
                | "decodeURIComponent"
                    if args.len() == 1 && !self.is_local(fn_name) =>
                {
                    let s = self.emit_coerced_expr(&args[0], &HirType::String, offset);
                    return format!("NFRuntime.{fn_name}({s})");
                }
                "isNaN" if args.len() == 1 => {
                    let d = self.wrap_double(&args[0], offset);
                    return format!("({d}).isNaN");
                }
                "isFinite" if args.len() == 1 => {
                    let d = self.wrap_double(&args[0], offset);
                    return format!("({d}).isFinite");
                }
                "parseInt" if !args.is_empty() => {
                    let s = self.emit_expr_with_offset(&args[0], offset);
                    if let Some(rx) = args.get(1) {
                        let r = self.wrap_int(rx, offset);
                        return format!("NFRuntime.parseInt({s}, {r})");
                    }
                    return format!("NFRuntime.parseInt({s})");
                }
                "parseFloat" if args.len() == 1 => {
                    let s = self.emit_expr_with_offset(&args[0], offset);
                    return format!("NFRuntime.parseFloat({s})");
                }
                "stopAllSounds" if args.is_empty() && !self.is_local(fn_name) => {
                    if self.class_has_stage {
                        return "self.stopAllSounds()".into();
                    }
                    return "NFRuntime.stopAllSounds()".into();
                }
                "trace" => {
                    return format!("NFRuntime.trace({})", self.emit_args(args, offset));
                }
                "eval" => {
                    self.record_issue(
                        ConversionIssueKind::ManualAttention,
                        offset,
                        "'eval()' is not supported on the native Swift target.",
                    );
                    return format!(
                        "/* FIXME [NeoFlash Swift]: eval({}) */ NFValue.undefined",
                        super::escape_block_comment_body(&self.emit_args(args, offset))
                    );
                }
                "stop"
                | "play"
                | "nextFrame"
                | "prevFrame"
                | "gotoAndPlay"
                | "gotoAndStop"
                | "addChild"
                | "removeChild"
                | "dispatchEvent"
                | "hasEventListener"
                | "createTextField"
                | "createEmptyMovieClip"
                | "getNextHighestDepth"
                | "attachMovie"
                | "attachBitmap"
                    if !self.is_local(fn_name) =>
                {
                    return format!("self.{fn_name}({})", self.emit_args(args, offset));
                }
                other if self.local_func_sigs.contains_key(other) => {
                    let sig = self.local_func_sigs[other].clone();
                    let call_args = self.emit_typed_call_args(args, &sig.params, offset);
                    return format!("{}({call_args})", escape_swift_ident(other));
                }
                other if !self.is_local(other) && self.instance_methods.contains(other) => {
                    let sig_opt = self.instance_method_sigs.get(other).cloned();
                    let call_args = if let Some(sig) = &sig_opt {
                        self.emit_typed_call_args(args, &sig.params, offset)
                    } else {
                        self.emit_args(args, offset)
                    };
                    let raw_call = format!("self.{}({call_args})", escape_swift_ident(other));
                    return self.wrap_throwing_call(raw_call, sig_opt.is_some_and(|s| s.can_throw));
                }
                other if !self.is_local(other) && self.static_methods.contains(other) => {
                    let cls = self
                        .current_class_short
                        .clone()
                        .unwrap_or_else(|| "Self".into());
                    let sig_opt = self.static_method_sigs.get(other).cloned();
                    let call_args = if let Some(sig) = &sig_opt {
                        self.emit_typed_call_args(args, &sig.params, offset)
                    } else {
                        self.emit_args(args, offset)
                    };
                    let raw_call = format!("{cls}.{}({call_args})", escape_swift_ident(other));
                    return self.wrap_throwing_call(raw_call, sig_opt.is_some_and(|s| s.can_throw));
                }
                other if !self.is_local(other) && self.global_funcs.contains_key(other) => {
                    let sig_opt = self.global_funcs.get(other).cloned();
                    let call_args = if let Some(sig) = &sig_opt {
                        self.emit_typed_call_args(args, &sig.params, offset)
                    } else {
                        self.emit_args(args, offset)
                    };
                    let raw_call = format!("{}({call_args})", escape_swift_ident(other));
                    return self.wrap_throwing_call(raw_call, sig_opt.is_some_and(|s| s.can_throw));
                }
                _ => {}
            }
        }

        // 3. `Math.*`, `SoundMixer.*`, `Key.*`, `Vector.<T>.push`, custom `.as` class methods, AS2 drawing API
        if let Expr::Member(base, method) = callee {
            if matches!(base.as_ref(), Expr::Name(n) if n == "this")
                && let Some(sig) = self.instance_method_sigs.get(method).cloned()
            {
                let call_args = self.emit_typed_call_args(args, &sig.params, offset);
                let raw_call = format!("self.{}({call_args})", escape_swift_ident(method));
                return self.wrap_throwing_call(raw_call, sig.can_throw);
            }
            if let Expr::Name(cls) = base.as_ref()
                && !self.is_local(cls)
            {
                if cls == "Math" {
                    match method.as_str() {
                        "floor" | "ceil" | "round" | "sin" | "cos" | "tan" | "asin" | "acos"
                        | "atan" | "sqrt" | "exp" | "log"
                            if args.len() == 1 =>
                        {
                            let a = self.wrap_double(&args[0], offset);
                            return format!("Foundation.{method}({a})");
                        }
                        "abs" if args.len() == 1 => {
                            let a = self.wrap_double(&args[0], offset);
                            return format!("Foundation.fabs({a})");
                        }
                        "min" | "max" if args.len() >= 2 => {
                            let mut acc = self.wrap_double(&args[0], offset);
                            for extra in &args[1..] {
                                let b = self.wrap_double(extra, offset);
                                acc = format!("{method}({acc}, {b})");
                            }
                            return acc;
                        }
                        "pow" if args.len() == 2 => {
                            let a = self.wrap_double(&args[0], offset);
                            let b = self.wrap_double(&args[1], offset);
                            return format!("Foundation.pow({a}, {b})");
                        }
                        "atan2" if args.len() == 2 => {
                            let a = self.wrap_double(&args[0], offset);
                            let b = self.wrap_double(&args[1], offset);
                            return format!("Foundation.atan2({a}, {b})");
                        }
                        "random" if args.is_empty() => {
                            return "Double.random(in: 0.0..<1.0)".into();
                        }
                        _ => {}
                    }
                }
                if cls == "SoundMixer" && method == "stopAll" {
                    return "NFSoundMixer.stopAll()".into();
                }
                if cls == "NFRuntime" && method == "imul" && args.len() == 2 {
                    let a = self.emit_expr_with_offset(&args[0], offset);
                    let b = self.emit_expr_with_offset(&args[1], offset);
                    return format!("NFRuntime.imul({a}, {b})");
                }
                if cls == "JSON" && method == "parse" && !args.is_empty() {
                    let s = self.emit_coerced_expr(&args[0], &HirType::String, offset);
                    let raw = format!("NFJSON.parse({s})");
                    return if self.in_try_depth > 0 || self.in_throwing_fn {
                        format!("try {raw}")
                    } else {
                        format!("((try? {raw}) ?? .null)")
                    };
                }
                if cls == "JSON" && method == "stringify" && !args.is_empty() {
                    let v = self.emit_expr_with_offset(&args[0], offset);
                    return format!("NFJSON.stringify({v})");
                }
                if cls == "SharedObject" && method == "getLocal" && !args.is_empty() {
                    let name_s = self.emit_coerced_expr(&args[0], &HirType::String, offset);
                    if let Some(p_arg) = args.get(1) {
                        let p_s = if is_null_or_undef(p_arg) {
                            "nil".to_owned()
                        } else {
                            self.emit_coerced_expr(p_arg, &HirType::String, offset)
                        };
                        return format!("NFSharedObject.getLocal({name_s}, {p_s})");
                    }
                    return format!("NFSharedObject.getLocal({name_s})");
                }
                if cls == "String" && method == "fromCharCode" {
                    if args.len() == 1 {
                        return format!("String.fromCharCode({})", self.wrap_int(&args[0], offset));
                    }
                    let codes = args
                        .iter()
                        .map(|a| self.wrap_int(a, offset))
                        .collect::<Vec<_>>()
                        .join(", ");
                    return format!("String.fromCharCode({codes})");
                }
                if !self.source_is_as3 && cls == "Key" {
                    match method.as_str() {
                        "getCode" => {
                            if let Some(ev) = &self.in_key_handler_event {
                                return format!("{ev}.keyCode");
                            }
                            return "0".into();
                        }
                        "isDown" if args.len() == 1 => {
                            let code = self.wrap_int(&args[0], offset);
                            if self.use_key_down_shim {
                                return format!("self._isKeyDown({code})");
                            }
                            return format!("(self.stage?.isKeyDown({code}) ?? false)");
                        }
                        _ => {}
                    }
                }
                if let Some(cinfo) = self.lookup_class_info(cls).cloned()
                    && let Some(sig) = cinfo.static_methods.get(method).cloned()
                {
                    let call_args = self.emit_typed_call_args(args, &sig.params, offset);
                    let raw_call = format!(
                        "{}.{}({call_args})",
                        cinfo.swift_name,
                        escape_swift_ident(method)
                    );
                    return self.wrap_throwing_call(raw_call, sig.can_throw);
                }
                if let Some(builtin_sw) = map_builtin_static_class_swift(cls) {
                    return format!(
                        "{builtin_sw}.{}({})",
                        escape_swift_ident(method),
                        self.emit_args(args, offset)
                    );
                }
            }

            if is_root_display_expr(base)
                && !matches!(
                    method.as_str(),
                    "addEventListener"
                        | "removeEventListener"
                        | "dispatchEvent"
                        | "hasEventListener"
                        | "localToGlobal"
                        | "globalToLocal"
                        | "getBounds"
                )
            {
                let m_lit = encode_swift_string_literal(method);
                let wrapped: Vec<String> =
                    args.iter().map(|a| self.wrap_nfvalue(a, offset)).collect();
                return format!(
                    "(self.root?.nfInvokeMethod({m_lit}, [{}]) ?? .undefinedVal)",
                    wrapped.join(", ")
                );
            }

            let base_ty = self.infer(base);
            if let HirType::Vector(elem_ty) = &base_ty {
                if matches!(method.as_str(), "push" | "unshift") {
                    let c = self.emit_subexpr(callee, 18, offset);
                    let coerced_args = args
                        .iter()
                        .map(|arg| self.emit_coerced_expr(arg, elem_ty, offset))
                        .collect::<Vec<_>>()
                        .join(", ");
                    return format!("{c}({coerced_args})");
                }
                if method == "indexOf" && !args.is_empty() {
                    let c = self.emit_subexpr(callee, 18, offset);
                    let first = self.emit_coerced_expr(&args[0], elem_ty, offset);
                    if let Some(from_arg) = args.get(1) {
                        let from_sw = self.wrap_int(from_arg, offset);
                        return format!("{c}({first}, {from_sw})");
                    }
                    return format!("{c}({first})");
                }
            }

            if self.expr_produces_nfvalue(base) || Self::is_nfvalue_type(&base_ty) {
                match method.as_str() {
                    "pop" | "shift" | "reverse" | "toString" if args.is_empty() => {
                        let b = self.emit_optional_chain_base(base, offset);
                        return format!("{b}.{method}()");
                    }
                    "join" => {
                        let b = self.emit_optional_chain_base(base, offset);
                        if let Some(sep) = args.first() {
                            let s = self.emit_coerced_expr(sep, &HirType::String, offset);
                            return format!("{b}.join({s})");
                        }
                        return format!("{b}.join()");
                    }
                    "hasOwnProperty" if !args.is_empty() => {
                        let b = self.emit_optional_chain_base(base, offset);
                        let k = self.emit_coerced_expr(&args[0], &HirType::String, offset);
                        return format!("{b}.hasOwnProperty({k})");
                    }
                    "slice" | "splice" => {
                        let b = self.emit_optional_chain_base(base, offset);
                        let mut c_args = Vec::new();
                        if let Some(a0) = args.first() {
                            c_args.push(self.wrap_int(a0, offset));
                        }
                        if let Some(a1) = args.get(1) {
                            c_args.push(self.wrap_int(a1, offset));
                        }
                        if method == "splice" && args.len() > 2 {
                            for extra in &args[2..] {
                                c_args.push(self.wrap_nfvalue(extra, offset));
                            }
                        }
                        return format!("{b}.{method}({})", c_args.join(", "));
                    }
                    "concat" => {
                        let b = self.emit_optional_chain_base(base, offset);
                        let c_args = args
                            .iter()
                            .map(|a| self.wrap_nfvalue(a, offset))
                            .collect::<Vec<_>>()
                            .join(", ");
                        return format!("{b}.concat({c_args})");
                    }
                    "push" | "unshift" | "indexOf" | "lastIndexOf" => {
                        let b = self.emit_optional_chain_base(base, offset);
                        let coerced_args = args
                            .iter()
                            .enumerate()
                            .map(|(idx, arg)| {
                                if (method == "indexOf" || method == "lastIndexOf") && idx == 1 {
                                    self.wrap_int(arg, offset)
                                } else {
                                    self.wrap_nfvalue(arg, offset)
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        return format!("{b}.{method}({coerced_args})");
                    }
                    _ => {
                        // Dynamic `@dynamicCallable` invocation on `NFValue[dynamicMember: method]`!
                        let b = self.emit_optional_chain_base(base, offset);
                        let wrapped = args
                            .iter()
                            .map(|arg| self.wrap_nfvalue(arg, offset))
                            .collect::<Vec<_>>()
                            .join(", ");
                        return format!("{b}.{}({wrapped})", escape_swift_ident(method));
                    }
                }
            }

            if let HirType::Class(cn) = &base_ty {
                if short_type_name(cn) == "ByteArray" && method == "readObject" && args.is_empty() {
                    let b = self.emit_optional_chain_base(base, offset);
                    let raw = format!("{b}.readObject()");
                    return if self.in_try_depth > 0 || self.in_throwing_fn {
                        format!("try {raw}")
                    } else {
                        format!("((try? {raw}) ?? .null)")
                    };
                }
                if let Some(cinfo) = self.lookup_class_info(cn).cloned()
                    && let Some(sig) = cinfo.instance_methods.get(method).cloned()
                {
                    let b = self.emit_optional_chain_base(base, offset);
                    let call_args = self.emit_typed_call_args(args, &sig.params, offset);
                    let raw_call = format!("{b}.{}({call_args})", escape_swift_ident(method));
                    return self.wrap_throwing_call(raw_call, sig.can_throw);
                }
            }

            if !self.expr_produces_nfvalue(base)
                && (matches!(base.as_ref(), Expr::Member(inner, g) if g == "graphics" && !self.expr_produces_nfvalue(inner))
                    || matches!(&base_ty, HirType::Class(cn) if short_type_name(cn) == "Graphics"))
                && matches!(
                    method.as_str(),
                    "moveTo"
                        | "lineTo"
                        | "curveTo"
                        | "cubicCurveTo"
                        | "drawRect"
                        | "drawRoundRect"
                        | "drawRoundRectComplex"
                        | "drawCircle"
                        | "drawEllipse"
                )
            {
                let b = self.emit_optional_chain_base(base, offset);
                let a = args
                    .iter()
                    .map(|arg| self.emit_coerced_expr(arg, &HirType::Float, offset))
                    .collect::<Vec<_>>()
                    .join(", ");
                return format!("{b}.{method}({a})");
            }

            if !self.source_is_as3
                && (matches!(base_ty, HirType::MovieClip | HirType::Sprite)
                    || matches!(base.as_ref(), Expr::Name(n) if n == "this")
                    || matches!(&base_ty, HirType::Class(cn) if self.symbol_id_by_swift_class.contains_key(cn.as_str())))
                && matches!(
                    method.as_str(),
                    "clear"
                        | "beginFill"
                        | "lineStyle"
                        | "moveTo"
                        | "lineTo"
                        | "curveTo"
                        | "endFill"
                )
                && !matches!(base.as_ref(), Expr::Member(_, g) if g == "graphics")
            {
                let b = self.emit_optional_chain_base(base, offset);
                let mapped_args = match method.as_str() {
                    "beginFill" if args.len() >= 2 => format!(
                        "{}, {}",
                        self.emit_expr_with_offset(&args[0], offset),
                        self.emit_scaled_100_to_01_swift(&args[1], offset)
                    ),
                    "lineStyle" if args.len() >= 3 => format!(
                        "{}, {}, {}",
                        self.emit_coerced_expr(&args[0], &HirType::Float, offset),
                        self.emit_expr_with_offset(&args[1], offset),
                        self.emit_scaled_100_to_01_swift(&args[2], offset)
                    ),
                    "lineStyle" if !args.is_empty() => {
                        let mut parts =
                            vec![self.emit_coerced_expr(&args[0], &HirType::Float, offset)];
                        for rest in &args[1..] {
                            parts.push(self.emit_expr_with_offset(rest, offset));
                        }
                        parts.join(", ")
                    }
                    "moveTo" | "lineTo" | "curveTo" => args
                        .iter()
                        .map(|arg| self.emit_coerced_expr(arg, &HirType::Float, offset))
                        .collect::<Vec<_>>()
                        .join(", "),
                    _ => self.emit_args(args, offset),
                };
                return format!("{b}.graphics.{method}({mapped_args})");
            }
        }

        if self.callee_is_nfvalue_callable(callee) {
            let c = self.emit_subexpr(callee, 18, offset);
            let wrapped = args
                .iter()
                .map(|a| self.wrap_nfvalue(a, offset))
                .collect::<Vec<_>>()
                .join(", ");
            return format!("{c}({wrapped})");
        }
        let c = self.emit_subexpr(callee, 18, offset);
        let a = self.emit_args(args, offset);
        format!("{c}({a})")
    }

    fn callee_is_nfvalue_callable(&self, callee: &Expr) -> bool {
        match callee {
            Expr::Member(base, method) => {
                if is_stage_expr(base) {
                    return false;
                }
                if matches!(base.as_ref(), Expr::Name(n) if n == "this") {
                    return !self.instance_methods.contains(method)
                        && self
                            .instance_props
                            .get(method)
                            .is_some_and(Self::is_nfvalue_type);
                }
                if self.expr_produces_nfvalue(base) {
                    return true;
                }
                if matches!(
                    method.as_str(),
                    "onLoad"
                        | "onData"
                        | "onStatus"
                        | "onMetaData"
                        | "onSoundComplete"
                        | "onChanged"
                        | "onSetFocus"
                        | "onKillFocus"
                ) {
                    return true;
                }
                let bty = self.infer(base);
                if let HirType::Class(cn) = &bty
                    && let Some(cinfo) = self.lookup_class_info(cn)
                {
                    return !cinfo.instance_methods.contains_key(method)
                        && cinfo
                            .instance_fields
                            .get(method)
                            .is_some_and(Self::is_nfvalue_type);
                }
                false
            }
            Expr::Name(n) => {
                if self.local_func_sigs.contains_key(n) {
                    return false;
                }
                self.expr_produces_nfvalue(callee)
            }
            _ => self.expr_produces_nfvalue(callee),
        }
    }

    fn emit_swift_new(&mut self, ty: &str, args: &[Expr], offset: usize) -> String {
        let trimmed = ty.trim();
        if let Some(elem) = trimmed
            .strip_prefix("Vector.<")
            .or_else(|| trimmed.strip_prefix("__AS3__.vec.Vector.<"))
            .and_then(|s| s.strip_suffix('>'))
        {
            return self.emit_swift_new_vector(elem, args, offset);
        }
        let short = short_type_name(trimmed);
        match short {
            "ByteArray" => "NFByteArray()".into(),
            "Stage3D" | "Context3D" => {
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    format!("{short} is not supported on the native Swift target."),
                );
                format!("/* FIXME [NeoFlash Swift]: new {short}() */ NFValue.null")
            }
            "Array" => {
                if args.len() > 1 {
                    let parts: Vec<String> =
                        args.iter().map(|a| self.wrap_nfvalue(a, offset)).collect();
                    format!("NFValue.fromArray([{}])", parts.join(", "))
                } else {
                    "NFValue.fromArray([])".into()
                }
            }
            "Object" | "Dictionary" => "NFValue.fromObject([])".into(),
            "Date" => {
                if args.is_empty() {
                    "NFDate()".into()
                } else if args.len() == 1 {
                    format!("NFDate({})", self.emit_expr_with_offset(&args[0], offset))
                } else {
                    let ints = args
                        .iter()
                        .map(|a| self.wrap_int(a, offset))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("NFDate({ints})")
                }
            }
            "RegExp" => {
                let pat = args
                    .first()
                    .map(|a| self.emit_coerced_expr(a, &HirType::String, offset))
                    .unwrap_or_else(|| "\"\"".into());
                let flg = args
                    .get(1)
                    .map(|a| self.emit_coerced_expr(a, &HirType::String, offset))
                    .unwrap_or_else(|| "\"\"".into());
                format!("NFRegExp({pat}, {flg})")
            }
            "Timer" => {
                let delay = args
                    .first()
                    .map(|a| self.wrap_double(a, offset))
                    .unwrap_or_else(|| "100.0".into());
                let rep = args
                    .get(1)
                    .map(|a| self.wrap_int(a, offset))
                    .unwrap_or_else(|| "0".into());
                format!("NFTimer({delay}, {rep})")
            }
            "BitmapData" => {
                let w = args
                    .first()
                    .map(|a| self.wrap_int(a, offset))
                    .unwrap_or_else(|| "1".into());
                let h = args
                    .get(1)
                    .map(|a| self.wrap_int(a, offset))
                    .unwrap_or_else(|| "1".into());
                let tr = args
                    .get(2)
                    .map(|a| self.emit_coerced_expr(a, &HirType::Bool, offset))
                    .unwrap_or_else(|| "true".into());
                let col = args
                    .get(3)
                    .map(|a| self.emit_coerced_expr(a, &HirType::UInt, offset))
                    .unwrap_or_else(|| "0xFFFFFFFF".into());
                format!("NFBitmapData({w}, {h}, {tr}, UInt32(truncatingIfNeeded: {col}))")
            }
            "Point" => {
                let x = args
                    .first()
                    .map(|a| self.wrap_double(a, offset))
                    .unwrap_or_else(|| "0.0".into());
                let y = args
                    .get(1)
                    .map(|a| self.wrap_double(a, offset))
                    .unwrap_or_else(|| "0.0".into());
                format!("NFPoint({x}, {y})")
            }
            "Rectangle" => {
                let x = args
                    .first()
                    .map(|a| self.wrap_double(a, offset))
                    .unwrap_or_else(|| "0.0".into());
                let y = args
                    .get(1)
                    .map(|a| self.wrap_double(a, offset))
                    .unwrap_or_else(|| "0.0".into());
                let w = args
                    .get(2)
                    .map(|a| self.wrap_double(a, offset))
                    .unwrap_or_else(|| "0.0".into());
                let h = args
                    .get(3)
                    .map(|a| self.wrap_double(a, offset))
                    .unwrap_or_else(|| "0.0".into());
                format!("NFRectangle({x}, {y}, {w}, {h})")
            }
            "Error" | "TypeError" | "RangeError" | "ArgumentError" | "ReferenceError"
            | "SyntaxError" | "VerifyError" | "SecurityError" | "EOFError" | "IOError" => {
                let msg = args
                    .first()
                    .map(|a| self.emit_coerced_expr(a, &HirType::String, offset))
                    .unwrap_or_else(|| "\"\"".into());
                let code = args
                    .get(1)
                    .map(|a| self.wrap_int(a, offset))
                    .unwrap_or_else(|| "0".into());
                format!("NFScriptError({msg}, {code})")
            }
            "SoundTransform" => {
                let vol = args
                    .first()
                    .map(|a| self.emit_coerced_expr(a, &HirType::Float, offset))
                    .unwrap_or_else(|| "1.0".into());
                let pan = args
                    .get(1)
                    .map(|a| self.emit_coerced_expr(a, &HirType::Float, offset))
                    .unwrap_or_else(|| "0.0".into());
                format!("NFSoundTransform({vol}, {pan})")
            }
            "DropShadowFilter" => {
                let dist = args
                    .first()
                    .map(|a| self.emit_coerced_expr(a, &HirType::Float, offset))
                    .unwrap_or_else(|| "4.0".into());
                let angle = args
                    .get(1)
                    .map(|a| self.emit_coerced_expr(a, &HirType::Float, offset))
                    .unwrap_or_else(|| "45.0".into());
                let color = args
                    .get(2)
                    .map(|a| self.emit_coerced_expr(a, &HirType::UInt, offset))
                    .unwrap_or_else(|| "0".into());
                let alpha = args
                    .get(3)
                    .map(|a| self.emit_coerced_expr(a, &HirType::Float, offset))
                    .unwrap_or_else(|| "1.0".into());
                let bx = args
                    .get(4)
                    .map(|a| self.emit_coerced_expr(a, &HirType::Float, offset))
                    .unwrap_or_else(|| "4.0".into());
                let by = args
                    .get(5)
                    .map(|a| self.emit_coerced_expr(a, &HirType::Float, offset))
                    .unwrap_or_else(|| "4.0".into());
                format!(
                    "NFFilterSpec.makeDropShadow({dist}, {angle}, UInt32(truncatingIfNeeded: {color}), {alpha}, {bx}, {by})"
                )
            }
            "GlowFilter" => {
                let color = args
                    .first()
                    .map(|a| self.emit_coerced_expr(a, &HirType::UInt, offset))
                    .unwrap_or_else(|| "0xFF0000".into());
                let alpha = args
                    .get(1)
                    .map(|a| self.emit_coerced_expr(a, &HirType::Float, offset))
                    .unwrap_or_else(|| "1.0".into());
                let bx = args
                    .get(2)
                    .map(|a| self.emit_coerced_expr(a, &HirType::Float, offset))
                    .unwrap_or_else(|| "6.0".into());
                let by = args
                    .get(3)
                    .map(|a| self.emit_coerced_expr(a, &HirType::Float, offset))
                    .unwrap_or_else(|| "6.0".into());
                format!(
                    "NFFilterSpec.makeGlow(UInt32(truncatingIfNeeded: {color}), {alpha}, {bx}, {by})"
                )
            }
            "BlurFilter" => {
                let bx = args
                    .first()
                    .map(|a| self.emit_coerced_expr(a, &HirType::Float, offset))
                    .unwrap_or_else(|| "4.0".into());
                let by = args
                    .get(1)
                    .map(|a| self.emit_coerced_expr(a, &HirType::Float, offset))
                    .unwrap_or_else(|| "4.0".into());
                format!("NFFilterSpec.makeBlur({bx}, {by})")
            }
            _ => {
                if let Some(cinfo) = self.lookup_class_info(trimmed).cloned() {
                    let call_args = self.emit_typed_call_args(args, &cinfo.ctor_params, offset);
                    return format!("{}({call_args})", cinfo.swift_name);
                }
                let sw_ty = swift_type_for_hir(&HirType::from_annotation(trimmed));
                format!("{sw_ty}({})", self.emit_args(args, offset))
            }
        }
    }

    fn emit_args(&mut self, args: &[Expr], offset: usize) -> String {
        args.iter()
            .map(|a| self.emit_expr_with_offset(a, offset))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn emit_subexpr(&mut self, expr: &Expr, parent_prec: u8, offset: usize) -> String {
        let my_prec = match expr {
            Expr::Assign(_, _, _) => 2,
            Expr::Conditional(_, _, _) => 3,
            Expr::Binary(op, _, _) => op_prec(op),
            Expr::TypeTest(_, _, _, _) | Expr::InstanceOf(_, _) => 10,
            Expr::Unary(_, _) | Expr::Delete(_, _) => 16,
            _ => 19,
        };
        let s = self.emit_expr_with_offset(expr, offset);
        if my_prec < parent_prec {
            format!("({s})")
        } else {
            s
        }
    }
}

fn infer_builtin_display_member_type(prop: &str) -> Option<HirType> {
    match prop {
        "stage" => Some(HirType::Class("Stage".into())),
        "parent" | "root" | "mask" | "_parent" => Some(HirType::MovieClip),
        "graphics" => Some(HirType::Class("Graphics".into())),
        "blendMode" => Some(HirType::Class("NFBlendMode".into())),
        "filters" => Some(HirType::Vector(Box::new(HirType::Class(
            "BitmapFilter".into(),
        )))),
        "x" | "y" | "scaleX" | "scaleY" | "alpha" | "rotation" | "width" | "height" | "mouseX"
        | "mouseY" | "_x" | "_y" | "_xscale" | "_yscale" | "_alpha" | "_rotation" | "_width"
        | "_height" | "_xmouse" | "_ymouse" => Some(HirType::Float),
        "visible" | "_visible" | "buttonMode" | "useHandCursor" | "tabEnabled"
        | "doubleClickEnabled" | "mouseEnabled" | "mouseChildren" | "enabled" | "selectable"
        | "multiline" | "wordWrap" | "border" | "background" | "displayAsPassword"
        | "embedFonts" => Some(HirType::Bool),
        "currentFrame" | "totalFrames" | "numChildren" | "_currentframe" | "_totalframes"
        | "maxChars" | "tabIndex" => Some(HirType::Int),
        "name" | "_name" | "currentLabel" | "currentFrameLabel" => Some(HirType::String),
        "type" => Some(HirType::Class("NFTextKind".into())),
        "scrollRect" | "scale9Grid" => Some(HirType::Class("Rectangle".into())),
        "defaultTextFormat" => Some(HirType::Class("TextFormat".into())),
        _ => None,
    }
}

fn is_root_display_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Name(n) => matches!(n.as_str(), "root" | "_root" | "_level0"),
        Expr::Member(b, p) if matches!(b.as_ref(), Expr::Name(n) if n == "this") => {
            matches!(p.as_str(), "root" | "_root")
        }
        Expr::TypeTest(op, inner, _, _) if op == "as" => is_root_display_expr(inner),
        _ => false,
    }
}

fn map_builtin_static_class_swift(cls: &str) -> Option<&'static str> {
    match cls {
        "SoundMixer" => Some("NFSoundMixer"),
        "SharedObject" => Some("NFSharedObject"),
        "SharedObjectFlushStatus" => Some("NFSharedObjectFlushStatus"),
        "BitmapData" => Some("NFBitmapData"),
        "PixelSnapping" => Some("NFPixelSnapping"),
        "ByteArray" => Some("NFByteArray"),
        "Endian" => Some("NFEndian"),
        "CompressionAlgorithm" => Some("NFCompressionAlgorithm"),
        "ObjectEncoding" => Some("NFObjectEncoding"),
        "JSON" => Some("NFJSON"),
        "Date" => Some("NFDate"),
        "XML" => Some("NFXML"),
        "XMLList" => Some("NFXMLList"),
        "XMLDocument" => Some("NFXMLDocument"),
        "TimerEvent" => Some("NFTimerEvent"),
        "Event" => Some("NFEvent"),
        "MouseEvent" => Some("NFMouseEvent"),
        "KeyboardEvent" => Some("NFKeyboardEvent"),
        "TouchEvent" => Some("NFTouchEvent"),
        "ErrorEvent" => Some("NFErrorEvent"),
        "AsyncErrorEvent" => Some("NFAsyncErrorEvent"),
        "IOErrorEvent" => Some("NFIOErrorEvent"),
        "SecurityErrorEvent" => Some("NFSecurityErrorEvent"),
        "ProgressEvent" => Some("NFProgressEvent"),
        "HTTPStatusEvent" => Some("NFHTTPStatusEvent"),
        "NetStatusEvent" => Some("NFNetStatusEvent"),
        "TextEvent" => Some("NFTextEvent"),
        "FocusEvent" => Some("NFFocusEvent"),
        "URLLoaderDataFormat" => Some("NFURLLoaderDataFormat"),
        "URLVariables" => Some("NFURLVariables"),
        "TextFieldAutoSize" => Some("NFTextFieldAutoSize"),
        "TextFormatAlign" => Some("NFTextFormatAlign"),
        "AntiAliasType" => Some("NFAntiAliasType"),
        "GridFitType" => Some("NFGridFitType"),
        "GradientType" => Some("NFGradientType"),
        "SpreadMethod" => Some("NFSpreadMethod"),
        "InterpolationMethod" => Some("NFInterpolationMethod"),
        "GraphicsPathWinding" => Some("NFGraphicsPathWinding"),
        "GraphicsPathCommand" => Some("NFGraphicsPathCommand"),
        "BitmapFilterType" => Some("NFBitmapFilterType"),
        "PNGEncoderOptions" => Some("NFPNGEncoderOptions"),
        "JPEGEncoderOptions" => Some("NFJPEGEncoderOptions"),
        "Capabilities" => Some("NFCapabilities"),
        "System" => Some("NFSystem"),
        "Mouse" => Some("NFMouse"),
        "Selection" => Some("NFSelection"),
        "DepthManager" => Some("NFDepthManager"),
        "LocaleID" | "Locale" => Some("NFLocale"),
        "Camera" => Some("NFCamera"),
        "Microphone" => Some("NFMicrophone"),
        _ => None,
    }
}

type BoundSwiftClassContext<'a> = (
    &'a Symbol,
    Vec<LinkedStageBinding>,
    String,
    String,
    BTreeMap<String, HirType>,
);

#[allow(clippy::too_many_arguments)]
fn emit_swift_class(
    parsed: &ParsedClass,
    cls_name: &str,
    location: &str,
    bound_ctx: Option<BoundSwiftClassContext<'_>>,
    stage: &ProjectStageIndex,
    symbol_id_by_swift_class: &BTreeMap<String, String>,
    host_methods: &BTreeMap<String, SwiftProxyMethod>,
    class_registry: &BTreeMap<String, SwiftClassInfo>,
    global_vars: &BTreeMap<String, HirType>,
    global_funcs: &BTreeMap<String, SwiftCallableSig>,
) -> (String, Vec<ConversionIssue>) {
    let mut emitter = SwiftEmitter::new(
        location,
        &parsed.source,
        parsed.language == ScriptLanguage::ActionScript3 || is_js_or_ts(parsed.language),
        stage,
        symbol_id_by_swift_class,
        host_methods,
        class_registry,
        global_vars,
        global_funcs,
        bound_ctx.as_ref().map(|(sym, _, _, _, _)| sym.id.clone()),
        Some(parsed.qualified_name.clone()),
    );
    emitter.current_class_short = Some(cls_name.to_owned());

    let is_linked_symbol = bound_ctx.is_some();
    let extends_nf_dispatcher =
        is_linked_symbol || inherits_nf_event_dispatcher(parsed.base_class.as_deref(), stage);
    let extends_display_object = is_linked_symbol
        || parsed.base_class.as_deref().is_some_and(|base| {
            matches!(
                HirType::from_annotation(base),
                HirType::MovieClip | HirType::Sprite | HirType::Button | HirType::TextField
            ) || matches!(
                short_type_name(base),
                "DisplayObject"
                    | "InteractiveObject"
                    | "DisplayObjectContainer"
                    | "Shape"
                    | "Bitmap"
                    | "Video"
                    | "Loader"
            )
        });
    emitter.class_has_stage = extends_display_object;

    let mut cls_props = BTreeMap::new();
    let mut stage_binding_by_name: BTreeMap<String, LinkedStageBinding> = BTreeMap::new();
    if let Some((_, ref bindings, _, _, ref extra_prop_types)) = bound_ctx {
        for (k, v) in extra_prop_types {
            cls_props.insert(k.clone(), v.clone());
        }
        for b in bindings {
            cls_props.insert(b.raw_name.clone(), b.hir_type.clone());
            emitter.stage_child_props.insert(b.raw_name.clone());
            stage_binding_by_name.insert(b.raw_name.clone(), b.clone());
        }
    }

    for f in &parsed.fields {
        let ty =
            f.ty.as_deref()
                .map_or(HirType::Unknown, HirType::from_annotation);
        if f.is_static {
            emitter.static_props.insert(f.name.clone());
        } else if !stage_binding_by_name.contains_key(&f.name) {
            cls_props.insert(f.name.clone(), ty);
        }
    }
    for m in &parsed.methods {
        let Some(mn) = &m.function.name else {
            continue;
        };
        if mn == &parsed.short_name && m.accessor.is_none() && !m.is_static {
            continue;
        }
        if let Some(acc) = m.accessor {
            let prop_ty = match acc {
                AccessorKind::Get => m
                    .function
                    .returns
                    .as_deref()
                    .map_or(HirType::Unknown, HirType::from_annotation),
                AccessorKind::Set => m
                    .function
                    .params
                    .first()
                    .and_then(|(_, t)| t.as_deref())
                    .map_or(HirType::Unknown, HirType::from_annotation),
            };
            if m.is_static {
                emitter.static_props.insert(mn.clone());
            } else {
                cls_props.entry(mn.clone()).or_insert(prop_ty);
            }
            continue;
        }
        let param_tys: Vec<HirType> = m
            .function
            .params
            .iter()
            .map(|(_, t)| {
                t.as_deref()
                    .map_or(HirType::Unknown, HirType::from_annotation)
            })
            .collect();
        let rty = m
            .function
            .returns
            .as_deref()
            .map_or(HirType::Void, HirType::from_annotation);
        let return_is_optional = !parsed.is_interface
            && !m.is_override
            && is_swift_reference_hir_type(&rty)
            && stmts_return_null(&m.function.body);
        let can_throw = !parsed.is_interface
            && !m.is_override
            && mn != "onResize"
            && stmts_can_throw_uncaught(&m.function.body);
        let sig = SwiftCallableSig {
            params: param_tys,
            return_ty: rty.clone(),
            return_is_optional,
            can_throw,
        };
        if m.is_static {
            emitter.static_methods.insert(mn.clone());
            emitter.static_method_sigs.insert(mn.clone(), sig);
        } else {
            emitter.instance_methods.insert(mn.clone());
            emitter.instance_method_sigs.insert(mn.clone(), sig);
            emitter.env.functions.insert(mn.clone(), rty);
        }
    }
    emitter.set_instance_props(cls_props.clone());

    if parsed.is_interface {
        let mut reqs = Vec::new();
        let mut iface_accessors: BTreeMap<String, (bool, bool, HirType)> = BTreeMap::new();
        for m in &parsed.methods {
            if let Some(acc) = m.accessor {
                let raw_name = m.function.name.as_deref().unwrap_or("prop").to_owned();
                let pty = match acc {
                    AccessorKind::Get => m
                        .function
                        .returns
                        .as_deref()
                        .map_or(HirType::Unknown, HirType::from_annotation),
                    AccessorKind::Set => m
                        .function
                        .params
                        .first()
                        .and_then(|(_, t)| t.as_deref())
                        .map_or(HirType::Unknown, HirType::from_annotation),
                };
                let entry = iface_accessors
                    .entry(raw_name)
                    .or_insert_with(|| (false, false, pty.clone()));
                match acc {
                    AccessorKind::Get => {
                        entry.0 = true;
                        entry.2 = pty;
                    }
                    AccessorKind::Set => entry.1 = true,
                }
            }
        }
        for (raw_name, (has_get, has_set, pty)) in iface_accessors {
            let fname = escape_swift_ident(&raw_name);
            let sw_ty = swift_type_for_hir(&pty);
            let spec = if has_get && has_set {
                "{ get set }"
            } else {
                "{ get }"
            };
            reqs.push(format!("    var {fname}: {sw_ty} {spec}"));
        }
        for m in &parsed.methods {
            if m.accessor.is_some() {
                continue;
            }
            let fname = escape_swift_ident(m.function.name.as_deref().unwrap_or("method"));
            emitter.push_scope();
            let params = emitter.emit_swift_params(&m.function, m.function.offset, false);
            emitter.pop_scope();
            let rty = m
                .function
                .returns
                .as_deref()
                .map_or(HirType::Void, HirType::from_annotation);
            let ret = if rty == HirType::Void {
                String::new()
            } else {
                format!(" -> {}", swift_type_for_hir(&rty))
            };
            reqs.push(format!("    func {fname}({params}){ret}"));
        }
        let code = format!(
            "@MainActor\npublic protocol {cls_name}: AnyObject {{\n{}\n}}",
            reqs.join("\n")
        );
        return (code, emitter.issues);
    }

    let mut members = Vec::new();
    let mut emitted_stage_props = BTreeSet::new();

    for f in &parsed.fields {
        if !f.is_static
            && let Some(sb) = stage_binding_by_name.get(&f.name)
            && (f.value.is_none() || f.value.as_ref().is_some_and(is_null_or_undef))
        {
            emitted_stage_props.insert(f.name.clone());
            members.push(format!(
                "    public let {}: {} = {}",
                sb.swift_prop_name, sb.swift_type, sb.init_expr
            ));
            continue;
        }
        let fname = escape_swift_ident(&f.name);
        let ty =
            f.ty.as_deref()
                .map_or(HirType::Unknown, HirType::from_annotation);
        let stat = if f.is_static { "static " } else { "" };
        let kw = if f.is_const { "let" } else { "var" };
        if let Some(val) = &f.value {
            if matches!(
                ty,
                HirType::TextField
                    | HirType::MovieClip
                    | HirType::Sprite
                    | HirType::Button
                    | HirType::Class(_)
            ) && is_null_or_undef(val)
            {
                let sw_ty = format!("{}?", swift_type_for_hir(&ty));
                members.push(format!("    public {stat}{kw} {fname}: {sw_ty} = nil"));
            } else {
                let sw_ty = swift_type_for_hir(&ty);
                let vc = emitter.emit_coerced_expr(val, &ty, f.offset);
                members.push(format!("    public {stat}{kw} {fname}: {sw_ty} = {vc}"));
            }
        } else {
            let (sw_ty, def_val) = swift_property_type_and_default(&ty);
            members.push(format!("    public {stat}var {fname}: {sw_ty} = {def_val}"));
        }
    }

    if let Some((_, ref bindings, ref extra_props, _, _)) = bound_ctx {
        for sb in bindings {
            if !emitted_stage_props.contains(&sb.raw_name) {
                members.push(format!(
                    "    public let {}: {} = {}",
                    sb.swift_prop_name, sb.swift_type, sb.init_expr
                ));
            }
        }
        if !extra_props.trim().is_empty() {
            for line in extra_props.lines() {
                if !line.trim().is_empty() {
                    members.push(format!("    {}", line.trim()));
                }
            }
        }
    }

    // Group getter/setter pairs by `(is_static, name)` so Swift emits one unified computed `var`!
    type AccessorPair<'m> = (Option<&'m ParsedMethod>, Option<&'m ParsedMethod>);
    let mut accessor_pairs: BTreeMap<(bool, String), AccessorPair<'_>> = BTreeMap::new();
    for m in &parsed.methods {
        if let Some(acc) = m.accessor {
            let raw_name = m.function.name.as_deref().unwrap_or("prop").to_owned();
            let entry = accessor_pairs.entry((m.is_static, raw_name)).or_default();
            match acc {
                AccessorKind::Get => entry.0 = Some(m),
                AccessorKind::Set => entry.1 = Some(m),
            }
        }
    }
    let mut emitted_accessors: BTreeSet<(bool, String)> = BTreeSet::new();
    let mut has_explicit_ctor = false;
    let has_nonempty_base = is_linked_symbol
        || parsed
            .base_class
            .as_deref()
            .is_some_and(|b| short_type_name(b) != "Object");

    for m in &parsed.methods {
        let raw_name = m.function.name.as_deref().unwrap_or(&parsed.short_name);
        let is_ctor = raw_name == parsed.short_name && m.accessor.is_none() && !m.is_static;
        let fname = escape_swift_ident(raw_name);
        let stat = if m.is_static && !is_ctor {
            "static "
        } else {
            ""
        };
        let ovr = if m.is_override && !is_ctor && !m.is_static {
            "override "
        } else {
            ""
        };

        emitter.in_static_method = m.is_static && !is_ctor;
        if let Some(_acc) = m.accessor {
            let key = (m.is_static, raw_name.to_owned());
            if !emitted_accessors.insert(key.clone()) {
                continue;
            }
            let (getter_opt, setter_opt) = accessor_pairs.get(&key).copied().unwrap_or_default();
            let prop_hir = getter_opt
                .and_then(|g| g.function.returns.as_deref())
                .or_else(|| {
                    setter_opt
                        .and_then(|s| s.function.params.first().and_then(|(_, t)| t.as_deref()))
                })
                .map_or(HirType::Unknown, HirType::from_annotation);
            let prop_sw_ty = swift_type_for_hir(&prop_hir);
            let mut clauses = Vec::new();
            if let Some(g) = getter_opt {
                emitter.current_return_type = prop_hir.clone();
                emitter.push_scope();
                emitter.indent = 3;
                let gbody = emitter.emit_function_body(&g.function.body);
                emitter.indent = 0;
                emitter.pop_scope();
                emitter.current_return_type = HirType::Void;
                clauses.push(format!("        get {{\n{gbody}\n        }}"));
            } else {
                let def = swift_nonoptional_default_expr(&prop_hir);
                clauses.push(format!("        get {{ {def} }}"));
            }
            if let Some(s) = setter_opt {
                let sparam = s
                    .function
                    .params
                    .first()
                    .map(|(n, _)| escape_swift_ident(n))
                    .unwrap_or_else(|| "newValue".into());
                let raw_sparam = s
                    .function
                    .params
                    .first()
                    .map(|(n, _)| n.clone())
                    .unwrap_or_else(|| "newValue".into());
                emitter.push_scope();
                emitter.mark_local(&raw_sparam);
                emitter
                    .current_fn_params
                    .push((raw_sparam.clone(), prop_hir.clone()));
                emitter.env.define(raw_sparam, prop_hir.clone());
                emitter.indent = 3;
                let sbody = emitter.emit_function_body(&s.function.body);
                emitter.indent = 0;
                emitter.pop_scope();
                clauses.push(format!("        set({sparam}) {{\n{sbody}\n        }}"));
            }
            members.push(format!(
                "    public {stat}{ovr}var {fname}: {prop_sw_ty} {{\n{}\n    }}",
                clauses.join("\n")
            ));
            continue;
        }

        if is_ctor {
            has_explicit_ctor = true;
            let ctor_has_required_params = (0..m.function.params.len()).any(|idx| {
                m.function
                    .defaults
                    .get(idx)
                    .and_then(Option::as_ref)
                    .is_none()
            });
            emitter.current_return_type = HirType::Void;
            emitter.current_return_is_optional = false;
            emitter.push_scope();
            let params = emitter.emit_swift_params(&m.function, m.function.offset, true);
            emitter.indent = 2;
            let body = emitter.emit_function_body(&m.function.body);
            emitter.indent = 0;
            emitter.pop_scope();

            let has_explicit_super = m.function.body.iter().any(|s| {
                matches!(
                    s.unlocated(),
                    Stmt::Expr(Expr::Call(c, _, _)) if matches!(c.as_ref(), Expr::Name(n) if n == "super")
                )
            });

            if let Some((bound_sym, _, _, _, _)) = &bound_ctx
                && params.is_empty()
            {
                let sym_name_lit = encode_swift_string_literal(&bound_sym.name);
                let clean_body = if has_explicit_super {
                    body.lines()
                        .filter(|l| !l.trim().starts_with("super.init("))
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    body
                };
                let mut ctor_lines = vec![
                    "        super.init(name: name)".to_owned(),
                    "        self._nfConfigureLinkedSymbol()".to_owned(),
                ];
                if !clean_body.trim().is_empty() {
                    ctor_lines.push(clean_body);
                }
                members.push(format!(
                    "    public override init(name: String = {sym_name_lit}) {{\n{}\n    }}",
                    ctor_lines.join("\n")
                ));
            } else {
                let mut ctor_lines = Vec::new();
                if has_nonempty_base && !has_explicit_super {
                    ctor_lines.push("        super.init()".into());
                }
                if is_linked_symbol {
                    ctor_lines.push("        self._nfConfigureLinkedSymbol()".into());
                }
                if !body.trim().is_empty() {
                    ctor_lines.push(body);
                }
                let overrides_event_init = parsed
                    .base_class
                    .as_deref()
                    .is_some_and(|b| short_type_name(b) == "Event")
                    && m.function.params.len() == 3
                    && matches!(
                        m.function.params[0]
                            .1
                            .as_deref()
                            .map(HirType::from_annotation),
                        Some(HirType::String)
                    )
                    && matches!(
                        m.function.params[1]
                            .1
                            .as_deref()
                            .map(HirType::from_annotation),
                        Some(HirType::Bool)
                    )
                    && matches!(
                        m.function.params[2]
                            .1
                            .as_deref()
                            .map(HirType::from_annotation),
                        Some(HirType::Bool)
                    );
                let init_ovr =
                    if (has_nonempty_base && params.is_empty() && !extends_display_object)
                        || overrides_event_init
                    {
                        "override "
                    } else {
                        ""
                    };
                members.push(format!(
                    "    public {init_ovr}init({params}) {{\n{}\n    }}",
                    ctor_lines.join("\n")
                ));
                if ctor_has_required_params && !is_linked_symbol {
                    let fb_init = if extends_display_object {
                        "    public override init(name: String = \"\") {\n        super.init(name: name)\n    }"
                    } else if has_nonempty_base {
                        "    public override init() {\n        super.init()\n    }"
                    } else {
                        "    public init() {}"
                    };
                    members.push(fb_init.into());
                }
            }
        } else {
            let rty = m
                .function
                .returns
                .as_deref()
                .map_or(HirType::Void, HirType::from_annotation);
            let ret_opt = !m.is_override
                && emitter.is_swift_reference_class(&rty)
                && stmts_return_null(&m.function.body);
            let can_throw = !m.is_override
                && raw_name != "onResize"
                && stmts_can_throw_uncaught(&m.function.body);
            emitter.current_return_type = rty.clone();
            emitter.current_return_is_optional = ret_opt;
            emitter.in_throwing_fn = can_throw;
            emitter.push_scope();
            let params = emitter.emit_swift_params(&m.function, m.function.offset, true);
            emitter.indent = 2;
            let body = emitter.emit_function_body(&m.function.body);
            emitter.indent = 0;
            emitter.pop_scope();
            emitter.current_return_type = HirType::Void;
            emitter.current_return_is_optional = false;
            emitter.in_throwing_fn = false;

            let throws_kw = if can_throw { " throws" } else { "" };
            let (discardable, ret) = if rty == HirType::Void {
                ("", String::new())
            } else {
                let opt_suf = if ret_opt { "?" } else { "" };
                (
                    "    @discardableResult\n",
                    format!(" -> {}{opt_suf}", swift_type_for_hir(&rty)),
                )
            };
            members.push(format!(
                "{discardable}    public {stat}{ovr}func {fname}({params}){throws_kw}{ret} {{\n{body}\n    }}"
            ));
        }
    }

    emitter.in_static_method = false;
    if !has_explicit_ctor {
        if let Some((bound_sym, _, _, _, _)) = &bound_ctx {
            let sym_name_lit = encode_swift_string_literal(&bound_sym.name);
            members.insert(
                0,
                format!(
                    "    public override init(name: String = {sym_name_lit}) {{\n        super.init(name: name)\n        self._nfConfigureLinkedSymbol()\n    }}"
                ),
            );
        } else if extends_display_object {
            members.insert(
                0,
                "    public override init(name: String = \"\") {\n        super.init(name: name)\n    }"
                    .into(),
            );
        } else if has_nonempty_base {
            members.insert(
                0,
                "    public override init() {\n        super.init()\n    }".into(),
            );
        } else {
            members.insert(0, "    public init() {}".into());
        }
    }

    if let Some((_, _, _, ref extra_helpers, _)) = bound_ctx {
        if !extra_helpers.trim().is_empty() {
            let indented: String = extra_helpers
                .lines()
                .map(|l| {
                    if l.trim().is_empty() {
                        String::new()
                    } else {
                        format!("    {l}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            members.push(indented);
        }
        members.push(
            "    public override func instantiateSymbol(_ symbolId: String) -> NFDisplayObject? {\n        NFGeneratedSymbolRegistry.instantiate(symbolId)\n    }".into(),
        );
        members.push(
            "    public override func bindPreallocatedNamedChild(_ rawName: String) -> NFDisplayObject? {\n        self._nfBindLinkedChild(rawName)\n    }".into(),
        );
        members.push(
            "    public override func runFrameScript(frameIndex: Int) {\n        self._nfRunLinkedFrameScript(frameIndex: frameIndex)\n    }".into(),
        );
        members.push(
            "    public override func attachInstanceHandlers(elementId: String, name: String, target: NFDisplayObject) {\n        self._nfAttachLinkedInstanceHandlers(elementId: elementId, name: name, target: target)\n    }".into(),
        );
    }

    let mut supers = Vec::new();
    if is_linked_symbol {
        supers.push("NFMovieClip".to_owned());
    } else if let Some(base) = &parsed.base_class
        && short_type_name(base) != "Object"
    {
        if let Some(cinfo) = class_registry
            .get(base.as_str())
            .or_else(|| class_registry.get(short_type_name(base)))
        {
            supers.push(cinfo.swift_name.clone());
        } else {
            supers.push(swift_type_for_hir(&HirType::from_annotation(base)));
        }
    }
    let extends_project_class = !is_linked_symbol
        && parsed.base_class.as_deref().is_some_and(|b| {
            short_type_name(b) != "Object"
                && (class_registry.contains_key(b)
                    || class_registry.contains_key(short_type_name(b)))
        });
    let inherits_dyn_proto = extends_nf_dispatcher || extends_project_class;
    if !inherits_dyn_proto {
        supers.push("NFDynamicObjectProtocol".to_owned());
    }
    for iface in &parsed.interfaces {
        if let Some(cinfo) = class_registry
            .get(iface.as_str())
            .or_else(|| class_registry.get(short_type_name(iface)))
        {
            supers.push(cinfo.swift_name.clone());
        } else {
            supers.push(sanitize_swift_type_ident(short_type_name(iface)));
        }
    }

    // Emit `NFDynamicObjectProtocol` reflection methods so `NFValue`-typed receivers work dynamically!
    let dyn_ovr = if inherits_dyn_proto { "override " } else { "" };
    let mut get_cases = Vec::new();
    let mut set_cases = Vec::new();
    for f in &parsed.fields {
        if f.is_static {
            continue;
        }
        let key_lit = encode_swift_string_literal(&f.name);
        let prop_id = escape_swift_ident(&f.name);
        get_cases.push(format!(
            "        case {key_lit}: return NFValue.wrap(self.{prop_id})"
        ));
        if !f.is_const && !stage_binding_by_name.contains_key(&f.name) {
            let fty =
                f.ty.as_deref()
                    .map_or(HirType::Unknown, HirType::from_annotation);
            let assign_rhs = match &fty {
                HirType::Float => "value.asDouble".to_owned(),
                HirType::Int => "value.asInt".to_owned(),
                HirType::UInt => "value.asUInt".to_owned(),
                HirType::Bool => "value.isTruthy".to_owned(),
                HirType::String => "value.asString".to_owned(),
                other if SwiftEmitter::is_nfvalue_or_unknown(other) => "value".to_owned(),
                other if emitter.is_swift_reference_class(other) => {
                    let sw_ty = swift_type_for_hir(other);
                    if f.value.is_none() || f.value.as_ref().is_some_and(is_null_or_undef) {
                        format!("value.asObject(as: {sw_ty}.self)")
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };
            set_cases.push(format!(
                "        case {key_lit}: self.{prop_id} = {assign_rhs}"
            ));
        }
    }
    let get_fallback = if inherits_dyn_proto {
        "super.nfGetProperty(key)"
    } else {
        ".undefinedVal"
    };
    let set_fallback = if inherits_dyn_proto {
        "super.nfSetProperty(key, value)"
    } else {
        "break"
    };
    let get_body = if get_cases.is_empty() {
        format!("        return {get_fallback}")
    } else {
        format!(
            "        switch key {{\n{}\n        default: return {get_fallback}\n        }}",
            get_cases.join("\n")
        )
    };
    let set_body = if set_cases.is_empty() {
        if inherits_dyn_proto {
            "        super.nfSetProperty(key, value)".to_owned()
        } else {
            "        _ = key; _ = value".to_owned()
        }
    } else {
        format!(
            "        switch key {{\n{}\n        default: {set_fallback}\n        }}",
            set_cases.join("\n")
        )
    };
    let mut inv_cases = Vec::new();
    for m in &parsed.methods {
        if m.is_static || m.accessor.is_some() {
            continue;
        }
        let raw_name = m.function.name.as_deref().unwrap_or(&parsed.short_name);
        if raw_name == parsed.short_name {
            continue;
        }
        if m.function.params.is_empty() && m.function.rest.is_none() {
            let key_lit = encode_swift_string_literal(raw_name);
            let fname = escape_swift_ident(raw_name);
            let rty = m
                .function
                .returns
                .as_deref()
                .map_or(HirType::Void, HirType::from_annotation);
            let can_throw = !m.is_override
                && raw_name != "onResize"
                && stmts_can_throw_uncaught(&m.function.body);
            let call_expr = if can_throw {
                format!("(try? self.{fname}())")
            } else {
                format!("self.{fname}()")
            };
            if rty == HirType::Void {
                inv_cases.push(format!(
                    "        case {key_lit}: {call_expr}; return .undefinedVal"
                ));
            } else {
                inv_cases.push(format!(
                    "        case {key_lit}: return NFValue.wrap({call_expr})"
                ));
            }
        }
    }
    let inv_fallback = if inherits_dyn_proto {
        "return super.nfInvokeMethod(name, args)".to_owned()
    } else {
        "_ = name; _ = args; return .null".to_owned()
    };
    let inv_body = if inv_cases.is_empty() {
        format!("        {inv_fallback}")
    } else {
        format!(
            "        switch name {{\n{}\n        default: {inv_fallback}\n        }}",
            inv_cases.join("\n")
        )
    };
    members.push(format!(
        "    public {dyn_ovr}func nfGetProperty(_ key: String) -> NFValue {{\n{get_body}\n    }}"
    ));
    members.push(format!(
        "    public {dyn_ovr}func nfSetProperty(_ key: String, _ value: NFValue) {{\n{set_body}\n    }}"
    ));
    members.push(format!(
        "    public {dyn_ovr}func nfInvokeMethod(_ name: String, _ args: [NFValue]) -> NFValue {{\n{inv_body}\n    }}"
    ));

    let super_clause = if supers.is_empty() {
        String::new()
    } else {
        format!(": {}", supers.join(", "))
    };

    let code = format!(
        "@MainActor\npublic class {cls_name}{super_clause} {{\n{}\n}}",
        members.join("\n\n")
    );
    (code, emitter.issues)
}

fn classify_swift_event_type(ev_arg: &Expr) -> Option<String> {
    if let Some(s) = decode_string_literal(ev_arg) {
        return Some(s);
    }
    if let Expr::Member(base, prop) = ev_arg
        && let Expr::Name(cls) = base.as_ref()
    {
        match (cls.as_str(), prop.as_str()) {
            ("MouseEvent", "MOUSE_MOVE") => return Some("mouseMove".into()),
            ("MouseEvent", "MOUSE_OVER") => return Some("mouseOver".into()),
            ("MouseEvent", "MOUSE_OUT") => return Some("mouseOut".into()),
            ("MouseEvent", "RELEASE_OUTSIDE") => return Some("releaseOutside".into()),
            ("TouchEvent", "TOUCH_TAP") => return Some("touchTap".into()),
            ("TouchEvent", "TOUCH_BEGIN") => return Some("touchBegin".into()),
            ("TouchEvent", "TOUCH_MOVE") => return Some("touchMove".into()),
            ("TouchEvent", "TOUCH_END") => return Some("touchEnd".into()),
            ("Event", "CHANGE") => return Some("change".into()),
            ("Event", "ADDED_TO_STAGE") => return Some("addedToStage".into()),
            ("Event", "REMOVED_FROM_STAGE") => return Some("removedFromStage".into()),
            ("Event" | "SoundEvent", "SOUND_COMPLETE") => return Some("soundComplete".into()),
            ("FocusEvent", "FOCUS_IN") => return Some("focusIn".into()),
            ("FocusEvent", "FOCUS_OUT") => return Some("focusOut".into()),
            _ => {}
        }
    }
    classify_as3_event_type(ev_arg)
}

fn match_clip_factory(callee: &Expr, method_name: &str) -> Option<Expr> {
    match callee {
        Expr::Member(base, m) if m == method_name => Some((**base).clone()),
        Expr::Name(m) if m == method_name => Some(Expr::Name("this".into())),
        _ => None,
    }
}

fn listener_target_key(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(n) => Some(n.clone()),
        Expr::Member(b, p) if matches!(b.as_ref(), Expr::Name(n) if n == "this") => {
            Some(format!("this.{p}"))
        }
        _ => None,
    }
}

fn extract_as2_callback_target(lhs: &Expr) -> Option<(Expr, &str)> {
    match lhs {
        Expr::Member(base, prop) => Some(((**base).clone(), prop.as_str())),
        Expr::Name(prop)
            if matches!(
                prop.as_str(),
                "onEnterFrame"
                    | "onKeyDown"
                    | "onKeyUp"
                    | "onResize"
                    | "onRelease"
                    | "onPress"
                    | "onReleaseOutside"
                    | "onRollOver"
                    | "onRollOut"
                    | "onMouseDown"
                    | "onMouseUp"
                    | "onMouseMove"
                    | "onChanged"
            ) =>
        {
            Some((Expr::Name("this".into()), prop.as_str()))
        }
        _ => None,
    }
}

fn is_null_or_undef(expr: &Expr) -> bool {
    matches!(expr, Expr::Literal(s) if s == "null" || s == "undefined")
        || matches!(expr, Expr::Name(s) if s == "undefined" || s == "null")
}

fn is_nonneg_int_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Literal(s) if s.parse::<u64>().is_ok() || s.starts_with("0x") || s.starts_with("0X")
    )
}

fn parse_object_script_for_swift(
    script: &Script,
    includes: &BTreeMap<String, ResourceUnit>,
) -> Result<ParsedScript, String> {
    match parse_timeline_script(script, "", includes) {
        Ok(parsed) => Ok(parsed),
        Err(err) if script.language == ScriptLanguage::ActionScript3 => {
            // Allow classic `on(release)` / `onClipEvent(...)` snippets typed on a Stage button/clip
            // inside an AS3 document to parse cleanly through the AS2 grammar fallback!
            let as2_probe = Script {
                language: ScriptLanguage::ActionScript2,
                source: script.source.clone(),
            };
            parse_timeline_script(&as2_probe, "", includes).map_err(|_| err)
        }
        Err(err) => Err(err),
    }
}

fn map_legacy_triggers_for_swift(
    triggers: &[LegacyTrigger],
    fallback: ObjectEvent,
) -> Vec<(ObjectEvent, Option<&'static str>)> {
    let mut out = Vec::new();
    for trig in triggers {
        let entry = match trig.event.to_ascii_lowercase().as_str() {
            "load" | "initialize" => (ObjectEvent::Load, None),
            "enterframe" => (ObjectEvent::EnterFrame, None),
            "press" => (ObjectEvent::Click, Some("press")),
            "release" => (ObjectEvent::Click, Some("release")),
            "releaseoutside" | "dragout" => (ObjectEvent::Click, Some("releaseOutside")),
            "rollover" | "dragover" => (ObjectEvent::Click, Some("rollOver")),
            "rollout" => (ObjectEvent::Click, Some("rollOut")),
            "mousedown" => (ObjectEvent::Click, Some("clipMouseDown")),
            "mouseup" => (ObjectEvent::Click, Some("clipMouseUp")),
            "mousemove" => (ObjectEvent::Click, Some("clipMouseMove")),
            _ => continue,
        };
        if !out.contains(&entry) {
            out.push(entry);
        }
    }
    if out.is_empty() {
        out.push((fallback, None));
    }
    out
}

fn unquote_string_literal(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let quote = bytes[0];
    if (quote != b'"' && quote != b'\'') || bytes[bytes.len() - 1] != quote {
        return None;
    }
    let inner = &raw[1..raw.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(esc) = chars.next() {
                match esc {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    other => out.push(other),
                }
            }
        } else {
            out.push(ch);
        }
    }
    Some(out)
}

fn contains_swift_ident(code: &str, ident: &str) -> bool {
    let bytes = code.as_bytes();
    let ibytes = ident.as_bytes();
    if ibytes.is_empty() || bytes.len() < ibytes.len() {
        return false;
    }
    let is_word_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'`';
    let mut pos = 0usize;
    while pos + ibytes.len() <= bytes.len() {
        if &bytes[pos..pos + ibytes.len()] == ibytes {
            let prev_ok = pos == 0 || !is_word_char(bytes[pos - 1]);
            let next_ok =
                pos + ibytes.len() == bytes.len() || !is_word_char(bytes[pos + ibytes.len()]);
            if prev_ok && next_ok {
                return true;
            }
        }
        pos += 1;
    }
    false
}

fn expr_may_have_side_effects(expr: &Expr) -> bool {
    match expr {
        Expr::Name(_) | Expr::Literal(_) | Expr::RegExpLiteral(_, _) | Expr::TypeValue(_, _) => {
            false
        }
        Expr::Member(b, _) => expr_may_have_side_effects(b),
        Expr::Index(b, i) => expr_may_have_side_effects(b) || expr_may_have_side_effects(i),
        _ => true,
    }
}

fn proxy_method_ident_from_literal(raw_lit: &str) -> Option<String> {
    let decoded = unquote_string_literal(raw_lit)?;
    let trimmed = decoded.trim();
    if trimmed.is_empty() || !trimmed.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut ident = String::with_capacity(trimmed.len());
    let mut prev_underscore = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            ident.push(ch);
            prev_underscore = ch == '_';
        } else if !prev_underscore && !ident.is_empty() {
            ident.push('_');
            prev_underscore = true;
        }
    }
    let cleaned = ident.trim_matches('_');
    if cleaned.is_empty() {
        return None;
    }
    if cleaned.as_bytes()[0].is_ascii_digit() {
        Some(format!("_{cleaned}"))
    } else {
        Some(cleaned.to_owned())
    }
}

fn is_external_interface_base(expr: &Expr) -> bool {
    match expr {
        Expr::Name(n) => n == "ExternalInterface",
        Expr::Member(_, prop) => prop == "ExternalInterface",
        _ => false,
    }
}

fn match_host_proxy_call<'b>(callee: &'b Expr, args: &'b [Expr]) -> Option<(String, &'b [Expr])> {
    if let Expr::Member(base, method) = callee
        && matches!(base.as_ref(), Expr::Name(n) if n == "SwiftHost")
    {
        let clean = method.trim();
        if !clean.is_empty() {
            return Some((clean.to_owned(), args));
        }
    }
    if matches!(callee, Expr::Name(n) if n == "fscommand")
        && let Some(Expr::Literal(cmd_lit)) = args.first()
        && let Some(name) = proxy_method_ident_from_literal(cmd_lit)
    {
        return Some((name, &args[1..]));
    }
    if let Expr::Member(base, m) = callee
        && m == "call"
        && is_external_interface_base(base)
        && let Some(Expr::Literal(fn_lit)) = args.first()
        && let Some(name) = proxy_method_ident_from_literal(fn_lit)
    {
        return Some((name, &args[1..]));
    }
    None
}

fn swift_default_for_proxy_type(swift_ty: &str) -> &'static str {
    match swift_ty.trim() {
        "Bool" => "false",
        "Int" | "UInt" => "0",
        "Double" => "0.0",
        "String" => "\"\"",
        "[Double]" | "[Int]" | "[UInt]" | "[Bool]" | "[String]" | "[NFValue]" => "[]",
        "[String: NFValue]" => "[:]",
        _ => ".undefined",
    }
}

#[derive(Clone, Debug)]
struct InferredProxySig {
    params: Vec<(String, String, String)>, // (name, swift_type, as_type)
    return_swift_type: Option<String>,
}

fn merge_proxy_swift_types(
    existing_swift: &str,
    existing_as: &str,
    next_swift: &str,
    _next_as: &str,
) -> (String, String) {
    if existing_swift == next_swift {
        return (existing_swift.to_owned(), existing_as.to_owned());
    }
    let is_num_a = matches!(existing_swift, "Int" | "UInt" | "Double");
    let is_num_b = matches!(next_swift, "Int" | "UInt" | "Double");
    if is_num_a && is_num_b {
        if existing_swift == "Double" || next_swift == "Double" {
            return ("Double".into(), "Number".into());
        }
        return ("Int".into(), "int".into());
    }
    ("NFValue".into(), "*".into())
}

fn infer_arg_param_name(arg: &Expr, idx: usize) -> String {
    let raw = match arg {
        Expr::Name(n)
            if !matches!(
                n.as_str(),
                "true" | "false" | "null" | "undefined" | "this" | "NaN" | "Infinity"
            ) =>
        {
            Some(n.as_str())
        }
        Expr::Member(_, prop) => Some(prop.as_str()),
        _ => None,
    };
    if let Some(candidate) = raw {
        let cleaned: String = candidate
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let trimmed = cleaned.trim_start_matches('_');
        if !trimmed.is_empty() && !trimmed.as_bytes()[0].is_ascii_digit() {
            return trimmed.to_owned();
        }
    }
    format!("arg{idx}")
}

fn infer_portable_proxy_arg_type(
    arg: &Expr,
    env: &TypeEnv,
    stage: &ProjectStageIndex,
) -> (&'static str, &'static str) {
    if matches!(arg, Expr::Object(_)) {
        return ("[String: NFValue]", "Object");
    }
    if let Expr::NewVector(elem, _, _) = arg {
        return match short_type_name(elem.trim()) {
            "Number" => ("[Double]", "Vector.<Number>"),
            "int" => ("[Int]", "Vector.<int>"),
            "uint" => ("[UInt]", "Vector.<uint>"),
            "Boolean" => ("[Bool]", "Vector.<Boolean>"),
            "String" => ("[String]", "Vector.<String>"),
            _ => ("[NFValue]", "Array"),
        };
    }
    if matches!(arg, Expr::Array(_)) {
        return ("[NFValue]", "Array");
    }
    match env.infer_expr(arg, stage) {
        HirType::Int => ("Int", "int"),
        HirType::UInt => ("UInt", "uint"),
        HirType::Float => ("Double", "Number"),
        HirType::Bool => ("Bool", "Boolean"),
        HirType::String => ("String", "String"),
        HirType::Vector(elem) => match elem.as_ref() {
            HirType::Float => ("[Double]", "Vector.<Number>"),
            HirType::Int => ("[Int]", "Vector.<int>"),
            HirType::UInt => ("[UInt]", "Vector.<uint>"),
            HirType::Bool => ("[Bool]", "Vector.<Boolean>"),
            HirType::String => ("[String]", "Vector.<String>"),
            _ => ("[NFValue]", "Array"),
        },
        HirType::Array => ("[NFValue]", "Array"),
        _ => ("NFValue", "*"),
    }
}

struct ProxyCallScanner<'a> {
    stage: &'a ProjectStageIndex,
    explicit_host_methods: &'a BTreeMap<String, SwiftProxyMethod>,
    discovered_order: Vec<String>,
    discovered: BTreeMap<String, InferredProxySig>,
    env: TypeEnv,
    current_fn_return: Option<&'static str>,
}

impl<'a> ProxyCallScanner<'a> {
    fn record_call(&mut self, name: String, payload_args: &[Expr], expected_ret: Option<&str>) {
        if self.explicit_host_methods.contains_key(&name) {
            return;
        }
        let mut inferred_params = Vec::with_capacity(payload_args.len());
        let mut used_names = BTreeSet::new();
        for (idx, arg) in payload_args.iter().enumerate() {
            let base_pname = infer_arg_param_name(arg, idx);
            let mut pname = base_pname.clone();
            let mut suffix = 2usize;
            while !used_names.insert(pname.clone()) {
                pname = format!("{base_pname}{suffix}");
                suffix += 1;
            }
            let (sw_ty, as_ty) = infer_portable_proxy_arg_type(arg, &self.env, self.stage);
            inferred_params.push((pname, sw_ty.to_owned(), as_ty.to_owned()));
        }

        if let Some(existing) = self.discovered.get_mut(&name) {
            for (idx, (pname, sw_ty, as_ty)) in inferred_params.into_iter().enumerate() {
                if let Some(slot) = existing.params.get_mut(idx) {
                    let (merged_sw, merged_as) =
                        merge_proxy_swift_types(&slot.1, &slot.2, &sw_ty, &as_ty);
                    slot.1 = merged_sw;
                    slot.2 = merged_as;
                    if slot.0.starts_with("arg") && !pname.starts_with("arg") {
                        slot.0 = pname;
                    }
                } else {
                    existing.params.push((pname, sw_ty, as_ty));
                }
            }
            // Deduplicate param names if widening merged a duplicate
            let mut seen = BTreeSet::new();
            for (idx, slot) in existing.params.iter_mut().enumerate() {
                if !seen.insert(slot.0.clone()) {
                    slot.0 = format!("arg{idx}");
                    seen.insert(slot.0.clone());
                }
            }
            match (&existing.return_swift_type, expected_ret) {
                (_, None) => {}
                (None, Some(r)) => existing.return_swift_type = Some(r.to_owned()),
                (Some(prev), Some(r)) if prev != r => {
                    let (merged, _) = merge_proxy_swift_types(prev, "*", r, "*");
                    existing.return_swift_type = Some(merged);
                }
                _ => {}
            }
        } else {
            self.discovered_order.push(name.clone());
            self.discovered.insert(
                name,
                InferredProxySig {
                    params: inferred_params,
                    return_swift_type: expected_ret.map(str::to_owned),
                },
            );
        }
    }

    fn scan_stmt_list(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.scan_stmt(stmt);
        }
    }

    fn scan_stmt(&mut self, stmt: &Stmt) {
        match stmt.unlocated() {
            Stmt::Located(_, inner) => self.scan_stmt(inner),
            Stmt::Empty | Stmt::Break(_) | Stmt::Continue(_) | Stmt::DefaultXmlNamespace(_) => {}
            Stmt::Block(b) | Stmt::InitClip(_, b, _) => {
                self.env.push_scope();
                self.scan_stmt_list(b);
                self.env.pop_scope();
            }
            Stmt::Var(vars) => {
                for (vname, vty, val_opt) in vars {
                    let expected = vty
                        .as_deref()
                        .and_then(map_portable_as_type_to_swift)
                        .map(|(sw, _)| sw)
                        .or(Some("NFValue"));
                    if let Some(val) = val_opt {
                        self.scan_expr(val, expected);
                    }
                    let hir_ty =
                        vty.as_deref()
                            .map(HirType::from_annotation)
                            .unwrap_or_else(|| {
                                val_opt.as_ref().map_or(HirType::Unknown, |e| {
                                    self.env.infer_expr(e, self.stage)
                                })
                            });
                    self.env.define(vname.clone(), hir_ty);
                }
            }
            Stmt::Function(f) => self.scan_function(f),
            Stmt::Expr(expr) => self.scan_expr(expr, None),
            Stmt::If(cond, yes, no) => {
                self.scan_expr(cond, Some("Bool"));
                self.scan_stmt(yes);
                if let Some(n) = no {
                    self.scan_stmt(n);
                }
            }
            Stmt::While(cond, body) => {
                self.scan_expr(cond, Some("Bool"));
                self.scan_stmt(body);
            }
            Stmt::DoWhile(body, cond) => {
                self.scan_stmt(body);
                self.scan_expr(cond, Some("Bool"));
            }
            Stmt::For(init, cond, update, body) => {
                self.env.push_scope();
                self.scan_stmt(init);
                if let Some(c) = cond {
                    self.scan_expr(c, Some("Bool"));
                }
                self.scan_stmt(body);
                if let Some(u) = update {
                    self.scan_expr(u, None);
                }
                self.env.pop_scope();
            }
            Stmt::ForIn(init, coll, _, body) => {
                self.env.push_scope();
                self.scan_stmt(init);
                self.scan_expr(coll, Some("NFValue"));
                self.scan_stmt(body);
                self.env.pop_scope();
            }
            Stmt::Switch(disc, cases) => {
                self.scan_expr(disc, Some("NFValue"));
                for Case { test, body } in cases {
                    if let Some(t) = test {
                        self.scan_expr(t, Some("NFValue"));
                    }
                    self.scan_stmt_list(body);
                }
            }
            Stmt::Return(val, _) => {
                if let Some(v) = val {
                    let ret_hint = self.current_fn_return.or(Some("NFValue"));
                    self.scan_expr(v, ret_hint);
                }
            }
            Stmt::Throw(val) => self.scan_expr(val, Some("NFValue")),
            Stmt::Try(body, catches, fin) => {
                self.scan_stmt_list(body);
                for c in catches {
                    self.env.push_scope();
                    self.scan_stmt_list(&c.body);
                    self.env.pop_scope();
                }
                if let Some(f) = fin {
                    self.scan_stmt_list(f);
                }
            }
            Stmt::Label(_, inner) | Stmt::IfFrameLoaded(_, inner) => self.scan_stmt(inner),
            Stmt::With(tgt, body, _) | Stmt::TellTarget(tgt, body, _) => {
                self.scan_expr(tgt, Some("NFValue"));
                self.scan_stmt(body);
            }
            Stmt::LegacyEvent(_, func) => self.scan_function(func),
        }
    }

    fn scan_function(&mut self, f: &Function) {
        let saved_ret = self.current_fn_return;
        self.current_fn_return = f
            .returns
            .as_deref()
            .and_then(map_portable_as_type_to_swift)
            .map(|(sw, _)| sw);
        self.env.push_scope();
        for (i, (pname, pty)) in f.params.iter().enumerate() {
            let hty = pty
                .as_deref()
                .map_or(HirType::Unknown, HirType::from_annotation);
            self.env.define(pname.clone(), hty);
            if let Some(Some(def)) = f.defaults.get(i) {
                self.scan_expr(def, Some("NFValue"));
            }
        }
        self.scan_stmt_list(&f.body);
        self.env.pop_scope();
        self.current_fn_return = saved_ret;
    }

    fn scan_expr(&mut self, expr: &Expr, expected_ret: Option<&str>) {
        match expr {
            Expr::Call(callee, args, _) => {
                if let Some((method, payload_args)) = match_host_proxy_call(callee, args) {
                    for arg in payload_args {
                        self.scan_expr(arg, Some("NFValue"));
                    }
                    self.record_call(method, payload_args, expected_ret);
                    return;
                }
                self.scan_expr(callee, Some("NFValue"));
                for arg in args {
                    self.scan_expr(arg, Some("NFValue"));
                }
            }
            Expr::Member(base, _)
            | Expr::Delete(base, _)
            | Expr::Update(_, base, _)
            | Expr::TypeTest(_, base, _, _) => self.scan_expr(base, Some("NFValue")),
            Expr::Unary(op, inner) => {
                let hint = if op == "!" || op == "not" {
                    Some("Bool")
                } else if op == "void" {
                    None
                } else {
                    Some("Double")
                };
                self.scan_expr(inner, hint);
            }
            Expr::Index(a, b) | Expr::InstanceOf(a, b) | Expr::XmlFilter(a, b, _) => {
                self.scan_expr(a, Some("NFValue"));
                self.scan_expr(b, Some("NFValue"));
            }
            Expr::Binary(op, lhs, rhs) => {
                let hint = match op.as_str() {
                    "&&" | "||" | "and" | "or" => Some("Bool"),
                    "-" | "*" | "/" | "%" => Some("Double"),
                    "&" | "|" | "^" | "<<" | ">>" | ">>>" => Some("Int"),
                    "+" | "add" => {
                        let lt = self.env.infer_expr(lhs, self.stage);
                        let rt = self.env.infer_expr(rhs, self.stage);
                        if lt == HirType::String || rt == HirType::String {
                            Some("String")
                        } else {
                            Some("Double")
                        }
                    }
                    _ => Some("NFValue"),
                };
                self.scan_expr(lhs, hint);
                self.scan_expr(rhs, hint);
            }
            Expr::Assign(op, lhs, rhs) => {
                self.scan_expr(lhs, Some("NFValue"));
                let hint = if op == "=" {
                    match self.env.infer_expr(lhs, self.stage) {
                        HirType::Bool => Some("Bool"),
                        HirType::Int => Some("Int"),
                        HirType::UInt => Some("UInt"),
                        HirType::Float => Some("Double"),
                        HirType::String => Some("String"),
                        HirType::Vector(elem) => match elem.as_ref() {
                            HirType::Float => Some("[Double]"),
                            HirType::Int => Some("[Int]"),
                            HirType::UInt => Some("[UInt]"),
                            HirType::Bool => Some("[Bool]"),
                            HirType::String => Some("[String]"),
                            _ => Some("[NFValue]"),
                        },
                        HirType::Array => Some("[NFValue]"),
                        _ => Some("NFValue"),
                    }
                } else {
                    Some("Double")
                };
                self.scan_expr(rhs, hint);
            }
            Expr::Conditional(c, y, n) => {
                self.scan_expr(c, Some("Bool"));
                self.scan_expr(y, expected_ret.or(Some("NFValue")));
                self.scan_expr(n, expected_ret.or(Some("NFValue")));
            }
            Expr::Array(items) => {
                for it in items.iter().flatten() {
                    self.scan_expr(it, Some("NFValue"));
                }
            }
            Expr::Object(props) => {
                for (_, val) in props {
                    self.scan_expr(val, Some("NFValue"));
                }
            }
            Expr::Function(f) => self.scan_function(f),
            Expr::NewError(_, args, _)
            | Expr::NewBuiltin(_, args, _)
            | Expr::NewClass(_, args, _)
            | Expr::NewValue(_, args, _)
            | Expr::NewVector(_, args, _)
            | Expr::XmlLiteral(_, args) => {
                for arg in args {
                    self.scan_expr(arg, Some("NFValue"));
                }
            }
            Expr::XmlSelect {
                base,
                local,
                namespace,
                ..
            } => {
                self.scan_expr(base, Some("NFValue"));
                self.scan_expr(local, Some("NFValue"));
                if let Some(ns) = namespace {
                    self.scan_expr(ns, Some("NFValue"));
                }
            }
            Expr::XmlWithAttr {
                local, namespace, ..
            } => {
                self.scan_expr(local, Some("NFValue"));
                if let Some(ns) = namespace {
                    self.scan_expr(ns, Some("NFValue"));
                }
            }
            Expr::Literal(_)
            | Expr::Name(_)
            | Expr::RegExpLiteral(_, _)
            | Expr::TypeValue(_, _) => {}
        }
    }
}

fn discover_implicit_host_proxies(
    project: &Project,
    parsed_units: &[(String, ParsedClass)],
    stage: &ProjectStageIndex,
    includes: &BTreeMap<String, ResourceUnit>,
    explicit_host_methods: &BTreeMap<String, SwiftProxyMethod>,
    js_ts_type_aliases: &BTreeMap<String, String>,
    js_ts_declared_classes: &BTreeSet<String>,
) -> Vec<SwiftProxyMethod> {
    let mut scanner = ProxyCallScanner {
        stage,
        explicit_host_methods,
        discovered_order: Vec::new(),
        discovered: BTreeMap::new(),
        env: TypeEnv::new(None, None),
        current_fn_return: None,
    };

    // 1. Scan non-SwiftHost `.as` / `.js` / `.ts` classes
    for (_, parsed) in parsed_units {
        scanner.env = TypeEnv::new(None, Some(parsed.qualified_name.clone()));
        for f in &parsed.fields {
            let fty =
                f.ty.as_deref()
                    .map_or(HirType::Unknown, HirType::from_annotation);
            scanner.env.define(f.name.clone(), fty);
            if let Some(val) = &f.value {
                let hint =
                    f.ty.as_deref()
                        .and_then(map_portable_as_type_to_swift)
                        .map(|(sw, _)| sw)
                        .or(Some("NFValue"));
                scanner.scan_expr(val, hint);
            }
        }
        for m in &parsed.methods {
            scanner.scan_function(&m.function);
        }
    }

    // 2. Scan all symbol timelines and stage element `ObjectScript`s
    for sym in &project.symbols {
        let mut sym_env = TypeEnv::new(Some(sym.id.clone()), None);
        if let Some(children) = stage.symbol_children.get(&sym.id) {
            for (cname, info) in children {
                sym_env.define(cname.clone(), info.hir_type.clone());
            }
        }
        scanner.env = sym_env;

        for entry in &sym.timeline.scripts {
            if entry.script.source.trim().is_empty() {
                continue;
            }
            if is_js_or_ts(entry.script.language) {
                if let Ok((parsed, _)) = parse_js_ts_script(
                    &entry.script,
                    "",
                    js_ts_type_aliases,
                    js_ts_declared_classes,
                ) {
                    scanner.scan_stmt_list(&parsed.statements);
                }
            } else if matches!(
                entry.script.language,
                ScriptLanguage::ActionScript1
                    | ScriptLanguage::ActionScript2
                    | ScriptLanguage::ActionScript3
            ) && let Ok(parsed) = parse_timeline_script(&entry.script, "", includes)
            {
                scanner.scan_stmt_list(&parsed.statements);
            }
        }

        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    for os in &el.scripts {
                        if os.script.source.trim().is_empty() {
                            continue;
                        }
                        if is_js_or_ts(os.script.language) {
                            if let Ok((parsed, _)) = parse_js_ts_script(
                                &os.script,
                                "",
                                js_ts_type_aliases,
                                js_ts_declared_classes,
                            ) {
                                scanner.scan_stmt_list(&parsed.statements);
                            }
                        } else if matches!(
                            os.script.language,
                            ScriptLanguage::ActionScript1
                                | ScriptLanguage::ActionScript2
                                | ScriptLanguage::ActionScript3
                        ) && let Ok(parsed) =
                            parse_object_script_for_swift(&os.script, includes)
                        {
                            scanner.scan_stmt_list(&parsed.statements);
                        }
                    }
                }
            }
        }
    }

    scanner
        .discovered_order
        .into_iter()
        .filter_map(|name| {
            let sig = scanner.discovered.remove(&name)?;
            Some(SwiftProxyMethod {
                name,
                params: sig
                    .params
                    .into_iter()
                    .map(|(pname, swift_type, as_type)| SwiftProxyParam {
                        name: pname,
                        swift_type,
                        as_type,
                    })
                    .collect(),
                return_swift_type: sig.return_swift_type,
            })
        })
        .collect()
}

fn has_top_level_optional_chain(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = (depth - 1).max(0),
            b'?' if depth == 0 => {
                if let Some(&next) = bytes.get(i + 1)
                    && matches!(next, b'.' | b'[' | b'(')
                {
                    return true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

fn is_swift_reference_hir_type(ty: &HirType) -> bool {
    match ty {
        HirType::MovieClip | HirType::Sprite | HirType::Button | HirType::TextField => true,
        HirType::Class(cn) => !matches!(
            short_type_name(cn),
            "Point"
                | "Rectangle"
                | "Matrix"
                | "ColorTransform"
                | "SoundTransform"
                | "Vector3D"
                | "Matrix3D"
                | "PerspectiveProjection"
                | "TextLineMetrics"
                | "DropShadowFilter"
                | "GlowFilter"
                | "BlurFilter"
                | "BevelFilter"
                | "GradientGlowFilter"
                | "GradientBevelFilter"
                | "ColorMatrixFilter"
                | "ConvolutionFilter"
                | "BitmapFilter"
                | "PNGEncoderOptions"
                | "JPEGEncoderOptions"
                | "Error"
                | "TypeError"
                | "RangeError"
                | "ArgumentError"
                | "ReferenceError"
                | "SyntaxError"
                | "VerifyError"
        ),
        _ => false,
    }
}
