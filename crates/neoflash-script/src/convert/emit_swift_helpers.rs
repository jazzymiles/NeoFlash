//! Helper passes for `emit_swift.rs`: Swift 6 literal normalization, chunked `NFValue`
//! builders, function/timeline `var` hoisting, linked `Symbol` Stage bindings, and
//! `NFDynamicObjectProtocol` reflection stubs (`T-499`).

use super::emit_swift::escape_swift_ident;
use super::hir::{HirType, ProjectStageIndex, sanitize_swift_type_ident, short_type_name};
use crate::actionscript::ast::{Case, Expr, Stmt};
use neoflash_core::{ElementKind, LayerRole, Project, Symbol, layer_effectively_visible};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// Convert an ActionScript / JS / TS literal token into a valid Swift 6 literal expression.
///
/// Normalizes single-quoted strings (`'abc'`), hex/unicode escapes (`\x1b`, `\u00a0`, `\v`, `\b`, `\f`),
/// accidental `\(...)` sequences inside string literals, and leading/trailing-dot floats (`.5`, `5.`).
pub(crate) fn format_swift_literal(raw: &str, as_nfvalue: bool) -> String {
    let trimmed = raw.trim();
    if trimmed == "null" {
        return if as_nfvalue {
            "NFValue.null".into()
        } else {
            "nil".into()
        };
    }
    if trimmed == "undefined" {
        return if as_nfvalue {
            "NFValue.undefinedVal".into()
        } else {
            "nil".into()
        };
    }
    if trimmed == "true" || trimmed == "false" {
        return if as_nfvalue {
            format!("NFValue.bool({trimmed})")
        } else {
            trimmed.to_owned()
        };
    }
    if trimmed == "NaN" {
        return if as_nfvalue {
            "NFValue.number(.nan)".into()
        } else {
            "Double.nan".into()
        };
    }
    if trimmed == "Infinity" {
        return if as_nfvalue {
            "NFValue.number(.infinity)".into()
        } else {
            "Double.infinity".into()
        };
    }

    // Quoted string literal (`"..."` or `'...'`)
    if (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
        || (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2)
    {
        let decoded = decode_script_string_literal(trimmed);
        let swift_lit = encode_swift_string_literal(&decoded);
        return if as_nfvalue {
            format!("NFValue.string({swift_lit})")
        } else {
            swift_lit
        };
    }

    // Hex integer literal (`0x...` / `0X...`)
    if let Some(hex_digits) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        let clean_hex: String = hex_digits.chars().filter(|c| *c != '_').collect();
        if let Ok(uval) = u64::from_str_radix(&clean_hex, 16) {
            if as_nfvalue {
                if uval <= i64::MAX as u64 {
                    return format!("NFValue.int({uval})");
                }
                return format!("NFValue.number({uval}.0)");
            }
            return format!("0x{clean_hex}");
        }
    }

    // Decimal int / float literal (normalize `.5` -> `0.5`, `5.` -> `5.0`, `.5e2` -> `0.5e2`)
    let normalized_num = normalize_swift_number_literal(trimmed);
    if as_nfvalue {
        if !normalized_num.contains('.')
            && !normalized_num.contains('e')
            && !normalized_num.contains('E')
            && let Ok(ival) = normalized_num.parse::<i64>()
        {
            return format!("NFValue.int({ival})");
        }
        return format!("NFValue.number({normalized_num})");
    }
    normalized_num
}

pub(crate) fn decode_script_string_literal(raw: &str) -> String {
    if raw.len() < 2 {
        return String::new();
    }
    let inner = &raw[1..raw.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let chars: Vec<char> = inner.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if ch != '\\' {
            out.push(ch);
            i += 1;
            continue;
        }
        i += 1;
        if i >= chars.len() {
            out.push('\\');
            break;
        }
        match chars[i] {
            'n' => {
                out.push('\n');
                i += 1;
            }
            'r' => {
                out.push('\r');
                i += 1;
            }
            't' => {
                out.push('\t');
                i += 1;
            }
            'b' => {
                out.push('\u{0008}');
                i += 1;
            }
            'f' => {
                out.push('\u{000C}');
                i += 1;
            }
            'v' => {
                out.push('\u{000B}');
                i += 1;
            }
            '0' => {
                out.push('\0');
                i += 1;
            }
            '\\' => {
                out.push('\\');
                i += 1;
            }
            '\'' => {
                out.push('\'');
                i += 1;
            }
            '"' => {
                out.push('"');
                i += 1;
            }
            'x' if i + 2 < chars.len() => {
                let hex: String = chars[i + 1..=i + 2].iter().collect();
                if let Ok(code) = u32::from_str_radix(&hex, 16)
                    && let Some(sc) = char::from_u32(code)
                {
                    out.push(sc);
                    i += 3;
                } else {
                    out.push('x');
                    i += 1;
                }
            }
            'u' => {
                if i + 1 < chars.len() && chars[i + 1] == '{' {
                    let mut end = i + 2;
                    while end < chars.len() && chars[end] != '}' {
                        end += 1;
                    }
                    if end < chars.len() {
                        let hex: String = chars[i + 2..end].iter().collect();
                        if let Ok(code) = u32::from_str_radix(&hex, 16)
                            && let Some(sc) = char::from_u32(code)
                        {
                            out.push(sc);
                            i = end + 1;
                            continue;
                        }
                    }
                } else if i + 4 < chars.len() {
                    let hex: String = chars[i + 1..=i + 4].iter().collect();
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        // Handle UTF-16 surrogate pairs (`\uD83D\uDE80`)
                        if (0xD800..=0xDBFF).contains(&code)
                            && i + 10 < chars.len()
                            && chars[i + 5] == '\\'
                            && chars[i + 6] == 'u'
                        {
                            let low_hex: String = chars[i + 7..=i + 10].iter().collect();
                            if let Ok(low) = u32::from_str_radix(&low_hex, 16)
                                && (0xDC00..=0xDFFF).contains(&low)
                            {
                                let combined = 0x10000 + (((code - 0xD800) << 10) | (low - 0xDC00));
                                if let Some(sc) = char::from_u32(combined) {
                                    out.push(sc);
                                    i += 11;
                                    continue;
                                }
                            }
                        }
                        if let Some(sc) = char::from_u32(code) {
                            out.push(sc);
                            i += 5;
                            continue;
                        }
                    }
                }
                out.push('u');
                i += 1;
            }
            '\n' => {
                // Line continuation
                i += 1;
            }
            '\r' => {
                i += 1;
                if i < chars.len() && chars[i] == '\n' {
                    i += 1;
                }
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    out
}

pub(crate) fn encode_swift_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

pub(crate) fn normalize_swift_number_literal(raw: &str) -> String {
    let clean: String = raw.chars().filter(|c| *c != '_').collect();
    if clean.is_empty() {
        return "0".into();
    }
    let (sign, rest) = if let Some(r) = clean.strip_prefix('-') {
        ("-", r)
    } else if let Some(r) = clean.strip_prefix('+') {
        ("", r)
    } else {
        ("", clean.as_str())
    };

    let (mantissa, exp_suffix) = if let Some(pos) = rest.find(['e', 'E']) {
        (&rest[..pos], &rest[pos..])
    } else {
        (rest, "")
    };

    let fixed_mantissa = if let Some(after_dot) = mantissa.strip_prefix('.') {
        if after_dot.is_empty() {
            "0.0".to_owned()
        } else {
            format!("0.{after_dot}")
        }
    } else if let Some(before_dot) = mantissa.strip_suffix('.') {
        if before_dot.is_empty() {
            "0.0".to_owned()
        } else {
            format!("{before_dot}.0")
        }
    } else {
        mantissa.to_owned()
    };

    format!("{sign}{fixed_mantissa}{exp_suffix}")
}

/// Hoisted variable declaration discovered inside a function or frame script body.
#[derive(Clone, Debug)]
pub(crate) struct HoistedVarSpec {
    pub(crate) name: String,
    pub(crate) ty: Option<String>,
    pub(crate) first_init: Option<Expr>,
    pub(crate) extra_decls: Vec<(Option<String>, Option<Expr>)>,
    pub(crate) declared_in_nested_block: bool,
    pub(crate) needs_function_hoist: bool,
}

/// Returns true if any top-level `return <expr>` in `stmts` (ignoring nested function bodies)
/// explicitly returns `null` or `undefined`.
pub(crate) fn stmts_return_null(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_returns_null)
}

fn stmt_returns_null(stmt: &Stmt) -> bool {
    match stmt.unlocated() {
        Stmt::Located(_, inner) | Stmt::Label(_, inner) | Stmt::IfFrameLoaded(_, inner) => {
            stmt_returns_null(inner)
        }
        Stmt::Return(Some(expr), _) => {
            matches!(expr, Expr::Literal(s) if s == "null" || s == "undefined")
                || matches!(expr, Expr::Name(s) if s == "null" || s == "undefined")
        }
        Stmt::Block(b) | Stmt::InitClip(_, b, _) => stmts_return_null(b),
        Stmt::If(_, y, n) => {
            stmt_returns_null(y) || n.as_ref().is_some_and(|no| stmt_returns_null(no))
        }
        Stmt::While(_, b) | Stmt::DoWhile(b, _) | Stmt::ForIn(_, _, _, b) => stmt_returns_null(b),
        Stmt::For(_, _, _, b) => stmt_returns_null(b),
        Stmt::Switch(_, cases) => cases.iter().any(|c| stmts_return_null(&c.body)),
        Stmt::Try(b, catches, fin) => {
            stmts_return_null(b)
                || catches.iter().any(|c| stmts_return_null(&c.body))
                || fin.as_ref().is_some_and(|f| stmts_return_null(f))
        }
        Stmt::With(_, b, _) | Stmt::TellTarget(_, b, _) => stmt_returns_null(b),
        _ => false,
    }
}

/// Collect dynamic `this.<prop>` member accesses/assignments in timeline or object scripts
/// that are not already declared properties, stage children, or builtin `NFMovieClip` members.
pub(crate) fn collect_dynamic_this_props(stmts: &[Stmt], known: &BTreeSet<String>) -> Vec<String> {
    let mut found = BTreeSet::new();
    let mut order = Vec::new();
    walk_stmts_for_this_props(stmts, known, &mut found, &mut order);
    order
}

fn walk_stmts_for_this_props(
    stmts: &[Stmt],
    known: &BTreeSet<String>,
    found: &mut BTreeSet<String>,
    order: &mut Vec<String>,
) {
    for s in stmts {
        walk_stmt_for_this_props(s, known, found, order);
    }
}

fn walk_stmt_for_this_props(
    stmt: &Stmt,
    known: &BTreeSet<String>,
    found: &mut BTreeSet<String>,
    order: &mut Vec<String>,
) {
    match stmt.unlocated() {
        Stmt::Located(_, inner) | Stmt::Label(_, inner) | Stmt::IfFrameLoaded(_, inner) => {
            walk_stmt_for_this_props(inner, known, found, order);
        }
        Stmt::Block(b) | Stmt::InitClip(_, b, _) => {
            walk_stmts_for_this_props(b, known, found, order);
        }
        Stmt::Var(vars) => {
            for (_, _, val_opt) in vars {
                if let Some(v) = val_opt {
                    walk_expr_for_this_props(v, known, found, order);
                }
            }
        }
        Stmt::Function(f) | Stmt::LegacyEvent(_, f) => {
            walk_stmts_for_this_props(&f.body, known, found, order);
        }
        Stmt::Expr(e) | Stmt::Throw(e) => walk_expr_for_this_props(e, known, found, order),
        Stmt::Return(Some(e), _) => walk_expr_for_this_props(e, known, found, order),
        Stmt::If(c, y, n) => {
            walk_expr_for_this_props(c, known, found, order);
            walk_stmt_for_this_props(y, known, found, order);
            if let Some(no) = n {
                walk_stmt_for_this_props(no, known, found, order);
            }
        }
        Stmt::While(c, b) | Stmt::DoWhile(b, c) => {
            walk_expr_for_this_props(c, known, found, order);
            walk_stmt_for_this_props(b, known, found, order);
        }
        Stmt::For(i, c, u, b) => {
            walk_stmt_for_this_props(i, known, found, order);
            if let Some(ce) = c {
                walk_expr_for_this_props(ce, known, found, order);
            }
            if let Some(ue) = u {
                walk_expr_for_this_props(ue, known, found, order);
            }
            walk_stmt_for_this_props(b, known, found, order);
        }
        Stmt::ForIn(i, coll, _, b) => {
            walk_stmt_for_this_props(i, known, found, order);
            walk_expr_for_this_props(coll, known, found, order);
            walk_stmt_for_this_props(b, known, found, order);
        }
        Stmt::Switch(d, cases) => {
            walk_expr_for_this_props(d, known, found, order);
            for c in cases {
                if let Some(t) = &c.test {
                    walk_expr_for_this_props(t, known, found, order);
                }
                walk_stmts_for_this_props(&c.body, known, found, order);
            }
        }
        Stmt::Try(b, catches, fin) => {
            walk_stmts_for_this_props(b, known, found, order);
            for c in catches {
                walk_stmts_for_this_props(&c.body, known, found, order);
            }
            if let Some(f) = fin {
                walk_stmts_for_this_props(f, known, found, order);
            }
        }
        Stmt::With(t, b, _) | Stmt::TellTarget(t, b, _) => {
            walk_expr_for_this_props(t, known, found, order);
            walk_stmt_for_this_props(b, known, found, order);
        }
        _ => {}
    }
}

fn walk_expr_for_this_props(
    expr: &Expr,
    known: &BTreeSet<String>,
    found: &mut BTreeSet<String>,
    order: &mut Vec<String>,
) {
    if let Expr::Member(base, prop) = expr
        && matches!(base.as_ref(), Expr::Name(n) if n == "this")
        && is_candidate_dynamic_this_prop(prop, known)
        && found.insert(prop.clone())
    {
        order.push(prop.clone());
    }
    if let Expr::Function(f) = expr {
        walk_stmts_for_this_props(&f.body, known, found, order);
        return;
    }
    for child in crate::actionscript::scope::children(expr) {
        walk_expr_for_this_props(child, known, found, order);
    }
}

fn is_candidate_dynamic_this_prop(prop: &str, known: &BTreeSet<String>) -> bool {
    if prop.is_empty() || known.contains(prop) || !is_candidate_implicit_var(prop, known) {
        return false;
    }
    !matches!(
        prop,
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
            | "onLoad"
            | "onChanged"
            | "onSetFocus"
            | "onKillFocus"
            | "stop"
            | "play"
            | "nextFrame"
            | "prevFrame"
            | "gotoAndPlay"
            | "gotoAndStop"
            | "addChild"
            | "addChildAt"
            | "removeChild"
            | "removeChildAt"
            | "removeChildren"
            | "getChildAt"
            | "getChildByName"
            | "getChildIndex"
            | "setChildIndex"
            | "swapChildren"
            | "swapChildrenAt"
            | "contains"
            | "createEmptyMovieClip"
            | "createTextField"
            | "attachBitmap"
            | "attachMovie"
            | "duplicateMovieClip"
            | "removeMovieClip"
            | "swapDepths"
            | "getNextHighestDepth"
            | "getDepth"
            | "getInstanceAtDepth"
            | "startDrag"
            | "stopDrag"
            | "hitTest"
            | "hitTestObject"
            | "hitTestPoint"
            | "getBounds"
            | "getRect"
            | "localToGlobal"
            | "globalToLocal"
            | "addEventListener"
            | "removeEventListener"
            | "dispatchEvent"
            | "hasEventListener"
            | "willTrigger"
            | "clear"
            | "beginFill"
            | "beginGradientFill"
            | "beginBitmapFill"
            | "lineStyle"
            | "lineGradientStyle"
            | "moveTo"
            | "lineTo"
            | "curveTo"
            | "endFill"
            | "stopAllSounds"
            | "getVolume"
            | "getPan"
            | "setVolume"
            | "setPan"
            | "attachSound"
            | "loadSound"
            | "getBytesLoaded"
            | "getBytesTotal"
            | "mask"
            | "blendMode"
            | "filters"
            | "transform"
            | "colorTransform"
            | "scrollRect"
            | "scale9Grid"
            | "buttonMode"
            | "mouseEnabled"
            | "mouseChildren"
            | "enabled"
            | "currentFrame"
            | "totalFrames"
            | "_currentframe"
            | "_totalframes"
            | "currentLabel"
            | "currentFrameLabel"
            | "currentLabels"
            | "framesLoaded"
            | "numChildren"
            | "loaderInfo"
    )
}

/// Scan `stmts` (stopping at nested function boundaries for `var` declarations, while tracking
/// lexical references inside nested closures) to find variables that genuinely escape their
/// declaring block (`needs_function_hoist == true`) or implicit undeclared AS1/AS2 assignments!
pub(crate) fn collect_hoisted_vars(
    stmts: &[Stmt],
    collect_implicit_as2: bool,
    known_globals_and_props: &BTreeSet<String>,
) -> Vec<HoistedVarSpec> {
    let mut ctx = HoistScanCtx {
        collect_implicit_as2,
        known: known_globals_and_props,
        next_scope_id: 1,
        scope_stack: vec![0],
        by_name: BTreeMap::new(),
        order: Vec::new(),
        decl_scopes: BTreeMap::new(),
        same_scope_redeclared: BTreeSet::new(),
        implicit_as2: BTreeSet::new(),
        ref_ancestors: BTreeMap::new(),
    };
    ctx.walk_stmts(stmts, false);

    let HoistScanCtx {
        mut by_name,
        order,
        decl_scopes,
        same_scope_redeclared,
        implicit_as2,
        ref_ancestors,
        ..
    } = ctx;

    for (name, spec) in &mut by_name {
        let dscopes = decl_scopes.get(name);
        let escapes_decl_block = ref_ancestors.get(name).is_some_and(|refs| {
            refs.iter()
                .any(|anc| !anc.iter().any(|s| dscopes.is_some_and(|ds| ds.contains(s))))
        });
        let root_redeclared = same_scope_redeclared.contains(name)
            && dscopes.is_some_and(|ds| ds.contains(&0))
            && !spec.extra_decls.is_empty();
        spec.needs_function_hoist = implicit_as2.contains(name)
            || (spec.declared_in_nested_block && escapes_decl_block)
            || root_redeclared;
    }

    order
        .into_iter()
        .filter_map(|n| by_name.remove(&n))
        .collect()
}

struct HoistScanCtx<'a> {
    collect_implicit_as2: bool,
    known: &'a BTreeSet<String>,
    next_scope_id: usize,
    scope_stack: Vec<usize>,
    by_name: BTreeMap<String, HoistedVarSpec>,
    order: Vec<String>,
    decl_scopes: BTreeMap<String, BTreeSet<usize>>,
    same_scope_redeclared: BTreeSet<String>,
    implicit_as2: BTreeSet<String>,
    ref_ancestors: BTreeMap<String, Vec<Vec<usize>>>,
}

impl<'a> HoistScanCtx<'a> {
    fn push_block_scope(&mut self) {
        let id = self.next_scope_id;
        self.next_scope_id += 1;
        self.scope_stack.push(id);
    }

    fn pop_block_scope(&mut self) {
        self.scope_stack.pop();
    }

    fn current_scope(&self) -> usize {
        *self.scope_stack.last().unwrap_or(&0)
    }

    fn mark_decl_in_current_scope(&mut self, name: &str) {
        let cur = self.current_scope();
        self.decl_scopes
            .entry(name.to_owned())
            .or_default()
            .insert(cur);
    }

    fn record_ref(&mut self, name: &str) {
        self.ref_ancestors
            .entry(name.to_owned())
            .or_default()
            .push(self.scope_stack.clone());
    }

    fn walk_stmts(&mut self, stmts: &[Stmt], in_nested_closure: bool) {
        for stmt in stmts {
            self.walk_stmt(stmt, in_nested_closure);
        }
    }

    fn walk_scoped_stmt(&mut self, stmt: &Stmt, in_nested_closure: bool) {
        self.push_block_scope();
        match stmt.unlocated() {
            Stmt::Block(b) | Stmt::InitClip(_, b, _) => self.walk_stmts(b, in_nested_closure),
            other => self.walk_stmt(other, in_nested_closure),
        }
        self.pop_block_scope();
    }

    fn walk_stmt(&mut self, stmt: &Stmt, in_nested_closure: bool) {
        match stmt.unlocated() {
            Stmt::Located(_, inner) => self.walk_stmt(inner, in_nested_closure),
            Stmt::Empty | Stmt::Break(_) | Stmt::Continue(_) | Stmt::DefaultXmlNamespace(_) => {}
            Stmt::Function(f) | Stmt::LegacyEvent(_, f) => {
                // Local function declarations have their own `var` hoisting pass, but their bodies
                // can still reference outer variables from our `scope_stack`!
                self.push_block_scope();
                for (p, _) in &f.params {
                    self.mark_decl_in_current_scope(p);
                }
                if let Some(r) = &f.rest {
                    self.mark_decl_in_current_scope(r);
                }
                self.walk_stmts(&f.body, true);
                self.pop_block_scope();
            }
            Stmt::Var(vars) => {
                let cur_scope = self.current_scope();
                let nested = cur_scope != 0;
                for (vname, vty, val_opt) in vars {
                    if let Some(v) = val_opt {
                        self.walk_expr(v, in_nested_closure);
                    }
                    let inserted = self
                        .decl_scopes
                        .entry(vname.clone())
                        .or_default()
                        .insert(cur_scope);
                    if !in_nested_closure {
                        if !inserted {
                            self.same_scope_redeclared.insert(vname.clone());
                        }
                        if let Some(existing) = self.by_name.get_mut(vname) {
                            existing.extra_decls.push((vty.clone(), val_opt.clone()));
                            if nested {
                                existing.declared_in_nested_block = true;
                            }
                        } else {
                            self.order.push(vname.clone());
                            self.by_name.insert(
                                vname.clone(),
                                HoistedVarSpec {
                                    name: vname.clone(),
                                    ty: vty.clone(),
                                    first_init: val_opt.clone(),
                                    extra_decls: Vec::new(),
                                    declared_in_nested_block: nested,
                                    needs_function_hoist: false,
                                },
                            );
                        }
                    }
                }
            }
            Stmt::Block(b) | Stmt::InitClip(_, b, _) => {
                self.push_block_scope();
                self.walk_stmts(b, in_nested_closure);
                self.pop_block_scope();
            }
            Stmt::If(c, y, n) => {
                self.walk_expr(c, in_nested_closure);
                self.walk_scoped_stmt(y, in_nested_closure);
                if let Some(no_s) = n {
                    self.walk_scoped_stmt(no_s, in_nested_closure);
                }
            }
            Stmt::While(c, b) => {
                self.walk_expr(c, in_nested_closure);
                self.walk_scoped_stmt(b, in_nested_closure);
            }
            Stmt::DoWhile(b, c) => {
                self.walk_scoped_stmt(b, in_nested_closure);
                self.walk_expr(c, in_nested_closure);
            }
            Stmt::For(init, cond, upd, body) => {
                self.push_block_scope();
                self.walk_stmt(init, in_nested_closure);
                if let Some(c) = cond {
                    self.walk_expr(c, in_nested_closure);
                }
                if let Some(u) = upd {
                    self.walk_expr(u, in_nested_closure);
                }
                self.walk_scoped_stmt(body, in_nested_closure);
                self.pop_block_scope();
            }
            Stmt::ForIn(init, coll, _, body) => {
                self.walk_expr(coll, in_nested_closure);
                self.push_block_scope();
                match init.unlocated() {
                    Stmt::Var(vars) => {
                        for (vname, _, _) in vars {
                            self.mark_decl_in_current_scope(vname);
                        }
                    }
                    other => self.walk_stmt(other, in_nested_closure),
                }
                self.walk_scoped_stmt(body, in_nested_closure);
                self.pop_block_scope();
            }
            Stmt::Switch(disc, cases) => {
                self.walk_expr(disc, in_nested_closure);
                for Case { test, body } in cases {
                    if let Some(t) = test {
                        self.walk_expr(t, in_nested_closure);
                    }
                    self.push_block_scope();
                    self.walk_stmts(body, in_nested_closure);
                    self.pop_block_scope();
                }
            }
            Stmt::Try(b, catches, fin) => {
                self.push_block_scope();
                self.walk_stmts(b, in_nested_closure);
                self.pop_block_scope();
                for c in catches {
                    self.push_block_scope();
                    self.mark_decl_in_current_scope(&c.name);
                    self.walk_stmts(&c.body, in_nested_closure);
                    self.pop_block_scope();
                }
                if let Some(f) = fin {
                    self.push_block_scope();
                    self.walk_stmts(f, in_nested_closure);
                    self.pop_block_scope();
                }
            }
            Stmt::Label(_, inner) | Stmt::IfFrameLoaded(_, inner) => {
                self.walk_stmt(inner, in_nested_closure);
            }
            Stmt::With(t, body, _) | Stmt::TellTarget(t, body, _) => {
                self.walk_expr(t, in_nested_closure);
                self.walk_scoped_stmt(body, in_nested_closure);
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.walk_expr(e, in_nested_closure),
            Stmt::Return(Some(e), _) => self.walk_expr(e, in_nested_closure),
            Stmt::Return(None, _) => {}
        }
    }

    fn walk_expr(&mut self, expr: &Expr, in_nested_closure: bool) {
        if let Expr::Name(n) = expr {
            self.record_ref(n);
            return;
        }
        if let Expr::Function(f) = expr {
            self.push_block_scope();
            for (p, _) in &f.params {
                self.mark_decl_in_current_scope(p);
            }
            if let Some(r) = &f.rest {
                self.mark_decl_in_current_scope(r);
            }
            self.walk_stmts(&f.body, true);
            self.pop_block_scope();
            return;
        }
        if !in_nested_closure
            && self.collect_implicit_as2
            && let Expr::Assign(op, lhs, rhs) = expr
            && let Expr::Name(vname) = lhs.as_ref()
            && is_candidate_implicit_var(vname, self.known)
            && !self.by_name.contains_key(vname)
        {
            self.implicit_as2.insert(vname.clone());
            self.order.push(vname.clone());
            self.by_name.insert(
                vname.clone(),
                HoistedVarSpec {
                    name: vname.clone(),
                    ty: None,
                    first_init: (op == "=").then(|| (**rhs).clone()),
                    extra_decls: Vec::new(),
                    declared_in_nested_block: true,
                    needs_function_hoist: true,
                },
            );
        }
        for child in crate::actionscript::scope::children(expr) {
            self.walk_expr(child, in_nested_closure);
        }
    }
}

fn is_candidate_implicit_var(name: &str, known: &BTreeSet<String>) -> bool {
    if known.contains(name) {
        return false;
    }
    !matches!(
        name,
        "this"
            | "super"
            | "_root"
            | "_parent"
            | "_global"
            | "_level0"
            | "root"
            | "parent"
            | "stage"
            | "graphics"
            | "x"
            | "y"
            | "_x"
            | "_y"
            | "scaleX"
            | "scaleY"
            | "_xscale"
            | "_yscale"
            | "alpha"
            | "_alpha"
            | "visible"
            | "_visible"
            | "rotation"
            | "_rotation"
            | "width"
            | "height"
            | "_width"
            | "_height"
            | "name"
            | "_name"
            | "blendMode"
            | "filters"
            | "enabled"
            | "buttonMode"
            | "useHandCursor"
            | "mouseEnabled"
            | "mouseChildren"
            | "tabEnabled"
            | "tabChildren"
            | "tabIndex"
            | "cacheAsBitmap"
            | "opaqueBackground"
            | "scrollRect"
            | "scale9Grid"
            | "mask"
            | "onEnterFrame"
            | "onRelease"
            | "onPress"
            | "onReleaseOutside"
            | "onRollOver"
            | "onRollOut"
            | "onMouseDown"
            | "onMouseUp"
            | "onMouseMove"
            | "onKeyDown"
            | "onKeyUp"
            | "onResize"
            | "onLoad"
            | "onChanged"
    )
}

/// Preallocated Stage child binding computed for a `Symbol` linked to a source class (`document_class` / `class_name`).
#[derive(Clone, Debug)]
pub(crate) struct LinkedStageBinding {
    pub(crate) raw_name: String,
    pub(crate) swift_prop_name: String,
    pub(crate) swift_type: String,
    pub(crate) hir_type: HirType,
    pub(crate) init_expr: String,
}

pub(crate) fn compute_linked_stage_bindings(
    _project: &Project,
    symbol: &Symbol,
    symbol_swift_class_names: &BTreeMap<String, String>,
) -> Vec<LinkedStageBinding> {
    let reserved: BTreeSet<&str> = BTreeSet::from([
        "name",
        "parent",
        "mask",
        "x",
        "y",
        "scaleX",
        "scaleY",
        "rotation",
        "skew",
        "alpha",
        "visible",
        "width",
        "height",
        "mouseX",
        "mouseY",
        "_name",
        "_x",
        "_y",
        "_xscale",
        "_yscale",
        "_rotation",
        "_alpha",
        "_visible",
        "_width",
        "_height",
        "_xmouse",
        "_ymouse",
        "_parent",
        "_root",
        "_currentframe",
        "_totalframes",
        "isScriptControlled",
        "isPlaybackScriptControlled",
        "colorTransform",
        "blendMode",
        "filters",
        "stage",
        "root",
        "children",
        "graphics",
        "buttonMode",
        "mouseEnabled",
        "mouseChildren",
        "dynamicProps",
        "numChildren",
        "currentFrame",
        "totalFrames",
        "isPlaying",
        "symbolKind",
        "graphicPlayback",
        "frameLabels",
        "scenes",
        "stageMetadata",
        "host",
        "timelineSpec",
        "currentLabel",
        "currentFrameLabel",
        "currentLabels",
        "framesLoaded",
        "trackAsMenu",
        "scrollRect",
        "scale9Grid",
        "opaqueBackground",
        "transform",
        "cacheAsBitmap",
        "useHandCursor",
        "tabEnabled",
        "tabChildren",
        "tabIndex",
        "doubleClickEnabled",
        "loaderInfo",
        "enabled",
        "buttonState",
        "inputState",
        "stageMouseX",
        "stageMouseY",
        "self",
        "super",
    ]);

    let mut by_raw_name: BTreeMap<String, (String, HirType, String)> = BTreeMap::new();
    for layer in &symbol.timeline.layers {
        if !layer_effectively_visible(&symbol.timeline, layer)
            || matches!(
                layer.properties.role,
                LayerRole::Folder | LayerRole::Guide | LayerRole::Mask
            )
        {
            continue;
        }
        for kf in &layer.keyframes {
            for el in &kf.elements {
                let raw = el.name.trim();
                if raw.is_empty() {
                    continue;
                }
                let raw_lit = encode_swift_string_literal(raw);
                let (sw_ty, hir_ty, init_expr) = match &el.kind {
                    ElementKind::Rectangle { .. }
                    | ElementKind::Ellipse { .. }
                    | ElementKind::Path { .. }
                    | ElementKind::PaintedPath { .. } => (
                        "NFShape".to_owned(),
                        HirType::Class("Shape".into()),
                        format!("NFShape(name: {raw_lit})"),
                    ),
                    ElementKind::Text { .. } | ElementKind::TextBox { .. } => (
                        "NFTextField".to_owned(),
                        HirType::TextField,
                        format!("NFTextField(name: {raw_lit})"),
                    ),
                    ElementKind::Bitmap { .. } => (
                        "NFBitmap".to_owned(),
                        HirType::Class("Bitmap".into()),
                        format!(
                            "NFBitmap(name: {raw_lit}, spec: NFBitmapSpec(resourceName: \"\", width: 0, height: 0))"
                        ),
                    ),
                    ElementKind::Video { .. } => (
                        "NFVideo".to_owned(),
                        HirType::Class("Video".into()),
                        format!(
                            "NFVideo(name: {raw_lit}, spec: NFVideoSpec(source: \"\", width: 0, height: 0))"
                        ),
                    ),
                    ElementKind::Instance { symbol_id, .. } => {
                        if let Some(cls) = symbol_swift_class_names.get(symbol_id) {
                            (
                                cls.clone(),
                                HirType::Class(cls.clone()),
                                format!("{cls}(name: {raw_lit})"),
                            )
                        } else {
                            (
                                "NFMovieClip".to_owned(),
                                HirType::MovieClip,
                                format!("NFMovieClip(name: {raw_lit})"),
                            )
                        }
                    }
                };

                by_raw_name
                    .entry(raw.to_owned())
                    .and_modify(|existing| {
                        if existing.0 != sw_ty {
                            existing.0 = "NFDisplayObject".to_owned();
                            existing.1 = HirType::Class("DisplayObject".into());
                            existing.2 = format!("NFDisplayObject(name: {raw_lit})");
                        }
                    })
                    .or_insert((sw_ty, hir_ty, init_expr));
            }
        }
    }

    let mut used_props: BTreeSet<String> = reserved.iter().map(|s| (*s).to_owned()).collect();
    let mut out = Vec::new();
    for (raw_name, (swift_type, hir_type, init_expr)) in by_raw_name {
        let prop_name = sanitize_stage_prop_ident(&raw_name);
        if !used_props.insert(prop_name.clone()) {
            continue;
        }
        out.push(LinkedStageBinding {
            raw_name,
            swift_prop_name: prop_name,
            swift_type,
            hir_type,
            init_expr,
        });
    }
    out
}

fn sanitize_stage_prop_ident(raw: &str) -> String {
    let mut cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    cleaned = cleaned.trim_matches('_').to_owned();
    if cleaned.is_empty() {
        return "instance".into();
    }
    if cleaned.as_bytes()[0].is_ascii_digit() {
        cleaned.insert(0, '_');
    }
    escape_swift_ident(&cleaned)
}

/// Iteratively flatten chained `a + b + c + ...` expressions into an ordered operand slice.
pub(crate) fn flatten_plus_chain<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
    let mut stack = vec![expr];
    while let Some(cur) = stack.pop() {
        if let Expr::Binary(op, lhs, rhs) = cur
            && (op == "+" || op == "add")
        {
            stack.push(rhs);
            stack.push(lhs);
        } else {
            out.push(cur);
        }
    }
}

/// Returns true if `base_cls` (or any transitive superclass in `stage.class_bases`) inherits from `NFEventDispatcher`.
pub(crate) fn inherits_nf_event_dispatcher(
    base_cls: Option<&str>,
    stage: &ProjectStageIndex,
) -> bool {
    let mut cur = base_cls;
    let mut visited = BTreeSet::new();
    while let Some(b) = cur {
        if !visited.insert(b.to_owned()) {
            break;
        }
        let short = short_type_name(b);
        if matches!(
            short,
            "EventDispatcher"
                | "DisplayObject"
                | "InteractiveObject"
                | "DisplayObjectContainer"
                | "Sprite"
                | "MovieClip"
                | "SimpleButton"
                | "Button"
                | "Shape"
                | "Bitmap"
                | "Video"
                | "TextField"
                | "Sound"
                | "Timer"
                | "URLLoader"
                | "Loader"
                | "NetConnection"
                | "NetStream"
        ) {
            return true;
        }
        cur = stage
            .class_bases
            .get(b)
            .or_else(|| {
                stage
                    .project_classes
                    .get(short)
                    .and_then(|q| stage.class_bases.get(q))
            })
            .map(String::as_str);
    }
    false
}

/// Compute collision-free Swift class identifiers across all `ParsedClass` units, disambiguating
/// same-short-name classes across distinct packages (`a.Counter` vs `b.Counter` -> `ACounter`, `BCounter`).
pub(crate) fn assign_swift_class_names<T>(units: &[(T, super::hir::ParsedClass)]) -> Vec<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (_, p) in units {
        *counts
            .entry(sanitize_swift_type_ident(&p.short_name))
            .or_insert(0) += 1;
    }
    let mut used = BTreeSet::new();
    let mut names = Vec::with_capacity(units.len());
    for (_, p) in units {
        let base_short = sanitize_swift_type_ident(&p.short_name);
        let preferred =
            if counts.get(&base_short).copied().unwrap_or(0) > 1 && !p.package.trim().is_empty() {
                format!("{}{}", sanitize_swift_type_ident(&p.package), base_short)
            } else {
                base_short
            };
        let mut cand = preferred.clone();
        let mut idx = 2usize;
        while !used.insert(cand.clone()) {
            cand = format!("{preferred}_{idx}");
            idx += 1;
        }
        names.push(cand);
    }
    names
}
