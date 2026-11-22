//! Deterministic AST-to-ActionScript 3.0 emitter (`AS1/AS2 -> AS3`).

use super::hir::{
    AccessorKind, HirType, LiftedObjectHandler, LiftedTriggerKind, MemberAccess, ParsedClass,
    ParsedScript, ProjectStageIndex, TypeEnv, decode_string_literal, fold_100_to_01,
    is_atomic_expr, key_press_spec_to_code, map_as2_direct_prop_to_modern, map_as2_key_constant,
    map_as2_percent_prop_to_modern, rewrite_lifted_clip_scope_stmts, sanitize_as_class_ident,
    short_type_name, stmts_use_key_is_down,
};
use super::{ConversionIssue, ConversionIssueKind};
use crate::actionscript::ast::{Case, CatchClause, Expr, Function, PropertyKey, Stmt};
use crate::byte_offset_to_line_col;
use neoflash_core::SymbolKind;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub(crate) struct SynthesizedLinkage {
    pub(crate) linkage_id: String,
    pub(crate) class_name: String,
    pub(crate) symbol_kind: SymbolKind,
}

pub(crate) struct As3EmitOutput {
    pub(crate) source: String,
    pub(crate) issues: Vec<ConversionIssue>,
    pub(crate) synthesized_linkages: Vec<SynthesizedLinkage>,
}

struct As3Emitter<'a> {
    location: &'a str,
    raw_source: &'a str,
    stage: &'a ProjectStageIndex,
    env: TypeEnv,
    imports: BTreeSet<String>,
    issues: Vec<ConversionIssue>,
    synthesized_linkages: BTreeMap<String, SynthesizedLinkage>,
    declared_in_scope: Vec<BTreeSet<String>>,
    in_key_handler_event: Option<String>,
    use_key_down_shim: bool,
    helper_counter: usize,
    indent: usize,
    /// Map from listener variable/this key (`"this"` or var name) -> generated AS3 handler function names
    key_down_handlers: BTreeMap<String, String>,
    key_up_handlers: BTreeMap<String, String>,
    resize_handlers: BTreeMap<String, String>,
    /// Set of listener targets that have an explicit `Key.addListener(t)` or `Stage.addListener(t)` in the same block
    explicit_key_listeners: BTreeSet<String>,
    explicit_stage_listeners: BTreeSet<String>,
}

impl<'a> As3Emitter<'a> {
    fn new(
        location: &'a str,
        raw_source: &'a str,
        stage: &'a ProjectStageIndex,
        symbol_id: Option<String>,
        class_name: Option<String>,
    ) -> Self {
        Self {
            location,
            raw_source,
            stage,
            env: TypeEnv::new(symbol_id, class_name),
            imports: BTreeSet::new(),
            issues: Vec::new(),
            synthesized_linkages: BTreeMap::new(),
            declared_in_scope: vec![BTreeSet::new()],
            in_key_handler_event: None,
            use_key_down_shim: false,
            helper_counter: 0,
            indent: 0,
            key_down_handlers: BTreeMap::new(),
            key_up_handlers: BTreeMap::new(),
            resize_handlers: BTreeMap::new(),
            explicit_key_listeners: BTreeSet::new(),
            explicit_stage_listeners: BTreeSet::new(),
        }
    }

    fn pad(&self) -> String {
        "    ".repeat(self.indent)
    }

    fn next_helper(&mut self, prefix: &str) -> String {
        self.helper_counter += 1;
        if self.helper_counter == 1 {
            format!("__{prefix}")
        } else {
            format!("__{prefix}_{}", self.helper_counter)
        }
    }

    fn push_fn_scope(&mut self) {
        self.env.push_scope();
        self.declared_in_scope.push(BTreeSet::new());
    }

    fn pop_fn_scope(&mut self) {
        self.env.pop_scope();
        if self.declared_in_scope.len() > 1 {
            self.declared_in_scope.pop();
        }
    }

    fn mark_declared(&mut self, name: &str) -> bool {
        if let Some(set) = self.declared_in_scope.last_mut() {
            set.insert(name.to_owned())
        } else {
            true
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

    fn map_type_annotation(&mut self, raw: &str, is_return: bool) -> String {
        let trimmed = raw.trim();
        match trimmed {
            "Void" | "void" if is_return => "void".into(),
            "Button" | "SimpleButton" => {
                self.imports.insert("flash.display.SimpleButton".into());
                "SimpleButton".into()
            }
            "MovieClip" => {
                self.imports.insert("flash.display.MovieClip".into());
                "MovieClip".into()
            }
            "Sprite" => {
                self.imports.insert("flash.display.Sprite".into());
                "Sprite".into()
            }
            "TextField" => {
                self.imports.insert("flash.text.TextField".into());
                "TextField".into()
            }
            "TextFormat" => {
                self.imports.insert("flash.text.TextFormat".into());
                "TextFormat".into()
            }
            "Sound" => {
                self.imports.insert("flash.media.Sound".into());
                "Sound".into()
            }
            "SharedObject" => {
                self.imports.insert("flash.net.SharedObject".into());
                "SharedObject".into()
            }
            "Point" | "flash.geom.Point" => {
                self.imports.insert("flash.geom.Point".into());
                "Point".into()
            }
            "Rectangle" | "flash.geom.Rectangle" => {
                self.imports.insert("flash.geom.Rectangle".into());
                "Rectangle".into()
            }
            "Matrix" | "flash.geom.Matrix" => {
                self.imports.insert("flash.geom.Matrix".into());
                "Matrix".into()
            }
            "ColorTransform" | "flash.geom.ColorTransform" => {
                self.imports.insert("flash.geom.ColorTransform".into());
                "ColorTransform".into()
            }
            "BitmapData" | "flash.display.BitmapData" => {
                self.imports.insert("flash.display.BitmapData".into());
                "BitmapData".into()
            }
            "LoadVars" | "Color" | "MovieClipLoader" => "Object".into(),
            other => {
                if let Some(qname) = self.stage.project_classes.get(other)
                    && qname.contains('.')
                {
                    self.imports.insert(qname.clone());
                } else if other.contains('.') && !other.starts_with("Vector.<") {
                    self.imports.insert(other.to_owned());
                    return short_type_name(other).to_owned();
                }
                other.to_owned()
            }
        }
    }

    fn preserve_compatible_imports(&mut self, raw_imports: &[String]) {
        for imp in raw_imports {
            if imp.starts_with("mx.") {
                continue;
            }
            self.imports.insert(imp.clone());
        }
    }

    fn precollect_functions_and_listeners(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match stmt.unlocated() {
                Stmt::Function(f) => {
                    if let Some(name) = &f.name {
                        let ret = f
                            .returns
                            .as_deref()
                            .map_or(HirType::Unknown, HirType::from_annotation);
                        self.env.functions.insert(name.clone(), ret);
                    }
                }
                Stmt::Expr(Expr::Call(callee, args, _)) => {
                    if let Expr::Member(base, method) = callee.as_ref()
                        && method == "addListener"
                        && let Some(first) = args.first()
                    {
                        let target_key = listener_target_key(first);
                        if matches!(base.as_ref(), Expr::Name(n) if n == "Key")
                            && let Some(k) = target_key
                        {
                            self.explicit_key_listeners.insert(k);
                        } else if matches!(base.as_ref(), Expr::Name(n) if n == "Stage")
                            && let Some(k) = target_key
                        {
                            self.explicit_stage_listeners.insert(k);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn resolve_linkage_class(&mut self, linkage_id: &str) -> String {
        if let Some(info) = self.stage.linkages.get(linkage_id) {
            let cls = info.class_name.clone();
            self.synthesized_linkages
                .entry(linkage_id.to_owned())
                .or_insert_with(|| SynthesizedLinkage {
                    linkage_id: linkage_id.to_owned(),
                    class_name: cls.clone(),
                    symbol_kind: info.symbol_kind,
                });
            let short = short_type_name(&cls).to_owned();
            if cls.contains('.') {
                self.imports.insert(cls);
            }
            short
        } else {
            let cls = sanitize_as_class_ident(linkage_id);
            self.synthesized_linkages
                .entry(linkage_id.to_owned())
                .or_insert_with(|| SynthesizedLinkage {
                    linkage_id: linkage_id.to_owned(),
                    class_name: cls.clone(),
                    symbol_kind: SymbolKind::MovieClip,
                });
            cls
        }
    }

    fn emit_add_child_on(&self, parent_expr: &Expr, child_ident: &str) -> String {
        let pad = self.pad();
        if matches!(parent_expr, Expr::Name(n) if n == "this") {
            format!("{pad}addChild({child_ident});")
        } else {
            let recv = self.emit_expr_immut(parent_expr);
            format!("{pad}{recv}.addChild({child_ident});")
        }
    }

    fn emit_init_object_assignments(
        &mut self,
        target_ident: &str,
        init_expr: &Expr,
        out_lines: &mut Vec<String>,
    ) {
        let pad = self.pad();
        if let Expr::Object(props) = init_expr {
            for (PropertyKey { name, .. }, val) in props {
                let lhs = Expr::Member(Box::new(Expr::Name(target_ident.to_owned())), name.clone());
                let assign = Expr::Assign("=".into(), Box::new(lhs), Box::new(val.clone()));
                let rendered = self.emit_expr(&assign);
                out_lines.push(format!("{pad}{rendered};"));
            }
        } else {
            let init_code = self.emit_expr(init_expr);
            let init_var = self.next_helper("initObj");
            let key_var = self.next_helper("initKey");
            out_lines.push(format!("{pad}var {init_var}:Object = {init_code};"));
            out_lines.push(format!("{pad}for (var {key_var}:String in {init_var}) {{"));
            out_lines.push(format!(
                "{pad}    {target_ident}[{key_var}] = {init_var}[{key_var}];"
            ));
            out_lines.push(format!("{pad}}}"));
        }
    }

    fn emit_stmt_list(&mut self, stmts: &[Stmt]) -> String {
        let mut lines = Vec::new();
        for stmt in stmts {
            let rendered = self.emit_stmt(stmt);
            if !rendered.trim().is_empty() {
                lines.push(rendered);
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
            Stmt::InitClip(_, body, _) => self.emit_stmt_list(body),
            Stmt::IfFrameLoaded(_, body) => self.emit_stmt(body),
            Stmt::TellTarget(target, body, _) => {
                let t = self.emit_expr(target);
                self.indent += 1;
                let inner = self.emit_stmt(body);
                self.indent -= 1;
                format!("{pad}with ({t}) {{\n{inner}\n{pad}}}")
            }
            Stmt::With(target, body, _) => {
                let t = self.emit_expr(target);
                self.indent += 1;
                let inner = self.emit_stmt(body);
                self.indent -= 1;
                format!("{pad}with ({t}) {{\n{inner}\n{pad}}}")
            }
            Stmt::Var(vars) => {
                let mut out_lines = Vec::new();
                for (name, ty_opt, val_opt) in vars {
                    // 1. Check listener object literal: `var kl = { onKeyDown: fn, onResize: fn }`
                    if let Some(Expr::Object(props)) = val_opt {
                        let has_listener_cb = props.iter().any(|(k, v)| {
                            matches!(k.name.as_str(), "onKeyDown" | "onKeyUp" | "onResize")
                                && matches!(v, Expr::Function(_))
                        });
                        if has_listener_cb {
                            for (k, v) in props {
                                if let Expr::Function(fn_box) = v {
                                    match k.name.as_str() {
                                        "onKeyDown" => {
                                            let fn_code =
                                                self.emit_key_callback_fn(name, true, fn_box);
                                            out_lines.push(fn_code);
                                        }
                                        "onKeyUp" => {
                                            let fn_code =
                                                self.emit_key_callback_fn(name, false, fn_box);
                                            out_lines.push(fn_code);
                                        }
                                        "onResize" => {
                                            let fn_code =
                                                self.emit_resize_callback_fn(name, fn_box);
                                            out_lines.push(fn_code);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }

                    // 2. Check `var x = parent.createEmptyMovieClip(name, depth)`
                    if let Some(Expr::Call(callee, args, call_off)) = val_opt {
                        if let Some(parent_expr) =
                            match_clip_factory(callee, "createEmptyMovieClip")
                            && !args.is_empty()
                        {
                            self.imports.insert("flash.display.Sprite".into());
                            let name_code = self.emit_expr(&args[0]);
                            if args.len() >= 2 {
                                self.record_issue(
                                    ConversionIssueKind::Approximated,
                                    *call_off,
                                    "Converted createEmptyMovieClip to new Sprite() + addChild(), ignoring legacy sparse depth.",
                                );
                            }
                            let first_decl = self.mark_declared(name);
                            self.env.define(name.clone(), HirType::Sprite);
                            if first_decl {
                                out_lines.push(format!("{pad}var {name}:Sprite = new Sprite();"));
                            } else {
                                out_lines.push(format!("{pad}{name} = new Sprite();"));
                            }
                            out_lines.push(format!("{pad}{name}.name = {name_code};"));
                            out_lines.push(self.emit_add_child_on(&parent_expr, name));
                            continue;
                        }
                        // 3. Check `var x = parent.attachMovie("linkage", name, depth, initObj)`
                        if let Some(parent_expr) = match_clip_factory(callee, "attachMovie")
                            && args.len() >= 2
                            && let Some(linkage_str) = decode_string_literal(&args[0])
                        {
                            let cls_short = self.resolve_linkage_class(&linkage_str);
                            let name_code = self.emit_expr(&args[1]);
                            let first_decl = self.mark_declared(name);
                            self.env
                                .define(name.clone(), HirType::Class(cls_short.clone()));
                            if first_decl {
                                out_lines.push(format!(
                                    "{pad}var {name}:{cls_short} = new {cls_short}();"
                                ));
                            } else {
                                out_lines.push(format!("{pad}{name} = new {cls_short}();"));
                            }
                            out_lines.push(format!("{pad}{name}.name = {name_code};"));
                            if let Some(init_obj) = args.get(3) {
                                self.emit_init_object_assignments(name, init_obj, &mut out_lines);
                            }
                            out_lines.push(self.emit_add_child_on(&parent_expr, name));
                            continue;
                        }
                    }

                    // 4. Inside a key handler, `var code:Number = Key.getCode()` -> `var code:uint = event.keyCode`
                    if let Some(ev_param) = self.in_key_handler_event.clone()
                        && let Some(Expr::Call(callee, _, _)) = val_opt
                        && let Expr::Member(base, m) = callee.as_ref()
                        && matches!(base.as_ref(), Expr::Name(n) if n == "Key")
                        && m == "getCode"
                    {
                        let first_decl = self.mark_declared(name);
                        self.env.define(name.clone(), HirType::UInt);
                        if first_decl {
                            out_lines.push(format!("{pad}var {name}:uint = {ev_param}.keyCode;"));
                        } else {
                            out_lines.push(format!("{pad}{name} = {ev_param}.keyCode;"));
                        }
                        continue;
                    }

                    // Ordinary variable declaration
                    let inferred = val_opt
                        .as_ref()
                        .map(|e| self.env.infer_expr(e, self.stage))
                        .unwrap_or(HirType::Unknown);
                    let hir_ty = if let Some(raw_ty) = ty_opt {
                        HirType::from_annotation(raw_ty)
                    } else if inferred != HirType::Unknown {
                        inferred
                    } else {
                        HirType::Object
                    };
                    let as3_ty = if let Some(raw_ty) = ty_opt {
                        self.map_type_annotation(raw_ty, false)
                    } else {
                        let mapped = hir_ty.to_as3_annotation();
                        self.map_type_annotation(&mapped, false)
                    };
                    let first_decl = self.mark_declared(name);
                    self.env.define(name.clone(), hir_ty);

                    if let Some(val) = val_opt {
                        let val_code = self.emit_expr(val);
                        if first_decl {
                            out_lines.push(format!("{pad}var {name}:{as3_ty} = {val_code};"));
                        } else {
                            out_lines.push(format!("{pad}{name} = {val_code};"));
                        }
                    } else if first_decl {
                        out_lines.push(format!("{pad}var {name}:{as3_ty};"));
                    }
                }
                out_lines.join("\n")
            }
            Stmt::Function(f) => self.emit_function_decl(f),
            Stmt::Block(body) => {
                self.indent += 1;
                let inner = self.emit_stmt_list(body);
                self.indent -= 1;
                format!("{pad}{{\n{inner}\n{pad}}}")
            }
            Stmt::If(cond, yes, no) => self.emit_if(cond, yes, no.as_deref()),
            Stmt::While(cond, body) => {
                let c = self.emit_expr(cond);
                let b = self.emit_LoopOrBranchBody(body);
                format!("{pad}while ({c}) {b}")
            }
            Stmt::DoWhile(body, cond) => {
                let b = self.emit_LoopOrBranchBody(body);
                let c = self.emit_expr(cond);
                format!("{pad}do {b} while ({c});")
            }
            Stmt::For(init, cond, upd, body) => {
                let init_str = match init.unlocated() {
                    Stmt::Empty => String::new(),
                    Stmt::Var(_) => self.emit_stmt(init).trim().trim_end_matches(';').to_owned(),
                    Stmt::Expr(e) => self.emit_expr(e),
                    _ => String::new(),
                };
                let cond_str = cond.as_ref().map(|e| self.emit_expr(e)).unwrap_or_default();
                let upd_str = upd.as_ref().map(|e| self.emit_expr(e)).unwrap_or_default();
                let b = self.emit_LoopOrBranchBody(body);
                format!("{pad}for ({init_str}; {cond_str}; {upd_str}) {b}")
            }
            Stmt::ForIn(init, coll, each, body) => {
                let kw = if *each { "for each" } else { "for" };
                let init_str = match init.unlocated() {
                    Stmt::Var(vars) if !vars.is_empty() => {
                        let vname = &vars[0].0;
                        let vty = vars[0]
                            .1
                            .as_deref()
                            .map(|t| self.map_type_annotation(t, false))
                            .unwrap_or_else(|| "String".into());
                        let first_decl = self.mark_declared(vname);
                        self.env.define(vname.clone(), HirType::String);
                        if first_decl {
                            format!("var {vname}:{vty}")
                        } else {
                            vname.clone()
                        }
                    }
                    Stmt::Expr(e) => self.emit_expr(e),
                    _ => "var _k:String".into(),
                };
                let coll_str = self.emit_expr(coll);
                let b = self.emit_LoopOrBranchBody(body);
                format!("{pad}{kw} ({init_str} in {coll_str}) {b}")
            }
            Stmt::Switch(disc, cases) => {
                let d = self.emit_expr(disc);
                let mut out = format!("{pad}switch ({d}) {{\n");
                self.indent += 1;
                let case_pad = self.pad();
                for Case { test, body } in cases {
                    match test {
                        Some(t) => {
                            let tc = self.emit_expr(t);
                            out.push_str(&format!("{case_pad}case {tc}:\n"));
                        }
                        None => out.push_str(&format!("{case_pad}default:\n")),
                    }
                    self.indent += 1;
                    let b = self.emit_stmt_list(body);
                    if !b.trim().is_empty() {
                        out.push_str(&b);
                        out.push('\n');
                    }
                    self.indent -= 1;
                }
                self.indent -= 1;
                out.push_str(&format!("{pad}}}"));
                out
            }
            Stmt::Break(lbl) => match lbl {
                Some(l) => format!("{pad}break {l};"),
                None => format!("{pad}break;"),
            },
            Stmt::Continue(lbl) => match lbl {
                Some(l) => format!("{pad}continue {l};"),
                None => format!("{pad}continue;"),
            },
            Stmt::Label(lbl, body) => {
                let b = self.emit_stmt(body);
                format!("{pad}{lbl}: {}", b.trim_start())
            }
            Stmt::Throw(e) => {
                let ec = self.emit_expr(e);
                format!("{pad}throw {ec};")
            }
            Stmt::Try(body, catches, fin) => {
                self.indent += 1;
                let b = self.emit_stmt_list(body);
                self.indent -= 1;
                let mut out = format!("{pad}try {{\n{b}\n{pad}}}");
                for CatchClause { name, ty, body, .. } in catches {
                    let cty = ty
                        .as_deref()
                        .map(|t| self.map_type_annotation(t, false))
                        .unwrap_or_else(|| "*".into());
                    self.env.push_scope();
                    self.env
                        .define(name.clone(), HirType::from_annotation(&cty));
                    self.indent += 1;
                    let cb = self.emit_stmt_list(body);
                    self.indent -= 1;
                    self.env.pop_scope();
                    out.push_str(&format!(" catch ({name}:{cty}) {{\n{cb}\n{pad}}}"));
                }
                if let Some(fin_stmts) = fin {
                    self.indent += 1;
                    let fb = self.emit_stmt_list(fin_stmts);
                    self.indent -= 1;
                    out.push_str(&format!(" finally {{\n{fb}\n{pad}}}"));
                }
                out
            }
            Stmt::Return(val, _) => match val {
                Some(e) => {
                    let ec = self.emit_expr(e);
                    format!("{pad}return {ec};")
                }
                None => format!("{pad}return;"),
            },
            Stmt::DefaultXmlNamespace(e) => {
                let ec = self.emit_expr(e);
                format!("{pad}default xml namespace = {ec};")
            }
            Stmt::LegacyEvent(trigs, func) => {
                // If an inline LegacyEvent appears inside a script, lower it cleanly.
                let mut chunks = Vec::new();
                for trig in trigs {
                    if let Some(kind) = LiftedTriggerKind::from_legacy(trig) {
                        let handler = LiftedObjectHandler {
                            instance_name: "this".into(),
                            parent_is_this: true,
                            trigger: kind,
                            body: func.body.clone(),
                        };
                        chunks.push(self.emit_lifted_handler(&handler));
                    }
                }
                chunks.join("\n")
            }
            Stmt::Expr(expr) => self.emit_expr_stmt(expr, offset),
        }
    }

    #[allow(non_snake_case)]
    fn emit_LoopOrBranchBody(&mut self, stmt: &Stmt) -> String {
        let pad = self.pad();
        match stmt.unlocated() {
            Stmt::Block(stmts) => {
                self.indent += 1;
                let inner = self.emit_stmt_list(stmts);
                self.indent -= 1;
                format!("{{\n{inner}\n{pad}}}")
            }
            _ => {
                self.indent += 1;
                let inner = self.emit_stmt(stmt);
                self.indent -= 1;
                format!("{{\n{inner}\n{pad}}}")
            }
        }
    }

    fn emit_if(&mut self, cond: &Expr, yes: &Stmt, no: Option<&Stmt>) -> String {
        let pad = self.pad();
        let c = self.emit_expr(cond);
        let yes_block = self.emit_LoopOrBranchBody(yes);
        let mut out = format!("{pad}if ({c}) {yes_block}");
        if let Some(alt) = no {
            match alt.unlocated() {
                Stmt::If(nc, ny, nn) => {
                    let rest = self.emit_if(nc, ny, nn.as_deref());
                    out.push_str(" else ");
                    out.push_str(rest.trim_start());
                }
                _ => {
                    let no_block = self.emit_LoopOrBranchBody(alt);
                    out.push_str(&format!(" else {no_block}"));
                }
            }
        }
        out
    }

    fn emit_key_callback_fn(&mut self, owner_key: &str, is_down: bool, func: &Function) -> String {
        let pad = self.pad();
        self.imports.insert("flash.events.KeyboardEvent".into());
        let fn_name = self.next_helper(if is_down { "onKeyDown" } else { "onKeyUp" });
        if is_down {
            self.key_down_handlers
                .insert(owner_key.to_owned(), fn_name.clone());
        } else {
            self.key_up_handlers
                .insert(owner_key.to_owned(), fn_name.clone());
        }
        let prev_ev = self.in_key_handler_event.replace("event".into());
        self.push_fn_scope();
        self.env
            .define("event", HirType::Class("KeyboardEvent".into()));
        self.indent += 1;
        let body = self.emit_stmt_list(&func.body);
        self.indent -= 1;
        self.pop_fn_scope();
        self.in_key_handler_event = prev_ev;

        let mut out =
            format!("{pad}function {fn_name}(event:KeyboardEvent):void {{\n{body}\n{pad}}}");
        if !self.explicit_key_listeners.contains(owner_key) {
            let ev_const = if is_down {
                "KeyboardEvent.KEY_DOWN"
            } else {
                "KeyboardEvent.KEY_UP"
            };
            out.push_str(&format!(
                "\n{pad}if (stage != null) {{\n{pad}    stage.focus = this;\n{pad}    stage.addEventListener({ev_const}, {fn_name});\n{pad}}}"
            ));
        }
        out
    }

    fn emit_resize_callback_fn(&mut self, owner_key: &str, func: &Function) -> String {
        let pad = self.pad();
        self.imports.insert("flash.events.Event".into());
        let fn_name = self.next_helper("onResize");
        self.resize_handlers
            .insert(owner_key.to_owned(), fn_name.clone());
        self.push_fn_scope();
        self.env.define("event", HirType::Class("Event".into()));
        self.indent += 1;
        let body = self.emit_stmt_list(&func.body);
        self.indent -= 1;
        self.pop_fn_scope();

        let mut out = format!("{pad}function {fn_name}(event:Event):void {{\n{body}\n{pad}}}");
        if !self.explicit_stage_listeners.contains(owner_key) {
            out.push_str(&format!(
                "\n{pad}if (stage != null) {{\n{pad}    stage.addEventListener(Event.RESIZE, {fn_name});\n{pad}}}"
            ));
        }
        out
    }

    fn emit_expr_stmt(&mut self, expr: &Expr, offset: usize) -> String {
        let pad = self.pad();

        // 1. Callback assignments: `this.onEnterFrame = fn`, `this.onKeyDown = fn`, `this.onResize = fn`, `mc.onRelease = fn`
        if let Expr::Assign(op, lhs, rhs) = expr
            && op == "="
        {
            // Assignment from `createEmptyMovieClip` or `attachMovie`
            if let Expr::Call(callee, args, call_off) = rhs.as_ref() {
                if let Some(parent_expr) = match_clip_factory(callee, "createEmptyMovieClip")
                    && !args.is_empty()
                {
                    self.imports.insert("flash.display.Sprite".into());
                    let target_code = self.emit_expr(lhs);
                    let name_code = self.emit_expr(&args[0]);
                    if args.len() >= 2 {
                        self.record_issue(
                            ConversionIssueKind::Approximated,
                            *call_off,
                            "Converted createEmptyMovieClip to new Sprite() + addChild(), ignoring legacy sparse depth.",
                        );
                    }
                    let lines = [
                        format!("{pad}{target_code} = new Sprite();"),
                        format!("{pad}{target_code}.name = {name_code};"),
                        self.emit_add_child_on(&parent_expr, &target_code),
                    ];
                    return lines.join("\n");
                }
                if let Some(parent_expr) = match_clip_factory(callee, "attachMovie")
                    && args.len() >= 2
                    && let Some(linkage_str) = decode_string_literal(&args[0])
                {
                    let cls_short = self.resolve_linkage_class(&linkage_str);
                    let target_code = self.emit_expr(lhs);
                    let name_code = self.emit_expr(&args[1]);
                    let mut lines = vec![
                        format!("{pad}{target_code} = new {cls_short}();"),
                        format!("{pad}{target_code}.name = {name_code};"),
                    ];
                    if let Some(init_obj) = args.get(3) {
                        self.emit_init_object_assignments(&target_code, init_obj, &mut lines);
                    }
                    lines.push(self.emit_add_child_on(&parent_expr, &target_code));
                    return lines.join("\n");
                }
            }

            // Stage.scaleMode / Stage.align writes: guard with `if (stage != null)`
            if let Expr::Member(base, prop) = lhs.as_ref()
                && matches!(base.as_ref(), Expr::Name(n) if n == "Stage")
                && matches!(prop.as_str(), "scaleMode" | "align" | "displayState")
            {
                let rhs_code = self.emit_expr(rhs);
                return format!(
                    "{pad}if (stage != null) {{\n{pad}    stage.{prop} = {rhs_code};\n{pad}}}"
                );
            }

            if let Some((owner_expr, cb_prop)) = extract_callback_target(lhs) {
                match cb_prop {
                    "onEnterFrame" => {
                        self.imports.insert("flash.events.Event".into());
                        let recv = if matches!(&owner_expr, Expr::Name(n) if n == "this") {
                            String::new()
                        } else {
                            format!("{}.", self.emit_expr(&owner_expr))
                        };
                        if matches!(rhs.as_ref(), Expr::Literal(l) if l == "null" || l == "undefined")
                        {
                            return format!("{pad}/* cleared onEnterFrame */");
                        }
                        let fn_name = self.next_helper("onEnterFrame");
                        let body = self.emit_callback_body(rhs, "Event");
                        return format!(
                            "{pad}function {fn_name}(event:Event):void {{\n{body}\n{pad}}}\n{pad}{recv}addEventListener(Event.ENTER_FRAME, {fn_name});"
                        );
                    }
                    "onKeyDown" | "onKeyUp" => {
                        let owner_key =
                            listener_target_key(&owner_expr).unwrap_or_else(|| "this".into());
                        let is_down = cb_prop == "onKeyDown";
                        if let Expr::Function(fn_box) = rhs.as_ref() {
                            return self.emit_key_callback_fn(&owner_key, is_down, fn_box);
                        }
                    }
                    "onResize" => {
                        let owner_key =
                            listener_target_key(&owner_expr).unwrap_or_else(|| "this".into());
                        if let Expr::Function(fn_box) = rhs.as_ref() {
                            return self.emit_resize_callback_fn(&owner_key, fn_box);
                        }
                    }
                    "onRelease" | "onPress" | "onReleaseOutside" | "onRollOver" | "onRollOut" => {
                        self.imports.insert("flash.events.MouseEvent".into());
                        let ev_const = match cb_prop {
                            "onRelease" => "MouseEvent.CLICK",
                            "onPress" => "MouseEvent.MOUSE_DOWN",
                            "onReleaseOutside" => "MouseEvent.MOUSE_UP",
                            "onRollOver" => "MouseEvent.ROLL_OVER",
                            _ => "MouseEvent.ROLL_OUT",
                        };
                        let recv = if matches!(&owner_expr, Expr::Name(n) if n == "this") {
                            String::new()
                        } else {
                            format!("{}.", self.emit_expr(&owner_expr))
                        };
                        let fn_name = self.next_helper(cb_prop);
                        let body = self.emit_callback_body(rhs, "MouseEvent");
                        return format!(
                            "{pad}function {fn_name}(event:MouseEvent):void {{\n{body}\n{pad}}}\n{pad}{recv}addEventListener({ev_const}, {fn_name});"
                        );
                    }
                    _ => {}
                }
            }
        }

        // 2. `Key.addListener(target)` & `Stage.addListener(target)` & `mc.removeMovieClip()`
        if let Expr::Call(callee, args, _) = expr {
            if let Expr::Member(base, method) = callee.as_ref() {
                if matches!(base.as_ref(), Expr::Name(n) if n == "Key")
                    && method == "addListener"
                    && let Some(first) = args.first()
                {
                    let key = listener_target_key(first).unwrap_or_else(|| "this".into());
                    self.imports.insert("flash.events.KeyboardEvent".into());
                    let mut reg = Vec::new();
                    if let Some(kd) = self.key_down_handlers.get(&key).cloned() {
                        reg.push(format!(
                            "{pad}    stage.addEventListener(KeyboardEvent.KEY_DOWN, {kd});"
                        ));
                    }
                    if let Some(ku) = self.key_up_handlers.get(&key).cloned() {
                        reg.push(format!(
                            "{pad}    stage.addEventListener(KeyboardEvent.KEY_UP, {ku});"
                        ));
                    }
                    if reg.is_empty() {
                        return String::new();
                    }
                    return format!(
                        "{pad}if (stage != null) {{\n{pad}    stage.focus = this;\n{}\n{pad}}}",
                        reg.join("\n")
                    );
                }
                if matches!(base.as_ref(), Expr::Name(n) if n == "Stage")
                    && method == "addListener"
                    && let Some(first) = args.first()
                {
                    let key = listener_target_key(first).unwrap_or_else(|| "this".into());
                    self.imports.insert("flash.events.Event".into());
                    if let Some(rh) = self.resize_handlers.get(&key).cloned() {
                        return format!(
                            "{pad}if (stage != null) {{\n{pad}    stage.addEventListener(Event.RESIZE, {rh});\n{pad}}}"
                        );
                    }
                    return String::new();
                }
                if method == "removeMovieClip" && args.is_empty() {
                    let recv = self.emit_expr(base);
                    return format!(
                        "{pad}if ({recv}.parent != null) {{\n{pad}    {recv}.parent.removeChild({recv});\n{pad}}}"
                    );
                }
            } else if let Expr::Name(fn_name) = callee.as_ref()
                && fn_name == "removeMovieClip"
                && args.is_empty()
            {
                return format!(
                    "{pad}if (parent != null) {{\n{pad}    parent.removeChild(this);\n{pad}}}"
                );
            }
        }

        let _ = offset;
        let code = self.emit_expr(expr);
        format!("{pad}{code};")
    }

    fn emit_callback_body(&mut self, rhs: &Expr, ev_type: &str) -> String {
        self.push_fn_scope();
        self.env.define("event", HirType::Class(ev_type.to_owned()));
        self.indent += 1;
        let out = if let Expr::Function(f) = rhs {
            self.emit_stmt_list(&f.body)
        } else {
            let pad = self.pad();
            let callee = self.emit_expr(rhs);
            format!("{pad}{callee}();")
        };
        self.indent -= 1;
        self.pop_fn_scope();
        out
    }

    fn emit_function_decl(&mut self, f: &Function) -> String {
        let pad = self.pad();
        let name = f.name.as_deref().unwrap_or("__anon");
        let ret_as3 = match &f.returns {
            Some(r) => self.map_type_annotation(r, true),
            None => {
                if function_has_value_return(&f.body) {
                    "*".into()
                } else {
                    "void".into()
                }
            }
        };
        let fixed_body = ensure_as3_return_completeness(&f.body, &ret_as3);

        self.push_fn_scope();
        let mut params_out = Vec::new();
        for (i, (pname, pty)) in f.params.iter().enumerate() {
            let hir_ty = pty
                .as_deref()
                .map_or(HirType::Object, HirType::from_annotation);
            let mapped = pty
                .as_deref()
                .map(|t| self.map_type_annotation(t, false))
                .unwrap_or_else(|| "*".into());
            self.mark_declared(pname);
            self.env.define(pname.clone(), hir_ty);
            if let Some(Some(def)) = f.defaults.get(i) {
                let dc = self.emit_expr(def);
                params_out.push(format!("{pname}:{mapped} = {dc}"));
            } else {
                params_out.push(format!("{pname}:{mapped}"));
            }
        }
        if let Some(rest) = &f.rest {
            self.mark_declared(rest);
            self.env.define(rest.clone(), HirType::Array);
            params_out.push(format!("...{rest}:Array"));
        }

        self.indent += 1;
        let body_str = self.emit_stmt_list(&fixed_body);
        self.indent -= 1;
        self.pop_fn_scope();

        format!(
            "{pad}function {name}({}):{ret_as3} {{\n{body_str}\n{pad}}}",
            params_out.join(", ")
        )
    }

    fn emit_lifted_handler(&mut self, handler: &LiftedObjectHandler) -> String {
        let pad = self.pad();
        let inst = &handler.instance_name;
        let rewritten_body = if handler.parent_is_this {
            handler.body.clone()
        } else {
            rewrite_lifted_clip_scope_stmts(&handler.body, inst)
        };

        match &handler.trigger {
            LiftedTriggerKind::Load => self.emit_stmt_list(&rewritten_body),
            LiftedTriggerKind::EnterFrame => {
                self.imports.insert("flash.events.Event".into());
                let fn_name = self.next_helper(&format!("on_{inst}_enterFrame"));
                self.push_fn_scope();
                self.indent += 1;
                let b = self.emit_stmt_list(&rewritten_body);
                self.indent -= 1;
                self.pop_fn_scope();
                let target = if inst == "this" {
                    String::new()
                } else {
                    format!("this.{inst}.")
                };
                format!(
                    "{pad}function {fn_name}(event:Event):void {{\n{b}\n{pad}}}\n{pad}{target}addEventListener(Event.ENTER_FRAME, {fn_name});"
                )
            }
            LiftedTriggerKind::MouseClick
            | LiftedTriggerKind::MouseDown
            | LiftedTriggerKind::MouseUp
            | LiftedTriggerKind::RollOver
            | LiftedTriggerKind::RollOut => {
                self.imports.insert("flash.events.MouseEvent".into());
                let ev_const = match handler.trigger {
                    LiftedTriggerKind::MouseClick => "MouseEvent.CLICK",
                    LiftedTriggerKind::MouseDown => "MouseEvent.MOUSE_DOWN",
                    LiftedTriggerKind::MouseUp => "MouseEvent.MOUSE_UP",
                    LiftedTriggerKind::RollOver => "MouseEvent.ROLL_OVER",
                    _ => "MouseEvent.ROLL_OUT",
                };
                let fn_name = self.next_helper(&format!("on_{inst}_mouse"));
                self.push_fn_scope();
                self.indent += 1;
                let b = self.emit_stmt_list(&rewritten_body);
                self.indent -= 1;
                self.pop_fn_scope();
                let target = if inst == "this" {
                    String::new()
                } else {
                    format!("this.{inst}.")
                };
                format!(
                    "{pad}function {fn_name}(event:MouseEvent):void {{\n{b}\n{pad}}}\n{pad}{target}addEventListener({ev_const}, {fn_name});"
                )
            }
            LiftedTriggerKind::KeyPress(spec) => {
                self.imports.insert("flash.events.KeyboardEvent".into());
                let cond = if let Some((kb_const, code)) = key_press_spec_to_code(spec) {
                    if !kb_const.is_empty() {
                        self.imports.insert("flash.ui.Keyboard".into());
                        format!("event.keyCode == {kb_const}")
                    } else {
                        format!("event.keyCode == {code}")
                    }
                } else {
                    "true".into()
                };
                let fn_name = self.next_helper(&format!("on_{inst}_keyPress"));
                self.push_fn_scope();
                self.indent += 2;
                let b = self.emit_stmt_list(&rewritten_body);
                self.indent -= 2;
                self.pop_fn_scope();
                format!(
                    "{pad}function {fn_name}(event:KeyboardEvent):void {{\n{pad}    if ({cond}) {{\n{b}\n{pad}    }}\n{pad}}}\n{pad}if (stage != null) {{\n{pad}    stage.addEventListener(KeyboardEvent.KEY_DOWN, {fn_name});\n{pad}}}"
                )
            }
            LiftedTriggerKind::KeyDown | LiftedTriggerKind::KeyUp => {
                self.imports.insert("flash.events.KeyboardEvent".into());
                let ev_const = if handler.trigger == LiftedTriggerKind::KeyDown {
                    "KeyboardEvent.KEY_DOWN"
                } else {
                    "KeyboardEvent.KEY_UP"
                };
                let fn_name = self.next_helper(&format!("on_{inst}_key"));
                let prev = self.in_key_handler_event.replace("event".into());
                self.push_fn_scope();
                self.indent += 1;
                let b = self.emit_stmt_list(&rewritten_body);
                self.indent -= 1;
                self.pop_fn_scope();
                self.in_key_handler_event = prev;
                format!(
                    "{pad}function {fn_name}(event:KeyboardEvent):void {{\n{b}\n{pad}}}\n{pad}if (stage != null) {{\n{pad}    stage.addEventListener({ev_const}, {fn_name});\n{pad}}}"
                )
            }
        }
    }

    fn emit_expr_immut(&self, expr: &Expr) -> String {
        match expr {
            Expr::Name(n) => n.clone(),
            Expr::Literal(l) => l.clone(),
            Expr::Member(b, p) => format!("{}.{}", self.emit_expr_immut(b), p),
            _ => "this".into(),
        }
    }

    fn emit_expr(&mut self, expr: &Expr) -> String {
        match expr {
            Expr::Literal(raw) => raw.clone(),
            Expr::Name(name) => {
                if let Some(modern) = map_as2_direct_prop_to_modern(name) {
                    return format!("this.{modern}");
                }
                if let Some(modern_pct) = map_as2_percent_prop_to_modern(name) {
                    return format!("(this.{modern_pct} * 100)");
                }
                if name == "_root" || name == "_level0" {
                    self.imports.insert("flash.display.MovieClip".into());
                    return "MovieClip(root)".into();
                }
                if name == "_global" {
                    return "Object(root)".into();
                }
                name.clone()
            }
            Expr::Member(base, prop) => {
                // `Key.LEFT` -> `Keyboard.LEFT`
                if matches!(base.as_ref(), Expr::Name(n) if n == "Key")
                    && let Some((kb_const, _)) = map_as2_key_constant(prop)
                {
                    self.imports.insert("flash.ui.Keyboard".into());
                    return format!("Keyboard.{kb_const}");
                }
                // `Stage.width` / `Stage.height` / `Stage.scaleMode` / `Stage.align`
                if matches!(base.as_ref(), Expr::Name(n) if n == "Stage") {
                    return match prop.as_str() {
                        "width" => "(stage != null ? stage.stageWidth : 0)".into(),
                        "height" => "(stage != null ? stage.stageHeight : 0)".into(),
                        "scaleMode" => "(stage != null ? stage.scaleMode : \"showAll\")".into(),
                        "align" => "(stage != null ? stage.align : \"\")".into(),
                        other => format!("stage.{other}"),
                    };
                }
                let recv = self.emit_expr(base);
                if let Some(modern) = map_as2_direct_prop_to_modern(prop) {
                    return format!("{recv}.{modern}");
                }
                if let Some(modern_pct) = map_as2_percent_prop_to_modern(prop) {
                    return format!("({recv}.{modern_pct} * 100)");
                }
                format!("{recv}.{prop}")
            }
            Expr::Index(base, idx) => {
                let b = self.emit_expr(base);
                let i = self.emit_expr(idx);
                format!("{b}[{i}]")
            }
            Expr::Array(items) => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|opt| opt.as_ref().map(|e| self.emit_expr(e)).unwrap_or_default())
                    .collect();
                format!("[{}]", parts.join(", "))
            }
            Expr::Object(props) => {
                let parts: Vec<String> = props
                    .iter()
                    .map(|(PropertyKey { source, .. }, val)| {
                        format!("{source}: {}", self.emit_expr(val))
                    })
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
            Expr::Unary(op, inner) => {
                let mapped_op = if op == "not" { "!" } else { op.as_str() };
                let ic = self.emit_expr(inner);
                if matches!(mapped_op, "typeof" | "void") {
                    format!("{mapped_op} {ic}")
                } else if is_atomic_expr(inner) {
                    format!("{mapped_op}{ic}")
                } else {
                    format!("{mapped_op}({ic})")
                }
            }
            Expr::Update(op, inner, prefix) => {
                if let Some((recv_code, modern_pct)) = self.extract_percent_member(inner) {
                    let step = if op == "++" { "0.01" } else { "-0.01" };
                    return format!("({recv_code}.{modern_pct} += {step})");
                }
                let ic = self.emit_assign_target(inner);
                if *prefix {
                    format!("{op}{ic}")
                } else {
                    format!("{ic}{op}")
                }
            }
            Expr::Assign(op, lhs, rhs) => {
                if let Some((recv_code, modern_pct)) = self.extract_percent_member(lhs) {
                    match op.as_str() {
                        "=" => {
                            if let Some(folded) = fold_100_to_01(rhs, false) {
                                return format!("{recv_code}.{modern_pct} = {folded}");
                            }
                            let rc = self.emit_expr(rhs);
                            return format!("{recv_code}.{modern_pct} = ({rc}) / 100");
                        }
                        "+=" | "-=" => {
                            if let Some(folded) = fold_100_to_01(rhs, false) {
                                return format!("{recv_code}.{modern_pct} {op} {folded}");
                            }
                            let rc = self.emit_expr(rhs);
                            return format!("{recv_code}.{modern_pct} {op} ({rc}) / 100");
                        }
                        _ => {
                            let rc = self.emit_expr(rhs);
                            return format!("{recv_code}.{modern_pct} {op} {rc}");
                        }
                    }
                }
                let lc = self.emit_assign_target(lhs);
                let rc = self.emit_expr(rhs);
                format!("{lc} {op} {rc}")
            }
            Expr::Binary(op, lhs, rhs) => {
                let mapped_op = match op.as_str() {
                    "eq" => "==",
                    "ne" | "<>" => "!=",
                    "and" => "&&",
                    "or" => "||",
                    "add" => "+",
                    other => other,
                };
                // Normalize `!= undefined` / `== undefined` on typed objects -> `!= null` / `== null`
                if matches!(mapped_op, "==" | "!=") {
                    if matches!(rhs.as_ref(), Expr::Name(n) if n == "undefined")
                        && self.env.infer_expr(lhs, self.stage).is_nullable_object()
                    {
                        let lc = self.emit_expr(lhs);
                        return format!("{lc} {mapped_op} null");
                    }
                    if matches!(lhs.as_ref(), Expr::Name(n) if n == "undefined")
                        && self.env.infer_expr(rhs, self.stage).is_nullable_object()
                    {
                        let rc = self.emit_expr(rhs);
                        return format!("null {mapped_op} {rc}");
                    }
                }
                let lc = self.emit_sub_expr(lhs, mapped_op, false);
                let rc = self.emit_sub_expr(rhs, mapped_op, true);
                format!("{lc} {mapped_op} {rc}")
            }
            Expr::Conditional(cond, yes, no) => {
                let c = self.emit_expr(cond);
                let y = self.emit_expr(yes);
                let n = self.emit_expr(no);
                format!("({c} ? {y} : {n})")
            }
            Expr::InstanceOf(lhs, rhs) => {
                let l = self.emit_expr(lhs);
                let r = self.emit_expr(rhs);
                format!("({l} instanceof {r})")
            }
            Expr::TypeTest(op, lhs, ty_name, _) => {
                let l = self.emit_expr(lhs);
                let mapped = self.map_type_annotation(ty_name, false);
                if op == "instanceof" {
                    format!("({l} is {mapped})")
                } else {
                    format!("({l} {op} {mapped})")
                }
            }
            Expr::TypeValue(ty_name, _) => self.map_type_annotation(ty_name, false),
            Expr::Delete(inner, _) => {
                let ic = self.emit_expr(inner);
                format!("delete {ic}")
            }
            Expr::Function(f) => {
                let ret_as3 = match &f.returns {
                    Some(r) => self.map_type_annotation(r, true),
                    None => {
                        if function_has_value_return(&f.body) {
                            "*".into()
                        } else {
                            "void".into()
                        }
                    }
                };
                let fixed_body = ensure_as3_return_completeness(&f.body, &ret_as3);
                self.push_fn_scope();
                let mut params_out = Vec::new();
                for (pname, pty) in &f.params {
                    let mapped = pty
                        .as_deref()
                        .map(|t| self.map_type_annotation(t, false))
                        .unwrap_or_else(|| "*".into());
                    self.mark_declared(pname);
                    self.env
                        .define(pname.clone(), HirType::from_annotation(&mapped));
                    params_out.push(format!("{pname}:{mapped}"));
                }
                self.indent += 1;
                let body_str = self.emit_stmt_list(&fixed_body);
                self.indent -= 1;
                self.pop_fn_scope();
                let pad = self.pad();
                let fname = f.name.as_deref().unwrap_or("");
                format!(
                    "function {fname}({}):{ret_as3} {{\n{body_str}\n{pad}}}",
                    params_out.join(", ")
                )
            }
            Expr::NewBuiltin(ty, args, _)
            | Expr::NewClass(ty, args, _)
            | Expr::NewValue(ty, args, _)
            | Expr::NewError(ty, args, _) => {
                let mapped = self.map_type_annotation(ty, false);
                let ac = self.emit_args(args);
                format!("new {mapped}({ac})")
            }
            Expr::NewVector(ty, args, _) => {
                let mapped = self.map_type_annotation(ty, false);
                let ac = self.emit_args(args);
                format!("new {mapped}({ac})")
            }
            Expr::RegExpLiteral(pat, flags) => {
                let p: String = serde_json::from_str(pat).unwrap_or_default();
                let fl: String = serde_json::from_str(flags).unwrap_or_default();
                format!("/{p}/{fl}")
            }
            Expr::Call(callee, args, call_off) => self.emit_call(callee, args, *call_off),
            _ => "null".into(),
        }
    }

    fn emit_call(&mut self, callee: &Expr, args: &[Expr], call_off: usize) -> String {
        // 1. Key.getCode() / Key.getAscii() / Key.isDown(code)
        if let Expr::Member(base, method) = callee {
            if matches!(base.as_ref(), Expr::Name(n) if n == "Key") {
                match method.as_str() {
                    "getCode" => {
                        if let Some(ev) = &self.in_key_handler_event {
                            return format!("{ev}.keyCode");
                        }
                        return "0".into();
                    }
                    "getAscii" => {
                        if let Some(ev) = &self.in_key_handler_event {
                            return format!("{ev}.charCode");
                        }
                        return "0".into();
                    }
                    "isDown" => {
                        let code_arg = args
                            .first()
                            .map(|a| self.emit_expr(a))
                            .unwrap_or_else(|| "0".into());
                        return format!("_isKeyDown({code_arg})");
                    }
                    _ => {}
                }
            }

            // 2. Drawing API on MovieClip/Sprite: `mc.clear()`, `mc.beginFill(c, a100)`, `mc.lineStyle(w, c, a100)`, `mc.moveTo`, `mc.lineTo`, `mc.curveTo`, `mc.endFill`
            if !matches!(base.as_ref(), Expr::Member(_, g) if g == "graphics")
                && is_as2_drawing_method(method)
            {
                let recv = self.emit_expr(base);
                let converted_args = self.emit_drawing_args_to_as3(method, args);
                return format!("{recv}.graphics.{method}({converted_args})");
            }

            // 3. Expression-context `createEmptyMovieClip` fallback
            if method == "createEmptyMovieClip" && !args.is_empty() {
                self.imports.insert("flash.display.Sprite".into());
                self.record_issue(
                    ConversionIssueKind::Approximated,
                    call_off,
                    "Converted createEmptyMovieClip expression to new Sprite() + addChild().",
                );
                let recv = self.emit_expr(base);
                let name_code = self.emit_expr(&args[0]);
                return format!(
                    "(function():Sprite {{ var _s:Sprite = new Sprite(); _s.name = {name_code}; {recv}.addChild(_s); return _s; }})()"
                );
            }
        }

        // Bare Drawing API calls on timeline (`clear()`, `beginFill(...)`, etc.)
        if let Expr::Name(fn_name) = callee {
            if is_as2_drawing_method(fn_name) {
                let converted_args = self.emit_drawing_args_to_as3(fn_name, args);
                return format!("this.graphics.{fn_name}({converted_args})");
            }
            if fn_name == "random" && args.len() == 1 {
                let n = self.emit_expr(&args[0]);
                return format!("int(Math.floor(Math.random() * ({n})))");
            }
            if fn_name == "getTimer" && args.is_empty() {
                self.imports.insert("flash.utils.getTimer".into());
                return "getTimer()".into();
            }
            if fn_name == "getURL" && !args.is_empty() {
                self.imports.insert("flash.net.navigateToURL".into());
                self.imports.insert("flash.net.URLRequest".into());
                let url = self.emit_expr(&args[0]);
                if let Some(win) = args.get(1) {
                    let w = self.emit_expr(win);
                    return format!("navigateToURL(new URLRequest({url}), {w})");
                }
                return format!("navigateToURL(new URLRequest({url}))");
            }
        }

        let c = self.emit_expr(callee);
        let a = self.emit_args(args);
        format!("{c}({a})")
    }

    fn emit_drawing_args_to_as3(&mut self, method: &str, args: &[Expr]) -> String {
        let alpha_idx = match method {
            "beginFill" => Some(1usize),
            "lineStyle" => Some(2usize),
            _ => None,
        };
        args.iter()
            .enumerate()
            .map(|(idx, arg)| {
                if Some(idx) == alpha_idx {
                    if let Some(folded) = fold_100_to_01(arg, false) {
                        folded
                    } else {
                        let ac = self.emit_expr(arg);
                        format!("({ac}) / 100")
                    }
                } else {
                    self.emit_expr(arg)
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn extract_percent_member(&mut self, expr: &Expr) -> Option<(String, &'static str)> {
        match expr {
            Expr::Member(base, prop) => {
                let modern = map_as2_percent_prop_to_modern(prop)?;
                let recv = self.emit_expr(base);
                Some((recv, modern))
            }
            Expr::Name(prop) => {
                let modern = map_as2_percent_prop_to_modern(prop)?;
                Some(("this".into(), modern))
            }
            _ => None,
        }
    }

    fn emit_assign_target(&mut self, expr: &Expr) -> String {
        match expr {
            Expr::Name(name) => {
                if let Some(modern) = map_as2_direct_prop_to_modern(name) {
                    return format!("this.{modern}");
                }
                name.clone()
            }
            Expr::Member(base, prop) => {
                let recv = self.emit_expr(base);
                if let Some(modern) = map_as2_direct_prop_to_modern(prop) {
                    return format!("{recv}.{modern}");
                }
                format!("{recv}.{prop}")
            }
            _ => self.emit_expr(expr),
        }
    }

    fn emit_args(&mut self, args: &[Expr]) -> String {
        args.iter()
            .map(|a| self.emit_expr(a))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn emit_sub_expr(&mut self, expr: &Expr, parent_op: &str, is_right: bool) -> String {
        let code = self.emit_expr(expr);
        if let Expr::Binary(child_op, _, _) = expr {
            let p_prec = op_prec(parent_op);
            let c_prec = op_prec(child_op);
            if c_prec < p_prec
                || (is_right && c_prec == p_prec && matches!(parent_op, "-" | "/" | "%"))
            {
                return format!("({code})");
            }
        } else if matches!(expr, Expr::Conditional(_, _, _) | Expr::Assign(_, _, _)) {
            return format!("({code})");
        }
        code
    }

    fn key_down_shim_block(&mut self) -> String {
        self.imports.insert("flash.events.KeyboardEvent".into());
        let pad = self.pad();
        [
            format!("{pad}var _keysDown:Object = {{}};"),
            format!("{pad}function _onTrackKeyDown(event:KeyboardEvent):void {{"),
            format!("{pad}    _keysDown[event.keyCode] = true;"),
            format!("{pad}}}"),
            format!("{pad}function _onTrackKeyUp(event:KeyboardEvent):void {{"),
            format!("{pad}    _keysDown[event.keyCode] = false;"),
            format!("{pad}}}"),
            format!("{pad}function _isKeyDown(code:uint):Boolean {{"),
            format!("{pad}    return Boolean(_keysDown[code]);"),
            format!("{pad}}}"),
            format!("{pad}if (stage != null) {{"),
            format!("{pad}    stage.focus = this;"),
            format!("{pad}    stage.addEventListener(KeyboardEvent.KEY_DOWN, _onTrackKeyDown);"),
            format!("{pad}    stage.addEventListener(KeyboardEvent.KEY_UP, _onTrackKeyUp);"),
            format!("{pad}}}"),
        ]
        .join("\n")
    }

    fn format_imports(&self, body: &str) -> Vec<String> {
        self.imports
            .iter()
            .filter(|imp| {
                if imp.ends_with(".*") {
                    true
                } else {
                    let short = short_type_name(imp);
                    body.contains(short)
                }
            })
            .map(|imp| format!("import {imp};"))
            .collect()
    }
}

pub(crate) fn emit_as3_timeline_script(
    parsed: &ParsedScript,
    lifted: &[LiftedObjectHandler],
    symbol_id: &str,
    location: &str,
    stage: &ProjectStageIndex,
) -> As3EmitOutput {
    let mut emitter = As3Emitter::new(
        location,
        &parsed.expanded_source,
        stage,
        Some(symbol_id.to_owned()),
        None,
    );
    emitter.preserve_compatible_imports(&parsed.imports);
    emitter.precollect_functions_and_listeners(&parsed.statements);

    let needs_key_shim = stmts_use_key_is_down(&parsed.statements)
        || lifted.iter().any(|h| stmts_use_key_is_down(&h.body));
    emitter.use_key_down_shim = needs_key_shim;

    let mut sections = Vec::new();
    if needs_key_shim {
        emitter.record_issue(
            ConversionIssueKind::ShimInjected,
            0,
            "Injected KeyboardEvent KEY_DOWN/KEY_UP stage listener shim for AS2 Key.isDown().",
        );
        sections.push(emitter.key_down_shim_block());
    }

    let main_body = emitter.emit_stmt_list(&parsed.statements);
    if !main_body.trim().is_empty() {
        sections.push(main_body);
    }

    for handler in lifted {
        let lifted_code = emitter.emit_lifted_handler(handler);
        if !lifted_code.trim().is_empty() {
            sections.push(lifted_code);
        }
    }

    let combined_body = sections.join("\n\n");
    let import_lines = emitter.format_imports(&combined_body);

    let final_source = if import_lines.is_empty() {
        format!("{combined_body}\n")
    } else if combined_body.trim().is_empty() {
        format!("{}\n", import_lines.join("\n"))
    } else {
        format!("{}\n\n{combined_body}\n", import_lines.join("\n"))
    };

    As3EmitOutput {
        source: final_source,
        issues: emitter.issues,
        synthesized_linkages: emitter.synthesized_linkages.into_values().collect(),
    }
}

pub(crate) fn emit_as3_class(
    parsed: &ParsedClass,
    location: &str,
    stage: &ProjectStageIndex,
) -> As3EmitOutput {
    let mut emitter = As3Emitter::new(
        location,
        &parsed.source,
        stage,
        None,
        Some(parsed.qualified_name.clone()),
    );
    emitter.preserve_compatible_imports(&parsed.imports);

    for f in &parsed.fields {
        if !f.is_static {
            let ty =
                f.ty.as_deref()
                    .map_or(HirType::Object, HirType::from_annotation);
            emitter.env.define(f.name.clone(), ty);
        }
    }

    emitter.indent = 2;
    let mut member_blocks = Vec::new();

    for f in &parsed.fields {
        let pad = emitter.pad();
        let acc = f.access.as3_keyword();
        let stat = if f.is_static { "static " } else { "" };
        let kw = if f.is_const { "const" } else { "var" };
        let ty =
            f.ty.as_deref()
                .map(|t| emitter.map_type_annotation(t, false))
                .unwrap_or_else(|| "*".into());
        if let Some(val) = &f.value {
            let vc = emitter.emit_expr(val);
            member_blocks.push(format!("{pad}{acc} {stat}{kw} {}:{ty} = {vc};", f.name));
        } else {
            member_blocks.push(format!("{pad}{acc} {stat}{kw} {}:{ty};", f.name));
        }
    }

    for m in &parsed.methods {
        let pad = emitter.pad();
        let fname = m.function.name.as_deref().unwrap_or(&parsed.short_name);
        let is_ctor = fname == parsed.short_name && m.accessor.is_none() && !m.is_static;
        let acc = if is_ctor || parsed.is_interface {
            if parsed.is_interface { "" } else { "public " }
        } else {
            match m.access {
                MemberAccess::Public => "public ",
                MemberAccess::Private => "private ",
                MemberAccess::Protected => "protected ",
                MemberAccess::Internal => "internal ",
            }
        };
        let stat = if m.is_static && !parsed.is_interface {
            "static "
        } else {
            ""
        };
        let ovr = if !parsed.is_interface
            && !is_ctor
            && !m.is_static
            && (m.is_override || method_needs_as3_override(parsed, fname, stage))
        {
            "override "
        } else {
            ""
        };
        let get_set = match m.accessor {
            Some(AccessorKind::Get) => "get ",
            Some(AccessorKind::Set) => "set ",
            None => "",
        };

        emitter.push_fn_scope();
        let mut params_out = Vec::new();
        for (i, (pname, pty)) in m.function.params.iter().enumerate() {
            let mapped = pty
                .as_deref()
                .map(|t| emitter.map_type_annotation(t, false))
                .unwrap_or_else(|| "*".into());
            emitter.mark_declared(pname);
            emitter
                .env
                .define(pname.clone(), HirType::from_annotation(&mapped));
            if let Some(Some(def)) = m.function.defaults.get(i) {
                let dc = emitter.emit_expr(def);
                params_out.push(format!("{pname}:{mapped} = {dc}"));
            } else {
                params_out.push(format!("{pname}:{mapped}"));
            }
        }
        if let Some(rest) = &m.function.rest {
            emitter.mark_declared(rest);
            emitter.env.define(rest.clone(), HirType::Array);
            params_out.push(format!("...{rest}:Array"));
        }

        let ret_part = if is_ctor {
            String::new()
        } else {
            let ret_ty = m
                .function
                .returns
                .as_deref()
                .map(|r| emitter.map_type_annotation(r, true))
                .unwrap_or_else(|| {
                    if m.accessor == Some(AccessorKind::Set) {
                        "void".into()
                    } else if function_has_value_return(&m.function.body) {
                        "*".into()
                    } else {
                        "void".into()
                    }
                });
            format!(":{ret_ty}")
        };

        if parsed.is_interface {
            emitter.pop_fn_scope();
            member_blocks.push(format!(
                "{pad}function {get_set}{fname}({}){ret_part};",
                params_out.join(", ")
            ));
        } else {
            let ret_clean = ret_part.trim_start_matches(':');
            let fixed_body = if is_ctor {
                m.function.body.clone()
            } else {
                ensure_as3_return_completeness(&m.function.body, ret_clean)
            };
            emitter.indent = 3;
            let body_str = emitter.emit_stmt_list(&fixed_body);
            emitter.indent = 2;
            emitter.pop_fn_scope();
            member_blocks.push(format!(
                "{pad}{acc}{ovr}{stat}function {get_set}{fname}({}){ret_part} {{\n{body_str}\n{pad}}}",
                params_out.join(", ")
            ));
        }
    }

    let extends_clause = if let Some(base) = &parsed.base_class {
        let mapped_base = emitter.map_type_annotation(base, false);
        format!(" extends {mapped_base}")
    } else if parsed.is_interface && !parsed.interfaces.is_empty() {
        let mapped: Vec<String> = parsed
            .interfaces
            .iter()
            .map(|i| emitter.map_type_annotation(i, false))
            .collect();
        format!(" extends {}", mapped.join(", "))
    } else {
        String::new()
    };

    let implements_clause = if !parsed.is_interface && !parsed.interfaces.is_empty() {
        let mapped: Vec<String> = parsed
            .interfaces
            .iter()
            .map(|i| emitter.map_type_annotation(i, false))
            .collect();
        format!(" implements {}", mapped.join(", "))
    } else {
        String::new()
    };

    let vis = if parsed.is_public {
        "public "
    } else {
        "internal "
    };
    let dyn_kw = if parsed.is_dynamic && !parsed.is_interface {
        "dynamic "
    } else {
        ""
    };
    let kind_kw = if parsed.is_interface {
        "interface"
    } else {
        "class"
    };

    let class_decl = format!(
        "    {vis}{dyn_kw}{kind_kw} {}{extends_clause}{implements_clause} {{\n{}\n    }}",
        parsed.short_name,
        member_blocks.join("\n\n")
    );

    let import_lines: Vec<String> = emitter
        .format_imports(&class_decl)
        .into_iter()
        .filter(|line| {
            // Do not import the class itself or same-package classes needlessly
            !line.ends_with(&format!(" {};", parsed.short_name))
        })
        .map(|l| format!("    {l}"))
        .collect();

    let pkg_head = if parsed.package.is_empty() {
        "package {".to_owned()
    } else {
        format!("package {} {{", parsed.package)
    };

    let full_source = if import_lines.is_empty() {
        format!("{pkg_head}\n{class_decl}\n}}\n")
    } else {
        format!(
            "{pkg_head}\n{}\n\n{class_decl}\n}}\n",
            import_lines.join("\n")
        )
    };

    As3EmitOutput {
        source: full_source,
        issues: emitter.issues,
        synthesized_linkages: emitter.synthesized_linkages.into_values().collect(),
    }
}

fn method_needs_as3_override(
    parsed: &ParsedClass,
    method_name: &str,
    stage: &ProjectStageIndex,
) -> bool {
    let Some(mut cur_base) = parsed.base_class.clone() else {
        return false;
    };
    if method_name == "toString" {
        return true;
    }
    let mut visited = BTreeSet::new();
    while visited.insert(cur_base.clone()) {
        let qname = stage
            .project_classes
            .get(&cur_base)
            .cloned()
            .unwrap_or_else(|| cur_base.clone());
        if stage
            .class_instance_methods
            .get(&qname)
            .is_some_and(|m| m.contains(method_name))
        {
            return true;
        }
        if let Some(next) = stage.class_bases.get(&qname) {
            cur_base = next.clone();
        } else {
            break;
        }
    }
    false
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

fn extract_callback_target(lhs: &Expr) -> Option<(Expr, &str)> {
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

fn is_as2_drawing_method(name: &str) -> bool {
    matches!(
        name,
        "clear" | "beginFill" | "lineStyle" | "moveTo" | "lineTo" | "curveTo" | "endFill"
    )
}

fn function_has_value_return(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s.unlocated() {
        Stmt::Return(Some(_), _) => true,
        Stmt::Function(_) | Stmt::LegacyEvent(_, _) => false,
        Stmt::Try(b, c, f) => {
            function_has_value_return(b)
                || c.iter().any(|cl| function_has_value_return(&cl.body))
                || f.as_ref().is_some_and(|fin| function_has_value_return(fin))
        }
        other => crate::actionscript::scope::statements(other)
            .into_iter()
            .any(|child| function_has_value_return(std::slice::from_ref(child))),
    })
}

/// Ensure functions typed with non-`void`/`*` return types in AS3 satisfy strict all-paths return checking.
fn ensure_as3_return_completeness(stmts: &[Stmt], ret_type: &str) -> Vec<Stmt> {
    if matches!(ret_type, "" | "void" | "*") {
        return stmts.to_vec();
    }
    let default_expr = match ret_type {
        "Number" | "int" | "uint" => Expr::Literal("0".into()),
        "Boolean" => Expr::Literal("false".into()),
        _ => Expr::Literal("null".into()),
    };
    let mut patched: Vec<Stmt> = stmts
        .iter()
        .map(|s| replace_bare_returns(s, &default_expr))
        .collect();
    if !block_always_returns(&patched) {
        patched.push(Stmt::Return(Some(default_expr), 0));
    }
    patched
}

fn replace_bare_returns(stmt: &Stmt, default_expr: &Expr) -> Stmt {
    match stmt {
        Stmt::Located(off, inner) => {
            Stmt::Located(*off, Box::new(replace_bare_returns(inner, default_expr)))
        }
        Stmt::Return(None, off) => Stmt::Return(Some(default_expr.clone()), *off),
        Stmt::Block(b) => Stmt::Block(
            b.iter()
                .map(|s| replace_bare_returns(s, default_expr))
                .collect(),
        ),
        Stmt::If(c, y, n) => Stmt::If(
            c.clone(),
            Box::new(replace_bare_returns(y, default_expr)),
            n.as_ref()
                .map(|no| Box::new(replace_bare_returns(no, default_expr))),
        ),
        Stmt::While(c, b) => {
            Stmt::While(c.clone(), Box::new(replace_bare_returns(b, default_expr)))
        }
        Stmt::For(i, c, u, b) => Stmt::For(
            i.clone(),
            c.clone(),
            u.clone(),
            Box::new(replace_bare_returns(b, default_expr)),
        ),
        Stmt::ForIn(i, c, e, b) => Stmt::ForIn(
            i.clone(),
            c.clone(),
            *e,
            Box::new(replace_bare_returns(b, default_expr)),
        ),
        Stmt::DoWhile(b, c) => {
            Stmt::DoWhile(Box::new(replace_bare_returns(b, default_expr)), c.clone())
        }
        Stmt::Switch(d, cases) => Stmt::Switch(
            d.clone(),
            cases
                .iter()
                .map(|cs| Case {
                    test: cs.test.clone(),
                    body: cs
                        .body
                        .iter()
                        .map(|s| replace_bare_returns(s, default_expr))
                        .collect(),
                })
                .collect(),
        ),
        Stmt::Try(b, catches, fin) => Stmt::Try(
            b.iter()
                .map(|s| replace_bare_returns(s, default_expr))
                .collect(),
            catches
                .iter()
                .map(|cl| CatchClause {
                    body: cl
                        .body
                        .iter()
                        .map(|s| replace_bare_returns(s, default_expr))
                        .collect(),
                    ..cl.clone()
                })
                .collect(),
            fin.as_ref().map(|f| {
                f.iter()
                    .map(|s| replace_bare_returns(s, default_expr))
                    .collect()
            }),
        ),
        other => other.clone(),
    }
}

fn block_always_returns(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_always_returns)
}

fn stmt_always_returns(stmt: &Stmt) -> bool {
    match stmt.unlocated() {
        Stmt::Return(Some(_), _) | Stmt::Throw(_) => true,
        Stmt::Block(b) => block_always_returns(b),
        Stmt::If(_, yes, Some(no)) => stmt_always_returns(yes) && stmt_always_returns(no),
        _ => false,
    }
}

pub(crate) fn op_prec(op: &str) -> u8 {
    match op {
        "," => 1,
        "||" | "or" => 4,
        "&&" | "and" => 5,
        "|" => 6,
        "^" => 7,
        "&" => 8,
        "==" | "!=" | "===" | "!==" | "eq" | "ne" | "<>" => 9,
        "<" | "<=" | ">" | ">=" | "in" => 10,
        "<<" | ">>" | ">>>" => 11,
        "+" | "-" | "add" => 12,
        "*" | "/" | "%" => 13,
        _ => 20,
    }
}
