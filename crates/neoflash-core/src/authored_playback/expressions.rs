use super::*;

impl AuthoredPlayback {
    pub(super) fn eval_preview_condition(
        &self,
        project: &Project,
        path: &[Id],
        cond: &str,
    ) -> bool {
        let is_as1 = self.is_as1();
        self.eval_preview_value(project, path, cond)
            .is_truthy(is_as1)
    }

    pub(super) fn eval_preview_expr(&self, project: &Project, path: &[Id], expr: &str) -> String {
        self.eval_preview_value(project, path, expr)
            .to_display_string()
    }

    pub fn eval_preview_value(&self, project: &Project, path: &[Id], expr: &str) -> PreviewValue {
        let scope_path = self.effective_scope_path(project, path);
        let is_as1 = self.is_as1();
        let lang = self.current_language;
        let is_avm1 = matches!(
            lang,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        );
        let mut trimmed = expr.trim();
        while trimmed.starts_with('(') && trimmed.ends_with(')') && is_single_paren_group(trimmed) {
            trimmed = trimmed[1..trimmed.len() - 1].trim();
        }
        if trimmed.is_empty() {
            return PreviewValue::String(String::new());
        }
        // 1. Ternary `cond ? yes : no`
        if let Some((cond_s, yes_s, no_s)) = split_top_level_ternary(trimmed) {
            return if self.eval_preview_condition(project, &scope_path, cond_s) {
                self.eval_preview_value(project, &scope_path, yes_s)
            } else {
                self.eval_preview_value(project, &scope_path, no_s)
            };
        }
        // 2. Logical OR (`||` and AS1/AS2 `or`)
        let or_ops: &[&str] = if is_avm1 { &["||", "or"] } else { &["||"] };
        if let Some((lhs_s, op, rhs_s)) = rfind_top_level_ops(trimmed, or_ops) {
            let lv = self.eval_preview_value(project, &scope_path, lhs_s);
            if op == "or" {
                let l_ok = lv.is_truthy(is_as1);
                let rv = self.eval_preview_value(project, &scope_path, rhs_s);
                return PreviewValue::Bool(l_ok || rv.is_truthy(is_as1));
            }
            if lv.is_truthy(is_as1) {
                return lv;
            }
            return self.eval_preview_value(project, &scope_path, rhs_s);
        }
        // 3. Logical AND (`&&` and AS1/AS2 `and`)
        let and_ops: &[&str] = if is_avm1 { &["&&", "and"] } else { &["&&"] };
        if let Some((lhs_s, op, rhs_s)) = rfind_top_level_ops(trimmed, and_ops) {
            let lv = self.eval_preview_value(project, &scope_path, lhs_s);
            if op == "and" {
                let l_ok = lv.is_truthy(is_as1);
                let rv = self.eval_preview_value(project, &scope_path, rhs_s);
                return PreviewValue::Bool(l_ok && rv.is_truthy(is_as1));
            }
            if !lv.is_truthy(is_as1) {
                return lv;
            }
            return self.eval_preview_value(project, &scope_path, rhs_s);
        }
        // 4. Bitwise OR (`|`)
        let bitor_parts = split_top_level_op(trimmed, "|");
        if bitor_parts.len() > 1 {
            let mut iter = bitor_parts.into_iter().map(|p| {
                self.eval_preview_value(project, &scope_path, p)
                    .to_i32(is_as1)
            });
            let first = iter.next().unwrap_or(0);
            return PreviewValue::Number(f64::from(iter.fold(first, |acc, r| acc | r)));
        }
        // 5. Bitwise XOR (`^`)
        let bitxor_parts = split_top_level_op(trimmed, "^");
        if bitxor_parts.len() > 1 {
            let mut iter = bitxor_parts.into_iter().map(|p| {
                self.eval_preview_value(project, &scope_path, p)
                    .to_i32(is_as1)
            });
            let first = iter.next().unwrap_or(0);
            return PreviewValue::Number(f64::from(iter.fold(first, |acc, r| acc ^ r)));
        }
        // 6. Bitwise AND (`&`)
        let bitand_parts = split_top_level_op(trimmed, "&");
        if bitand_parts.len() > 1 {
            let mut iter = bitand_parts.into_iter().map(|p| {
                self.eval_preview_value(project, &scope_path, p)
                    .to_i32(is_as1)
            });
            let first = iter.next().unwrap_or(0);
            return PreviewValue::Number(f64::from(iter.fold(first, |acc, r| acc & r)));
        }
        // 7. Equality (`===`, `!==`, `==`, `!=`, plus AS1/AS2 `<>`, `eq`, `ne`)
        let eq_ops: &[&str] = if is_avm1 {
            &["===", "!==", "<>", "==", "!=", "eq", "ne"]
        } else {
            &["===", "!==", "==", "!="]
        };
        if let Some((lhs_s, op, rhs_s)) = rfind_top_level_ops(trimmed, eq_ops) {
            let lv = self.eval_preview_value(project, &scope_path, lhs_s);
            let rv = self.eval_preview_value(project, &scope_path, rhs_s);
            let res = match op {
                "===" => lv.strict_eq(&rv),
                "!==" => !lv.strict_eq(&rv),
                "==" => lv.loose_eq(&rv),
                "!=" => !lv.loose_eq(&rv),
                "<>" => lv.to_number(is_as1) != rv.to_number(is_as1),
                "eq" => lv.to_es_string_for(lang) == rv.to_es_string_for(lang),
                "ne" => lv.to_es_string_for(lang) != rv.to_es_string_for(lang),
                _ => false,
            };
            return PreviewValue::Bool(res);
        }
        // 8. Relational & Membership / Type (`<=`, `>=`, `<`, `>`, AS1/AS2 `lt`..`ge`, `in`, `instanceof`, AS3 `is`/`as`)
        let rel_ops: &[&str] = if is_avm1 {
            &[
                "<=",
                ">=",
                "<",
                ">",
                "lt",
                "le",
                "gt",
                "ge",
                "instanceof",
                "in",
            ]
        } else if lang == ScriptLanguage::ActionScript3 {
            &["<=", ">=", "<", ">", "instanceof", "in", "is", "as"]
        } else {
            &["<=", ">=", "<", ">", "instanceof", "in"]
        };
        if let Some((lhs_s, op, rhs_s)) = rfind_top_level_ops(trimmed, rel_ops) {
            let lv = self.eval_preview_value(project, &scope_path, lhs_s);
            if matches!(op, "instanceof" | "is" | "as") {
                let ty = rhs_s.trim();
                let matched = match ty {
                    "Array" => matches!(lv, PreviewValue::Array(_)),
                    "Object" => {
                        matches!(lv, PreviewValue::Array(_) | PreviewValue::Object(_))
                            || (op != "instanceof"
                                && !matches!(lv, PreviewValue::Undefined | PreviewValue::Null))
                    }
                    "String" => op != "instanceof" && matches!(lv, PreviewValue::String(_)),
                    "Boolean" => op != "instanceof" && matches!(lv, PreviewValue::Bool(_)),
                    "Number" => op != "instanceof" && matches!(lv, PreviewValue::Number(_)),
                    "int" => {
                        op != "instanceof"
                            && matches!(
                                lv,
                                PreviewValue::Number(n)
                                    if n.is_finite()
                                        && (n - n.trunc()).abs() < 1e-9
                                        && n >= f64::from(i32::MIN)
                                        && n <= f64::from(i32::MAX)
                            )
                    }
                    "uint" => {
                        op != "instanceof"
                            && matches!(
                                lv,
                                PreviewValue::Number(n)
                                    if n.is_finite()
                                        && (n - n.trunc()).abs() < 1e-9
                                        && n >= 0.0
                                        && n <= f64::from(u32::MAX)
                            )
                    }
                    "*" => op != "instanceof",
                    _ => false,
                };
                if op == "as" {
                    return if matched { lv } else { PreviewValue::Null };
                }
                return PreviewValue::Bool(matched);
            }
            let rv = self.eval_preview_value(project, &scope_path, rhs_s);
            if op == "in" {
                let has = match &rv {
                    PreviewValue::Array(items) => {
                        let key_str = lv.to_es_string_for(lang);
                        if key_str == "length" {
                            true
                        } else if let Ok(idx) = key_str.trim().parse::<usize>() {
                            items
                                .get(idx)
                                .is_some_and(|v| !matches!(v, PreviewValue::Undefined))
                        } else {
                            false
                        }
                    }
                    PreviewValue::Object(entries) => {
                        let key_str = lv.to_es_string_for(lang);
                        entries.iter().any(|(k, _)| k == &key_str)
                    }
                    _ => false,
                };
                return PreviewValue::Bool(has);
            }
            if matches!(op, "lt" | "le" | "gt" | "ge") {
                let ls = lv.to_es_string_for(lang);
                let rs = rv.to_es_string_for(lang);
                let ord = ls.encode_utf16().cmp(rs.encode_utf16());
                let res = match op {
                    "lt" => ord.is_lt(),
                    "le" => !ord.is_gt(),
                    "gt" => ord.is_gt(),
                    "ge" => !ord.is_lt(),
                    _ => false,
                };
                return PreviewValue::Bool(res);
            }
            let ord = lv.compare_rel(&rv, is_as1);
            let res = match op {
                "<" => ord == Some(std::cmp::Ordering::Less),
                "<=" => matches!(
                    ord,
                    Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                ),
                ">" => ord == Some(std::cmp::Ordering::Greater),
                ">=" => matches!(
                    ord,
                    Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
                ),
                _ => false,
            };
            return PreviewValue::Bool(res);
        }
        // 9. Bitwise Shifts (`>>>`, `<<`, `>>` — left-to-right)
        if let Some((lhs_s, op, rhs_s)) = rfind_top_level_ops(trimmed, &[">>>", "<<", ">>"]) {
            let lv = self.eval_preview_value(project, &scope_path, lhs_s);
            let rv = self.eval_preview_value(project, &scope_path, rhs_s);
            let shift = rv.to_u32(is_as1) & 0x1f;
            let res = match op {
                "<<" => f64::from(lv.to_i32(is_as1).wrapping_shl(shift)),
                ">>" => f64::from(lv.to_i32(is_as1).wrapping_shr(shift)),
                ">>>" => f64::from(lv.to_u32(is_as1).wrapping_shr(shift)),
                _ => 0.0,
            };
            return PreviewValue::Number(res);
        }
        // 10. Additive (`+`, binary `-`, and AS1/AS2 `add` — left-to-right)
        let add_ops: &[&str] = if is_avm1 {
            &["+", "-", "add"]
        } else {
            &["+", "-"]
        };
        if let Some((lhs_s, op, rhs_s)) = rfind_top_level_ops(trimmed, add_ops) {
            let lv = self.eval_preview_value(project, &scope_path, lhs_s);
            let rv = self.eval_preview_value(project, &scope_path, rhs_s);
            return match op {
                "+" => eval_add_values(lv, rv, lang),
                "-" => PreviewValue::Number(lv.to_number(is_as1) - rv.to_number(is_as1)),
                "add" => {
                    let mut s = lv.to_es_string_for(lang);
                    s.push_str(&rv.to_es_string_for(lang));
                    PreviewValue::String(s)
                }
                _ => PreviewValue::Undefined,
            };
        }
        // 11. Multiplicative (`*`, `/`, `%` — left-to-right)
        if let Some((lhs_s, op, rhs_s)) = rfind_top_level_ops(trimmed, &["*", "/", "%"]) {
            let lv = self
                .eval_preview_value(project, &scope_path, lhs_s)
                .to_number(is_as1);
            let rv = self
                .eval_preview_value(project, &scope_path, rhs_s)
                .to_number(is_as1);
            return match op {
                "*" => PreviewValue::Number(lv * rv),
                "/" => {
                    if lv.is_nan() || rv.is_nan() {
                        PreviewValue::Number(f64::NAN)
                    } else if rv.abs() < 1e-12 {
                        PreviewValue::Number(0.0)
                    } else {
                        PreviewValue::Number(lv / rv)
                    }
                }
                "%" => {
                    if lv.is_nan() || rv.is_nan() {
                        PreviewValue::Number(f64::NAN)
                    } else if rv.abs() < 1e-12 {
                        PreviewValue::Number(0.0)
                    } else {
                        PreviewValue::Number(lv % rv)
                    }
                }
                _ => PreviewValue::Undefined,
            };
        }
        self.eval_preview_atom_value(project, &scope_path, trimmed)
    }

    pub(super) fn eval_preview_atom_value(
        &self,
        project: &Project,
        path: &[Id],
        atom: &str,
    ) -> PreviewValue {
        let s = atom.trim();
        let is_as1 = self.is_as1();
        let lang = self.current_language;
        if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
            || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
        {
            return PreviewValue::String(unescape_string_literal(&s[1..s.len() - 1]));
        }
        match s {
            "true" => return PreviewValue::Bool(true),
            "false" => return PreviewValue::Bool(false),
            "null" => return PreviewValue::Null,
            "undefined" => return PreviewValue::Undefined,
            "NaN" => return PreviewValue::Number(f64::NAN),
            "Infinity" => return PreviewValue::Number(f64::INFINITY),
            "-Infinity" => return PreviewValue::Number(f64::NEG_INFINITY),
            "newline"
                if matches!(
                    lang,
                    ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                ) =>
            {
                return PreviewValue::String("\n".to_owned());
            }
            _ => {}
        }
        if let Some(inner) = s.strip_prefix('!') {
            return PreviewValue::Bool(!self.eval_preview_condition(project, path, inner));
        }
        if matches!(
            lang,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        ) && let Some(inner) = strip_unary_keyword(s, "not")
        {
            return PreviewValue::Bool(!self.eval_preview_condition(project, path, inner));
        }
        if let Some(inner) = s.strip_prefix('~')
            && !inner.trim().is_empty()
        {
            let val = self.eval_preview_value(project, path, inner);
            return PreviewValue::Number(f64::from(!val.to_i32(is_as1)));
        }
        if let Some(inner) = strip_unary_keyword(s, "typeof") {
            let trimmed_inner = inner.trim();
            if self
                .resolve_receiver_path(project, path, trimmed_inner)
                .is_some()
                && !self
                    .vars
                    .contains_key(&(path.to_vec(), trimmed_inner.to_owned()))
            {
                let mc_ty = if matches!(
                    lang,
                    ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                ) {
                    "movieclip"
                } else {
                    "object"
                };
                return PreviewValue::String(mc_ty.to_owned());
            }
            let val = self.eval_preview_value(project, path, inner);
            return PreviewValue::String(val.type_of_str(lang).to_owned());
        }
        if let Some(inner) = strip_unary_keyword(s, "void") {
            let _ = self.eval_preview_value(project, path, inner);
            return PreviewValue::Undefined;
        }
        if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))
            && let Ok(v) = u64::from_str_radix(hex, 16)
        {
            return PreviewValue::Number(v as f64);
        }
        if let Ok(n) = s.parse::<f64>() {
            return PreviewValue::Number(n);
        }
        if let Some(inner) = s.strip_prefix('-')
            && !inner.trim().is_empty()
        {
            let val = self.eval_preview_value(project, path, inner);
            return PreviewValue::Number(-val.to_number(is_as1));
        }
        if let Some(inner) = s.strip_prefix('+')
            && !inner.trim().is_empty()
        {
            let val = self.eval_preview_value(project, path, inner);
            return PreviewValue::Number(val.to_number(is_as1));
        }
        if let Some(arr_val) = self.eval_array_or_object_literal(project, path, s) {
            return arr_val;
        }
        if let Some(builtin_val) = self.eval_builtin_pure_call(project, path, s) {
            return builtin_val;
        }
        if let Some(indexed_val) = self.eval_postfix_container_access(project, path, s) {
            return indexed_val;
        }
        if let Some(val) = self.vars.get(&(path.to_vec(), s.to_owned())) {
            return val.clone();
        }
        let (target_path, prop) = if let Some((recv, member)) = split_receiver_member(s)
            && let Some(resolved) = self.resolve_receiver_path(project, path, recv)
        {
            (resolved, member.trim())
        } else {
            (path.to_vec(), s)
        };
        if let Some(val) = self.vars.get(&(target_path.clone(), prop.to_owned())) {
            return val.clone();
        }
        match prop {
            "_currentframe" | "currentFrame" => {
                let f = self
                    .player
                    .clip_frame(&target_path)
                    .unwrap_or_else(|| self.player.frame());
                PreviewValue::Number(f64::from(f + 1))
            }
            "_totalframes" | "totalFrames" => {
                let sym_id = if target_path.is_empty() {
                    Some(self.symbol.as_str())
                } else {
                    self.player
                        .previous
                        .get(&target_path)
                        .and_then(|n| n.symbol_id.as_deref())
                };
                let count = sym_id
                    .and_then(|id| project.symbol(id))
                    .map_or(1, |sym| sym.timeline.frame_count.max(1));
                PreviewValue::Number(f64::from(count))
            }
            "_name" | "name" => PreviewValue::String(
                target_path
                    .last()
                    .map(|id| {
                        let parent = &target_path[..target_path.len() - 1];
                        self.element_display_name(project, parent, id)
                    })
                    .unwrap_or_else(|| "root".into()),
            ),
            "_visible" | "visible" => {
                let vis = self
                    .player
                    .overrides
                    .get(&target_path)
                    .and_then(|o| o.visible)
                    .unwrap_or(true);
                PreviewValue::Bool(vis)
            }
            "x" | "_x" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "x")
                    .unwrap_or(0.0),
            ),
            "y" | "_y" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "y")
                    .unwrap_or(0.0),
            ),
            "rotation" | "_rotation" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "rotation")
                    .unwrap_or(0.0),
            ),
            "scaleX" | "scale_x" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "scale_x")
                    .unwrap_or(1.0),
            ),
            "_xscale" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "scale_x")
                    .map_or(100.0, |v| v * 100.0),
            ),
            "scaleY" | "scale_y" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "scale_y")
                    .unwrap_or(1.0),
            ),
            "_yscale" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "scale_y")
                    .map_or(100.0, |v| v * 100.0),
            ),
            "alpha" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "alpha")
                    .unwrap_or(1.0),
            ),
            "_alpha" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "alpha")
                    .map_or(100.0, |a| a * 100.0),
            ),
            _ => PreviewValue::Undefined,
        }
    }

    pub(super) fn eval_builtin_pure_call(
        &self,
        project: &Project,
        path: &[Id],
        s: &str,
    ) -> Option<PreviewValue> {
        let (head, args_raw) = parse_qualified_call(s.trim())?;
        if !is_supported_builtin_pure_call_head_for(head, self.current_language) {
            return None;
        }
        let is_as1 = self.is_as1();
        let lang = self.current_language;
        let args: Vec<PreviewValue> = split_top_level_comma(args_raw)
            .into_iter()
            .map(|a| self.eval_preview_value(project, path, a))
            .collect();
        match head {
            "eval" => {
                let first = args.first()?;
                if matches!(first, PreviewValue::Array(_) | PreviewValue::Object(_)) {
                    return Some(first.clone());
                }
                if matches!(
                    lang,
                    ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                ) {
                    let PreviewValue::String(ref path_str) = *first else {
                        return Some(PreviewValue::Undefined);
                    };
                    return Some(self.resolve_eval_path_value(project, path, path_str, is_as1));
                }
                if matches!(
                    lang,
                    ScriptLanguage::JavaScript | ScriptLanguage::TypeScript
                ) {
                    let PreviewValue::String(ref code_str) = *first else {
                        return Some(first.clone());
                    };
                    if code_str.contains("eval") {
                        return Some(PreviewValue::Undefined);
                    }
                    return Some(self.eval_preview_value(project, path, code_str));
                }
                None
            }
            "chr" | "mbchr" => {
                let unit = args.first().map_or(0, |v| v.to_i32(is_as1)) as u16;
                if unit == 0 {
                    Some(PreviewValue::String(String::new()))
                } else {
                    let ch = char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER);
                    Some(PreviewValue::String(ch.to_string()))
                }
            }
            "ord" | "mbord" => {
                let st = args
                    .first()
                    .map_or_else(String::new, |v| v.to_es_string_for(lang));
                let unit = st.encode_utf16().next().unwrap_or(0);
                let code = if unit == 0 {
                    0.0
                } else {
                    f64::from(u32::from(
                        char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER),
                    ))
                };
                Some(PreviewValue::Number(code))
            }
            "length" | "mblength" => {
                let st = args
                    .first()
                    .map_or_else(String::new, |v| v.to_es_string_for(lang));
                Some(PreviewValue::Number(st.encode_utf16().count() as f64))
            }
            "substring" | "mbsubstring" => {
                let st = args
                    .first()
                    .map_or_else(String::new, |v| v.to_es_string_for(lang));
                let units: Vec<u16> = st.encode_utf16().collect();
                let start_1 = args.get(1).map_or(0, |v| v.to_i32(is_as1));
                let start = if start_1 >= 1 {
                    (start_1 as usize).saturating_sub(1).min(units.len())
                } else {
                    0
                };
                let count = args
                    .get(2)
                    .and_then(|v| usize::try_from(v.to_i32(is_as1)).ok());
                let end = count
                    .and_then(|c| start.checked_add(c))
                    .filter(|e| *e <= units.len())
                    .unwrap_or(units.len());
                Some(PreviewValue::String(String::from_utf16_lossy(
                    &units[start.min(end)..end],
                )))
            }
            "String" => Some(PreviewValue::String(
                args.first()
                    .map_or_else(String::new, |v| v.to_es_string_for(lang)),
            )),
            "Number" => Some(PreviewValue::Number(
                args.first().map_or(0.0, |v| v.to_number(is_as1)),
            )),
            "Boolean" => Some(PreviewValue::Bool(
                args.first().is_some_and(|v| v.is_truthy(is_as1)),
            )),
            "int" => Some(PreviewValue::Number(f64::from(
                args.first().map_or(0, |v| v.to_i32(is_as1)),
            ))),
            "uint" => Some(PreviewValue::Number(f64::from(
                args.first().map_or(0, |v| v.to_u32(is_as1)),
            ))),
            "parseFloat" => {
                let st = args
                    .first()
                    .map(|v| v.to_es_string_for(lang))
                    .unwrap_or_default();
                let trimmed = st.trim_start();
                let end = trimmed
                    .char_indices()
                    .take_while(|&(idx, c)| {
                        c.is_ascii_digit()
                            || c == '.'
                            || ((c == '+' || c == '-') && idx == 0)
                            || c == 'e'
                            || c == 'E'
                    })
                    .map(|(idx, c)| idx + c.len_utf8())
                    .last()
                    .unwrap_or(0);
                let parsed = trimmed[..end].parse::<f64>().unwrap_or(f64::NAN);
                Some(PreviewValue::Number(parsed))
            }
            "parseInt" => {
                let st = args
                    .first()
                    .map(|v| v.to_es_string_for(lang))
                    .unwrap_or_default();
                let radix_raw = args.get(1).map_or(0.0, |v| v.to_number(is_as1));
                let mut trimmed = st.trim_start();
                let mut sign = 1.0f64;
                if let Some(rest) = trimmed.strip_prefix('-') {
                    sign = -1.0;
                    trimmed = rest;
                } else if let Some(rest) = trimmed.strip_prefix('+') {
                    trimmed = rest;
                }
                let mut radix = if radix_raw.is_finite() && radix_raw != 0.0 {
                    radix_raw.trunc() as i32
                } else {
                    0
                };
                if (radix == 0 || radix == 16)
                    && let Some(hex) = trimmed
                        .strip_prefix("0x")
                        .or_else(|| trimmed.strip_prefix("0X"))
                {
                    trimmed = hex;
                    radix = 16;
                } else if radix == 0 {
                    radix = 10;
                }
                if !(2..=36).contains(&radix) {
                    return Some(PreviewValue::Number(f64::NAN));
                }
                let r_u32 = radix as u32;
                let digits: String = trimmed.chars().take_while(|c| c.is_digit(r_u32)).collect();
                if digits.is_empty() {
                    return Some(PreviewValue::Number(f64::NAN));
                }
                let val = i128::from_str_radix(&digits, r_u32)
                    .map(|v| (v as f64) * sign)
                    .unwrap_or(f64::NAN);
                Some(PreviewValue::Number(val))
            }
            "Math.abs" => Some(PreviewValue::Number(
                args.first().map_or(f64::NAN, |v| v.to_number(is_as1).abs()),
            )),
            "Math.floor" => Some(PreviewValue::Number(
                args.first()
                    .map_or(f64::NAN, |v| v.to_number(is_as1).floor()),
            )),
            "Math.ceil" => Some(PreviewValue::Number(
                args.first()
                    .map_or(f64::NAN, |v| v.to_number(is_as1).ceil()),
            )),
            "Math.round" => Some(PreviewValue::Number(args.first().map_or(f64::NAN, |v| {
                let n = v.to_number(is_as1);
                if n.is_finite() { (n + 0.5).floor() } else { n }
            }))),
            "Math.sqrt" => Some(PreviewValue::Number(
                args.first()
                    .map_or(f64::NAN, |v| v.to_number(is_as1).sqrt()),
            )),
            "Math.pow" => {
                let a = args.first().map_or(f64::NAN, |v| v.to_number(is_as1));
                let b = args.get(1).map_or(f64::NAN, |v| v.to_number(is_as1));
                Some(PreviewValue::Number(a.powf(b)))
            }
            "Math.min" => {
                if args.is_empty() {
                    return Some(PreviewValue::Number(f64::INFINITY));
                }
                let mut acc = f64::INFINITY;
                for v in &args {
                    let n = v.to_number(is_as1);
                    if n.is_nan() {
                        return Some(PreviewValue::Number(f64::NAN));
                    }
                    if n < acc {
                        acc = n;
                    }
                }
                Some(PreviewValue::Number(acc))
            }
            "Math.max" => {
                if args.is_empty() {
                    return Some(PreviewValue::Number(f64::NEG_INFINITY));
                }
                let mut acc = f64::NEG_INFINITY;
                for v in &args {
                    let n = v.to_number(is_as1);
                    if n.is_nan() {
                        return Some(PreviewValue::Number(f64::NAN));
                    }
                    if n > acc {
                        acc = n;
                    }
                }
                Some(PreviewValue::Number(acc))
            }
            _ => None,
        }
    }

    pub(super) fn eval_array_or_object_literal(
        &self,
        project: &Project,
        path: &[Id],
        s: &str,
    ) -> Option<PreviewValue> {
        let trimmed = s.trim();
        if trimmed.starts_with('[')
            && find_matching_delim(trimmed, b'[', b']') == Some(trimmed.len() - 1)
        {
            let inner = trimmed[1..trimmed.len() - 1].trim();
            if inner.is_empty() {
                return Some(PreviewValue::Array(Vec::new()));
            }
            let mut parts = split_top_level_comma(inner);
            if parts.last() == Some(&"") {
                parts.pop();
            }
            let items = parts
                .into_iter()
                .map(|part| {
                    if part.trim().is_empty() {
                        PreviewValue::Undefined
                    } else {
                        self.eval_preview_value(project, path, part)
                    }
                })
                .collect();
            return Some(PreviewValue::Array(items));
        }

        let array_call_args = if trimmed == "new Array" {
            Some("")
        } else if let Some(rest) = trimmed.strip_prefix("new ") {
            extract_call_arg(rest.trim_start(), "Array")
        } else {
            extract_call_arg(trimmed, "Array")
        };
        if let Some(arg_str) = array_call_args {
            let args = split_top_level_comma(arg_str);
            if args.is_empty() {
                return Some(PreviewValue::Array(Vec::new()));
            }
            if args.len() == 1 {
                let first = self.eval_preview_value(project, path, args[0]);
                if let PreviewValue::Number(n) = first
                    && n.is_finite()
                    && n >= 0.0
                    && (n - n.trunc()).abs() < 1e-9
                    && (n as usize) <= 10_000
                {
                    return Some(PreviewValue::Array(vec![
                        PreviewValue::Undefined;
                        n as usize
                    ]));
                }
                return Some(PreviewValue::Array(vec![first]));
            }
            let items = args
                .into_iter()
                .map(|a| self.eval_preview_value(project, path, a))
                .collect();
            return Some(PreviewValue::Array(items));
        }

        if trimmed == "new Object"
            || trimmed
                .strip_prefix("new ")
                .and_then(|r| extract_call_arg(r.trim_start(), "Object"))
                == Some("")
        {
            return Some(PreviewValue::Object(Vec::new()));
        }

        if trimmed.starts_with('{')
            && find_matching_delim(trimmed, b'{', b'}') == Some(trimmed.len() - 1)
        {
            let inner = trimmed[1..trimmed.len() - 1].trim();
            if inner.is_empty() {
                return Some(PreviewValue::Object(Vec::new()));
            }
            let mut parts = split_top_level_comma(inner);
            if parts.last() == Some(&"") {
                parts.pop();
            }
            let mut entries: Vec<(String, PreviewValue)> = Vec::with_capacity(parts.len());
            for entry_str in parts {
                let (raw_key, raw_val) = split_top_level_colon(entry_str)?;
                let key = parse_object_literal_key(raw_key)?;
                let val = self.eval_preview_value(project, path, raw_val);
                if let Some((_, slot)) = entries.iter_mut().find(|(k, _)| k == &key) {
                    *slot = val;
                } else {
                    entries.push((key, val));
                }
            }
            return Some(PreviewValue::Object(entries));
        }

        None
    }

    pub(super) fn resolve_container_chain<'a>(
        &self,
        project: &Project,
        path: &[Id],
        base: &'a str,
        tokens: &[PostfixToken<'a>],
    ) -> Option<(PreviewValue, usize)> {
        if base.starts_with('(')
            || base.starts_with('[')
            || base.starts_with('{')
            || base.starts_with('"')
            || base.starts_with('\'')
            || base.starts_with("new ")
            || base.ends_with(')')
        {
            return Some((self.eval_preview_value(project, path, base), 0));
        }
        if let Some(val) = self.vars.get(&(path.to_vec(), base.to_owned())) {
            return Some((val.clone(), 0));
        }
        // Check receiver-qualified container variable, e.g. `this.fig[0]`, `_root.board[1][2]`, `mc.state.hp`
        let mut recv_prefix = base.to_owned();
        for (idx, tok) in tokens.iter().enumerate() {
            let PostfixToken::Prop(prop_name) = tok else {
                break;
            };
            if let Some(recv_path) = self.resolve_receiver_path(project, path, &recv_prefix)
                && let Some(val) = self.vars.get(&(recv_path, (*prop_name).to_owned()))
                && (idx + 1 < tokens.len()
                    || matches!(val, PreviewValue::Array(_) | PreviewValue::Object(_)))
            {
                return Some((val.clone(), idx + 1));
            }
            recv_prefix.push('.');
            recv_prefix.push_str(prop_name);
        }
        None
    }

    pub(super) fn resolve_mutable_container_root<'a>(
        &self,
        project: &Project,
        scope_path: &[Id],
        base: &'a str,
        tokens: &[PostfixToken<'a>],
        allow_empty_remaining: bool,
    ) -> Option<(Vec<Id>, String, usize)> {
        if let Some(PreviewValue::Array(_) | PreviewValue::Object(_)) =
            self.vars.get(&(scope_path.to_vec(), base.to_owned()))
            && (allow_empty_remaining || !tokens.is_empty())
        {
            return Some((scope_path.to_vec(), base.to_owned(), 0));
        }
        let mut recv_prefix = base.to_owned();
        for (idx, tok) in tokens.iter().enumerate() {
            let PostfixToken::Prop(prop_name) = tok else {
                break;
            };
            if let Some(recv_path) = self.resolve_receiver_path(project, scope_path, &recv_prefix)
                && let Some(PreviewValue::Array(_) | PreviewValue::Object(_)) =
                    self.vars.get(&(recv_path.clone(), (*prop_name).to_owned()))
                && (allow_empty_remaining || idx + 1 < tokens.len())
            {
                return Some((recv_path, (*prop_name).to_owned(), idx + 1));
            }
            recv_prefix.push('.');
            recv_prefix.push_str(prop_name);
        }
        None
    }

    pub(super) fn eval_postfix_container_access(
        &self,
        project: &Project,
        path: &[Id],
        s: &str,
    ) -> Option<PreviewValue> {
        let (base, tokens) = parse_postfix_steps(s.trim())?;
        if tokens.is_empty() {
            return None;
        }
        let (mut cur, start_idx) = self.resolve_container_chain(project, path, base, &tokens)?;
        if start_idx >= tokens.len() {
            return Some(cur);
        }
        let is_as1 = self.is_as1();
        for tok in &tokens[start_idx..] {
            cur = match tok {
                PostfixToken::Prop(prop) => match &cur {
                    PreviewValue::Array(items) if *prop == "length" => {
                        PreviewValue::Number(items.len() as f64)
                    }
                    PreviewValue::String(st) if *prop == "length" => {
                        PreviewValue::Number(st.encode_utf16().count() as f64)
                    }
                    PreviewValue::Object(entries) => entries
                        .iter()
                        .find(|(k, _)| k == prop)
                        .map(|(_, v)| v.clone())
                        .unwrap_or(PreviewValue::Undefined),
                    _ => PreviewValue::Undefined,
                },
                PostfixToken::Index(idx_expr) => {
                    let key_val = self.eval_preview_value(project, path, idx_expr);
                    match (&cur, &key_val) {
                        (PreviewValue::Array(items), PreviewValue::Number(n))
                            if n.is_finite() && *n >= 0.0 && (*n - n.trunc()).abs() < 1e-9 =>
                        {
                            items
                                .get(*n as usize)
                                .cloned()
                                .unwrap_or(PreviewValue::Undefined)
                        }
                        (PreviewValue::Array(items), PreviewValue::String(st)) => {
                            if st == "length" {
                                PreviewValue::Number(items.len() as f64)
                            } else if let Ok(idx) = st.trim().parse::<usize>() {
                                items.get(idx).cloned().unwrap_or(PreviewValue::Undefined)
                            } else {
                                PreviewValue::Undefined
                            }
                        }
                        (PreviewValue::Object(entries), _) => {
                            let key = key_val.to_es_string(is_as1);
                            entries
                                .iter()
                                .find(|(k, _)| k == &key)
                                .map(|(_, v)| v.clone())
                                .unwrap_or(PreviewValue::Undefined)
                        }
                        _ => PreviewValue::Undefined,
                    }
                }
                PostfixToken::Call(method, args_raw) => {
                    self.eval_postfix_method(project, path, &cur, method, args_raw)?
                }
            };
        }
        Some(cur)
    }

    pub(super) fn eval_postfix_method(
        &self,
        project: &Project,
        path: &[Id],
        receiver: &PreviewValue,
        method: &str,
        args_raw: &str,
    ) -> Option<PreviewValue> {
        let is_as1 = self.is_as1();
        let lang = self.current_language;
        let args: Vec<PreviewValue> = split_top_level_comma(args_raw)
            .into_iter()
            .map(|a| self.eval_preview_value(project, path, a))
            .collect();

        match (receiver, method) {
            (PreviewValue::Array(items), "join") => {
                let sep = match args.first() {
                    None | Some(PreviewValue::Undefined) => ",".to_owned(),
                    Some(v) => v.to_es_string_for(lang),
                };
                Some(PreviewValue::String(PreviewValue::join_array_elements(
                    items, &sep, lang,
                )))
            }
            (PreviewValue::Array(items), "toString") if args.is_empty() => Some(
                PreviewValue::String(PreviewValue::join_array_elements(items, ",", lang)),
            ),
            (PreviewValue::Array(items), "slice") => {
                let len = items.len() as i64;
                let rel = |raw: Option<&PreviewValue>, default: i64| -> usize {
                    let Some(v) = raw else {
                        return default.clamp(0, len) as usize;
                    };
                    if matches!(v, PreviewValue::Undefined) {
                        return default.clamp(0, len) as usize;
                    }
                    let n = v.to_number(is_as1);
                    if n.is_nan() || n == 0.0 {
                        0
                    } else if n == f64::INFINITY {
                        len as usize
                    } else if n == f64::NEG_INFINITY {
                        0
                    } else {
                        let i = n.trunc() as i64;
                        (if i < 0 { (len + i).max(0) } else { i.min(len) }) as usize
                    }
                };
                let start = rel(args.first(), 0);
                let end = rel(args.get(1), len);
                let out = if end > start {
                    items[start..end].to_vec()
                } else {
                    Vec::new()
                };
                Some(PreviewValue::Array(out))
            }
            (PreviewValue::Array(items), "concat") => {
                let mut out = items.clone();
                for arg in args {
                    if let PreviewValue::Array(extra) = arg {
                        let room = 10_000usize.saturating_sub(out.len());
                        out.extend(extra.into_iter().take(room));
                    } else if out.len() < 10_000 {
                        out.push(arg);
                    }
                }
                Some(PreviewValue::Array(out))
            }
            (PreviewValue::Array(items), "indexOf") => {
                let needle = args.first().unwrap_or(&PreviewValue::Undefined);
                let len = items.len() as i64;
                let from_raw = args.get(1).map_or(0.0, |v| v.to_number(is_as1));
                let mut start = if from_raw.is_nan() || from_raw == 0.0 {
                    0
                } else if from_raw.is_infinite() {
                    if from_raw.is_sign_positive() { len } else { 0 }
                } else {
                    let i = from_raw.trunc() as i64;
                    if i < 0 { (len + i).max(0) } else { i }
                };
                if start < 0 {
                    start = 0;
                }
                let found = (start as usize..items.len())
                    .find(|&idx| items[idx].strict_eq(needle))
                    .map_or(-1.0, |idx| idx as f64);
                Some(PreviewValue::Number(found))
            }
            (PreviewValue::Array(items), "lastIndexOf") => {
                let needle = args.first().unwrap_or(&PreviewValue::Undefined);
                if items.is_empty() {
                    return Some(PreviewValue::Number(-1.0));
                }
                let len = items.len() as i64;
                let from_raw = args
                    .get(1)
                    .map_or((len - 1) as f64, |v| v.to_number(is_as1));
                let end_idx = if from_raw.is_nan() {
                    0i64
                } else if from_raw.is_infinite() {
                    if from_raw.is_sign_positive() {
                        len - 1
                    } else {
                        -1
                    }
                } else {
                    let i = from_raw.trunc() as i64;
                    if i < 0 { len + i } else { i.min(len - 1) }
                };
                if end_idx < 0 {
                    return Some(PreviewValue::Number(-1.0));
                }
                let found = (0..=end_idx as usize)
                    .rev()
                    .find(|&idx| items[idx].strict_eq(needle))
                    .map_or(-1.0, |idx| idx as f64);
                Some(PreviewValue::Number(found))
            }
            (PreviewValue::String(st), "split") => {
                let limit = args
                    .get(1)
                    .and_then(|v| {
                        if matches!(v, PreviewValue::Undefined) {
                            None
                        } else {
                            let n = v.to_number(is_as1);
                            (n.is_finite() && n >= 0.0).then_some((n.trunc() as usize).min(10_000))
                        }
                    })
                    .unwrap_or(10_000);
                if limit == 0 {
                    return Some(PreviewValue::Array(Vec::new()));
                }
                let out: Vec<PreviewValue> = match args.first() {
                    None | Some(PreviewValue::Undefined) => vec![PreviewValue::String(st.clone())],
                    Some(sep_val) => {
                        let sep = sep_val.to_es_string_for(lang);
                        if sep.is_empty() {
                            st.chars()
                                .take(limit)
                                .map(|c| PreviewValue::String(c.to_string()))
                                .collect()
                        } else {
                            st.split(&sep)
                                .take(limit)
                                .map(|part| PreviewValue::String(part.to_owned()))
                                .collect()
                        }
                    }
                };
                Some(PreviewValue::Array(out))
            }
            (PreviewValue::String(st), "charAt") => {
                let n = args.first().map_or(0.0, |v| v.to_number(is_as1));
                let idx = if n.is_nan() || n == 0.0 {
                    0isize
                } else if !n.is_finite() {
                    -1
                } else {
                    n.trunc() as isize
                };
                let ch = if idx < 0 {
                    String::new()
                } else {
                    st.chars()
                        .nth(idx as usize)
                        .map_or_else(String::new, |c| c.to_string())
                };
                Some(PreviewValue::String(ch))
            }
            (PreviewValue::String(st), "charCodeAt") => {
                let n = args.first().map_or(0.0, |v| v.to_number(is_as1));
                let idx = if n.is_nan() || n == 0.0 {
                    0isize
                } else if !n.is_finite() {
                    -1
                } else {
                    n.trunc() as isize
                };
                let code = if idx < 0 {
                    f64::NAN
                } else {
                    st.encode_utf16()
                        .nth(idx as usize)
                        .map_or(f64::NAN, f64::from)
                };
                Some(PreviewValue::Number(code))
            }
            (PreviewValue::String(st), "indexOf") => {
                let needle = args
                    .first()
                    .map_or_else(|| "undefined".to_owned(), |v| v.to_es_string_for(lang));
                let chars: Vec<char> = st.chars().collect();
                let from_raw = args.get(1).map_or(0.0, |v| v.to_number(is_as1));
                let start_char = if !from_raw.is_finite() {
                    if from_raw.is_sign_positive() {
                        chars.len()
                    } else {
                        0
                    }
                } else {
                    (from_raw.trunc() as isize).clamp(0, chars.len() as isize) as usize
                };
                let byte_start = st
                    .char_indices()
                    .nth(start_char)
                    .map_or(st.len(), |(b, _)| b);
                let found = st[byte_start..].find(&needle).map_or(-1.0, |rel_b| {
                    st[..byte_start + rel_b].chars().count() as f64
                });
                Some(PreviewValue::Number(found))
            }
            (PreviewValue::String(st), "lastIndexOf") => {
                let needle = args
                    .first()
                    .map_or_else(|| "undefined".to_owned(), |v| v.to_es_string_for(lang));
                let found = st
                    .rfind(&needle)
                    .map_or(-1.0, |b| st[..b].chars().count() as f64);
                Some(PreviewValue::Number(found))
            }
            (PreviewValue::String(st), "slice" | "substring" | "substr") => {
                let chars: Vec<char> = st.chars().collect();
                let len = chars.len() as i64;
                let (s_idx, e_idx) = if method == "substr" {
                    let s_raw = args.first().map_or(0.0, |v| v.to_number(is_as1));
                    let start = if s_raw.is_nan() || s_raw == 0.0 {
                        0
                    } else if !s_raw.is_finite() {
                        if s_raw.is_sign_positive() { len } else { 0 }
                    } else {
                        let i = s_raw.trunc() as i64;
                        if i < 0 { (len + i).max(0) } else { i.min(len) }
                    };
                    let count = match args.get(1) {
                        None | Some(PreviewValue::Undefined) => len - start,
                        Some(v) => {
                            let c = v.to_number(is_as1);
                            if c.is_nan() || c <= 0.0 {
                                0
                            } else if c.is_infinite() {
                                len - start
                            } else {
                                (c.trunc() as i64).min(len - start)
                            }
                        }
                    };
                    (start as usize, (start + count) as usize)
                } else if method == "substring" {
                    let norm = |raw: Option<&PreviewValue>, def: i64| -> i64 {
                        let Some(v) = raw else { return def };
                        if matches!(v, PreviewValue::Undefined) {
                            return def;
                        }
                        let n = v.to_number(is_as1);
                        if n.is_nan() || n <= 0.0 {
                            0
                        } else if n.is_infinite() {
                            len
                        } else {
                            (n.trunc() as i64).clamp(0, len)
                        }
                    };
                    let a = norm(args.first(), 0);
                    let b = norm(args.get(1), len);
                    (a.min(b) as usize, a.max(b) as usize)
                } else {
                    let norm = |raw: Option<&PreviewValue>, def: i64| -> i64 {
                        let Some(v) = raw else { return def };
                        if matches!(v, PreviewValue::Undefined) {
                            return def;
                        }
                        let n = v.to_number(is_as1);
                        if n.is_nan() || n == 0.0 {
                            0
                        } else if n.is_infinite() {
                            if n.is_sign_positive() { len } else { 0 }
                        } else {
                            let i = n.trunc() as i64;
                            if i < 0 { (len + i).max(0) } else { i.min(len) }
                        }
                    };
                    let a = norm(args.first(), 0);
                    let b = norm(args.get(1), len);
                    (a as usize, b.max(a) as usize)
                };
                Some(PreviewValue::String(
                    chars[s_idx..e_idx].iter().collect::<String>(),
                ))
            }
            (PreviewValue::String(st), "toUpperCase") if args.is_empty() => {
                Some(PreviewValue::String(st.to_uppercase()))
            }
            (PreviewValue::String(st), "toLowerCase") if args.is_empty() => {
                Some(PreviewValue::String(st.to_lowercase()))
            }
            (PreviewValue::String(st), "trim") if args.is_empty() => {
                Some(PreviewValue::String(st.trim().to_owned()))
            }
            (val, "toString")
                if args.is_empty()
                    && !matches!(val, PreviewValue::Undefined | PreviewValue::Null) =>
            {
                Some(PreviewValue::String(val.to_es_string_for(lang)))
            }
            (val, "valueOf")
                if args.is_empty()
                    && !matches!(val, PreviewValue::Undefined | PreviewValue::Null) =>
            {
                Some(val.clone())
            }
            _ => None,
        }
    }

    pub(super) fn concrete_access_steps(
        &self,
        project: &Project,
        scope_path: &[Id],
        tokens: &[PostfixToken<'_>],
        is_as1: bool,
    ) -> Option<Vec<ConcreteAccessStep>> {
        let mut out = Vec::with_capacity(tokens.len());
        for tok in tokens {
            let step = match tok {
                PostfixToken::Call(_, _) => return None,
                PostfixToken::Prop(p) => ConcreteAccessStep::Prop((*p).to_owned()),
                PostfixToken::Index(idx_expr) => {
                    let kv = self.eval_preview_value(project, scope_path, idx_expr);
                    match kv {
                        PreviewValue::Number(n)
                            if n.is_finite()
                                && n >= 0.0
                                && (n - n.trunc()).abs() < 1e-9
                                && (n as usize) <= 10_000 =>
                        {
                            ConcreteAccessStep::Index(n as usize)
                        }
                        PreviewValue::String(ref st) => {
                            if let Ok(idx) = st.trim().parse::<usize>()
                                && idx <= 10_000
                            {
                                ConcreteAccessStep::Index(idx)
                            } else {
                                ConcreteAccessStep::Prop(st.clone())
                            }
                        }
                        other => ConcreteAccessStep::Prop(other.to_es_string(is_as1)),
                    }
                }
            };
            out.push(step);
        }
        Some(out)
    }

    pub(super) fn is_supported_rhs(&self, rhs: &str) -> bool {
        is_supported_preview_rhs_for(rhs, self.current_language)
    }

    pub(super) fn find_var_key_in_scope(
        &self,
        target_path: &[Id],
        name: &str,
        is_as1: bool,
    ) -> Option<String> {
        if self
            .vars
            .contains_key(&(target_path.to_vec(), name.to_owned()))
        {
            return Some(name.to_owned());
        }
        if is_as1 {
            return self
                .vars
                .keys()
                .find(|(p, k)| p.as_slice() == target_path && k.eq_ignore_ascii_case(name))
                .map(|(_, k)| k.clone());
        }
        None
    }

    pub(super) fn find_named_child_id_matching(
        &self,
        project: &Project,
        parent_path: &[Id],
        instance_name: &str,
        is_as1: bool,
    ) -> Option<Id> {
        if let Some(exact) = self.find_named_child_id(project, parent_path, instance_name) {
            return Some(exact);
        }
        if !is_as1 {
            return None;
        }
        let sym_id = if parent_path.is_empty() {
            Some(self.symbol.as_str())
        } else {
            self.player
                .previous
                .get(parent_path)
                .and_then(|n| n.symbol_id.as_deref())
        }?;
        let sym = project.symbol(sym_id)?;
        let cur_frame = self
            .player
            .clip_frame(parent_path)
            .unwrap_or_else(|| self.player.frame());
        for layer in sym.timeline.layers.iter().rev() {
            for el in crate::evaluate::sample_layer_in_at_fps(
                &sym.timeline,
                layer,
                cur_frame,
                f64::from(project.fps.max(1)),
            )
            .into_iter()
            .rev()
            {
                if matches!(el.kind, ElementKind::Instance { .. })
                    && (el.name.eq_ignore_ascii_case(instance_name)
                        || el.id.eq_ignore_ascii_case(instance_name))
                {
                    let mut candidate = parent_path.to_vec();
                    candidate.push(el.id.clone());
                    if self.player.previous.contains_key(&candidate) {
                        return Some(el.id);
                    }
                }
            }
        }
        None
    }
}
