//! Deterministic AST-to-JavaScript (ES6+) emitter (`AS1/AS2 -> JS` and `AS3 -> JS`).
//!
//! Emits clean, idiomatic ES6+ JavaScript targeting NeoFlash's web/JS runtime without
//! any internal ActionScript VM intrinsics (`__nfFamilyPropertyValue`, etc.).

use super::emit_as2::{classify_as3_event_type, is_stage_expr};
use super::emit_as3::{SynthesizedLinkage, op_prec};
use super::hir::{
    AccessorKind, HirType, LiftedObjectHandler, LiftedTriggerKind, ParsedClass, ParsedScript,
    ProjectStageIndex, TypeEnv, decode_string_literal, fold_100_to_01, is_atomic_expr,
    key_press_spec_to_code, map_as2_direct_prop_to_modern, map_as2_key_constant,
    map_as2_percent_prop_to_modern, map_as3_keyboard_constant, sanitize_as_class_ident,
    short_type_name, stmts_use_key_is_down,
};
use super::{ConversionIssue, ConversionIssueKind};
use crate::actionscript::ast::{Case, CatchClause, Expr, Function, PropertyKey, Stmt};
use crate::byte_offset_to_line_col;
use neoflash_core::ScriptLanguage;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct JsEmitOutput {
    pub(crate) source: String,
    pub(crate) module_path: Option<String>,
    pub(crate) issues: Vec<ConversionIssue>,
    pub(crate) synthesized_linkages: Vec<SynthesizedLinkage>,
}

#[derive(Clone, Debug, Default)]
struct JsClassContext {
    short_name: String,
    instance_fields: BTreeSet<String>,
    static_fields: BTreeSet<String>,
    instance_methods: BTreeSet<String>,
    static_methods: BTreeSet<String>,
}

struct JsEmitter<'a> {
    location: &'a str,
    raw_source: &'a str,
    source_is_as3: bool,
    stage: &'a ProjectStageIndex,
    /// Map from project class short name -> normalized `.js` path (e.g. `"Widget"` -> `"app/Widget.js"`).
    module_paths_by_short: &'a BTreeMap<String, String>,
    current_module_path: &'a str,
    env: TypeEnv,
    class_ctx: Option<JsClassContext>,
    imported_classes: BTreeSet<String>,
    needs_sprite_shim: bool,
    use_key_down_shim: bool,
    in_key_handler_event: Option<String>,
    issues: Vec<ConversionIssue>,
    synthesized_linkages: BTreeMap<String, SynthesizedLinkage>,
    /// Stack of lexical block scopes inside the current function/script (`let` deduplication).
    block_scopes: Vec<BTreeSet<String>>,
    /// Depths of function boundaries inside `block_scopes`.
    fn_scope_indices: Vec<usize>,
    key_down_handlers: BTreeMap<String, String>,
    key_up_handlers: BTreeMap<String, String>,
    resize_handlers: BTreeMap<String, String>,
    explicit_key_listeners: BTreeSet<String>,
    explicit_stage_listeners: BTreeSet<String>,
    helper_counter: usize,
    indent: usize,
}

impl<'a> JsEmitter<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        location: &'a str,
        raw_source: &'a str,
        source_is_as3: bool,
        stage: &'a ProjectStageIndex,
        module_paths_by_short: &'a BTreeMap<String, String>,
        current_module_path: &'a str,
        symbol_id: Option<String>,
        class_name: Option<String>,
    ) -> Self {
        Self {
            location,
            raw_source,
            source_is_as3,
            stage,
            module_paths_by_short,
            current_module_path,
            env: TypeEnv::new(symbol_id, class_name),
            class_ctx: None,
            imported_classes: BTreeSet::new(),
            needs_sprite_shim: false,
            use_key_down_shim: false,
            in_key_handler_event: None,
            issues: Vec::new(),
            synthesized_linkages: BTreeMap::new(),
            block_scopes: vec![BTreeSet::new()],
            fn_scope_indices: vec![0],
            key_down_handlers: BTreeMap::new(),
            key_up_handlers: BTreeMap::new(),
            resize_handlers: BTreeMap::new(),
            explicit_key_listeners: BTreeSet::new(),
            explicit_stage_listeners: BTreeSet::new(),
            helper_counter: 0,
            indent: 0,
        }
    }

    fn pad(&self) -> String {
        "    ".repeat(self.indent)
    }

    fn next_helper(&mut self, prefix: &str) -> String {
        self.helper_counter += 1;
        if self.helper_counter == 1 {
            format!("_{prefix}")
        } else {
            format!("_{prefix}_{}", self.helper_counter)
        }
    }

    fn push_block_scope(&mut self) {
        self.env.push_scope();
        self.block_scopes.push(BTreeSet::new());
    }

    fn pop_block_scope(&mut self) {
        self.env.pop_scope();
        if self.block_scopes.len() > 1 {
            self.block_scopes.pop();
        }
    }

    fn push_fn_scope(&mut self) {
        self.env.push_scope();
        let idx = self.block_scopes.len();
        self.block_scopes.push(BTreeSet::new());
        self.fn_scope_indices.push(idx);
    }

    fn pop_fn_scope(&mut self) {
        self.env.pop_scope();
        if let Some(idx) = self.fn_scope_indices.pop() {
            while self.block_scopes.len() > idx && self.block_scopes.len() > 1 {
                self.block_scopes.pop();
            }
        }
    }

    /// True if `name` is already declared in any active lexical block of the current function.
    fn is_declared_in_current_fn(&self, name: &str) -> bool {
        let start = self.fn_scope_indices.last().copied().unwrap_or(0);
        self.block_scopes[start..]
            .iter()
            .any(|scope| scope.contains(name))
    }

    /// True if `name` is declared in ANY active lexical scope (including outer timeline scope).
    fn is_local_or_outer_var(&self, name: &str) -> bool {
        self.block_scopes.iter().any(|scope| scope.contains(name))
            || self.env.functions.contains_key(name)
    }

    fn mark_declared(&mut self, name: &str) {
        if let Some(top) = self.block_scopes.last_mut() {
            top.insert(name.to_owned());
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

    fn note_class_reference(&mut self, raw_type: &str) {
        let short = short_type_name(raw_type.trim());
        if self.module_paths_by_short.contains_key(short) {
            if self
                .class_ctx
                .as_ref()
                .is_none_or(|ctx| ctx.short_name != short)
            {
                self.imported_classes.insert(short.to_owned());
            }
        } else if short == "Sprite" {
            self.needs_sprite_shim = true;
        }
    }

    fn key_down_shim_block(&self) -> String {
        let pad = self.pad();
        format!(
            "{pad}const _keysDown = Object.create(null);\n\
             {pad}if (this.stage) {{\n\
             {pad}    this.stage.addEventListener(\"keyDown\", (event) => {{ _keysDown[event.keyCode] = true; }});\n\
             {pad}    this.stage.addEventListener(\"keyUp\", (event) => {{ delete _keysDown[event.keyCode]; }});\n\
             {pad}}}\n\
             {pad}const _isKeyDown = (code) => Boolean(_keysDown[code]);"
        )
    }

    fn sprite_shim_block() -> &'static str {
        "const Sprite = globalThis.Sprite || class Sprite {\n\
         \x20   constructor() {\n\
         \x20       this.name = \"\";\n\
         \x20       this.x = 0;\n\
         \x20       this.y = 0;\n\
         \x20       this.scaleX = 1;\n\
         \x20       this.scaleY = 1;\n\
         \x20       this.alpha = 1;\n\
         \x20       this.rotation = 0;\n\
         \x20       this.visible = true;\n\
         \x20       this.children = [];\n\
         \x20       this.graphics = {\n\
         \x20           clear() {},\n\
         \x20           lineStyle() {},\n\
         \x20           beginFill() {},\n\
         \x20           moveTo() {},\n\
         \x20           lineTo() {},\n\
         \x20           curveTo() {},\n\
         \x20           drawRect() {},\n\
         \x20           drawCircle() {},\n\
         \x20           endFill() {}\n\
         \x20       };\n\
         \x20   }\n\
         \x20   addChild(child) {\n\
         \x20       this.children.push(child);\n\
         \x20       return child;\n\
         \x20   }\n\
         };"
    }

    /// Pre-scan statements for top-level function declarations and AS2 `Key.addListener`/`Stage.addListener`.
    fn precollect(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match stmt.unlocated() {
                Stmt::Function(f) => {
                    if let Some(name) = &f.name {
                        let ret = f
                            .returns
                            .as_deref()
                            .map_or(HirType::Unknown, HirType::from_annotation);
                        self.env.functions.insert(name.clone(), ret);
                        self.mark_declared(name);
                    }
                }
                Stmt::Expr(Expr::Call(callee, args, _))
                    if args.len() == 1 && !self.source_is_as3 =>
                {
                    if let Expr::Member(base, method) = callee.as_ref()
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
                _ => {}
            }
        }
    }

    /// Hoist any `var` declared inside nested `if`/`while`/`try` blocks that is also referenced
    /// outside that nested block in the same function, so ES6 `let` preserves AS `var` reachability.
    fn collect_hoisted_vars(&self, stmts: &[Stmt]) -> Vec<String> {
        let mut top_declared = BTreeSet::new();
        for stmt in stmts {
            if let Stmt::Var(vars) = stmt.unlocated() {
                for (vname, _, _) in vars {
                    top_declared.insert(vname.clone());
                }
            }
        }

        let mut nested_vars = Vec::new();
        let mut nested_set = BTreeSet::new();
        for stmt in stmts {
            match stmt.unlocated() {
                Stmt::Var(_)
                | Stmt::For(_, _, _, _)
                | Stmt::ForIn(_, _, _, _)
                | Stmt::Function(_)
                | Stmt::LegacyEvent(_, _) => {}
                other => {
                    collect_nested_var_decls(other, &mut nested_vars, &mut nested_set);
                }
            }
        }

        let mut hoisted = Vec::new();
        for v in nested_vars {
            if !top_declared.contains(&v) && !self.is_declared_in_current_fn(&v) {
                hoisted.push(v);
            }
        }
        hoisted
    }

    fn emit_stmt_list(&mut self, stmts: &[Stmt]) -> String {
        let hoisted = self.collect_hoisted_vars(stmts);
        let mut out = Vec::new();
        let pad = self.pad();
        for hv in hoisted {
            let safe = sanitize_js_ident(&hv);
            self.mark_declared(&hv);
            self.env.define(hv, HirType::Unknown);
            out.push(format!("{pad}let {safe};"));
        }
        for stmt in stmts {
            let rendered = self.emit_stmt(stmt);
            if !rendered.trim().is_empty() {
                out.push(rendered);
            }
        }
        out.join("\n")
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
                self.push_block_scope();
                self.indent += 1;
                let inner = self.emit_stmt_list(body);
                self.indent -= 1;
                self.pop_block_scope();
                if inner.trim().is_empty() {
                    format!("{pad}{{}}")
                } else {
                    format!("{pad}{{\n{inner}\n{pad}}}")
                }
            }
            Stmt::Var(vars) => {
                let mut lines = Vec::new();
                for (raw_name, raw_ty, val_opt) in vars {
                    let vname = sanitize_js_ident(raw_name);
                    if let Some(val) = val_opt
                        && !self.source_is_as3
                        && let Some(expanded) =
                            self.try_emit_dynamic_clip_var(raw_name, &vname, val, offset)
                    {
                        lines.push(expanded);
                        continue;
                    }

                    let decl_ty = raw_ty
                        .as_deref()
                        .map(HirType::from_annotation)
                        .unwrap_or_else(|| {
                            val_opt
                                .as_ref()
                                .map_or(HirType::Unknown, |v| self.env.infer_expr(v, self.stage))
                        });
                    self.env.define(raw_name.clone(), decl_ty);

                    let already = self.is_declared_in_current_fn(raw_name);
                    self.mark_declared(raw_name);

                    if let Some(val) = val_opt {
                        let rhs = self.emit_expr_with_offset(val, offset);
                        if already {
                            lines.push(format!("{pad}{vname} = {rhs};"));
                        } else {
                            lines.push(format!("{pad}let {vname} = {rhs};"));
                        }
                    } else if !already {
                        lines.push(format!("{pad}let {vname};"));
                    }
                }
                lines.join("\n")
            }
            Stmt::Expr(expr) => {
                if let Some(special) = self.try_emit_special_expr_stmt(expr, offset) {
                    return special;
                }
                let code = self.emit_expr_with_offset(expr, offset);
                if code.trim().is_empty() {
                    String::new()
                } else {
                    format!("{pad}{code};")
                }
            }
            Stmt::If(cond, yes, no) => {
                let c = self.emit_expr_with_offset(cond, offset);
                let yes_block = self.emit_branch_body(yes);
                if let Some(no_stmt) = no {
                    if matches!(no_stmt.unlocated(), Stmt::If(_, _, _)) {
                        let nested = self.emit_stmt(no_stmt);
                        let trimmed = nested.trim_start();
                        format!("{pad}if ({c}) {yes_block} else {trimmed}")
                    } else {
                        let no_block = self.emit_branch_body(no_stmt);
                        format!("{pad}if ({c}) {yes_block} else {no_block}")
                    }
                } else {
                    format!("{pad}if ({c}) {yes_block}")
                }
            }
            Stmt::While(cond, body) => {
                let c = self.emit_expr_with_offset(cond, offset);
                let b = self.emit_branch_body(body);
                format!("{pad}while ({c}) {b}")
            }
            Stmt::DoWhile(body, cond) => {
                let b = self.emit_branch_body(body);
                let c = self.emit_expr_with_offset(cond, offset);
                format!("{pad}do {b} while ({c});")
            }
            Stmt::For(init, cond, update, body) => {
                self.push_block_scope();
                let init_str = match init.unlocated() {
                    Stmt::Empty => String::new(),
                    Stmt::Var(vars) => {
                        let any_already = vars
                            .iter()
                            .any(|(n, _, _)| self.is_declared_in_current_fn(n));
                        let mut parts = Vec::new();
                        for (raw_name, raw_ty, val_opt) in vars {
                            let vname = sanitize_js_ident(raw_name);
                            let ty = raw_ty
                                .as_deref()
                                .map(HirType::from_annotation)
                                .unwrap_or_else(|| {
                                    val_opt.as_ref().map_or(HirType::Unknown, |v| {
                                        self.env.infer_expr(v, self.stage)
                                    })
                                });
                            self.env.define(raw_name.clone(), ty);
                            self.mark_declared(raw_name);
                            if let Some(v) = val_opt {
                                parts.push(format!(
                                    "{vname} = {}",
                                    self.emit_expr_with_offset(v, offset)
                                ));
                            } else {
                                parts.push(vname);
                            }
                        }
                        if any_already {
                            parts.join(", ")
                        } else {
                            format!("let {}", parts.join(", "))
                        }
                    }
                    Stmt::Expr(e) => self.emit_expr_with_offset(e, offset),
                    other => self
                        .emit_stmt(other)
                        .trim()
                        .trim_end_matches(';')
                        .to_owned(),
                };
                let cond_str = cond
                    .as_ref()
                    .map(|e| self.emit_expr_with_offset(e, offset))
                    .unwrap_or_default();
                let upd_str = update
                    .as_ref()
                    .map(|e| self.emit_expr_with_offset(e, offset))
                    .unwrap_or_default();
                let b = self.emit_branch_body(body);
                self.pop_block_scope();
                format!("{pad}for ({init_str}; {cond_str}; {upd_str}) {b}")
            }
            Stmt::ForIn(init, coll, is_each, body) => {
                self.push_block_scope();
                let binding = match init.unlocated() {
                    Stmt::Var(vars) if !vars.is_empty() => {
                        let raw_name = &vars[0].0;
                        let vname = sanitize_js_ident(raw_name);
                        let already = self.is_declared_in_current_fn(raw_name);
                        self.mark_declared(raw_name);
                        self.env.define(raw_name.clone(), HirType::Unknown);
                        if already {
                            vname
                        } else {
                            format!("const {vname}")
                        }
                    }
                    Stmt::Expr(e) => self.emit_expr_with_offset(e, offset),
                    _ => "const _k".into(),
                };
                let coll_ty = self.env.infer_expr(coll, self.stage);
                let coll_str = self.emit_expr_with_offset(coll, offset);
                let b = self.emit_branch_body(body);
                self.pop_block_scope();
                if *is_each {
                    match coll_ty {
                        HirType::Array | HirType::Vector(_) => {
                            format!("{pad}for ({binding} of {coll_str}) {b}")
                        }
                        HirType::Dictionary => {
                            format!("{pad}for ({binding} of {coll_str}.values()) {b}")
                        }
                        _ => format!(
                            "{pad}for ({binding} of (Array.isArray({coll_str}) ? {coll_str} : Object.values({coll_str} || {{}}))) {b}"
                        ),
                    }
                } else if coll_ty == HirType::Dictionary {
                    format!("{pad}for ({binding} of {coll_str}.keys()) {b}")
                } else {
                    format!("{pad}for ({binding} in {coll_str}) {b}")
                }
            }
            Stmt::Switch(disc, cases) => {
                let d = self.emit_expr_with_offset(disc, offset);
                self.push_block_scope();
                let mut lines = vec![format!("{pad}switch ({d}) {{")];
                self.indent += 1;
                let cpad = self.pad();
                for Case { test, body } in cases {
                    if let Some(t) = test {
                        lines.push(format!(
                            "{cpad}case {}:",
                            self.emit_expr_with_offset(t, offset)
                        ));
                    } else {
                        lines.push(format!("{cpad}default:"));
                    }
                    self.indent += 1;
                    let b = self.emit_stmt_list(body);
                    if !b.trim().is_empty() {
                        lines.push(b);
                    }
                    self.indent -= 1;
                }
                self.indent -= 1;
                self.pop_block_scope();
                lines.push(format!("{pad}}}"));
                lines.join("\n")
            }
            Stmt::Break(lbl) => match lbl {
                Some(l) => format!("{pad}break {l};"),
                None => format!("{pad}break;"),
            },
            Stmt::Continue(lbl) => match lbl {
                Some(l) => format!("{pad}continue {l};"),
                None => format!("{pad}continue;"),
            },
            Stmt::Return(val, _) => match val {
                Some(v) => format!("{pad}return {};", self.emit_expr_with_offset(v, offset)),
                None => format!("{pad}return;"),
            },
            Stmt::Throw(val) => format!("{pad}throw {};", self.emit_expr_with_offset(val, offset)),
            Stmt::Try(body, catches, fin) => {
                self.push_block_scope();
                self.indent += 1;
                let b = self.emit_stmt_list(body);
                self.indent -= 1;
                self.pop_block_scope();
                let mut out = format!("{pad}try {{\n{b}\n{pad}}}");
                if let Some(first_catch) = catches.first() {
                    if catches.len() == 1 {
                        let cname = sanitize_js_ident(&first_catch.name);
                        self.push_block_scope();
                        self.mark_declared(&first_catch.name);
                        self.env.define(first_catch.name.clone(), HirType::Object);
                        self.indent += 1;
                        let cb = self.emit_stmt_list(&first_catch.body);
                        self.indent -= 1;
                        self.pop_block_scope();
                        out.push_str(&format!(" catch ({cname}) {{\n{cb}\n{pad}}}"));
                    } else {
                        // Multiple typed AS3 catches -> dispatch inside a single ES6 catch(_err)
                        self.push_block_scope();
                        self.mark_declared("_err");
                        self.indent += 1;
                        let in_pad = self.pad();
                        let mut branches = Vec::new();
                        for (idx, CatchClause { name, ty, body, .. }) in catches.iter().enumerate()
                        {
                            self.push_block_scope();
                            let cname = sanitize_js_ident(name);
                            self.mark_declared(name);
                            self.indent += 1;
                            let inner_pad = self.pad();
                            let bind_line = if cname != "_err" {
                                format!("{inner_pad}const {cname} = _err;\n")
                            } else {
                                String::new()
                            };
                            let cb = self.emit_stmt_list(body);
                            self.indent -= 1;
                            self.pop_block_scope();
                            let kw = if idx == 0 { "if" } else { "else if" };
                            if let Some(ty_str) =
                                ty.as_deref().filter(|t| *t != "*" && *t != "Object")
                            {
                                let short = short_type_name(ty_str);
                                self.note_class_reference(short);
                                branches.push(format!(
                                    "{kw} (_err instanceof {short}) {{\n{bind_line}{cb}\n{in_pad}}}"
                                ));
                            } else {
                                branches.push(format!("else {{\n{bind_line}{cb}\n{in_pad}}}"));
                            }
                        }
                        self.indent -= 1;
                        self.pop_block_scope();
                        out.push_str(&format!(
                            " catch (_err) {{\n{in_pad}{}\n{pad}}}",
                            branches.join(" ")
                        ));
                    }
                }
                if let Some(f) = fin {
                    self.push_block_scope();
                    self.indent += 1;
                    let fb = self.emit_stmt_list(f);
                    self.indent -= 1;
                    self.pop_block_scope();
                    out.push_str(&format!(" finally {{\n{fb}\n{pad}}}"));
                }
                out
            }
            Stmt::Label(lbl, inner) => {
                let inner_str = self.emit_stmt(inner);
                format!("{pad}{lbl}: {}", inner_str.trim_start())
            }
            Stmt::Function(f) => self.emit_function_decl(f, offset),
            Stmt::With(target, body, _) | Stmt::TellTarget(target, body, _) => {
                // Strict mode ES6 forbids `with (...)`. Lower cleanly to a scoped block with ManualAttention note.
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    "Legacy 'with' / 'tellTarget' scope is not valid in ES6 strict mode; lowered to a lexical block.",
                );
                let tgt = self.emit_expr_with_offset(target, offset);
                let inner = self.emit_branch_body(body);
                format!("{pad}/* [NeoFlash Convert] target: {tgt} */ {inner}")
            }
            Stmt::DefaultXmlNamespace(e) => {
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    "E4X 'default xml namespace' is not supported in ES6 JavaScript.",
                );
                let val = self.emit_expr_with_offset(e, offset);
                format!("{pad}/* FIXME [NeoFlash Convert]: default xml namespace = {val} */")
            }
            Stmt::IfFrameLoaded(_, body) => self.emit_stmt(body),
            Stmt::InitClip(_, body, _) => self.emit_stmt_list(body),
            Stmt::LegacyEvent(triggers, handler) => triggers
                .iter()
                .filter_map(LiftedTriggerKind::from_legacy)
                .map(|trigger| {
                    self.emit_lifted_handler(&LiftedObjectHandler {
                        instance_name: None,
                        parent_is_this: true,
                        trigger,
                        body: handler.body.clone(),
                    })
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    fn emit_branch_body(&mut self, stmt: &Stmt) -> String {
        match stmt.unlocated() {
            Stmt::Block(body) => {
                self.push_block_scope();
                self.indent += 1;
                let inner = self.emit_stmt_list(body);
                self.indent -= 1;
                self.pop_block_scope();
                let pad = self.pad();
                if inner.trim().is_empty() {
                    "{}".into()
                } else {
                    format!("{{\n{inner}\n{pad}}}")
                }
            }
            other => {
                self.push_block_scope();
                self.indent += 1;
                let inner = self.emit_stmt(other);
                self.indent -= 1;
                self.pop_block_scope();
                let pad = self.pad();
                format!("{{\n{inner}\n{pad}}}")
            }
        }
    }

    fn emit_function_decl(&mut self, f: &Function, offset: usize) -> String {
        let pad = self.pad();
        let raw_name = f.name.as_deref().unwrap_or("_anon");
        let fname = sanitize_js_ident(raw_name);
        self.mark_declared(raw_name);

        let is_key_fn = f.params.first().is_some_and(|(_, ty)| {
            ty.as_deref()
                .is_some_and(|t| short_type_name(t) == "KeyboardEvent")
        });
        let saved_key_ev = self.in_key_handler_event.clone();
        if is_key_fn {
            self.in_key_handler_event = f.params.first().map(|(n, _)| n.clone());
        }

        self.push_fn_scope();
        let params_str = self.emit_fn_params(f, offset);
        self.indent += 1;
        let body = self.emit_stmt_list(&f.body);
        self.indent -= 1;
        self.pop_fn_scope();
        self.in_key_handler_event = saved_key_ev;

        if body.trim().is_empty() {
            format!("{pad}function {fname}({params_str}) {{}}")
        } else {
            format!("{pad}function {fname}({params_str}) {{\n{body}\n{pad}}}")
        }
    }

    fn emit_fn_params(&mut self, f: &Function, offset: usize) -> String {
        let mut out = Vec::new();
        for (i, (raw_p, pty)) in f.params.iter().enumerate() {
            let pname = sanitize_js_ident(raw_p);
            let ty = pty
                .as_deref()
                .map_or(HirType::Unknown, HirType::from_annotation);
            self.mark_declared(raw_p);
            self.env.define(raw_p.clone(), ty);
            if let Some(Some(def)) = f.defaults.get(i) {
                let dc = self.emit_expr_with_offset(def, offset);
                out.push(format!("{pname} = {dc}"));
            } else {
                out.push(pname);
            }
        }
        if let Some(rest) = &f.rest {
            let rname = sanitize_js_ident(rest);
            self.mark_declared(rest);
            self.env.define(rest.clone(), HirType::Array);
            out.push(format!("...{rname}"));
        }
        out.join(", ")
    }

    fn try_emit_dynamic_clip_var(
        &mut self,
        raw_var_name: &str,
        js_var_name: &str,
        val: &Expr,
        offset: usize,
    ) -> Option<String> {
        let Expr::Call(callee, args, _) = val else {
            return None;
        };
        let pad = self.pad();
        let already = self.is_declared_in_current_fn(raw_var_name);
        let decl_kw = if already { "" } else { "let " };

        if let Some(parent_expr) = match_clip_factory(callee, "createEmptyMovieClip")
            && !args.is_empty()
        {
            self.needs_sprite_shim = true;
            self.mark_declared(raw_var_name);
            self.env.define(raw_var_name.to_owned(), HirType::Sprite);
            let parent_js = self.emit_expr_with_offset(&parent_expr, offset);
            let name_js = self.emit_expr_with_offset(&args[0], offset);
            return Some(format!(
                "{pad}{decl_kw}{js_var_name} = new Sprite();\n\
                 {pad}{js_var_name}.name = {name_js};\n\
                 {pad}{parent_js}.addChild({js_var_name});"
            ));
        }

        if let Some(parent_expr) = match_clip_factory(callee, "attachMovie")
            && args.len() >= 2
        {
            let parent_js = self.emit_expr_with_offset(&parent_expr, offset);
            let name_js = self.emit_expr_with_offset(&args[1], offset);
            let ctor_name = if let Some(linkage_id) = decode_string_literal(&args[0]) {
                let cls_name = if let Some(info) = self.stage.linkages.get(&linkage_id) {
                    let short = short_type_name(&info.class_name).to_owned();
                    self.synthesized_linkages.insert(
                        linkage_id.clone(),
                        SynthesizedLinkage {
                            linkage_id: linkage_id.clone(),
                            class_name: short.clone(),
                            symbol_kind: info.symbol_kind,
                        },
                    );
                    short
                } else {
                    let synth = sanitize_as_class_ident(&linkage_id);
                    self.synthesized_linkages.insert(
                        linkage_id.clone(),
                        SynthesizedLinkage {
                            linkage_id: linkage_id.clone(),
                            class_name: synth.clone(),
                            symbol_kind: neoflash_core::SymbolKind::MovieClip,
                        },
                    );
                    synth
                };
                self.imported_classes.insert(cls_name.clone());
                self.mark_declared(raw_var_name);
                self.env
                    .define(raw_var_name.to_owned(), HirType::Class(cls_name.clone()));
                cls_name
            } else {
                self.needs_sprite_shim = true;
                self.mark_declared(raw_var_name);
                self.env.define(raw_var_name.to_owned(), HirType::Sprite);
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    "Dynamic non-literal attachMovie linkage ID lowered to Sprite; instantiate concrete exported class.",
                );
                "Sprite".into()
            };

            let mut lines = vec![
                format!("{pad}{decl_kw}{js_var_name} = new {ctor_name}();"),
                format!("{pad}{js_var_name}.name = {name_js};"),
            ];
            if let Some(init_arg) = args.get(3) {
                lines.extend(self.emit_init_object_expansion(js_var_name, init_arg, offset));
            }
            lines.push(format!("{pad}{parent_js}.addChild({js_var_name});"));
            return Some(lines.join("\n"));
        }

        None
    }

    fn emit_init_object_expansion(
        &mut self,
        target_var: &str,
        init_arg: &Expr,
        offset: usize,
    ) -> Vec<String> {
        let pad = self.pad();
        let mut lines = Vec::new();
        if let Expr::Object(props) = init_arg {
            for (key, val) in props {
                let prop_name = &key.name;
                if let Some(modern) = map_as2_percent_prop_to_modern(prop_name) {
                    let rhs = self.emit_scaled_100_to_01(val, offset);
                    lines.push(format!("{pad}{target_var}.{modern} = {rhs};"));
                } else if let Some(modern) = map_as2_direct_prop_to_modern(prop_name) {
                    let rhs = self.emit_expr_with_offset(val, offset);
                    lines.push(format!("{pad}{target_var}.{modern} = {rhs};"));
                } else {
                    let rhs = self.emit_expr_with_offset(val, offset);
                    lines.push(format!("{pad}{target_var}.{prop_name} = {rhs};"));
                }
            }
        } else {
            let init_js = self.emit_expr_with_offset(init_arg, offset);
            lines.push(format!("{pad}Object.assign({target_var}, {init_js});"));
        }
        lines
    }

    fn try_emit_special_expr_stmt(&mut self, expr: &Expr, offset: usize) -> Option<String> {
        let pad = self.pad();

        // 1. AS1/AS2 callback assignments (`this.onEnterFrame = ...`, `this.onKeyDown = ...`, `onResize = ...`)
        if !self.source_is_as3
            && let Expr::Assign(op, lhs, rhs) = expr
            && op == "="
            && let Some((target_expr, cb_prop)) = extract_as2_callback_target(lhs)
        {
            match cb_prop {
                "onEnterFrame" => {
                    if is_nullish_expr(rhs) {
                        return Some(format!(
                            "{pad}/* [NeoFlash Convert] clear onEnterFrame listener on {} */",
                            self.emit_expr_with_offset(&target_expr, offset)
                        ));
                    }
                    let tgt = self.emit_expr_with_offset(&target_expr, offset);
                    let listener = self.emit_callback_rhs_js(rhs, "event", false, offset);
                    return Some(format!(
                        "{pad}{tgt}.addEventListener(\"enterFrame\", {listener});"
                    ));
                }
                "onRelease" | "onPress" | "onRollOver" | "onRollOut" | "onReleaseOutside" => {
                    if is_nullish_expr(rhs) {
                        return Some(String::new());
                    }
                    let ev_name = match cb_prop {
                        "onRelease" => "click",
                        "onPress" => "mouseDown",
                        "onReleaseOutside" => "mouseUp",
                        "onRollOver" => "rollOver",
                        _ => "rollOut",
                    };
                    let tgt = self.emit_expr_with_offset(&target_expr, offset);
                    let listener = self.emit_callback_rhs_js(rhs, "event", false, offset);
                    return Some(format!(
                        "{pad}{tgt}.addEventListener(\"{ev_name}\", {listener});"
                    ));
                }
                "onKeyDown" | "onKeyUp" => {
                    let is_down = cb_prop == "onKeyDown";
                    let fn_name = self.next_helper(if is_down { "onKeyDown" } else { "onKeyUp" });
                    let listener_decl =
                        self.emit_named_callback_const(&fn_name, rhs, "event", true, offset);
                    let key = listener_target_key(&target_expr).unwrap_or_else(|| "this".into());
                    let has_explicit = self.explicit_key_listeners.contains(&key);
                    if is_down {
                        self.key_down_handlers.insert(key, fn_name.clone());
                    } else {
                        self.key_up_handlers.insert(key, fn_name.clone());
                    }
                    if has_explicit {
                        return Some(listener_decl);
                    }
                    let ev_str = if is_down { "keyDown" } else { "keyUp" };
                    return Some(format!(
                        "{listener_decl}\n\
                         {pad}if (this.stage) {{\n\
                         {pad}    this.stage.focus = this;\n\
                         {pad}    this.stage.addEventListener(\"{ev_str}\", {fn_name});\n\
                         {pad}}}"
                    ));
                }
                "onResize" => {
                    let fn_name = self.next_helper("onResize");
                    let listener_decl =
                        self.emit_named_callback_const(&fn_name, rhs, "event", false, offset);
                    let key = listener_target_key(&target_expr).unwrap_or_else(|| "this".into());
                    let has_explicit = self.explicit_stage_listeners.contains(&key);
                    self.resize_handlers.insert(key, fn_name.clone());
                    if has_explicit {
                        return Some(listener_decl);
                    }
                    return Some(format!(
                        "{listener_decl}\n\
                         {pad}if (this.stage) {{\n\
                         {pad}    this.stage.addEventListener(\"resize\", {fn_name});\n\
                         {pad}}}"
                    ));
                }
                _ => {}
            }
        }

        // 2. AS2 `Key.addListener(obj)` / `Stage.addListener(obj)` / `removeMovieClip`
        if !self.source_is_as3
            && let Expr::Call(callee, args, _) = expr
        {
            if let Expr::Member(base, method) = callee.as_ref()
                && let Expr::Name(mgr) = base.as_ref()
                && args.len() == 1
            {
                let key = listener_target_key(&args[0]).unwrap_or_else(|| "this".into());
                if mgr == "Key" && method == "addListener" {
                    let mut reg = Vec::new();
                    if let Some(kd) = self.key_down_handlers.get(&key) {
                        reg.push(format!(
                            "{pad}    this.stage.addEventListener(\"keyDown\", {kd});"
                        ));
                    } else {
                        let arg_js = self.emit_expr_with_offset(&args[0], offset);
                        reg.push(format!(
                            "{pad}    if (typeof {arg_js}.onKeyDown === \"function\") {{ this.stage.addEventListener(\"keyDown\", (e) => {arg_js}.onKeyDown(e)); }}"
                        ));
                    }
                    if let Some(ku) = self.key_up_handlers.get(&key) {
                        reg.push(format!(
                            "{pad}    this.stage.addEventListener(\"keyUp\", {ku});"
                        ));
                    }
                    return Some(format!(
                        "{pad}if (this.stage) {{\n\
                         {pad}    this.stage.focus = this;\n\
                         {}\n\
                         {pad}}}",
                        reg.join("\n")
                    ));
                }
                if mgr == "Key" && method == "removeListener" {
                    let mut rem = Vec::new();
                    if let Some(kd) = self.key_down_handlers.get(&key) {
                        rem.push(format!(
                            "{pad}    this.stage.removeEventListener(\"keyDown\", {kd});"
                        ));
                    }
                    if let Some(ku) = self.key_up_handlers.get(&key) {
                        rem.push(format!(
                            "{pad}    this.stage.removeEventListener(\"keyUp\", {ku});"
                        ));
                    }
                    if rem.is_empty() {
                        return Some(String::new());
                    }
                    return Some(format!(
                        "{pad}if (this.stage) {{\n{}\n{pad}}}",
                        rem.join("\n")
                    ));
                }
                if mgr == "Stage" && method == "addListener" {
                    if let Some(rsz) = self.resize_handlers.get(&key) {
                        return Some(format!(
                            "{pad}if (this.stage) {{\n\
                             {pad}    this.stage.addEventListener(\"resize\", {rsz});\n\
                             {pad}}}"
                        ));
                    }
                    let arg_js = self.emit_expr_with_offset(&args[0], offset);
                    return Some(format!(
                        "{pad}if (this.stage && typeof {arg_js}.onResize === \"function\") {{\n\
                         {pad}    this.stage.addEventListener(\"resize\", (e) => {arg_js}.onResize(e));\n\
                         {pad}}}"
                    ));
                }
            }

            if let Expr::Member(base, method) = callee.as_ref()
                && method == "removeMovieClip"
                && args.is_empty()
            {
                let b = self.emit_expr_with_offset(base, offset);
                return Some(format!(
                    "{pad}if ({b} && {b}.parent) {{ {b}.parent.removeChild({b}); }}"
                ));
            }
            if let Expr::Name(fn_name) = callee.as_ref()
                && fn_name == "removeMovieClip"
                && args.len() == 1
            {
                let t = self.emit_expr_with_offset(&args[0], offset);
                return Some(format!(
                    "{pad}if ({t} && {t}.parent) {{ {t}.parent.removeChild({t}); }}"
                ));
            }
        }

        None
    }

    fn emit_callback_rhs_js(
        &mut self,
        rhs: &Expr,
        ev_param: &str,
        is_key_handler: bool,
        offset: usize,
    ) -> String {
        if let Expr::Function(f) = rhs {
            let saved_ke = self.in_key_handler_event.clone();
            if is_key_handler {
                self.in_key_handler_event = Some(ev_param.to_owned());
            }
            self.push_fn_scope();
            self.mark_declared(ev_param);
            self.env
                .define(ev_param.to_owned(), HirType::Class("Event".into()));
            self.indent += 1;
            let body = self.emit_stmt_list(&f.body);
            self.indent -= 1;
            self.pop_fn_scope();
            self.in_key_handler_event = saved_ke;
            let pad = self.pad();
            // Use arrow function when inside timeline/method so `this` stays bound to the timeline/instance!
            format!("({ev_param}) => {{\n{body}\n{pad}}}")
        } else {
            self.emit_listener_arg_expr(rhs, offset)
        }
    }

    fn emit_named_callback_const(
        &mut self,
        const_name: &str,
        rhs: &Expr,
        ev_param: &str,
        is_key_handler: bool,
        offset: usize,
    ) -> String {
        let pad = self.pad();
        self.mark_declared(const_name);
        let arrow = self.emit_callback_rhs_js(rhs, ev_param, is_key_handler, offset);
        format!("{pad}const {const_name} = {arrow};")
    }

    /// Emit a listener argument passed to `addEventListener(type, listener)`, adding `.bind(this)`
    /// automatically when passing an instance method of the enclosing ES6 class!
    fn emit_listener_arg_expr(&mut self, listener: &Expr, offset: usize) -> String {
        if let Some(ctx) = &self.class_ctx {
            match listener {
                Expr::Name(name)
                    if !self.is_declared_in_current_fn(name)
                        && ctx.instance_methods.contains(name) =>
                {
                    return format!("this.{name}.bind(this)");
                }
                Expr::Member(base, prop)
                    if matches!(base.as_ref(), Expr::Name(n) if n == "this")
                        && ctx.instance_methods.contains(prop) =>
                {
                    return format!("this.{prop}.bind(this)");
                }
                _ => {}
            }
        }
        self.emit_expr_with_offset(listener, offset)
    }

    fn emit_scaled_100_to_01(&mut self, expr: &Expr, offset: usize) -> String {
        if let Some(folded) = fold_100_to_01(expr, false) {
            return folded;
        }
        let rendered = self.emit_expr_with_offset(expr, offset);
        if is_atomic_expr(expr) {
            format!("{rendered} / 100")
        } else {
            format!("({rendered}) / 100")
        }
    }

    fn emit_expr_with_offset(&mut self, expr: &Expr, offset: usize) -> String {
        match expr {
            Expr::Literal(raw) => raw.clone(),
            Expr::RegExpLiteral(pat, flags) => format!("/{pat}/{flags}"),
            Expr::TypeValue(name, _) => {
                let short = short_type_name(name);
                self.note_class_reference(short);
                short.to_owned()
            }
            Expr::Name(name) => self.emit_ident_expr(name),
            Expr::Member(base, prop) => self.emit_member_expr(base, prop, offset),
            Expr::Index(base, idx) => {
                let base_ty = self.env.infer_expr(base, self.stage);
                let b = self.emit_subexpr(base, 18, offset);
                let i = self.emit_expr_with_offset(idx, offset);
                if base_ty == HirType::Dictionary {
                    format!("{b}.get({i})")
                } else {
                    format!("{b}[{i}]")
                }
            }
            Expr::Array(items) => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|it| {
                        it.as_ref()
                            .map(|e| self.emit_expr_with_offset(e, offset))
                            .unwrap_or_default()
                    })
                    .collect();
                format!("[{}]", parts.join(", "))
            }
            Expr::Object(props) => {
                if props.is_empty() {
                    return "{}".into();
                }
                let parts: Vec<String> = props
                    .iter()
                    .map(|(PropertyKey { source, .. }, v)| {
                        format!("{source}: {}", self.emit_expr_with_offset(v, offset))
                    })
                    .collect();
                format!("{{ {} }}", parts.join(", "))
            }
            Expr::Unary(op, inner) => {
                let js_op = if op == "not" { "!" } else { op.as_str() };
                let inner_str = self.emit_subexpr(inner, 16, offset);
                if matches!(js_op, "typeof" | "void") {
                    format!("{js_op} {inner_str}")
                } else {
                    format!("{js_op}{inner_str}")
                }
            }
            Expr::Delete(target, _) => {
                if let Expr::Index(base, idx) = target.as_ref()
                    && self.env.infer_expr(base, self.stage) == HirType::Dictionary
                {
                    let b = self.emit_subexpr(base, 18, offset);
                    let i = self.emit_expr_with_offset(idx, offset);
                    return format!("{b}.delete({i})");
                }
                format!("delete {}", self.emit_subexpr(target, 16, offset))
            }
            Expr::Update(op, target, prefix) => {
                if !self.source_is_as3
                    && let Some((base_js, modern)) = self.match_as2_percent_lvalue(target, offset)
                {
                    let delta = if op == "++" { "+=" } else { "-=" };
                    return format!("({base_js}.{modern} {delta} 0.01)");
                }
                let t = self.emit_subexpr(target, 17, offset);
                if *prefix {
                    format!("{op}{t}")
                } else {
                    format!("{t}{op}")
                }
            }
            Expr::Binary(op, lhs, rhs) => {
                if op == "in" && self.env.infer_expr(rhs, self.stage) == HirType::Dictionary {
                    let k = self.emit_expr_with_offset(lhs, offset);
                    let d = self.emit_subexpr(rhs, 18, offset);
                    return format!("{d}.has({k})");
                }
                let mapped_op = match op.as_str() {
                    "and" => "&&",
                    "or" => "||",
                    "eq" => "===",
                    "ne" | "<>" => "!==",
                    other => other,
                };
                let prec = op_prec(mapped_op);
                let l = self.emit_subexpr(lhs, prec, offset);
                let r = self.emit_subexpr_right(rhs, prec, offset);
                format!("{l} {mapped_op} {r}")
            }
            Expr::Conditional(cond, yes, no) => {
                let c = self.emit_subexpr(cond, 3, offset);
                let y = self.emit_expr_with_offset(yes, offset);
                let n = self.emit_expr_with_offset(no, offset);
                format!("{c} ? {y} : {n}")
            }
            Expr::Assign(op, lhs, rhs) => {
                if let Expr::Index(base, idx) = lhs.as_ref()
                    && self.env.infer_expr(base, self.stage) == HirType::Dictionary
                {
                    let b = self.emit_subexpr(base, 18, offset);
                    let i = self.emit_expr_with_offset(idx, offset);
                    let r = self.emit_expr_with_offset(rhs, offset);
                    if op == "=" {
                        return format!("{b}.set({i}, {r})");
                    }
                    let bin_op = op.trim_end_matches('=');
                    return format!("{b}.set({i}, ({b}.get({i}) {bin_op} ({r})))");
                }

                if !self.source_is_as3
                    && let Some((base_js, modern)) = self.match_as2_percent_lvalue(lhs, offset)
                {
                    match op.as_str() {
                        "=" => {
                            let r = self.emit_scaled_100_to_01(rhs, offset);
                            return format!("{base_js}.{modern} = {r}");
                        }
                        "+=" | "-=" => {
                            let r = if let Some(folded) = fold_100_to_01(rhs, false) {
                                folded
                            } else {
                                format!("({}) / 100", self.emit_expr_with_offset(rhs, offset))
                            };
                            return format!("{base_js}.{modern} {op} {r}");
                        }
                        "*=" | "/=" => {
                            let r = self.emit_expr_with_offset(rhs, offset);
                            return format!("{base_js}.{modern} {op} {r}");
                        }
                        _ => {}
                    }
                }

                let l = self.emit_lvalue_expr(lhs, offset);
                let r = self.emit_expr_with_offset(rhs, offset);
                format!("{l} {op} {r}")
            }
            Expr::Call(callee, args, call_off) => self.emit_call_expr(callee, args, *call_off),
            Expr::NewError(ty, args, call_off)
            | Expr::NewBuiltin(ty, args, call_off)
            | Expr::NewClass(ty, args, call_off)
            | Expr::NewValue(ty, args, call_off) => self.emit_new_expr(ty, args, *call_off),
            Expr::NewVector(_, args, call_off) => {
                if args.len() == 1
                    && let Expr::Array(items) = &args[0]
                {
                    let parts: Vec<String> = items
                        .iter()
                        .map(|it| {
                            it.as_ref()
                                .map(|e| self.emit_expr_with_offset(e, *call_off))
                                .unwrap_or_default()
                        })
                        .collect();
                    format!("[{}]", parts.join(", "))
                } else if let Some(len_arg) = args.first() {
                    let l = self.emit_expr_with_offset(len_arg, *call_off);
                    format!("new Array({l})")
                } else {
                    "[]".into()
                }
            }
            Expr::InstanceOf(lhs, rhs) => {
                let l = self.emit_subexpr(lhs, 10, offset);
                let r = self.emit_subexpr(rhs, 10, offset);
                format!("{l} instanceof {r}")
            }
            Expr::TypeTest(op, inner, ty_name, tt_off) => {
                let short = short_type_name(ty_name);
                let inner_js = self.emit_subexpr(inner, 10, *tt_off);
                if op == "as" {
                    match short {
                        "Number" | "int" | "uint" => {
                            format!("(typeof {inner_js} === \"number\" ? {inner_js} : null)")
                        }
                        "String" => {
                            format!("(typeof {inner_js} === \"string\" ? {inner_js} : null)")
                        }
                        "Boolean" => {
                            format!("(typeof {inner_js} === \"boolean\" ? {inner_js} : null)")
                        }
                        "Array" => format!("(Array.isArray({inner_js}) ? {inner_js} : null)"),
                        "Object" | "*" => inner_js,
                        other => {
                            self.note_class_reference(other);
                            format!("({inner_js} instanceof {other} ? {inner_js} : null)")
                        }
                    }
                } else {
                    match short {
                        "Number" => format!("(typeof {inner_js} === \"number\")"),
                        "int" => {
                            format!(
                                "(typeof {inner_js} === \"number\" && ({inner_js} | 0) === {inner_js})"
                            )
                        }
                        "uint" => {
                            format!(
                                "(typeof {inner_js} === \"number\" && ({inner_js} >>> 0) === {inner_js})"
                            )
                        }
                        "String" => format!("(typeof {inner_js} === \"string\")"),
                        "Boolean" => format!("(typeof {inner_js} === \"boolean\")"),
                        "Function" => format!("(typeof {inner_js} === \"function\")"),
                        "Array" => format!("Array.isArray({inner_js})"),
                        "Object" => {
                            format!("({inner_js} !== null && typeof {inner_js} === \"object\")")
                        }
                        other => {
                            self.note_class_reference(other);
                            format!("({inner_js} instanceof {other})")
                        }
                    }
                }
            }
            Expr::Function(f) => {
                self.push_fn_scope();
                let params = self.emit_fn_params(f, offset);
                self.indent += 1;
                let body = self.emit_stmt_list(&f.body);
                self.indent -= 1;
                self.pop_fn_scope();
                let pad = self.pad();
                if let Some(fname) = &f.name {
                    let safe = sanitize_js_ident(fname);
                    format!("function {safe}({params}) {{\n{body}\n{pad}}}")
                } else {
                    format!("function ({params}) {{\n{body}\n{pad}}}")
                }
            }
            Expr::XmlLiteral(raw, _) => {
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    "E4X XML literal is not natively supported in ES6 JavaScript; manual DOMParser/XML migration required.",
                );
                format!("/* FIXME [NeoFlash Convert]: E4X XML literal {raw} */ null")
            }
            Expr::XmlSelect {
                base,
                local,
                attribute,
                descendant,
                ..
            } => {
                let b = self.emit_expr_with_offset(base, offset);
                let l = self.emit_expr_with_offset(local, offset);
                let op_sym = if *descendant { ".." } else { "." };
                let at_sym = if *attribute { "@" } else { "" };
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    format!(
                        "E4X XML query '{op_sym}{at_sym}{l}' is not supported in ES6 JavaScript; manual migration required."
                    ),
                );
                format!("/* FIXME [NeoFlash Convert]: E4X {b}{op_sym}{at_sym}{l} */ undefined")
            }
            Expr::XmlWithAttr {
                local,
                attribute,
                descendant,
                ..
            } => {
                let l = self.emit_expr_with_offset(local, offset);
                let op_sym = if *descendant { ".." } else { "" };
                let at_sym = if *attribute { "@" } else { "" };
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    format!(
                        "E4X XML selector '{op_sym}{at_sym}{l}' is not supported in ES6 JavaScript; manual migration required."
                    ),
                );
                format!("/* FIXME [NeoFlash Convert]: E4X {op_sym}{at_sym}{l} */ undefined")
            }
            Expr::XmlFilter(base, predicate, x_off) => {
                let b = self.emit_expr_with_offset(base, *x_off);
                let p = self.emit_expr_with_offset(predicate, *x_off);
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    *x_off,
                    "E4X XML filter predicate '.(...)' is not supported in ES6 JavaScript; manual migration required.",
                );
                format!("/* FIXME [NeoFlash Convert]: E4X {b}.({p}) */ []")
            }
        }
    }

    fn emit_ident_expr(&mut self, name: &str) -> String {
        if matches!(
            name,
            "this"
                | "super"
                | "true"
                | "false"
                | "null"
                | "undefined"
                | "NaN"
                | "Infinity"
                | "arguments"
        ) {
            return name.to_owned();
        }

        // Local/outer lexical variable takes precedence
        if self.is_local_or_outer_var(name) {
            return sanitize_js_ident(name);
        }

        // Class instance/static field or method implicit `this.` / `ClassName.` resolution
        if let Some(ctx) = self.class_ctx.clone() {
            if ctx.instance_fields.contains(name) || ctx.instance_methods.contains(name) {
                return format!("this.{name}");
            }
            if ctx.static_fields.contains(name) || ctx.static_methods.contains(name) {
                return format!("{}.{name}", ctx.short_name);
            }
        }

        // Timeline stage child element (`scoreText` -> `this.scoreText`)
        if let Some(sym_id) = &self.env.symbol_id
            && self.stage.child_type(sym_id, name).is_some()
        {
            return format!("this.{name}");
        }

        // Bare AS1/AS2 properties (`_x` -> `this.x`, `_alpha` -> `(this.alpha * 100)`, `_parent` -> `this.parent`)
        if !self.source_is_as3 {
            if let Some(modern) = map_as2_percent_prop_to_modern(name) {
                return format!("(this.{modern} * 100)");
            }
            if let Some(modern) = map_as2_direct_prop_to_modern(name) {
                return format!("this.{modern}");
            }
        }

        // Bare AS3 DisplayObject properties on timeline or MovieClip subclass (`stage` -> `this.stage`, `x` -> `this.x`)
        if matches!(
            name,
            "stage"
                | "root"
                | "parent"
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
                | "currentFrame"
                | "totalFrames"
                | "mouseX"
                | "mouseY"
                | "numChildren"
        ) {
            return format!("this.{name}");
        }

        // Project class reference
        if self.module_paths_by_short.contains_key(name) {
            self.note_class_reference(name);
            return name.to_owned();
        }
        if name == "Sprite" {
            self.needs_sprite_shim = true;
        }

        sanitize_js_ident(name)
    }

    fn emit_lvalue_expr(&mut self, lhs: &Expr, offset: usize) -> String {
        if let Expr::Name(name) = lhs {
            if !self.is_local_or_outer_var(name) {
                if let Some(ctx) = &self.class_ctx {
                    if ctx.instance_fields.contains(name) {
                        return format!("this.{name}");
                    }
                    if ctx.static_fields.contains(name) {
                        return format!("{}.{name}", ctx.short_name);
                    }
                }
                if !self.source_is_as3
                    && let Some(modern) = map_as2_direct_prop_to_modern(name)
                {
                    return format!("this.{modern}");
                }
                if matches!(
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
                ) {
                    return format!("this.{name}");
                }
            }
            return sanitize_js_ident(name);
        }
        if !self.source_is_as3
            && let Expr::Member(base, prop) = lhs
            && let Some(modern) = map_as2_direct_prop_to_modern(prop)
        {
            let b = self.emit_subexpr(base, 18, offset);
            return format!("{b}.{modern}");
        }
        self.emit_expr_with_offset(lhs, offset)
    }

    fn match_as2_percent_lvalue(
        &mut self,
        expr: &Expr,
        offset: usize,
    ) -> Option<(String, &'static str)> {
        match expr {
            Expr::Member(base, prop) => {
                let modern = map_as2_percent_prop_to_modern(prop)?;
                let b = self.emit_subexpr(base, 18, offset);
                Some((b, modern))
            }
            Expr::Name(prop) if !self.is_local_or_outer_var(prop) => {
                let modern = map_as2_percent_prop_to_modern(prop)?;
                Some(("this".into(), modern))
            }
            _ => None,
        }
    }

    fn emit_member_expr(&mut self, base: &Expr, prop: &str, offset: usize) -> String {
        // 1. AS2 `Key.*` and AS3 `Keyboard.*` constants -> integer keyCode literals
        if let Expr::Name(cls) = base {
            if cls == "Key"
                && let Some((_, code)) = map_as2_key_constant(prop)
            {
                return code.to_string();
            }
            if cls == "Keyboard" {
                if let Some((_, code)) = map_as3_keyboard_constant(prop) {
                    return code.to_string();
                }
                if prop.len() == 1 {
                    let ch = prop.as_bytes()[0];
                    if ch.is_ascii_uppercase() || ch.is_ascii_digit() {
                        return ch.to_string();
                    }
                }
            }

            // Event / MouseEvent / KeyboardEvent / TimerEvent string constants
            if let Some(ev_str) = classify_as3_event_type(&Expr::Member(
                Box::new(Expr::Name(cls.clone())),
                prop.to_owned(),
            )) {
                return format!("\"{ev_str}\"");
            }
            if cls == "Event" {
                match prop {
                    "REMOVED_FROM_STAGE" => return "\"removedFromStage\"".into(),
                    "REMOVED" => return "\"removed\"".into(),
                    "CHANGE" => return "\"change\"".into(),
                    _ => {}
                }
            }
            if cls == "MouseEvent" && prop == "MOUSE_MOVE" {
                return "\"mouseMove\"".into();
            }
            if cls == "TimerEvent" {
                match prop {
                    "TIMER" => return "\"timer\"".into(),
                    "TIMER_COMPLETE" => return "\"timerComplete\"".into(),
                    _ => {}
                }
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

            // AS2 `Stage.width` / `Stage.height` / `Stage.scaleMode` / `Stage.align`
            if !self.source_is_as3 && cls == "Stage" {
                return match prop {
                    "width" => "this.stage.stageWidth".into(),
                    "height" => "this.stage.stageHeight".into(),
                    "scaleMode" => "this.stage.scaleMode".into(),
                    "align" => "this.stage.align".into(),
                    "displayState" => "this.stage.displayState".into(),
                    _ => format!("this.stage.{prop}"),
                };
            }
        }

        // 2. AS3 `stage.stage3Ds` guardrail
        if is_stage_expr(base) && prop == "stage3Ds" {
            self.record_issue(
                ConversionIssueKind::ManualAttention,
                offset,
                "Stage3D GPU contexts are not supported in NeoFlash 2D JS; manual WebGL migration required.",
            );
            return "/* FIXME [NeoFlash Convert]: stage.stage3Ds */ []".into();
        }

        // 3. AS1/AS2 `_alpha`/`_xscale`/`_yscale` read (`0..1` -> `0..100`) and `_x` -> `x`
        if !self.source_is_as3 {
            let b = self.emit_subexpr(base, 18, offset);
            if let Some(modern) = map_as2_percent_prop_to_modern(prop) {
                return format!("({b}.{modern} * 100)");
            }
            if let Some(modern) = map_as2_direct_prop_to_modern(prop) {
                return format!("{b}.{modern}");
            }
            return format!("{b}.{prop}");
        }

        let b = self.emit_subexpr(base, 18, offset);
        format!("{b}.{prop}")
    }

    fn emit_call_expr(&mut self, callee: &Expr, args: &[Expr], offset: usize) -> String {
        // 1. Type casts and bare timeline methods
        if let Expr::Name(fn_name) = callee {
            match fn_name.as_str() {
                "int" if args.len() == 1 => {
                    let inner = self.emit_expr_with_offset(&args[0], offset);
                    return format!("(({inner}) | 0)");
                }
                "uint" if args.len() == 1 => {
                    let inner = self.emit_expr_with_offset(&args[0], offset);
                    return format!("(({inner}) >>> 0)");
                }
                "Number" | "String" | "Boolean" if args.len() == 1 => {
                    let inner = self.emit_expr_with_offset(&args[0], offset);
                    return format!("{fn_name}({inner})");
                }
                "stop" | "play" | "nextFrame" | "prevFrame" | "gotoAndPlay" | "gotoAndStop"
                | "addChild" | "addChildAt" | "removeChild" | "removeChildAt"
                | "getChildByName" | "getChildAt" | "dispatchEvent"
                    if !self.is_local_or_outer_var(fn_name) =>
                {
                    let a = self.emit_args(args, offset);
                    return format!("this.{fn_name}({a})");
                }
                "addEventListener" | "removeEventListener"
                    if !self.is_local_or_outer_var(fn_name) =>
                {
                    let a = self.emit_event_listener_args(args, offset);
                    return format!("this.{fn_name}({a})");
                }
                "getTimer" if args.is_empty() => {
                    return "Math.trunc(performance.now())".into();
                }
                "eval" if !self.source_is_as3 && args.len() == 1 => {
                    let inner = self.emit_expr_with_offset(&args[0], offset);
                    return format!("this[{inner}]");
                }
                other
                    if self.source_is_as3
                        && args.len() == 1
                        && !self.is_local_or_outer_var(other)
                        && (self.module_paths_by_short.contains_key(other)
                            || matches!(
                                other,
                                "MovieClip"
                                    | "Sprite"
                                    | "DisplayObject"
                                    | "TextField"
                                    | "Object"
                                    | "Array"
                            )) =>
                {
                    // AS3 cast `MovieClip(expr)` -> `expr`
                    if other == "Array" {
                        return format!("Array({})", self.emit_expr_with_offset(&args[0], offset));
                    }
                    return self.emit_expr_with_offset(&args[0], offset);
                }
                _ => {}
            }
        }

        // 2. Member calls (`addEventListener`, AS2 `Key.*`, AS2 drawing API, `createEmptyMovieClip`)
        if let Expr::Member(base, method) = callee {
            if matches!(method.as_str(), "addEventListener" | "removeEventListener") {
                let b = self.emit_subexpr(base, 18, offset);
                let a = self.emit_event_listener_args(args, offset);
                return format!("{b}.{method}({a})");
            }

            if !self.source_is_as3 {
                if let Expr::Name(cls) = base.as_ref()
                    && cls == "Key"
                {
                    match method.as_str() {
                        "getCode" => {
                            if let Some(ev) = &self.in_key_handler_event {
                                return format!("{ev}.keyCode");
                            }
                            return "0".into();
                        }
                        "getAscii" => {
                            if let Some(ev) = &self.in_key_handler_event {
                                return format!("({ev}.charCode || {ev}.keyCode)");
                            }
                            return "0".into();
                        }
                        "isDown" if args.len() == 1 => {
                            let code = self.emit_expr_with_offset(&args[0], offset);
                            if self.use_key_down_shim {
                                return format!("_isKeyDown({code})");
                            }
                            return format!(
                                "Boolean(globalThis._keysDown && globalThis._keysDown[{code}])"
                            );
                        }
                        _ => {}
                    }
                }

                // AS2 Drawing API on MovieClip -> `.graphics.*` with `0..100` -> `0..1` alpha
                if matches!(
                    method.as_str(),
                    "clear"
                        | "beginFill"
                        | "lineStyle"
                        | "moveTo"
                        | "lineTo"
                        | "curveTo"
                        | "endFill"
                ) && !matches!(base.as_ref(), Expr::Member(_, g) if g == "graphics")
                    && !matches!(base.as_ref(), Expr::Name(n) if n == "graphics")
                {
                    let b = self.emit_subexpr(base, 18, offset);
                    let mapped_args = match method.as_str() {
                        "beginFill" if args.len() >= 2 => {
                            let mut parts = vec![
                                self.emit_expr_with_offset(&args[0], offset),
                                self.emit_scaled_100_to_01(&args[1], offset),
                            ];
                            for extra in &args[2..] {
                                parts.push(self.emit_expr_with_offset(extra, offset));
                            }
                            parts.join(", ")
                        }
                        "lineStyle" if args.len() >= 3 => {
                            let mut parts = vec![
                                self.emit_expr_with_offset(&args[0], offset),
                                self.emit_expr_with_offset(&args[1], offset),
                                self.emit_scaled_100_to_01(&args[2], offset),
                            ];
                            for extra in &args[3..] {
                                parts.push(self.emit_expr_with_offset(extra, offset));
                            }
                            parts.join(", ")
                        }
                        _ => self.emit_args(args, offset),
                    };
                    return format!("{b}.graphics.{method}({mapped_args})");
                }

                if method == "getNextHighestDepth" && args.is_empty() {
                    let b = self.emit_subexpr(base, 18, offset);
                    return format!("({b}.numChildren || 0)");
                }
            }
        }

        // Inline expression-position `createEmptyMovieClip`
        if !self.source_is_as3
            && let Some(parent_expr) = match_clip_factory(callee, "createEmptyMovieClip")
            && !args.is_empty()
        {
            self.needs_sprite_shim = true;
            let p = self.emit_expr_with_offset(&parent_expr, offset);
            let n = self.emit_expr_with_offset(&args[0], offset);
            return format!(
                "(() => {{ const _c = new Sprite(); _c.name = {n}; {p}.addChild(_c); return _c; }})()"
            );
        }

        let c = self.emit_subexpr(callee, 18, offset);
        let a = self.emit_args(args, offset);
        format!("{c}({a})")
    }

    fn emit_event_listener_args(&mut self, args: &[Expr], offset: usize) -> String {
        let mut parts = Vec::new();
        for (idx, arg) in args.iter().enumerate() {
            if idx == 1 {
                parts.push(self.emit_listener_arg_expr(arg, offset));
            } else {
                parts.push(self.emit_expr_with_offset(arg, offset));
            }
        }
        parts.join(", ")
    }

    fn emit_new_expr(&mut self, ty: &str, args: &[Expr], offset: usize) -> String {
        let short = short_type_name(ty);
        match short {
            "Dictionary" => {
                let weak = args.first().is_some_and(|a| {
                    matches!(a, Expr::Literal(s) if s == "true")
                        || matches!(a, Expr::Name(s) if s == "true")
                });
                if weak {
                    "new WeakMap()".into()
                } else {
                    "new Map()".into()
                }
            }
            "ByteArray" => {
                self.record_issue(
                    ConversionIssueKind::Approximated,
                    offset,
                    "flash.utils.ByteArray approximated with Uint8Array in JavaScript.",
                );
                "new Uint8Array()".into()
            }
            "Stage3D" | "Context3D" => {
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    format!("{short} is not supported in NeoFlash 2D JS."),
                );
                format!("/* FIXME [NeoFlash Convert]: new {short}() */ null")
            }
            "Sprite" => {
                self.needs_sprite_shim = true;
                format!("new Sprite({})", self.emit_args(args, offset))
            }
            other => {
                self.note_class_reference(other);
                format!("new {other}({})", self.emit_args(args, offset))
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
        let my_prec = expr_prec(expr);
        let s = self.emit_expr_with_offset(expr, offset);
        if my_prec < parent_prec {
            format!("({s})")
        } else {
            s
        }
    }

    fn emit_subexpr_right(&mut self, expr: &Expr, parent_prec: u8, offset: usize) -> String {
        let my_prec = expr_prec(expr);
        let s = self.emit_expr_with_offset(expr, offset);
        if my_prec <= parent_prec && my_prec < 18 {
            format!("({s})")
        } else {
            s
        }
    }

    fn emit_lifted_handler(&mut self, handler: &LiftedObjectHandler) -> String {
        let pad = self.pad();
        let target = self.emit_expr_with_offset(&handler.target(), 0);
        let rewritten_body = handler.scoped_body();

        match &handler.trigger {
            LiftedTriggerKind::NeedsBinding(event) => {
                let function = self.next_helper("bind_legacy_event");
                let note = format!(
                    "Connect {function} to the data source or event corresponding to legacy '{event}'."
                );
                self.record_issue(ConversionIssueKind::ManualAttention, 0, &note);
                self.push_fn_scope();
                self.indent += 1;
                let body = self.emit_stmt_list(&rewritten_body);
                self.indent -= 1;
                self.pop_fn_scope();
                format!(
                    "{pad}/* FIXME [NeoFlash Convert]: {note} */\n{pad}function {function}() {{\n{body}\n{pad}}}"
                )
            }
            LiftedTriggerKind::Load => {
                self.push_block_scope();
                self.indent += 1;
                let inner = self.emit_stmt_list(&rewritten_body);
                self.indent -= 1;
                self.pop_block_scope();
                if inner.trim().is_empty() {
                    String::new()
                } else {
                    format!("{pad}if ({target}) {{\n{inner}\n{pad}}}")
                }
            }
            LiftedTriggerKind::KeyPress(spec) => {
                let fn_name = self.next_helper("on_object_key");
                let saved_ke = self.in_key_handler_event.clone();
                self.in_key_handler_event = Some("event".into());
                self.push_fn_scope();
                self.mark_declared("event");
                self.indent += 1;
                let gpad = self.pad();
                let cond = if let Some((_, key_code)) = key_press_spec_to_code(spec) {
                    format!("event.keyCode === {key_code}")
                } else {
                    "true".into()
                };
                self.indent += 1;
                let inner = self.emit_stmt_list(&rewritten_body);
                self.indent -= 1;
                let body = format!("{gpad}if ({cond}) {{\n{inner}\n{gpad}}}");
                self.indent -= 1;
                self.pop_fn_scope();
                self.in_key_handler_event = saved_ke;
                format!(
                    "{pad}const {fn_name} = (event) => {{\n{body}\n{pad}}};\n\
                     {pad}if (this.stage) {{\n\
                     {pad}    this.stage.focus = this;\n\
                     {pad}    this.stage.addEventListener(\"keyDown\", {fn_name});\n\
                     {pad}}}"
                )
            }
            other => {
                let (is_stage_key, ev_str, suffix) = match other {
                    LiftedTriggerKind::Unload => (false, "removedFromStage", "unload"),
                    LiftedTriggerKind::StageMouseMove => (false, "mouseMove", "stageMouseMove"),
                    LiftedTriggerKind::StageMouseDown => (false, "mouseDown", "stageMouseDown"),
                    LiftedTriggerKind::StageMouseUp => (false, "mouseUp", "stageMouseUp"),
                    LiftedTriggerKind::EnterFrame => (false, "enterFrame", "enterFrame"),
                    LiftedTriggerKind::MouseClick => (false, "click", "click"),
                    LiftedTriggerKind::MouseDown => (false, "mouseDown", "mouseDown"),
                    LiftedTriggerKind::MouseUp => (false, "mouseUp", "mouseUp"),
                    LiftedTriggerKind::RollOver => (false, "rollOver", "rollOver"),
                    LiftedTriggerKind::RollOut => (false, "rollOut", "rollOut"),
                    LiftedTriggerKind::KeyDown => (true, "keyDown", "keyDown"),
                    LiftedTriggerKind::KeyUp => (true, "keyUp", "keyUp"),
                    LiftedTriggerKind::Load
                    | LiftedTriggerKind::KeyPress(_)
                    | LiftedTriggerKind::NeedsBinding(_) => unreachable!(),
                };
                let target = if matches!(
                    other,
                    LiftedTriggerKind::StageMouseDown
                        | LiftedTriggerKind::StageMouseUp
                        | LiftedTriggerKind::StageMouseMove
                ) {
                    "this.stage"
                } else {
                    &target
                };
                let fn_name = self.next_helper(&format!("on_object_{suffix}"));
                let saved_ke = self.in_key_handler_event.clone();
                if is_stage_key {
                    self.in_key_handler_event = Some("event".into());
                }
                self.push_fn_scope();
                self.mark_declared("event");
                self.indent += 1;
                let inner = self.emit_stmt_list(&rewritten_body);
                self.indent -= 1;
                self.pop_fn_scope();
                self.in_key_handler_event = saved_ke;

                if is_stage_key {
                    format!(
                        "{pad}const {fn_name} = (event) => {{\n{inner}\n{pad}}};\n\
                         {pad}if (this.stage) {{\n\
                         {pad}    this.stage.focus = this;\n\
                         {pad}    this.stage.addEventListener(\"{ev_str}\", {fn_name});\n\
                         {pad}}}"
                    )
                } else {
                    format!(
                        "{pad}const {fn_name} = (event) => {{\n{inner}\n{pad}}};\n\
                         {pad}if ({target}) {{\n\
                         {pad}    {target}.addEventListener(\"{ev_str}\", {fn_name});\n\
                         {pad}}}"
                    )
                }
            }
        }
    }

    fn format_es_imports(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for cls in &self.imported_classes {
            if let Some(target_path) = self.module_paths_by_short.get(cls) {
                let spec = relative_module_specifier(self.current_module_path, target_path);
                lines.push(format!("import {{ {cls} }} from \"{spec}\";"));
            }
        }
        lines
    }
}

pub(crate) fn emit_js_timeline_script(
    parsed: &ParsedScript,
    lifted: &[LiftedObjectHandler],
    symbol_id: &str,
    location: &str,
    stage: &ProjectStageIndex,
    module_paths_by_short: &BTreeMap<String, String>,
) -> JsEmitOutput {
    let source_is_as3 = parsed.language == ScriptLanguage::ActionScript3;
    let mut emitter = JsEmitter::new(
        location,
        &parsed.expanded_source,
        source_is_as3,
        stage,
        module_paths_by_short,
        "",
        Some(symbol_id.to_owned()),
        None,
    );
    emitter.precollect(&parsed.statements);

    let needs_key_shim = !source_is_as3
        && (stmts_use_key_is_down(&parsed.statements)
            || lifted.iter().any(|h| stmts_use_key_is_down(&h.body)));
    emitter.use_key_down_shim = needs_key_shim;

    let mut sections = Vec::new();
    if needs_key_shim {
        emitter.record_issue(
            ConversionIssueKind::ShimInjected,
            0,
            "Injected stage keyDown/keyUp listener shim for AS2 Key.isDown() in JavaScript.",
        );
        sections.push(emitter.key_down_shim_block());
    }

    let main_body = emitter.emit_stmt_list(&parsed.statements);
    if !main_body.trim().is_empty() {
        sections.push(main_body);
    }

    for handler in lifted {
        let code = emitter.emit_lifted_handler(handler);
        if !code.trim().is_empty() {
            sections.push(code);
        }
    }

    // Export top-level timeline functions onto `this` so child clips, external frames, and test hosts
    // can invoke `clip.fnName()` directly while preserving lexical `fnName()` calls inside the frame.
    let mut exported_fns = Vec::new();
    for stmt in &parsed.statements {
        if let Stmt::Function(f) = stmt.unlocated()
            && let Some(fname) = &f.name
        {
            let safe = sanitize_js_ident(fname);
            exported_fns.push(format!("this.{safe} = {safe};"));
        }
    }
    if !exported_fns.is_empty() {
        sections.push(exported_fns.join("\n"));
    }

    if emitter.needs_sprite_shim {
        sections.insert(0, JsEmitter::sprite_shim_block().to_owned());
    }

    let import_lines = emitter.format_es_imports();
    let body = sections.join("\n\n");
    let full = if import_lines.is_empty() {
        format!("{body}\n")
    } else if body.trim().is_empty() {
        format!("{}\n", import_lines.join("\n"))
    } else {
        format!("{}\n\n{body}\n", import_lines.join("\n"))
    };

    JsEmitOutput {
        source: full,
        module_path: None,
        issues: emitter.issues,
        synthesized_linkages: emitter.synthesized_linkages.into_values().collect(),
    }
}

pub(crate) fn emit_js_class(
    parsed: &ParsedClass,
    unit_path: &str,
    location: &str,
    stage: &ProjectStageIndex,
    module_paths_by_short: &BTreeMap<String, String>,
) -> JsEmitOutput {
    let mod_path = to_js_module_path(unit_path, &parsed.qualified_name);
    let mod_path_ref = mod_path.clone();
    let source_is_as3 = parsed.language == ScriptLanguage::ActionScript3;

    let mut emitter = JsEmitter::new(
        location,
        &parsed.source,
        source_is_as3,
        stage,
        module_paths_by_short,
        &mod_path_ref,
        None,
        Some(parsed.qualified_name.clone()),
    );

    if parsed.is_interface {
        return JsEmitOutput {
            source: format!(
                "// Interface {} (erased at JavaScript runtime)\nexport const {} = Object.freeze({{}});\n",
                parsed.qualified_name, parsed.short_name
            ),
            module_path: Some(mod_path),
            issues: Vec::new(),
            synthesized_linkages: Vec::new(),
        };
    }

    let mut instance_fields = BTreeSet::new();
    let mut static_fields = BTreeSet::new();
    let mut instance_methods = BTreeSet::new();
    let mut static_methods = BTreeSet::new();

    // Inherit base class fields/methods from ProjectStageIndex so implicit `this.` works across inheritance!
    let mut cur_base = parsed.base_class.clone();
    let mut visited = BTreeSet::new();
    while let Some(b) = cur_base {
        if !visited.insert(b.clone()) {
            break;
        }
        let qname = stage
            .project_classes
            .get(&b)
            .cloned()
            .unwrap_or_else(|| b.clone());
        if let Some(fmap) = stage.class_fields.get(&qname) {
            for k in fmap.keys() {
                instance_fields.insert(k.clone());
            }
        }
        if let Some(mset) = stage.class_instance_methods.get(&qname) {
            for m in mset {
                instance_methods.insert(m.clone());
            }
        }
        cur_base = stage.class_bases.get(&qname).cloned();
    }

    for f in &parsed.fields {
        let ty =
            f.ty.as_deref()
                .map_or(HirType::Unknown, HirType::from_annotation);
        if f.is_static {
            static_fields.insert(f.name.clone());
        } else {
            instance_fields.insert(f.name.clone());
            emitter.env.define(f.name.clone(), ty);
        }
    }
    for m in &parsed.methods {
        if let Some(mname) = &m.function.name
            && mname != &parsed.short_name
        {
            if m.is_static {
                static_methods.insert(mname.clone());
            } else {
                instance_methods.insert(mname.clone());
            }
        }
    }

    let extends_short = parsed.base_class.as_deref().and_then(|b| {
        let s = short_type_name(b);
        if s == "Object" {
            None
        } else {
            Some(s.to_owned())
        }
    });

    let mut need_display_base_shim: Option<String> = None;
    if let Some(ext) = &extends_short {
        if module_paths_by_short.contains_key(ext) {
            emitter.note_class_reference(ext);
        } else if ext == "Sprite" {
            emitter.needs_sprite_shim = true;
        } else if matches!(
            ext.as_str(),
            "MovieClip" | "SimpleButton" | "Button" | "EventDispatcher" | "DisplayObject"
        ) {
            need_display_base_shim = Some(ext.clone());
        }
    }

    emitter.class_ctx = Some(JsClassContext {
        short_name: parsed.short_name.clone(),
        instance_fields,
        static_fields,
        instance_methods,
        static_methods,
    });

    emitter.indent = 1;
    let mut member_blocks = Vec::new();

    for f in &parsed.fields {
        let pad = emitter.pad();
        let stat = if f.is_static { "static " } else { "" };
        let fname = sanitize_js_ident(&f.name);
        if let Some(val) = &f.value {
            let vc = emitter.emit_expr_with_offset(val, f.offset);
            member_blocks.push(format!("{pad}{stat}{fname} = {vc};"));
        } else {
            let def = match f.ty.as_deref().map(HirType::from_annotation) {
                Some(HirType::Int | HirType::UInt | HirType::Float) => "0",
                Some(HirType::Bool) => "false",
                _ => "null",
            };
            member_blocks.push(format!("{pad}{stat}{fname} = {def};"));
        }
    }

    for m in &parsed.methods {
        let pad = emitter.pad();
        let raw_name = m.function.name.as_deref().unwrap_or(&parsed.short_name);
        let is_ctor = raw_name == parsed.short_name && m.accessor.is_none() && !m.is_static;
        let mname = if is_ctor {
            "constructor".to_owned()
        } else {
            sanitize_js_ident(raw_name)
        };
        let stat = if m.is_static && !is_ctor {
            "static "
        } else {
            ""
        };
        let get_set = match m.accessor {
            Some(AccessorKind::Get) => "get ",
            Some(AccessorKind::Set) => "set ",
            None => "",
        };

        let is_key_fn = m.function.params.first().is_some_and(|(_, ty)| {
            ty.as_deref()
                .is_some_and(|t| short_type_name(t) == "KeyboardEvent")
        });
        let saved_ke = emitter.in_key_handler_event.clone();
        if is_key_fn {
            emitter.in_key_handler_event = m.function.params.first().map(|(n, _)| n.clone());
        }

        emitter.push_fn_scope();
        let params_str = emitter.emit_fn_params(&m.function, m.function.offset);
        emitter.indent = 2;
        let mut body_parts = Vec::new();
        if is_ctor && extends_short.is_some() && !stmts_contain_super_call(&m.function.body) {
            body_parts.push(format!("{}super();", emitter.pad()));
        }
        let body_str = emitter.emit_stmt_list(&m.function.body);
        if !body_str.trim().is_empty() {
            body_parts.push(body_str);
        }
        emitter.indent = 1;
        emitter.pop_fn_scope();
        emitter.in_key_handler_event = saved_ke;

        if body_parts.is_empty() {
            member_blocks.push(format!("{pad}{stat}{get_set}{mname}({params_str}) {{}}"));
        } else {
            member_blocks.push(format!(
                "{pad}{stat}{get_set}{mname}({params_str}) {{\n{}\n{pad}}}",
                body_parts.join("\n")
            ));
        }
    }

    let extends_clause = extends_short
        .as_ref()
        .map(|b| format!(" extends {b}"))
        .unwrap_or_default();

    let class_decl = if member_blocks.is_empty() {
        format!("export class {}{extends_clause} {{}}", parsed.short_name)
    } else {
        format!(
            "export class {}{extends_clause} {{\n{}\n}}",
            parsed.short_name,
            member_blocks.join("\n\n")
        )
    };

    let mut preambles = emitter.format_es_imports();
    if emitter.needs_sprite_shim {
        preambles.push(JsEmitter::sprite_shim_block().to_owned());
    }
    if let Some(base_cls) = need_display_base_shim {
        preambles.push(format!(
            "const {base_cls} = globalThis.{base_cls} || class {base_cls} {{\n\
             \x20   constructor() {{ this.x = 0; this.y = 0; this.scaleX = 1; this.scaleY = 1; this.alpha = 1; this.rotation = 0; this.visible = true; this.children = []; }}\n\
             \x20   addChild(c) {{ this.children.push(c); return c; }}\n\
             \x20   addEventListener() {{}}\n\
             \x20   removeEventListener() {{}}\n\
             }};"
        ));
    }

    let full = if preambles.is_empty() {
        format!("{class_decl}\n")
    } else {
        format!("{}\n\n{class_decl}\n", preambles.join("\n"))
    };

    JsEmitOutput {
        source: full,
        module_path: Some(mod_path),
        issues: emitter.issues,
        synthesized_linkages: emitter.synthesized_linkages.into_values().collect(),
    }
}

pub(crate) fn to_js_module_path(unit_path: &str, qualified_name: &str) -> String {
    let normalized = crate::modules::normalize_module_path(unit_path);
    if !normalized.is_empty() {
        if let Some(stem) = normalized.strip_suffix(".as") {
            return format!("{stem}.js");
        }
        if normalized.ends_with(".js") || normalized.ends_with(".mjs") {
            return normalized;
        }
    }
    if !qualified_name.is_empty() {
        return format!("{}.js", qualified_name.replace('.', "/"));
    }
    "Module.js".into()
}

pub(crate) fn relative_module_specifier(from_module_path: &str, to_module_path: &str) -> String {
    let from_norm = crate::modules::normalize_module_path(from_module_path);
    let to_norm = crate::modules::normalize_module_path(to_module_path);
    if from_norm.is_empty() {
        return format!("./{to_norm}");
    }
    let from_parts: Vec<&str> = from_norm.split('/').collect();
    let to_parts: Vec<&str> = to_norm.split('/').collect();
    let from_dir = &from_parts[..from_parts.len().saturating_sub(1)];
    let mut common = 0usize;
    while common < from_dir.len()
        && common + 1 < to_parts.len()
        && from_dir[common] == to_parts[common]
    {
        common += 1;
    }
    let up_count = from_dir.len() - common;
    let mut rel = if up_count == 0 {
        vec!["."]
    } else {
        vec![".."; up_count]
    };
    for seg in &to_parts[common..] {
        rel.push(seg);
    }
    rel.join("/")
}

fn sanitize_js_ident(raw: &str) -> String {
    match raw {
        "let" | "yield" | "await" | "enum" | "implements" | "interface" | "package" | "private"
        | "protected" | "public" | "static" => format!("_{raw}"),
        other => other.to_owned(),
    }
}

fn collect_nested_var_decls(stmt: &Stmt, out: &mut Vec<String>, seen: &mut BTreeSet<String>) {
    match stmt.unlocated() {
        Stmt::Var(vars) => {
            for (vname, _, _) in vars {
                if seen.insert(vname.clone()) {
                    out.push(vname.clone());
                }
            }
        }
        Stmt::Function(_) | Stmt::LegacyEvent(_, _) => {}
        other => {
            for child in crate::actionscript::scope::statements(other) {
                collect_nested_var_decls(child, out, seen);
            }
        }
    }
}

fn stmts_contain_super_call(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s.unlocated() {
        Stmt::Expr(Expr::Call(callee, _, _)) => {
            matches!(callee.as_ref(), Expr::Name(n) if n == "super")
        }
        Stmt::Block(b) => stmts_contain_super_call(b),
        _ => false,
    })
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
                "onEnterFrame" | "onKeyDown" | "onKeyUp" | "onResize" | "onRelease" | "onPress"
            ) =>
        {
            Some((Expr::Name("this".into()), prop.as_str()))
        }
        _ => None,
    }
}

fn is_nullish_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::Literal(s) if s == "null")
        || matches!(expr, Expr::Name(s) if s == "undefined" || s == "null")
}

fn expr_prec(expr: &Expr) -> u8 {
    match expr {
        Expr::Assign(_, _, _) => 2,
        Expr::Conditional(_, _, _) => 3,
        Expr::Binary(op, _, _) => op_prec(op),
        Expr::TypeTest(_, _, _, _) | Expr::InstanceOf(_, _) => 10,
        Expr::Unary(_, _) | Expr::Delete(_, _) => 16,
        Expr::Update(_, _, _) => 17,
        _ => 19,
    }
}
