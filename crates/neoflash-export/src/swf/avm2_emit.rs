//! AVM2 expression, call, lvalue, closure, and statement bytecode emitters.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn compile_avm2_closure_value(
    pool: &mut AbcPoolBuilder,
    self_name: Option<&str>,
    params: &[String],
    opt_defaults: &[AbcOptDefault],
    rest_param: Option<String>,
    body: &[SwfStmt],
    code: &mut SwfBitWriter,
    state: &mut Avm2CompilerState,
    env: &mut Avm2LexicalEnv,
) -> Result<(), String> {
    let param_name_indices: Vec<u32> = params.iter().map(|p| pool.add_string(p)).collect();
    let encoded_opts: Vec<(u32, u8)> = opt_defaults.iter().map(|d| d.encode(pool)).collect();
    let m_idx = state.alloc_method_sig(param_name_indices, encoded_opts, rest_param);

    let mut child_outer = env.outer_slots.clone();
    if env.needs_activation {
        let own_outer_idx = env.init_scope_depth + 1;
        for (vname, &slot_id) in &env.own_slots {
            child_outer.insert(vname.clone(), (own_outer_idx, slot_id));
        }
    }

    let mut self_slot_to_bind = None;
    if let Some(sn) = self_name
        && !sn.is_empty()
        && !params.iter().any(|p| p == sn)
    {
        let mut d_refs = HashSet::new();
        let mut c_free = HashSet::new();
        collect_refs_and_child_free_in_stmts(body, &mut d_refs, &mut c_free);
        if d_refs.contains(sn) || c_free.contains(sn) {
            let qn = pool.add_qname("", sn);
            let slot_id = if let Some(&(_, existing_slot)) =
                env.activation_traits.iter().find(|&&(q, _)| q == qn)
            {
                existing_slot
            } else {
                let sid = (env.activation_traits.len() + 1) as u32;
                env.activation_traits.push((qn, sid));
                sid
            };
            child_outer.insert(sn.to_owned(), (env.init_scope_depth + 1, slot_id));
            self_slot_to_bind = Some(slot_id);
        }
    }

    let child_init_scope = env.init_scope_depth
        + if env.needs_activation { 2 } else { 1 }
        + (env.active_catch_regs.len() as u32);
    compile_avm2_scoped_unit(
        pool,
        state,
        m_idx,
        params,
        body,
        &child_outer,
        child_init_scope,
        true,
        &env.imports,
        None,
        |_, _| Ok(()),
    )?;

    code.u8(0x40); // newfunction
    code.u32_v(m_idx);
    if let Some(slot_id) = self_slot_to_bind {
        code.u8(0x2A); // dup
        code.u8(0x65); // getscopeobject 1
        code.u8(0x01);
        code.u8(0x2B); // swap
        code.u8(0x6D); // setslot
        code.u32_v(slot_id);
    }
    Ok(())
}

pub(super) fn emit_avm2_load_ident(
    pool: &mut AbcPoolBuilder,
    name: &str,
    code: &mut SwfBitWriter,
    env: &Avm2LexicalEnv,
) {
    if name == "this" || name == "_root" {
        code.u8(0xD0); // getlocal_0
    } else if let Some(&reg) = env.local_regs.get(name) {
        emit_avm2_getlocal(code, reg);
    } else if let Some(&slot_id) = env.own_slots.get(name) {
        code.u8(0x65); // getscopeobject 1
        code.u8(0x01);
        code.u8(0x6C); // getslot
        code.u32_v(slot_id);
    } else if let Some(&(outer_idx, slot_id)) = env.outer_slots.get(name) {
        code.u8(0x67); // getouterscope
        code.u32_v(outer_idx);
        code.u8(0x6C); // getslot
        code.u32_v(slot_id);
    } else {
        let mn = env.resolve_ident_multiname(pool, name);
        code.u8(0x60); // getlex
        code.u32_v(mn);
    }
}

fn dotted_member_chain(expr: &SwfExpr) -> Option<Vec<String>> {
    let mut rev_parts = Vec::new();
    let mut cur = expr;
    loop {
        if rev_parts.len() > MAX_SWF_AST_DEPTH {
            return None;
        }
        match cur {
            SwfExpr::Ident(id) => {
                rev_parts.push(id.clone());
                rev_parts.reverse();
                return Some(rev_parts);
            }
            SwfExpr::Member(inner, prop) => {
                rev_parts.push(prop.clone());
                cur = inner;
            }
            _ => return None,
        }
    }
}

fn resolve_avm2_member_multiname(
    pool: &mut AbcPoolBuilder,
    env: &Avm2LexicalEnv,
    prop: &str,
    is_attr: bool,
) -> u32 {
    if is_attr {
        if env.uses_dxns || !env.imports.wildcard_packages.is_empty() {
            let ns_set = env.open_package_ns_set(pool);
            pool.add_multiname_attr(prop, ns_set)
        } else {
            pool.add_qname_attr("", prop)
        }
    } else if env.imports.private_methods.contains(prop) && !env.imports.private_ns_name.is_empty()
    {
        let pub_ns = pool.add_namespace(0x16, "");
        let priv_ns = pool.add_namespace(0x05, &env.imports.private_ns_name);
        let ns_set = pool.add_ns_set(vec![pub_ns, priv_ns]);
        pool.add_multiname(prop, ns_set)
    } else if env.uses_dxns {
        let ns_set = env.open_package_ns_set(pool);
        pool.add_multiname(prop, ns_set)
    } else {
        pool.add_qname("", prop)
    }
}

fn try_resolve_static_avm2_type_multiname(
    pool: &mut AbcPoolBuilder,
    rhs: &SwfExpr,
    env: &Avm2LexicalEnv,
) -> Option<u32> {
    if matches!(rhs, SwfExpr::Ident(id) if id == "*") {
        return Some(0);
    }
    let parts = dotted_member_chain(rhs)?;
    let root = &parts[0];
    if matches!(root.as_str(), "this" | "_root" | "super")
        || env.local_regs.contains_key(root)
        || env.own_slots.contains_key(root)
        || env.outer_slots.contains_key(root)
        || env.declared_names.contains(root)
    {
        return None;
    }
    if parts.len() >= 2 {
        let cls_name = parts.last().cloned().unwrap_or_default();
        if !cls_name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase())
        {
            return None;
        }
        let pkg = parts[..parts.len() - 1].join(".");
        return Some(pool.add_qname(&pkg, &cls_name));
    }
    let is_known_static_type = matches!(root.as_str(), "int" | "uint")
        || env.imports.imported_classes.contains_key(root)
        || root.chars().next().is_some_and(|c| c.is_ascii_uppercase());
    if is_known_static_type {
        Some(env.resolve_type_multiname(pool, root))
    } else {
        None
    }
}

fn compile_avm2_type_rhs(
    pool: &mut AbcPoolBuilder,
    rhs: &SwfExpr,
    code: &mut SwfBitWriter,
    state: &mut Avm2CompilerState,
    env: &mut Avm2LexicalEnv,
) -> Result<(), String> {
    if let Some(parts) = dotted_member_chain(rhs) {
        let root = &parts[0];
        if !matches!(root.as_str(), "this" | "_root" | "super")
            && !env.local_regs.contains_key(root)
            && !env.own_slots.contains_key(root)
            && !env.outer_slots.contains_key(root)
        {
            let mn = if parts.len() >= 2 {
                let cls_name = parts.last().cloned().unwrap_or_default();
                let pkg = parts[..parts.len() - 1].join(".");
                pool.add_qname(&pkg, &cls_name)
            } else {
                env.resolve_type_multiname(pool, root)
            };
            code.u8(0x60); // getlex
            code.u32_v(mn);
            return Ok(());
        }
    }
    compile_avm2_expr(pool, rhs, code, state, env)
}

pub(super) fn compile_avm2_expr(
    pool: &mut AbcPoolBuilder,
    expr: &SwfExpr,
    code: &mut SwfBitWriter,
    state: &mut Avm2CompilerState,
    env: &mut Avm2LexicalEnv,
) -> Result<(), String> {
    if env.compile_depth >= MAX_SWF_AST_DEPTH {
        return Err(format!(
            "expression exceeds maximum compilation depth ({MAX_SWF_AST_DEPTH}) in bounded SWF compiler"
        ));
    }
    env.compile_depth += 1;
    let res = compile_avm2_expr_inner(pool, expr, code, state, env);
    env.compile_depth -= 1;
    res
}

fn compile_avm2_expr_inner(
    pool: &mut AbcPoolBuilder,
    expr: &SwfExpr,
    code: &mut SwfBitWriter,
    state: &mut Avm2CompilerState,
    env: &mut Avm2LexicalEnv,
) -> Result<(), String> {
    match expr {
        SwfExpr::Function {
            name,
            params,
            opt_defaults,
            rest_param,
            body,
        } => {
            compile_avm2_closure_value(
                pool,
                name.as_deref(),
                params,
                opt_defaults,
                rest_param.clone(),
                body,
                code,
                state,
                env,
            )?;
        }
        SwfExpr::Null => code.u8(0x20),        // pushnull
        SwfExpr::Undefined => code.u8(0x21),   // pushundefined
        SwfExpr::Bool(true) => code.u8(0x26),  // pushtrue
        SwfExpr::Bool(false) => code.u8(0x27), // pushfalse
        SwfExpr::Int(iv) => emit_avm2_int(pool, code, *iv),
        SwfExpr::Float(fv) => {
            let d_idx = pool.add_double(*fv);
            code.u8(0x2F); // pushdouble
            code.u32_v(d_idx);
        }
        SwfExpr::Str(s) => {
            let s_idx = pool.add_string(s);
            code.u8(0x2C); // pushstring
            code.u32_v(s_idx);
        }
        SwfExpr::Ident(name) => {
            emit_avm2_load_ident(pool, name, code, env);
        }
        SwfExpr::ArrayLit(elems) => {
            for el in elems {
                compile_avm2_expr(pool, el, code, state, env)?;
            }
            code.u8(0x56); // newarray
            code.u32_v(elems.len() as u32);
        }
        SwfExpr::ObjectLit(props) => {
            for (key, val) in props {
                let k_idx = pool.add_string(key);
                code.u8(0x2C); // pushstring
                code.u32_v(k_idx);
                compile_avm2_expr(pool, val, code, state, env)?;
            }
            code.u8(0x55); // newobject
            code.u32_v(props.len() as u32);
        }
        SwfExpr::Member(obj, prop) => {
            if matches!(obj.as_ref(), SwfExpr::Ident(id) if id == "super") {
                code.u8(0xD0); // getlocal_0
                let mn = pool.add_qname("", prop);
                code.u8(0x04); // getsuper
                code.u32_v(mn);
            } else {
                compile_avm2_expr(pool, obj, code, state, env)?;
                let mn = resolve_avm2_member_multiname(pool, env, prop, false);
                code.u8(0x66); // getproperty
                code.u32_v(mn);
            }
        }
        SwfExpr::Index(obj, idx) => {
            let super_ref = matches!(obj.as_ref(), SwfExpr::Ident(id) if id == "super");
            let get_op = if super_ref { 0x04 } else { 0x66 }; // getsuper / getproperty
            if super_ref {
                code.u8(0xD0); // getlocal_0
            } else {
                compile_avm2_expr(pool, obj, code, state, env)?;
            }
            if let SwfExpr::Int(iv) = idx.as_ref() {
                let mn = pool.add_qname("", &iv.to_string());
                code.u8(get_op);
                code.u32_v(mn);
            } else if let SwfExpr::Str(s) = idx.as_ref() {
                let mn = pool.add_qname("", s);
                code.u8(get_op);
                code.u32_v(mn);
            } else {
                compile_avm2_expr(pool, idx, code, state, env)?;
                let mn_l = pool.intern_multiname_l();
                code.u8(get_op);
                code.u32_v(mn_l);
            }
        }
        SwfExpr::XmlLit { is_list, pieces } => {
            let xml_cls_mn = pool.add_qname("", if *is_list { "XMLList" } else { "XML" });
            code.u8(0x60); // getlex XML / XMLList
            code.u32_v(xml_cls_mn);
            if pieces.is_empty() {
                let empty_idx = pool.add_string("");
                code.u8(0x2C); // pushstring ""
                code.u32_v(empty_idx);
            } else {
                for (i, piece) in pieces.iter().enumerate() {
                    match piece {
                        SwfXmlCompiledPiece::Raw(raw) => {
                            let s_idx = pool.add_string(raw);
                            code.u8(0x2C); // pushstring
                            code.u32_v(s_idx);
                        }
                        SwfXmlCompiledPiece::ElemExpr(inner) => {
                            compile_avm2_expr(pool, inner, code, state, env)?;
                            code.u8(0x71); // esc_xelem
                        }
                        SwfXmlCompiledPiece::AttrExpr(inner) => {
                            compile_avm2_expr(pool, inner, code, state, env)?;
                            code.u8(0x72); // esc_xattr
                        }
                    }
                    if i > 0 {
                        code.u8(0xA0); // add
                    }
                }
            }
            code.u8(0x42); // construct 1
            code.u32_v(1);
        }
        SwfExpr::XmlAttr { object, attr } => {
            compile_avm2_expr(pool, object, code, state, env)?;
            match attr {
                SwfXmlSelector::Named(name) => {
                    let mn = resolve_avm2_member_multiname(pool, env, name, true);
                    code.u8(0x66); // getproperty (QNameA / MultinameA)
                    code.u32_v(mn);
                }
                SwfXmlSelector::Computed(idx) => {
                    compile_avm2_expr(pool, idx, code, state, env)?;
                    let mn_la = pool.intern_multiname_la();
                    code.u8(0x66); // getproperty (MultinameLA)
                    code.u32_v(mn_la);
                }
            }
        }
        SwfExpr::XmlDescendants {
            object,
            target,
            is_attr,
        } => {
            compile_avm2_expr(pool, object, code, state, env)?;
            match target {
                SwfXmlSelector::Named(name) => {
                    let mn = if *is_attr {
                        let ns_set = env.open_package_ns_set(pool);
                        pool.add_multiname_attr(name, ns_set)
                    } else {
                        resolve_avm2_member_multiname(pool, env, name, false)
                    };
                    code.u8(0x59); // getdescendants
                    code.u32_v(mn);
                }
                SwfXmlSelector::Computed(idx) => {
                    compile_avm2_expr(pool, idx, code, state, env)?;
                    let mn = if *is_attr {
                        pool.intern_multiname_la()
                    } else {
                        pool.intern_multiname_l()
                    };
                    code.u8(0x59); // getdescendants (MultinameL / MultinameLA)
                    code.u32_v(mn);
                }
            }
        }
        SwfExpr::Unary { op, expr } => {
            compile_avm2_expr(pool, expr, code, state, env)?;
            match op {
                SwfUnaryOp::Not => code.u8(0x96),    // not
                SwfUnaryOp::Neg => code.u8(0x90),    // negate
                SwfUnaryOp::Pos => code.u8(0x75),    // convert_d
                SwfUnaryOp::BitNot => code.u8(0x97), // bitnot
                SwfUnaryOp::Typeof => code.u8(0x95), // typeof
            }
        }
        SwfExpr::Delete(target) => match target.as_ref() {
            SwfLValue::Ident(name) => {
                if env.local_regs.contains_key(name)
                    || env.own_slots.contains_key(name)
                    || env.outer_slots.contains_key(name)
                {
                    code.u8(0x27); // pushfalse (cannot delete lexical binding)
                } else {
                    let mn = pool.add_qname("", name);
                    code.u8(0x5E); // findproperty
                    code.u32_v(mn);
                    code.u8(0x6A); // deleteproperty
                    code.u32_v(mn);
                }
            }
            SwfLValue::Member(obj, prop) => {
                compile_avm2_expr(pool, obj, code, state, env)?;
                let mn = resolve_avm2_member_multiname(pool, env, prop, false);
                code.u8(0x6A); // deleteproperty
                code.u32_v(mn);
            }
            SwfLValue::Index(obj, idx) => {
                if let SwfExpr::Int(iv) = idx.as_ref() {
                    compile_avm2_expr(pool, obj, code, state, env)?;
                    let mn = pool.add_qname("", &iv.to_string());
                    code.u8(0x6A); // deleteproperty
                    code.u32_v(mn);
                } else if let SwfExpr::Str(s) = idx.as_ref() {
                    compile_avm2_expr(pool, obj, code, state, env)?;
                    let mn = pool.add_qname("", s);
                    code.u8(0x6A); // deleteproperty
                    code.u32_v(mn);
                } else {
                    compile_avm2_expr(pool, obj, code, state, env)?;
                    compile_avm2_expr(pool, idx, code, state, env)?;
                    let mn_l = pool.intern_multiname_l();
                    code.u8(0x6A); // deleteproperty (MultinameL)
                    code.u32_v(mn_l);
                }
            }
            SwfLValue::XmlAttr { object, attr } => {
                compile_avm2_expr(pool, object, code, state, env)?;
                match attr {
                    SwfXmlSelector::Named(name) => {
                        let mn = resolve_avm2_member_multiname(pool, env, name, true);
                        code.u8(0x6A); // deleteproperty (QNameA / MultinameA)
                        code.u32_v(mn);
                    }
                    SwfXmlSelector::Computed(idx) => {
                        compile_avm2_expr(pool, idx, code, state, env)?;
                        let mn_la = pool.intern_multiname_la();
                        code.u8(0x6A); // deleteproperty (MultinameLA)
                        code.u32_v(mn_la);
                    }
                }
            }
        },
        SwfExpr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            compile_avm2_expr(pool, cond, code, state, env)?;
            let else_patch = emit_avm2_s24_placeholder(code, 0x12); // iffalse
            compile_avm2_expr(pool, then_expr, code, state, env)?;
            code.u8(0x82); // coerce_a
            let end_patch = emit_avm2_s24_placeholder(code, 0x10); // jump
            let else_pos = code.buf.len();
            patch_avm2_s24(code, else_patch, else_pos)?;
            compile_avm2_expr(pool, else_expr, code, state, env)?;
            code.u8(0x82); // coerce_a
            let end_pos = code.buf.len();
            patch_avm2_s24(code, end_patch, end_pos)?;
        }
        SwfExpr::Binary { op, lhs, rhs } => {
            if matches!(op, SwfBinaryOp::And | SwfBinaryOp::Or) {
                compile_avm2_expr(pool, lhs, code, state, env)?;
                code.u8(0x82); // coerce_a
                code.u8(0x2A); // dup
                let skip_patch = emit_avm2_s24_placeholder(
                    code,
                    if *op == SwfBinaryOp::And {
                        0x12 // iffalse
                    } else {
                        0x11 // iftrue
                    },
                );
                code.u8(0x29); // pop
                compile_avm2_expr(pool, rhs, code, state, env)?;
                code.u8(0x82); // coerce_a
                let end_pos = code.buf.len();
                patch_avm2_s24(code, skip_patch, end_pos)?;
                return Ok(());
            }
            if matches!(op, SwfBinaryOp::Is | SwfBinaryOp::As)
                && let Some(type_mn) = try_resolve_static_avm2_type_multiname(pool, rhs, env)
            {
                compile_avm2_expr(pool, lhs, code, state, env)?;
                code.u8(if *op == SwfBinaryOp::Is {
                    0xB2 // istype
                } else {
                    0x86 // astype
                });
                code.u32_v(type_mn);
                return Ok(());
            }
            compile_avm2_expr(pool, lhs, code, state, env)?;
            if matches!(
                op,
                SwfBinaryOp::Is | SwfBinaryOp::As | SwfBinaryOp::InstanceOf
            ) {
                compile_avm2_type_rhs(pool, rhs, code, state, env)?;
            } else {
                compile_avm2_expr(pool, rhs, code, state, env)?;
            }
            match op {
                SwfBinaryOp::Add => code.u8(0xA0),    // add
                SwfBinaryOp::Sub => code.u8(0xA1),    // subtract
                SwfBinaryOp::Mul => code.u8(0xA2),    // multiply
                SwfBinaryOp::Div => code.u8(0xA3),    // divide
                SwfBinaryOp::Mod => code.u8(0xA4),    // modulo
                SwfBinaryOp::Shl => code.u8(0xA5),    // lshift
                SwfBinaryOp::Shr => code.u8(0xA6),    // rshift
                SwfBinaryOp::UShr => code.u8(0xA7),   // urshift
                SwfBinaryOp::BitAnd => code.u8(0xA8), // bitand
                SwfBinaryOp::BitOr => code.u8(0xA9),  // bitor
                SwfBinaryOp::BitXor => code.u8(0xAA), // bitxor
                SwfBinaryOp::Eq => code.u8(0xAB),     // equals
                SwfBinaryOp::Ne => {
                    code.u8(0xAB); // equals
                    code.u8(0x96); // not
                }
                SwfBinaryOp::StrictEq => code.u8(0xAC), // strictequals
                SwfBinaryOp::StrictNe => {
                    code.u8(0xAC); // strictequals
                    code.u8(0x96); // not
                }
                SwfBinaryOp::Lt => code.u8(0xAD), // lessthan
                SwfBinaryOp::Le => code.u8(0xAE), // lessequals
                SwfBinaryOp::Gt => code.u8(0xAF), // greaterthan
                SwfBinaryOp::Ge => code.u8(0xB0), // greaterequals
                SwfBinaryOp::InstanceOf => code.u8(0xB1), // instanceof
                SwfBinaryOp::Is => code.u8(0xB3), // istypelate
                SwfBinaryOp::As => code.u8(0x87), // astypelate
                SwfBinaryOp::In => code.u8(0xB4), // in
                SwfBinaryOp::And | SwfBinaryOp::Or => unreachable!("handled above"),
            }
        }
        SwfExpr::Call { callee, args } => {
            compile_avm2_call(pool, callee, args, false, code, state, env)?;
        }
        SwfExpr::New { callee, args } => {
            let Some(parts) = dotted_member_chain(callee) else {
                return Err("unsupported 'new' constructor target in AVM2".to_owned());
            };
            if parts.len() == 1
                && (env.local_regs.contains_key(&parts[0])
                    || env.own_slots.contains_key(&parts[0])
                    || env.outer_slots.contains_key(&parts[0]))
            {
                emit_avm2_load_ident(pool, &parts[0], code, env);
                for a in args {
                    compile_avm2_expr(pool, a, code, state, env)?;
                }
                code.u8(0x42); // construct
                code.u32_v(args.len() as u32);
            } else {
                let mn = if parts.len() >= 2 {
                    let cname = parts.last().cloned().unwrap_or_default();
                    let p = parts[..parts.len() - 1].join(".");
                    pool.add_qname(&p, &cname)
                } else {
                    env.resolve_type_multiname(pool, &parts[0])
                };
                code.u8(0x5D); // findpropstrict
                code.u32_v(mn);
                for a in args {
                    compile_avm2_expr(pool, a, code, state, env)?;
                }
                code.u8(0x4A); // constructprop
                code.u32_v(mn);
                code.u32_v(args.len() as u32);
            }
        }
    }
    Ok(())
}

pub(super) fn compile_avm2_call(
    pool: &mut AbcPoolBuilder,
    callee: &SwfExpr,
    args: &[SwfExpr],
    discard_result: bool,
    code: &mut SwfBitWriter,
    state: &mut Avm2CompilerState,
    env: &mut Avm2LexicalEnv,
) -> Result<(), String> {
    if matches!(callee, SwfExpr::Ident(id) if id == "super") {
        code.u8(0xD0); // getlocal_0
        for a in args {
            compile_avm2_expr(pool, a, code, state, env)?;
        }
        code.u8(0x49); // constructsuper
        code.u32_v(args.len() as u32);
        if let Some(post) = env.post_super.take() {
            if !post.frame_bindings.is_empty() {
                code.u8(0x5D); // findpropstrict
                code.u32_v(post.add_frame_script_mn);
                for (frame_idx, fn_mn) in &post.frame_bindings {
                    emit_avm2_int(
                        pool,
                        code,
                        i32::try_from(*frame_idx).expect("SWF preflight bounds AVM2 frame indexes"),
                    );
                    code.u8(0x60); // getlex
                    code.u32_v(*fn_mn);
                }
                code.u8(0x4F); // callpropvoid
                code.u32_v(post.add_frame_script_mn);
                code.u32_v((post.frame_bindings.len() * 2) as u32);
            }
            let init_res = compile_avm2_stmt_list(pool, &post.field_stmts, code, state, env);
            env.post_super = Some(post);
            init_res?;
        }
        if !discard_result {
            code.u8(0x21); // pushundefined
        }
        return Ok(());
    }

    if let SwfExpr::Member(recv, method_name) = callee
        && matches!(recv.as_ref(), SwfExpr::Ident(id) if id == "super")
    {
        code.u8(0xD0); // getlocal_0
        for a in args {
            compile_avm2_expr(pool, a, code, state, env)?;
        }
        let mn = pool.add_qname("", method_name);
        code.u8(if discard_result { 0x4E } else { 0x45 }); // callsupervoid / callsuper
        code.u32_v(mn);
        code.u32_v(args.len() as u32);
        return Ok(());
    }

    if let SwfExpr::Ident(name) = callee
        && (env.local_regs.contains_key(name)
            || env.own_slots.contains_key(name)
            || env.outer_slots.contains_key(name))
    {
        emit_avm2_load_ident(pool, name, code, env);
        code.u8(0xD0); // getlocal_0 (receiver)
        for a in args {
            compile_avm2_expr(pool, a, code, state, env)?;
        }
        code.u8(0x41); // call
        code.u32_v(args.len() as u32);
        if discard_result {
            code.u8(0x29); // pop
        }
        return Ok(());
    }

    let direct_fn = match callee {
        SwfExpr::Ident(name) => Some(name.as_str()),
        SwfExpr::Member(recv, prop) if matches!(recv.as_ref(), SwfExpr::Ident(id) if id == "this" || id == "_root") => {
            Some(prop.as_str())
        }
        _ => None,
    };

    if let Some(short_fn) = direct_fn {
        let is_builtin = matches!(
            short_fn,
            "stop"
                | "play"
                | "nextFrame"
                | "prevFrame"
                | "stopAllSounds"
                | "gotoAndStop"
                | "gotoAndPlay"
                | "addFrameScript"
                | "addChild"
                | "removeChild"
                | "getChildByName"
                | "trace"
                | "int"
                | "uint"
                | "Number"
                | "String"
                | "Boolean"
                | "XML"
                | "XMLList"
                | "parseInt"
                | "parseFloat"
                | "isNaN"
                | "isFinite"
        );
        if is_builtin {
            let fn_mn = pool.add_qname("", short_fn);
            code.u8(0x5D); // findpropstrict
            code.u32_v(fn_mn);
            let effective_args =
                if matches!(short_fn, "gotoAndStop" | "gotoAndPlay") && args.len() > 2 {
                    &args[..2]
                } else {
                    args
                };
            for a in effective_args {
                compile_avm2_expr(pool, a, code, state, env)?;
            }
            code.u8(if discard_result { 0x4F } else { 0x46 }); // callpropvoid / callproperty
            code.u32_v(fn_mn);
            code.u32_v(effective_args.len() as u32);
            return Ok(());
        }

        let fn_mn = resolve_avm2_member_multiname(pool, env, short_fn, false);
        code.u8(0x60); // getlex
        code.u32_v(fn_mn);
        code.u8(0xD0); // getlocal_0 (receiver)
        for a in args {
            compile_avm2_expr(pool, a, code, state, env)?;
        }
        code.u8(0x41); // call
        code.u32_v(args.len() as u32);
        if discard_result {
            code.u8(0x29); // pop
        }
        return Ok(());
    }

    if let SwfExpr::Member(recv, method_name) = callee {
        compile_avm2_expr(pool, recv, code, state, env)?;
        for a in args {
            compile_avm2_expr(pool, a, code, state, env)?;
        }
        let mn = resolve_avm2_member_multiname(pool, env, method_name, false);
        code.u8(if discard_result { 0x4F } else { 0x46 }); // callpropvoid / callproperty
        code.u32_v(mn);
        code.u32_v(args.len() as u32);
        return Ok(());
    }

    compile_avm2_expr(pool, callee, code, state, env)?;
    code.u8(0xD0); // getlocal_0 (receiver)
    for a in args {
        compile_avm2_expr(pool, a, code, state, env)?;
    }
    code.u8(0x41); // call
    code.u32_v(args.len() as u32);
    if discard_result {
        code.u8(0x29); // pop
    }
    Ok(())
}

pub(super) fn compile_avm2_store_lvalue_with_rhs<F>(
    pool: &mut AbcPoolBuilder,
    target: &SwfLValue,
    code: &mut SwfBitWriter,
    state: &mut Avm2CompilerState,
    env: &mut Avm2LexicalEnv,
    read_previous: bool,
    emit_rhs: F,
) -> Result<(), String>
where
    F: FnOnce(
        &mut AbcPoolBuilder,
        &mut SwfBitWriter,
        &mut Avm2CompilerState,
        &mut Avm2LexicalEnv,
    ) -> Result<(), String>,
{
    match target {
        SwfLValue::Ident(name) => {
            if let Some(&reg) = env.local_regs.get(name) {
                if read_previous {
                    emit_avm2_getlocal(code, reg);
                }
                emit_rhs(pool, code, state, env)?;
                code.u8(0x82); // coerce_a
                emit_avm2_setlocal(code, reg);
            } else if let Some(&slot_id) = env.own_slots.get(name) {
                code.u8(0x65); // getscopeobject 1
                code.u8(0x01);
                if read_previous {
                    code.u8(0x2a);
                    code.u8(0x6c);
                    code.u32_v(slot_id);
                }
                emit_rhs(pool, code, state, env)?;
                code.u8(0x6D); // setslot
                code.u32_v(slot_id);
            } else if let Some(&(outer_idx, slot_id)) = env.outer_slots.get(name) {
                code.u8(0x67); // getouterscope
                code.u32_v(outer_idx);
                if read_previous {
                    code.u8(0x2a);
                    code.u8(0x6c);
                    code.u32_v(slot_id);
                }
                emit_rhs(pool, code, state, env)?;
                code.u8(0x6D); // setslot
                code.u32_v(slot_id);
            } else {
                let prop_mn = pool.add_qname("", name);
                code.u8(0x5E); // findproperty
                code.u32_v(prop_mn);
                if read_previous {
                    code.u8(0x2a);
                    code.u8(0x66);
                    code.u32_v(prop_mn);
                }
                emit_rhs(pool, code, state, env)?;
                code.u8(0x61); // setproperty
                code.u32_v(prop_mn);
            }
        }
        SwfLValue::Member(obj, _) | SwfLValue::Index(obj, _) => {
            let super_ref = matches!(obj.as_ref(), SwfExpr::Ident(id) if id == "super");
            if super_ref {
                code.u8(0xD0); // getlocal_0
            } else {
                compile_avm2_expr(pool, obj, code, state, env)?;
            }
            if read_previous {
                code.u8(0x2a); // dup receiver for the read
            }
            let property = match target {
                SwfLValue::Member(_, name) => {
                    if super_ref {
                        pool.add_qname("", name)
                    } else {
                        resolve_avm2_member_multiname(pool, env, name, false)
                    }
                }
                SwfLValue::Index(_, index) => match index.as_ref() {
                    SwfExpr::Int(value) => pool.add_qname("", &value.to_string()),
                    SwfExpr::Str(name) => pool.add_qname("", name),
                    expr => {
                        compile_avm2_expr(pool, expr, code, state, env)?;
                        if read_previous {
                            let key = env.alloc_temp_reg();
                            code.u8(0x2a); // keep one key on the stack
                            emit_avm2_setlocal(code, key);
                            code.u8(0x2b); // swap key below the second receiver
                            emit_avm2_getlocal(code, key);
                        }
                        pool.intern_multiname_l()
                    }
                },
                SwfLValue::Ident(_) | SwfLValue::XmlAttr { .. } => unreachable!(),
            };
            if read_previous {
                code.u8(if super_ref { 0x04 } else { 0x66 }); // getsuper / getproperty
                code.u32_v(property);
            }
            emit_rhs(pool, code, state, env)?;
            code.u8(if super_ref { 0x05 } else { 0x61 }); // setsuper / setproperty
            code.u32_v(property);
        }
        SwfLValue::XmlAttr { object, attr } => {
            compile_avm2_expr(pool, object, code, state, env)?;
            if read_previous {
                code.u8(0x2A); // dup receiver for the read
            }
            let attr_mn = match attr {
                SwfXmlSelector::Named(name) => resolve_avm2_member_multiname(pool, env, name, true),
                SwfXmlSelector::Computed(idx) => {
                    compile_avm2_expr(pool, idx, code, state, env)?;
                    if read_previous {
                        let key = env.alloc_temp_reg();
                        code.u8(0x2A); // keep one key on the stack
                        emit_avm2_setlocal(code, key);
                        code.u8(0x2B); // swap key below the second receiver
                        emit_avm2_getlocal(code, key);
                    }
                    pool.intern_multiname_la()
                }
            };
            if read_previous {
                code.u8(0x66); // getproperty
                code.u32_v(attr_mn);
            }
            emit_rhs(pool, code, state, env)?;
            code.u8(0x61); // setproperty
            code.u32_v(attr_mn);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn compile_avm2_function_decl_binding(
    pool: &mut AbcPoolBuilder,
    name: &str,
    params: &[String],
    opt_defaults: &[AbcOptDefault],
    rest_param: Option<String>,
    body: &[SwfStmt],
    code: &mut SwfBitWriter,
    state: &mut Avm2CompilerState,
    env: &mut Avm2LexicalEnv,
) -> Result<(), String> {
    if let Some(&reg) = env.local_regs.get(name) {
        compile_avm2_closure_value(
            pool,
            None,
            params,
            opt_defaults,
            rest_param,
            body,
            code,
            state,
            env,
        )?;
        code.u8(0x82); // coerce_a
        emit_avm2_setlocal(code, reg);
    } else if let Some(&slot_id) = env.own_slots.get(name) {
        code.u8(0x65); // getscopeobject 1
        code.u8(0x01);
        compile_avm2_closure_value(
            pool,
            None,
            params,
            opt_defaults,
            rest_param,
            body,
            code,
            state,
            env,
        )?;
        code.u8(0x6D); // setslot
        code.u32_v(slot_id);
    } else {
        let fn_mn = pool.add_qname("", name);
        code.u8(0x5E); // findproperty
        code.u32_v(fn_mn);
        compile_avm2_closure_value(
            pool,
            None,
            params,
            opt_defaults,
            rest_param,
            body,
            code,
            state,
            env,
        )?;
        code.u8(0x61); // setproperty
        code.u32_v(fn_mn);
    }
    Ok(())
}

pub(super) fn compile_avm2_stmt_list(
    pool: &mut AbcPoolBuilder,
    stmts: &[SwfStmt],
    code: &mut SwfBitWriter,
    state: &mut Avm2CompilerState,
    env: &mut Avm2LexicalEnv,
) -> Result<(), String> {
    for stmt in stmts {
        if let SwfStmt::FunctionDecl {
            name,
            params,
            opt_defaults,
            rest_param,
            body,
        } = stmt
        {
            compile_avm2_function_decl_binding(
                pool,
                name,
                params,
                opt_defaults,
                rest_param.clone(),
                body,
                code,
                state,
                env,
            )?;
        }
    }
    for stmt in stmts {
        if matches!(stmt, SwfStmt::FunctionDecl { .. }) {
            continue;
        }
        compile_avm2_stmt(pool, stmt, code, state, env)?;
    }
    Ok(())
}

fn emit_avm2_pop_catch_scopes(code: &mut SwfBitWriter, cur_depth: usize, target_depth: usize) {
    for _ in 0..cur_depth.saturating_sub(target_depth) {
        code.u8(0x1D); // popscope
    }
}

fn emit_avm2_catch_prologue(
    pool: &mut AbcPoolBuilder,
    exc_idx: u32,
    cname: &str,
    code: &mut SwfBitWriter,
    env: &mut Avm2LexicalEnv,
) -> u32 {
    // On exception handler entry: stack = [thrown], scope_stack = [].
    code.u8(0xD0); // getlocal_0
    code.u8(0x30); // pushscope (restores scope 0 = this; stack remains [thrown])
    if let Some(act_reg) = env.act_reg {
        emit_avm2_getlocal(code, act_reg);
        code.u8(0x30); // pushscope (restores scope 1 = activation; stack remains [thrown])
    }
    for &enc_reg in &env.active_catch_regs {
        emit_avm2_getlocal(code, enc_reg);
        code.u8(0x30); // pushscope (restores enclosing catch scopes; stack remains [thrown])
    }
    let catch_reg = env.alloc_temp_reg();
    if cname.is_empty() {
        code.u8(0x29); // pop [thrown]
        code.u8(0x5A); // newcatch
        code.u32_v(exc_idx);
        code.u8(0x2A); // dup
        emit_avm2_setlocal(code, catch_reg);
        code.u8(0x30); // pushscope
    } else {
        code.u8(0x5A); // newcatch -> [thrown, catch_scope]
        code.u32_v(exc_idx);
        code.u8(0x2A); // dup -> [thrown, catch_scope, catch_scope]
        emit_avm2_setlocal(code, catch_reg);
        code.u8(0x2A); // dup -> [thrown, catch_scope, catch_scope]
        code.u8(0x30); // pushscope -> [thrown, catch_scope]
        code.u8(0x2B); // swap -> [catch_scope, thrown]
        if let Some(&reg) = env.local_regs.get(cname) {
            code.u8(0x2A); // dup -> [catch_scope, thrown, thrown]
            code.u8(0x82); // coerce_a
            emit_avm2_setlocal(code, reg);
        }
        code.u8(0x6D); // setslot 1 on catch_scope
        code.u32_v(1);
        if let Some(&slot_id) = env.own_slots.get(cname) {
            code.u8(0x65); // getscopeobject 1
            code.u8(0x01);
            emit_avm2_getlocal(code, catch_reg);
            code.u8(0x6C); // getslot 1
            code.u32_v(1);
            code.u8(0x6D); // setslot
            code.u32_v(slot_id);
        } else if let Some(&(outer_idx, slot_id)) = env.outer_slots.get(cname) {
            code.u8(0x67); // getouterscope
            code.u32_v(outer_idx);
            emit_avm2_getlocal(code, catch_reg);
            code.u8(0x6C); // getslot 1
            code.u32_v(1);
            code.u8(0x6D); // setslot
            code.u32_v(slot_id);
        }
    }
    let _ = pool;
    catch_reg
}

pub(super) fn compile_avm2_stmt(
    pool: &mut AbcPoolBuilder,
    stmt: &SwfStmt,
    code: &mut SwfBitWriter,
    state: &mut Avm2CompilerState,
    env: &mut Avm2LexicalEnv,
) -> Result<(), String> {
    if env.compile_depth >= MAX_SWF_PARSE_DEPTH {
        return Err(format!(
            "statement exceeds maximum compilation depth ({MAX_SWF_PARSE_DEPTH}) in bounded SWF compiler"
        ));
    }
    env.compile_depth += 1;
    let res = compile_avm2_stmt_inner(pool, stmt, code, state, env);
    env.compile_depth -= 1;
    res
}

fn compile_avm2_stmt_inner(
    pool: &mut AbcPoolBuilder,
    stmt: &SwfStmt,
    code: &mut SwfBitWriter,
    state: &mut Avm2CompilerState,
    env: &mut Avm2LexicalEnv,
) -> Result<(), String> {
    match stmt {
        SwfStmt::VarDecl { name, init } => {
            if let Some(&reg) = env.local_regs.get(name) {
                if let Some(rhs) = init {
                    compile_avm2_expr(pool, rhs, code, state, env)?;
                } else {
                    code.u8(0x21); // pushundefined
                }
                code.u8(0x82); // coerce_a
                emit_avm2_setlocal(code, reg);
            } else if let Some(&slot_id) = env.own_slots.get(name) {
                code.u8(0x65); // getscopeobject 1
                code.u8(0x01);
                if let Some(rhs) = init {
                    compile_avm2_expr(pool, rhs, code, state, env)?;
                } else {
                    code.u8(0x21); // pushundefined
                }
                code.u8(0x6D); // setslot
                code.u32_v(slot_id);
            } else {
                let prop_mn = pool.add_qname("", name);
                code.u8(0x5E); // findproperty
                code.u32_v(prop_mn);
                if let Some(rhs) = init {
                    compile_avm2_expr(pool, rhs, code, state, env)?;
                } else {
                    code.u8(0x21); // pushundefined
                }
                code.u8(0x61); // setproperty
                code.u32_v(prop_mn);
            }
        }
        SwfStmt::Assign { target, op, value } => {
            compile_avm2_store_lvalue_with_rhs(
                pool,
                target,
                code,
                state,
                env,
                *op != SwfAssignOp::Assign,
                |p, c, st, ev| {
                    if *op == SwfAssignOp::Assign {
                        compile_avm2_expr(p, value, c, st, ev)
                    } else {
                        compile_avm2_expr(p, value, c, st, ev)?;
                        c.u8(match op {
                            SwfAssignOp::AddAssign => 0xA0, // add
                            SwfAssignOp::SubAssign => 0xA1, // subtract
                            SwfAssignOp::MulAssign => 0xA2, // multiply
                            SwfAssignOp::DivAssign => 0xA3, // divide
                            SwfAssignOp::ModAssign => 0xA4, // modulo
                            SwfAssignOp::BitAndAssign => 0xa8,
                            SwfAssignOp::BitOrAssign => 0xa9,
                            SwfAssignOp::BitXorAssign => 0xaa,
                            SwfAssignOp::ShlAssign => 0xa5,
                            SwfAssignOp::ShrAssign => 0xa6,
                            SwfAssignOp::UShrAssign => 0xa7,
                            SwfAssignOp::Assign => unreachable!(),
                        });
                        Ok(())
                    }
                },
            )?;
        }
        SwfStmt::IncDec { target, increment } => {
            compile_avm2_store_lvalue_with_rhs(
                pool,
                target,
                code,
                state,
                env,
                true,
                |_, c, _, _| {
                    c.u8(if *increment { 0x91 } else { 0x93 }); // increment / decrement
                    Ok(())
                },
            )?;
        }
        SwfStmt::Expr(SwfExpr::Call { callee, args }) => {
            compile_avm2_call(pool, callee, args, true, code, state, env)?;
        }
        SwfStmt::Expr(expr) => {
            compile_avm2_expr(pool, expr, code, state, env)?;
            code.u8(0x29); // pop
        }
        SwfStmt::Block(inner) => {
            compile_avm2_stmt_list(pool, inner, code, state, env)?;
        }
        SwfStmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            compile_avm2_expr(pool, cond, code, state, env)?;
            let skip_then_patch = emit_avm2_s24_placeholder(code, 0x12); // iffalse
            compile_avm2_stmt_list(pool, then_branch, code, state, env)?;
            if let Some(else_stmts) = else_branch
                && !else_stmts.is_empty()
            {
                let skip_else_patch = emit_avm2_s24_placeholder(code, 0x10); // jump
                let else_pos = code.buf.len();
                patch_avm2_s24(code, skip_then_patch, else_pos)?;
                compile_avm2_stmt_list(pool, else_stmts, code, state, env)?;
                let end_pos = code.buf.len();
                patch_avm2_s24(code, skip_else_patch, end_pos)?;
            } else {
                let end_pos = code.buf.len();
                patch_avm2_s24(code, skip_then_patch, end_pos)?;
            }
        }
        SwfStmt::While {
            cond,
            body,
            post_test,
        } => {
            let jump_to_cond = (!post_test).then(|| emit_avm2_s24_placeholder(code, 0x10));
            let body_pos = emit_avm2_loop_label(code);
            env.flow_stack.push(Avm2ControlFrame {
                is_loop: true,
                break_patches: Vec::new(),
                continue_patches: Vec::new(),
                finally_depth: env.finally_stack.len(),
                catch_scope_depth: env.active_catch_regs.len(),
            });
            compile_avm2_stmt_list(pool, body, code, state, env)?;
            let frame = env.flow_stack.pop().expect("while control frame");
            let cond_pos = code.buf.len();
            for cp in jump_to_cond.into_iter().chain(frame.continue_patches) {
                patch_avm2_s24(code, cp, cond_pos)?;
            }
            compile_avm2_expr(pool, cond, code, state, env)?;
            let back_patch = emit_avm2_s24_placeholder(code, 0x11); // iftrue L_body
            patch_avm2_s24(code, back_patch, body_pos)?;
            let end_pos = code.buf.len();
            for bp in frame.break_patches {
                patch_avm2_s24(code, bp, end_pos)?;
            }
        }
        SwfStmt::For {
            init,
            cond,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                compile_avm2_stmt(pool, init_stmt, code, state, env)?;
            }
            let jump_to_cond = emit_avm2_s24_placeholder(code, 0x10); // jump L_cond
            let body_pos = emit_avm2_loop_label(code);
            env.flow_stack.push(Avm2ControlFrame {
                is_loop: true,
                break_patches: Vec::new(),
                continue_patches: Vec::new(),
                finally_depth: env.finally_stack.len(),
                catch_scope_depth: env.active_catch_regs.len(),
            });
            compile_avm2_stmt_list(pool, body, code, state, env)?;
            let frame = env.flow_stack.pop().expect("for control frame");
            let update_pos = code.buf.len();
            for cp in frame.continue_patches {
                patch_avm2_s24(code, cp, update_pos)?;
            }
            if let Some(upd_stmt) = update {
                compile_avm2_stmt(pool, upd_stmt, code, state, env)?;
            }
            let cond_pos = code.buf.len();
            patch_avm2_s24(code, jump_to_cond, cond_pos)?;
            if let Some(c_expr) = cond {
                compile_avm2_expr(pool, c_expr, code, state, env)?;
            } else {
                code.u8(0x26); // pushtrue
            }
            let back_patch = emit_avm2_s24_placeholder(code, 0x11); // iftrue L_body
            patch_avm2_s24(code, back_patch, body_pos)?;
            let end_pos = code.buf.len();
            for bp in frame.break_patches {
                patch_avm2_s24(code, bp, end_pos)?;
            }
        }
        SwfStmt::ForIn {
            is_each,
            is_var_decl: _,
            var_name,
            object,
            body,
        } => {
            let obj_reg = env.alloc_temp_reg();
            let idx_reg = env.alloc_temp_reg();
            compile_avm2_expr(pool, object, code, state, env)?;
            code.u8(0x82); // coerce_a
            emit_avm2_setlocal(code, obj_reg);
            code.u8(0x24); // pushbyte 0
            code.u8(0x00);
            emit_avm2_setlocal(code, idx_reg);

            let jump_to_cond = emit_avm2_s24_placeholder(code, 0x10); // jump L_cond
            let body_pos = emit_avm2_loop_label(code);
            let next_op = if *is_each { 0x23 } else { 0x1E }; // nextvalue / nextname
            let target_lv = SwfLValue::Ident(var_name.clone());
            compile_avm2_store_lvalue_with_rhs(
                pool,
                &target_lv,
                code,
                state,
                env,
                false,
                |_, c, _, _| {
                    emit_avm2_getlocal(c, obj_reg);
                    emit_avm2_getlocal(c, idx_reg);
                    c.u8(next_op);
                    Ok(())
                },
            )?;

            env.flow_stack.push(Avm2ControlFrame {
                is_loop: true,
                break_patches: Vec::new(),
                continue_patches: Vec::new(),
                finally_depth: env.finally_stack.len(),
                catch_scope_depth: env.active_catch_regs.len(),
            });
            compile_avm2_stmt_list(pool, body, code, state, env)?;
            let frame = env.flow_stack.pop().expect("for-in control frame");

            let cond_pos = code.buf.len();
            patch_avm2_s24(code, jump_to_cond, cond_pos)?;
            for cp in frame.continue_patches {
                patch_avm2_s24(code, cp, cond_pos)?;
            }
            code.u8(0x32); // hasnext2
            code.u32_v(obj_reg);
            code.u32_v(idx_reg);
            let back_patch = emit_avm2_s24_placeholder(code, 0x11); // iftrue L_body
            patch_avm2_s24(code, back_patch, body_pos)?;

            let end_pos = code.buf.len();
            for bp in frame.break_patches {
                patch_avm2_s24(code, bp, end_pos)?;
            }
            code.u8(0x08); // kill obj_reg
            code.u32_v(obj_reg);
            code.u8(0x08); // kill idx_reg
            code.u32_v(idx_reg);
        }
        SwfStmt::Switch {
            discriminant,
            cases,
        } => {
            let disc_reg = env.alloc_temp_reg();
            compile_avm2_expr(pool, discriminant, code, state, env)?;
            code.u8(0x82); // coerce_a
            emit_avm2_setlocal(code, disc_reg);

            let mut case_patches: Vec<(usize, usize)> = Vec::new();
            let mut default_idx: Option<usize> = None;
            for (idx, (case_expr_opt, _)) in cases.iter().enumerate() {
                if let Some(case_expr) = case_expr_opt {
                    emit_avm2_getlocal(code, disc_reg);
                    compile_avm2_expr(pool, case_expr, code, state, env)?;
                    code.u8(0xAC); // strictequals
                    let p = emit_avm2_s24_placeholder(code, 0x11); // iftrue
                    case_patches.push((idx, p));
                } else if default_idx.is_none() {
                    default_idx = Some(idx);
                }
            }
            let nomatch_patch = emit_avm2_s24_placeholder(code, 0x10); // jump default/end

            env.flow_stack.push(Avm2ControlFrame {
                is_loop: false,
                break_patches: Vec::new(),
                continue_patches: Vec::new(),
                finally_depth: env.finally_stack.len(),
                catch_scope_depth: env.active_catch_regs.len(),
            });

            let mut body_positions = vec![0usize; cases.len()];
            for (idx, (_, cbody)) in cases.iter().enumerate() {
                body_positions[idx] = code.buf.len();
                compile_avm2_stmt_list(pool, cbody, code, state, env)?;
            }

            let frame = env.flow_stack.pop().expect("switch control frame");
            let end_pos = code.buf.len();
            for (cidx, patch_pos) in case_patches {
                patch_avm2_s24(code, patch_pos, body_positions[cidx])?;
            }
            if let Some(didx) = default_idx {
                patch_avm2_s24(code, nomatch_patch, body_positions[didx])?;
            } else {
                patch_avm2_s24(code, nomatch_patch, end_pos)?;
            }
            for bp in frame.break_patches {
                patch_avm2_s24(code, bp, end_pos)?;
            }
            code.u8(0x08); // kill disc_reg
            code.u32_v(disc_reg);
        }
        SwfStmt::Break => {
            let Some((target_flow_idx, finally_depth)) = env
                .flow_stack
                .iter()
                .enumerate()
                .next_back()
                .map(|(i, f)| (i, f.finally_depth))
            else {
                return Err("'break' used outside of a loop or switch statement".to_owned());
            };
            if env.finally_stack.len() > finally_depth {
                let top_fin = env.finally_stack.last_mut().expect("checked finally_stack");
                emit_avm2_pop_catch_scopes(
                    code,
                    env.active_catch_regs.len(),
                    top_fin.catch_scope_depth,
                );
                let patch = emit_avm2_s24_placeholder(code, 0x10); // jump
                top_fin.pending_unwinds.push(Avm2PendingUnwind::Break {
                    jump_patch: patch,
                    target_flow_idx,
                });
            } else {
                emit_avm2_pop_catch_scopes(
                    code,
                    env.active_catch_regs.len(),
                    env.flow_stack[target_flow_idx].catch_scope_depth,
                );
                let patch = emit_avm2_s24_placeholder(code, 0x10); // jump
                env.flow_stack[target_flow_idx].break_patches.push(patch);
            }
        }
        SwfStmt::Continue => {
            let Some((loop_idx, finally_depth)) = env
                .flow_stack
                .iter()
                .enumerate()
                .rev()
                .find(|(_, f)| f.is_loop)
                .map(|(i, f)| (i, f.finally_depth))
            else {
                return Err("'continue' used outside of a loop statement".to_owned());
            };
            if env.finally_stack.len() > finally_depth {
                let top_fin = env.finally_stack.last_mut().expect("checked finally_stack");
                emit_avm2_pop_catch_scopes(
                    code,
                    env.active_catch_regs.len(),
                    top_fin.catch_scope_depth,
                );
                let patch = emit_avm2_s24_placeholder(code, 0x10); // jump
                top_fin.pending_unwinds.push(Avm2PendingUnwind::Continue {
                    jump_patch: patch,
                    target_flow_idx: loop_idx,
                });
            } else {
                emit_avm2_pop_catch_scopes(
                    code,
                    env.active_catch_regs.len(),
                    env.flow_stack[loop_idx].catch_scope_depth,
                );
                let patch = emit_avm2_s24_placeholder(code, 0x10); // jump
                env.flow_stack[loop_idx].continue_patches.push(patch);
            }
        }
        SwfStmt::Try {
            try_body,
            catches,
            finally_body,
        } => {
            let mut normal_exit_patches = Vec::new();
            let outer_from = code.buf.len();
            let base_catch_depth = env.active_catch_regs.len();
            if matches!(try_body.first(), Some(SwfStmt::Try { .. })) {
                code.u8(0x02); // nop so nested try blocks have distinct `from` offsets
            }

            if finally_body.is_some() {
                env.finally_stack.push(Avm2FinallyFrame {
                    pending_unwinds: Vec::new(),
                    catch_scope_depth: base_catch_depth,
                });
            }
            let inner_from = code.buf.len();
            compile_avm2_stmt_list(pool, try_body, code, state, env)?;
            if code.buf.len() == inner_from {
                code.u8(0x02); // nop so [from, to) is non-empty
            }
            let mut saved_fin_frame = if finally_body.is_some() {
                env.finally_stack.pop()
            } else {
                None
            };
            normal_exit_patches.push(emit_avm2_s24_placeholder(code, 0x10)); // jump past handlers
            let inner_to = code.buf.len();

            if !catches.is_empty() {
                let base_exc_idx = env.exceptions.len();
                for (cname, ctype_opt, _) in catches {
                    let exc_type = match ctype_opt.as_deref() {
                        None | Some("*") => 0,
                        Some(t) => env.resolve_type_multiname(pool, t),
                    };
                    let var_name = if cname.is_empty() {
                        0
                    } else {
                        pool.add_qname("", cname)
                    };
                    env.exceptions.push(AbcExceptionSpec {
                        from: inner_from as u32,
                        to: inner_to as u32,
                        target: 0,
                        exc_type,
                        var_name,
                    });
                }

                for (i, (cname, _, cbody)) in catches.iter().enumerate() {
                    let catch_target = code.buf.len();
                    let exc_idx = base_exc_idx + i;
                    env.exceptions[exc_idx].target = catch_target as u32;
                    let catch_reg =
                        emit_avm2_catch_prologue(pool, exc_idx as u32, cname, code, env);
                    env.active_catch_regs.push(catch_reg);
                    env.max_catch_scopes = env.max_catch_scopes.max(env.active_catch_regs.len());
                    if let Some(fin_frame) = saved_fin_frame.take() {
                        env.finally_stack.push(fin_frame);
                    }
                    compile_avm2_stmt_list(pool, cbody, code, state, env)?;
                    if finally_body.is_some() {
                        saved_fin_frame = env.finally_stack.pop();
                    }
                    env.active_catch_regs.pop();
                    code.u8(0x1D); // popscope (catch scope)
                    code.u8(0x08); // kill catch_reg
                    code.u32_v(catch_reg);
                    if i + 1 < catches.len() || finally_body.is_some() {
                        normal_exit_patches.push(emit_avm2_s24_placeholder(code, 0x10));
                    }
                }
            }

            if let Some(fbody) = finally_body {
                let outer_to = code.buf.len();
                let unwind_target = code.buf.len();
                env.exceptions.push(AbcExceptionSpec {
                    from: outer_from as u32,
                    to: outer_to as u32,
                    target: unwind_target as u32,
                    exc_type: 0,
                    var_name: 0,
                });
                let unw_reg = env.alloc_temp_reg();
                code.u8(0xD0); // getlocal_0
                code.u8(0x30); // pushscope
                if let Some(act_reg) = env.act_reg {
                    emit_avm2_getlocal(code, act_reg);
                    code.u8(0x30); // pushscope
                }
                for &enc_reg in &env.active_catch_regs {
                    emit_avm2_getlocal(code, enc_reg);
                    code.u8(0x30); // pushscope
                }
                code.u8(0x82); // coerce_a
                emit_avm2_setlocal(code, unw_reg);
                compile_avm2_stmt_list(pool, fbody, code, state, env)?;
                emit_avm2_getlocal(code, unw_reg);
                code.u8(0x03); // throw (re-throw unwound exception)

                let normal_finally_pos = code.buf.len();
                for p in normal_exit_patches {
                    patch_avm2_s24(code, p, normal_finally_pos)?;
                }
                compile_avm2_stmt_list(pool, fbody, code, state, env)?;

                let pending = saved_fin_frame
                    .map(|f| f.pending_unwinds)
                    .unwrap_or_default();
                if !pending.is_empty() {
                    let skip_trampolines = emit_avm2_s24_placeholder(code, 0x10); // jump past unwind trampolines
                    for unw in pending {
                        let tramp_pos = code.buf.len();
                        match unw {
                            Avm2PendingUnwind::ReturnVal {
                                jump_patch,
                                ret_reg,
                            } => {
                                patch_avm2_s24(code, jump_patch, tramp_pos)?;
                                compile_avm2_stmt_list(pool, fbody, code, state, env)?;
                                if let Some(parent_fin) = env.finally_stack.last_mut() {
                                    emit_avm2_pop_catch_scopes(
                                        code,
                                        base_catch_depth,
                                        parent_fin.catch_scope_depth,
                                    );
                                    let next_jmp = emit_avm2_s24_placeholder(code, 0x10);
                                    parent_fin
                                        .pending_unwinds
                                        .push(Avm2PendingUnwind::ReturnVal {
                                            jump_patch: next_jmp,
                                            ret_reg,
                                        });
                                } else {
                                    emit_avm2_pop_catch_scopes(code, base_catch_depth, 0);
                                    emit_avm2_getlocal(code, ret_reg);
                                    code.u8(0x48); // returnvalue
                                }
                            }
                            Avm2PendingUnwind::ReturnVoid { jump_patch } => {
                                patch_avm2_s24(code, jump_patch, tramp_pos)?;
                                compile_avm2_stmt_list(pool, fbody, code, state, env)?;
                                if let Some(parent_fin) = env.finally_stack.last_mut() {
                                    emit_avm2_pop_catch_scopes(
                                        code,
                                        base_catch_depth,
                                        parent_fin.catch_scope_depth,
                                    );
                                    let next_jmp = emit_avm2_s24_placeholder(code, 0x10);
                                    parent_fin.pending_unwinds.push(
                                        Avm2PendingUnwind::ReturnVoid {
                                            jump_patch: next_jmp,
                                        },
                                    );
                                } else {
                                    emit_avm2_pop_catch_scopes(code, base_catch_depth, 0);
                                    code.u8(0x47); // returnvoid
                                }
                            }
                            Avm2PendingUnwind::Break {
                                jump_patch,
                                target_flow_idx,
                            } => {
                                patch_avm2_s24(code, jump_patch, tramp_pos)?;
                                compile_avm2_stmt_list(pool, fbody, code, state, env)?;
                                let target_depth = env.flow_stack[target_flow_idx].finally_depth;
                                if env.finally_stack.len() > target_depth {
                                    let parent_fin = env
                                        .finally_stack
                                        .last_mut()
                                        .expect("checked finally_stack");
                                    emit_avm2_pop_catch_scopes(
                                        code,
                                        base_catch_depth,
                                        parent_fin.catch_scope_depth,
                                    );
                                    let next_jmp = emit_avm2_s24_placeholder(code, 0x10);
                                    parent_fin.pending_unwinds.push(Avm2PendingUnwind::Break {
                                        jump_patch: next_jmp,
                                        target_flow_idx,
                                    });
                                } else {
                                    emit_avm2_pop_catch_scopes(
                                        code,
                                        base_catch_depth,
                                        env.flow_stack[target_flow_idx].catch_scope_depth,
                                    );
                                    let next_jmp = emit_avm2_s24_placeholder(code, 0x10);
                                    env.flow_stack[target_flow_idx].break_patches.push(next_jmp);
                                }
                            }
                            Avm2PendingUnwind::Continue {
                                jump_patch,
                                target_flow_idx,
                            } => {
                                patch_avm2_s24(code, jump_patch, tramp_pos)?;
                                compile_avm2_stmt_list(pool, fbody, code, state, env)?;
                                let target_depth = env.flow_stack[target_flow_idx].finally_depth;
                                if env.finally_stack.len() > target_depth {
                                    let parent_fin = env
                                        .finally_stack
                                        .last_mut()
                                        .expect("checked finally_stack");
                                    emit_avm2_pop_catch_scopes(
                                        code,
                                        base_catch_depth,
                                        parent_fin.catch_scope_depth,
                                    );
                                    let next_jmp = emit_avm2_s24_placeholder(code, 0x10);
                                    parent_fin
                                        .pending_unwinds
                                        .push(Avm2PendingUnwind::Continue {
                                            jump_patch: next_jmp,
                                            target_flow_idx,
                                        });
                                } else {
                                    emit_avm2_pop_catch_scopes(
                                        code,
                                        base_catch_depth,
                                        env.flow_stack[target_flow_idx].catch_scope_depth,
                                    );
                                    let next_jmp = emit_avm2_s24_placeholder(code, 0x10);
                                    env.flow_stack[target_flow_idx]
                                        .continue_patches
                                        .push(next_jmp);
                                }
                            }
                        }
                    }
                    let after_trampolines = code.buf.len();
                    patch_avm2_s24(code, skip_trampolines, after_trampolines)?;
                }
            } else {
                let after_catches = code.buf.len();
                for p in normal_exit_patches {
                    patch_avm2_s24(code, p, after_catches)?;
                }
            }
        }
        SwfStmt::Throw(expr) => {
            compile_avm2_expr(pool, expr, code, state, env)?;
            code.u8(0x03); // throw
        }
        SwfStmt::DefaultXmlNamespace(ns_expr) => {
            env.uses_dxns = true;
            if let SwfExpr::Str(uri) = ns_expr {
                let s_idx = pool.add_string(uri);
                code.u8(0x06); // dxns
                code.u32_v(s_idx);
            } else {
                compile_avm2_expr(pool, ns_expr, code, state, env)?;
                code.u8(0x07); // dxnslate
            }
        }
        SwfStmt::FunctionDecl {
            name,
            params,
            opt_defaults,
            rest_param,
            body,
        } => {
            compile_avm2_function_decl_binding(
                pool,
                name,
                params,
                opt_defaults,
                rest_param.clone(),
                body,
                code,
                state,
                env,
            )?;
        }
        SwfStmt::ClassDecl { .. } => {
            return Err(
                "ActionScript 3 nested class declaration is not supported inside a method or script; move class definitions to external .as source files"
                    .to_owned(),
            );
        }
        SwfStmt::Return(expr_opt) => {
            if let Some(expr) = expr_opt {
                compile_avm2_expr(pool, expr, code, state, env)?;
                if !env.finally_stack.is_empty() {
                    let ret_reg = env.alloc_temp_reg();
                    code.u8(0x82); // coerce_a
                    emit_avm2_setlocal(code, ret_reg);
                    let top_fin = env.finally_stack.last_mut().expect("checked finally_stack");
                    emit_avm2_pop_catch_scopes(
                        code,
                        env.active_catch_regs.len(),
                        top_fin.catch_scope_depth,
                    );
                    let patch = emit_avm2_s24_placeholder(code, 0x10); // jump to post-outer_to unwind trampoline
                    top_fin.pending_unwinds.push(Avm2PendingUnwind::ReturnVal {
                        jump_patch: patch,
                        ret_reg,
                    });
                } else {
                    emit_avm2_pop_catch_scopes(code, env.active_catch_regs.len(), 0);
                    code.u8(0x48); // returnvalue
                }
            } else if let Some(top_fin) = env.finally_stack.last_mut() {
                emit_avm2_pop_catch_scopes(
                    code,
                    env.active_catch_regs.len(),
                    top_fin.catch_scope_depth,
                );
                let patch = emit_avm2_s24_placeholder(code, 0x10); // jump to post-outer_to unwind trampoline
                top_fin
                    .pending_unwinds
                    .push(Avm2PendingUnwind::ReturnVoid { jump_patch: patch });
            } else {
                emit_avm2_pop_catch_scopes(code, env.active_catch_regs.len(), 0);
                code.u8(0x47); // returnvoid
            }
        }
        SwfStmt::EventHandler { .. } => {
            return Err(
                "ActionScript 3 does not support inline 'on(...)' or 'onClipEvent(...)' handlers"
                    .to_owned(),
            );
        }
    }
    Ok(())
}
