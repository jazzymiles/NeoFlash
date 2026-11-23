//! Deterministic AST-to-ActionScript 2.0 emitter (`AS3 -> AS2` and `AS1 -> AS2`).

use super::emit_as3::op_prec;
use super::hir::{
    AccessorKind, HirType, ParsedClass, ParsedScript, ProjectStageIndex, TypeEnv,
    extract_numeric_literal, fold_01_to_100, format_clean_number, is_atomic_expr,
    map_as3_keyboard_constant, map_modern_direct_prop_to_as2, map_modern_unit_prop_to_as2,
    short_type_name,
};
use super::{ConversionIssue, ConversionIssueKind};
use crate::actionscript::ast::{Case, Expr, Function, PropertyKey, Stmt};
use crate::byte_offset_to_line_col;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct As2EmitOutput {
    pub(crate) source: String,
    pub(crate) normalized_path: Option<String>,
    pub(crate) issues: Vec<ConversionIssue>,
}

enum DynamicDisplayKind {
    EmptyClip,
    LinkedClip(String),
    Bitmap(Vec<Expr>),
    TextField,
    Loader,
    Video(usize),
}

struct DynamicSpritePlan {
    parent_expr: Expr,
    custom_name: Option<Expr>,
    kind: DynamicDisplayKind,
    add_child_stmt_idx: usize,
    name_assign_stmt_idx: Option<usize>,
}

struct As2Emitter<'a> {
    location: &'a str,
    raw_source: &'a str,
    stage: &'a ProjectStageIndex,
    env: TypeEnv,
    imports: BTreeSet<String>,
    issues: Vec<ConversionIssue>,
    declared_in_scope: Vec<BTreeSet<String>>,
    key_handler_fns: BTreeSet<String>,
    in_key_handler_param: Option<String>,
    helper_counter: usize,
    indent: usize,
}

impl<'a> As2Emitter<'a> {
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
            declared_in_scope: vec![BTreeSet::new()],
            key_handler_fns: BTreeSet::new(),
            in_key_handler_param: None,
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

    fn preserve_as2_imports(&mut self, raw_imports: &[String]) {
        for imp in raw_imports {
            if is_as2_compatible_import(imp, self.stage) {
                self.imports.insert(imp.clone());
            }
        }
    }

    fn map_type_to_as2(&mut self, raw: &str, is_return: bool, offset: usize) -> String {
        let trimmed = raw.trim();
        if trimmed.starts_with("Vector.<") {
            self.record_issue(
                ConversionIssueKind::Approximated,
                offset,
                format!("Approximated AS3 '{trimmed}' as AS2 'Array'."),
            );
            return "Array".into();
        }
        if matches!(trimmed, "ByteArray" | "flash.utils.ByteArray") {
            self.record_issue(
                ConversionIssueKind::ManualAttention,
                offset,
                "AS3 ByteArray has no native AVM1 equivalent; typed as Object.",
            );
            return "Object".into();
        }
        if matches!(
            trimmed,
            "Stage3D" | "flash.display.Stage3D" | "Context3D" | "flash.display3D.Context3D"
        ) {
            self.record_issue(
                ConversionIssueKind::ManualAttention,
                offset,
                format!("AS3 '{trimmed}' is not available in ActionScript 2.0."),
            );
            return "Object".into();
        }
        if matches!(trimmed, "Dictionary" | "flash.utils.Dictionary") {
            self.record_issue(
                ConversionIssueKind::Approximated,
                offset,
                "Approximated AS3 'Dictionary' as AS2 'Object'.",
            );
            return "Object".into();
        }
        match trimmed {
            "void" | "Void" if is_return => "Void".into(),
            "int" | "uint" | "Number" => "Number".into(),
            "Boolean" => "Boolean".into(),
            "String" => "String".into(),
            "Array" => "Array".into(),
            "Function" => "Function".into(),
            "*" | "Object" | "Class" => "Object".into(),
            "Sprite"
            | "Shape"
            | "Bitmap"
            | "MovieClip"
            | "DisplayObject"
            | "DisplayObjectContainer"
            | "InteractiveObject"
            | "flash.display.Sprite"
            | "flash.display.Shape"
            | "flash.display.Bitmap"
            | "flash.display.MovieClip"
            | "flash.display.DisplayObject"
            | "flash.display.DisplayObjectContainer"
            | "flash.display.InteractiveObject" => "MovieClip".into(),
            "SimpleButton" | "Button" | "flash.display.SimpleButton" => "Button".into(),
            "TextField" | "flash.text.TextField" => "TextField".into(),
            "TextFormat" | "flash.text.TextFormat" => "TextFormat".into(),
            "StyleSheet" | "flash.text.StyleSheet" | "TextField.StyleSheet" => {
                "TextField.StyleSheet".into()
            }
            "Sound" | "flash.media.Sound" => "Sound".into(),
            "NetConnection" | "flash.net.NetConnection" => "NetConnection".into(),
            "NetStream" | "flash.net.NetStream" => "NetStream".into(),
            "SharedObject" | "flash.net.SharedObject" => "SharedObject".into(),
            "XML" | "XMLList" | "XMLDocument" | "flash.xml.XMLDocument" => "XML".into(),
            "XMLNode" | "flash.xml.XMLNode" => "XMLNode".into(),
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
            "GlowFilter" | "DropShadowFilter" | "BlurFilter" | "BevelFilter"
            | "ColorMatrixFilter" => {
                self.imports.insert(format!("flash.filters.{trimmed}"));
                trimmed.into()
            }
            "Video" | "flash.media.Video" => {
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    "AS2 Video requires an authored Video timeline placement; typed as Object.",
                );
                "Object".into()
            }
            "Event" | "MouseEvent" | "KeyboardEvent" | "TimerEvent" | "FocusEvent"
            | "ProgressEvent" | "IOErrorEvent" | "Stage" | "SoundChannel" | "SoundTransform"
            | "URLRequest" | "URLLoader" | "Loader" | "RegExp" => "Object".into(),
            other => {
                let short = short_type_name(other);
                if let Some(qname) = self.stage.project_classes.get(short) {
                    if qname.contains('.') {
                        self.imports.insert(qname.clone());
                    }
                    return short.to_owned();
                }
                if other.starts_with("flash.") && !is_as2_compatible_import(other, self.stage) {
                    return "Object".into();
                }
                short.to_owned()
            }
        }
    }

    fn precollect_key_listeners(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.scan_stmt_for_key_listeners(stmt);
        }
    }

    fn scan_stmt_for_key_listeners(&mut self, stmt: &Stmt) {
        match stmt.unlocated() {
            Stmt::Expr(e) => self.scan_expr_for_key_listeners(e),
            Stmt::Function(f) => {
                if let Some(name) = &f.name {
                    let ret = f
                        .returns
                        .as_deref()
                        .map_or(HirType::Unknown, HirType::from_annotation);
                    self.env.functions.insert(name.clone(), ret);
                }
                // Also detect functions whose single param is typed `KeyboardEvent`!
                if let Some(fname) = &f.name
                    && f.params.first().is_some_and(|(_, ty)| {
                        ty.as_deref()
                            .is_some_and(|t| short_type_name(t) == "KeyboardEvent")
                    })
                {
                    self.key_handler_fns.insert(fname.clone());
                }
                for child in &f.body {
                    self.scan_stmt_for_key_listeners(child);
                }
            }
            other => {
                for e in crate::actionscript::scope::expressions(other) {
                    self.scan_expr_for_key_listeners(e);
                }
                for s in crate::actionscript::scope::statements(other) {
                    self.scan_stmt_for_key_listeners(s);
                }
            }
        }
    }

    fn scan_expr_for_key_listeners(&mut self, expr: &Expr) {
        if let Expr::Call(callee, args, _) = expr {
            let is_add = match callee.as_ref() {
                Expr::Name(n) => n == "addEventListener",
                Expr::Member(_, m) => m == "addEventListener",
                _ => false,
            };
            if is_add
                && args.len() >= 2
                && is_keyboard_event_arg(&args[0])
                && let Some(fn_name) = extract_fn_ident(&args[1])
            {
                self.key_handler_fns.insert(fn_name);
            }
        }
        for c in crate::actionscript::scope::children(expr) {
            self.scan_expr_for_key_listeners(c);
        }
    }

    fn find_linkage_for_class(&self, cls_name: &str) -> Option<String> {
        let short = short_type_name(cls_name);
        for (linkage_id, info) in &self.stage.linkages {
            if short_type_name(&info.class_name) == short || linkage_id == short {
                return Some(linkage_id.clone());
            }
        }
        None
    }

    /// Analyze a block of statements to pair `var c = new Sprite|Bitmap|TextField|Loader|Video();` with `addChild(c);`.
    fn plan_dynamic_sprites(&self, stmts: &[Stmt]) -> BTreeMap<usize, DynamicSpritePlan> {
        let mut plans = BTreeMap::new();
        let mut candidates: BTreeMap<String, (usize, DynamicDisplayKind)> = BTreeMap::new();
        let mut name_assigns: BTreeMap<String, (usize, Expr)> = BTreeMap::new();

        for (idx, stmt) in stmts.iter().enumerate() {
            match stmt.unlocated() {
                Stmt::Var(vars) if vars.len() == 1 => {
                    let (vname, _, val_opt) = &vars[0];
                    if let Some(val) = val_opt
                        && let Some((ctor_name, ctor_args, ctor_off)) =
                            match_new_display_ctor_with_args(val)
                    {
                        let kind_opt = match ctor_name {
                            "Sprite" | "MovieClip" | "Shape" => Some(DynamicDisplayKind::EmptyClip),
                            "Bitmap" => Some(DynamicDisplayKind::Bitmap(ctor_args.to_vec())),
                            "TextField" => Some(DynamicDisplayKind::TextField),
                            "Loader" => Some(DynamicDisplayKind::Loader),
                            "Video" => Some(DynamicDisplayKind::Video(ctor_off)),
                            other => self
                                .find_linkage_for_class(other)
                                .map(DynamicDisplayKind::LinkedClip),
                        };
                        if let Some(kind) = kind_opt {
                            candidates.insert(vname.clone(), (idx, kind));
                        }
                    }
                }
                Stmt::Expr(Expr::Assign(op, lhs, rhs)) if op == "=" => {
                    if let Expr::Member(base, prop) = lhs.as_ref()
                        && prop == "name"
                        && let Expr::Name(vname) = base.as_ref()
                        && candidates.contains_key(vname)
                    {
                        name_assigns.insert(vname.clone(), (idx, (**rhs).clone()));
                    }
                }
                Stmt::Expr(Expr::Call(callee, args, _)) => {
                    if let Some(parent_expr) = match_add_child_call(callee)
                        && let Some(Expr::Name(vname)) = args.first()
                        && let Some((decl_idx, kind)) = candidates.remove(vname)
                    {
                        let name_info = name_assigns.remove(vname);
                        plans.insert(
                            decl_idx,
                            DynamicSpritePlan {
                                parent_expr,
                                custom_name: name_info.as_ref().map(|(_, e)| e.clone()),
                                kind,
                                add_child_stmt_idx: idx,
                                name_assign_stmt_idx: name_info.map(|(i, _)| i),
                            },
                        );
                    }
                }
                _ => {}
            }
        }
        plans
    }

    fn emit_stmt_list(&mut self, stmts: &[Stmt]) -> String {
        let plans = self.plan_dynamic_sprites(stmts);
        let mut skipped_indices = BTreeSet::new();
        for plan in plans.values() {
            skipped_indices.insert(plan.add_child_stmt_idx);
            if let Some(na_idx) = plan.name_assign_stmt_idx {
                skipped_indices.insert(na_idx);
            }
        }

        let mut lines = Vec::new();
        for (idx, stmt) in stmts.iter().enumerate() {
            if skipped_indices.contains(&idx) {
                continue;
            }
            if let Some(plan) = plans.get(&idx)
                && let Stmt::Var(vars) = stmt.unlocated()
            {
                let vname = &vars[0].0;
                let pad = self.pad();
                let parent_code = self.emit_expr_with_offset(&plan.parent_expr, 0);
                let name_code = match &plan.custom_name {
                    Some(ne) => self.emit_expr_with_offset(ne, 0),
                    None => serde_json::to_string(vname).unwrap(),
                };
                let first_decl = self.mark_declared(vname);
                match &plan.kind {
                    DynamicDisplayKind::TextField => {
                        self.env.define(vname.clone(), HirType::TextField);
                        let rhs = format!(
                            "{parent_code}.createTextField({name_code}, {parent_code}.getNextHighestDepth(), 0, 0, 100, 100)"
                        );
                        if first_decl {
                            lines.push(format!("{pad}var {vname}:TextField = {rhs};"));
                        } else {
                            lines.push(format!("{pad}{vname} = {rhs};"));
                        }
                    }
                    DynamicDisplayKind::Bitmap(bargs) => {
                        self.env.define(vname.clone(), HirType::MovieClip);
                        let rhs = format!(
                            "{parent_code}.createEmptyMovieClip({name_code}, {parent_code}.getNextHighestDepth())"
                        );
                        if first_decl {
                            lines.push(format!("{pad}var {vname}:MovieClip = {rhs};"));
                        } else {
                            lines.push(format!("{pad}{vname} = {rhs};"));
                        }
                        if let Some(bmd_arg) = bargs.first() {
                            let bmd = self.emit_expr_with_offset(bmd_arg, 0);
                            let snap = bargs
                                .get(1)
                                .map(|a| self.emit_expr_with_offset(a, 0))
                                .unwrap_or_else(|| "\"auto\"".into());
                            let smooth = bargs
                                .get(2)
                                .map(|a| self.emit_expr_with_offset(a, 0))
                                .unwrap_or_else(|| "false".into());
                            lines.push(format!(
                                "{pad}{vname}.attachBitmap({bmd}, 1, {snap}, {smooth});"
                            ));
                        }
                    }
                    DynamicDisplayKind::Loader => {
                        self.env.define(vname.clone(), HirType::MovieClip);
                        let rhs = format!(
                            "{parent_code}.createEmptyMovieClip({name_code}, {parent_code}.getNextHighestDepth())"
                        );
                        if first_decl {
                            lines.push(format!("{pad}var {vname}:MovieClip = {rhs};"));
                        } else {
                            lines.push(format!("{pad}{vname} = {rhs};"));
                        }
                        lines.push(format!(
                            "{pad}{vname}.contentLoaderInfo = {{_width: 0, _height: 0}};"
                        ));
                    }
                    DynamicDisplayKind::Video(voff) => {
                        self.record_issue(
                            ConversionIssueKind::ManualAttention,
                            *voff,
                            "AS2 Video must be an authored Video symbol on the timeline; lowered dynamic new Video() to a MovieClip placeholder.",
                        );
                        self.env.define(vname.clone(), HirType::MovieClip);
                        let rhs = format!(
                            "/* FIXME [NeoFlash Convert]: AS2 Video must be an authored Video placement */ {parent_code}.createEmptyMovieClip({name_code}, {parent_code}.getNextHighestDepth())"
                        );
                        if first_decl {
                            lines.push(format!("{pad}var {vname}:Object = {rhs};"));
                        } else {
                            lines.push(format!("{pad}{vname} = {rhs};"));
                        }
                        lines.push(format!(
                            "{pad}{vname}.attachVideo = function(_src:Object):Void {{}};"
                        ));
                    }
                    DynamicDisplayKind::LinkedClip(linkage) => {
                        self.env.define(vname.clone(), HirType::MovieClip);
                        let link_lit = serde_json::to_string(linkage).unwrap();
                        let rhs = format!(
                            "{parent_code}.attachMovie({link_lit}, {name_code}, {parent_code}.getNextHighestDepth())"
                        );
                        if first_decl {
                            lines.push(format!("{pad}var {vname}:MovieClip = {rhs};"));
                        } else {
                            lines.push(format!("{pad}{vname} = {rhs};"));
                        }
                    }
                    DynamicDisplayKind::EmptyClip => {
                        self.env.define(vname.clone(), HirType::MovieClip);
                        let rhs = format!(
                            "{parent_code}.createEmptyMovieClip({name_code}, {parent_code}.getNextHighestDepth())"
                        );
                        if first_decl {
                            lines.push(format!("{pad}var {vname}:MovieClip = {rhs};"));
                        } else {
                            lines.push(format!("{pad}{vname} = {rhs};"));
                        }
                    }
                }
                continue;
            }

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
            Stmt::DefaultXmlNamespace(e) => {
                let raw = self.approx_source_expr(e);
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    "E4X default xml namespace is not supported in ActionScript 2.0.",
                );
                format!(
                    "{pad}/* FIXME [NeoFlash Convert]: default xml namespace is not supported in AS2: {raw} */"
                )
            }
            Stmt::Var(vars) => {
                let mut out_lines = Vec::new();
                for (name, ty_opt, val_opt) in vars {
                    // AS3 `var chime:Sound = new Sound(new URLRequest(url));` -> AS2 `new Sound(this)` + `loadSound`
                    if let Some(val) = val_opt
                        && let Some((ctor_short, cargs, _)) = match_new_display_ctor_with_args(val)
                        && ctor_short == "Sound"
                    {
                        let first_decl = self.mark_declared(name);
                        self.env
                            .define(name.clone(), HirType::Class("Sound".into()));
                        if first_decl {
                            out_lines.push(format!("{pad}var {name}:Sound = new Sound(this);"));
                        } else {
                            out_lines.push(format!("{pad}{name} = new Sound(this);"));
                        }
                        if let Some(req_arg) = cargs.first() {
                            let url = self.unwrap_url_request_expr(req_arg, offset);
                            out_lines.push(format!("{pad}{name}.loadSound({url}, false);"));
                        }
                        continue;
                    }
                    // AS3 `var channel:SoundChannel = chime.play(ms, loops, new SoundTransform(vol, pan));`
                    if let Some(Expr::Call(callee, cargs, _)) = val_opt
                        && let Expr::Member(snd_base, m) = callee.as_ref()
                        && m == "play"
                        && (ty_opt
                            .as_deref()
                            .is_some_and(|t| short_type_name(t) == "SoundChannel")
                            || matches!(
                                self.env.infer_expr(snd_base, self.stage),
                                HirType::Class(ref c) if short_type_name(c) == "Sound"
                            ))
                    {
                        let snd_code = self.emit_expr_with_offset(snd_base, offset);
                        if let Some(st_arg) = cargs.get(2)
                            && let Some(("SoundTransform", st_args, _)) =
                                match_new_display_ctor_with_args(st_arg)
                            && let Some(vol_arg) = st_args.first()
                        {
                            let vol100 = fold_01_to_100(vol_arg).unwrap_or_else(|| {
                                let vc = self.emit_expr_with_offset(vol_arg, offset);
                                format!("({vc}) * 100")
                            });
                            out_lines.push(format!("{pad}{snd_code}.setVolume({vol100});"));
                        }
                        let sec_code = match cargs.first() {
                            Some(ms_arg) => {
                                if let Some(ms) = extract_numeric_literal(ms_arg) {
                                    format_clean_number(ms / 1000.0, false)
                                } else {
                                    let mc = self.emit_expr_with_offset(ms_arg, offset);
                                    format!("({mc}) / 1000")
                                }
                            }
                            None => "0".into(),
                        };
                        let loops_code = cargs
                            .get(1)
                            .map(|l| self.emit_expr_with_offset(l, offset))
                            .unwrap_or_else(|| "1".into());
                        out_lines.push(format!("{pad}{snd_code}.start({sec_code}, {loops_code});"));
                        let first_decl = self.mark_declared(name);
                        self.env.define(name.clone(), HirType::Object);
                        if first_decl {
                            out_lines.push(format!("{pad}var {name}:Object = {snd_code};"));
                        } else {
                            out_lines.push(format!("{pad}{name} = {snd_code};"));
                        }
                        continue;
                    }

                    // Check `var code:uint = event.keyCode` inside a key listener
                    let hir_ty = ty_opt
                        .as_deref()
                        .map(HirType::from_annotation)
                        .or_else(|| val_opt.as_ref().map(|e| self.env.infer_expr(e, self.stage)))
                        .unwrap_or(HirType::Unknown);
                    let as2_ty = match ty_opt {
                        Some(raw) => self.map_type_to_as2(raw, false, offset),
                        None => hir_ty.to_as2_annotation(),
                    };
                    let first_decl = self.mark_declared(name);
                    self.env.define(name.clone(), hir_ty);

                    if let Some(val) = val_opt {
                        let vc = self.emit_expr_with_offset(val, offset);
                        if first_decl {
                            out_lines.push(format!("{pad}var {name}:{as2_ty} = {vc};"));
                        } else {
                            out_lines.push(format!("{pad}{name} = {vc};"));
                        }
                    } else if first_decl {
                        out_lines.push(format!("{pad}var {name}:{as2_ty};"));
                    }
                }
                out_lines.join("\n")
            }
            Stmt::Function(f) => self.emit_function_decl(f, offset),
            Stmt::Block(body) => {
                self.indent += 1;
                let inner = self.emit_stmt_list(body);
                self.indent -= 1;
                format!("{pad}{{\n{inner}\n{pad}}}")
            }
            Stmt::If(cond, yes, no) => {
                // Unwrap `if (stage != null) { ... }` since global `Stage` is always present in AS2!
                if is_stage_non_null_check(cond) {
                    return match yes.unlocated() {
                        Stmt::Block(stmts) => self.emit_stmt_list(stmts),
                        _ => self.emit_stmt(yes),
                    };
                }
                self.emit_if(cond, yes, no.as_deref(), offset)
            }
            Stmt::While(cond, body) => {
                let c = self.emit_expr_with_offset(cond, offset);
                let b = self.emit_block_body(body);
                format!("{pad}while ({c}) {b}")
            }
            Stmt::DoWhile(body, cond) => {
                let b = self.emit_block_body(body);
                let c = self.emit_expr_with_offset(cond, offset);
                format!("{pad}do {b} while ({c});")
            }
            Stmt::For(init, cond, upd, body) => {
                let init_str = match init.unlocated() {
                    Stmt::Empty => String::new(),
                    Stmt::Var(_) => self.emit_stmt(init).trim().trim_end_matches(';').to_owned(),
                    Stmt::Expr(e) => self.emit_expr_with_offset(e, offset),
                    _ => String::new(),
                };
                let cond_str = cond
                    .as_ref()
                    .map(|e| self.emit_expr_with_offset(e, offset))
                    .unwrap_or_default();
                let upd_str = upd
                    .as_ref()
                    .map(|e| self.emit_expr_with_offset(e, offset))
                    .unwrap_or_default();
                let b = self.emit_block_body(body);
                format!("{pad}for ({init_str}; {cond_str}; {upd_str}) {b}")
            }
            Stmt::ForIn(init, coll, each, body) => {
                if *each {
                    self.record_issue(
                        ConversionIssueKind::Approximated,
                        offset,
                        "Lowered AS3 'for each..in' loop to AS2 key iteration.",
                    );
                    let coll_var = self.next_helper("eachColl");
                    let key_var = self.next_helper("eachKey");
                    let coll_code = self.emit_expr_with_offset(coll, offset);
                    let _ = self.mark_declared(&coll_var);
                    let _ = self.mark_declared(&key_var);
                    let assign_line = match init.unlocated() {
                        Stmt::Var(vars) if !vars.is_empty() => {
                            let vname = &vars[0].0;
                            let vty = vars[0]
                                .1
                                .as_deref()
                                .map(|t| self.map_type_to_as2(t, false, offset))
                                .unwrap_or_else(|| "Object".into());
                            let first_decl = self.mark_declared(vname);
                            self.env.define(vname.clone(), HirType::Object);
                            if first_decl {
                                format!("var {vname}:{vty} = {coll_var}[{key_var}];")
                            } else {
                                format!("{vname} = {coll_var}[{key_var}];")
                            }
                        }
                        Stmt::Expr(e) => {
                            let ec = self.emit_expr_with_offset(e, offset);
                            format!("{ec} = {coll_var}[{key_var}];")
                        }
                        _ => String::new(),
                    };
                    self.indent += 1;
                    let inner_pad = self.pad();
                    let inner_body = match body.unlocated() {
                        Stmt::Block(stmts) => self.emit_stmt_list(stmts),
                        _ => self.emit_stmt(body),
                    };
                    self.indent -= 1;
                    return format!(
                        "{pad}var {coll_var}:Object = {coll_code};\n{pad}for (var {key_var}:String in {coll_var}) {{\n{inner_pad}{assign_line}\n{inner_body}\n{pad}}}"
                    );
                }

                let init_str = match init.unlocated() {
                    Stmt::Var(vars) if !vars.is_empty() => {
                        let vname = &vars[0].0;
                        let vty = vars[0]
                            .1
                            .as_deref()
                            .map(|t| self.map_type_to_as2(t, false, offset))
                            .unwrap_or_else(|| "String".into());
                        let first_decl = self.mark_declared(vname);
                        self.env.define(vname.clone(), HirType::String);
                        if first_decl {
                            format!("var {vname}:{vty}")
                        } else {
                            vname.clone()
                        }
                    }
                    Stmt::Expr(e) => self.emit_expr_with_offset(e, offset),
                    _ => "var _k:String".into(),
                };
                let coll_str = self.emit_expr_with_offset(coll, offset);
                let b = self.emit_block_body(body);
                format!("{pad}for ({init_str} in {coll_str}) {b}")
            }
            Stmt::Switch(disc, cases) => {
                let d = self.emit_expr_with_offset(disc, offset);
                let mut out = format!("{pad}switch ({d}) {{\n");
                self.indent += 1;
                let case_pad = self.pad();
                for Case { test, body } in cases {
                    match test {
                        Some(t) => {
                            let tc = self.emit_expr_with_offset(t, offset);
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
            Stmt::Break(_) => format!("{pad}break;"),
            Stmt::Continue(_) => format!("{pad}continue;"),
            Stmt::Label(_, body) => self.emit_stmt(body),
            Stmt::Throw(e) => {
                let ec = self.emit_expr_with_offset(e, offset);
                format!("{pad}throw {ec};")
            }
            Stmt::Try(body, catches, fin) => {
                self.indent += 1;
                let b = self.emit_stmt_list(body);
                self.indent -= 1;
                let mut out = format!("{pad}try {{\n{b}\n{pad}}}");
                if let Some(first_catch) = catches.first() {
                    let cname = &first_catch.name;
                    self.env.push_scope();
                    self.env.define(cname.clone(), HirType::Object);
                    self.indent += 1;
                    let cb = self.emit_stmt_list(&first_catch.body);
                    self.indent -= 1;
                    self.env.pop_scope();
                    out.push_str(&format!(" catch ({cname}) {{\n{cb}\n{pad}}}"));
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
                    let ec = self.emit_expr_with_offset(e, offset);
                    format!("{pad}return {ec};")
                }
                None => format!("{pad}return;"),
            },
            Stmt::With(target, body, _) => {
                let t = self.emit_expr_with_offset(target, offset);
                let b = self.emit_block_body(body);
                format!("{pad}with ({t}) {b}")
            }
            Stmt::TellTarget(target, body, _) => {
                let t = self.emit_expr_with_offset(target, offset);
                let b = self.emit_block_body(body);
                format!("{pad}with ({t}) {b}")
            }
            Stmt::IfFrameLoaded(args, body) => {
                let ac = args
                    .iter()
                    .map(|a| self.emit_expr_with_offset(a, offset))
                    .collect::<Vec<_>>()
                    .join(", ");
                let b = self.emit_block_body(body);
                format!("{pad}ifFrameLoaded ({ac}) {b}")
            }
            Stmt::InitClip(ord, body, _) => {
                let inner = self.emit_stmt_list(body);
                match ord {
                    Some(n) => format!("{pad}#initclip {n}\n{inner}\n{pad}#endinitclip"),
                    None => format!("{pad}#initclip\n{inner}\n{pad}#endinitclip"),
                }
            }
            Stmt::LegacyEvent(trigs, func) => {
                let head_parts: Vec<String> = trigs
                    .iter()
                    .map(|t| {
                        if let Some(kp) = &t.key_press {
                            format!("keyPress {kp:?}")
                        } else {
                            t.event.to_owned()
                        }
                    })
                    .collect();
                let kw = if trigs.first().is_some_and(|t| t.clip_event) {
                    "onClipEvent"
                } else {
                    "on"
                };
                self.indent += 1;
                let inner = self.emit_stmt_list(&func.body);
                self.indent -= 1;
                format!("{pad}{kw}({}) {{\n{inner}\n{pad}}}", head_parts.join(", "))
            }
            Stmt::Expr(expr) => self.emit_expr_stmt(expr, offset),
        }
    }

    fn emit_block_body(&mut self, stmt: &Stmt) -> String {
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

    fn emit_if(&mut self, cond: &Expr, yes: &Stmt, no: Option<&Stmt>, offset: usize) -> String {
        let pad = self.pad();
        let c = self.emit_expr_with_offset(cond, offset);
        let yes_block = self.emit_block_body(yes);
        let mut out = format!("{pad}if ({c}) {yes_block}");
        if let Some(alt) = no {
            match alt.unlocated() {
                Stmt::If(nc, ny, nn) if !is_stage_non_null_check(nc) => {
                    let rest = self.emit_if(nc, ny, nn.as_deref(), offset);
                    out.push_str(" else ");
                    out.push_str(rest.trim_start());
                }
                _ => {
                    let no_block = self.emit_block_body(alt);
                    out.push_str(&format!(" else {no_block}"));
                }
            }
        }
        out
    }

    fn emit_function_decl(&mut self, f: &Function, offset: usize) -> String {
        let pad = self.pad();
        let name = f.name.as_deref().unwrap_or("_anon");
        let is_key_fn = self.key_handler_fns.contains(name);
        let prev_key_param = if is_key_fn {
            f.params.first().map(|(pname, _)| pname.clone())
        } else {
            None
        };
        let saved_kp = self.in_key_handler_param.clone();
        if prev_key_param.is_some() {
            self.in_key_handler_param = prev_key_param;
        }

        let ret_as2 = f
            .returns
            .as_deref()
            .map(|r| self.map_type_to_as2(r, true, offset))
            .unwrap_or_else(|| "Void".into());

        self.push_fn_scope();
        let mut params_out = Vec::new();
        let mut default_inits = Vec::new();
        for (i, (pname, pty)) in f.params.iter().enumerate() {
            let mapped = pty
                .as_deref()
                .map(|t| self.map_type_to_as2(t, false, offset))
                .unwrap_or_else(|| "Object".into());
            self.mark_declared(pname);
            self.env
                .define(pname.clone(), HirType::from_annotation(&mapped));
            params_out.push(format!("{pname}:{mapped}"));
            if let Some(Some(def_expr)) = f.defaults.get(i) {
                let dc = self.emit_expr_with_offset(def_expr, offset);
                default_inits.push(format!("if ({pname} === undefined) {{ {pname} = {dc}; }}"));
            }
        }
        if let Some(rest) = &f.rest {
            self.mark_declared(rest);
            self.env.define(rest.clone(), HirType::Array);
            default_inits.push(format!(
                "var {rest}:Array = arguments.slice({});",
                f.params.len()
            ));
        }

        self.indent += 1;
        let inner_pad = self.pad();
        let mut body_parts: Vec<String> = default_inits
            .into_iter()
            .map(|l| format!("{inner_pad}{l}"))
            .collect();
        let body_str = self.emit_stmt_list(&f.body);
        if !body_str.trim().is_empty() {
            body_parts.push(body_str);
        }
        self.indent -= 1;
        self.pop_fn_scope();
        self.in_key_handler_param = saved_kp;

        format!(
            "{pad}function {name}({}):{ret_as2} {{\n{}\n{pad}}}",
            params_out.join(", "),
            body_parts.join("\n")
        )
    }

    fn emit_expr_stmt(&mut self, expr: &Expr, offset: usize) -> String {
        let pad = self.pad();

        // 1. `stage.focus = target;` -> `Selection.setFocus(target);` and AS3 property lowerings
        if let Expr::Assign(op, lhs, rhs) = expr
            && op == "="
            && let Expr::Member(base, prop) = lhs.as_ref()
        {
            if is_stage_expr(base) {
                if prop == "focus" {
                    let rc = self.emit_expr_with_offset(rhs, offset);
                    return format!("{pad}Selection.setFocus({rc});");
                }
                if matches!(prop.as_str(), "scaleMode" | "align" | "displayState") {
                    let rc = self.emit_expr_with_offset(rhs, offset);
                    return format!("{pad}Stage.{prop} = {rc};");
                }
            }
            if prop == "defaultTextFormat" {
                let recv = self.emit_expr_with_offset(base, offset);
                let rc = self.emit_expr_with_offset(rhs, offset);
                return format!("{pad}{recv}.setNewTextFormat({rc});");
            }
            if prop == "htmlText" {
                let recv = self.emit_expr_with_offset(base, offset);
                let rc = self.emit_expr_with_offset(rhs, offset);
                return format!("{pad}{recv}.html = true;\n{pad}{recv}.htmlText = {rc};");
            }
            if prop == "buttonMode" {
                let recv = self.emit_expr_with_offset(base, offset);
                let rc = self.emit_expr_with_offset(rhs, offset);
                return format!("{pad}{recv}.useHandCursor = {rc};");
            }
            if prop == "client"
                && let Expr::Object(props) = rhs.as_ref()
            {
                let recv = self.emit_expr_with_offset(base, offset);
                let mut cb_lines = Vec::new();
                for (PropertyKey { name, .. }, val) in props {
                    let vc = self.emit_expr_with_offset(val, offset);
                    cb_lines.push(format!("{pad}{recv}.{name} = {vc};"));
                }
                return cb_lines.join("\n");
            }
        }

        if let Expr::Call(callee, args, call_off) = expr {
            // 2. `mc.graphics.drawRect(x, y, w, h)` & `mc.graphics.drawCircle(cx, cy, r)`
            if let Some((clip_expr, g_method)) = match_graphics_method(callee) {
                if g_method == "drawRect" && args.len() >= 4 {
                    return self.emit_draw_rect(&clip_expr, args, *call_off);
                }
                if g_method == "drawCircle" && args.len() >= 3 {
                    return self.emit_draw_circle(&clip_expr, args, *call_off);
                }
            }

            // 3. `addEventListener` / `removeEventListener`
            if let Some((recv_expr, is_remove)) = match_event_listener_call(callee)
                && args.len() >= 2
            {
                return self
                    .emit_as2_event_listener(&recv_expr, is_remove, &args[0], &args[1], *call_off);
            }

            // 4. `removeChild(c)` -> `c.removeMovieClip()`
            if let Some(_parent) = match_remove_child_call(callee)
                && let Some(child_arg) = args.first()
            {
                let cc = self.emit_expr_with_offset(child_arg, *call_off);
                return format!("{pad}{cc}.removeMovieClip();");
            }

            // 5. Unmatched `addChild(c)` (e.g. non-var or already attached) -> omit invalid AS2 `addChild`
            if match_add_child_call(callee).is_some() && !args.is_empty() {
                let cc = self.emit_expr_with_offset(&args[0], *call_off);
                return format!("{pad}/* [NeoFlash Convert] placed {cc} */");
            }
        }

        let code = self.emit_expr_with_offset(expr, offset);
        format!("{pad}{code};")
    }

    fn emit_draw_rect(&mut self, clip_expr: &Expr, args: &[Expr], offset: usize) -> String {
        let pad = self.pad();
        let recv = self.emit_expr_with_offset(clip_expr, offset);
        let simple = args[..4]
            .iter()
            .all(|a| matches!(a, Expr::Name(_) | Expr::Literal(_)));
        if simple {
            let x = self.emit_expr_with_offset(&args[0], offset);
            let y = self.emit_expr_with_offset(&args[1], offset);
            let w = self.emit_expr_with_offset(&args[2], offset);
            let h = self.emit_expr_with_offset(&args[3], offset);
            [
                format!("{pad}{recv}.moveTo({x}, {y});"),
                format!("{pad}{recv}.lineTo({x} + {w}, {y});"),
                format!("{pad}{recv}.lineTo({x} + {w}, {y} + {h});"),
                format!("{pad}{recv}.lineTo({x}, {y} + {h});"),
                format!("{pad}{recv}.lineTo({x}, {y});"),
            ]
            .join("\n")
        } else {
            let rx = self.next_helper("rx");
            let ry = self.next_helper("ry");
            let rw = self.next_helper("rw");
            let rh = self.next_helper("rh");
            let _ = self.mark_declared(&rx);
            let _ = self.mark_declared(&ry);
            let _ = self.mark_declared(&rw);
            let _ = self.mark_declared(&rh);
            let x = self.emit_expr_with_offset(&args[0], offset);
            let y = self.emit_expr_with_offset(&args[1], offset);
            let w = self.emit_expr_with_offset(&args[2], offset);
            let h = self.emit_expr_with_offset(&args[3], offset);
            [
                format!("{pad}var {rx}:Number = {x};"),
                format!("{pad}var {ry}:Number = {y};"),
                format!("{pad}var {rw}:Number = {w};"),
                format!("{pad}var {rh}:Number = {h};"),
                format!("{pad}{recv}.moveTo({rx}, {ry});"),
                format!("{pad}{recv}.lineTo({rx} + {rw}, {ry});"),
                format!("{pad}{recv}.lineTo({rx} + {rw}, {ry} + {rh});"),
                format!("{pad}{recv}.lineTo({rx}, {ry} + {rh});"),
                format!("{pad}{recv}.lineTo({rx}, {ry});"),
            ]
            .join("\n")
        }
    }

    fn emit_draw_circle(&mut self, clip_expr: &Expr, args: &[Expr], offset: usize) -> String {
        let pad = self.pad();
        let recv = self.emit_expr_with_offset(clip_expr, offset);
        let cx = self.next_helper("cx");
        let cy = self.next_helper("cy");
        let cr = self.next_helper("cr");
        let c1 = self.next_helper("c1");
        let c2 = self.next_helper("c2");
        for v in [&cx, &cy, &cr, &c1, &c2] {
            let _ = self.mark_declared(v);
        }
        let x_expr = self.emit_expr_with_offset(&args[0], offset);
        let y_expr = self.emit_expr_with_offset(&args[1], offset);
        let r_expr = self.emit_expr_with_offset(&args[2], offset);
        [
            format!("{pad}var {cx}:Number = {x_expr};"),
            format!("{pad}var {cy}:Number = {y_expr};"),
            format!("{pad}var {cr}:Number = {r_expr};"),
            format!("{pad}var {c1}:Number = {cr} * 0.41421356;"),
            format!("{pad}var {c2}:Number = {cr} * 0.70710678;"),
            format!("{pad}{recv}.moveTo({cx} + {cr}, {cy});"),
            format!("{pad}{recv}.curveTo({cx} + {cr}, {cy} + {c1}, {cx} + {c2}, {cy} + {c2});"),
            format!("{pad}{recv}.curveTo({cx} + {c1}, {cy} + {cr}, {cx}, {cy} + {cr});"),
            format!("{pad}{recv}.curveTo({cx} - {c1}, {cy} + {cr}, {cx} - {c2}, {cy} + {c2});"),
            format!("{pad}{recv}.curveTo({cx} - {cr}, {cy} + {c1}, {cx} - {cr}, {cy});"),
            format!("{pad}{recv}.curveTo({cx} - {cr}, {cy} - {c1}, {cx} - {c2}, {cy} - {c2});"),
            format!("{pad}{recv}.curveTo({cx} - {c1}, {cy} - {cr}, {cx}, {cy} - {cr});"),
            format!("{pad}{recv}.curveTo({cx} + {c1}, {cy} - {cr}, {cx} + {c2}, {cy} - {c2});"),
            format!("{pad}{recv}.curveTo({cx} + {cr}, {cy} - {c1}, {cx} + {cr}, {cy});"),
        ]
        .join("\n")
    }

    fn emit_as2_event_listener(
        &mut self,
        recv_expr: &Expr,
        is_remove: bool,
        ev_arg: &Expr,
        handler_arg: &Expr,
        offset: usize,
    ) -> String {
        let pad = self.pad();
        let ev_kind = classify_as3_event_type(ev_arg);

        if is_remove {
            let recv = self.emit_expr_with_offset(recv_expr, offset);
            return match ev_kind.as_deref() {
                Some("enterFrame") => format!("{pad}delete {recv}.onEnterFrame;"),
                Some("click") => format!("{pad}delete {recv}.onRelease;"),
                Some("mouseDown") => format!("{pad}delete {recv}.onPress;"),
                _ => format!("{pad}/* removed event listener */"),
            };
        }

        let self_var = "__self";
        let self_decl = if self.mark_declared(self_var) {
            format!("{pad}var {self_var}:Object = this;\n")
        } else {
            String::new()
        };

        match ev_kind.as_deref() {
            Some("keyDown" | "keyUp") => {
                let is_down = ev_kind.as_deref() == Some("keyDown");
                let cb_prop = if is_down { "onKeyDown" } else { "onKeyUp" };
                let ev_str = if is_down { "keyDown" } else { "keyUp" };
                let listener_var = self.next_helper("keyListener");
                let _ = self.mark_declared(&listener_var);
                let body = self.emit_as2_listener_invocation(
                    handler_arg,
                    self_var,
                    &format!(
                        "{{type: \"{ev_str}\", keyCode: Key.getCode(), charCode: Key.getAscii()}}"
                    ),
                    true,
                    offset,
                );
                format!(
                    "{self_decl}{pad}var {listener_var}:Object = {{}};\n{pad}{listener_var}.{cb_prop} = function():Void {{\n{body}\n{pad}}};\n{pad}Key.addListener({listener_var});"
                )
            }
            Some("resize") => {
                let listener_var = self.next_helper("stageListener");
                let _ = self.mark_declared(&listener_var);
                let body = self.emit_as2_listener_invocation(
                    handler_arg,
                    self_var,
                    "{type: \"resize\", target: Stage}",
                    false,
                    offset,
                );
                format!(
                    "{self_decl}{pad}var {listener_var}:Object = {{}};\n{pad}{listener_var}.onResize = function():Void {{\n{body}\n{pad}}};\n{pad}Stage.addListener({listener_var});"
                )
            }
            Some(
                ev @ ("enterFrame" | "click" | "mouseDown" | "mouseUp" | "rollOver" | "rollOut"),
            ) => {
                let cb_prop = match ev {
                    "enterFrame" => "onEnterFrame",
                    "click" => "onRelease",
                    "mouseDown" => "onPress",
                    "mouseUp" => "onReleaseOutside",
                    "rollOver" => "onRollOver",
                    _ => "onRollOut",
                };
                let recv = if is_stage_expr(recv_expr) {
                    "this".to_owned()
                } else {
                    self.emit_expr_with_offset(recv_expr, offset)
                };
                let body = self.emit_as2_listener_invocation(
                    handler_arg,
                    self_var,
                    &format!("{{type: \"{ev}\", target: {recv}}}"),
                    false,
                    offset,
                );
                format!("{self_decl}{pad}{recv}.{cb_prop} = function():Void {{\n{body}\n{pad}}};")
            }
            _ => {
                // If listening on `loader.contentLoaderInfo`, attach callback onto the loader clip instead of running inline!
                if let Expr::Member(loader_base, prop) = recv_expr
                    && prop == "contentLoaderInfo"
                {
                    let loader_code = self.emit_expr_with_offset(loader_base, offset);
                    let body = self.emit_as2_listener_invocation(
                        handler_arg,
                        self_var,
                        "{type: \"complete\", target: this}",
                        false,
                        offset,
                    );
                    return format!(
                        "{self_decl}{pad}{loader_code}.__onLoadComplete = function():Void {{\n{body}\n{pad}}};"
                    );
                }
                let body = self.emit_as2_listener_invocation(
                    handler_arg,
                    self_var,
                    "{type: \"load\", target: this}",
                    false,
                    offset,
                );
                format!("{self_decl}{body}")
            }
        }
    }

    fn emit_as2_listener_invocation(
        &mut self,
        handler_arg: &Expr,
        self_var: &str,
        event_obj_code: &str,
        is_key: bool,
        offset: usize,
    ) -> String {
        if let Expr::Function(f) = handler_arg {
            let saved_kp = self.in_key_handler_param.clone();
            if is_key {
                self.in_key_handler_param = f.params.first().map(|(p, _)| p.clone());
            }
            self.push_fn_scope();
            if let Some((pname, _)) = f.params.first() {
                self.mark_declared(pname);
                self.env.define(pname.clone(), HirType::Object);
            }
            self.indent += 1;
            let pad_in = self.pad();
            let mut lines = Vec::new();
            if let Some((pname, _)) = f.params.first()
                && !is_key
            {
                lines.push(format!("{pad_in}var {pname}:Object = {event_obj_code};"));
            }
            let inner = self.emit_stmt_list(&f.body);
            if !inner.trim().is_empty() {
                lines.push(inner);
            }
            self.indent -= 1;
            self.pop_fn_scope();
            self.in_key_handler_param = saved_kp;
            lines.join("\n")
        } else {
            self.indent += 1;
            let pad_in = self.pad();
            let fn_code = self.emit_expr_with_offset(handler_arg, offset);
            self.indent -= 1;
            format!("{pad_in}{fn_code}.call({self_var}, {event_obj_code});")
        }
    }

    fn emit_expr_with_offset(&mut self, expr: &Expr, offset: usize) -> String {
        match expr {
            Expr::XmlSelect { .. }
            | Expr::XmlFilter(_, _, _)
            | Expr::XmlWithAttr { .. }
            | Expr::XmlLiteral(_, _) => {
                let raw = self.approx_source_expr(expr);
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    format!(
                        "E4X XML expression requires manual conversion to AS2 XMLNode DOM: {raw}"
                    ),
                );
                format!(
                    "/* FIXME [NeoFlash Convert]: E4X XML is not supported in AS2: {raw} */ undefined"
                )
            }
            Expr::Literal(raw) => raw.clone(),
            Expr::Name(name) => {
                if matches!(name.as_str(), "ByteArray" | "Stage3D" | "Context3D") {
                    self.record_issue(
                        ConversionIssueKind::ManualAttention,
                        offset,
                        format!("'{name}' has no native AVM1 equivalent in ActionScript 2.0."),
                    );
                    return format!(
                        "/* FIXME [NeoFlash Convert]: {name} is not supported in AS2 */ undefined"
                    );
                }
                if name == "stage" {
                    return "Stage".into();
                }
                if name == "root" {
                    return "_root".into();
                }
                if name == "numChildren" && self.env.lookup(name).is_none() {
                    return "this.getNextHighestDepth()".into();
                }
                if let Some(as2_prop) = map_modern_direct_prop_to_as2(name)
                    && self.env.lookup(name).is_none()
                {
                    return format!("this.{as2_prop}");
                }
                if let Some(as2_pct) = map_modern_unit_prop_to_as2(name)
                    && self.env.lookup(name).is_none()
                {
                    return format!("(this.{as2_pct} / 100)");
                }
                name.clone()
            }
            Expr::Member(base, prop) => {
                // `event.keyCode` / `event.charCode` inside key listener
                if let Some(kp) = &self.in_key_handler_param
                    && matches!(base.as_ref(), Expr::Name(n) if n == kp)
                {
                    if prop == "keyCode" {
                        return "Key.getCode()".into();
                    }
                    if prop == "charCode" {
                        return "Key.getAscii()".into();
                    }
                }
                // `Keyboard.LEFT` -> `Key.LEFT`
                if matches!(base.as_ref(), Expr::Name(n) if n == "Keyboard")
                    && let Some((as2_k, _)) = map_as3_keyboard_constant(prop)
                {
                    return format!("Key.{as2_k}");
                }
                // `StageScaleMode.*` / `StageAlign.*`
                if matches!(base.as_ref(), Expr::Name(n) if n == "StageScaleMode") {
                    return match prop.as_str() {
                        "NO_SCALE" => "\"noScale\"".into(),
                        "SHOW_ALL" => "\"showAll\"".into(),
                        "EXACT_FIT" => "\"exactFit\"".into(),
                        "NO_BORDER" => "\"noBorder\"".into(),
                        _ => "\"showAll\"".into(),
                    };
                }
                if matches!(base.as_ref(), Expr::Name(n) if n == "StageAlign") {
                    return match prop.as_str() {
                        "TOP_LEFT" => "\"TL\"".into(),
                        "TOP" => "\"T\"".into(),
                        "TOP_RIGHT" => "\"TR\"".into(),
                        "LEFT" => "\"L\"".into(),
                        "RIGHT" => "\"R\"".into(),
                        "BOTTOM_LEFT" => "\"BL\"".into(),
                        "BOTTOM" => "\"B\"".into(),
                        "BOTTOM_RIGHT" => "\"BR\"".into(),
                        _ => "\"\"".into(),
                    };
                }
                if matches!(base.as_ref(), Expr::Name(n) if n == "TextFieldType") {
                    return match prop.as_str() {
                        "INPUT" => "\"input\"".into(),
                        "DYNAMIC" => "\"dynamic\"".into(),
                        _ => "\"dynamic\"".into(),
                    };
                }
                if matches!(base.as_ref(), Expr::Name(n) if n == "GradientType") {
                    return match prop.as_str() {
                        "LINEAR" => "\"linear\"".into(),
                        "RADIAL" => "\"radial\"".into(),
                        _ => "\"linear\"".into(),
                    };
                }
                // `stage.stageWidth` / `stage.stageHeight` / `stage.scaleMode` / `stage.align`
                if is_stage_expr(base) {
                    if prop == "stage3Ds" {
                        self.record_issue(
                            ConversionIssueKind::ManualAttention,
                            offset,
                            "stage.stage3Ds (Stage3D) is not supported in ActionScript 2.0.",
                        );
                        return "/* FIXME [NeoFlash Convert]: stage.stage3Ds (Stage3D) is not supported in AS2 */ undefined".into();
                    }
                    return match prop.as_str() {
                        "stageWidth" => "Stage.width".into(),
                        "stageHeight" => "Stage.height".into(),
                        "scaleMode" => "Stage.scaleMode".into(),
                        "align" => "Stage.align".into(),
                        "displayState" => "Stage.displayState".into(),
                        other => format!("Stage.{other}"),
                    };
                }

                let recv = self.emit_expr_with_offset(base, offset);
                // Keep TextField.text, Array.length, Point/Rectangle.x/y/width/height intact!
                let base_ty = self.env.infer_expr(base, self.stage);
                if is_geom_or_data_value_type(&base_ty) {
                    return format!("{recv}.{prop}");
                }
                if matches!(
                    prop.as_str(),
                    "currentLabel" | "currentFrameLabel" | "currentLabels" | "isPlaying"
                ) {
                    self.record_issue(
                        ConversionIssueKind::ManualAttention,
                        offset,
                        format!(
                            "AS3 MovieClip.{prop} timeline metadata is not available in ActionScript 2.0; inspect _currentframe instead."
                        ),
                    );
                }
                if prop == "numChildren" {
                    return format!("{recv}.getNextHighestDepth()");
                }
                if let Some(as2_prop) = map_modern_direct_prop_to_as2(prop) {
                    return format!("{recv}.{as2_prop}");
                }
                if let Some(as2_pct) = map_modern_unit_prop_to_as2(prop) {
                    return format!("({recv}.{as2_pct} / 100)");
                }
                format!("{recv}.{prop}")
            }
            Expr::Index(base, idx) => {
                let b = self.emit_expr_with_offset(base, offset);
                let i = self.emit_expr_with_offset(idx, offset);
                format!("{b}[{i}]")
            }
            Expr::Array(items) => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|opt| {
                        opt.as_ref()
                            .map(|e| self.emit_expr_with_offset(e, offset))
                            .unwrap_or_default()
                    })
                    .collect();
                format!("[{}]", parts.join(", "))
            }
            Expr::Object(props) => {
                let parts: Vec<String> = props
                    .iter()
                    .map(|(PropertyKey { source, .. }, val)| {
                        format!("{source}: {}", self.emit_expr_with_offset(val, offset))
                    })
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
            Expr::Unary(op, inner) => {
                let mapped_op = if op == "not" { "!" } else { op.as_str() };
                let ic = self.emit_expr_with_offset(inner, offset);
                if matches!(mapped_op, "typeof" | "void") {
                    format!("{mapped_op} {ic}")
                } else if is_atomic_expr(inner) {
                    format!("{mapped_op}{ic}")
                } else {
                    format!("{mapped_op}({ic})")
                }
            }
            Expr::Update(op, inner, prefix) => {
                if let Some((recv, as2_pct)) = self.extract_unit_member(inner, offset) {
                    let step = if op == "++" { "100" } else { "-100" };
                    return format!("({recv}.{as2_pct} += {step})");
                }
                let ic = self.emit_assign_target(inner, offset);
                if *prefix {
                    format!("{op}{ic}")
                } else {
                    format!("{ic}{op}")
                }
            }
            Expr::Assign(op, lhs, rhs) => {
                if let Some((recv, as2_pct)) = self.extract_unit_member(lhs, offset) {
                    match op.as_str() {
                        "=" => {
                            if let Some(folded) = fold_01_to_100(rhs) {
                                return format!("{recv}.{as2_pct} = {folded}");
                            }
                            let rc = self.emit_expr_with_offset(rhs, offset);
                            return format!("{recv}.{as2_pct} = ({rc}) * 100");
                        }
                        "+=" | "-=" => {
                            if let Some(folded) = fold_01_to_100(rhs) {
                                return format!("{recv}.{as2_pct} {op} {folded}");
                            }
                            let rc = self.emit_expr_with_offset(rhs, offset);
                            return format!("{recv}.{as2_pct} {op} ({rc}) * 100");
                        }
                        _ => {
                            let rc = self.emit_expr_with_offset(rhs, offset);
                            return format!("{recv}.{as2_pct} {op} {rc}");
                        }
                    }
                }
                let lc = self.emit_assign_target(lhs, offset);
                let rc = self.emit_expr_with_offset(rhs, offset);
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
                if matches!(mapped_op, "==" | "!=") {
                    if matches!(rhs.as_ref(), Expr::Literal(l) if l == "null") {
                        let lc = self.emit_expr_with_offset(lhs, offset);
                        return format!("{lc} {mapped_op} undefined");
                    }
                    if matches!(lhs.as_ref(), Expr::Literal(l) if l == "null") {
                        let rc = self.emit_expr_with_offset(rhs, offset);
                        return format!("undefined {mapped_op} {rc}");
                    }
                }
                let lc = self.emit_sub_expr(lhs, mapped_op, false, offset);
                let rc = self.emit_sub_expr(rhs, mapped_op, true, offset);
                format!("{lc} {mapped_op} {rc}")
            }
            Expr::Conditional(c, y, n) => {
                let cc = self.emit_expr_with_offset(c, offset);
                let yc = self.emit_expr_with_offset(y, offset);
                let nc = self.emit_expr_with_offset(n, offset);
                format!("({cc} ? {yc} : {nc})")
            }
            Expr::InstanceOf(lhs, rhs) => {
                let l = self.emit_expr_with_offset(lhs, offset);
                let r = self.emit_expr_with_offset(rhs, offset);
                format!("({l} instanceof {r})")
            }
            Expr::TypeTest(op, lhs, ty_name, toff) => {
                let l = self.emit_expr_with_offset(lhs, *toff);
                let short = short_type_name(ty_name);
                if op == "as" {
                    return l;
                }
                match short {
                    "Number" | "int" | "uint" => format!("(typeof ({l}) == \"number\")"),
                    "String" => format!("(typeof ({l}) == \"string\")"),
                    "Boolean" => format!("(typeof ({l}) == \"boolean\")"),
                    "Function" => format!("(typeof ({l}) == \"function\")"),
                    _ => {
                        let mapped = self.map_type_to_as2(ty_name, false, *toff);
                        format!("({l} instanceof {mapped})")
                    }
                }
            }
            Expr::TypeValue(ty_name, toff) => self.map_type_to_as2(ty_name, false, *toff),
            Expr::Delete(inner, _) => {
                let ic = self.emit_expr_with_offset(inner, offset);
                format!("delete {ic}")
            }
            Expr::Function(f) => {
                let ret_as2 = f
                    .returns
                    .as_deref()
                    .map(|r| self.map_type_to_as2(r, true, f.offset))
                    .unwrap_or_else(|| "Void".into());
                self.push_fn_scope();
                let mut params_out = Vec::new();
                for (pname, pty) in &f.params {
                    let mapped = pty
                        .as_deref()
                        .map(|t| self.map_type_to_as2(t, false, f.offset))
                        .unwrap_or_else(|| "Object".into());
                    self.mark_declared(pname);
                    self.env
                        .define(pname.clone(), HirType::from_annotation(&mapped));
                    params_out.push(format!("{pname}:{mapped}"));
                }
                self.indent += 1;
                let body_str = self.emit_stmt_list(&f.body);
                self.indent -= 1;
                self.pop_fn_scope();
                let pad = self.pad();
                let fname = f.name.as_deref().unwrap_or("");
                format!(
                    "function {fname}({}):{ret_as2} {{\n{body_str}\n{pad}}}",
                    params_out.join(", ")
                )
            }
            Expr::NewVector(_, args, voff) => {
                self.record_issue(
                    ConversionIssueKind::Approximated,
                    *voff,
                    "Converted AS3 'new Vector.<T>(...)' to AS2 Array.",
                );
                if let Some(len_arg) = args.first() {
                    let lc = self.emit_expr_with_offset(len_arg, *voff);
                    format!("new Array({lc})")
                } else {
                    "[]".into()
                }
            }
            Expr::NewBuiltin(ty, args, noff)
            | Expr::NewClass(ty, args, noff)
            | Expr::NewValue(ty, args, noff)
            | Expr::NewError(ty, args, noff) => {
                let short = short_type_name(ty);
                if matches!(short, "ByteArray" | "Stage3D" | "Context3D") {
                    let raw = format!("new {short}({})", self.emit_args(args, *noff));
                    self.record_issue(
                        ConversionIssueKind::ManualAttention,
                        *noff,
                        format!("AS3 '{raw}' has no native AVM1 equivalent in ActionScript 2.0."),
                    );
                    return format!(
                        "/* FIXME [NeoFlash Convert]: {short} is not supported in AS2: {raw} */ undefined"
                    );
                }
                if short == "Dictionary" {
                    self.record_issue(
                        ConversionIssueKind::Approximated,
                        *noff,
                        "Converted 'new Dictionary()' to '{}'.",
                    );
                    return "{}".into();
                }
                if matches!(
                    short,
                    "Sprite" | "MovieClip" | "Shape" | "Bitmap" | "Loader"
                ) {
                    return "this.createEmptyMovieClip(\"_clip_\" + this.getNextHighestDepth(), this.getNextHighestDepth())".into();
                }
                if short == "TextField" {
                    return "this.createTextField(\"_tf_\" + this.getNextHighestDepth(), this.getNextHighestDepth(), 0, 0, 100, 100)".into();
                }
                if short == "Video" {
                    self.record_issue(
                        ConversionIssueKind::ManualAttention,
                        *noff,
                        "AS2 Video must be an authored Video timeline placement; lowered dynamic new Video() to a MovieClip placeholder.",
                    );
                    return "/* FIXME [NeoFlash Convert]: AS2 Video must be an authored Video placement */ this.createEmptyMovieClip(\"_vid_\" + this.getNextHighestDepth(), this.getNextHighestDepth())".into();
                }
                if short == "URLRequest" {
                    if let Some(first) = args.first() {
                        return self.emit_expr_with_offset(first, *noff);
                    }
                    return "\"\"".into();
                }
                if short == "Sound" {
                    return "new Sound(this)".into();
                }
                let mapped = self.map_type_to_as2(ty, false, *noff);
                let ac = self.emit_args(args, *noff);
                format!("new {mapped}({ac})")
            }
            Expr::RegExpLiteral(pat, flags) => {
                let p: String = serde_json::from_str(pat).unwrap_or_default();
                let fl: String = serde_json::from_str(flags).unwrap_or_default();
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    format!("AS3 RegExp literal /{p}/{fl} has no native AS2 RegExp engine."),
                );
                format!(
                    "/* FIXME [NeoFlash Convert]: RegExp literal /{p}/{fl} requires manual AS2 string handling */ undefined"
                )
            }
            Expr::Call(callee, args, call_off) => self.emit_call(callee, args, *call_off),
        }
    }

    fn emit_call(&mut self, callee: &Expr, args: &[Expr], offset: usize) -> String {
        // 1. `int(expr)` -> `((expr) | 0)` & `uint(expr)` -> `((expr) >>> 0)`
        if let Expr::Name(fn_name) = callee {
            if fn_name == "int" && args.len() == 1 {
                let ac = self.emit_expr_with_offset(&args[0], offset);
                return format!("(({ac}) | 0)");
            }
            if fn_name == "uint" && args.len() == 1 {
                let ac = self.emit_expr_with_offset(&args[0], offset);
                return format!("(({ac}) >>> 0)");
            }
            if fn_name == "MovieClip" && args.len() == 1 {
                // `MovieClip(root)` cast -> inner expression in AS2
                return self.emit_expr_with_offset(&args[0], offset);
            }
            if fn_name == "navigateToURL" && !args.is_empty() {
                let url_expr = match &args[0] {
                    Expr::NewBuiltin(t, uargs, _)
                    | Expr::NewClass(t, uargs, _)
                    | Expr::NewValue(t, uargs, _)
                        if short_type_name(t) == "URLRequest" && !uargs.is_empty() =>
                    {
                        self.emit_expr_with_offset(&uargs[0], offset)
                    }
                    other => self.emit_expr_with_offset(other, offset),
                };
                if let Some(win) = args.get(1) {
                    let wc = self.emit_expr_with_offset(win, offset);
                    return format!("getURL({url_expr}, {wc})");
                }
                return format!("getURL({url_expr})");
            }
        }

        // `Vector.<T>(expr)` conversion call -> `Array(expr)`
        if let Expr::TypeValue(ty_name, toff) = callee
            && ty_name.starts_with("Vector.<")
        {
            self.record_issue(
                ConversionIssueKind::Approximated,
                *toff,
                format!("Converted '{ty_name}(...)' cast to AS2 Array."),
            );
            if let Some(first) = args.first() {
                return self.emit_expr_with_offset(first, offset);
            }
            return "[]".into();
        }

        // 2. `mc.graphics.*` -> `mc.*`
        if let Some((clip_expr, g_method)) = match_graphics_method(callee) {
            let recv = self.emit_expr_with_offset(&clip_expr, offset);
            match g_method {
                "clear" | "endFill" => return format!("{recv}.{g_method}()"),
                "moveTo" | "lineTo" | "curveTo" => {
                    let ac = self.emit_args(args, offset);
                    return format!("{recv}.{g_method}({ac})");
                }
                "beginFill" => {
                    if args.is_empty() {
                        return format!("{recv}.beginFill(0)");
                    }
                    let col = self.emit_expr_with_offset(&args[0], offset);
                    if let Some(alpha_arg) = args.get(1) {
                        let a100 = fold_01_to_100(alpha_arg).unwrap_or_else(|| {
                            let raw = self.emit_expr_with_offset(alpha_arg, offset);
                            format!("({raw}) * 100")
                        });
                        return format!("{recv}.beginFill({col}, {a100})");
                    }
                    return format!("{recv}.beginFill({col})");
                }
                "beginGradientFill" => {
                    let converted: Vec<String> = args
                        .iter()
                        .take(5)
                        .enumerate()
                        .map(|(idx, arg)| {
                            if idx == 2
                                && let Expr::Array(items) = arg
                            {
                                let scaled: Vec<String> = items
                                    .iter()
                                    .map(|opt| match opt {
                                        Some(el) => fold_01_to_100(el).unwrap_or_else(|| {
                                            let ec = self.emit_expr_with_offset(el, offset);
                                            format!("({ec}) * 100")
                                        }),
                                        None => String::new(),
                                    })
                                    .collect();
                                return format!("[{}]", scaled.join(", "));
                            }
                            self.emit_expr_with_offset(arg, offset)
                        })
                        .collect();
                    return format!("{recv}.beginGradientFill({})", converted.join(", "));
                }
                "beginBitmapFill" => {
                    let ac = self.emit_args(args, offset);
                    return format!("{recv}.beginBitmapFill({ac})");
                }
                "lineStyle" => {
                    if args.is_empty() {
                        return format!("{recv}.lineStyle()");
                    }
                    let mut out_args = vec![self.emit_expr_with_offset(&args[0], offset)];
                    if let Some(col_arg) = args.get(1) {
                        out_args.push(self.emit_expr_with_offset(col_arg, offset));
                    }
                    if let Some(alpha_arg) = args.get(2) {
                        let a100 = fold_01_to_100(alpha_arg).unwrap_or_else(|| {
                            let raw = self.emit_expr_with_offset(alpha_arg, offset);
                            format!("({raw}) * 100")
                        });
                        out_args.push(a100);
                    }
                    return format!("{recv}.lineStyle({})", out_args.join(", "));
                }
                _ => {}
            }
        }

        // `getChildByName(n)` -> `this[n]` / `mc[n]`
        if let Expr::Member(base, method) = callee
            && method == "getChildByName"
            && let Some(arg0) = args.first()
        {
            let recv = self.emit_expr_with_offset(base, offset);
            let ac = self.emit_expr_with_offset(arg0, offset);
            return format!("{recv}[{ac}]");
        }
        if let Expr::Name(method) = callee
            && method == "getChildByName"
            && let Some(arg0) = args.first()
        {
            let ac = self.emit_expr_with_offset(arg0, offset);
            return format!("this[{ac}]");
        }

        // AS3 `hitTestObject`, `hitTestPoint`, 2-arg `startDrag(lock, boundsRect)`, `attachNetStream`, `Loader.load`, `Sound.load`
        if let Expr::Member(base, method) = callee {
            if matches!(method.as_str(), "hitTestObject" | "hitTestPoint") && !args.is_empty() {
                let recv = self.emit_expr_with_offset(base, offset);
                let ac = self.emit_args(args, offset);
                return format!("{recv}.hitTest({ac})");
            }
            if method == "startDrag" && args.len() == 2 {
                let recv = self.emit_expr_with_offset(base, offset);
                let drag_args = self.emit_as3_2arg_start_drag(args, offset);
                return format!("{recv}.startDrag({drag_args})");
            }
            if method == "attachNetStream" && args.len() == 1 {
                let recv = self.emit_expr_with_offset(base, offset);
                let ns = self.emit_expr_with_offset(&args[0], offset);
                return format!("{recv}.attachVideo({ns})");
            }
            if method == "load" && args.len() == 1 {
                let recv = self.emit_expr_with_offset(base, offset);
                let url = self.unwrap_url_request_expr(&args[0], offset);
                if matches!(
                    self.env.infer_expr(base, self.stage),
                    HirType::Class(ref c) if short_type_name(c) == "Sound"
                ) {
                    return format!("{recv}.loadSound({url}, false)");
                }
                if matches!(
                    self.env.infer_expr(base, self.stage),
                    HirType::MovieClip | HirType::Sprite
                ) {
                    return format!("{recv}.loadMovie({url})");
                }
            }
        }
        if let Expr::Name(fn_name) = callee
            && fn_name == "startDrag"
            && args.len() == 2
        {
            let drag_args = self.emit_as3_2arg_start_drag(args, offset);
            return format!("this.startDrag({drag_args})");
        }

        let c = self.emit_expr_with_offset(callee, offset);
        let a = self.emit_args(args, offset);
        format!("{c}({a})")
    }

    fn unwrap_url_request_expr(&mut self, expr: &Expr, offset: usize) -> String {
        if let Some(("URLRequest", uargs, _)) = match_new_display_ctor_with_args(expr)
            && let Some(first) = uargs.first()
        {
            return self.emit_expr_with_offset(first, offset);
        }
        self.emit_expr_with_offset(expr, offset)
    }

    fn emit_as3_2arg_start_drag(&mut self, args: &[Expr], offset: usize) -> String {
        let lock = self.emit_expr_with_offset(&args[0], offset);
        if let Some(("Rectangle", rargs, _)) = match_new_display_ctor_with_args(&args[1])
            && rargs.len() >= 4
        {
            let left = self.emit_expr_with_offset(&rargs[0], offset);
            let top = self.emit_expr_with_offset(&rargs[1], offset);
            let right = match (
                extract_numeric_literal(&rargs[0]),
                extract_numeric_literal(&rargs[2]),
            ) {
                (Some(x), Some(w)) => format_clean_number(x + w, false),
                _ => {
                    let w = self.emit_expr_with_offset(&rargs[2], offset);
                    format!("({left}) + ({w})")
                }
            };
            let bottom = match (
                extract_numeric_literal(&rargs[1]),
                extract_numeric_literal(&rargs[3]),
            ) {
                (Some(y), Some(h)) => format_clean_number(y + h, false),
                _ => {
                    let h = self.emit_expr_with_offset(&rargs[3], offset);
                    format!("({top}) + ({h})")
                }
            };
            return format!("{lock}, {left}, {top}, {right}, {bottom}");
        }
        let rect = self.emit_expr_with_offset(&args[1], offset);
        format!("{lock}, ({rect}).x, ({rect}).y, ({rect}).right, ({rect}).bottom")
    }

    fn extract_unit_member(
        &mut self,
        expr: &Expr,
        offset: usize,
    ) -> Option<(String, &'static str)> {
        match expr {
            Expr::Member(base, prop) => {
                let base_ty = self.env.infer_expr(base, self.stage);
                if is_geom_or_data_value_type(&base_ty) {
                    return None;
                }
                let as2_pct = map_modern_unit_prop_to_as2(prop)?;
                let recv = self.emit_expr_with_offset(base, offset);
                Some((recv, as2_pct))
            }
            Expr::Name(prop) if self.env.lookup(prop).is_none() => {
                let as2_pct = map_modern_unit_prop_to_as2(prop)?;
                Some(("this".into(), as2_pct))
            }
            _ => None,
        }
    }

    fn emit_assign_target(&mut self, expr: &Expr, offset: usize) -> String {
        match expr {
            Expr::Name(name) => {
                if let Some(as2_prop) = map_modern_direct_prop_to_as2(name)
                    && self.env.lookup(name).is_none()
                {
                    return format!("this.{as2_prop}");
                }
                name.clone()
            }
            Expr::Member(base, prop) => {
                if is_stage_expr(base) {
                    return match prop.as_str() {
                        "scaleMode" => "Stage.scaleMode".into(),
                        "align" => "Stage.align".into(),
                        "displayState" => "Stage.displayState".into(),
                        other => format!("Stage.{other}"),
                    };
                }
                let base_ty = self.env.infer_expr(base, self.stage);
                let recv = self.emit_expr_with_offset(base, offset);
                if is_geom_or_data_value_type(&base_ty) {
                    return format!("{recv}.{prop}");
                }
                if let Some(as2_prop) = map_modern_direct_prop_to_as2(prop) {
                    return format!("{recv}.{as2_prop}");
                }
                format!("{recv}.{prop}")
            }
            _ => self.emit_expr_with_offset(expr, offset),
        }
    }

    fn emit_args(&mut self, args: &[Expr], offset: usize) -> String {
        args.iter()
            .map(|a| self.emit_expr_with_offset(a, offset))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn emit_sub_expr(
        &mut self,
        expr: &Expr,
        parent_op: &str,
        is_right: bool,
        offset: usize,
    ) -> String {
        let code = self.emit_expr_with_offset(expr, offset);
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

    fn approx_source_expr(&self, expr: &Expr) -> String {
        match expr {
            Expr::Literal(s) | Expr::Name(s) => s.clone(),
            Expr::Member(b, p) => format!("{}.{p}", self.approx_source_expr(b)),
            Expr::XmlSelect {
                base,
                local,
                attribute,
                descendant,
                ..
            } => {
                let dots = if *descendant { ".." } else { "." };
                let at = if *attribute { "@" } else { "" };
                let loc = self.approx_source_expr(local).trim_matches('"').to_owned();
                format!("{}{dots}{at}{loc}", self.approx_source_expr(base))
            }
            Expr::XmlFilter(base, pred, _) => {
                format!(
                    "{}.({})",
                    self.approx_source_expr(base),
                    self.approx_source_expr(pred)
                )
            }
            Expr::XmlWithAttr {
                local, attribute, ..
            } => {
                let at = if *attribute { "@" } else { "" };
                format!("{at}{}", self.approx_source_expr(local).trim_matches('"'))
            }
            Expr::XmlLiteral(encoded, _) => {
                let clean: String = serde_json::from_str::<serde_json::Value>(encoded)
                    .ok()
                    .and_then(|v| v.get("source")?.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "<xml/>".into());
                clean.replace("*/", "* /")
            }
            _ => "expr".into(),
        }
    }

    fn format_imports(&self, body: &str) -> Vec<String> {
        self.imports
            .iter()
            .filter(|imp| {
                if imp.ends_with(".*") {
                    true
                } else {
                    body.contains(short_type_name(imp))
                }
            })
            .map(|imp| format!("import {imp};"))
            .collect()
    }
}

pub(crate) fn emit_as2_timeline_script(
    parsed: &ParsedScript,
    symbol_id: Option<&str>,
    location: &str,
    stage: &ProjectStageIndex,
) -> As2EmitOutput {
    let mut emitter = As2Emitter::new(
        location,
        &parsed.expanded_source,
        stage,
        symbol_id.map(str::to_owned),
        None,
    );
    emitter.preserve_as2_imports(&parsed.imports);
    emitter.precollect_key_listeners(&parsed.statements);

    let body = emitter.emit_stmt_list(&parsed.statements);
    let import_lines = emitter.format_imports(&body);

    let source = if import_lines.is_empty() {
        format!("{body}\n")
    } else if body.trim().is_empty() {
        format!("{}\n", import_lines.join("\n"))
    } else {
        format!("{}\n\n{body}\n", import_lines.join("\n"))
    };

    As2EmitOutput {
        source,
        normalized_path: None,
        issues: emitter.issues,
    }
}

pub(crate) fn emit_as2_class(
    parsed: &ParsedClass,
    location: &str,
    stage: &ProjectStageIndex,
) -> As2EmitOutput {
    let mut emitter = As2Emitter::new(
        location,
        &parsed.source,
        stage,
        None,
        Some(parsed.qualified_name.clone()),
    );
    emitter.preserve_as2_imports(&parsed.imports);

    for f in &parsed.fields {
        if !f.is_static {
            let ty =
                f.ty.as_deref()
                    .map_or(HirType::Object, HirType::from_annotation);
            emitter.env.define(f.name.clone(), ty);
        }
    }
    for m in &parsed.methods {
        for s in &m.function.body {
            emitter.scan_stmt_for_key_listeners(s);
        }
    }

    emitter.indent = 1;
    let mut member_blocks = Vec::new();
    let mut deferred_ctor_inits: Vec<(String, Expr, usize)> = Vec::new();

    for f in &parsed.fields {
        let pad = emitter.pad();
        let acc = f.access.as2_keyword();
        let stat = if f.is_static { "static " } else { "" };
        let ty =
            f.ty.as_deref()
                .map(|t| emitter.map_type_to_as2(t, false, f.offset))
                .unwrap_or_else(|| "Object".into());
        if let Some(val) = &f.value {
            if !f.is_static && !is_as2_scalar_field_init(val) {
                deferred_ctor_inits.push((f.name.clone(), val.clone(), f.offset));
                member_blocks.push(format!("{pad}{acc} {stat}var {}:{ty};", f.name));
            } else {
                let vc = emitter.emit_expr_with_offset(val, f.offset);
                member_blocks.push(format!("{pad}{acc} {stat}var {}:{ty} = {vc};", f.name));
            }
        } else {
            member_blocks.push(format!("{pad}{acc} {stat}var {}:{ty};", f.name));
        }
    }

    let has_explicit_ctor = parsed.methods.iter().any(|m| {
        m.accessor.is_none()
            && !m.is_static
            && m.function.name.as_deref() == Some(parsed.short_name.as_str())
    });

    if !parsed.is_interface && !has_explicit_ctor && !deferred_ctor_inits.is_empty() {
        let pad = emitter.pad();
        emitter.indent = 2;
        let in_pad = emitter.pad();
        let mut ctor_lines = Vec::new();
        if parsed.base_class.is_some() {
            ctor_lines.push(format!("{in_pad}super();"));
        }
        for (fname, val, foff) in &deferred_ctor_inits {
            let vc = emitter.emit_expr_with_offset(val, *foff);
            ctor_lines.push(format!("{in_pad}this.{fname} = {vc};"));
        }
        emitter.indent = 1;
        member_blocks.push(format!(
            "{pad}public function {}() {{\n{}\n{pad}}}",
            parsed.short_name,
            ctor_lines.join("\n")
        ));
    }

    for m in &parsed.methods {
        let pad = emitter.pad();
        let fname = m.function.name.as_deref().unwrap_or(&parsed.short_name);
        let is_ctor = fname == parsed.short_name && m.accessor.is_none() && !m.is_static;
        let acc = if parsed.is_interface {
            ""
        } else if is_ctor {
            "public "
        } else {
            match m.access.as2_keyword() {
                "private" => "private ",
                _ => "public ",
            }
        };
        let stat = if m.is_static && !parsed.is_interface {
            "static "
        } else {
            ""
        };
        let get_set = match m.accessor {
            Some(AccessorKind::Get) => "get ",
            Some(AccessorKind::Set) => "set ",
            None => "",
        };

        let is_key_fn = emitter.key_handler_fns.contains(fname);
        let saved_kp = emitter.in_key_handler_param.clone();
        if is_key_fn {
            emitter.in_key_handler_param = m.function.params.first().map(|(p, _)| p.clone());
        }

        emitter.push_fn_scope();
        let mut params_out = Vec::new();
        let mut default_inits = Vec::new();
        for (i, (pname, pty)) in m.function.params.iter().enumerate() {
            let mapped = pty
                .as_deref()
                .map(|t| emitter.map_type_to_as2(t, false, m.function.offset))
                .unwrap_or_else(|| "Object".into());
            emitter.mark_declared(pname);
            emitter
                .env
                .define(pname.clone(), HirType::from_annotation(&mapped));
            params_out.push(format!("{pname}:{mapped}"));
            if let Some(Some(def_expr)) = m.function.defaults.get(i) {
                let dc = emitter.emit_expr_with_offset(def_expr, m.function.offset);
                default_inits.push(format!("if ({pname} === undefined) {{ {pname} = {dc}; }}"));
            }
        }
        if let Some(rest) = &m.function.rest {
            emitter.mark_declared(rest);
            emitter.env.define(rest.clone(), HirType::Array);
            default_inits.push(format!(
                "var {rest}:Array = arguments.slice({});",
                m.function.params.len()
            ));
        }

        let ret_part = if is_ctor {
            String::new()
        } else {
            let rty = m
                .function
                .returns
                .as_deref()
                .map(|r| emitter.map_type_to_as2(r, true, m.function.offset))
                .unwrap_or_else(|| "Void".into());
            format!(":{rty}")
        };

        if parsed.is_interface {
            emitter.pop_fn_scope();
            emitter.in_key_handler_param = saved_kp;
            member_blocks.push(format!(
                "{pad}function {get_set}{fname}({}){ret_part};",
                params_out.join(", ")
            ));
        } else {
            emitter.indent = 2;
            let in_pad = emitter.pad();
            let mut body_parts: Vec<String> = Vec::new();
            if is_ctor && !deferred_ctor_inits.is_empty() {
                for (df_name, df_val, df_off) in &deferred_ctor_inits {
                    let vc = emitter.emit_expr_with_offset(df_val, *df_off);
                    body_parts.push(format!("{in_pad}this.{df_name} = {vc};"));
                }
            }
            for di in default_inits {
                body_parts.push(format!("{in_pad}{di}"));
            }
            let body_str = emitter.emit_stmt_list(&m.function.body);
            if !body_str.trim().is_empty() {
                body_parts.push(body_str);
            }
            emitter.indent = 1;
            emitter.pop_fn_scope();
            emitter.in_key_handler_param = saved_kp;
            member_blocks.push(format!(
                "{pad}{acc}{stat}function {get_set}{fname}({}){ret_part} {{\n{}\n{pad}}}",
                params_out.join(", "),
                body_parts.join("\n")
            ));
        }
    }

    let extends_clause = if let Some(base) = &parsed.base_class {
        let short_base = short_type_name(base);
        if matches!(short_base, "EventDispatcher" | "Object") {
            String::new()
        } else {
            let mapped = emitter.map_type_to_as2(base, false, 0);
            format!(" extends {mapped}")
        }
    } else if parsed.is_interface
        && let Some(first_iface) = parsed.interfaces.first()
    {
        let mapped = emitter.map_type_to_as2(first_iface, false, 0);
        format!(" extends {mapped}")
    } else {
        String::new()
    };

    let as2_ifaces: Vec<String> = if !parsed.is_interface {
        parsed
            .interfaces
            .iter()
            .filter(|i| {
                let s = short_type_name(i);
                stage.project_classes.contains_key(s)
            })
            .map(|i| emitter.map_type_to_as2(i, false, 0))
            .collect()
    } else {
        Vec::new()
    };
    let implements_clause = if as2_ifaces.is_empty() {
        String::new()
    } else {
        format!(" implements {}", as2_ifaces.join(", "))
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
        "{dyn_kw}{kind_kw} {}{extends_clause}{implements_clause} {{\n{}\n}}",
        parsed.qualified_name,
        member_blocks.join("\n\n")
    );

    let import_lines: Vec<String> = emitter
        .format_imports(&class_decl)
        .into_iter()
        .filter(|l| !l.ends_with(&format!(" {};", parsed.qualified_name)))
        .collect();

    let full_source = if import_lines.is_empty() {
        format!("{class_decl}\n")
    } else {
        format!("{}\n\n{class_decl}\n", import_lines.join("\n"))
    };

    let expected_path = format!("{}.as", parsed.qualified_name.replace('.', "/"));
    As2EmitOutput {
        source: full_source,
        normalized_path: Some(expected_path),
        issues: emitter.issues,
    }
}

fn is_as2_compatible_import(imp: &str, stage: &ProjectStageIndex) -> bool {
    if imp.starts_with("flash.geom.") && imp != "flash.geom.Orientation3D" {
        return true;
    }
    if imp.starts_with("flash.filters.")
        && !matches!(
            imp,
            "flash.filters.BitmapFilterQuality" | "flash.filters.DisplacementMapFilterMode"
        )
    {
        return true;
    }
    if matches!(
        imp,
        "flash.display.BitmapData"
            | "flash.external.ExternalInterface"
            | "flash.net.FileReference"
            | "flash.net.FileReferenceList"
    ) || imp.starts_with("mx.")
    {
        return true;
    }
    let short = short_type_name(imp);
    stage.project_classes.contains_key(short)
}

fn is_as2_scalar_field_init(expr: &Expr) -> bool {
    match expr {
        Expr::Literal(_) => true,
        Expr::Name(n) => matches!(
            n.as_str(),
            "true" | "false" | "null" | "undefined" | "NaN" | "Infinity"
        ),
        Expr::Unary(op, inner) if matches!(op.as_str(), "+" | "-") => {
            is_as2_scalar_field_init(inner)
        }
        _ => false,
    }
}

fn match_new_display_ctor_with_args(expr: &Expr) -> Option<(&str, &[Expr], usize)> {
    match expr {
        Expr::NewBuiltin(ty, args, off)
        | Expr::NewClass(ty, args, off)
        | Expr::NewValue(ty, args, off) => Some((short_type_name(ty), args.as_slice(), *off)),
        _ => None,
    }
}

fn match_add_child_call(callee: &Expr) -> Option<Expr> {
    match callee {
        Expr::Name(n) if n == "addChild" || n == "addChildAt" => Some(Expr::Name("this".into())),
        Expr::Member(base, m) if m == "addChild" || m == "addChildAt" => Some((**base).clone()),
        _ => None,
    }
}

fn match_remove_child_call(callee: &Expr) -> Option<Expr> {
    match callee {
        Expr::Name(n) if n == "removeChild" => Some(Expr::Name("this".into())),
        Expr::Member(base, m) if m == "removeChild" => Some((**base).clone()),
        _ => None,
    }
}

pub(crate) fn match_graphics_method(callee: &Expr) -> Option<(Expr, &str)> {
    if let Expr::Member(g_base, method) = callee {
        match g_base.as_ref() {
            Expr::Name(n) if n == "graphics" => {
                return Some((Expr::Name("this".into()), method.as_str()));
            }
            Expr::Member(clip_base, g_prop) if g_prop == "graphics" => {
                return Some(((**clip_base).clone(), method.as_str()));
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn match_event_listener_call(callee: &Expr) -> Option<(Expr, bool)> {
    match callee {
        Expr::Name(n) if n == "addEventListener" => Some((Expr::Name("this".into()), false)),
        Expr::Name(n) if n == "removeEventListener" => Some((Expr::Name("this".into()), true)),
        Expr::Member(base, m) if m == "addEventListener" => Some(((**base).clone(), false)),
        Expr::Member(base, m) if m == "removeEventListener" => Some(((**base).clone(), true)),
        _ => None,
    }
}

pub(crate) fn is_stage_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::Name(n) if n == "stage" || n == "Stage")
        || matches!(expr, Expr::Member(b, p) if matches!(b.as_ref(), Expr::Name(n) if n == "this") && p == "stage")
}

pub(crate) fn is_stage_non_null_check(cond: &Expr) -> bool {
    if let Expr::Binary(op, l, r) = cond
        && matches!(op.as_str(), "!=" | "!==")
    {
        let l_stage = is_stage_expr(l);
        let r_stage = is_stage_expr(r);
        let l_null = matches!(l.as_ref(), Expr::Literal(s) if s == "null")
            || matches!(l.as_ref(), Expr::Name(s) if s == "undefined");
        let r_null = matches!(r.as_ref(), Expr::Literal(s) if s == "null")
            || matches!(r.as_ref(), Expr::Name(s) if s == "undefined");
        return (l_stage && r_null) || (r_stage && l_null);
    }
    false
}

pub(crate) fn classify_as3_event_type(ev_arg: &Expr) -> Option<String> {
    if let Some(s) = super::hir::decode_string_literal(ev_arg) {
        return Some(s);
    }
    if let Expr::Member(base, prop) = ev_arg
        && let Expr::Name(cls) = base.as_ref()
    {
        return match (cls.as_str(), prop.as_str()) {
            ("Event", "ENTER_FRAME") => Some("enterFrame".into()),
            ("Event", "RESIZE") => Some("resize".into()),
            ("Event", "ADDED_TO_STAGE" | "ADDED" | "INIT") => Some("load".into()),
            ("Event", "COMPLETE") => Some("complete".into()),
            ("KeyboardEvent", "KEY_DOWN") => Some("keyDown".into()),
            ("KeyboardEvent", "KEY_UP") => Some("keyUp".into()),
            ("MouseEvent", "CLICK") => Some("click".into()),
            ("MouseEvent", "MOUSE_DOWN") => Some("mouseDown".into()),
            ("MouseEvent", "MOUSE_UP") => Some("mouseUp".into()),
            ("MouseEvent", "ROLL_OVER" | "MOUSE_OVER") => Some("rollOver".into()),
            ("MouseEvent", "ROLL_OUT" | "MOUSE_OUT") => Some("rollOut".into()),
            _ => None,
        };
    }
    None
}

fn is_keyboard_event_arg(ev_arg: &Expr) -> bool {
    matches!(
        classify_as3_event_type(ev_arg).as_deref(),
        Some("keyDown" | "keyUp")
    )
}

fn extract_fn_ident(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(n) => Some(n.clone()),
        Expr::Member(b, p) if matches!(b.as_ref(), Expr::Name(n) if n == "this") => Some(p.clone()),
        _ => None,
    }
}

fn is_geom_or_data_value_type(ty: &HirType) -> bool {
    match ty {
        HirType::Array | HirType::Vector(_) | HirType::String | HirType::Dictionary => true,
        HirType::Class(c) => matches!(
            short_type_name(c),
            "Point" | "Rectangle" | "Matrix" | "ColorTransform"
        ),
        _ => false,
    }
}
