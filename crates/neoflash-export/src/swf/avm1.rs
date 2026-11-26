//! AVM1 (SWF 8 ActionScript 1/2) expression, statement, closure, button, and clip-action compiler.

use super::*;

pub(super) fn avm1_push_string(out: &mut Vec<u8>, s: &str) -> Result<(), String> {
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != 0).collect();
    let payload_len = bytes
        .len()
        .checked_add(2)
        .and_then(|len| u16::try_from(len).ok())
        .ok_or_else(|| {
            format!(
                "AVM1 string value encodes {} bytes; ActionPush supports at most {} bytes",
                bytes.len(),
                usize::from(u16::MAX) - 2
            )
        })?;
    out.push(0x96); // ActionPush
    out.extend_from_slice(&payload_len.to_le_bytes());
    out.push(0x00); // type 0 = String
    out.extend_from_slice(&bytes);
    out.push(0);
    Ok(())
}

pub(super) fn avm1_push_int(out: &mut Vec<u8>, v: i32) {
    out.push(0x96); // ActionPush
    out.extend_from_slice(&5u16.to_le_bytes());
    out.push(0x07); // type 7 = Integer (i32)
    out.extend_from_slice(&v.to_le_bytes());
}

pub(super) fn avm1_push_double(out: &mut Vec<u8>, v: f64) {
    out.push(0x96); // ActionPush
    out.extend_from_slice(&9u16.to_le_bytes());
    out.push(0x06); // type 6 = Double (SWF word-swapped f64: high 4 bytes LE, then low 4 bytes LE)
    let bits = v.to_bits();
    let hi = ((bits >> 32) as u32).to_le_bytes();
    let lo = (bits as u32).to_le_bytes();
    out.extend_from_slice(&hi);
    out.extend_from_slice(&lo);
}

pub(super) fn avm1_push_bool(out: &mut Vec<u8>, v: bool) {
    out.push(0x96); // ActionPush
    out.extend_from_slice(&2u16.to_le_bytes());
    out.push(0x05); // type 5 = Boolean
    out.push(u8::from(v));
}

pub(super) fn avm1_push_register(out: &mut Vec<u8>, reg: u8) {
    out.push(0x96); // ActionPush
    out.extend_from_slice(&2u16.to_le_bytes());
    out.push(0x04); // type 4 = Register
    out.push(reg);
}

pub(super) fn collect_direct_decls_in_stmts(
    stmts: &[SwfStmt],
    vars: &mut Vec<String>,
    funcs: &mut Vec<String>,
) {
    for stmt in stmts {
        match stmt {
            SwfStmt::VarDecl { name, .. } => {
                if !vars.contains(name) {
                    vars.push(name.clone());
                }
            }
            SwfStmt::FunctionDecl { name, .. } => {
                if !funcs.contains(name) {
                    funcs.push(name.clone());
                }
            }
            SwfStmt::Block(inner) | SwfStmt::While { body: inner, .. } => {
                collect_direct_decls_in_stmts(inner, vars, funcs);
            }
            SwfStmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_direct_decls_in_stmts(then_branch, vars, funcs);
                if let Some(eb) = else_branch {
                    collect_direct_decls_in_stmts(eb, vars, funcs);
                }
            }
            SwfStmt::For {
                init, update, body, ..
            } => {
                if let Some(i) = init {
                    collect_direct_decls_in_stmts(std::slice::from_ref(i.as_ref()), vars, funcs);
                }
                if let Some(u) = update {
                    collect_direct_decls_in_stmts(std::slice::from_ref(u.as_ref()), vars, funcs);
                }
                collect_direct_decls_in_stmts(body, vars, funcs);
            }
            SwfStmt::ForIn {
                is_var_decl,
                var_name,
                body,
                ..
            } => {
                if *is_var_decl && !vars.contains(var_name) {
                    vars.push(var_name.clone());
                }
                collect_direct_decls_in_stmts(body, vars, funcs);
            }
            SwfStmt::Switch { cases, .. } => {
                for (_, cbody) in cases {
                    collect_direct_decls_in_stmts(cbody, vars, funcs);
                }
            }
            SwfStmt::Try {
                try_body,
                catches,
                finally_body,
            } => {
                collect_direct_decls_in_stmts(try_body, vars, funcs);
                for (cname, _, cbody) in catches {
                    if !cname.is_empty() && !vars.contains(cname) {
                        vars.push(cname.clone());
                    }
                    collect_direct_decls_in_stmts(cbody, vars, funcs);
                }
                if let Some(fbody) = finally_body {
                    collect_direct_decls_in_stmts(fbody, vars, funcs);
                }
            }
            SwfStmt::ClassDecl { name, .. } => {
                if !vars.contains(name) {
                    vars.push(name.clone());
                }
            }
            SwfStmt::EventHandler { body, .. } => {
                collect_direct_decls_in_stmts(body, vars, funcs);
            }
            SwfStmt::Assign { .. }
            | SwfStmt::IncDec { .. }
            | SwfStmt::Expr(_)
            | SwfStmt::Return(_)
            | SwfStmt::DefaultXmlNamespace(_)
            | SwfStmt::Break
            | SwfStmt::Continue
            | SwfStmt::Throw(_) => {}
        }
    }
}

pub(super) fn collect_refs_and_child_free_in_expr(
    expr: &SwfExpr,
    direct_refs: &mut HashSet<String>,
    child_free: &mut HashSet<String>,
) {
    match expr {
        SwfExpr::Null
        | SwfExpr::Undefined
        | SwfExpr::Bool(_)
        | SwfExpr::Int(_)
        | SwfExpr::Float(_)
        | SwfExpr::Str(_) => {}
        SwfExpr::Ident(name) => {
            direct_refs.insert(name.clone());
        }
        SwfExpr::ArrayLit(elems) => {
            for el in elems {
                collect_refs_and_child_free_in_expr(el, direct_refs, child_free);
            }
        }
        SwfExpr::ObjectLit(props) => {
            for (_, val) in props {
                collect_refs_and_child_free_in_expr(val, direct_refs, child_free);
            }
        }
        SwfExpr::Function {
            name, params, body, ..
        } => {
            collect_free_vars_of_function(name.as_deref(), params, body, child_free);
        }
        SwfExpr::Member(obj, _) => {
            collect_refs_and_child_free_in_expr(obj, direct_refs, child_free);
        }
        SwfExpr::Index(obj, idx) => {
            collect_refs_and_child_free_in_expr(obj, direct_refs, child_free);
            collect_refs_and_child_free_in_expr(idx, direct_refs, child_free);
        }
        SwfExpr::XmlLit { pieces, .. } => {
            for piece in pieces {
                match piece {
                    SwfXmlCompiledPiece::Raw(_) => {}
                    SwfXmlCompiledPiece::ElemExpr(e) | SwfXmlCompiledPiece::AttrExpr(e) => {
                        collect_refs_and_child_free_in_expr(e, direct_refs, child_free);
                    }
                }
            }
        }
        SwfExpr::XmlAttr { object, attr }
        | SwfExpr::XmlDescendants {
            object,
            target: attr,
            ..
        } => {
            collect_refs_and_child_free_in_expr(object, direct_refs, child_free);
            if let SwfXmlSelector::Computed(idx) = attr {
                collect_refs_and_child_free_in_expr(idx, direct_refs, child_free);
            }
        }
        SwfExpr::Unary { expr, .. } => {
            collect_refs_and_child_free_in_expr(expr, direct_refs, child_free);
        }
        SwfExpr::Binary { lhs, rhs, .. } => {
            collect_refs_and_child_free_in_expr(lhs, direct_refs, child_free);
            collect_refs_and_child_free_in_expr(rhs, direct_refs, child_free);
        }
        SwfExpr::Call { callee, args } | SwfExpr::New { callee, args } => {
            collect_refs_and_child_free_in_expr(callee, direct_refs, child_free);
            for a in args {
                collect_refs_and_child_free_in_expr(a, direct_refs, child_free);
            }
        }
        SwfExpr::Delete(lv) => {
            collect_refs_and_child_free_in_lvalue(lv, direct_refs, child_free);
        }
        SwfExpr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            collect_refs_and_child_free_in_expr(cond, direct_refs, child_free);
            collect_refs_and_child_free_in_expr(then_expr, direct_refs, child_free);
            collect_refs_and_child_free_in_expr(else_expr, direct_refs, child_free);
        }
    }
}

pub(super) fn collect_refs_and_child_free_in_lvalue(
    target: &SwfLValue,
    direct_refs: &mut HashSet<String>,
    child_free: &mut HashSet<String>,
) {
    match target {
        SwfLValue::Ident(name) => {
            direct_refs.insert(name.clone());
        }
        SwfLValue::Member(obj, _) => {
            collect_refs_and_child_free_in_expr(obj, direct_refs, child_free);
        }
        SwfLValue::Index(obj, idx) => {
            collect_refs_and_child_free_in_expr(obj, direct_refs, child_free);
            collect_refs_and_child_free_in_expr(idx, direct_refs, child_free);
        }
        SwfLValue::XmlAttr { object, attr } => {
            collect_refs_and_child_free_in_expr(object, direct_refs, child_free);
            if let SwfXmlSelector::Computed(idx) = attr {
                collect_refs_and_child_free_in_expr(idx, direct_refs, child_free);
            }
        }
    }
}

pub(super) fn collect_refs_and_child_free_in_stmts(
    stmts: &[SwfStmt],
    direct_refs: &mut HashSet<String>,
    child_free: &mut HashSet<String>,
) {
    for stmt in stmts {
        match stmt {
            SwfStmt::VarDecl { name, init } => {
                direct_refs.insert(name.clone());
                if let Some(rhs) = init {
                    collect_refs_and_child_free_in_expr(rhs, direct_refs, child_free);
                }
            }
            SwfStmt::Assign { target, value, .. } => {
                collect_refs_and_child_free_in_lvalue(target, direct_refs, child_free);
                collect_refs_and_child_free_in_expr(value, direct_refs, child_free);
            }
            SwfStmt::IncDec { target, .. } => {
                collect_refs_and_child_free_in_lvalue(target, direct_refs, child_free);
            }
            SwfStmt::Expr(expr) => {
                collect_refs_and_child_free_in_expr(expr, direct_refs, child_free);
            }
            SwfStmt::Block(inner) => {
                collect_refs_and_child_free_in_stmts(inner, direct_refs, child_free);
            }
            SwfStmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                collect_refs_and_child_free_in_expr(cond, direct_refs, child_free);
                collect_refs_and_child_free_in_stmts(then_branch, direct_refs, child_free);
                if let Some(eb) = else_branch {
                    collect_refs_and_child_free_in_stmts(eb, direct_refs, child_free);
                }
            }
            SwfStmt::While { cond, body, .. } => {
                collect_refs_and_child_free_in_expr(cond, direct_refs, child_free);
                collect_refs_and_child_free_in_stmts(body, direct_refs, child_free);
            }
            SwfStmt::For {
                init,
                cond,
                update,
                body,
            } => {
                if let Some(i) = init {
                    collect_refs_and_child_free_in_stmts(
                        std::slice::from_ref(i.as_ref()),
                        direct_refs,
                        child_free,
                    );
                }
                if let Some(c) = cond {
                    collect_refs_and_child_free_in_expr(c, direct_refs, child_free);
                }
                if let Some(u) = update {
                    collect_refs_and_child_free_in_stmts(
                        std::slice::from_ref(u.as_ref()),
                        direct_refs,
                        child_free,
                    );
                }
                collect_refs_and_child_free_in_stmts(body, direct_refs, child_free);
            }
            SwfStmt::ForIn {
                var_name,
                object,
                body,
                ..
            } => {
                direct_refs.insert(var_name.clone());
                collect_refs_and_child_free_in_expr(object, direct_refs, child_free);
                collect_refs_and_child_free_in_stmts(body, direct_refs, child_free);
            }
            SwfStmt::Switch {
                discriminant,
                cases,
            } => {
                collect_refs_and_child_free_in_expr(discriminant, direct_refs, child_free);
                for (ce, cb) in cases {
                    if let Some(e) = ce {
                        collect_refs_and_child_free_in_expr(e, direct_refs, child_free);
                    }
                    collect_refs_and_child_free_in_stmts(cb, direct_refs, child_free);
                }
            }
            SwfStmt::Try {
                try_body,
                catches,
                finally_body,
            } => {
                collect_refs_and_child_free_in_stmts(try_body, direct_refs, child_free);
                for (cname, ctype, cbody) in catches {
                    direct_refs.insert(cname.clone());
                    if let Some(ct) = ctype {
                        direct_refs.insert(ct.clone());
                    }
                    collect_refs_and_child_free_in_stmts(cbody, direct_refs, child_free);
                }
                if let Some(fbody) = finally_body {
                    collect_refs_and_child_free_in_stmts(fbody, direct_refs, child_free);
                }
            }
            SwfStmt::Throw(expr) => {
                collect_refs_and_child_free_in_expr(expr, direct_refs, child_free);
            }
            SwfStmt::Break | SwfStmt::Continue => {}
            SwfStmt::ClassDecl {
                name,
                super_name,
                ctor,
                methods,
                fields,
                ..
            } => {
                direct_refs.insert(name.clone());
                if let Some(sup) = super_name {
                    direct_refs.insert(sup.clone());
                }
                if let Some((cparams, cbody)) = ctor {
                    collect_free_vars_of_function(None, cparams, cbody, child_free);
                }
                for (_, _, mparams, mbody) in methods {
                    collect_free_vars_of_function(None, mparams, mbody, child_free);
                }
                for (_, _, finit) in fields {
                    if let Some(fe) = finit {
                        collect_refs_and_child_free_in_expr(fe, direct_refs, child_free);
                    }
                }
            }
            SwfStmt::FunctionDecl {
                name, params, body, ..
            } => {
                direct_refs.insert(name.clone());
                collect_free_vars_of_function(Some(name.as_str()), params, body, child_free);
            }
            SwfStmt::Return(Some(expr)) | SwfStmt::DefaultXmlNamespace(expr) => {
                collect_refs_and_child_free_in_expr(expr, direct_refs, child_free);
            }
            SwfStmt::Return(None) => {}
            SwfStmt::EventHandler { body, .. } => {
                collect_refs_and_child_free_in_stmts(body, direct_refs, child_free);
            }
        }
    }
}

pub(super) fn collect_free_vars_of_function(
    self_name: Option<&str>,
    params: &[String],
    body: &[SwfStmt],
    out_free: &mut HashSet<String>,
) {
    let mut own_vars = Vec::new();
    let mut own_funcs = Vec::new();
    collect_direct_decls_in_stmts(body, &mut own_vars, &mut own_funcs);
    let mut direct_refs = HashSet::new();
    let mut child_free = HashSet::new();
    collect_refs_and_child_free_in_stmts(body, &mut direct_refs, &mut child_free);

    let mut own_bound: HashSet<&str> = HashSet::new();
    if let Some(sn) = self_name
        && !sn.is_empty()
    {
        own_bound.insert(sn);
    }
    for p in params {
        own_bound.insert(p.as_str());
    }
    for v in &own_vars {
        own_bound.insert(v.as_str());
    }
    for f in &own_funcs {
        own_bound.insert(f.as_str());
    }
    for r in direct_refs.into_iter().chain(child_free) {
        if !own_bound.contains(r.as_str()) {
            out_free.insert(r);
        }
    }
}

pub(super) fn avm1_compile_expr(
    out: &mut Vec<u8>,
    expr: &SwfExpr,
    reg_map: &HashMap<String, u8>,
) -> Result<(), String> {
    avm1_compile_expr_bounded(out, expr, reg_map, 0)
}

fn avm1_compile_expr_bounded(
    out: &mut Vec<u8>,
    expr: &SwfExpr,
    reg_map: &HashMap<String, u8>,
    depth: usize,
) -> Result<(), String> {
    if depth >= MAX_SWF_AST_DEPTH {
        return Err(format!(
            "expression exceeds maximum compilation depth ({MAX_SWF_AST_DEPTH}) in bounded SWF compiler"
        ));
    }
    let next_depth = depth + 1;
    let compile_child = |buf: &mut Vec<u8>, child: &SwfExpr| -> Result<(), String> {
        avm1_compile_expr_bounded(buf, child, reg_map, next_depth)
    };
    match expr {
        SwfExpr::Null => {
            out.push(0x96);
            out.extend_from_slice(&1u16.to_le_bytes());
            out.push(0x02); // Null
        }
        SwfExpr::Undefined => {
            out.push(0x96);
            out.extend_from_slice(&1u16.to_le_bytes());
            out.push(0x03); // Undefined
        }
        SwfExpr::Bool(b) => avm1_push_bool(out, *b),
        SwfExpr::Int(iv) => avm1_push_int(out, *iv),
        SwfExpr::Float(fv) => avm1_push_double(out, *fv),
        SwfExpr::Str(s) => avm1_push_string(out, s)?,
        SwfExpr::Ident(name) => {
            if let Some(&reg) = reg_map.get(name) {
                avm1_push_register(out, reg);
            } else {
                avm1_push_string(out, name)?;
                out.push(0x1c); // ActionGetVariable
            }
        }
        SwfExpr::ArrayLit(elems) => {
            for el in elems.iter().rev() {
                compile_child(out, el)?;
            }
            avm1_push_int(out, elems.len() as i32);
            out.push(0x42); // ActionInitArray
        }
        SwfExpr::ObjectLit(props) => {
            for (key, val) in props {
                avm1_push_string(out, key)?;
                compile_child(out, val)?;
            }
            avm1_push_int(out, props.len() as i32);
            out.push(0x43); // ActionInitObject
        }
        SwfExpr::Function {
            name, params, body, ..
        } => {
            // An unnamed ActionDefineFunction2 pushes the function value onto the stack;
            // if a function-expression self-name is present (`function fact(n)`), bind it
            // inside the function's own local activation scope via `arguments.callee`.
            avm1_compile_function(out, "", name.as_deref(), params, body)?;
        }
        SwfExpr::Member(obj, prop) => {
            compile_child(out, obj)?;
            avm1_push_string(out, prop)?;
            out.push(0x4e); // ActionGetMember
        }
        SwfExpr::Index(obj, idx) => {
            compile_child(out, obj)?;
            compile_child(out, idx)?;
            out.push(0x4e); // ActionGetMember
        }
        SwfExpr::XmlLit { .. } | SwfExpr::XmlAttr { .. } | SwfExpr::XmlDescendants { .. } => {
            return Err("ActionScript 3 E4X syntax is not supported in AVM1 SWF".to_owned());
        }
        SwfExpr::Unary { op, expr } => match op {
            SwfUnaryOp::Not => {
                compile_child(out, expr)?;
                out.push(0x12); // ActionNot
            }
            SwfUnaryOp::Neg => {
                avm1_push_int(out, 0);
                compile_child(out, expr)?;
                out.push(0x0b); // ActionSubtract
            }
            SwfUnaryOp::Pos => {
                compile_child(out, expr)?;
                out.push(0x4a); // ActionToNumber
            }
            SwfUnaryOp::BitNot => {
                compile_child(out, expr)?;
                avm1_push_int(out, -1);
                out.push(0x62); // ActionBitXor
            }
            SwfUnaryOp::Typeof => {
                compile_child(out, expr)?;
                out.push(0x44); // ActionTypeOf
            }
        },
        SwfExpr::Delete(target) => match target.as_ref() {
            SwfLValue::Ident(name) => {
                avm1_push_string(out, name)?;
                out.push(0x3b); // ActionDelete2
            }
            SwfLValue::Member(obj, prop) => {
                compile_child(out, obj)?;
                avm1_push_string(out, prop)?;
                out.push(0x3a); // ActionDelete
            }
            SwfLValue::Index(obj, idx) => {
                compile_child(out, obj)?;
                compile_child(out, idx)?;
                out.push(0x3a); // ActionDelete
            }
            SwfLValue::XmlAttr { .. } => {
                return Err("ActionScript 3 E4X syntax is not supported in AVM1 SWF".to_owned());
            }
        },
        SwfExpr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            compile_child(out, cond)?;
            out.push(0x12); // ActionNot
            let mut then_bytes = Vec::new();
            compile_child(&mut then_bytes, then_expr)?;
            let mut else_bytes = Vec::new();
            compile_child(&mut else_bytes, else_expr)?;
            let skip_else = i16::try_from(else_bytes.len())
                .map_err(|_| "AVM1 ternary branch exceeds i16 jump offset limit".to_owned())?;
            then_bytes.push(0x99); // ActionJump
            then_bytes.extend_from_slice(&2u16.to_le_bytes());
            then_bytes.extend_from_slice(&skip_else.to_le_bytes());
            let skip_then = i16::try_from(then_bytes.len())
                .map_err(|_| "AVM1 ternary branch exceeds i16 jump offset limit".to_owned())?;
            out.push(0x9d); // ActionIf
            out.extend_from_slice(&2u16.to_le_bytes());
            out.extend_from_slice(&skip_then.to_le_bytes());
            out.extend_from_slice(&then_bytes);
            out.extend_from_slice(&else_bytes);
        }
        SwfExpr::Binary { op, lhs, rhs } => {
            if matches!(op, SwfBinaryOp::And | SwfBinaryOp::Or) {
                compile_child(out, lhs)?;
                out.push(0x4c); // ActionPushDuplicate
                if *op == SwfBinaryOp::And {
                    out.push(0x12); // ActionNot: branch when lhs is false
                }
                let mut rhs_bytes = Vec::new();
                compile_child(&mut rhs_bytes, rhs)?;
                let skip_rhs = rhs_bytes
                    .len()
                    .checked_add(1)
                    .and_then(|len| i16::try_from(len).ok())
                    .ok_or_else(|| "AVM1 logical RHS exceeds i16 branch range".to_owned())?;
                out.push(0x9d); // ActionIf
                out.extend_from_slice(&2u16.to_le_bytes());
                out.extend_from_slice(&skip_rhs.to_le_bytes());
                out.push(0x17); // ActionPop the non-short-circuit lhs
                out.extend_from_slice(&rhs_bytes);
                return Ok(());
            }
            if *op == SwfBinaryOp::In {
                // Evaluate `rhs[lhs] !== undefined` in AVM1
                compile_child(out, rhs)?;
                compile_child(out, lhs)?;
                out.push(0x4e); // ActionGetMember
                out.push(0x96);
                out.extend_from_slice(&1u16.to_le_bytes());
                out.push(0x03); // Undefined
                out.push(0x66); // ActionStrictEquals
                out.push(0x12); // ActionNot
                return Ok(());
            }
            if matches!(op, SwfBinaryOp::Is | SwfBinaryOp::As) {
                return Err(
                    "ActionScript 3 'is'/'as' type operators are not supported in AVM1 SWF"
                        .to_owned(),
                );
            }
            compile_child(out, lhs)?;
            compile_child(out, rhs)?;
            match op {
                SwfBinaryOp::Add => out.push(0x47), // ActionAdd2
                SwfBinaryOp::Sub => out.push(0x0b), // ActionSubtract
                SwfBinaryOp::Mul => out.push(0x0c), // ActionMultiply
                SwfBinaryOp::Div => out.push(0x0d), // ActionDivide
                SwfBinaryOp::Mod => out.push(0x3f), // ActionModulo
                SwfBinaryOp::Eq => out.push(0x49),  // ActionEquals2
                SwfBinaryOp::Ne => {
                    out.push(0x49); // ActionEquals2
                    out.push(0x12); // ActionNot
                }
                SwfBinaryOp::StrictEq => out.push(0x66), // ActionStrictEquals
                SwfBinaryOp::StrictNe => {
                    out.push(0x66); // ActionStrictEquals
                    out.push(0x12); // ActionNot
                }
                SwfBinaryOp::Lt => out.push(0x48), // ActionLess2
                SwfBinaryOp::Ge => {
                    out.push(0x48); // ActionLess2
                    out.push(0x12); // ActionNot
                }
                SwfBinaryOp::Gt => out.push(0x67), // ActionGreater
                SwfBinaryOp::Le => {
                    out.push(0x67); // ActionGreater
                    out.push(0x12); // ActionNot
                }
                SwfBinaryOp::And
                | SwfBinaryOp::Or
                | SwfBinaryOp::In
                | SwfBinaryOp::Is
                | SwfBinaryOp::As => unreachable!("handled above"),
                SwfBinaryOp::BitAnd => out.push(0x60), // ActionBitAnd
                SwfBinaryOp::BitOr => out.push(0x61),  // ActionBitOr
                SwfBinaryOp::BitXor => out.push(0x62), // ActionBitXor
                SwfBinaryOp::Shl => out.push(0x63),    // ActionBitLShift
                SwfBinaryOp::Shr => out.push(0x64),    // ActionBitRShift
                SwfBinaryOp::UShr => out.push(0x65),   // ActionBitURShift
                SwfBinaryOp::InstanceOf => out.push(0x54), // ActionInstanceOf
            }
        }
        SwfExpr::Call { callee, args } => {
            avm1_compile_call_bounded(out, callee, args, false, reg_map, next_depth)?;
        }
        SwfExpr::New { callee, args } => {
            for a in args.iter().rev() {
                compile_child(out, a)?;
            }
            avm1_push_int(out, args.len() as i32);
            match callee.as_ref() {
                SwfExpr::Ident(name) => {
                    if let Some(&reg) = reg_map.get(name) {
                        avm1_push_register(out, reg);
                        out.push(0x96);
                        out.extend_from_slice(&1u16.to_le_bytes());
                        out.push(0x03); // Undefined method name
                        out.push(0x53); // ActionNewMethod
                        return Ok(());
                    }
                    avm1_push_string(out, name)?;
                    out.push(0x40); // ActionNewObject
                }
                SwfExpr::Member(obj, prop) => {
                    compile_child(out, obj)?;
                    avm1_push_string(out, prop)?;
                    out.push(0x53); // ActionNewMethod
                }
                other => {
                    compile_child(out, other)?;
                    out.push(0x96);
                    out.extend_from_slice(&1u16.to_le_bytes());
                    out.push(0x03); // Undefined method name
                    out.push(0x53); // ActionNewMethod
                }
            }
        }
    }
    Ok(())
}

pub(super) fn avm1_compile_call(
    out: &mut Vec<u8>,
    callee: &SwfExpr,
    args: &[SwfExpr],
    discard_result: bool,
    reg_map: &HashMap<String, u8>,
) -> Result<(), String> {
    avm1_compile_call_bounded(out, callee, args, discard_result, reg_map, 0)
}

fn avm1_compile_call_bounded(
    out: &mut Vec<u8>,
    callee: &SwfExpr,
    args: &[SwfExpr],
    discard_result: bool,
    reg_map: &HashMap<String, u8>,
    depth: usize,
) -> Result<(), String> {
    if depth >= MAX_SWF_AST_DEPTH {
        return Err(format!(
            "expression exceeds maximum compilation depth ({MAX_SWF_AST_DEPTH}) in bounded SWF compiler"
        ));
    }
    let next_depth = depth + 1;
    let compile_child = |buf: &mut Vec<u8>, child: &SwfExpr| -> Result<(), String> {
        avm1_compile_expr_bounded(buf, child, reg_map, next_depth)
    };
    if let SwfExpr::Ident(name) = callee
        && let Some(&reg) = reg_map.get(name)
    {
        for a in args.iter().rev() {
            compile_child(out, a)?;
        }
        avm1_push_int(out, args.len() as i32);
        avm1_push_register(out, reg);
        out.push(0x96);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.push(0x03); // Undefined method name -> invoke function object directly
        out.push(0x52); // ActionCallMethod
        if discard_result {
            out.push(0x17); // ActionPop
        }
        return Ok(());
    }

    if let SwfExpr::Ident(name) = callee
        && name == "super"
    {
        for a in args.iter().rev() {
            compile_child(out, a)?;
        }
        avm1_push_int(out, args.len() as i32);
        compile_child(out, callee)?;
        out.push(0x96);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.push(0x03); // Undefined method name -> invoke super constructor
        out.push(0x52); // ActionCallMethod
        if discard_result {
            out.push(0x17); // ActionPop
        }
        return Ok(());
    }

    let direct_fn = match callee {
        SwfExpr::Ident(name) => Some(name.as_str()),
        _ => None,
    };

    if let Some(short_fn) = direct_fn {
        match short_fn {
            "stop" if args.is_empty() && discard_result => {
                out.push(0x07); // ActionStop
                return Ok(());
            }
            "play" if args.is_empty() && discard_result => {
                out.push(0x06); // ActionPlay
                return Ok(());
            }
            "nextFrame" if args.is_empty() && discard_result => {
                out.push(0x04); // ActionNextFrame
                return Ok(());
            }
            "prevFrame" if args.is_empty() && discard_result => {
                out.push(0x05); // ActionPrevFrame
                return Ok(());
            }
            "stopAllSounds" if args.is_empty() && discard_result => {
                out.push(0x09); // ActionStopSounds
                return Ok(());
            }
            "gotoAndStop" | "gotoAndPlay" if !args.is_empty() && discard_result => {
                let is_play = short_fn == "gotoAndPlay";
                let target_arg = args.last().unwrap();
                match target_arg {
                    SwfExpr::Int(frame_i32) => {
                        let frame_1based = u32::try_from(*frame_i32).map_err(|_| {
                            format!(
                                "AVM1 numeric {short_fn} target {frame_i32} must be a one-based frame"
                            )
                        })?;
                        if frame_1based == 0 {
                            return Err(format!(
                                "AVM1 numeric {short_fn} target 0 must be a one-based frame"
                            ));
                        }
                        let frame_0 =
                            u16::try_from(frame_1based - 1).map_err(|_| {
                                format!(
                                    "AVM1 numeric {short_fn} target {frame_1based} exceeds the ActionGotoFrame limit of 65536 one-based frames"
                                )
                            })?;
                        out.push(0x81); // ActionGotoFrame
                        out.extend_from_slice(&2u16.to_le_bytes());
                        out.extend_from_slice(&frame_0.to_le_bytes());
                        if is_play {
                            out.push(0x06); // ActionPlay
                        }
                    }
                    SwfExpr::Float(fv) if fv.fract() == 0.0 => {
                        if !(1.0..=65536.0).contains(fv) {
                            return Err(format!(
                                "AVM1 numeric {short_fn} target {fv} must be within 1..=65536 one-based frames"
                            ));
                        }
                        let frame_0 = ((*fv as u32) - 1) as u16;
                        out.push(0x81); // ActionGotoFrame
                        out.extend_from_slice(&2u16.to_le_bytes());
                        out.extend_from_slice(&frame_0.to_le_bytes());
                        if is_play {
                            out.push(0x06); // ActionPlay
                        }
                    }
                    SwfExpr::Str(lbl) => {
                        if !is_play {
                            let lbl_bytes: Vec<u8> = lbl.bytes().filter(|&b| b != 0).collect();
                            let len = lbl_bytes
                                .len()
                                .checked_add(1)
                                .and_then(|len| u16::try_from(len).ok())
                                .ok_or_else(|| {
                                    format!(
                                        "AVM1 frame label encodes {} bytes; ActionGotoLabel supports at most {} bytes",
                                        lbl_bytes.len(),
                                        usize::from(u16::MAX) - 1
                                    )
                                })?;
                            out.push(0x8c); // ActionGotoLabel
                            out.extend_from_slice(&len.to_le_bytes());
                            out.extend_from_slice(&lbl_bytes);
                            out.push(0);
                        } else {
                            avm1_push_string(out, lbl)?;
                            out.push(0x9f); // ActionGotoFrame2
                            out.extend_from_slice(&1u16.to_le_bytes());
                            out.push(0x01); // play = true
                        }
                    }
                    SwfExpr::Unary {
                        op: SwfUnaryOp::Neg,
                        expr,
                    } if matches!(expr.as_ref(), SwfExpr::Int(_) | SwfExpr::Float(_)) => {
                        return Err(format!(
                            "AVM1 numeric {short_fn} target must be a positive one-based frame"
                        ));
                    }
                    other => {
                        compile_child(out, other)?;
                        out.push(0x9f); // ActionGotoFrame2
                        out.extend_from_slice(&1u16.to_le_bytes());
                        out.push(u8::from(is_play));
                    }
                }
                return Ok(());
            }
            "trace" if discard_result => {
                if let Some(first) = args.first() {
                    compile_child(out, first)?;
                } else {
                    avm1_push_string(out, "")?;
                }
                out.push(0x26); // ActionTrace
                return Ok(());
            }
            "fscommand" if discard_result && !args.is_empty() => {
                if let SwfExpr::Str(cmd) = &args[0] {
                    let param_str = match args.get(1) {
                        None => Some(""),
                        Some(SwfExpr::Str(s)) => Some(s.as_str()),
                        _ => None,
                    };
                    if let Some(p_str) = param_str {
                        let url_bytes: Vec<u8> = format!("FSCommand:{cmd}")
                            .bytes()
                            .filter(|&b| b != 0)
                            .collect();
                        let tgt_bytes: Vec<u8> = p_str.bytes().filter(|&b| b != 0).collect();
                        let len = url_bytes
                            .len()
                            .checked_add(1)
                            .and_then(|n| n.checked_add(tgt_bytes.len()))
                            .and_then(|n| n.checked_add(1))
                            .and_then(|n| u16::try_from(n).ok())
                            .ok_or_else(|| {
                                "AVM1 ActionGetURL FSCommand exceeds 65535 bytes".to_owned()
                            })?;
                        out.push(0x83); // ActionGetURL
                        out.extend_from_slice(&len.to_le_bytes());
                        out.extend_from_slice(&url_bytes);
                        out.push(0);
                        out.extend_from_slice(&tgt_bytes);
                        out.push(0);
                        return Ok(());
                    }
                }
                avm1_push_string(out, "FSCommand:")?;
                compile_child(out, &args[0])?;
                out.push(0x47); // ActionAdd2 -> "FSCommand:" + cmd
                if let Some(param_expr) = args.get(1) {
                    compile_child(out, param_expr)?;
                } else {
                    avm1_push_string(out, "")?;
                }
                out.push(0x9a); // ActionGetURL2
                out.extend_from_slice(&1u16.to_le_bytes());
                out.push(0x00);
                return Ok(());
            }
            "getURL" if discard_result && !args.is_empty() => {
                let send_method = match args.get(2) {
                    Some(SwfExpr::Str(m)) if m.eq_ignore_ascii_case("GET") => 1u8,
                    Some(SwfExpr::Str(m)) if m.eq_ignore_ascii_case("POST") => 2u8,
                    _ => 0u8,
                };
                if send_method == 0
                    && args.len() <= 2
                    && let SwfExpr::Str(url_s) = &args[0]
                {
                    let tgt_opt = match args.get(1) {
                        None => Some(""),
                        Some(SwfExpr::Str(t)) => Some(t.as_str()),
                        _ => None,
                    };
                    if let Some(tgt_s) = tgt_opt {
                        let url_bytes: Vec<u8> = url_s.bytes().filter(|&b| b != 0).collect();
                        let tgt_bytes: Vec<u8> = tgt_s.bytes().filter(|&b| b != 0).collect();
                        let len = url_bytes
                            .len()
                            .checked_add(1)
                            .and_then(|n| n.checked_add(tgt_bytes.len()))
                            .and_then(|n| n.checked_add(1))
                            .and_then(|n| u16::try_from(n).ok())
                            .ok_or_else(|| "AVM1 ActionGetURL exceeds 65535 bytes".to_owned())?;
                        out.push(0x83); // ActionGetURL
                        out.extend_from_slice(&len.to_le_bytes());
                        out.extend_from_slice(&url_bytes);
                        out.push(0);
                        out.extend_from_slice(&tgt_bytes);
                        out.push(0);
                        return Ok(());
                    }
                }
                compile_child(out, &args[0])?;
                if let Some(tgt_expr) = args.get(1) {
                    compile_child(out, tgt_expr)?;
                } else {
                    avm1_push_string(out, "")?;
                }
                out.push(0x9a); // ActionGetURL2
                out.extend_from_slice(&1u16.to_le_bytes());
                out.push(send_method);
                return Ok(());
            }
            _ => {
                for a in args.iter().rev() {
                    compile_child(out, a)?;
                }
                avm1_push_int(out, args.len() as i32);
                avm1_push_string(out, short_fn)?;
                out.push(0x3d); // ActionCallFunction
                if discard_result {
                    out.push(0x17); // ActionPop
                }
                return Ok(());
            }
        }
    }

    if let SwfExpr::Member(recv, method_name) = callee {
        for a in args.iter().rev() {
            compile_child(out, a)?;
        }
        avm1_push_int(out, args.len() as i32);
        compile_child(out, recv)?;
        avm1_push_string(out, method_name)?;
        out.push(0x52); // ActionCallMethod
        if discard_result {
            out.push(0x17); // ActionPop
        }
        return Ok(());
    }

    // Arbitrary callable expression (e.g. `makeAdder(10)(5)`, `(x => x * 2)(4)`, `fns[0](3)`):
    for a in args.iter().rev() {
        compile_child(out, a)?;
    }
    avm1_push_int(out, args.len() as i32);
    compile_child(out, callee)?;
    out.push(0x96);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.push(0x03); // Undefined method name -> call function value directly
    out.push(0x52); // ActionCallMethod
    if discard_result {
        out.push(0x17); // ActionPop
    }
    Ok(())
}

pub(super) fn avm1_store_lvalue_with_rhs<F>(
    out: &mut Vec<u8>,
    target: &SwfLValue,
    reg_map: &HashMap<String, u8>,
    read_previous: bool,
    emit_rhs: F,
) -> Result<(), String>
where
    F: FnOnce(&mut Vec<u8>) -> Result<(), String>,
{
    match target {
        SwfLValue::Ident(name) => {
            if let Some(&reg) = reg_map.get(name) {
                if read_previous {
                    avm1_push_register(out, reg);
                }
                emit_rhs(out)?;
                out.push(0x87); // ActionStoreRegister
                out.extend_from_slice(&1u16.to_le_bytes());
                out.push(reg);
                out.push(0x17); // ActionPop
            } else {
                avm1_push_string(out, name)?;
                if read_previous {
                    out.extend([0x4c, 0x1c]); // duplicate name, get variable
                }
                emit_rhs(out)?;
                out.push(0x1d); // ActionSetVariable
            }
        }
        SwfLValue::Member(obj, _) | SwfLValue::Index(obj, _) => {
            avm1_compile_expr(out, obj, reg_map)?;
            if read_previous {
                out.push(0x4c); // keep the receiver for the write
            }
            match target {
                SwfLValue::Member(_, name) => avm1_push_string(out, name)?,
                SwfLValue::Index(_, index) => avm1_compile_expr(out, index, reg_map)?,
                SwfLValue::Ident(_) | SwfLValue::XmlAttr { .. } => unreachable!(),
            }
            if read_previous {
                // [obj, obj, key] -> [obj, key, obj, key]. Register zero is scratch
                // only until GetMember; the getter and RHS may call arbitrary functions.
                out.extend([0x87, 1, 0, 0, 0x4d]); // StoreRegister 0, StackSwap
                avm1_push_register(out, 0);
                out.push(0x4e); // GetMember
            }
            emit_rhs(out)?;
            out.push(0x4f); // SetMember
        }
        SwfLValue::XmlAttr { .. } => {
            return Err("ActionScript 3 E4X syntax is not supported in AVM1 SWF".to_owned());
        }
    }
    Ok(())
}

pub(super) fn avm1_compile_function(
    out: &mut Vec<u8>,
    name: &str,
    self_expr_name: Option<&str>,
    params: &[String],
    body: &[SwfStmt],
) -> Result<(), String> {
    let mut own_vars = Vec::new();
    let mut own_funcs = Vec::new();
    collect_direct_decls_in_stmts(body, &mut own_vars, &mut own_funcs);
    let mut direct_refs = HashSet::new();
    let mut child_free = HashSet::new();
    collect_refs_and_child_free_in_stmts(body, &mut direct_refs, &mut child_free);

    let needs_self_binding = self_expr_name.is_some_and(|sn| {
        !sn.is_empty()
            && !params.iter().any(|p| p == sn)
            && (direct_refs.contains(sn) || child_free.contains(sn))
    });
    let mut flags = 0u16;
    if !needs_self_binding
        && !direct_refs.contains("arguments")
        && !child_free.contains("arguments")
    {
        flags |= 0x0008; // suppress_arguments
    }
    if !direct_refs.contains("super") && !child_free.contains("super") {
        flags |= 0x0020; // suppress_super
    }

    let mut fn_reg_map = HashMap::new();
    let mut next_reg = 1u8;
    let mut param_specs: Vec<(u8, &str)> = Vec::with_capacity(params.len());
    let mut seen_params = HashSet::new();
    for param in params {
        // Any parameter captured by a nested closure, shadowed by `var`/`function`, or duplicated
        // MUST use `reg = 0` so AVM1 binds it on the activation scope chain instead of a register!
        let must_use_activation = child_free.contains(param)
            || own_vars.contains(param)
            || own_funcs.contains(param)
            || !seen_params.insert(param.as_str())
            || next_reg >= 250;
        if must_use_activation {
            param_specs.push((0, param.as_str()));
        } else {
            let reg = next_reg;
            next_reg += 1;
            fn_reg_map.insert(param.clone(), reg);
            param_specs.push((reg, param.as_str()));
        }
    }

    let mut body_bytes = Vec::new();
    if needs_self_binding && let Some(self_name) = self_expr_name {
        avm1_push_string(&mut body_bytes, self_name)?;
        avm1_push_string(&mut body_bytes, "arguments")?;
        body_bytes.push(0x1c); // ActionGetVariable
        avm1_push_string(&mut body_bytes, "callee")?;
        body_bytes.push(0x4e); // ActionGetMember
        body_bytes.push(0x3c); // ActionDefineLocal
    }
    avm1_compile_stmt_list(&mut body_bytes, body, true, &fn_reg_map)?;
    let code_size = u16::try_from(body_bytes.len())
        .map_err(|_| "AVM1 function body exceeds 65535 bytes".to_owned())?;

    let mut header = Vec::new();
    header.extend(name.bytes().filter(|&b| b != 0));
    header.push(0);
    let num_params = u16::try_from(param_specs.len())
        .map_err(|_| "AVM1 function has too many parameters".to_owned())?;
    header.extend_from_slice(&num_params.to_le_bytes());
    header.push(next_reg); // register_count
    header.extend_from_slice(&flags.to_le_bytes());
    for (reg, pname) in param_specs {
        header.push(reg);
        header.extend(pname.bytes().filter(|&b| b != 0));
        header.push(0);
    }
    header.extend_from_slice(&code_size.to_le_bytes());
    let header_len = u16::try_from(header.len())
        .map_err(|_| "AVM1 function header exceeds 65535 bytes".to_owned())?;
    out.push(0x8e); // ActionDefineFunction2
    out.extend_from_slice(&header_len.to_le_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(&body_bytes);
    Ok(())
}

pub(super) struct Avm1ControlFrame {
    pub(super) is_switch: bool,
    pub(super) continue_target: Option<usize>,
    pub(super) continue_patches: Vec<usize>,
    pub(super) break_patches: Vec<usize>,
    pub(super) finally_depth: usize,
}

#[derive(Clone)]
struct Avm1FinallyEntry {
    stmts: Vec<SwfStmt>,
}

struct Avm1FlowCtx {
    frames: Vec<Avm1ControlFrame>,
    finally_stack: Vec<Avm1FinallyEntry>,
    try_depth: usize,
}

fn emit_avm1_branch_placeholder(out: &mut Vec<u8>, op: u8) -> usize {
    out.push(op);
    out.extend_from_slice(&2u16.to_le_bytes());
    let patch_pos = out.len();
    out.extend_from_slice(&0i16.to_le_bytes());
    patch_pos
}

fn patch_avm1_branch(out: &mut [u8], patch_pos: usize, target_pos: usize) -> Result<(), String> {
    let rel = (target_pos as isize) - ((patch_pos + 2) as isize);
    let off =
        i16::try_from(rel).map_err(|_| "AVM1 branch offset exceeds i16 jump limit".to_owned())?;
    out[patch_pos..patch_pos + 2].copy_from_slice(&off.to_le_bytes());
    Ok(())
}

fn avm1_emit_unwind_finally_range(
    out: &mut Vec<u8>,
    in_function: bool,
    reg_map: &HashMap<String, u8>,
    ctx: &mut Avm1FlowCtx,
    target_depth: usize,
) -> Result<(), String> {
    if ctx.finally_stack.len() <= target_depth {
        return Ok(());
    }
    let saved = ctx.finally_stack.clone();
    for depth in (target_depth..saved.len()).rev() {
        ctx.finally_stack.truncate(depth);
        let res =
            avm1_compile_stmt_list_with_flow(out, &saved[depth].stmts, in_function, reg_map, ctx);
        if let Err(e) = res {
            ctx.finally_stack = saved;
            return Err(e);
        }
    }
    ctx.finally_stack = saved;
    Ok(())
}

pub(super) fn avm1_compile_stmt_list(
    out: &mut Vec<u8>,
    stmts: &[SwfStmt],
    in_function: bool,
    reg_map: &HashMap<String, u8>,
) -> Result<(), String> {
    let mut ctx = Avm1FlowCtx {
        frames: Vec::new(),
        finally_stack: Vec::new(),
        try_depth: 0,
    };
    avm1_compile_stmt_list_with_flow(out, stmts, in_function, reg_map, &mut ctx)
}

fn avm1_compile_stmt_list_with_flow(
    out: &mut Vec<u8>,
    stmts: &[SwfStmt],
    in_function: bool,
    reg_map: &HashMap<String, u8>,
    ctx: &mut Avm1FlowCtx,
) -> Result<(), String> {
    for stmt in stmts {
        if let SwfStmt::FunctionDecl {
            name, params, body, ..
        } = stmt
        {
            if in_function {
                avm1_push_string(out, name)?;
                avm1_compile_function(out, "", Some(name), params, body)?;
                out.push(0x3c); // ActionDefineLocal on enclosing activation scope
            } else {
                avm1_compile_function(out, name, None, params, body)?;
            }
        }
    }
    for stmt in stmts {
        if matches!(stmt, SwfStmt::FunctionDecl { .. }) {
            continue;
        }
        avm1_compile_stmt_with_flow(out, stmt, in_function, reg_map, ctx)?;
    }
    Ok(())
}

fn avm1_compile_stmt_with_flow(
    out: &mut Vec<u8>,
    stmt: &SwfStmt,
    in_function: bool,
    reg_map: &HashMap<String, u8>,
    ctx: &mut Avm1FlowCtx,
) -> Result<(), String> {
    match stmt {
        SwfStmt::VarDecl { name, init } => {
            if let Some(rhs) = init {
                avm1_push_string(out, name)?;
                avm1_compile_expr(out, rhs, reg_map)?;
                out.push(if in_function { 0x3c } else { 0x1d });
            } else if in_function {
                avm1_push_string(out, name)?;
                out.push(0x41); // ActionDefineLocal2
            } else {
                avm1_push_string(out, name)?;
                out.push(0x96);
                out.extend_from_slice(&1u16.to_le_bytes());
                out.push(0x03); // Undefined
                out.push(0x1d); // ActionSetVariable
            }
        }
        SwfStmt::Assign { target, op, value } => {
            avm1_store_lvalue_with_rhs(out, target, reg_map, *op != SwfAssignOp::Assign, |buf| {
                if *op == SwfAssignOp::Assign {
                    avm1_compile_expr(buf, value, reg_map)
                } else {
                    avm1_compile_expr(buf, value, reg_map)?;
                    buf.push(match op {
                        SwfAssignOp::AddAssign => 0x47,
                        SwfAssignOp::SubAssign => 0x0b,
                        SwfAssignOp::MulAssign => 0x0c,
                        SwfAssignOp::DivAssign => 0x0d,
                        SwfAssignOp::ModAssign => 0x3f,
                        SwfAssignOp::BitAndAssign => 0x60,
                        SwfAssignOp::BitOrAssign => 0x61,
                        SwfAssignOp::BitXorAssign => 0x62,
                        SwfAssignOp::ShlAssign => 0x63,
                        SwfAssignOp::ShrAssign => 0x64,
                        SwfAssignOp::UShrAssign => 0x65,
                        SwfAssignOp::Assign => unreachable!(),
                    });
                    Ok(())
                }
            })?;
        }
        SwfStmt::IncDec { target, increment } => {
            avm1_store_lvalue_with_rhs(out, target, reg_map, true, |buf| {
                buf.push(if *increment { 0x50 } else { 0x51 }); // ActionIncrement / ActionDecrement
                Ok(())
            })?;
        }
        SwfStmt::Expr(SwfExpr::Call { callee, args }) => {
            avm1_compile_call(out, callee, args, true, reg_map)?;
        }
        SwfStmt::Expr(expr) => {
            avm1_compile_expr(out, expr, reg_map)?;
            out.push(0x17); // ActionPop
        }
        SwfStmt::Block(inner) => {
            avm1_compile_stmt_list_with_flow(out, inner, in_function, reg_map, ctx)?;
        }
        SwfStmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let mut end_patches = Vec::new();
            let mut cur_cond = cond;
            let mut cur_then = then_branch;
            let mut cur_else = else_branch.as_deref();
            loop {
                avm1_compile_expr(out, cur_cond, reg_map)?;
                out.push(0x12); // ActionNot: jump if cond is false
                let skip_then_patch = emit_avm1_branch_placeholder(out, 0x9d);
                avm1_compile_stmt_list_with_flow(out, cur_then, in_function, reg_map, ctx)?;
                match cur_else {
                    Some(
                        [
                            SwfStmt::If {
                                cond: next_cond,
                                then_branch: next_then,
                                else_branch: next_else,
                            },
                        ],
                    ) => {
                        end_patches.push(emit_avm1_branch_placeholder(out, 0x99));
                        let next_if_start = out.len();
                        patch_avm1_branch(out, skip_then_patch, next_if_start)?;
                        cur_cond = next_cond;
                        cur_then = next_then;
                        cur_else = next_else.as_deref();
                    }
                    Some(else_stmts) if !else_stmts.is_empty() => {
                        end_patches.push(emit_avm1_branch_placeholder(out, 0x99));
                        let else_start = out.len();
                        patch_avm1_branch(out, skip_then_patch, else_start)?;
                        avm1_compile_stmt_list_with_flow(
                            out,
                            else_stmts,
                            in_function,
                            reg_map,
                            ctx,
                        )?;
                        break;
                    }
                    _ => {
                        let after_then = out.len();
                        patch_avm1_branch(out, skip_then_patch, after_then)?;
                        break;
                    }
                }
            }
            let after_chain = out.len();
            for patch in end_patches {
                patch_avm1_branch(out, patch, after_chain)?;
            }
        }
        SwfStmt::While {
            cond,
            body,
            post_test,
        } => {
            let loop_start = out.len();
            let mut break_patches = Vec::new();
            if !post_test {
                avm1_compile_expr(out, cond, reg_map)?;
                out.push(0x12); // ActionNot
                break_patches.push(emit_avm1_branch_placeholder(out, 0x9d));
            }
            ctx.frames.push(Avm1ControlFrame {
                is_switch: false,
                continue_target: (!post_test).then_some(loop_start),
                continue_patches: Vec::new(),
                break_patches,
                finally_depth: ctx.finally_stack.len(),
            });
            let body_res = avm1_compile_stmt_list_with_flow(out, body, in_function, reg_map, ctx);
            let frame = ctx.frames.pop().unwrap();
            body_res?;
            let repeat = if *post_test {
                let cond_pos = out.len();
                for patch in frame.continue_patches {
                    patch_avm1_branch(out, patch, cond_pos)?;
                }
                avm1_compile_expr(out, cond, reg_map)?;
                emit_avm1_branch_placeholder(out, 0x9d) // ActionIf
            } else {
                emit_avm1_branch_placeholder(out, 0x99) // ActionJump
            };
            patch_avm1_branch(out, repeat, loop_start)?;
            let after_loop = out.len();
            for patch in frame.break_patches {
                patch_avm1_branch(out, patch, after_loop)?;
            }
        }
        SwfStmt::For {
            init,
            cond,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                avm1_compile_stmt_with_flow(out, init_stmt, in_function, reg_map, ctx)?;
            }
            let loop_start = out.len();
            let mut break_patches = Vec::new();
            if let Some(cond_expr) = cond {
                avm1_compile_expr(out, cond_expr, reg_map)?;
                out.push(0x12); // ActionNot
                break_patches.push(emit_avm1_branch_placeholder(out, 0x9d));
            }
            ctx.frames.push(Avm1ControlFrame {
                is_switch: false,
                continue_target: None,
                continue_patches: Vec::new(),
                break_patches,
                finally_depth: ctx.finally_stack.len(),
            });
            let body_res = avm1_compile_stmt_list_with_flow(out, body, in_function, reg_map, ctx);
            let frame = ctx.frames.pop().unwrap();
            body_res?;
            let update_start = out.len();
            for cp in frame.continue_patches {
                patch_avm1_branch(out, cp, update_start)?;
            }
            if let Some(upd_stmt) = update {
                avm1_compile_stmt_with_flow(out, upd_stmt, in_function, reg_map, ctx)?;
            }
            let back_patch = emit_avm1_branch_placeholder(out, 0x99);
            patch_avm1_branch(out, back_patch, loop_start)?;
            let after_loop = out.len();
            for bp in frame.break_patches {
                patch_avm1_branch(out, bp, after_loop)?;
            }
        }
        SwfStmt::ForIn {
            is_each,
            is_var_decl,
            var_name,
            object,
            body,
        } => {
            let depth_id = ctx.frames.len();
            let obj_tmp = format!("__nf_fe_{depth_id}");
            let key_tmp = format!("__nf_fk_{depth_id}");
            if *is_each {
                avm1_push_string(out, &obj_tmp)?;
                avm1_compile_expr(out, object, reg_map)?;
                out.push(if in_function { 0x3c } else { 0x1d });
                avm1_push_string(out, &obj_tmp)?;
                out.push(0x1c); // ActionGetVariable
            } else {
                avm1_compile_expr(out, object, reg_map)?;
            }
            out.push(0x55); // ActionEnumerate2 -> pushes [Null, k1, ..., kn]

            let loop_start = out.len();
            out.push(0x4c); // ActionPushDuplicate
            out.push(0x96);
            out.extend_from_slice(&1u16.to_le_bytes());
            out.push(0x02); // Null
            out.push(0x66); // ActionStrictEquals
            let done_patch = emit_avm1_branch_placeholder(out, 0x9d);

            if *is_each {
                avm1_push_string(out, &key_tmp)?;
                out.push(0x4d); // ActionSwap -> [..., key_tmp, ki]
                out.push(if in_function { 0x3c } else { 0x1d });
                if let Some(&reg) = reg_map.get(var_name) {
                    avm1_push_string(out, &obj_tmp)?;
                    out.push(0x1c);
                    avm1_push_string(out, &key_tmp)?;
                    out.push(0x1c);
                    out.push(0x4e); // ActionGetMember
                    out.push(0x87); // ActionStoreRegister
                    out.extend_from_slice(&1u16.to_le_bytes());
                    out.push(reg);
                    out.push(0x17); // ActionPop
                } else {
                    avm1_push_string(out, var_name)?;
                    avm1_push_string(out, &obj_tmp)?;
                    out.push(0x1c);
                    avm1_push_string(out, &key_tmp)?;
                    out.push(0x1c);
                    out.push(0x4e); // ActionGetMember
                    out.push(if in_function && *is_var_decl {
                        0x3c
                    } else {
                        0x1d
                    });
                }
            } else if let Some(&reg) = reg_map.get(var_name) {
                out.push(0x87); // ActionStoreRegister
                out.extend_from_slice(&1u16.to_le_bytes());
                out.push(reg);
                out.push(0x17); // ActionPop
            } else {
                avm1_push_string(out, var_name)?;
                out.push(0x4d); // ActionSwap -> [..., var_name, ki]
                out.push(if in_function && *is_var_decl {
                    0x3c
                } else {
                    0x1d
                });
            }

            ctx.frames.push(Avm1ControlFrame {
                is_switch: false,
                continue_target: Some(loop_start),
                continue_patches: Vec::new(),
                break_patches: Vec::new(),
                finally_depth: ctx.finally_stack.len(),
            });
            let body_res = avm1_compile_stmt_list_with_flow(out, body, in_function, reg_map, ctx);
            let frame = ctx.frames.pop().unwrap();
            body_res?;

            let back_patch = emit_avm1_branch_placeholder(out, 0x99);
            patch_avm1_branch(out, back_patch, loop_start)?;
            for cp in frame.continue_patches {
                patch_avm1_branch(out, cp, loop_start)?;
            }

            let normal_exit = out.len();
            patch_avm1_branch(out, done_patch, normal_exit)?;
            out.push(0x17); // ActionPop the Null sentinel

            if !frame.break_patches.is_empty() {
                let skip_drain_patch = emit_avm1_branch_placeholder(out, 0x99);
                let drain_start = out.len();
                for bp in frame.break_patches {
                    patch_avm1_branch(out, bp, drain_start)?;
                }
                // Drain remaining keys + Null sentinel on `break`
                out.push(0x96);
                out.extend_from_slice(&1u16.to_le_bytes());
                out.push(0x02); // Null
                out.push(0x66); // ActionStrictEquals
                out.push(0x12); // ActionNot
                let drain_back = emit_avm1_branch_placeholder(out, 0x9d);
                patch_avm1_branch(out, drain_back, drain_start)?;
                let after_drain = out.len();
                patch_avm1_branch(out, skip_drain_patch, after_drain)?;
            }
            if *is_each && !in_function {
                for tmp in [&obj_tmp, &key_tmp] {
                    avm1_push_string(out, tmp)?;
                    out.push(0x3b); // ActionDelete2
                    out.push(0x17); // ActionPop
                }
            }
        }
        SwfStmt::Switch {
            discriminant,
            cases,
        } => {
            avm1_compile_expr(out, discriminant, reg_map)?;
            let mut case_hit_patches: Vec<(usize, usize)> = Vec::new();
            let mut default_clause_idx: Option<usize> = None;
            for (c_idx, (case_expr_opt, _)) in cases.iter().enumerate() {
                if let Some(case_expr) = case_expr_opt {
                    out.push(0x4c); // ActionPushDuplicate
                    avm1_compile_expr(out, case_expr, reg_map)?;
                    out.push(0x66); // ActionStrictEquals
                    let hit_patch = emit_avm1_branch_placeholder(out, 0x9d);
                    case_hit_patches.push((c_idx, hit_patch));
                } else if default_clause_idx.is_none() {
                    default_clause_idx = Some(c_idx);
                }
            }
            out.push(0x17); // ActionPop discriminant on miss
            let miss_patch = emit_avm1_branch_placeholder(out, 0x99);

            let mut body_jump_patches: Vec<(usize, usize)> =
                Vec::with_capacity(case_hit_patches.len());
            for (c_idx, hit_patch) in case_hit_patches {
                let stub_pos = out.len();
                patch_avm1_branch(out, hit_patch, stub_pos)?;
                out.push(0x17); // ActionPop discriminant on hit
                let body_jmp = emit_avm1_branch_placeholder(out, 0x99);
                body_jump_patches.push((c_idx, body_jmp));
            }

            ctx.frames.push(Avm1ControlFrame {
                is_switch: true,
                continue_target: None,
                continue_patches: Vec::new(),
                break_patches: Vec::new(),
                finally_depth: ctx.finally_stack.len(),
            });
            let mut body_offsets = Vec::with_capacity(cases.len());
            let mut body_err = None;
            for (_, case_stmts) in cases {
                body_offsets.push(out.len());
                if let Err(e) =
                    avm1_compile_stmt_list_with_flow(out, case_stmts, in_function, reg_map, ctx)
                {
                    body_err = Some(e);
                    break;
                }
            }
            let frame = ctx.frames.pop().unwrap();
            if let Some(e) = body_err {
                return Err(e);
            }
            let after_switch = out.len();
            for (c_idx, body_jmp) in body_jump_patches {
                patch_avm1_branch(out, body_jmp, body_offsets[c_idx])?;
            }
            let miss_target = default_clause_idx.map_or(after_switch, |d_idx| body_offsets[d_idx]);
            patch_avm1_branch(out, miss_patch, miss_target)?;
            for bp in frame.break_patches {
                patch_avm1_branch(out, bp, after_switch)?;
            }
        }
        SwfStmt::Break => {
            let Some(finally_depth) = ctx.frames.last().map(|f| f.finally_depth) else {
                return Err("'break' is only valid inside a loop or switch statement".to_owned());
            };
            avm1_emit_unwind_finally_range(out, in_function, reg_map, ctx, finally_depth)?;
            let patch = emit_avm1_branch_placeholder(out, 0x99);
            ctx.frames.last_mut().unwrap().break_patches.push(patch);
        }
        SwfStmt::Continue => {
            let Some((loop_idx, finally_depth)) = ctx
                .frames
                .iter()
                .enumerate()
                .rev()
                .find(|(_, f)| !f.is_switch)
                .map(|(i, f)| (i, f.finally_depth))
            else {
                return Err("'continue' is only valid inside a loop statement".to_owned());
            };
            avm1_emit_unwind_finally_range(out, in_function, reg_map, ctx, finally_depth)?;
            let patch = emit_avm1_branch_placeholder(out, 0x99);
            if let Some(target) = ctx.frames[loop_idx].continue_target {
                patch_avm1_branch(out, patch, target)?;
            } else {
                ctx.frames[loop_idx].continue_patches.push(patch);
            }
        }
        SwfStmt::Throw(expr) => {
            if !in_function
                && let SwfExpr::Ident(id) = expr
                && id.starts_with("__nf_err_")
            {
                avm1_push_string(out, id)?;
                out.push(0x1c); // ActionGetVariable
                avm1_push_string(out, id)?;
                out.push(0x3b); // ActionDelete2
                out.push(0x17); // ActionPop
            } else {
                avm1_compile_expr(out, expr, reg_map)?;
            }
            out.push(0x2a); // ActionThrow
        }
        SwfStmt::Try {
            try_body,
            catches,
            finally_body,
        } => {
            let cur_try_depth = ctx.try_depth;
            ctx.try_depth += 1;

            let catch_var_name = if catches.len() == 1 && catches[0].1.is_none() {
                catches[0].0.clone()
            } else if !catches.is_empty() {
                format!("__nf_err_{cur_try_depth}")
            } else {
                String::new()
            };

            let flags: u8 = u8::from(!catches.is_empty()) | (u8::from(finally_body.is_some()) << 1);
            let cname_bytes: Vec<u8> = catch_var_name.bytes().filter(|&b| b != 0).collect();
            let header_len = u16::try_from(7 + cname_bytes.len() + 1)
                .map_err(|_| "AVM1 ActionTry header exceeds 65535 bytes".to_owned())?;

            out.push(0x8f); // ActionTry
            out.extend_from_slice(&header_len.to_le_bytes());
            out.push(flags);
            let sizes_patch_pos = out.len();
            out.extend_from_slice(&0u16.to_le_bytes()); // try_size
            out.extend_from_slice(&0u16.to_le_bytes()); // catch_size
            out.extend_from_slice(&0u16.to_le_bytes()); // finally_size
            out.extend_from_slice(&cname_bytes);
            out.push(0);

            if let Some(fin_stmts) = finally_body {
                ctx.finally_stack.push(Avm1FinallyEntry {
                    stmts: fin_stmts.clone(),
                });
            }

            let try_start = out.len();
            let try_res =
                avm1_compile_stmt_list_with_flow(out, try_body, in_function, reg_map, ctx);
            let try_end = out.len();

            let catch_start = out.len();
            let catch_res = if try_res.is_ok() {
                if catches.len() == 1 && catches[0].1.is_none() {
                    avm1_compile_stmt_list_with_flow(out, &catches[0].2, in_function, reg_map, ctx)
                } else if !catches.is_empty() {
                    let tmp_err = catch_var_name;
                    let mut tail_stmts = vec![SwfStmt::Throw(SwfExpr::Ident(tmp_err.clone()))];
                    for (cname, ctype_opt, cbody) in catches.iter().rev() {
                        let mut branch_body = Vec::with_capacity(cbody.len() + 2);
                        branch_body.push(SwfStmt::VarDecl {
                            name: cname.clone(),
                            init: Some(SwfExpr::Ident(tmp_err.clone())),
                        });
                        if !in_function && cname != &tmp_err {
                            branch_body.push(SwfStmt::Expr(SwfExpr::Delete(Box::new(
                                SwfLValue::Ident(tmp_err.clone()),
                            ))));
                        }
                        branch_body.extend(cbody.iter().cloned());
                        if let Some(ctype) = ctype_opt
                            && ctype != "*"
                            && ctype != "Object"
                        {
                            let type_expr = ctype
                                .split('.')
                                .fold(None, |acc: Option<SwfExpr>, seg| match acc {
                                    None => Some(SwfExpr::Ident(seg.to_owned())),
                                    Some(prev) => {
                                        Some(SwfExpr::Member(Box::new(prev), seg.to_owned()))
                                    }
                                })
                                .unwrap_or_else(|| SwfExpr::Ident(ctype.clone()));
                            tail_stmts = vec![SwfStmt::If {
                                cond: SwfExpr::Binary {
                                    op: SwfBinaryOp::InstanceOf,
                                    lhs: Box::new(SwfExpr::Ident(tmp_err.clone())),
                                    rhs: Box::new(type_expr),
                                },
                                then_branch: branch_body,
                                else_branch: Some(tail_stmts),
                            }];
                        } else {
                            tail_stmts = branch_body;
                        }
                    }
                    avm1_compile_stmt_list_with_flow(out, &tail_stmts, in_function, reg_map, ctx)
                } else {
                    Ok(())
                }
            } else {
                Ok(())
            };
            let catch_end = out.len();

            if finally_body.is_some() {
                let _ = ctx.finally_stack.pop();
            }
            if let Err(e) = try_res {
                ctx.try_depth = cur_try_depth;
                return Err(e);
            }
            if let Err(e) = catch_res {
                ctx.try_depth = cur_try_depth;
                return Err(e);
            }

            let finally_start = out.len();
            if let Some(fin_stmts) = finally_body
                && let Err(e) =
                    avm1_compile_stmt_list_with_flow(out, fin_stmts, in_function, reg_map, ctx)
            {
                ctx.try_depth = cur_try_depth;
                return Err(e);
            }
            let finally_end = out.len();
            ctx.try_depth = cur_try_depth;

            let try_size = u16::try_from(try_end - try_start)
                .map_err(|_| "AVM1 try block exceeds 65535 bytes".to_owned())?;
            let catch_size = u16::try_from(catch_end - catch_start)
                .map_err(|_| "AVM1 catch block exceeds 65535 bytes".to_owned())?;
            let finally_size = u16::try_from(finally_end - finally_start)
                .map_err(|_| "AVM1 finally block exceeds 65535 bytes".to_owned())?;
            out[sizes_patch_pos..sizes_patch_pos + 2].copy_from_slice(&try_size.to_le_bytes());
            out[sizes_patch_pos + 2..sizes_patch_pos + 4]
                .copy_from_slice(&catch_size.to_le_bytes());
            out[sizes_patch_pos + 4..sizes_patch_pos + 6]
                .copy_from_slice(&finally_size.to_le_bytes());
        }
        SwfStmt::ClassDecl {
            name,
            full_name,
            super_name,
            ctor,
            methods,
            fields,
        } => {
            let instance_prologue: Vec<SwfStmt> = fields
                .iter()
                .filter_map(|(is_static, f_name, f_init)| {
                    if !*is_static
                        && let Some(init_expr) = f_init
                        && !is_scalar_avm1_field_expr(init_expr)
                    {
                        Some(SwfStmt::Assign {
                            target: SwfLValue::Member(
                                Box::new(SwfExpr::Ident("this".to_owned())),
                                f_name.clone(),
                            ),
                            op: SwfAssignOp::Assign,
                            value: init_expr.clone(),
                        })
                    } else {
                        None
                    }
                })
                .collect();
            let empty_params: &[String] = &[];
            let (ctor_params, synthesized_body): (&[String], Vec<SwfStmt>) = match ctor {
                Some((p, b)) => {
                    if instance_prologue.is_empty() {
                        (p.as_slice(), b.clone())
                    } else {
                        let insert_pos = b
                            .iter()
                            .position(|s| {
                                matches!(
                                    s,
                                    SwfStmt::Expr(SwfExpr::Call { callee, .. })
                                        if matches!(callee.as_ref(), SwfExpr::Ident(id) if id == "super")
                                )
                            })
                            .map_or(0, |idx| idx + 1);
                        let mut merged = Vec::with_capacity(b.len() + instance_prologue.len());
                        merged.extend_from_slice(&b[..insert_pos]);
                        merged.extend(instance_prologue);
                        merged.extend_from_slice(&b[insert_pos..]);
                        (p.as_slice(), merged)
                    }
                }
                None => (empty_params, instance_prologue),
            };
            avm1_compile_function(out, name, None, ctor_params, &synthesized_body)?;
            // Register `_global.<pkg>.<Class> = <Class>` when packaged so both qualified
            // `pkg.Class` and short `Class` work in Flash Player 8 and round-trip on SWF import.
            let segs: Vec<&str> = full_name.split('.').filter(|s| !s.is_empty()).collect();
            if segs.len() >= 2 {
                for depth in 0..segs.len() - 1 {
                    let get_parent = |out: &mut Vec<u8>| -> Result<(), String> {
                        avm1_push_string(out, "_global")?;
                        out.push(0x1c); // GetVariable(_global)
                        for &seg in &segs[..depth] {
                            avm1_push_string(out, seg)?;
                            out.push(0x4e); // GetMember
                        }
                        Ok(())
                    };
                    get_parent(out)?;
                    avm1_push_string(out, segs[depth])?;
                    out.push(0x4e); // GetMember (_global...pkg)
                    out.push(0x12); // Not
                    let mut init_pkg = Vec::new();
                    get_parent(&mut init_pkg)?;
                    avm1_push_string(&mut init_pkg, segs[depth])?;
                    avm1_push_int(&mut init_pkg, 0);
                    init_pkg.push(0x43); // InitObject {}
                    init_pkg.push(0x4f); // SetMember
                    let skip_len = i16::try_from(init_pkg.len())
                        .map_err(|_| "AVM1 package init jump overflow".to_owned())?;
                    out.push(0x9d); // If (!exists) -> execute init_pkg, so invert: Not again!
                    // Wait: If jumps when top of stack is truthy! We want to SKIP init_pkg when pkg EXISTS!
                    // Above we pushed `!pkg`; let's remove `0x12` (Not) so `If (pkg)` skips `init_pkg`!
                    out.pop(); // pop the 0x9d
                    out.pop(); // pop the 0x12 (Not) -> top of stack is `Boolean(pkg)`!
                    out.push(0x9d); // ActionIf: jump over `init_pkg` if `pkg` already exists!
                    out.extend_from_slice(&2u16.to_le_bytes());
                    out.extend_from_slice(&skip_len.to_le_bytes());
                    out.extend_from_slice(&init_pkg);
                }
                avm1_push_string(out, "_global")?;
                out.push(0x1c);
                for &seg in &segs[..segs.len() - 1] {
                    avm1_push_string(out, seg)?;
                    out.push(0x4e);
                }
                avm1_push_string(out, segs[segs.len() - 1])?;
                avm1_push_string(out, name)?;
                out.push(0x1c); // GetVariable(Cls)
                out.push(0x4f); // SetMember (_global.<pkg>.<Cls> = Cls)
            }
            if let Some(sup) = super_name {
                avm1_push_string(out, name)?;
                out.push(0x1c); // ActionGetVariable(Sub)
                avm1_push_string(out, sup)?;
                out.push(0x1c); // ActionGetVariable(Sup)
                out.push(0x69); // ActionExtends
            }
            for (is_static, m_name, m_params, m_body) in methods {
                avm1_push_string(out, name)?;
                out.push(0x1c); // ActionGetVariable(Cls)
                if !*is_static {
                    avm1_push_string(out, "prototype")?;
                    out.push(0x4e); // ActionGetMember
                }
                avm1_push_string(out, m_name)?;
                avm1_compile_function(out, "", None, m_params, m_body)?;
                out.push(0x4f); // ActionSetMember
            }
            for (is_static, f_name, f_init) in fields {
                if let Some(init_expr) = f_init {
                    if !*is_static && !is_scalar_avm1_field_expr(init_expr) {
                        continue;
                    }
                    avm1_push_string(out, name)?;
                    out.push(0x1c); // ActionGetVariable(Cls)
                    if !*is_static {
                        avm1_push_string(out, "prototype")?;
                        out.push(0x4e); // ActionGetMember
                    }
                    avm1_push_string(out, f_name)?;
                    avm1_compile_expr(out, init_expr, reg_map)?;
                    out.push(0x4f); // ActionSetMember
                }
            }
        }
        SwfStmt::FunctionDecl {
            name, params, body, ..
        } => {
            avm1_compile_function(out, name, None, params, body)?;
        }
        SwfStmt::Return(expr_opt) => {
            if let Some(expr) = expr_opt {
                avm1_compile_expr(out, expr, reg_map)?;
            } else {
                out.push(0x96);
                out.extend_from_slice(&1u16.to_le_bytes());
                out.push(0x03); // Undefined
            }
            out.push(0x3e); // ActionReturn (ActionTry automatically runs active finally blocks)
        }
        SwfStmt::EventHandler { body, .. } => {
            avm1_compile_stmt_list_with_flow(out, body, in_function, reg_map, ctx)?;
        }
        SwfStmt::DefaultXmlNamespace(_) => {
            return Err(
                "ActionScript 3 'default xml namespace' is not supported in AVM1 SWF".to_owned(),
            );
        }
    }
    Ok(())
}

fn is_scalar_avm1_field_expr(expr: &SwfExpr) -> bool {
    match expr {
        SwfExpr::Null
        | SwfExpr::Undefined
        | SwfExpr::Bool(_)
        | SwfExpr::Int(_)
        | SwfExpr::Float(_)
        | SwfExpr::Str(_) => true,
        SwfExpr::Ident(id) => matches!(id.as_str(), "NaN" | "Infinity"),
        SwfExpr::Unary {
            op: SwfUnaryOp::Neg | SwfUnaryOp::Pos,
            expr: inner,
        } => is_scalar_avm1_field_expr(inner),
        _ => false,
    }
}

pub(super) fn compile_avm1_actions_mode(
    source: &str,
    allow_event_handlers: bool,
) -> Result<Vec<u8>, String> {
    let stmts = parse_swf_statements_strict(source, false)?;
    if !allow_event_handlers && find_event_handler_in_stmts(&stmts).is_some() {
        return Err(
            "AVM1 object event handler 'on(...)' / 'onClipEvent(...)' is only allowed on MovieClip or Button instances, not frame or init scripts"
                .to_owned(),
        );
    }
    let mut out = Vec::new();
    let empty_regs = HashMap::new();
    avm1_compile_stmt_list(&mut out, &stmts, false, &empty_regs)?;
    out.push(0x00); // ActionEndFlag
    Ok(out)
}

/// Compile an AS1/AS2 script source into AVM1 action bytecode (terminated by `0x00`).
pub(crate) fn compile_avm1_actions(source: &str) -> Result<Vec<u8>, String> {
    compile_avm1_actions_mode(source, false)
}
