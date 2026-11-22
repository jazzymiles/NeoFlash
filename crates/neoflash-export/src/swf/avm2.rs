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
        let ns_idx = self.add_namespace(0x16, pkg); // 0x16 = PackageNamespace
        let name_idx = self.add_string(name);
        let key = (0x07, ns_idx, name_idx); // 0x07 = QName
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
        let name_idx = self.add_string(name);
        let key = (0x09, name_idx, ns_set_idx); // 0x09 = CONSTANT_Multiname (name, ns_set)
        if let Some(&idx) = self.mn_map.get(&key) {
            return idx;
        }
        let idx = (self.multinames.len() + 1) as u32;
        self.multinames.push(key);
        self.mn_map.insert(key, idx);
        idx
    }

    pub(super) fn intern_multiname_l(&mut self) -> u32 {
        if let Some(idx) = self.multiname_l {
            return idx;
        }
        let pub_ns = self.add_namespace(0x16, "");
        let ns_set_idx = (self.ns_sets.len() + 1) as u32;
        self.ns_sets.push(vec![pub_ns]);
        let idx = (self.multinames.len() + 1) as u32;
        self.multinames.push((0x1B, ns_set_idx, 0)); // 0x1B = CONSTANT_MultinameL
        self.multiname_l = Some(idx);
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

#[derive(Clone, Default)]
pub(super) struct AbcClassSpec {
    pub(super) pkg: String,
    pub(super) class_name: String,
    pub(super) super_pkg: String,
    pub(super) super_name: String,
    pub(super) imported_classes: HashMap<String, String>,
    pub(super) wildcard_packages: Vec<String>,
    pub(super) ctor_params: Vec<String>,
    pub(super) has_explicit_ctor: bool,
    pub(super) has_explicit_super: bool,
    pub(super) ctor_statements: Vec<SwfStmt>,
    pub(super) instance_fields: Vec<(String, Option<SwfExpr>)>,
    pub(super) static_fields: Vec<(String, Option<SwfExpr>)>,
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

pub(super) fn parse_as3_source_to_spec_strict(
    source: &str,
    fallback_qname: &str,
    external_source: bool,
) -> Result<AbcClassSpec, String> {
    let tokens = tokenize_swf_script(source)?;
    let mut parser = SwfParser::new(tokens, true);

    let starts_with_package = parser.peek() == Some(&SwfToken::Ident("package".to_owned()));
    let has_class_tok = parser
        .tokens
        .iter()
        .any(|t| *t == SwfToken::Ident("class".to_owned()));

    if !starts_with_package && !has_class_tok {
        if external_source {
            return Err(
                "external ActionScript 3 source file must declare a package and class".to_owned(),
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
            "ActionScript 3 frame script cannot declare a package or class; move package/class definitions to an external .as source file"
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

    if parser.next() != Some(SwfToken::Ident("class".to_owned())) {
        return Err("expected 'class' declaration in ActionScript 3 source".to_owned());
    }
    let class_name = parser.expect_ident()?;
    let mut super_pkg = String::new();
    let mut super_name = "Object".to_owned();

    if parser.peek() == Some(&SwfToken::Ident("extends".to_owned())) {
        parser.pos += 1;
        let sup = parse_dotted_name(&mut parser)?;
        if sup.contains('.') {
            let (sp, sc) = split_qualified_class(&sup);
            super_pkg = sp;
            super_name = sc;
        } else if let Some(imp_pkg) = imported_classes.get(&sup) {
            super_pkg.clone_from(imp_pkg);
            super_name = sup;
        } else if matches!(
            sup.as_str(),
            "MovieClip"
                | "Sprite"
                | "SimpleButton"
                | "Shape"
                | "DisplayObject"
                | "DisplayObjectContainer"
                | "InteractiveObject"
                | "Bitmap"
        ) {
            super_pkg = "flash.display".to_owned();
            super_name = sup;
        } else if sup == "EventDispatcher" {
            super_pkg = "flash.events".to_owned();
            super_name = sup;
        } else if matches!(
            sup.as_str(),
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
            super_pkg = String::new();
            super_name = sup;
        } else {
            super_pkg.clone_from(&pkg);
            super_name = sup;
        }
    }

    if parser.peek() == Some(&SwfToken::Ident("implements".to_owned())) {
        parser.pos += 1;
        loop {
            let _ = parse_dotted_name(&mut parser)?;
            if !parser.eat(&SwfToken::Comma) {
                break;
            }
        }
    }

    parser.expect(&SwfToken::LBrace)?;

    let mut ctor_params = Vec::new();
    let mut has_explicit_ctor = false;
    let mut ctor_statements = Vec::new();
    let mut instance_fields = Vec::new();
    let mut static_fields = Vec::new();
    let mut static_methods = Vec::new();
    let mut extra_methods = Vec::new();

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
                }
                parser.pos += 1;
            } else {
                break;
            }
        }
        if parser.peek() == Some(&SwfToken::Ident("function".to_owned())) {
            parser.pos += 1;
            let fn_name = parser.expect_ident()?;
            parser.expect(&SwfToken::LParen)?;
            let mut params = Vec::new();
            if !parser.eat(&SwfToken::RParen) {
                loop {
                    let pname = parser.expect_ident()?;
                    parser.skip_type_annotation()?;
                    if parser.eat(&SwfToken::Assign) {
                        let _ = parser.parse_expr()?;
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
            parser.expect(&SwfToken::LBrace)?;
            let body = parser.parse_stmt_list(true)?;
            parser.expect(&SwfToken::RBrace)?;
            if fn_name == class_name && !is_static {
                ctor_params = params;
                has_explicit_ctor = true;
                ctor_statements.extend(body);
            } else if is_static {
                static_methods.push((fn_name, params, body));
            } else {
                extra_methods.push((fn_name, params, body));
            }
        } else if matches!(parser.peek(), Some(SwfToken::Ident(kw)) if kw == "var" || kw == "const")
        {
            let mut tmp = Vec::new();
            parser.parse_stmt_into(&mut tmp)?;
            for s in tmp {
                if let SwfStmt::VarDecl { name, init } = s {
                    if is_static {
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

    if extra_methods.is_empty() && static_methods.is_empty() {
        extra_methods.push(("init".to_owned(), Vec::new(), Vec::new()));
    }
    let has_explicit_super = stmts_contain_super_ctor(&ctor_statements);

    extract_script_imports(source, &mut imported_classes, &mut wildcard_packages);

    Ok(AbcClassSpec {
        pkg,
        class_name,
        super_pkg,
        super_name,
        imported_classes,
        wildcard_packages,
        ctor_params,
        has_explicit_ctor,
        has_explicit_super,
        ctor_statements,
        instance_fields,
        static_fields,
        static_methods,
        frame_scripts: Vec::new(),
        extra_methods,
    })
}

pub(super) struct AbcMethodSpec {
    pub(super) param_names: Vec<u32>,
    pub(super) needs_activation: bool,
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
        let idx = self.methods.len() as u32;
        self.methods.push(AbcMethodSpec {
            param_names,
            needs_activation: false,
        });
        idx
    }
}

pub(super) struct Avm2ControlFrame {
    pub(super) is_loop: bool,
    pub(super) break_patches: Vec<usize>,
    pub(super) continue_patches: Vec<usize>,
    pub(super) finally_depth: usize,
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
}

pub(super) struct Avm2LexicalEnv {
    pub(super) init_scope_depth: u32,
    pub(super) needs_activation: bool,
    pub(super) act_reg: Option<u32>,
    pub(super) next_temp_reg: u32,
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
        SwfExpr::Unary { expr, .. } => expr_has_direct_self_named_fn(expr),
        SwfExpr::Delete(target) => match target.as_ref() {
            SwfLValue::Ident(_) => false,
            SwfLValue::Member(obj, _) => expr_has_direct_self_named_fn(obj),
            SwfLValue::Index(obj, idx) => {
                expr_has_direct_self_named_fn(obj) || expr_has_direct_self_named_fn(idx)
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
    }
}

pub(super) fn stmts_have_direct_self_named_fn(stmts: &[SwfStmt]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        SwfStmt::VarDecl { init: Some(e), .. }
        | SwfStmt::Expr(e)
        | SwfStmt::Throw(e)
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

    let mut env = Avm2LexicalEnv {
        init_scope_depth,
        needs_activation,
        act_reg,
        next_temp_reg: next_reg,
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

    let (max_stack, computed_locals) =
        compute_avm2_method_metrics(&code.buf, params.len() as u32, pool, &env.exceptions);
    let local_count = computed_locals.max(env.next_temp_reg).max(1);
    let pushed_scopes = if env.needs_activation { 2 } else { 1 };
    if let Some(mspec) = state.methods.get_mut(method_idx as usize) {
        mspec.needs_activation = env.needs_activation;
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

pub(super) fn try_compile_avm2_source(
    source: &str,
    external_source: bool,
) -> Result<AbcClassSpec, String> {
    let spec = parse_as3_source_to_spec_strict(source, "Main", external_source)?;
    let mut pool = AbcPoolBuilder::default();
    let mut state = Avm2CompilerState::default();
    let empty_outer = HashMap::new();
    let chain = resolve_abc_ancestor_chain(&spec, std::slice::from_ref(&spec));
    let chain_len = chain.len() as u32;
    let cinit_scope_depth = 2 + chain_len;
    let method_scope_depth = 3 + chain_len;
    let imports = Avm2ImportCtx {
        current_pkg: spec.pkg.clone(),
        imported_classes: spec.imported_classes.clone(),
        wildcard_packages: spec.wildcard_packages.clone(),
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
    let ctor_m = state.alloc_method(ctor_p_indices);
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

    for (_, sm_params, sm_stmts) in &spec.static_methods {
        let p_indices: Vec<u32> = sm_params.iter().map(|p| pool.add_string(p)).collect();
        let m_idx = state.alloc_method(p_indices);
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
    for (_, m_params, m_stmts) in &spec.extra_methods {
        let p_indices: Vec<u32> = m_params.iter().map(|p| pool.add_string(p)).collect();
        let m_idx = state.alloc_method(p_indices);
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
) -> (u32, u32) {
    let mut cur_stack = 0i32;
    let mut max_stack = 1i32;
    let mut max_local = param_count;
    let mut pos = 0usize;
    let catch_targets: HashSet<usize> = exceptions.iter().map(|e| e.target as usize).collect();
    let is_mn_l = |mn: u32| -> i32 { i32::from(pool.multiname_l == Some(mn)) };
    while pos < code.len() {
        if catch_targets.contains(&pos) {
            cur_stack = cur_stack.max(1);
            max_stack = max_stack.max(cur_stack);
        }
        let op = code[pos];
        pos += 1;
        match op {
            0x02 | 0x09 | 0x1D | 0x47 => {
                // nop / label / popscope / returnvoid
            }
            0x03 | 0x29 | 0x30 | 0x48 => {
                // throw / pop / pushscope / returnvalue
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
            0x66 | 0x6A => {
                // getproperty / deleteproperty (u30)
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
            0x70 | 0x73..=0x76 | 0x82 | 0x85 | 0x90 | 0x91 | 0x93 | 0x95 | 0x96 | 0x97 => {
                // unary in-place (incl. convert_s, coerce_a, coerce_s, typeof, bitnot)
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
    )
}
