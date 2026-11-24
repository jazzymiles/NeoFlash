//! AVM2 `callproperty` dispatch, builtin methods, `+` coercion, and `Graphics` operations.

use super::super::*;

impl SwfBytecodePlayer {
    pub(in crate::swf::player) fn apply_avm2_graphics_op(
        gfx: &mut RuntimeGraphicsState,
        prop: &str,
        args: &[Avm1Value],
    ) -> bool {
        let a0 = args.first().map_or(0.0, avm2_to_number);
        let a1 = args.get(1).map_or(0.0, avm2_to_number);
        let a2 = args.get(2).map_or(0.0, avm2_to_number);
        let a3 = args.get(3).map_or(0.0, avm2_to_number);
        match prop {
            "beginFill" => {
                let rgb = ecma_f64_to_u32(a0) & 0x00FF_FFFF;
                let alpha = args.get(1).map_or(1.0, avm2_to_number);
                gfx.begin_fill(rgb, alpha);
                true
            }
            "endFill" => {
                gfx.end_fill();
                true
            }
            "lineStyle" => {
                let th_opt = match args.first() {
                    None | Some(Avm1Value::Undefined | Avm1Value::Null) => None,
                    Some(v) => {
                        let n = avm2_to_number(v);
                        (!n.is_nan()).then_some(n)
                    }
                };
                let rgb = args
                    .get(1)
                    .map_or(0, |v| ecma_f64_to_u32(avm2_to_number(v)) & 0x00FF_FFFF);
                let alpha = args.get(2).map_or(1.0, avm2_to_number);
                gfx.line_style(th_opt, rgb, alpha);
                true
            }
            "moveTo" => {
                gfx.move_to(a0, a1);
                true
            }
            "lineTo" => {
                gfx.line_to(a0, a1);
                true
            }
            "curveTo" => {
                gfx.curve_to(a0, a1, a2, a3);
                true
            }
            "drawRect" | "drawRoundRect" => {
                gfx.draw_rect(a0, a1, a2, a3);
                true
            }
            "drawCircle" => {
                let r = a2.abs();
                gfx.draw_ellipse(a0 - r, a1 - r, r * 2.0, r * 2.0);
                true
            }
            "drawEllipse" => {
                gfx.draw_ellipse(a0, a1, a2, a3);
                true
            }
            "clear" => {
                gfx.clear();
                true
            }
            _ => false,
        }
    }

    /// AVM2 Add converts objects before choosing numeric addition or concatenation.
    pub(in crate::swf::player) fn avm2_add_operand(
        &mut self,
        value: Avm1Value,
        abc: usize,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if !matches!(value, Avm1Value::Object(_)) {
            return match &value {
                Avm1Value::MovieClip(_)
                | Avm1Value::MovieClipIdentity(_)
                | Avm1Value::Super { .. } => {
                    Avm1Value::String(self.avm2_coerce_to_string_eval(&value, abc, steps, depth))
                }
                Avm1Value::String(s) if is_avm2_fn_token(s) => {
                    Avm1Value::String(avm2_convert_string(&value, &self.avm1_objects))
                }
                _ => value,
            };
        }
        for name in ["valueOf", "toString"] {
            if !self.avm2_has_property(&value, name) {
                if name == "toString" {
                    return Avm1Value::String(avm2_convert_string(&value, &self.avm1_objects));
                }
                continue;
            }
            let method = self.get_avm2_property_eval(&value, name, steps, depth);
            if self.avm2_thrown.is_some() {
                return Avm1Value::Undefined;
            }
            if matches!(&method, Avm1Value::String(s) if is_avm2_fn_token(s)) {
                let v = self.invoke_avm2_callable_with_this(
                    &method,
                    value.clone(),
                    Vec::new(),
                    steps,
                    depth,
                );
                if self.avm2_thrown.is_some()
                    || matches!(
                        &v,
                        Avm1Value::Number(_)
                            | Avm1Value::Null
                            | Avm1Value::Undefined
                            | Avm1Value::Bool(_)
                    )
                    || matches!(&v, Avm1Value::String(s) if !is_avm2_fn_token(s))
                {
                    return v;
                }
            }
        }
        let error = self.construct_avm2_named(
            abc,
            "",
            "TypeError",
            vec![Avm1Value::String(
                "Cannot convert object to primitive value".to_owned(),
            )],
            steps,
            depth,
        );
        self.avm2_thrown = Some(error);
        Avm1Value::Undefined
    }

    pub(in crate::swf::player) fn call_avm2_property(
        &mut self,
        abc_idx: usize,
        receiver: &Avm1Value,
        prop: &str,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::String(tok) = receiver
            && is_avm2_fn_token(tok)
        {
            match prop {
                "call" | "apply" => {
                    let this_arg = args.first().cloned().unwrap_or(Avm1Value::Null);
                    let call_args = if prop == "call" {
                        args.into_iter().skip(1).collect()
                    } else if let Some(Avm1Value::Object(arr_id)) = args.get(1) {
                        let len = self
                            .avm1_objects
                            .get(*arr_id)
                            .and_then(|o| o.array_elements.as_ref().map(Vec::len))
                            .unwrap_or(0);
                        (0..len)
                            .map(|i| {
                                self.get_avm1_object_prop_chain(*arr_id, &i.to_string())
                                    .unwrap_or(Avm1Value::Undefined)
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    return self.invoke_avm2_callable_with_this(
                        receiver, this_arg, call_args, steps, depth,
                    );
                }
                "toString" | "toLocaleString" => {
                    return Avm1Value::String("function Function() {}".to_owned());
                }
                "valueOf" => {
                    return receiver.clone();
                }
                _ => {}
            }
        }

        if let Avm1Value::MovieClip(mc) = receiver {
            if mc == "__avm2_type:XML" || mc == "XML" {
                match prop {
                    "settings" | "defaultSettings" => {
                        let s = if prop == "defaultSettings" {
                            E4xSettings::default()
                        } else {
                            self.e4x_arena.borrow().settings.clone()
                        };
                        let mut props = HashMap::new();
                        props.insert(
                            "ignoreComments".to_owned(),
                            Avm1Value::Bool(s.ignore_comments),
                        );
                        props.insert(
                            "ignoreProcessingInstructions".to_owned(),
                            Avm1Value::Bool(s.ignore_processing_instructions),
                        );
                        props.insert(
                            "ignoreWhitespace".to_owned(),
                            Avm1Value::Bool(s.ignore_whitespace),
                        );
                        props.insert(
                            "prettyPrinting".to_owned(),
                            Avm1Value::Bool(s.pretty_printing),
                        );
                        props.insert(
                            "prettyIndent".to_owned(),
                            Avm1Value::Number(s.pretty_indent as f64),
                        );
                        let oid = self.avm1_objects.len();
                        self.avm1_objects.push(Avm1Object {
                            props,
                            ..Default::default()
                        });
                        return Avm1Value::Object(oid);
                    }
                    "setSettings" => {
                        if let Some(Avm1Value::Object(soid)) = args.first() {
                            if let Some(so) = self.avm1_objects.get(*soid).cloned() {
                                let mut arena = self.e4x_arena.borrow_mut();
                                if let Some(v) = so.props.get("ignoreComments") {
                                    arena.settings.ignore_comments = v.to_bool(9);
                                }
                                if let Some(v) = so.props.get("ignoreProcessingInstructions") {
                                    arena.settings.ignore_processing_instructions = v.to_bool(9);
                                }
                                if let Some(v) = so.props.get("ignoreWhitespace") {
                                    arena.settings.ignore_whitespace = v.to_bool(9);
                                }
                                if let Some(v) = so.props.get("prettyPrinting") {
                                    arena.settings.pretty_printing = v.to_bool(9);
                                }
                                if let Some(v) = so.props.get("prettyIndent") {
                                    arena.settings.pretty_indent =
                                        ecma_f64_to_i32(avm2_to_number(v)).clamp(0, 64) as usize;
                                }
                            }
                        } else {
                            self.e4x_arena.borrow_mut().settings = E4xSettings::default();
                        }
                        return Avm1Value::Undefined;
                    }
                    _ => {}
                }
            }
            if mc == "__avm2_type:ApplicationDomain" {
                match prop {
                    "hasDefinition" => {
                        let Some(first) = args.first() else {
                            self.throw_avm2_builtin_error(
                                "TypeError",
                                2007,
                                "Parameter name must be non-null.",
                            );
                            return Avm1Value::Bool(false);
                        };
                        if matches!(first, Avm1Value::Null | Avm1Value::Undefined) {
                            self.throw_avm2_builtin_error(
                                "TypeError",
                                2007,
                                "Parameter name must be non-null.",
                            );
                            return Avm1Value::Bool(false);
                        }
                        let q = avm2_convert_string(first, &self.avm1_objects);
                        return Avm1Value::Bool(
                            self.resolve_avm2_definition_by_name(abc_idx, &q).is_some(),
                        );
                    }
                    "getDefinition" => {
                        let q = args.first().map_or_else(String::new, |v| {
                            avm2_convert_string(v, &self.avm1_objects)
                        });
                        if let Some(def) = self.resolve_avm2_definition_by_name(abc_idx, &q) {
                            return def;
                        }
                        let short_q = q
                            .rsplit_once("::")
                            .or_else(|| q.rsplit_once('.'))
                            .map_or(q.as_str(), |(_, s)| s);
                        self.throw_avm2_builtin_error(
                            "ReferenceError",
                            1065,
                            &format!("Variable {short_q} is not defined."),
                        );
                        return Avm1Value::Undefined;
                    }
                    _ => {}
                }
            }
        }

        if let Avm1Value::Object(oid) = receiver {
            if self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_catch_var.is_some())
            {
                self.throw_avm2_builtin_error(
                    "ReferenceError",
                    1069,
                    &format!(
                        "Property {prop} not found on catch scope and there is no default value."
                    ),
                );
                return Avm1Value::Undefined;
            }
            if self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_vector.is_some())
                && let Some(ret) =
                    self.call_avm2_vector_method(*oid, prop, &args, abc_idx, steps, depth)
            {
                return ret;
            }
            if self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_xml.is_some())
                && let Some(ret) =
                    self.call_avm2_xml_method(*oid, prop, &args, abc_idx, steps, depth)
            {
                return ret;
            }
            if self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_dictionary.is_some())
            {
                match prop {
                    "hasOwnProperty" => {
                        let s = if let Some(first) = args.first() {
                            self.avm2_coerce_to_string_eval(first, abc_idx, steps, depth)
                        } else {
                            String::new()
                        };
                        if self.avm2_thrown.is_some() {
                            return Avm1Value::Undefined;
                        }
                        let has = self
                            .avm1_objects
                            .get(*oid)
                            .and_then(|o| o.avm2_dictionary.as_ref())
                            .is_some_and(|d| d.contains(&Avm2DictKey::String(s)));
                        return Avm1Value::Bool(has);
                    }
                    "propertyIsEnumerable" => {
                        let has = if let Some(first) = args.first() {
                            let (dk, _) =
                                Avm2DictionaryData::key_from_value(first, &self.avm1_objects);
                            self.avm1_objects
                                .get(*oid)
                                .and_then(|o| o.avm2_dictionary.as_ref())
                                .is_some_and(|d| d.contains(&dk))
                        } else {
                            false
                        };
                        return Avm1Value::Bool(has);
                    }
                    "setPropertyIsEnumerable" => return Avm1Value::Undefined,
                    "toString" => return Avm1Value::String("[object Dictionary]".to_owned()),
                    _ => {}
                }
            }
            if let Some(Avm1Value::String(sc)) = self
                .avm1_objects
                .get(*oid)
                .and_then(|o| o.props.get("__avm2_short_class__"))
                && (sc == "QName" || sc == "Namespace")
            {
                match prop {
                    "valueOf" => return receiver.clone(),
                    "toString" => {
                        if let Some(s) = self
                            .avm1_objects
                            .get(*oid)
                            .and_then(|o| o.props.get("qname_formatted"))
                            .cloned()
                        {
                            return s;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(owner_oid) = self
                .avm1_objects
                .get(*oid)
                .and_then(|o| o.avm2_graphics_owner)
            {
                let disp_target = self
                    .avm1_objects
                    .get(owner_oid)
                    .and_then(|o| o.props.get("__display_target__").cloned())
                    .and_then(|v| match v {
                        Avm1Value::MovieClipIdentity(t) => Some(t),
                        _ => None,
                    });
                if let Some(owner) = self.avm1_objects.get_mut(owner_oid) {
                    let gfx = owner
                        .graphics
                        .get_or_insert_with(RuntimeGraphicsState::default);
                    let _ = Self::apply_avm2_graphics_op(gfx, prop, &args);
                }
                if let Some(t) = disp_target {
                    if t == "_root" {
                        let _ = Self::apply_avm2_graphics_op(&mut self.root_graphics, prop, &args);
                    } else if let Some(slot) = self.target_slot_mut(&t) {
                        let _ = Self::apply_avm2_graphics_op(&mut slot.graphics, prop, &args);
                    }
                }
                return Avm1Value::Undefined;
            }
            if prop == "toString"
                && self
                    .avm1_objects
                    .get(*oid)
                    .is_some_and(|o| o.avm2_event.is_some())
            {
                return Avm1Value::String(
                    Avm1Value::Object(*oid).to_trace_string(9, &self.avm1_objects),
                );
            }
            if let Some((c_abc, c_idx)) = self.avm1_objects.get(*oid).and_then(|o| o.avm2_class) {
                let mut cur = Some((c_abc, c_idx));
                for _ in 0..32 {
                    let Some((a, c)) = cur else { break };
                    let method_opt = self
                        .abc_tags
                        .get(a)
                        .and_then(|l| l.abc.instances.get(c))
                        .and_then(|inst| {
                            inst.traits.iter().find_map(|tr| {
                                (self.abc_tags[a].abc.split_qname(tr.name).1 == prop)
                                    .then_some(&tr.data)
                                    .and_then(|td| match td {
                                        AbcTraitData::Method { method }
                                        | AbcTraitData::Function { method } => Some(*method),
                                        _ => None,
                                    })
                            })
                        });
                    if let Some(m_idx) = method_opt {
                        return self.run_avm2_method_with_this(
                            a,
                            m_idx,
                            Avm1Value::Object(*oid),
                            args,
                            steps,
                            depth + 1,
                        );
                    }
                    cur = self.avm2_superclass(a, c);
                }
            }
            if self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_display.is_some())
                && let Some(ret) =
                    self.call_avm2_display_builtin_method(*oid, prop, &args, steps, depth)
            {
                return ret;
            }
        }
        self.call_avm2_property_tail(abc_idx, receiver, prop, args, steps, depth)
    }

    #[inline(never)]
    fn call_avm2_property_tail(
        &mut self,
        abc_idx: usize,
        receiver: &Avm1Value,
        prop: &str,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::Object(oid) = receiver {
            if matches!(prop, "addChild" | "removeChild" | "getChildByName")
                && let Some(Avm1Value::MovieClipIdentity(disp_target)) = self
                    .avm1_objects
                    .get(*oid)
                    .and_then(|o| o.props.get("__display_target__").cloned())
            {
                match prop {
                    "addChild" => {
                        if let Some(child_arg) = args.first() {
                            return self.avm2_add_child_to_target(&disp_target, child_arg);
                        }
                        return Avm1Value::Undefined;
                    }
                    "removeChild" => {
                        if let Some(child_arg) = args.first() {
                            return self.avm2_remove_child_value(child_arg);
                        }
                        return Avm1Value::Undefined;
                    }
                    "getChildByName" => {
                        let want = args.first().map_or_else(String::new, |v| {
                            avm2_convert_string(v, &self.avm1_objects)
                        });
                        if !want.is_empty()
                            && let Some(pslot) = self.target_slot(&disp_target)
                            && let Some((d, slot)) =
                                pslot.children.iter().find(|(_, s)| s.name == want)
                        {
                            return Avm1Value::MovieClipIdentity(join_target_path(
                                &disp_target,
                                &live_slot_segment(*d, slot),
                            ));
                        }
                        return Avm1Value::Null;
                    }
                    _ => {}
                }
            }
            if let Some(Avm1Value::String(tok)) = self
                .avm1_objects
                .get(*oid)
                .and_then(|o| o.props.get(prop))
                .cloned()
                && is_avm2_fn_token(&tok)
            {
                return self.invoke_avm2_callable_with_this(
                    &Avm1Value::String(tok),
                    Avm1Value::Object(*oid),
                    args,
                    steps,
                    depth,
                );
            }
        }

        if let Avm1Value::MovieClip(mc) = receiver
            && (parse_avm2_class_token(mc).is_some()
                || mc.starts_with("__avm2_type:")
                || mc.starts_with("__avm2_vec_type:"))
            && matches!(prop, "toString" | "toLocaleString" | "valueOf")
            && !mc.contains(".prototype")
        {
            return match prop {
                "valueOf" => receiver.clone(),
                _ => {
                    let s = self.avm2_coerce_to_string_eval(receiver, abc_idx, steps, depth);
                    Avm1Value::String(s)
                }
            };
        }

        if let Avm1Value::MovieClip(mc) = receiver
            && let Some((c_abc, c_idx)) = parse_avm2_class_token(mc)
        {
            let mut cur = Some((c_abc, c_idx));
            for _ in 0..32 {
                let Some((a, c)) = cur else { break };
                if let Some(m_idx) = self
                    .abc_tags
                    .get(a)
                    .and_then(|l| l.abc.classes.get(c))
                    .and_then(|cls| {
                        cls.traits.iter().find_map(|tr| {
                            (self.abc_tags[a].abc.split_qname(tr.name).1 == prop)
                                .then_some(&tr.data)
                                .and_then(|td| match td {
                                    AbcTraitData::Method { method }
                                    | AbcTraitData::Function { method } => Some(*method),
                                    _ => None,
                                })
                        })
                    })
                {
                    return self.run_avm2_method_with_this(
                        a,
                        m_idx,
                        Avm1Value::MovieClip(mc.clone()),
                        args,
                        steps,
                        depth + 1,
                    );
                }
                cur = self.avm2_superclass(a, c);
            }
        }

        if let Avm1Value::MovieClip(mc) | Avm1Value::MovieClipIdentity(mc) = receiver {
            if let Some(gfx_target) = mc.strip_prefix("__avm2_graphics:") {
                if gfx_target == "_root" {
                    let _ = Self::apply_avm2_graphics_op(&mut self.root_graphics, prop, &args);
                } else if let Some(slot) = self.target_slot_mut(gfx_target) {
                    let _ = Self::apply_avm2_graphics_op(&mut slot.graphics, prop, &args);
                }
                return Avm1Value::Undefined;
            }
            let target = self.canonical_movie_target(mc);
            if target != "_root"
                && !target.is_empty()
                && target != "Math"
                && !mc.starts_with("__avm2_")
                && mc != prop
                && let Some(cid) = self.target_slot(&target).map(|s| s.character_id)
            {
                match prop {
                    "addChild" => {
                        if let Some(child_arg) = args.first() {
                            return self.avm2_add_child_to_target(&target, child_arg);
                        }
                        return Avm1Value::Undefined;
                    }
                    "removeChild" => {
                        if let Some(child_arg) = args.first() {
                            return self.avm2_remove_child_value(child_arg);
                        }
                        return Avm1Value::Undefined;
                    }
                    "getChildByName" => {
                        let want = args.first().map_or_else(String::new, |v| {
                            avm2_convert_string(v, &self.avm1_objects)
                        });
                        if !want.is_empty()
                            && let Some(pslot) = self.target_slot(&target)
                            && let Some((d, slot)) =
                                pslot.children.iter().find(|(_, s)| s.name == want)
                        {
                            return Avm1Value::MovieClipIdentity(join_target_path(
                                &target,
                                &live_slot_segment(*d, slot),
                            ));
                        }
                        return Avm1Value::Null;
                    }
                    "stop" => {
                        if let Some(slot) = self.target_slot_mut(&target) {
                            slot.playing = false;
                        }
                        return Avm1Value::Undefined;
                    }
                    "play" => {
                        if let Some(slot) = self.target_slot_mut(&target) {
                            slot.playing = true;
                        }
                        return Avm1Value::Undefined;
                    }
                    "nextFrame" => {
                        if let Some(slot) = self.target_slot_mut(&target) {
                            slot.current_frame =
                                (slot.current_frame + 1).min(slot.total_frames.saturating_sub(1));
                            slot.playing = false;
                        }
                        return Avm1Value::Undefined;
                    }
                    "prevFrame" => {
                        if let Some(slot) = self.target_slot_mut(&target) {
                            slot.current_frame = slot.current_frame.saturating_sub(1);
                            slot.playing = false;
                        }
                        return Avm1Value::Undefined;
                    }
                    "gotoAndStop" | "gotoAndPlay" => {
                        if let Some(first) = args.first() {
                            let resolved_label = match first {
                                Avm1Value::String(lbl) => self
                                    .sprites
                                    .get(&cid)
                                    .and_then(|def| def.resolve_avm2_label(lbl)),
                                _ => None,
                            };
                            if let Some(slot) = self.target_slot_mut(&target) {
                                let total = slot.total_frames.max(1);
                                let resolved = match first {
                                    Avm1Value::Number(n) if *n >= 1.0 => {
                                        let f = (*n as i64).saturating_sub(1).max(0) as u32;
                                        Some(f.min(total.saturating_sub(1)))
                                    }
                                    _ => resolved_label.map(|f| f.min(total.saturating_sub(1))),
                                };
                                if let Some(f) = resolved {
                                    slot.current_frame = f;
                                    slot.playing = prop == "gotoAndPlay";
                                }
                            }
                        }
                        return Avm1Value::Undefined;
                    }
                    _ => {}
                }
            }
        }

        if let Avm1Value::MovieClip(mc) = receiver {
            if (mc == "__avm2_type:Object.prototype.toString" || mc == "Object.prototype.toString")
                && prop == "call"
            {
                let this_arg = args.first().unwrap_or(&Avm1Value::Undefined);
                let full = self.avm2_runtime_class_label(this_arg);
                let short = full
                    .strip_prefix("__AS3__.vec::")
                    .unwrap_or_else(|| full.rsplit_once("::").map_or(full.as_str(), |(_, s)| s));
                return Avm1Value::String(format!("[object {short}]"));
            }
            if (mc == "__avm2_type:Object.prototype.valueOf" || mc == "Object.prototype.valueOf")
                && prop == "call"
            {
                return args.first().cloned().unwrap_or(Avm1Value::Undefined);
            }
            if mc == "__avm2_type:XML.prototype" && prop == "toString" {
                return Avm1Value::String(String::new());
            }
            if let Some(mname) = mc.strip_prefix("__avm2_type:XML.prototype.")
                && prop == "call"
                && let Some(Avm1Value::Object(oid)) = args.first()
                && let Some(ret) =
                    self.call_avm2_xml_method(*oid, mname, &args[1..], abc_idx, steps, depth)
            {
                return ret;
            }
        }

        let is_global_receiver = matches!(
            receiver,
            Avm1Value::MovieClip(mc) if mc == "_root" || mc == prop || mc.starts_with("__avm2_type:")
        );
        if is_global_receiver {
            match prop {
                "int" => {
                    let i = args
                        .first()
                        .map_or(0, |v| ecma_f64_to_i32(avm2_to_number(v)));
                    return Avm1Value::Number(f64::from(i));
                }
                "uint" => {
                    let u = args
                        .first()
                        .map_or(0, |v| ecma_f64_to_u32(avm2_to_number(v)));
                    return Avm1Value::Number(f64::from(u));
                }
                "Number" => {
                    let n = args.first().map_or(0.0, avm2_to_number);
                    return Avm1Value::Number(n);
                }
                "String" => {
                    let s = match args.first() {
                        None => String::new(),
                        Some(v) => self.avm2_coerce_to_string_eval(v, abc_idx, steps, depth),
                    };
                    return Avm1Value::String(s);
                }
                "Boolean" => {
                    let b = args.first().is_some_and(|v| v.to_bool(9));
                    return Avm1Value::Bool(b);
                }
                "Object" => {
                    return match args.first() {
                        None | Some(Avm1Value::Undefined | Avm1Value::Null) => {
                            let obj_id = self.avm1_objects.len();
                            self.avm1_objects.push(Avm1Object::default());
                            Avm1Value::Object(obj_id)
                        }
                        Some(v) => v.clone(),
                    };
                }
                "MovieClip"
                | "Sprite"
                | "Shape"
                | "SimpleButton"
                | "Stage"
                | "TextField"
                | "Bitmap"
                | "DisplayObject"
                | "DisplayObjectContainer"
                | "InteractiveObject"
                | "EventDispatcher"
                | "Event"
                | "Array"
                | "Function" => {
                    if args.len() == 1 {
                        let v = &args[0];
                        return if matches!(v, Avm1Value::Undefined | Avm1Value::Null) {
                            Avm1Value::Null
                        } else if self.avm2_is_type_name(abc_idx, v, "", prop) {
                            v.clone()
                        } else {
                            Avm1Value::Null
                        };
                    }
                }
                "Error" | "TypeError" | "RangeError" | "ReferenceError" | "ArgumentError"
                | "VerifyError" | "SyntaxError" | "URIError" | "EvalError" | "SecurityError" => {
                    return self.construct_avm2_named(abc_idx, "", prop, args, steps, depth);
                }
                "isNaN" => {
                    let n = args.first().map_or(f64::NAN, avm2_to_number);
                    return Avm1Value::Bool(n.is_nan());
                }
                "isFinite" => {
                    let n = args.first().map_or(f64::NAN, avm2_to_number);
                    return Avm1Value::Bool(n.is_finite());
                }
                "parseInt" => {
                    let s = match args.first() {
                        None => "NaN".to_owned(),
                        Some(Avm1Value::Undefined | Avm1Value::Null) => "null".to_owned(),
                        Some(first) => {
                            self.avm2_coerce_to_string_eval(first, abc_idx, steps, depth)
                        }
                    };
                    let radix = args
                        .get(1)
                        .map_or(0, |v| ecma_f64_to_i32(avm2_to_number(v)));
                    return Avm1Value::Number(parse_avm_int_radix(&s, radix));
                }
                "getTimer" => {
                    return Avm1Value::Number(0.0);
                }
                "parseFloat" => {
                    let Some(first) = args.first() else {
                        return Avm1Value::Number(f64::NAN);
                    };
                    let s = self.avm2_coerce_to_string_eval(first, abc_idx, steps, depth);
                    return Avm1Value::Number(parse_avm2_float(&s));
                }
                "XML" => {
                    return self.construct_avm2_xml(&args, true, abc_idx, steps, depth);
                }
                "XMLList" => {
                    return self.construct_avm2_xml_list(&args, true, abc_idx, steps, depth);
                }
                "getQualifiedClassName" => {
                    let target = args.first().unwrap_or(&Avm1Value::Undefined);
                    return Avm1Value::String(self.avm2_runtime_class_label(target));
                }
                "getDefinitionByName" => {
                    let q = args
                        .first()
                        .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                    if let Some(def) = self.resolve_avm2_definition_by_name(abc_idx, &q) {
                        return def;
                    }
                    let short_q = q
                        .rsplit_once("::")
                        .or_else(|| q.rsplit_once('.'))
                        .map_or(q.as_str(), |(_, s)| s);
                    self.throw_avm2_builtin_error(
                        "ReferenceError",
                        1065,
                        &format!("Variable {short_q} is not defined."),
                    );
                    return Avm1Value::Undefined;
                }
                _ => {
                    if prop.starts_with("Vector.<") {
                        let elem_ty =
                            self.resolve_avm2_vector_elem_type_from_name(abc_idx, "", prop);
                        return self.call_avm2_vector_cast(elem_ty, &args, abc_idx, steps, depth);
                    }
                    if let Some((c_abc, c_idx)) = self.find_avm2_class(abc_idx, "", prop) {
                        return self.coerce_avm2_class_cast(c_abc, c_idx, &args);
                    }
                }
            }
        }

        match prop {
            "trace" => {
                let mut parts = Vec::with_capacity(args.len());
                for a in &args {
                    let s = self.avm2_coerce_to_string_eval(a, abc_idx, steps, depth);
                    if self.avm2_thrown.is_some() {
                        return Avm1Value::Undefined;
                    }
                    parts.push(s);
                }
                self.record_trace(parts.join(" "));
                return Avm1Value::Undefined;
            }
            "addFrameScript" => {
                for pair in args.as_chunks::<2>().0 {
                    let f_idx = ecma_f64_to_i32(avm2_to_number(&pair[0])).max(0) as u32;
                    match &pair[1] {
                        Avm1Value::Null | Avm1Value::Undefined => {
                            self.avm2_frame_scripts.remove(&f_idx);
                        }
                        Avm1Value::String(s) if is_avm2_fn_token(s) => {
                            self.avm2_frame_scripts
                                .insert(f_idx, Avm1Value::String(s.clone()));
                        }
                        _ => {}
                    }
                }
                return Avm1Value::Undefined;
            }
            "addChild" => {
                if let Some(child_arg) = args.first() {
                    return self.avm2_add_child_to_target("_root", child_arg);
                }
                return Avm1Value::Undefined;
            }
            "removeChild" => {
                if let Some(child_arg) = args.first() {
                    return self.avm2_remove_child_value(child_arg);
                }
                return Avm1Value::Undefined;
            }
            "getChildByName" => {
                let want = args
                    .first()
                    .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                if !want.is_empty()
                    && let Some((d, slot)) = self.display_list.iter().find(|(_, s)| s.name == want)
                {
                    return Avm1Value::MovieClipIdentity(live_slot_segment(*d, slot));
                }
                return Avm1Value::Null;
            }
            "stop" => {
                self.playing = false;
                return Avm1Value::Undefined;
            }
            "play" => {
                self.playing = true;
                return Avm1Value::Undefined;
            }
            "nextFrame" => {
                let total = self.root_timeline.frames.len().max(1) as u32;
                self.current_frame = (self.current_frame + 1).min(total.saturating_sub(1));
                self.playing = false;
                return Avm1Value::Undefined;
            }
            "prevFrame" => {
                self.current_frame = self.current_frame.saturating_sub(1);
                self.playing = false;
                return Avm1Value::Undefined;
            }
            "gotoAndStop" | "gotoAndPlay" => {
                if let Some(first) = args.first() {
                    let scene_offset = args
                        .get(1)
                        .and_then(|s| match s {
                            Avm1Value::String(name) => self
                                .scenes
                                .iter()
                                .find(|(_, sn)| sn == name)
                                .map(|(off, _)| *off),
                            _ => None,
                        })
                        .unwrap_or(0);
                    let total = self.root_timeline.frames.len().max(1) as u32;
                    let resolved = match first {
                        Avm1Value::Number(n) if *n >= 1.0 => {
                            let f = scene_offset + ((*n as i64).saturating_sub(1).max(0) as u32);
                            Some(f.min(total.saturating_sub(1)))
                        }
                        Avm1Value::String(lbl) => {
                            self.root_timeline.resolve_avm2_label(lbl).or_else(|| {
                                self.scenes
                                    .iter()
                                    .find(|(_, sn)| sn == lbl)
                                    .map(|(off, _)| *off)
                            })
                        }
                        _ => None,
                    };
                    if let Some(f) = resolved {
                        self.current_frame = f;
                        self.playing = prop == "gotoAndPlay";
                    }
                }
                return Avm1Value::Undefined;
            }
            "stopAll" => {
                self.stop_swf_sounds();
                return Avm1Value::Undefined;
            }
            _ => {}
        }

        if let Some(gval) = self.avm2_globals.get(prop).cloned() {
            let is_callable_global = match &gval {
                Avm1Value::String(tok) => is_avm2_fn_token(tok),
                Avm1Value::MovieClip(mc) => {
                    mc.starts_with("__avm2_type:")
                        || mc.starts_with("__avm2_class:")
                        || mc.starts_with("__avm2_vec_type:")
                }
                _ => false,
            };
            if is_callable_global {
                return self.invoke_avm2_callable_with_this(
                    &gval,
                    Avm1Value::MovieClip("_root".into()),
                    args,
                    steps,
                    depth,
                );
            }
        }

        if let Avm1Value::Object(oid) = receiver
            && self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.array_elements.is_some())
        {
            match prop {
                "every" | "some" | "forEach" | "filter" | "map" => {
                    let cb = args.first().cloned().unwrap_or(Avm1Value::Null);
                    let this_obj = args
                        .get(1)
                        .cloned()
                        .unwrap_or(Avm1Value::MovieClip("_root".into()));
                    if matches!(cb, Avm1Value::Null | Avm1Value::Undefined) {
                        return match prop {
                            "every" => Avm1Value::Bool(true),
                            "some" => Avm1Value::Bool(false),
                            "filter" | "map" => {
                                let id = self.avm1_objects.len();
                                self.avm1_objects.push(Avm1Object {
                                    array_elements: Some(Vec::new()),
                                    ..Default::default()
                                });
                                Avm1Value::Object(id)
                            }
                            _ => Avm1Value::Undefined,
                        };
                    }
                    let snapshot = self
                        .avm1_objects
                        .get(*oid)
                        .and_then(|o| o.array_elements.clone())
                        .unwrap_or_default();
                    let mut out_elems = Vec::new();
                    for (i, item) in snapshot.into_iter().enumerate() {
                        let r = self.invoke_avm2_callable_with_this(
                            &cb,
                            this_obj.clone(),
                            vec![
                                item.clone(),
                                Avm1Value::Number(i as f64),
                                Avm1Value::Object(*oid),
                            ],
                            steps,
                            depth,
                        );
                        if self.avm2_thrown.is_some() {
                            return Avm1Value::Undefined;
                        }
                        match prop {
                            "every" if !r.to_bool(9) => return Avm1Value::Bool(false),
                            "some" if r.to_bool(9) => return Avm1Value::Bool(true),
                            "filter" if r.to_bool(9) => out_elems.push(item),
                            "map" => out_elems.push(r),
                            _ => {}
                        }
                    }
                    return match prop {
                        "every" => Avm1Value::Bool(true),
                        "some" => Avm1Value::Bool(false),
                        "filter" | "map" => {
                            let id = self.avm1_objects.len();
                            self.avm1_objects.push(Avm1Object {
                                array_elements: Some(out_elems),
                                ..Default::default()
                            });
                            Avm1Value::Object(id)
                        }
                        _ => Avm1Value::Undefined,
                    };
                }
                "concat" => {
                    const MAX_AVM2_CONCAT_LEN: usize = 65_536;
                    let mut out_elems = self
                        .avm1_objects
                        .get(*oid)
                        .and_then(|o| o.array_elements.as_ref())
                        .map(|v| v[..v.len().min(MAX_AVM2_CONCAT_LEN)].to_vec())
                        .unwrap_or_default();
                    for a in args {
                        if out_elems.len() >= MAX_AVM2_CONCAT_LEN {
                            break;
                        }
                        let rem = MAX_AVM2_CONCAT_LEN - out_elems.len();
                        if let Avm1Value::Object(aid) = &a
                            && let Some(sub_len) = self
                                .avm1_objects
                                .get(*aid)
                                .and_then(|o| o.array_elements.as_ref().map(Vec::len))
                        {
                            let take_n = sub_len.min(rem);
                            if let Some(sub) = self
                                .avm1_objects
                                .get(*aid)
                                .and_then(|o| o.array_elements.as_ref())
                            {
                                out_elems.extend_from_slice(&sub[..take_n]);
                            }
                        } else {
                            out_elems.push(a);
                        }
                    }
                    let id = self.avm1_objects.len();
                    self.avm1_objects.push(Avm1Object {
                        array_elements: Some(out_elems),
                        ..Default::default()
                    });
                    return Avm1Value::Object(id);
                }
                "toString" | "toLocaleString" => {
                    let len = self
                        .avm1_objects
                        .get(*oid)
                        .and_then(|o| o.array_elements.as_ref().map(Vec::len))
                        .unwrap_or(0);
                    let mut parts = Vec::with_capacity(len);
                    for i in 0..len {
                        let e = self
                            .get_avm1_object_prop_chain(*oid, &i.to_string())
                            .unwrap_or(Avm1Value::Undefined);
                        if matches!(e, Avm1Value::Undefined | Avm1Value::Null) {
                            parts.push(String::new());
                        } else if prop == "toLocaleString" {
                            let loc = self.call_avm2_property(
                                abc_idx,
                                &e,
                                "toLocaleString",
                                Vec::new(),
                                steps,
                                depth,
                            );
                            if self.avm2_thrown.is_some() {
                                return Avm1Value::Undefined;
                            }
                            let s = self.avm2_coerce_to_string_eval(&loc, abc_idx, steps, depth);
                            if self.avm2_thrown.is_some() {
                                return Avm1Value::Undefined;
                            }
                            parts.push(s);
                        } else {
                            let s = self.avm2_coerce_to_string_eval(&e, abc_idx, steps, depth);
                            if self.avm2_thrown.is_some() {
                                return Avm1Value::Undefined;
                            }
                            parts.push(s);
                        }
                    }
                    return Avm1Value::String(parts.join(","));
                }
                "hasOwnProperty" => {
                    let key = args
                        .first()
                        .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                    let has = self.avm1_objects.get(*oid).is_some_and(|o| {
                        o.props.contains_key(&key)
                            || o.array_elements.as_ref().is_some_and(|el| {
                                key.parse::<usize>().is_ok_and(|idx| {
                                    idx < el.len() && !o.deleted_indices.contains(&idx)
                                })
                            })
                    });
                    return Avm1Value::Bool(has);
                }
                _ => {}
            }
        }
        if matches!(receiver, Avm1Value::Object(_)) && prop == "valueOf" {
            return receiver.clone();
        }

        if let Avm1Value::String(s) = receiver
            && !is_avm2_fn_token(s)
        {
            match prop {
                "charAt" => {
                    let units: Vec<u16> = s.encode_utf16().collect();
                    let idx_f = args.first().map_or(0.0, avm2_to_number);
                    let idx = if idx_f.is_nan() {
                        0isize
                    } else {
                        idx_f.trunc() as isize
                    };
                    if idx < 0 || (idx as usize) >= units.len() {
                        return Avm1Value::String(String::new());
                    }
                    return Avm1Value::String(String::from_utf16_lossy(&[units[idx as usize]]));
                }
                "charCodeAt" => {
                    let units: Vec<u16> = s.encode_utf16().collect();
                    let idx_f = args.first().map_or(0.0, avm2_to_number);
                    if !idx_f.is_finite() && !idx_f.is_nan() {
                        return Avm1Value::Number(f64::NAN);
                    }
                    let idx = if idx_f.is_nan() {
                        0isize
                    } else {
                        idx_f.trunc() as isize
                    };
                    if idx < 0 || (idx as usize) >= units.len() {
                        return Avm1Value::Number(f64::NAN);
                    }
                    return Avm1Value::Number(f64::from(units[idx as usize]));
                }
                "substr" => {
                    let units: Vec<u16> = s.encode_utf16().collect();
                    let total = units.len() as f64;
                    let to_int_f64 = |v: &Avm1Value| -> f64 {
                        let n = avm2_to_number(v);
                        if n.is_nan() || n == 0.0 {
                            0.0
                        } else if n.is_infinite() {
                            n
                        } else {
                            n.trunc()
                        }
                    };
                    let mut start = args.first().map_or(0.0, to_int_f64);
                    if start < 0.0 {
                        start = (total + start).max(0.0);
                    }
                    let end = match args.get(1) {
                        None | Some(Avm1Value::Undefined) => total,
                        Some(v) => {
                            let len_arg = to_int_f64(v);
                            let mut e = start + len_arg;
                            if len_arg < 0.0 && e < 0.0 {
                                e += total;
                            }
                            e
                        }
                    };
                    let su = if start.is_nan() {
                        0usize
                    } else {
                        start.clamp(0.0, total) as usize
                    };
                    let eu = if end.is_nan() {
                        0usize
                    } else {
                        end.clamp(0.0, total) as usize
                    }
                    .max(su);
                    return Avm1Value::String(String::from_utf16_lossy(&units[su..eu]));
                }
                "toLocaleString" => {
                    return Avm1Value::String(s.clone());
                }
                _ => {}
            }
        }

        // Check if `prop` is a method on the class in `abc_idx` (only for global/timeline receivers)
        if matches!(receiver, Avm1Value::MovieClip(_))
            && let Some(loaded) = self.abc_tags.get(abc_idx)
        {
            for inst in &loaded.abc.instances {
                if (inst.flags & 0x04) != 0 {
                    continue;
                }
                for tr in &inst.traits {
                    let (_, tname) = loaded.abc.split_qname(tr.name);
                    if tname == prop
                        && let AbcTraitData::Method { method } | AbcTraitData::Function { method } =
                            tr.data
                    {
                        return self.run_avm2_method_bounded(
                            abc_idx,
                            method,
                            args,
                            steps,
                            depth + 1,
                        );
                    }
                }
            }
            for cls in &loaded.abc.classes {
                for tr in &cls.traits {
                    let (_, tname) = loaded.abc.split_qname(tr.name);
                    if tname == prop
                        && let AbcTraitData::Method { method } | AbcTraitData::Function { method } =
                            tr.data
                    {
                        return self.run_avm2_method_bounded(
                            abc_idx,
                            method,
                            args,
                            steps,
                            depth + 1,
                        );
                    }
                }
            }
        }

        let mut dummy_locals = HashMap::new();
        self.call_avm1_method(receiver, prop, args, &mut dummy_locals, steps, depth)
    }
}
