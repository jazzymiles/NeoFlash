//! AVM2 (SWF 9/10 ActionScript 3) class parser, lexical scope analyzer, and bytecode emitter.

use super::*;

#[derive(Default)]
pub(super) struct AbcPoolBuilder {
    pub(super) ints: Vec<i32>,
    pub(super) int_map: HashMap<i32, u32>,
    pub(super) uints: Vec<u32>,
    pub(super) doubles: Vec<f64>,
    pub(super) strings: Vec<String>,
    pub(super) string_map: HashMap<String, u32>,
    pub(super) namespaces: Vec<(u8, u32)>,
    pub(super) ns_map: HashMap<(u8, u32), u32>,
    pub(super) ns_sets: Vec<Vec<u32>>,
    pub(super) multinames: Vec<(u8, u32, u32)>,
    pub(super) mn_map: HashMap<(u8, u32, u32), u32>,
    pub(super) multiname_l: Option<u32>,
    pub(super) multiname_la: Option<u32>,
}

impl AbcPoolBuilder {
    pub(super) fn add_int(&mut self, value: i32) -> u32 {
        if let Some(&index) = self.int_map.get(&value) {
            return index;
        }
        let index = (self.ints.len() + 1) as u32;
        self.ints.push(value);
        self.int_map.insert(value, index);
        index
    }

    pub(super) fn add_double(&mut self, value: f64) -> u32 {
        let bits = value.to_bits();
        if let Some(pos) = self.doubles.iter().position(|d| d.to_bits() == bits) {
            return (pos + 1) as u32;
        }
        let index = (self.doubles.len() + 1) as u32;
        self.doubles.push(value);
        index
    }

    pub(super) fn add_string(&mut self, s: &str) -> u32 {
        if let Some(&idx) = self.string_map.get(s) {
            return idx;
        }
        let idx = (self.strings.len() + 1) as u32;
        self.strings.push(s.to_owned());
        self.string_map.insert(s.to_owned(), idx);
        idx
    }

    pub(super) fn add_namespace(&mut self, kind: u8, name: &str) -> u32 {
        let s_idx = self.add_string(name);
        if let Some(&idx) = self.ns_map.get(&(kind, s_idx)) {
            return idx;
        }
        let idx = (self.namespaces.len() + 1) as u32;
        self.namespaces.push((kind, s_idx));
        self.ns_map.insert((kind, s_idx), idx);
        idx
    }

    pub(super) fn add_qname(&mut self, pkg: &str, name: &str) -> u32 {
        let (ns_idx, name_idx) = if pkg.is_empty() && name == "*" {
            (0, 0)
        } else {
            (self.add_namespace(0x16, pkg), self.add_string(name))
        };
        let key = (0x07, ns_idx, name_idx); // 0x07 = QName
        if let Some(&idx) = self.mn_map.get(&key) {
            return idx;
        }
        let idx = (self.multinames.len() + 1) as u32;
        self.multinames.push(key);
        self.mn_map.insert(key, idx);
        idx
    }

    pub(super) fn add_ns_qname(&mut self, ns_kind: u8, ns_name: &str, name: &str) -> u32 {
        let ns_idx = self.add_namespace(ns_kind, ns_name);
        let name_idx = self.add_string(name);
        let key = (0x07, ns_idx, name_idx);
        if let Some(&idx) = self.mn_map.get(&key) {
            return idx;
        }
        let idx = (self.multinames.len() + 1) as u32;
        self.multinames.push(key);
        self.mn_map.insert(key, idx);
        idx
    }

    pub(super) fn add_qname_attr(&mut self, pkg: &str, name: &str) -> u32 {
        let (ns_idx, name_idx) = if pkg.is_empty() && name == "*" {
            (0, 0)
        } else {
            (self.add_namespace(0x16, pkg), self.add_string(name))
        };
        let key = (0x0D, ns_idx, name_idx); // 0x0D = CONSTANT_QNameA
        if let Some(&idx) = self.mn_map.get(&key) {
            return idx;
        }
        let idx = (self.multinames.len() + 1) as u32;
        self.multinames.push(key);
        self.mn_map.insert(key, idx);
        idx
    }

    pub(super) fn add_ns_set(&mut self, ns_indices: Vec<u32>) -> u32 {
        if let Some(pos) = self.ns_sets.iter().position(|s| *s == ns_indices) {
            return (pos + 1) as u32;
        }
        let idx = (self.ns_sets.len() + 1) as u32;
        self.ns_sets.push(ns_indices);
        idx
    }

    pub(super) fn add_multiname(&mut self, name: &str, ns_set_idx: u32) -> u32 {
        let name_idx = if name == "*" {
            0
        } else {
            self.add_string(name)
        };
        let key = (0x09, name_idx, ns_set_idx); // 0x09 = CONSTANT_Multiname (name, ns_set)
        if let Some(&idx) = self.mn_map.get(&key) {
            return idx;
        }
        let idx = (self.multinames.len() + 1) as u32;
        self.multinames.push(key);
        self.mn_map.insert(key, idx);
        idx
    }

    pub(super) fn add_multiname_attr(&mut self, name: &str, ns_set_idx: u32) -> u32 {
        let name_idx = if name == "*" {
            0
        } else {
            self.add_string(name)
        };
        let key = (0x0E, name_idx, ns_set_idx); // 0x0E = CONSTANT_MultinameA (name, ns_set)
        if let Some(&idx) = self.mn_map.get(&key) {
            return idx;
        }
        let idx = (self.multinames.len() + 1) as u32;
        self.multinames.push(key);
        self.mn_map.insert(key, idx);
        idx
    }

    pub(super) fn add_interface_multiname(&mut self, pkg: &str, name: &str) -> u32 {
        let ns_idx = self.add_namespace(0x16, pkg);
        let ns_set_idx = self.add_ns_set(vec![ns_idx]);
        self.add_multiname(name, ns_set_idx)
    }

    pub(super) fn intern_multiname_l(&mut self) -> u32 {
        if let Some(idx) = self.multiname_l {
            return idx;
        }
        let pub_ns = self.add_namespace(0x16, "");
        let ns_set_idx = self.add_ns_set(vec![pub_ns]);
        let idx = (self.multinames.len() + 1) as u32;
        self.multinames.push((0x1B, ns_set_idx, 0)); // 0x1B = CONSTANT_MultinameL
        self.multiname_l = Some(idx);
        idx
    }

    pub(super) fn intern_multiname_la(&mut self) -> u32 {
        if let Some(idx) = self.multiname_la {
            return idx;
        }
        let pub_ns = self.add_namespace(0x16, "");
        let ns_set_idx = self.add_ns_set(vec![pub_ns]);
        let idx = (self.multinames.len() + 1) as u32;
        self.multinames.push((0x1C, ns_set_idx, 0)); // 0x1C = CONSTANT_MultinameLA
        self.multiname_la = Some(idx);
        idx
    }
}

pub(super) fn emit_avm2_int(pool: &mut AbcPoolBuilder, code: &mut SwfBitWriter, value: i32) {
    if let Ok(short) = i16::try_from(value) {
        code.u8(0x25); // pushshort; the operand is interpreted through its low signed 16 bits
        code.u32_v(u32::from(short as u16));
    } else {
        code.u8(0x2D); // pushint
        code.u32_v(pool.add_int(value));
    }
}

/// Adobe's verifier needs an instruction boundary marked before a backward jump.
pub(super) fn emit_avm2_loop_label(code: &mut SwfBitWriter) -> usize {
    let target = code.buf.len();
    code.u8(0x09);
    target
}

pub(super) fn emit_avm2_s24_placeholder(code: &mut SwfBitWriter, op: u8) -> usize {
    code.u8(op);
    let pos = code.buf.len();
    code.u8(0);
    code.u8(0);
    code.u8(0);
    pos
}

pub(super) fn patch_avm2_s24(
    code: &mut SwfBitWriter,
    s24_pos: usize,
    target_pos: usize,
) -> Result<(), String> {
    let after_insn = s24_pos + 3;
    if target_pos < after_insn && code.buf.get(target_pos) != Some(&0x09) {
        return Err("AVM2 backward branch does not target a label".to_owned());
    }
    let rel = (target_pos as isize) - (after_insn as isize);
    let offset = i32::try_from(rel)
        .ok()
        .filter(|v| (-0x80_0000..=0x7F_FFFF).contains(v))
        .ok_or_else(|| "AVM2 branch offset exceeds 24-bit signed range".to_owned())?;
    let bytes = offset.to_le_bytes();
    code.buf[s24_pos] = bytes[0];
    code.buf[s24_pos + 1] = bytes[1];
    code.buf[s24_pos + 2] = bytes[2];
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum AbcOptDefault {
    Undefined,
    Null,
    Bool(bool),
    Int(i32),
    Float(f64),
    Str(String),
    ConstRef {
        raw: String,
        param_name: String,
        line: usize,
        col: usize,
    },
}

impl AbcOptDefault {
    pub(super) fn encode(&self, pool: &mut AbcPoolBuilder) -> (u32, u8) {
        match self {
            Self::Undefined => (0, 0x00),
            Self::Null => (0x0C, 0x0C),
            Self::Bool(true) => (0x0B, 0x0B),
            Self::Bool(false) => (0x0A, 0x0A),
            Self::Int(v) => (pool.add_int(*v), 0x03),
            Self::Float(v) => (pool.add_double(*v), 0x06),
            Self::Str(s) => (pool.add_string(s), 0x01),
            Self::ConstRef { .. } => (0, 0x00),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct AbcMethodMeta {
    pub(super) is_private: bool,
    pub(super) is_override: bool,
    pub(super) min_arity: usize,
    pub(super) max_arity: Option<usize>,
    pub(super) rest_param: Option<String>,
    pub(super) opt_defaults: Vec<AbcOptDefault>,
}

#[derive(Clone, Default)]
pub(super) struct AbcClassSpec {
    pub(super) pkg: String,
    pub(super) class_name: String,
    pub(super) is_interface: bool,
    pub(super) super_pkg: String,
    pub(super) super_name: String,
    pub(super) interfaces: Vec<(String, String)>,
    pub(super) interface_methods: Vec<(String, Vec<String>, AbcMethodMeta)>,
    pub(super) method_meta: HashMap<String, AbcMethodMeta>,
    pub(super) ctor_rest_param: Option<String>,
    pub(super) ctor_opt_defaults: Vec<AbcOptDefault>,
    pub(super) static_method_meta: HashMap<String, AbcMethodMeta>,
    pub(super) imported_classes: HashMap<String, String>,
    pub(super) wildcard_packages: Vec<String>,
    pub(super) ctor_params: Vec<String>,
    pub(super) has_explicit_ctor: bool,
    pub(super) has_explicit_super: bool,
    pub(super) ctor_statements: Vec<SwfStmt>,
    pub(super) instance_fields: Vec<(String, Option<SwfExpr>)>,
    pub(super) static_fields: Vec<(String, Option<SwfExpr>)>,
    pub(super) static_consts: Vec<(String, bool, Option<SwfExpr>)>,
    pub(super) static_methods: Vec<(String, Vec<String>, Vec<SwfStmt>)>,
    pub(super) frame_scripts: Vec<(u32, String)>,
    pub(super) extra_methods: Vec<(String, Vec<String>, Vec<SwfStmt>)>,
}

pub(super) fn split_qualified_class(qname: &str) -> (String, String) {
    match qname.rsplit_once('.') {
        Some((pkg, cls)) => (pkg.to_owned(), cls.to_owned()),
        None => (String::new(), qname.to_owned()),
    }
}

pub(super) fn parse_dotted_name(parser: &mut SwfParser) -> Result<String, String> {
    let mut name = parser.expect_ident()?;
    while parser.eat(&SwfToken::Dot) {
        let next = parser.expect_ident()?;
        name.push('.');
        name.push_str(&next);
    }
    Ok(name)
}

fn is_super_ctor_stmt(stmt: &SwfStmt) -> bool {
    matches!(
        stmt,
        SwfStmt::Expr(SwfExpr::Call { callee, .. })
            if matches!(callee.as_ref(), SwfExpr::Ident(id) if id == "super")
    )
}

pub(super) fn stmts_contain_super_ctor(stmts: &[SwfStmt]) -> bool {
    stmts.iter().any(|s| match s {
        s if is_super_ctor_stmt(s) => true,
        SwfStmt::Block(inner) => stmts_contain_super_ctor(inner),
        SwfStmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmts_contain_super_ctor(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|eb| stmts_contain_super_ctor(eb))
        }
        SwfStmt::Try {
            try_body,
            catches,
            finally_body,
        } => {
            stmts_contain_super_ctor(try_body)
                || catches
                    .iter()
                    .any(|(_, _, cb)| stmts_contain_super_ctor(cb))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| stmts_contain_super_ctor(fb))
        }
        _ => false,
    })
}

pub(super) fn build_instance_field_assigns(cls: &AbcClassSpec) -> Vec<SwfStmt> {
    cls.instance_fields
        .iter()
        .filter_map(|(fname, init_opt)| {
            init_opt.as_ref().map(|init_expr| SwfStmt::Assign {
                target: SwfLValue::Member(
                    Box::new(SwfExpr::Ident("this".to_owned())),
                    fname.clone(),
                ),
                op: SwfAssignOp::Assign,
                value: init_expr.clone(),
            })
        })
        .collect()
}

pub(super) fn build_effective_ctor_stmts(cls: &AbcClassSpec) -> Vec<SwfStmt> {
    if cls.has_explicit_super {
        return cls.ctor_statements.clone();
    }
    let field_assigns = build_instance_field_assigns(cls);
    if field_assigns.is_empty() {
        return cls.ctor_statements.clone();
    }
    let mut out = Vec::with_capacity(field_assigns.len() + cls.ctor_statements.len());
    out.extend(field_assigns);
    out.extend_from_slice(&cls.ctor_statements);
    out
}

pub(super) fn extract_script_imports(
    source: &str,
    imported_classes: &mut HashMap<String, String>,
    wildcard_packages: &mut Vec<String>,
) {
    let Ok(tokens) = tokenize_swf_script(source) else {
        return;
    };
    let mut parser = SwfParser::new(tokens, true);
    while parser.pos < parser.tokens.len() {
        if parser.peek() == Some(&SwfToken::Ident("import".to_owned())) {
            if consume_import_directive(&mut parser, imported_classes, wildcard_packages).is_err() {
                break;
            }
        } else {
            parser.pos += 1;
        }
    }
}

fn consume_import_directive(
    parser: &mut SwfParser,
    imported_classes: &mut HashMap<String, String>,
    wildcard_packages: &mut Vec<String>,
) -> Result<(), String> {
    parser.pos += 1; // 'import'
    let mut parts = vec![parser.expect_ident()?];
    let mut is_wildcard = false;
    while parser.eat(&SwfToken::Dot) {
        if parser.eat(&SwfToken::Star) {
            is_wildcard = true;
            break;
        }
        parts.push(parser.expect_ident()?);
    }
    parser.consume_optional_semi();
    if is_wildcard {
        let pkg = parts.join(".");
        if !pkg.is_empty() && !wildcard_packages.contains(&pkg) {
            wildcard_packages.push(pkg);
        }
    } else if parts.len() >= 2 {
        let cls = parts.pop().unwrap_or_default();
        let pkg = parts.join(".");
        imported_classes.insert(cls, pkg);
    }
    Ok(())
}

fn consume_metadata_brackets(parser: &mut SwfParser) -> Result<(), String> {
    let mut depth = 1usize;
    while depth > 0 {
        match parser.next() {
            Some(SwfToken::LBracket) => depth += 1,
            Some(SwfToken::RBracket) => depth -= 1,
            Some(_) => {}
            None => return Err("unterminated metadata tag in AS3 source".to_owned()),
        }
    }
    Ok(())
}

fn resolve_parsed_as3_type_ref(
    raw: &str,
    current_pkg: &str,
    imported_classes: &HashMap<String, String>,
) -> (String, String) {
    if raw.contains('.') {
        let (p, c) = split_qualified_class(raw);
        return (p, c);
    }
    if let Some(imp_pkg) = imported_classes.get(raw) {
        return (imp_pkg.clone(), raw.to_owned());
    }
    if matches!(
        raw,
        "MovieClip"
            | "Sprite"
            | "SimpleButton"
            | "Shape"
            | "DisplayObject"
            | "DisplayObjectContainer"
            | "InteractiveObject"
            | "Bitmap"
            | "IBitmapDrawable"
    ) {
        return ("flash.display".to_owned(), raw.to_owned());
    }
    if matches!(raw, "EventDispatcher" | "IEventDispatcher") {
        return ("flash.events".to_owned(), raw.to_owned());
    }
    if matches!(raw, "IDataInput" | "IDataOutput" | "IExternalizable") {
        return ("flash.utils".to_owned(), raw.to_owned());
    }
    if matches!(
        raw,
        "Object"
            | "Array"
            | "Error"
            | "TypeError"
            | "RangeError"
            | "ReferenceError"
            | "ArgumentError"
            | "VerifyError"
            | "SyntaxError"
            | "URIError"
            | "EvalError"
            | "SecurityError"
    ) {
        return (String::new(), raw.to_owned());
    }
    (current_pkg.to_owned(), raw.to_owned())
}

fn expr_to_abc_opt_default(expr: &SwfExpr, pname: &str) -> Result<AbcOptDefault, String> {
    match expr {
        SwfExpr::Undefined => Ok(AbcOptDefault::Undefined),
        SwfExpr::Null => Ok(AbcOptDefault::Null),
        SwfExpr::Bool(b) => Ok(AbcOptDefault::Bool(*b)),
        SwfExpr::Int(i) => Ok(AbcOptDefault::Int(*i)),
        SwfExpr::Float(f) => Ok(AbcOptDefault::Float(*f)),
        SwfExpr::Str(s) => Ok(AbcOptDefault::Str(s.clone())),
        SwfExpr::Ident(id) if id == "NaN" => Ok(AbcOptDefault::Float(f64::NAN)),
        SwfExpr::Ident(id) if id == "Infinity" => Ok(AbcOptDefault::Float(f64::INFINITY)),
        SwfExpr::Unary {
            op: SwfUnaryOp::Neg,
            expr: inner,
        } => match inner.as_ref() {
            SwfExpr::Int(i) => Ok(i.checked_neg().map_or_else(
                || AbcOptDefault::Float(-(f64::from(*i))),
                AbcOptDefault::Int,
            )),
            SwfExpr::Float(f) => Ok(AbcOptDefault::Float(-*f)),
            SwfExpr::Ident(id) if id == "Infinity" => Ok(AbcOptDefault::Float(f64::NEG_INFINITY)),
            _ => Err(format!(
                "ActionScript 3 optional parameter '{pname}' initializer must be a compile-time constant"
            )),
        },
        SwfExpr::Unary {
            op: SwfUnaryOp::Pos,
            expr: inner,
        } => match inner.as_ref() {
            SwfExpr::Int(i) => Ok(AbcOptDefault::Int(*i)),
            SwfExpr::Float(f) => Ok(AbcOptDefault::Float(*f)),
            _ => Err(format!(
                "ActionScript 3 optional parameter '{pname}' initializer must be a compile-time constant"
            )),
        },
        _ => Err(format!(
            "ActionScript 3 optional parameter '{pname}' initializer must be a compile-time constant"
        )),
    }
}

fn swf_dotted_const_chain(expr: &SwfExpr) -> Option<String> {
    match expr {
        SwfExpr::Ident(id) if !matches!(id.as_str(), "this" | "super" | "arguments") => {
            Some(id.clone())
        }
        SwfExpr::Member(inner, prop) => {
            let mut prefix = swf_dotted_const_chain(inner)?;
            prefix.push('.');
            prefix.push_str(prop);
            Some(prefix)
        }
        _ => None,
    }
}

fn builtin_abc_opt_default(raw: &str) -> Option<AbcOptDefault> {
    match raw {
        "NaN" | "Number.NaN" => Some(AbcOptDefault::Float(f64::NAN)),
        "Infinity" | "Number.POSITIVE_INFINITY" => Some(AbcOptDefault::Float(f64::INFINITY)),
        "Number.NEGATIVE_INFINITY" => Some(AbcOptDefault::Float(f64::NEG_INFINITY)),
        "int.MAX_VALUE" => Some(AbcOptDefault::Int(i32::MAX)),
        "int.MIN_VALUE" => Some(AbcOptDefault::Int(i32::MIN)),
        "uint.MAX_VALUE" => Some(AbcOptDefault::Float(f64::from(u32::MAX))),
        "uint.MIN_VALUE" => Some(AbcOptDefault::Int(0)),
        _ => None,
    }
}

pub(super) fn parse_abc_opt_default_or_const_ref(
    expr: &SwfExpr,
    pname: &str,
    line: usize,
    col: usize,
) -> Result<AbcOptDefault, String> {
    if let Ok(lit) = expr_to_abc_opt_default(expr, pname) {
        return Ok(lit);
    }
    if let Some(raw) = swf_dotted_const_chain(expr) {
        if let Some(b) = builtin_abc_opt_default(&raw) {
            return Ok(b);
        }
        return Ok(AbcOptDefault::ConstRef {
            raw,
            param_name: pname.to_owned(),
            line,
            col,
        });
    }
    Err(format!(
        "line {line}:{col}: ActionScript 3 optional parameter '{pname}' initializer must be a compile-time constant"
    ))
}

fn parse_as3_param_list(parser: &mut SwfParser) -> Result<(Vec<String>, AbcMethodMeta), String> {
    parser.expect(&SwfToken::LParen)?;
    let mut params = Vec::new();
    let mut min_arity = 0usize;
    let mut opt_defaults = Vec::new();
    let mut rest_param = None;
    if !parser.eat(&SwfToken::RParen) {
        loop {
            if parser.eat(&SwfToken::DotDot) {
                parser.expect(&SwfToken::Dot)?;
                let rname = parser.expect_ident()?;
                parser.skip_type_annotation()?;
                rest_param = Some(rname);
                parser.expect(&SwfToken::RParen)?;
                break;
            }
            let param_tok_pos = parser.pos;
            let (line, col) = parser.span_at(param_tok_pos);
            let pname = parser.expect_ident()?;
            parser.skip_type_annotation()?;
            if parser.eat(&SwfToken::Assign) {
                let def_expr = parser.parse_expr()?;
                opt_defaults.push(parse_abc_opt_default_or_const_ref(
                    &def_expr, &pname, line, col,
                )?);
            } else if !opt_defaults.is_empty() {
                return Err(format!(
                    "line {line}:{col}: ActionScript 3 required parameter '{pname}' cannot follow an optional parameter"
                ));
            } else {
                min_arity += 1;
            }
            params.push(pname);
            if parser.eat(&SwfToken::Comma) {
                continue;
            }
            parser.expect(&SwfToken::RParen)?;
            break;
        }
    }
    parser.skip_type_annotation()?;
    let max_arity = if rest_param.is_some() {
        None
    } else {
        Some(params.len())
    };
    Ok((
        params,
        AbcMethodMeta {
            is_private: false,
            is_override: false,
            min_arity,
            max_arity,
            rest_param,
            opt_defaults,
        },
    ))
}

pub(super) fn parse_as3_source_to_spec_strict(
    source: &str,
    fallback_qname: &str,
    external_source: bool,
) -> Result<AbcClassSpec, String> {
    let (tokens, spans) = tokenize_swf_script_with_spans(source)?;
    let mut parser = SwfParser::with_spans(tokens, spans, true);

    let starts_with_package = parser.peek() == Some(&SwfToken::Ident("package".to_owned()));
    let has_type_decl_tok = parser
        .tokens
        .iter()
        .any(|t| matches!(t, SwfToken::Ident(kw) if kw == "class" || kw == "interface"));

    if !starts_with_package && !has_type_decl_tok {
        if external_source {
            return Err(
                "external ActionScript 3 source file must declare a package and class or interface"
                    .to_owned(),
            );
        }
        let stmts = parser.parse_stmt_list(false)?;
        if parser.pos < parser.tokens.len() {
            return Err(format!(
                "unexpected trailing token {:?} in ActionScript 3 script",
                parser.peek()
            ));
        }
        let mut imported_classes = HashMap::new();
        let mut wildcard_packages = Vec::new();
        extract_script_imports(source, &mut imported_classes, &mut wildcard_packages);
        let (pkg, fc) = split_qualified_class(fallback_qname);
        return Ok(AbcClassSpec {
            pkg,
            class_name: if fc.is_empty() { "Main".to_owned() } else { fc },
            super_pkg: "flash.display".to_owned(),
            super_name: "MovieClip".to_owned(),
            imported_classes,
            wildcard_packages,
            has_explicit_super: stmts_contain_super_ctor(&stmts),
            ctor_statements: stmts,
            ..Default::default()
        });
    }

    if !external_source {
        return Err(
            "ActionScript 3 frame script cannot declare a package, class, or interface; move package/class/interface definitions to an external .as source file"
                .to_owned(),
        );
    }

    let mut pkg = String::new();
    let mut has_package_brace = false;
    if starts_with_package {
        parser.pos += 1;
        if parser.peek() != Some(&SwfToken::LBrace) {
            pkg = parse_dotted_name(&mut parser)?;
        }
        parser.expect(&SwfToken::LBrace)?;
        has_package_brace = true;
    }

    let mut imported_classes: HashMap<String, String> = HashMap::new();
    let mut wildcard_packages: Vec<String> = Vec::new();
    loop {
        if parser.eat(&SwfToken::Semi) {
            continue;
        }
        if parser.eat(&SwfToken::LBracket) {
            consume_metadata_brackets(&mut parser)?;
            continue;
        }
        if parser.peek() == Some(&SwfToken::Ident("import".to_owned())) {
            consume_import_directive(&mut parser, &mut imported_classes, &mut wildcard_packages)?;
            continue;
        }
        break;
    }

    while let Some(SwfToken::Ident(m)) = parser.peek() {
        if matches!(m.as_str(), "public" | "internal" | "final" | "dynamic") {
            parser.pos += 1;
        } else {
            break;
        }
    }

    let decl_kw = match parser.next() {
        Some(SwfToken::Ident(kw)) if kw == "class" || kw == "interface" => kw,
        _ => {
            return Err(
                "expected 'class' or 'interface' declaration in ActionScript 3 source".to_owned(),
            );
        }
    };

    if decl_kw == "interface" {
        let iface_name = parser.expect_ident()?;
        let mut super_interfaces = Vec::new();
        if parser.peek() == Some(&SwfToken::Ident("extends".to_owned())) {
            parser.pos += 1;
            loop {
                let sup_if = parse_dotted_name(&mut parser)?;
                super_interfaces.push(resolve_parsed_as3_type_ref(
                    &sup_if,
                    &pkg,
                    &imported_classes,
                ));
                if !parser.eat(&SwfToken::Comma) {
                    break;
                }
            }
        }
        if parser.peek() == Some(&SwfToken::Ident("implements".to_owned())) {
            return Err(
                "ActionScript 3 interface cannot use 'implements'; use 'extends' for super-interfaces"
                    .to_owned(),
            );
        }
        parser.expect(&SwfToken::LBrace)?;
        let mut interface_methods = Vec::new();
        while parser.peek().is_some() && parser.peek() != Some(&SwfToken::RBrace) {
            if parser.eat(&SwfToken::Semi) {
                continue;
            }
            if parser.eat(&SwfToken::LBracket) {
                consume_metadata_brackets(&mut parser)?;
                continue;
            }
            if parser.peek() == Some(&SwfToken::Ident("import".to_owned())) {
                consume_import_directive(
                    &mut parser,
                    &mut imported_classes,
                    &mut wildcard_packages,
                )?;
                continue;
            }
            while let Some(SwfToken::Ident(m)) = parser.peek() {
                if matches!(m.as_str(), "public" | "internal") {
                    parser.pos += 1;
                } else {
                    break;
                }
            }
            if parser.next() != Some(SwfToken::Ident("function".to_owned())) {
                return Err(format!(
                    "ActionScript 3 interface '{iface_name}' may only declare method signatures"
                ));
            }
            let fn_name = parser.expect_ident()?;
            let (params, parsed_meta) = parse_as3_param_list(&mut parser)?;
            if parser.peek() == Some(&SwfToken::LBrace) {
                return Err(format!(
                    "ActionScript 3 interface method '{iface_name}.{fn_name}' cannot have an implementation body"
                ));
            }
            parser.consume_optional_semi();
            interface_methods.push((fn_name, params, parsed_meta));
        }
        parser.expect(&SwfToken::RBrace)?;
        if has_package_brace {
            while parser.eat(&SwfToken::Semi) {}
            parser.expect(&SwfToken::RBrace)?;
        }
        while parser.eat(&SwfToken::Semi) {}
        if parser.pos < parser.tokens.len() {
            return Err(format!(
                "unexpected trailing token {:?} after ActionScript 3 interface",
                parser.peek()
            ));
        }
        extract_script_imports(source, &mut imported_classes, &mut wildcard_packages);
        return Ok(AbcClassSpec {
            pkg,
            class_name: iface_name,
            is_interface: true,
            interfaces: super_interfaces,
            interface_methods,
            imported_classes,
            wildcard_packages,
            ..Default::default()
        });
    }

    let class_name = parser.expect_ident()?;
    let mut super_pkg = String::new();
    let mut super_name = "Object".to_owned();

    if parser.peek() == Some(&SwfToken::Ident("extends".to_owned())) {
        parser.pos += 1;
        let sup = parse_dotted_name(&mut parser)?;
        let (sp, sn) = resolve_parsed_as3_type_ref(&sup, &pkg, &imported_classes);
        super_pkg = sp;
        super_name = sn;
    }

    let mut interfaces = Vec::new();
    if parser.peek() == Some(&SwfToken::Ident("implements".to_owned())) {
        parser.pos += 1;
        loop {
            let iface = parse_dotted_name(&mut parser)?;
            interfaces.push(resolve_parsed_as3_type_ref(&iface, &pkg, &imported_classes));
            if !parser.eat(&SwfToken::Comma) {
                break;
            }
        }
    }

    parser.expect(&SwfToken::LBrace)?;

    let mut ctor_params = Vec::new();
    let mut ctor_rest_param = None;
    let mut ctor_opt_defaults = Vec::new();
    let mut has_explicit_ctor = false;
    let mut ctor_statements = Vec::new();
    let mut instance_fields = Vec::new();
    let mut static_fields = Vec::new();
    let mut static_consts = Vec::new();
    let mut static_methods = Vec::new();
    let mut static_method_meta = HashMap::new();
    let mut extra_methods = Vec::new();
    let mut method_meta = HashMap::new();

    while parser.peek().is_some() && parser.peek() != Some(&SwfToken::RBrace) {
        if parser.eat(&SwfToken::Semi) {
            continue;
        }
        if parser.eat(&SwfToken::LBracket) {
            consume_metadata_brackets(&mut parser)?;
            continue;
        }
        if parser.peek() == Some(&SwfToken::Ident("import".to_owned())) {
            consume_import_directive(&mut parser, &mut imported_classes, &mut wildcard_packages)?;
            continue;
        }
        let mut is_static = false;
        let mut is_override = false;
        let mut is_private = false;
        while let Some(SwfToken::Ident(m)) = parser.peek() {
            if matches!(
                m.as_str(),
                "public"
                    | "private"
                    | "protected"
                    | "internal"
                    | "static"
                    | "override"
                    | "final"
                    | "dynamic"
            ) {
                if m == "static" {
                    is_static = true;
                } else if m == "override" {
                    is_override = true;
                } else if m == "private" {
                    is_private = true;
                }
                parser.pos += 1;
            } else {
                break;
            }
        }
        if parser.peek() == Some(&SwfToken::Ident("function".to_owned())) {
            parser.pos += 1;
            let fn_name = parser.expect_ident()?;
            let (params, mut parsed_meta) = parse_as3_param_list(&mut parser)?;
            parsed_meta.is_private = is_private;
            parsed_meta.is_override = is_override;
            parser.expect(&SwfToken::LBrace)?;
            let body = parser.parse_stmt_list(true)?;
            parser.expect(&SwfToken::RBrace)?;
            if is_override && (is_static || is_private || fn_name == class_name) {
                return Err(format!(
                    "ActionScript 3 'override' modifier is only valid on non-private instance methods ('{class_name}.{fn_name}')"
                ));
            }
            if fn_name == class_name && !is_static {
                ctor_params = params;
                ctor_rest_param = parsed_meta.rest_param;
                ctor_opt_defaults = parsed_meta.opt_defaults;
                has_explicit_ctor = true;
                ctor_statements.extend(body);
            } else if is_static {
                static_method_meta.insert(fn_name.clone(), parsed_meta);
                static_methods.push((fn_name, params, body));
            } else {
                method_meta.insert(fn_name.clone(), parsed_meta);
                extra_methods.push((fn_name, params, body));
            }
        } else if matches!(parser.peek(), Some(SwfToken::Ident(kw)) if kw == "var" || kw == "const")
        {
            let is_const_decl = matches!(parser.peek(), Some(SwfToken::Ident(kw)) if kw == "const");
            if is_override {
                return Err(format!(
                    "ActionScript 3 'override' modifier cannot be applied to 'var' or 'const' in '{class_name}'"
                ));
            }
            let mut tmp = Vec::new();
            parser.parse_stmt_into(&mut tmp)?;
            for s in tmp {
                if let SwfStmt::VarDecl { name, init } = s {
                    if is_static {
                        if is_const_decl {
                            static_consts.push((name.clone(), is_private, init.clone()));
                        }
                        static_fields.push((name, init));
                    } else {
                        instance_fields.push((name, init));
                    }
                } else {
                    ctor_statements.push(s);
                }
            }
        } else {
            parser.parse_stmt_into(&mut ctor_statements)?;
        }
    }

    parser.expect(&SwfToken::RBrace)?;
    if has_package_brace {
        while parser.eat(&SwfToken::Semi) {}
        parser.expect(&SwfToken::RBrace)?;
    }
    while parser.eat(&SwfToken::Semi) {}
    if parser.pos < parser.tokens.len() {
        return Err(format!(
            "unexpected trailing token {:?} after ActionScript 3 class",
            parser.peek()
        ));
    }

    let has_explicit_super = stmts_contain_super_ctor(&ctor_statements);

    extract_script_imports(source, &mut imported_classes, &mut wildcard_packages);

    Ok(AbcClassSpec {
        pkg,
        class_name,
        is_interface: false,
        super_pkg,
        super_name,
        interfaces,
        interface_methods: Vec::new(),
        method_meta,
        ctor_rest_param,
        ctor_opt_defaults,
        static_method_meta,
        imported_classes,
        wildcard_packages,
        ctor_params,
        has_explicit_ctor,
        has_explicit_super,
        ctor_statements,
        instance_fields,
        static_fields,
        static_consts,
        static_methods,
        frame_scripts: Vec::new(),
        extra_methods,
    })
}

pub(super) struct AbcMethodSpec {
    pub(super) param_names: Vec<u32>,
    pub(super) opt_defaults: Vec<(u32, u8)>,
    pub(super) needs_activation: bool,
    pub(super) uses_dxns: bool,
    pub(super) rest_param: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct AbcExceptionSpec {
    pub(super) from: u32,
    pub(super) to: u32,
    pub(super) target: u32,
    pub(super) exc_type: u32,
    pub(super) var_name: u32,
}

pub(super) struct AbcMethodBodySpec {
    pub(super) method_idx: u32,
    pub(super) max_stack: u32,
    pub(super) local_count: u32,
    pub(super) init_scope_depth: u32,
    pub(super) max_scope_depth: u32,
    pub(super) code: Vec<u8>,
    pub(super) exceptions: Vec<AbcExceptionSpec>,
    pub(super) activation_traits: Vec<(u32, u32)>,
}

#[derive(Default)]
pub(super) struct Avm2CompilerState {
    pub(super) methods: Vec<AbcMethodSpec>,
    pub(super) bodies: Vec<AbcMethodBodySpec>,
}

impl Avm2CompilerState {
    pub(super) fn alloc_method(&mut self, param_names: Vec<u32>) -> u32 {
        self.alloc_method_sig(param_names, Vec::new(), None)
    }

    pub(super) fn alloc_method_sig(
        &mut self,
        param_names: Vec<u32>,
        opt_defaults: Vec<(u32, u8)>,
        rest_param: Option<String>,
    ) -> u32 {
        let idx = self.methods.len() as u32;
        self.methods.push(AbcMethodSpec {
            param_names,
            opt_defaults,
            needs_activation: false,
            uses_dxns: false,
            rest_param,
        });
        idx
    }
}

pub(super) struct Avm2ControlFrame {
    pub(super) is_loop: bool,
    pub(super) break_patches: Vec<usize>,
    pub(super) continue_patches: Vec<usize>,
    pub(super) finally_depth: usize,
    pub(super) catch_scope_depth: usize,
}

#[derive(Clone)]
pub(super) enum Avm2PendingUnwind {
    ReturnVal {
        jump_patch: usize,
        ret_reg: u32,
    },
    ReturnVoid {
        jump_patch: usize,
    },
    Break {
        jump_patch: usize,
        target_flow_idx: usize,
    },
    Continue {
        jump_patch: usize,
        target_flow_idx: usize,
    },
}

#[derive(Clone, Default)]
pub(super) struct Avm2FinallyFrame {
    pub(super) pending_unwinds: Vec<Avm2PendingUnwind>,
    pub(super) catch_scope_depth: usize,
}

#[derive(Clone)]
pub(super) struct Avm2PostSuperInit {
    pub(super) frame_bindings: Vec<(u32, u32)>,
    pub(super) add_frame_script_mn: u32,
    pub(super) field_stmts: Vec<SwfStmt>,
}

#[derive(Clone, Default)]
pub(super) struct Avm2ImportCtx {
    pub(super) current_pkg: String,
    pub(super) imported_classes: HashMap<String, String>,
    pub(super) wildcard_packages: Vec<String>,
    pub(super) private_ns_name: String,
    pub(super) private_methods: HashSet<String>,
}

pub(super) struct Avm2LexicalEnv {
    pub(super) init_scope_depth: u32,
    pub(super) compile_depth: usize,
    pub(super) needs_activation: bool,
    pub(super) uses_dxns: bool,
    pub(super) act_reg: Option<u32>,
    pub(super) active_catch_regs: Vec<u32>,
    pub(super) max_catch_scopes: usize,
    pub(super) next_temp_reg: u32,
    pub(super) declared_names: HashSet<String>,
    pub(super) local_regs: HashMap<String, u32>,
    pub(super) own_slots: HashMap<String, u32>,
    pub(super) outer_slots: HashMap<String, (u32, u32)>,
    pub(super) activation_traits: Vec<(u32, u32)>,
    pub(super) exceptions: Vec<AbcExceptionSpec>,
    pub(super) flow_stack: Vec<Avm2ControlFrame>,
    pub(super) finally_stack: Vec<Avm2FinallyFrame>,
    pub(super) imports: Avm2ImportCtx,
    pub(super) post_super: Option<Avm2PostSuperInit>,
}

impl Avm2LexicalEnv {
    pub(super) fn alloc_temp_reg(&mut self) -> u32 {
        let reg = self.next_temp_reg;
        self.next_temp_reg += 1;
        reg
    }

    pub(super) fn open_package_ns_set(&self, pool: &mut AbcPoolBuilder) -> u32 {
        let mut ns_indices = Vec::with_capacity(2 + self.imports.wildcard_packages.len());
        ns_indices.push(pool.add_namespace(0x16, ""));
        if !self.imports.current_pkg.is_empty() {
            let cur_ns = pool.add_namespace(0x16, &self.imports.current_pkg);
            if !ns_indices.contains(&cur_ns) {
                ns_indices.push(cur_ns);
            }
        }
        for wpkg in &self.imports.wildcard_packages {
            if !wpkg.is_empty() {
                let wns = pool.add_namespace(0x16, wpkg);
                if !ns_indices.contains(&wns) {
                    ns_indices.push(wns);
                }
            }
        }
        pool.add_ns_set(ns_indices)
    }

    pub(super) fn resolve_ident_multiname(&self, pool: &mut AbcPoolBuilder, name: &str) -> u32 {
        if self.imports.private_methods.contains(name) && !self.imports.private_ns_name.is_empty() {
            let pub_ns = pool.add_namespace(0x16, "");
            let priv_ns = pool.add_namespace(0x05, &self.imports.private_ns_name);
            let ns_set = pool.add_ns_set(vec![pub_ns, priv_ns]);
            return pool.add_multiname(name, ns_set);
        }
        if let Some(imp_pkg) = self.imports.imported_classes.get(name) {
            return pool.add_qname(imp_pkg, name);
        }
        if !self.imports.wildcard_packages.is_empty() {
            let ns_set = self.open_package_ns_set(pool);
            return pool.add_multiname(name, ns_set);
        }
        pool.add_qname("", name)
    }

    pub(super) fn resolve_type_multiname(&self, pool: &mut AbcPoolBuilder, type_name: &str) -> u32 {
        if type_name.contains('.') {
            let (tp, ts) = split_qualified_class(type_name);
            return pool.add_qname(&tp, &ts);
        }
        if let Some(imp_pkg) = self.imports.imported_classes.get(type_name) {
            return pool.add_qname(imp_pkg, type_name);
        }
        if matches!(
            type_name,
            "Object"
                | "Array"
                | "String"
                | "Number"
                | "int"
                | "uint"
                | "Boolean"
                | "Function"
                | "Class"
                | "XML"
                | "XMLList"
                | "QName"
                | "Namespace"
                | "Date"
                | "RegExp"
                | "Error"
                | "TypeError"
                | "RangeError"
                | "ReferenceError"
                | "ArgumentError"
                | "VerifyError"
                | "SyntaxError"
                | "URIError"
                | "EvalError"
                | "SecurityError"
        ) {
            return pool.add_qname("", type_name);
        }
        if !self.imports.wildcard_packages.is_empty() || !self.imports.current_pkg.is_empty() {
            let ns_set = self.open_package_ns_set(pool);
            return pool.add_multiname(type_name, ns_set);
        }
        pool.add_qname("", type_name)
    }
}

pub(super) fn builtin_flash_superclass(
    pkg: &str,
    name: &str,
) -> Option<(&'static str, &'static str)> {
    match (pkg, name) {
        ("", "Object") => None,
        ("flash.display", "MovieClip") => Some(("flash.display", "Sprite")),
        ("flash.display", "Sprite") => Some(("flash.display", "DisplayObjectContainer")),
        ("flash.display", "DisplayObjectContainer") => Some(("flash.display", "InteractiveObject")),
        ("flash.display", "InteractiveObject") | ("flash.display", "SimpleButton") => {
            Some(("flash.display", "DisplayObject"))
        }
        ("flash.text", "TextField") => Some(("flash.display", "InteractiveObject")),
        ("flash.display", "Shape" | "Bitmap") | ("flash.media", "Video") => {
            Some(("flash.display", "DisplayObject"))
        }
        ("flash.display", "DisplayObject")
        | ("flash.utils", "Timer")
        | ("flash.net", "URLLoader")
        | ("flash.media", "Sound" | "SoundChannel") => Some(("flash.events", "EventDispatcher")),
        ("flash.events", "EventDispatcher") => Some(("", "Object")),
        (
            "",
            "TypeError" | "RangeError" | "ReferenceError" | "ArgumentError" | "VerifyError"
            | "SyntaxError" | "URIError" | "EvalError" | "SecurityError",
        )
        | ("flash.errors", _) => Some(("", "Error")),
        _ => Some(("", "Object")),
    }
}

/// Returns the ordered list of superclasses strictly below `Object` (`[HighestAncestor, ..., DirectSuper]`).
pub(super) fn resolve_abc_ancestor_chain(
    cls: &AbcClassSpec,
    all_classes: &[AbcClassSpec],
) -> Vec<(String, String)> {
    let mut rev_chain = Vec::new();
    let mut seen = HashSet::new();
    seen.insert((cls.pkg.clone(), cls.class_name.clone()));
    let mut cur = (cls.super_pkg.clone(), cls.super_name.clone());
    while !(cur.0.is_empty() && cur.1 == "Object") && !cur.1.is_empty() {
        if !seen.insert(cur.clone()) {
            break;
        }
        let next = if let Some(parent) = all_classes
            .iter()
            .find(|c| c.pkg == cur.0 && c.class_name == cur.1)
        {
            Some((parent.super_pkg.clone(), parent.super_name.clone()))
        } else {
            builtin_flash_superclass(&cur.0, &cur.1).map(|(sp, sn)| (sp.to_owned(), sn.to_owned()))
        };
        rev_chain.push(cur);
        let Some(n) = next else { break };
        cur = n;
    }
    rev_chain.reverse();
    rev_chain
}

pub(super) fn expr_has_direct_self_named_fn(expr: &SwfExpr) -> bool {
    match expr {
        SwfExpr::Function {
            name: Some(sn),
            params,
            body,
            ..
        } => {
            if !sn.is_empty() && !params.iter().any(|p| p == sn) {
                let mut d_refs = HashSet::new();
                let mut c_free = HashSet::new();
                collect_refs_and_child_free_in_stmts(body, &mut d_refs, &mut c_free);
                if d_refs.contains(sn) || c_free.contains(sn) {
                    return true;
                }
            }
            false
        }
        SwfExpr::Function { name: None, .. } => false,
        SwfExpr::ArrayLit(elems) => elems.iter().any(expr_has_direct_self_named_fn),
        SwfExpr::ObjectLit(props) => props.iter().any(|(_, v)| expr_has_direct_self_named_fn(v)),
        SwfExpr::Member(obj, _) => expr_has_direct_self_named_fn(obj),
        SwfExpr::Index(obj, idx) => {
            expr_has_direct_self_named_fn(obj) || expr_has_direct_self_named_fn(idx)
        }
        SwfExpr::XmlLit { pieces, .. } => pieces.iter().any(|p| match p {
            SwfXmlCompiledPiece::Raw(_) => false,
            SwfXmlCompiledPiece::ElemExpr(e) | SwfXmlCompiledPiece::AttrExpr(e) => {
                expr_has_direct_self_named_fn(e)
            }
        }),
        SwfExpr::XmlAttr { object, attr }
        | SwfExpr::XmlDescendants {
            object,
            target: attr,
            ..
        } => {
            expr_has_direct_self_named_fn(object)
                || match attr {
                    SwfXmlSelector::Named(_) => false,
                    SwfXmlSelector::Computed(idx) => expr_has_direct_self_named_fn(idx),
                }
        }
        SwfExpr::Unary { expr, .. } => expr_has_direct_self_named_fn(expr),
        SwfExpr::Delete(target) => match target.as_ref() {
            SwfLValue::Ident(_) => false,
            SwfLValue::Member(obj, _) => expr_has_direct_self_named_fn(obj),
            SwfLValue::Index(obj, idx) => {
                expr_has_direct_self_named_fn(obj) || expr_has_direct_self_named_fn(idx)
            }
            SwfLValue::XmlAttr { object, attr } => {
                expr_has_direct_self_named_fn(object)
                    || match attr {
                        SwfXmlSelector::Named(_) => false,
                        SwfXmlSelector::Computed(idx) => expr_has_direct_self_named_fn(idx),
                    }
            }
        },
        SwfExpr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            expr_has_direct_self_named_fn(cond)
                || expr_has_direct_self_named_fn(then_expr)
                || expr_has_direct_self_named_fn(else_expr)
        }
        SwfExpr::Binary { lhs, rhs, .. } => {
            expr_has_direct_self_named_fn(lhs) || expr_has_direct_self_named_fn(rhs)
        }
        SwfExpr::Call { callee, args } | SwfExpr::New { callee, args } => {
            expr_has_direct_self_named_fn(callee) || args.iter().any(expr_has_direct_self_named_fn)
        }
        SwfExpr::Null
        | SwfExpr::Undefined
        | SwfExpr::Bool(_)
        | SwfExpr::Int(_)
        | SwfExpr::Float(_)
        | SwfExpr::Str(_)
        | SwfExpr::Ident(_) => false,
    }
}

fn lvalue_has_direct_self_named_fn(target: &SwfLValue) -> bool {
    match target {
        SwfLValue::Ident(_) => false,
        SwfLValue::Member(obj, _) => expr_has_direct_self_named_fn(obj),
        SwfLValue::Index(obj, idx) => {
            expr_has_direct_self_named_fn(obj) || expr_has_direct_self_named_fn(idx)
        }
        SwfLValue::XmlAttr { object, attr } => {
            expr_has_direct_self_named_fn(object)
                || match attr {
                    SwfXmlSelector::Named(_) => false,
                    SwfXmlSelector::Computed(idx) => expr_has_direct_self_named_fn(idx),
                }
        }
    }
}

pub(super) fn stmts_have_dxns(stmts: &[SwfStmt]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        SwfStmt::DefaultXmlNamespace(_) => true,
        SwfStmt::Block(inner) | SwfStmt::While { body: inner, .. } => stmts_have_dxns(inner),
        SwfStmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmts_have_dxns(then_branch)
                || else_branch.as_ref().is_some_and(|eb| stmts_have_dxns(eb))
        }
        SwfStmt::For {
            init, update, body, ..
        } => {
            init.as_ref()
                .is_some_and(|i| stmts_have_dxns(std::slice::from_ref(i)))
                || update
                    .as_ref()
                    .is_some_and(|u| stmts_have_dxns(std::slice::from_ref(u)))
                || stmts_have_dxns(body)
        }
        SwfStmt::ForIn { body, .. } => stmts_have_dxns(body),
        SwfStmt::Switch { cases, .. } => cases.iter().any(|(_, cbody)| stmts_have_dxns(cbody)),
        SwfStmt::Try {
            try_body,
            catches,
            finally_body,
        } => {
            stmts_have_dxns(try_body)
                || catches.iter().any(|(_, _, cbody)| stmts_have_dxns(cbody))
                || finally_body
                    .as_ref()
                    .is_some_and(|fbody| stmts_have_dxns(fbody))
        }
        _ => false,
    })
}

pub(super) fn stmts_have_direct_self_named_fn(stmts: &[SwfStmt]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        SwfStmt::VarDecl { init: Some(e), .. }
        | SwfStmt::Expr(e)
        | SwfStmt::Throw(e)
        | SwfStmt::DefaultXmlNamespace(e)
        | SwfStmt::Return(Some(e)) => expr_has_direct_self_named_fn(e),
        SwfStmt::Assign { target, value, .. } => {
            lvalue_has_direct_self_named_fn(target) || expr_has_direct_self_named_fn(value)
        }
        SwfStmt::IncDec { target, .. } => lvalue_has_direct_self_named_fn(target),
        SwfStmt::Block(inner) => stmts_have_direct_self_named_fn(inner),
        SwfStmt::While { cond, body, .. } => {
            expr_has_direct_self_named_fn(cond) || stmts_have_direct_self_named_fn(body)
        }
        SwfStmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_has_direct_self_named_fn(cond)
                || stmts_have_direct_self_named_fn(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|eb| stmts_have_direct_self_named_fn(eb))
        }
        SwfStmt::For {
            init,
            cond,
            update,
            body,
        } => {
            init.as_ref()
                .is_some_and(|i| stmts_have_direct_self_named_fn(std::slice::from_ref(i)))
                || cond.as_ref().is_some_and(expr_has_direct_self_named_fn)
                || update
                    .as_ref()
                    .is_some_and(|u| stmts_have_direct_self_named_fn(std::slice::from_ref(u)))
                || stmts_have_direct_self_named_fn(body)
        }
        SwfStmt::ForIn { object, body, .. } => {
            expr_has_direct_self_named_fn(object) || stmts_have_direct_self_named_fn(body)
        }
        SwfStmt::Switch {
            discriminant,
            cases,
        } => {
            expr_has_direct_self_named_fn(discriminant)
                || cases.iter().any(|(c_opt, cbody)| {
                    c_opt.as_ref().is_some_and(expr_has_direct_self_named_fn)
                        || stmts_have_direct_self_named_fn(cbody)
                })
        }
        SwfStmt::Try {
            try_body,
            catches,
            finally_body,
        } => {
            stmts_have_direct_self_named_fn(try_body)
                || catches
                    .iter()
                    .any(|(_, _, cbody)| stmts_have_direct_self_named_fn(cbody))
                || finally_body
                    .as_ref()
                    .is_some_and(|fbody| stmts_have_direct_self_named_fn(fbody))
        }
        _ => false,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn compile_avm2_scoped_unit<F>(
    pool: &mut AbcPoolBuilder,
    state: &mut Avm2CompilerState,
    method_idx: u32,
    params: &[String],
    stmts: &[SwfStmt],
    outer_slots: &HashMap<String, (u32, u32)>,
    init_scope_depth: u32,
    is_function_scope: bool,
    imports: &Avm2ImportCtx,
    post_super: Option<Avm2PostSuperInit>,
    emit_after_this_scope: F,
) -> Result<(), String>
where
    F: FnOnce(&mut AbcPoolBuilder, &mut SwfBitWriter) -> Result<(), String>,
{
    let mut own_vars = Vec::new();
    let mut own_funcs = Vec::new();
    collect_direct_decls_in_stmts(stmts, &mut own_vars, &mut own_funcs);
    let mut direct_refs = HashSet::new();
    let mut child_free = HashSet::new();
    collect_refs_and_child_free_in_stmts(stmts, &mut direct_refs, &mut child_free);
    if let Some(ps) = &post_super {
        collect_refs_and_child_free_in_stmts(&ps.field_stmts, &mut direct_refs, &mut child_free);
    }

    let mut local_regs = HashMap::new();
    let mut own_slots = HashMap::new();
    let mut activation_traits = Vec::new();
    let mut captured_param_copies: Vec<(u32, u32)> = Vec::new();
    let rest_param_opt = state
        .methods
        .get(method_idx as usize)
        .and_then(|m| m.rest_param.clone());
    let mut next_reg = (params.len() as u32) + 1;
    let mut seen_decls = HashSet::new();

    for (idx, pname) in params.iter().enumerate() {
        let p_reg = (idx + 1) as u32;
        if !seen_decls.insert(pname.clone()) {
            continue;
        }
        if child_free.contains(pname) {
            let slot_id = (activation_traits.len() + 1) as u32;
            let qn = pool.add_qname("", pname);
            activation_traits.push((qn, slot_id));
            own_slots.insert(pname.clone(), slot_id);
            captured_param_copies.push((p_reg, slot_id));
        } else {
            local_regs.insert(pname.clone(), p_reg);
        }
    }
    if let Some(rname) = &rest_param_opt {
        let r_reg = next_reg;
        next_reg += 1;
        if seen_decls.insert(rname.clone()) {
            if child_free.contains(rname) {
                let slot_id = (activation_traits.len() + 1) as u32;
                let qn = pool.add_qname("", rname);
                activation_traits.push((qn, slot_id));
                own_slots.insert(rname.clone(), slot_id);
                captured_param_copies.push((r_reg, slot_id));
            } else {
                local_regs.insert(rname.clone(), r_reg);
            }
        }
    }

    let mut declared_names: HashSet<String> = params
        .iter()
        .cloned()
        .chain(rest_param_opt.iter().cloned())
        .chain(own_funcs.iter().cloned())
        .chain(own_vars.iter().cloned())
        .collect();
    if is_function_scope {
        for decl_name in own_funcs.into_iter().chain(own_vars) {
            if !seen_decls.insert(decl_name.clone()) {
                continue;
            }
            if child_free.contains(&decl_name) {
                let slot_id = (activation_traits.len() + 1) as u32;
                let qn = pool.add_qname("", &decl_name);
                activation_traits.push((qn, slot_id));
                own_slots.insert(decl_name, slot_id);
            } else {
                let reg = next_reg;
                next_reg += 1;
                local_regs.insert(decl_name, reg);
            }
        }
    } else {
        for k in outer_slots.keys() {
            declared_names.insert(k.clone());
        }
    }

    let post_super_has_self_fn = post_super
        .as_ref()
        .is_some_and(|ps| stmts_have_direct_self_named_fn(&ps.field_stmts));
    let needs_activation = !activation_traits.is_empty()
        || stmts_have_direct_self_named_fn(stmts)
        || post_super_has_self_fn;
    let mut code = SwfBitWriter::new();
    code.u8(0xD0); // getlocal_0
    code.u8(0x30); // pushscope (local scope 0 = this)
    emit_after_this_scope(pool, &mut code)?;

    let act_reg = if needs_activation {
        let r = next_reg;
        next_reg += 1;
        code.u8(0x57); // newactivation
        code.u8(0x2A); // dup
        emit_avm2_setlocal(&mut code, r);
        code.u8(0x30); // pushscope (local scope 1 = activation)
        for (p_reg, slot_id) in captured_param_copies {
            code.u8(0x65); // getscopeobject 1
            code.u8(0x01);
            emit_avm2_getlocal(&mut code, p_reg);
            code.u8(0x6D); // setslot
            code.u32_v(slot_id);
        }
        Some(r)
    } else {
        None
    };

    let uses_dxns = stmts_have_dxns(stmts)
        || post_super
            .as_ref()
            .is_some_and(|ps| stmts_have_dxns(&ps.field_stmts));
    let mut env = Avm2LexicalEnv {
        init_scope_depth,
        compile_depth: 0,
        needs_activation,
        uses_dxns,
        act_reg,
        active_catch_regs: Vec::new(),
        max_catch_scopes: 0,
        next_temp_reg: next_reg,
        declared_names,
        local_regs,
        own_slots,
        outer_slots: outer_slots.clone(),
        activation_traits,
        exceptions: Vec::new(),
        flow_stack: Vec::new(),
        finally_stack: Vec::new(),
        imports: imports.clone(),
        post_super,
    };

    compile_avm2_stmt_list(pool, stmts, &mut code, state, &mut env)?;
    code.u8(0x47); // returnvoid

    let (max_stack, computed_locals, max_pushed_scopes) = compute_avm2_method_metrics(
        &code.buf,
        (params.len() + usize::from(rest_param_opt.is_some())) as u32,
        pool,
        &env.exceptions,
    );
    let local_count = computed_locals.max(env.next_temp_reg).max(1);
    let min_pushed_scopes =
        (if env.needs_activation { 2 } else { 1 }) + (env.max_catch_scopes as u32);
    let pushed_scopes = max_pushed_scopes.max(min_pushed_scopes);
    if let Some(mspec) = state.methods.get_mut(method_idx as usize) {
        mspec.needs_activation = env.needs_activation;
        mspec.uses_dxns = env.uses_dxns;
    }
    state.bodies.push(AbcMethodBodySpec {
        method_idx,
        max_stack,
        local_count,
        init_scope_depth,
        max_scope_depth: init_scope_depth + pushed_scopes,
        code: code.buf,
        exceptions: env.exceptions,
        activation_traits: env.activation_traits,
    });
    Ok(())
}

pub(super) type AbcConstTable = HashMap<(String, String), (bool, String, AbcOptDefault)>;

pub(super) fn build_abc_const_table(classes: &[AbcClassSpec]) -> AbcConstTable {
    fn resolve_one(
        owner_qname: &str,
        cname: &str,
        classes: &[AbcClassSpec],
        cache: &mut AbcConstTable,
        visiting: &mut HashSet<(String, String)>,
    ) -> Option<(bool, String, AbcOptDefault)> {
        let key = (owner_qname.to_owned(), cname.to_owned());
        if let Some(hit) = cache.get(&key) {
            return Some(hit.clone());
        }
        if !visiting.insert(key.clone()) {
            return None;
        }
        let spec = classes.iter().find(|c| spec_qname(c) == owner_qname)?;
        let (_, is_priv, init_opt) = spec.static_consts.iter().find(|(n, _, _)| n == cname)?;
        let expr = init_opt.as_ref()?;
        let val = if let Ok(lit) = expr_to_abc_opt_default(expr, cname) {
            Some(lit)
        } else if let Some(raw) = swf_dotted_const_chain(expr) {
            if let Some(b) = builtin_abc_opt_default(&raw) {
                Some(b)
            } else if !raw.contains('.') {
                resolve_one(owner_qname, &raw, classes, cache, visiting).map(|(_, _, v)| v)
            } else {
                let (r_cls, r_prop) = raw.rsplit_once('.')?;
                let target_qname = if let Some(imp_pkg) = spec.imported_classes.get(r_cls) {
                    if imp_pkg.is_empty() {
                        r_cls.to_owned()
                    } else {
                        format!("{imp_pkg}.{r_cls}")
                    }
                } else if !r_cls.contains('.')
                    && !spec.pkg.is_empty()
                    && classes
                        .iter()
                        .any(|c| c.pkg == spec.pkg && c.class_name == r_cls)
                {
                    format!("{}.{r_cls}", spec.pkg)
                } else {
                    r_cls.to_owned()
                };
                resolve_one(&target_qname, r_prop, classes, cache, visiting).and_then(
                    |(priv_flag, _, v)| (!priv_flag || target_qname == owner_qname).then_some(v),
                )
            }
        } else {
            None
        };
        visiting.remove(&key);
        let resolved = (*is_priv, spec.pkg.clone(), val?);
        cache.insert(key, resolved.clone());
        Some(resolved)
    }

    let mut table = AbcConstTable::new();
    let mut visiting = HashSet::new();
    for cls in classes {
        let qn = spec_qname(cls);
        for (cname, _, _) in &cls.static_consts {
            let _ = resolve_one(&qn, cname, classes, &mut table, &mut visiting);
        }
    }
    for cls in classes {
        let qn = spec_qname(cls);
        let chain = resolve_abc_ancestor_chain(cls, classes);
        for (apkg, aname) in chain {
            let anc_qn = if apkg.is_empty() {
                aname
            } else {
                format!("{apkg}.{aname}")
            };
            let inherited: Vec<_> = table
                .iter()
                .filter(|((owner, _), (is_priv, _, _))| owner == &anc_qn && !*is_priv)
                .map(|((_, prop), entry)| ((qn.clone(), prop.clone()), entry.clone()))
                .collect();
            for (k, v) in inherited {
                table.entry(k).or_insert(v);
            }
        }
    }
    table
}

fn resolve_single_abc_opt_default(
    opt: &mut AbcOptDefault,
    caller_qname: Option<&str>,
    caller_pkg: &str,
    imported_classes: &HashMap<String, String>,
    wildcard_packages: &[String],
    known_classes: &[AbcClassSpec],
    const_table: &AbcConstTable,
) -> Result<(), String> {
    let AbcOptDefault::ConstRef {
        raw,
        param_name,
        line,
        col,
    } = opt.clone()
    else {
        return Ok(());
    };
    if let Some(b) = builtin_abc_opt_default(&raw) {
        *opt = b;
        return Ok(());
    }
    if !raw.contains('.') {
        if let Some(cqn) = caller_qname
            && let Some((_, _, val)) = const_table.get(&(cqn.to_owned(), raw.clone()))
        {
            *opt = val.clone();
            return Ok(());
        }
        return Err(format!(
            "line {line}:{col}: unresolved or non-constant default value '{raw}' for optional parameter '{param_name}'"
        ));
    }
    let (cls_part, prop) = raw.rsplit_once('.').unwrap();
    let mut candidates = Vec::new();
    if cls_part.contains('.') {
        candidates.push(cls_part.to_owned());
    } else {
        if let Some(cqn) = caller_qname
            && cqn.rsplit('.').next() == Some(cls_part)
        {
            candidates.push(cqn.to_owned());
        }
        if let Some(imp_pkg) = imported_classes.get(cls_part) {
            candidates.push(if imp_pkg.is_empty() {
                cls_part.to_owned()
            } else {
                format!("{imp_pkg}.{cls_part}")
            });
        }
        if !caller_pkg.is_empty() {
            candidates.push(format!("{caller_pkg}.{cls_part}"));
        }
        for wp in wildcard_packages {
            if !wp.is_empty() {
                candidates.push(format!("{wp}.{cls_part}"));
            }
        }
        candidates.push(cls_part.to_owned());
        for kc in known_classes {
            if kc.class_name == cls_part && kc.pkg.is_empty() {
                candidates.push(kc.class_name.clone());
            }
        }
    }
    for cand in candidates {
        if let Some((is_priv, _, val)) = const_table.get(&(cand.clone(), prop.to_owned())) {
            if *is_priv && caller_qname != Some(cand.as_str()) {
                return Err(format!(
                    "line {line}:{col}: private static const '{raw}' is not accessible as default value for optional parameter '{param_name}'"
                ));
            }
            *opt = val.clone();
            return Ok(());
        }
    }
    Err(format!(
        "line {line}:{col}: unresolved or non-constant default value '{raw}' for optional parameter '{param_name}'"
    ))
}

fn resolve_abc_opt_defaults_in_expr(
    expr: &mut SwfExpr,
    caller_qname: Option<&str>,
    caller_pkg: &str,
    imported_classes: &HashMap<String, String>,
    wildcard_packages: &[String],
    known_classes: &[AbcClassSpec],
    const_table: &AbcConstTable,
) -> Result<(), String> {
    match expr {
        SwfExpr::Function {
            opt_defaults, body, ..
        } => {
            for d in opt_defaults {
                resolve_single_abc_opt_default(
                    d,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
            }
            resolve_abc_opt_defaults_in_stmts(
                body,
                caller_qname,
                caller_pkg,
                imported_classes,
                wildcard_packages,
                known_classes,
                const_table,
            )
        }
        SwfExpr::ArrayLit(elems) => {
            for e in elems {
                resolve_abc_opt_defaults_in_expr(
                    e,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
            }
            Ok(())
        }
        SwfExpr::ObjectLit(props) => {
            for (_, v) in props {
                resolve_abc_opt_defaults_in_expr(
                    v,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
            }
            Ok(())
        }
        SwfExpr::Member(a, _) | SwfExpr::Unary { expr: a, .. } => resolve_abc_opt_defaults_in_expr(
            a,
            caller_qname,
            caller_pkg,
            imported_classes,
            wildcard_packages,
            known_classes,
            const_table,
        ),
        SwfExpr::Index(a, b) | SwfExpr::Binary { lhs: a, rhs: b, .. } => {
            resolve_abc_opt_defaults_in_expr(
                a,
                caller_qname,
                caller_pkg,
                imported_classes,
                wildcard_packages,
                known_classes,
                const_table,
            )?;
            resolve_abc_opt_defaults_in_expr(
                b,
                caller_qname,
                caller_pkg,
                imported_classes,
                wildcard_packages,
                known_classes,
                const_table,
            )
        }
        SwfExpr::Call { callee, args } | SwfExpr::New { callee, args } => {
            resolve_abc_opt_defaults_in_expr(
                callee,
                caller_qname,
                caller_pkg,
                imported_classes,
                wildcard_packages,
                known_classes,
                const_table,
            )?;
            for a in args {
                resolve_abc_opt_defaults_in_expr(
                    a,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
            }
            Ok(())
        }
        SwfExpr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            resolve_abc_opt_defaults_in_expr(
                cond,
                caller_qname,
                caller_pkg,
                imported_classes,
                wildcard_packages,
                known_classes,
                const_table,
            )?;
            resolve_abc_opt_defaults_in_expr(
                then_expr,
                caller_qname,
                caller_pkg,
                imported_classes,
                wildcard_packages,
                known_classes,
                const_table,
            )?;
            resolve_abc_opt_defaults_in_expr(
                else_expr,
                caller_qname,
                caller_pkg,
                imported_classes,
                wildcard_packages,
                known_classes,
                const_table,
            )
        }
        _ => Ok(()),
    }
}

pub(super) fn resolve_abc_opt_defaults_in_stmts(
    stmts: &mut [SwfStmt],
    caller_qname: Option<&str>,
    caller_pkg: &str,
    imported_classes: &HashMap<String, String>,
    wildcard_packages: &[String],
    known_classes: &[AbcClassSpec],
    const_table: &AbcConstTable,
) -> Result<(), String> {
    for stmt in stmts {
        match stmt {
            SwfStmt::FunctionDecl {
                opt_defaults, body, ..
            } => {
                for d in opt_defaults {
                    resolve_single_abc_opt_default(
                        d,
                        caller_qname,
                        caller_pkg,
                        imported_classes,
                        wildcard_packages,
                        known_classes,
                        const_table,
                    )?;
                }
                resolve_abc_opt_defaults_in_stmts(
                    body,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
            }
            SwfStmt::VarDecl { init: Some(e), .. }
            | SwfStmt::Expr(e)
            | SwfStmt::Throw(e)
            | SwfStmt::Return(Some(e))
            | SwfStmt::DefaultXmlNamespace(e) => {
                resolve_abc_opt_defaults_in_expr(
                    e,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
            }
            SwfStmt::Assign { value, .. } => {
                resolve_abc_opt_defaults_in_expr(
                    value,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
            }
            SwfStmt::Block(inner) => {
                resolve_abc_opt_defaults_in_stmts(
                    inner,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
            }
            SwfStmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                resolve_abc_opt_defaults_in_expr(
                    cond,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
                resolve_abc_opt_defaults_in_stmts(
                    then_branch,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
                if let Some(eb) = else_branch {
                    resolve_abc_opt_defaults_in_stmts(
                        eb,
                        caller_qname,
                        caller_pkg,
                        imported_classes,
                        wildcard_packages,
                        known_classes,
                        const_table,
                    )?;
                }
            }
            SwfStmt::While { cond, body, .. } => {
                resolve_abc_opt_defaults_in_expr(
                    cond,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
                resolve_abc_opt_defaults_in_stmts(
                    body,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
            }
            SwfStmt::For {
                init,
                cond,
                update,
                body,
            } => {
                if let Some(i) = init {
                    resolve_abc_opt_defaults_in_stmts(
                        std::slice::from_mut(i.as_mut()),
                        caller_qname,
                        caller_pkg,
                        imported_classes,
                        wildcard_packages,
                        known_classes,
                        const_table,
                    )?;
                }
                if let Some(c) = cond {
                    resolve_abc_opt_defaults_in_expr(
                        c,
                        caller_qname,
                        caller_pkg,
                        imported_classes,
                        wildcard_packages,
                        known_classes,
                        const_table,
                    )?;
                }
                if let Some(u) = update {
                    resolve_abc_opt_defaults_in_stmts(
                        std::slice::from_mut(u.as_mut()),
                        caller_qname,
                        caller_pkg,
                        imported_classes,
                        wildcard_packages,
                        known_classes,
                        const_table,
                    )?;
                }
                resolve_abc_opt_defaults_in_stmts(
                    body,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
            }
            SwfStmt::ForIn { object, body, .. } => {
                resolve_abc_opt_defaults_in_expr(
                    object,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
                resolve_abc_opt_defaults_in_stmts(
                    body,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
            }
            SwfStmt::Switch {
                discriminant,
                cases,
            } => {
                resolve_abc_opt_defaults_in_expr(
                    discriminant,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
                for (c_opt, cbody) in cases {
                    if let Some(ce) = c_opt {
                        resolve_abc_opt_defaults_in_expr(
                            ce,
                            caller_qname,
                            caller_pkg,
                            imported_classes,
                            wildcard_packages,
                            known_classes,
                            const_table,
                        )?;
                    }
                    resolve_abc_opt_defaults_in_stmts(
                        cbody,
                        caller_qname,
                        caller_pkg,
                        imported_classes,
                        wildcard_packages,
                        known_classes,
                        const_table,
                    )?;
                }
            }
            SwfStmt::Try {
                try_body,
                catches,
                finally_body,
            } => {
                resolve_abc_opt_defaults_in_stmts(
                    try_body,
                    caller_qname,
                    caller_pkg,
                    imported_classes,
                    wildcard_packages,
                    known_classes,
                    const_table,
                )?;
                for (_, _, cb) in catches {
                    resolve_abc_opt_defaults_in_stmts(
                        cb,
                        caller_qname,
                        caller_pkg,
                        imported_classes,
                        wildcard_packages,
                        known_classes,
                        const_table,
                    )?;
                }
                if let Some(fb) = finally_body {
                    resolve_abc_opt_defaults_in_stmts(
                        fb,
                        caller_qname,
                        caller_pkg,
                        imported_classes,
                        wildcard_packages,
                        known_classes,
                        const_table,
                    )?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn resolve_abc_opt_defaults_on_spec(
    spec: &mut AbcClassSpec,
    known_classes: &[AbcClassSpec],
    const_table: &AbcConstTable,
) -> Result<(), String> {
    let qn = spec_qname(spec);
    let pkg = spec.pkg.clone();
    let imp = spec.imported_classes.clone();
    let wild = spec.wildcard_packages.clone();
    for (_, _, meta) in &mut spec.interface_methods {
        for d in &mut meta.opt_defaults {
            resolve_single_abc_opt_default(
                d,
                Some(&qn),
                &pkg,
                &imp,
                &wild,
                known_classes,
                const_table,
            )?;
        }
    }
    for d in &mut spec.ctor_opt_defaults {
        resolve_single_abc_opt_default(
            d,
            Some(&qn),
            &pkg,
            &imp,
            &wild,
            known_classes,
            const_table,
        )?;
    }
    for meta in spec.method_meta.values_mut() {
        for d in &mut meta.opt_defaults {
            resolve_single_abc_opt_default(
                d,
                Some(&qn),
                &pkg,
                &imp,
                &wild,
                known_classes,
                const_table,
            )?;
        }
    }
    for meta in spec.static_method_meta.values_mut() {
        for d in &mut meta.opt_defaults {
            resolve_single_abc_opt_default(
                d,
                Some(&qn),
                &pkg,
                &imp,
                &wild,
                known_classes,
                const_table,
            )?;
        }
    }
    resolve_abc_opt_defaults_in_stmts(
        &mut spec.ctor_statements,
        Some(&qn),
        &pkg,
        &imp,
        &wild,
        known_classes,
        const_table,
    )?;
    for (_, _, body) in &mut spec.extra_methods {
        resolve_abc_opt_defaults_in_stmts(
            body,
            Some(&qn),
            &pkg,
            &imp,
            &wild,
            known_classes,
            const_table,
        )?;
    }
    for (_, _, body) in &mut spec.static_methods {
        resolve_abc_opt_defaults_in_stmts(
            body,
            Some(&qn),
            &pkg,
            &imp,
            &wild,
            known_classes,
            const_table,
        )?;
    }
    for (_, finit) in &mut spec.instance_fields {
        if let Some(e) = finit {
            resolve_abc_opt_defaults_in_expr(
                e,
                Some(&qn),
                &pkg,
                &imp,
                &wild,
                known_classes,
                const_table,
            )?;
        }
    }
    for (_, sfinit) in &mut spec.static_fields {
        if let Some(e) = sfinit {
            resolve_abc_opt_defaults_in_expr(
                e,
                Some(&qn),
                &pkg,
                &imp,
                &wild,
                known_classes,
                const_table,
            )?;
        }
    }
    Ok(())
}

#[allow(dead_code)]
pub(super) fn try_compile_avm2_source(
    source: &str,
    external_source: bool,
) -> Result<AbcClassSpec, String> {
    try_compile_avm2_source_in_project(None, source, external_source)
}

pub(super) fn try_compile_avm2_source_in_project(
    project: Option<&Project>,
    source: &str,
    external_source: bool,
) -> Result<AbcClassSpec, String> {
    let mut spec = parse_as3_source_to_spec_strict(source, "Main", external_source)?;
    let mut all_specs = Vec::new();
    if let Some(proj) = project {
        let included = swf_collect_included_source_paths(proj);
        for src in &proj.sources {
            if src.language != ScriptLanguage::ActionScript3 {
                continue;
            }
            let norm_lower = src
                .path
                .trim()
                .replace('\\', "/")
                .trim_start_matches('/')
                .to_ascii_lowercase();
            let Ok(expanded) = expand_swf_includes(proj, src.language, &src.path, &src.source)
            else {
                continue;
            };
            if included.contains(&norm_lower) && !as3_source_declares_package_or_class(&expanded) {
                continue;
            }
            let fallback = strip_as_extension_ci(src.path.rsplit('/').next().unwrap_or("Main"));
            if let Ok(other_spec) = parse_as3_source_to_spec_strict(&expanded, fallback, true)
                && (!external_source || spec_qname(&other_spec) != spec_qname(&spec))
            {
                all_specs.push(other_spec);
            }
        }
    }
    all_specs.push(spec.clone());
    let ordered_known = order_and_resolve_abc_classes(&all_specs);
    let const_table = build_abc_const_table(&ordered_known);
    resolve_abc_opt_defaults_on_spec(&mut spec, &ordered_known, &const_table)?;
    if spec.is_interface {
        return Ok(spec);
    }
    let mut pool = AbcPoolBuilder::default();
    let mut state = Avm2CompilerState::default();
    let empty_outer = HashMap::new();
    let chain = resolve_abc_ancestor_chain(&spec, std::slice::from_ref(&spec));
    let chain_len = chain.len() as u32;
    let cinit_scope_depth = 2 + chain_len;
    let method_scope_depth = 3 + chain_len;
    let private_methods: HashSet<String> = spec
        .method_meta
        .iter()
        .filter(|(_, m)| m.is_private)
        .map(|(n, _)| n.clone())
        .collect();
    let private_ns_name = if spec.pkg.is_empty() {
        spec.class_name.clone()
    } else {
        format!("{}:{}", spec.pkg, spec.class_name)
    };
    let imports = Avm2ImportCtx {
        current_pkg: spec.pkg.clone(),
        imported_classes: spec.imported_classes.clone(),
        wildcard_packages: spec.wildcard_packages.clone(),
        private_ns_name,
        private_methods,
    };

    let mut cinit_stmts = Vec::new();
    for (sf_name, sf_init) in &spec.static_fields {
        if let Some(expr) = sf_init {
            cinit_stmts.push(SwfStmt::Assign {
                target: SwfLValue::Member(
                    Box::new(SwfExpr::Ident("this".to_owned())),
                    sf_name.clone(),
                ),
                op: SwfAssignOp::Assign,
                value: expr.clone(),
            });
        }
    }
    if !cinit_stmts.is_empty() {
        let cinit_m = state.alloc_method(Vec::new());
        compile_avm2_scoped_unit(
            &mut pool,
            &mut state,
            cinit_m,
            &[],
            &cinit_stmts,
            &empty_outer,
            cinit_scope_depth,
            false,
            &imports,
            None,
            |_, _| Ok(()),
        )?;
    }

    let ctor_p_indices: Vec<u32> = spec
        .ctor_params
        .iter()
        .map(|p| pool.add_string(p))
        .collect();
    let ctor_opts: Vec<(u32, u8)> = spec
        .ctor_opt_defaults
        .iter()
        .map(|d| d.encode(&mut pool))
        .collect();
    let ctor_m = state.alloc_method_sig(ctor_p_indices, ctor_opts, spec.ctor_rest_param.clone());
    let effective_ctor = build_effective_ctor_stmts(&spec);
    let post_super = spec.has_explicit_super.then(|| Avm2PostSuperInit {
        frame_bindings: Vec::new(),
        add_frame_script_mn: pool.add_qname("", "addFrameScript"),
        field_stmts: build_instance_field_assigns(&spec),
    });
    compile_avm2_scoped_unit(
        &mut pool,
        &mut state,
        ctor_m,
        &spec.ctor_params,
        &effective_ctor,
        &empty_outer,
        method_scope_depth,
        spec.has_explicit_ctor,
        &imports,
        post_super,
        |_, _| Ok(()),
    )?;

    for (sm_name, sm_params, sm_stmts) in &spec.static_methods {
        let p_indices: Vec<u32> = sm_params.iter().map(|p| pool.add_string(p)).collect();
        let sm_meta = spec
            .static_method_meta
            .get(sm_name)
            .cloned()
            .unwrap_or_default();
        let sm_opts: Vec<(u32, u8)> = sm_meta
            .opt_defaults
            .iter()
            .map(|d| d.encode(&mut pool))
            .collect();
        let m_idx = state.alloc_method_sig(p_indices, sm_opts, sm_meta.rest_param);
        compile_avm2_scoped_unit(
            &mut pool,
            &mut state,
            m_idx,
            sm_params,
            sm_stmts,
            &empty_outer,
            method_scope_depth,
            true,
            &imports,
            None,
            |_, _| Ok(()),
        )?;
    }
    for (m_name, m_params, m_stmts) in &spec.extra_methods {
        let p_indices: Vec<u32> = m_params.iter().map(|p| pool.add_string(p)).collect();
        let m_meta = spec.method_meta.get(m_name).cloned().unwrap_or_default();
        let m_opts: Vec<(u32, u8)> = m_meta
            .opt_defaults
            .iter()
            .map(|d| d.encode(&mut pool))
            .collect();
        let m_idx = state.alloc_method_sig(p_indices, m_opts, m_meta.rest_param);
        compile_avm2_scoped_unit(
            &mut pool,
            &mut state,
            m_idx,
            m_params,
            m_stmts,
            &empty_outer,
            method_scope_depth,
            true,
            &imports,
            None,
            |_, _| Ok(()),
        )?;
    }
    Ok(spec)
}

pub(super) fn emit_avm2_getlocal(code: &mut SwfBitWriter, reg: u32) {
    match reg {
        0 => code.u8(0xD0), // getlocal_0
        1 => code.u8(0xD1), // getlocal_1
        2 => code.u8(0xD2), // getlocal_2
        3 => code.u8(0xD3), // getlocal_3
        r => {
            code.u8(0x62); // getlocal
            code.u32_v(r);
        }
    }
}

pub(super) fn emit_avm2_setlocal(code: &mut SwfBitWriter, reg: u32) {
    match reg {
        0 => code.u8(0xD4), // setlocal_0
        1 => code.u8(0xD5), // setlocal_1
        2 => code.u8(0xD6), // setlocal_2
        3 => code.u8(0xD7), // setlocal_3
        r => {
            code.u8(0x63); // setlocal
            code.u32_v(r);
        }
    }
}

pub(super) fn read_abc_u30(code: &[u8], pos: &mut usize) -> u32 {
    let mut result = 0u32;
    let mut shift = 0u32;
    for _ in 0..5 {
        let Some(&b) = code.get(*pos) else { break };
        *pos += 1;
        result |= u32::from(b & 0x7F) << shift;
        if (b & 0x80) == 0 {
            break;
        }
        shift += 7;
    }
    result
}

pub(super) fn compute_avm2_method_metrics(
    code: &[u8],
    param_count: u32,
    pool: &AbcPoolBuilder,
    exceptions: &[AbcExceptionSpec],
) -> (u32, u32, u32) {
    let mut cur_stack = 0i32;
    let mut max_stack = 1i32;
    let mut cur_scope = 0i32;
    let mut max_scope = 1i32;
    let mut max_local = param_count;
    let mut pos = 0usize;
    let catch_targets: HashSet<usize> = exceptions.iter().map(|e| e.target as usize).collect();
    let is_mn_l = |mn: u32| -> i32 {
        i32::from(
            pool.multinames
                .get((mn as usize).wrapping_sub(1))
                .is_some_and(|&(k, _, _)| k == 0x1B || k == 0x1C),
        )
    };
    while pos < code.len() {
        if catch_targets.contains(&pos) {
            cur_stack = cur_stack.max(1);
            max_stack = max_stack.max(cur_stack);
            cur_scope = 0;
        }
        let op = code[pos];
        pos += 1;
        match op {
            0x02 | 0x09 | 0x47 => {
                // nop / label / returnvoid
            }
            0x06 => {
                // dxns u30
                let _ = read_abc_u30(code, &mut pos);
            }
            0x1D => {
                // popscope
                cur_scope = (cur_scope - 1).max(0);
            }
            0x1C | 0x30 => {
                // pushwith / pushscope
                cur_stack = (cur_stack - 1).max(0);
                cur_scope += 1;
                max_scope = max_scope.max(cur_scope);
            }
            0x03 | 0x07 | 0x29 | 0x48 => {
                // throw / dxnslate / pop / returnvalue
                cur_stack = (cur_stack - 1).max(0);
            }
            0x04 => {
                // getsuper u30
                let mn = read_abc_u30(code, &mut pos);
                cur_stack = (cur_stack - is_mn_l(mn)).max(1);
                max_stack = max_stack.max(cur_stack);
            }
            0x05 => {
                // setsuper u30
                let mn = read_abc_u30(code, &mut pos);
                cur_stack = (cur_stack - 2 - is_mn_l(mn)).max(0);
            }
            0x08 => {
                // kill u30
                let reg = read_abc_u30(code, &mut pos);
                max_local = max_local.max(reg);
            }
            0x10 => {
                // jump s24
                pos = pos.saturating_add(3);
            }
            0x11 | 0x12 => {
                // iftrue / iffalse s24
                pos = pos.saturating_add(3);
                cur_stack = (cur_stack - 1).max(0);
            }
            0x1E | 0x23 => {
                // nextname / nextvalue
                cur_stack = (cur_stack - 1).max(1);
                max_stack = max_stack.max(cur_stack);
            }
            0x20 | 0x21 | 0x26 | 0x27 | 0x28 | 0x2A | 0x57 => {
                // pushnull / pushundefined / pushtrue / pushfalse / pushnan / dup / newactivation
                cur_stack += 1;
                max_stack = max_stack.max(cur_stack);
            }
            0x2B => {
                // swap
                max_stack = max_stack.max(cur_stack.max(2));
            }
            0x24 | 0x65 => {
                // pushbyte u8 / getscopeobject u8
                pos = pos.saturating_add(1);
                cur_stack += 1;
                max_stack = max_stack.max(cur_stack);
            }
            0x25 | 0x2C | 0x2D | 0x2E | 0x2F | 0x40 | 0x5A | 0x60 | 0x67 => {
                // pushshort / pushstring / pushint / pushuint / pushdouble / newfunction / newcatch / getlex / getouterscope
                let _ = read_abc_u30(code, &mut pos);
                cur_stack += 1;
                max_stack = max_stack.max(cur_stack);
            }
            0x5D | 0x5E => {
                // findpropstrict / findproperty
                let mn = read_abc_u30(code, &mut pos);
                cur_stack = (cur_stack + 1 - is_mn_l(mn)).max(1);
                max_stack = max_stack.max(cur_stack);
            }
            0x32 => {
                // hasnext2 u30 u30
                let r1 = read_abc_u30(code, &mut pos);
                let r2 = read_abc_u30(code, &mut pos);
                max_local = max_local.max(r1).max(r2);
                cur_stack += 1;
                max_stack = max_stack.max(cur_stack);
            }
            0x41 | 0x49 => {
                // call / constructsuper (u30 argc)
                let argc = read_abc_u30(code, &mut pos) as i32;
                if op == 0x41 {
                    cur_stack = (cur_stack - argc - 1).max(1);
                } else {
                    cur_stack = (cur_stack - argc - 1).max(0);
                }
            }
            0x42 => {
                // construct (u30 argc)
                let argc = read_abc_u30(code, &mut pos) as i32;
                cur_stack = (cur_stack - argc).max(1);
                max_stack = max_stack.max(cur_stack);
            }
            0x45 | 0x46 | 0x4A | 0x4C => {
                // callsuper / callproperty / constructprop / callproplex (u30 mn, u30 argc)
                let mn = read_abc_u30(code, &mut pos);
                let argc = read_abc_u30(code, &mut pos) as i32;
                cur_stack = (cur_stack - argc - is_mn_l(mn)).max(1);
            }
            0x4E | 0x4F => {
                // callsupervoid / callpropvoid (u30 mn, u30 argc)
                let mn = read_abc_u30(code, &mut pos);
                let argc = read_abc_u30(code, &mut pos) as i32;
                cur_stack = (cur_stack - argc - 1 - is_mn_l(mn)).max(0);
            }
            0x55 => {
                // newobject (u30 pair_count)
                let pairs = read_abc_u30(code, &mut pos) as i32;
                cur_stack = (cur_stack - 2 * pairs + 1).max(1);
                max_stack = max_stack.max(cur_stack);
            }
            0x56 => {
                // newarray (u30 argc)
                let argc = read_abc_u30(code, &mut pos) as i32;
                cur_stack = (cur_stack - argc + 1).max(1);
                max_stack = max_stack.max(cur_stack);
            }
            0x58 | 0x6C => {
                // newclass / getslot (u30)
                let _ = read_abc_u30(code, &mut pos);
                max_stack = max_stack.max(cur_stack.max(1));
            }
            0x59 | 0x66 | 0x6A => {
                // getdescendants / getproperty / deleteproperty (u30)
                let mn = read_abc_u30(code, &mut pos);
                cur_stack = (cur_stack - is_mn_l(mn)).max(1);
                max_stack = max_stack.max(cur_stack);
            }
            0x61 | 0x68 => {
                // setproperty / initproperty (u30)
                let mn = read_abc_u30(code, &mut pos);
                cur_stack = (cur_stack - 2 - is_mn_l(mn)).max(0);
            }
            0x6D => {
                // setslot (u30)
                let _ = read_abc_u30(code, &mut pos);
                cur_stack = (cur_stack - 2).max(0);
            }
            0x62 => {
                // getlocal u30
                let reg = read_abc_u30(code, &mut pos);
                max_local = max_local.max(reg);
                cur_stack += 1;
                max_stack = max_stack.max(cur_stack);
            }
            0x63 => {
                // setlocal u30
                let reg = read_abc_u30(code, &mut pos);
                max_local = max_local.max(reg);
                cur_stack = (cur_stack - 1).max(0);
            }
            0x70..=0x76 | 0x82 | 0x85 | 0x90 | 0x91 | 0x93 | 0x95 | 0x96 | 0x97 => {
                // unary in-place (incl. convert_s, esc_xelem, esc_xattr, coerce_a, coerce_s, typeof, bitnot)
                max_stack = max_stack.max(cur_stack.max(1));
            }
            0x80 | 0x86 | 0xB2 => {
                // coerce / astype / istype (u30)
                let _ = read_abc_u30(code, &mut pos);
                max_stack = max_stack.max(cur_stack.max(1));
            }
            0x87 | 0xA0..=0xB1 | 0xB3 | 0xB4 | 0xC5..=0xC7 => {
                // binary: pops 2, pushes 1 (incl. astypelate, instanceof, istypelate, in)
                cur_stack = (cur_stack - 1).max(1);
            }
            0xD0..=0xD3 => {
                let reg = u32::from(op - 0xD0);
                max_local = max_local.max(reg);
                cur_stack += 1;
                max_stack = max_stack.max(cur_stack);
            }
            0xD4..=0xD7 => {
                let reg = u32::from(op - 0xD4);
                max_local = max_local.max(reg);
                cur_stack = (cur_stack - 1).max(0);
            }
            _ => {}
        }
    }
    (
        (max_stack.max(1) + 2) as u32,
        max_local.saturating_add(1).max(1),
        max_scope.max(1) as u32,
    )
}
