use super::*;

impl AuthoredPlayback {
    pub(super) fn resolve_eval_path_value(
        &self,
        project: &Project,
        scope_path: &[Id],
        raw_path: &str,
        is_as1: bool,
    ) -> PreviewValue {
        let Ok(parts) = crate::as2_object::reference_path(raw_path) else {
            return PreviewValue::Undefined;
        };
        if parts.is_empty() {
            return PreviewValue::Undefined;
        }
        let match_kw = |s: &str, kw: &str| {
            if is_as1 {
                s.eq_ignore_ascii_case(kw)
            } else {
                s == kw
            }
        };
        if parts.len() == 1
            && parts[0]
                .strip_prefix("_level")
                .or_else(|| {
                    is_as1
                        .then(|| {
                            let low = parts[0].to_ascii_lowercase();
                            low.starts_with("_level").then_some(&parts[0][6..])
                        })
                        .flatten()
                })
                .is_some_and(|rest| rest != "0" && rest.chars().all(|c| c.is_ascii_digit()))
        {
            return PreviewValue::Undefined;
        }
        let mut cur_mc: Option<Vec<Id>> = Some(scope_path.to_vec());
        let mut cur_val: Option<PreviewValue> = None;

        for (idx, seg) in parts.iter().enumerate() {
            let is_last = idx + 1 == parts.len();
            if let Some(val) = cur_val.take() {
                let next_val = match val {
                    PreviewValue::Array(items) => {
                        if match_kw(seg, "length") {
                            PreviewValue::Number(items.len() as f64)
                        } else if let Ok(i) = seg.parse::<usize>() {
                            items.get(i).cloned().unwrap_or(PreviewValue::Undefined)
                        } else {
                            return PreviewValue::Undefined;
                        }
                    }
                    PreviewValue::Object(entries) => entries
                        .into_iter()
                        .find(|(k, _)| k == seg || (is_as1 && k.eq_ignore_ascii_case(seg)))
                        .map(|(_, v)| v)
                        .unwrap_or(PreviewValue::Undefined),
                    _ => return PreviewValue::Undefined,
                };
                if is_last {
                    return next_val;
                }
                cur_val = Some(next_val);
                continue;
            }

            let Some(mut mc_path) = cur_mc.take() else {
                return PreviewValue::Undefined;
            };
            if idx == 0 && (match_kw(seg, "_root") || match_kw(seg, "_level0")) {
                mc_path.clear();
                if is_last {
                    return PreviewValue::String("root".to_owned());
                }
                cur_mc = Some(mc_path);
                continue;
            }
            if match_kw(seg, "this") {
                if is_last {
                    return self.eval_mc_builtin_prop(project, &mc_path, "_name");
                }
                cur_mc = Some(mc_path);
                continue;
            }
            if match_kw(seg, "_parent") {
                if mc_path.is_empty() {
                    return PreviewValue::Undefined;
                }
                mc_path.pop();
                if is_last {
                    return self.eval_mc_builtin_prop(project, &mc_path, "_name");
                }
                cur_mc = Some(mc_path);
                continue;
            }
            if let Some(var_key) = self.find_var_key_in_scope(&mc_path, seg, is_as1)
                && let Some(v) = self.vars.get(&(mc_path.clone(), var_key)).cloned()
            {
                if is_last {
                    return v;
                }
                cur_val = Some(v);
                continue;
            }
            if let Some(child_id) =
                self.find_named_child_id_matching(project, &mc_path, seg, is_as1)
            {
                mc_path.push(child_id);
                if is_last {
                    return self.eval_mc_builtin_prop(project, &mc_path, "_name");
                }
                cur_mc = Some(mc_path);
                continue;
            }
            if is_last {
                let canon = if is_as1 {
                    seg.to_ascii_lowercase()
                } else {
                    seg.clone()
                };
                return self.eval_mc_builtin_prop(project, &mc_path, &canon);
            }
            return PreviewValue::Undefined;
        }
        PreviewValue::Undefined
    }

    pub(super) fn eval_mc_builtin_prop(
        &self,
        project: &Project,
        target_path: &[Id],
        prop: &str,
    ) -> PreviewValue {
        match prop {
            "_currentframe" | "currentFrame" => {
                let f = self
                    .player
                    .clip_frame(target_path)
                    .unwrap_or_else(|| self.player.frame());
                PreviewValue::Number(f64::from(f + 1))
            }
            "_totalframes" | "totalFrames" => {
                let sym_id = if target_path.is_empty() {
                    Some(self.symbol.as_str())
                } else {
                    self.player
                        .previous
                        .get(target_path)
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
                    .get(target_path)
                    .and_then(|o| o.visible)
                    .unwrap_or(true);
                PreviewValue::Bool(vis)
            }
            "x" | "_x" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "x")
                    .unwrap_or(0.0),
            ),
            "y" | "_y" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "y")
                    .unwrap_or(0.0),
            ),
            "rotation" | "_rotation" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "rotation")
                    .unwrap_or(0.0),
            ),
            "scaleX" | "scale_x" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "scale_x")
                    .unwrap_or(1.0),
            ),
            "_xscale" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "scale_x")
                    .map_or(100.0, |v| v * 100.0),
            ),
            "scaleY" | "scale_y" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "scale_y")
                    .unwrap_or(1.0),
            ),
            "_yscale" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "scale_y")
                    .map_or(100.0, |v| v * 100.0),
            ),
            "alpha" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "alpha")
                    .unwrap_or(1.0),
            ),
            "_alpha" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "alpha")
                    .map_or(100.0, |a| a * 100.0),
            ),
            _ => PreviewValue::Undefined,
        }
    }

    pub(super) fn apply_eval_path_write(
        &mut self,
        project: &Project,
        scope_path: &[Id],
        raw_path: &str,
        rhs_val: PreviewValue,
    ) -> bool {
        let Ok(parts) = crate::as2_object::reference_path(raw_path) else {
            return false;
        };
        if parts.is_empty() {
            return false;
        }
        let is_as1 = self.is_as1();
        let lang = self.current_language;
        let match_kw = |s: &str, kw: &str| {
            if is_as1 {
                s.eq_ignore_ascii_case(kw)
            } else {
                s == kw
            }
        };
        let mut mc_path = scope_path.to_vec();
        for (idx, seg) in parts[..parts.len() - 1].iter().enumerate() {
            if idx == 0 && (match_kw(seg, "_root") || match_kw(seg, "_level0")) {
                mc_path.clear();
                continue;
            }
            if match_kw(seg, "this") {
                continue;
            }
            if match_kw(seg, "_parent") {
                if mc_path.is_empty() {
                    return false;
                }
                mc_path.pop();
                continue;
            }
            if let Some(var_key) = self.find_var_key_in_scope(&mc_path, seg, is_as1)
                && matches!(
                    self.vars.get(&(mc_path.clone(), var_key.clone())),
                    Some(PreviewValue::Array(_) | PreviewValue::Object(_))
                )
            {
                let steps: Vec<ConcreteAccessStep> = parts[idx + 1..]
                    .iter()
                    .map(|p| {
                        if let Ok(i) = p.parse::<usize>()
                            && i <= 10_000
                        {
                            ConcreteAccessStep::Index(i)
                        } else {
                            ConcreteAccessStep::Prop(p.clone())
                        }
                    })
                    .collect();
                let Some(slot) = self.vars.get_mut(&(mc_path, var_key)) else {
                    return false;
                };
                return mutate_preview_container(slot, &steps, "=", &rhs_val, lang);
            }
            let Some(child_id) = self.find_named_child_id_matching(project, &mc_path, seg, is_as1)
            else {
                return false;
            };
            mc_path.push(child_id);
        }
        let last = &parts[parts.len() - 1];
        let canon = if is_as1 {
            last.to_ascii_lowercase()
        } else {
            last.clone()
        };
        if matches!(canon.as_str(), "_visible" | "visible") {
            self.player.set_visible(&mc_path, rhs_val.is_truthy(is_as1));
            return true;
        }
        let built_in = match canon.as_str() {
            "x" | "_x" => Some(("x", 1.0)),
            "y" | "_y" => Some(("y", 1.0)),
            "rotation" | "_rotation" => Some(("rotation", 1.0)),
            "scaleX" | "scale_x" => Some(("scale_x", 1.0)),
            "_xscale" => Some(("scale_x", 0.01)),
            "scaleY" | "scale_y" => Some(("scale_y", 1.0)),
            "_yscale" => Some(("scale_y", 0.01)),
            "alpha" => Some(("alpha", 1.0)),
            "_alpha" => Some(("alpha", 0.01)),
            _ => None,
        };
        if let Some((prop_name, scale_factor)) = built_in {
            let mut next = rhs_val.to_compound_lhs_number(is_as1) * scale_factor;
            if prop_name == "alpha" {
                next = next.clamp(0.0, 1.0);
            }
            self.player.set_property(&mc_path, prop_name, next);
            return true;
        }
        let key = self
            .find_var_key_in_scope(&mc_path, last, is_as1)
            .unwrap_or_else(|| last.clone());
        self.vars.insert((mc_path, key), rhs_val);
        true
    }

    pub(super) fn try_apply_delete_target(
        &mut self,
        project: &Project,
        path: &[Id],
        target_expr: &str,
    ) -> Option<bool> {
        let scope_path = self.effective_scope_path(project, path);
        let is_as1 = self.is_as1();
        let is_avm1 = matches!(
            self.current_language,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        );
        if let Some((recv, prop)) = split_receiver_member(target_expr)
            && let Some(ev) = callback_prop_to_event(prop)
            && let Some(target_path) = self.resolve_receiver_path(project, &scope_path, recv)
        {
            self.clear_registered_handler(&target_path, ev);
            return Some(true);
        }
        if let Some((base, tokens)) = parse_postfix_steps(target_expr.trim())
            && !tokens.is_empty()
            && let Some((owner_path, root_var, start_idx)) =
                self.resolve_mutable_container_root(project, &scope_path, base, &tokens, false)
            && let Some(steps) =
                self.concrete_access_steps(project, &scope_path, &tokens[start_idx..], is_as1)
            && !steps.is_empty()
            && let Some(root_slot) = self.vars.get_mut(&(owner_path, root_var))
        {
            return delete_preview_container(root_slot, &steps, is_avm1);
        }
        let (recv_opt, prop_raw) = match split_receiver_member(target_expr) {
            Some((r, p)) => (Some(r), p.trim()),
            None => (None, target_expr.trim()),
        };
        if !is_valid_identifier(prop_raw) {
            return None;
        }
        let target_path =
            self.resolve_receiver_path(project, &scope_path, recv_opt.unwrap_or("this"))?;
        if let Some(key) = self.find_var_key_in_scope(&target_path, prop_raw, is_as1) {
            self.vars.remove(&(target_path, key));
            return Some(true);
        }
        Some(!is_avm1)
    }

    pub(super) fn try_apply_nested_container_mutation(
        &mut self,
        project: &Project,
        scope_path: &[Id],
        lhs: &str,
        op: &str,
        rhs: &str,
        lang: ScriptLanguage,
    ) -> bool {
        if !is_supported_preview_rhs_for(rhs, lang) {
            return false;
        }
        let is_as1 = lang == ScriptLanguage::ActionScript1;
        let Some((base, tokens)) = parse_postfix_steps(lhs.trim()) else {
            return false;
        };
        if tokens.is_empty() {
            return false;
        }
        let Some((owner_path, root_var, start_idx)) =
            self.resolve_mutable_container_root(project, scope_path, base, &tokens, false)
        else {
            return false;
        };
        let Some(steps) =
            self.concrete_access_steps(project, scope_path, &tokens[start_idx..], is_as1)
        else {
            return false;
        };
        if steps.is_empty() {
            return false;
        }
        let rhs_val = self.eval_preview_value(project, scope_path, rhs);
        let Some(root_slot) = self.vars.get_mut(&(owner_path, root_var)) else {
            return false;
        };
        mutate_preview_container(root_slot, &steps, op, &rhs_val, lang)
    }

    pub(super) fn try_apply_container_method_call(
        &mut self,
        project: &Project,
        path: &[Id],
        stmt: &str,
    ) -> bool {
        let Some((target_expr, method, args_str)) = parse_postfix_method_call(stmt.trim()) else {
            return false;
        };
        if !matches!(method, "push" | "pop" | "shift" | "unshift" | "reverse") {
            return false;
        }
        let scope_path = self.effective_scope_path(project, path);
        let (base, tokens) = match parse_postfix_steps(target_expr) {
            Some(parsed) => parsed,
            None if is_valid_identifier(target_expr) => (target_expr, Vec::new()),
            None => return false,
        };
        let Some((owner_path, root_var, start_idx)) =
            self.resolve_mutable_container_root(project, &scope_path, base, &tokens, true)
        else {
            return false;
        };
        let is_as1 = self.is_as1();
        let Some(steps) =
            self.concrete_access_steps(project, &scope_path, &tokens[start_idx..], is_as1)
        else {
            return false;
        };
        let arg_vals: Vec<PreviewValue> = split_top_level_comma(args_str)
            .into_iter()
            .map(|a| self.eval_preview_value(project, &scope_path, a))
            .collect();
        if matches!(method, "pop" | "shift" | "reverse") && !arg_vals.is_empty() {
            return false;
        }
        let Some(root_slot) = self.vars.get_mut(&(owner_path, root_var)) else {
            return false;
        };
        let Some(PreviewValue::Array(items)) = navigate_preview_container_mut(root_slot, &steps)
        else {
            return false;
        };
        match method {
            "push" => {
                for v in arg_vals {
                    if items.len() < 10_000 {
                        items.push(v);
                    }
                }
                true
            }
            "pop" => {
                items.pop();
                true
            }
            "shift" => {
                if !items.is_empty() {
                    items.remove(0);
                }
                true
            }
            "unshift" => {
                for (i, v) in arg_vals.into_iter().enumerate() {
                    if items.len() < 10_000 {
                        items.insert(i, v);
                    }
                }
                true
            }
            "reverse" => {
                items.reverse();
                true
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum PostfixToken<'a> {
    Index(&'a str),
    Prop(&'a str),
    Call(&'a str, &'a str),
}

#[derive(Clone, Debug)]
pub(super) enum ConcreteAccessStep {
    Index(usize),
    Prop(String),
}

pub(super) fn navigate_preview_container_mut<'a>(
    mut cur: &'a mut PreviewValue,
    steps: &[ConcreteAccessStep],
) -> Option<&'a mut PreviewValue> {
    for step in steps {
        cur = match (cur, step) {
            (PreviewValue::Array(items), ConcreteAccessStep::Index(idx)) => items.get_mut(*idx)?,
            (PreviewValue::Object(entries), ConcreteAccessStep::Prop(key)) => {
                &mut entries.iter_mut().find(|(k, _)| k == key)?.1
            }
            _ => return None,
        };
    }
    Some(cur)
}

pub(super) fn delete_preview_container(
    cur: &mut PreviewValue,
    steps: &[ConcreteAccessStep],
    is_avm1: bool,
) -> Option<bool> {
    let (first, rest) = steps.split_first()?;
    if rest.is_empty() {
        return match (cur, first) {
            (PreviewValue::Array(items), ConcreteAccessStep::Index(idx)) => {
                if *idx >= items.len() {
                    Some(!is_avm1)
                } else if is_avm1 && matches!(items[*idx], PreviewValue::Undefined) {
                    Some(false)
                } else {
                    items[*idx] = PreviewValue::Undefined;
                    Some(true)
                }
            }
            (PreviewValue::Array(_), ConcreteAccessStep::Prop(key)) if key == "length" => {
                Some(false)
            }
            (PreviewValue::Object(entries), ConcreteAccessStep::Prop(key)) => {
                if let Some(pos) = entries.iter().position(|(k, _)| k == key) {
                    entries.remove(pos);
                    Some(true)
                } else {
                    Some(!is_avm1)
                }
            }
            (PreviewValue::Object(entries), ConcreteAccessStep::Index(idx)) => {
                let key = idx.to_string();
                if let Some(pos) = entries.iter().position(|(k, _)| k == &key) {
                    entries.remove(pos);
                    Some(true)
                } else {
                    Some(!is_avm1)
                }
            }
            _ => None,
        };
    }
    match (cur, first) {
        (PreviewValue::Array(items), ConcreteAccessStep::Index(idx)) => {
            delete_preview_container(items.get_mut(*idx)?, rest, is_avm1)
        }
        (PreviewValue::Object(entries), ConcreteAccessStep::Prop(key)) => {
            let child = &mut entries.iter_mut().find(|(k, _)| k == key)?.1;
            delete_preview_container(child, rest, is_avm1)
        }
        (PreviewValue::Object(entries), ConcreteAccessStep::Index(idx)) => {
            let key = idx.to_string();
            let child = &mut entries.iter_mut().find(|(k, _)| k == &key)?.1;
            delete_preview_container(child, rest, is_avm1)
        }
        _ => None,
    }
}

pub(super) fn mutate_preview_container(
    cur: &mut PreviewValue,
    steps: &[ConcreteAccessStep],
    op: &str,
    rhs_val: &PreviewValue,
    lang: ScriptLanguage,
) -> bool {
    let Some((first, rest)) = steps.split_first() else {
        return false;
    };
    if rest.is_empty() {
        return match (cur, first) {
            (PreviewValue::Array(items), ConcreteAccessStep::Index(idx)) => {
                if *idx > 10_000 {
                    return false;
                }
                if *idx >= items.len() {
                    items.resize(*idx + 1, PreviewValue::Undefined);
                }
                let Some(next) = apply_compound_var_op(op, &items[*idx], rhs_val, lang) else {
                    return false;
                };
                items[*idx] = next;
                true
            }
            (PreviewValue::Array(items), ConcreteAccessStep::Prop(key))
                if key == "length" && op == "=" =>
            {
                if let PreviewValue::Number(n) = rhs_val
                    && n.is_finite()
                    && *n >= 0.0
                    && (*n - n.trunc()).abs() < 1e-9
                    && (*n as usize) <= 10_000
                {
                    items.resize(*n as usize, PreviewValue::Undefined);
                    true
                } else {
                    false
                }
            }
            (PreviewValue::Object(entries), ConcreteAccessStep::Prop(key)) => {
                if let Some((_, slot)) = entries.iter_mut().find(|(k, _)| k == key) {
                    let Some(next) = apply_compound_var_op(op, slot, rhs_val, lang) else {
                        return false;
                    };
                    *slot = next;
                } else {
                    let Some(next) =
                        apply_compound_var_op(op, &PreviewValue::Undefined, rhs_val, lang)
                    else {
                        return false;
                    };
                    entries.push((key.clone(), next));
                }
                true
            }
            (PreviewValue::Object(entries), ConcreteAccessStep::Index(idx)) => {
                let key = idx.to_string();
                if let Some((_, slot)) = entries.iter_mut().find(|(k, _)| k == &key) {
                    let Some(next) = apply_compound_var_op(op, slot, rhs_val, lang) else {
                        return false;
                    };
                    *slot = next;
                } else {
                    let Some(next) =
                        apply_compound_var_op(op, &PreviewValue::Undefined, rhs_val, lang)
                    else {
                        return false;
                    };
                    entries.push((key, next));
                }
                true
            }
            _ => false,
        };
    }

    match (cur, first) {
        (PreviewValue::Array(items), ConcreteAccessStep::Index(idx)) => {
            let Some(child) = items.get_mut(*idx) else {
                return false;
            };
            mutate_preview_container(child, rest, op, rhs_val, lang)
        }
        (PreviewValue::Object(entries), ConcreteAccessStep::Prop(key)) => {
            let Some((_, child)) = entries.iter_mut().find(|(k, _)| k == key) else {
                return false;
            };
            mutate_preview_container(child, rest, op, rhs_val, lang)
        }
        (PreviewValue::Object(entries), ConcreteAccessStep::Index(idx)) => {
            let key = idx.to_string();
            let Some((_, child)) = entries.iter_mut().find(|(k, _)| k == &key) else {
                return false;
            };
            mutate_preview_container(child, rest, op, rhs_val, lang)
        }
        _ => false,
    }
}

pub(super) fn split_top_level_colon(entry: &str) -> Option<(&str, &str)> {
    let parts = split_top_level_op(entry, ":");
    if parts.len() >= 2 {
        let colon_pos = entry.find(':')?;
        let _ = colon_pos;
        // Rejoin rhs in case value contains ternary `:` by splitting only at the first top-level `:`
        let bytes = entry.as_bytes();
        let mut i = 0;
        let mut in_s = false;
        let mut in_d = false;
        let mut paren = 0usize;
        let mut brace = 0usize;
        let mut bracket = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            if in_s {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == b'\'' {
                    in_s = false;
                }
                i += 1;
                continue;
            }
            if in_d {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == b'"' {
                    in_d = false;
                }
                i += 1;
                continue;
            }
            match b {
                b'\'' => in_s = true,
                b'"' => in_d = true,
                b'(' => paren += 1,
                b')' => paren = paren.saturating_sub(1),
                b'{' => brace += 1,
                b'}' => brace = brace.saturating_sub(1),
                b'[' => bracket += 1,
                b']' => bracket = bracket.saturating_sub(1),
                b':' if paren == 0 && brace == 0 && bracket == 0 => {
                    return Some((entry[..i].trim(), entry[i + 1..].trim()));
                }
                _ => {}
            }
            i += 1;
        }
    }
    None
}

pub(super) fn parse_object_literal_key(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
        || (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2)
    {
        return Some(unescape_string_literal(&trimmed[1..trimmed.len() - 1]));
    }
    if is_valid_identifier(trimmed) || trimmed.parse::<i64>().is_ok() {
        return Some(trimmed.to_owned());
    }
    None
}

pub(super) fn parse_postfix_steps(expr: &str) -> Option<(&str, Vec<PostfixToken<'_>>)> {
    let s = expr.trim();
    if s.is_empty() {
        return None;
    }
    let bytes = s.as_bytes();
    let base_end = match bytes[0] {
        b'(' => find_matching_delim(s, b'(', b')')? + 1,
        b'[' => find_matching_delim(s, b'[', b']')? + 1,
        b'{' => find_matching_delim(s, b'{', b'}')? + 1,
        b'\'' | b'"' => {
            let quote = bytes[0];
            let mut i = 1usize;
            let mut end = None;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    end = Some(i + 1);
                    break;
                }
                i += 1;
            }
            end?
        }
        b if b.is_ascii_alphabetic() || b == b'_' || b == b'$' => {
            let mut i = 1usize;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
            {
                i += 1;
            }
            if &s[..i] == "new" && bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                let mut k = i;
                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                let ctor_start = k;
                while k < bytes.len()
                    && (bytes[k].is_ascii_alphanumeric() || bytes[k] == b'_' || bytes[k] == b'$')
                {
                    k += 1;
                }
                let ctor_name = &s[ctor_start..k];
                if !matches!(ctor_name, "Array" | "Object") {
                    return None;
                }
                let mut m = k;
                while m < bytes.len() && bytes[m].is_ascii_whitespace() {
                    m += 1;
                }
                if m < bytes.len() && bytes[m] == b'(' {
                    let close = find_matching_delim(&s[m..], b'(', b')')?;
                    m + close + 1
                } else {
                    k
                }
            } else {
                let mut m = i;
                while m < bytes.len() && bytes[m].is_ascii_whitespace() {
                    m += 1;
                }
                if m < bytes.len()
                    && bytes[m] == b'('
                    && (&s[..i] == "Array" || is_supported_builtin_pure_call_head(&s[..i]))
                {
                    let close = find_matching_delim(&s[m..], b'(', b')')?;
                    m + close + 1
                } else {
                    i
                }
            }
        }
        _ => return None,
    };

    let base = s[..base_end].trim();
    let mut rest = s[base_end..].trim_start();
    let mut tokens = Vec::new();
    while !rest.is_empty() {
        if rest.starts_with('[') {
            let close = find_matching_delim(rest, b'[', b']')?;
            tokens.push(PostfixToken::Index(rest[1..close].trim()));
            rest = rest[close + 1..].trim_start();
        } else {
            let after_dot = rest.strip_prefix('.')?.trim_start();
            let ab = after_dot.as_bytes();
            if ab.is_empty() || !(ab[0].is_ascii_alphabetic() || ab[0] == b'_' || ab[0] == b'$') {
                return None;
            }
            let mut i = 1usize;
            while i < ab.len() && (ab[i].is_ascii_alphanumeric() || ab[i] == b'_' || ab[i] == b'$')
            {
                i += 1;
            }
            let prop = &after_dot[..i];
            let tail = after_dot[i..].trim_start();
            if tail.starts_with('(') {
                let close = find_matching_delim(tail, b'(', b')')?;
                tokens.push(PostfixToken::Call(prop, tail[1..close].trim()));
                rest = tail[close + 1..].trim_start();
            } else {
                tokens.push(PostfixToken::Prop(prop));
                rest = tail;
            }
        }
    }
    Some((base, tokens))
}

pub(super) fn parse_postfix_method_call(stmt: &str) -> Option<(&str, &str, &str)> {
    let s = stmt.trim();
    if !s.ends_with(')') {
        return None;
    }
    let bytes = s.as_bytes();
    let mut in_s = false;
    let mut in_d = false;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let mut call_open = None;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if in_s {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_s = false;
            }
            i += 1;
            continue;
        }
        if in_d {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_d = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' => in_s = true,
            b'"' => in_d = true,
            b'(' => {
                if paren == 0 && brace == 0 && bracket == 0 {
                    call_open = Some(i);
                }
                paren += 1;
            }
            b')' => paren = paren.saturating_sub(1),
            b'{' => brace += 1,
            b'}' => brace = brace.saturating_sub(1),
            b'[' => bracket += 1,
            b']' => bracket = bracket.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    let open = call_open?;
    if find_matching_delim(&s[open..], b'(', b')') != Some(s.len() - 1 - open) {
        return None;
    }
    let callee = s[..open].trim();
    let cb = callee.as_bytes();
    let mut last_dot = None;
    let mut in_s = false;
    let mut in_d = false;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let mut j = 0usize;
    while j < cb.len() {
        let b = cb[j];
        if in_s {
            if b == b'\\' {
                j += 2;
                continue;
            }
            if b == b'\'' {
                in_s = false;
            }
            j += 1;
            continue;
        }
        if in_d {
            if b == b'\\' {
                j += 2;
                continue;
            }
            if b == b'"' {
                in_d = false;
            }
            j += 1;
            continue;
        }
        match b {
            b'\'' => in_s = true,
            b'"' => in_d = true,
            b'(' => paren += 1,
            b')' => paren = paren.saturating_sub(1),
            b'{' => brace += 1,
            b'}' => brace = brace.saturating_sub(1),
            b'[' => bracket += 1,
            b']' => bracket = bracket.saturating_sub(1),
            b'.' if paren == 0 && brace == 0 && bracket == 0 => last_dot = Some(j),
            _ => {}
        }
        j += 1;
    }
    let dot = last_dot?;
    let target = callee[..dot].trim();
    let method = callee[dot + 1..].trim();
    if target.is_empty() || !is_valid_identifier(method) {
        return None;
    }
    Some((target, method, s[open + 1..s.len() - 1].trim()))
}

pub(super) fn eval_add_values(
    lhs: PreviewValue,
    rhs: PreviewValue,
    lang: ScriptLanguage,
) -> PreviewValue {
    let is_as1 = lang == ScriptLanguage::ActionScript1;
    if matches!(lhs, PreviewValue::String(_) | PreviewValue::Array(_))
        || matches!(rhs, PreviewValue::String(_) | PreviewValue::Array(_))
    {
        let mut s = lhs.to_es_string_for(lang);
        s.push_str(&rhs.to_es_string_for(lang));
        PreviewValue::String(s)
    } else {
        PreviewValue::Number(lhs.to_number(is_as1) + rhs.to_number(is_as1))
    }
}

pub(super) fn callback_prop_to_event(prop: &str) -> Option<RegisteredEvent> {
    match prop.trim() {
        "onRelease" | "onPress" | "onclick" => Some(RegisteredEvent::Click),
        "onEnterFrame" => Some(RegisteredEvent::EnterFrame),
        _ => None,
    }
}
