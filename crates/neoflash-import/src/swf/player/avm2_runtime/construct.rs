//! AVM2 class/value instantiation, slot defaults, activation keys, and callable invocation.

use super::super::*;

impl SwfBytecodePlayer {
    pub(in crate::swf::player) fn construct_avm2_value(
        &mut self,
        abc_idx: usize,
        ctor: &Avm1Value,
        cargs: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::MovieClip(s) | Avm1Value::String(s) = ctor {
            if let Some(vec_tok) = s.strip_prefix("__avm2_vec_type:") {
                let elem_ty = Avm2VectorElemType::decode_token(vec_tok);
                return self.construct_avm2_vector(elem_ty, &cargs, abc_idx, steps, depth);
            }
            if let Some((c_abc, c_cls)) = parse_avm2_class_token(s) {
                return self.instantiate_avm2_user_class(c_abc, c_cls, cargs, steps, depth);
            }
            let tname = s.strip_prefix("__avm2_type:").unwrap_or(s.as_str());
            return self.construct_avm2_named(abc_idx, "", tname, cargs, steps, depth);
        }
        let obj_id = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object::default());
        Avm1Value::Object(obj_id)
    }

    pub(in crate::swf::player) fn construct_avm2_named(
        &mut self,
        abc_idx: usize,
        pkg: &str,
        cls_name: &str,
        cargs: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        match cls_name {
            "Dictionary" => {
                let weak = cargs.first().is_some_and(|v| v.to_bool(9));
                let obj_id = self.avm1_objects.len();
                self.avm1_objects.push(Avm1Object {
                    avm2_dictionary: Some(Avm2DictionaryData {
                        weak_keys: weak,
                        entries: Vec::new(),
                    }),
                    ..Default::default()
                });
                return Avm1Value::Object(obj_id);
            }
            "XML" => {
                return self.construct_avm2_xml(&cargs, false, abc_idx, steps, depth);
            }
            "XMLList" => {
                return self.construct_avm2_xml_list(&cargs, false, abc_idx, steps, depth);
            }
            "QName" => {
                let (uri_opt, ln) = if cargs.len() >= 2 {
                    let l = if let Avm1Value::Object(qoid) = &cargs[1]
                        && let Some(qo) = self.avm1_objects.get(*qoid)
                        && let Some(Avm1Value::String(existing_ln)) = qo.props.get("localName")
                    {
                        existing_ln.clone()
                    } else if matches!(cargs[1], Avm1Value::Undefined) {
                        String::new()
                    } else {
                        avm2_convert_string(&cargs[1], &self.avm1_objects)
                    };
                    let u = match &cargs[0] {
                        Avm1Value::Null => None,
                        Avm1Value::Undefined => {
                            if l == "*" {
                                None
                            } else {
                                Some(self.e4x_arena.borrow().default_ns.clone())
                            }
                        }
                        Avm1Value::Object(ns_oid)
                            if self
                                .avm1_objects
                                .get(*ns_oid)
                                .is_some_and(|o| o.props.contains_key("uri")) =>
                        {
                            match self
                                .avm1_objects
                                .get(*ns_oid)
                                .and_then(|o| o.props.get("uri"))
                            {
                                Some(Avm1Value::Null) => None,
                                Some(v) => Some(avm2_convert_string(v, &self.avm1_objects)),
                                None => Some(String::new()),
                            }
                        }
                        other => Some(avm2_convert_string(other, &self.avm1_objects)),
                    };
                    (u, l)
                } else if let Some(first) = cargs.first() {
                    if let Avm1Value::Object(qoid) = first
                        && let Some(qo) = self.avm1_objects.get(*qoid)
                        && let Some(Avm1Value::String(ln)) = qo.props.get("localName")
                    {
                        let u = match qo.props.get("uri") {
                            Some(Avm1Value::Null) => None,
                            Some(v) => Some(avm2_convert_string(v, &self.avm1_objects)),
                            None => Some(String::new()),
                        };
                        (u, ln.clone())
                    } else if matches!(first, Avm1Value::Undefined) {
                        (
                            Some(self.e4x_arena.borrow().default_ns.clone()),
                            String::new(),
                        )
                    } else {
                        let l = avm2_convert_string(first, &self.avm1_objects);
                        if l == "*" {
                            (None, l)
                        } else {
                            (Some(self.e4x_arena.borrow().default_ns.clone()), l)
                        }
                    }
                } else {
                    (
                        Some(self.e4x_arena.borrow().default_ns.clone()),
                        String::new(),
                    )
                };
                return self.alloc_avm2_qname_opt(uri_opt.as_deref(), &ln);
            }
            "Namespace" => {
                let (pfx, uri) = if cargs.len() >= 2 {
                    (
                        avm2_convert_string(&cargs[0], &self.avm1_objects),
                        avm2_convert_string(&cargs[1], &self.avm1_objects),
                    )
                } else if let Some(first) = cargs.first() {
                    (
                        String::new(),
                        avm2_convert_string(first, &self.avm1_objects),
                    )
                } else {
                    (String::new(), String::new())
                };
                let mut props = HashMap::new();
                props.insert("prefix".to_owned(), Avm1Value::String(pfx));
                props.insert("uri".to_owned(), Avm1Value::String(uri.clone()));
                props.insert("qname_formatted".to_owned(), Avm1Value::String(uri));
                props.insert(
                    "__avm2_short_class__".to_owned(),
                    Avm1Value::String("Namespace".to_owned()),
                );
                let oid = self.avm1_objects.len();
                self.avm1_objects.push(Avm1Object {
                    props,
                    ..Default::default()
                });
                return Avm1Value::Object(oid);
            }
            "Array" => {
                let (elems, deleted_indices) = if cargs.len() == 1
                    && let Avm1Value::Number(n) = cargs[0]
                    && n.is_finite()
                    && n.fract() == 0.0
                    && (0.0..=4096.0).contains(&n)
                {
                    let len = n as usize;
                    (vec![Avm1Value::Undefined; len], (0..len).collect())
                } else {
                    (cargs, BTreeSet::new())
                };
                let obj_id = self.avm1_objects.len();
                self.avm1_objects.push(Avm1Object {
                    array_elements: Some(elems),
                    deleted_indices,
                    ..Default::default()
                });
                return Avm1Value::Object(obj_id);
            }
            "String" => {
                let s = cargs
                    .first()
                    .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                return Avm1Value::String(s);
            }
            "Number" => {
                let n = cargs.first().map_or(0.0, avm2_to_number);
                return Avm1Value::Number(n);
            }
            "int" => {
                let i = cargs
                    .first()
                    .map_or(0, |v| ecma_f64_to_i32(avm2_to_number(v)));
                return Avm1Value::Number(f64::from(i));
            }
            "uint" => {
                let u = cargs
                    .first()
                    .map_or(0, |v| ecma_f64_to_u32(avm2_to_number(v)));
                return Avm1Value::Number(f64::from(u));
            }
            "Boolean" => {
                let b = cargs.first().is_some_and(|v| v.to_bool(9));
                return Avm1Value::Bool(b);
            }
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "ArgumentError"
            | "VerifyError" | "SyntaxError" | "URIError" | "EvalError" | "SecurityError" => {
                let msg = cargs
                    .first()
                    .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                let id_num = cargs.get(1).map_or(0.0, avm2_to_number);
                let mut props = HashMap::new();
                props.insert("name".to_owned(), Avm1Value::String(cls_name.to_owned()));
                props.insert("message".to_owned(), Avm1Value::String(msg));
                props.insert("errorID".to_owned(), Avm1Value::Number(id_num));
                let obj_id = self.avm1_objects.len();
                self.avm1_objects.push(Avm1Object {
                    props,
                    avm2_builtin_error: Some(cls_name.to_owned()),
                    ..Default::default()
                });
                return Avm1Value::Object(obj_id);
            }
            "Event" => {
                let ev_ty = cargs
                    .first()
                    .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                let bubbles = cargs.get(1).is_some_and(|v| v.to_bool(9));
                let cancelable = cargs.get(2).is_some_and(|v| v.to_bool(9));
                let obj_id = self.avm1_objects.len();
                self.avm1_objects.push(Avm1Object {
                    avm2_event: Some((ev_ty, bubbles, cancelable, obj_id, obj_id, 2)),
                    ..Default::default()
                });
                return Avm1Value::Object(obj_id);
            }
            "Sprite" | "MovieClip" | "Shape" | "SimpleButton" | "TextField" => {
                if let Some((c_abc, c_idx)) = self.find_avm2_class(abc_idx, pkg, cls_name) {
                    return self.instantiate_avm2_user_class(c_abc, c_idx, cargs, steps, depth);
                }
                let inst_name = self.next_avm2_instance_name();
                let mut props = HashMap::new();
                props.insert(
                    "__avm2_display_type__".to_owned(),
                    Avm1Value::String(cls_name.to_owned()),
                );
                props.insert("x".to_owned(), Avm1Value::Number(0.0));
                props.insert("y".to_owned(), Avm1Value::Number(0.0));
                props.insert("scaleX".to_owned(), Avm1Value::Number(1.0));
                props.insert("scaleY".to_owned(), Avm1Value::Number(1.0));
                props.insert("alpha".to_owned(), Avm1Value::Number(1.0));
                props.insert("visible".to_owned(), Avm1Value::Bool(true));
                props.insert("rotation".to_owned(), Avm1Value::Number(0.0));
                props.insert("name".to_owned(), Avm1Value::String(inst_name));
                let obj_id = self.avm1_objects.len();
                self.avm1_objects.push(Avm1Object {
                    props,
                    avm2_display: Some(Avm2DisplayMeta {
                        display_type: cls_name.to_owned(),
                        short_class_name: cls_name.to_owned(),
                        constructed: true,
                        super_constructed: true,
                        total_frames: 1,
                        playing: true,
                        ..Default::default()
                    }),
                    graphics: Some(RuntimeGraphicsState::default()),
                    ..Default::default()
                });
                return Avm1Value::Object(obj_id);
            }
            "Object" => {
                let obj_id = self.avm1_objects.len();
                self.avm1_objects.push(Avm1Object::default());
                return Avm1Value::Object(obj_id);
            }
            _ => {}
        }
        if let Some((c_abc, c_idx)) = self.find_avm2_class(abc_idx, pkg, cls_name) {
            return self.instantiate_avm2_user_class(c_abc, c_idx, cargs, steps, depth);
        }
        let obj_id = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object::default());
        Avm1Value::Object(obj_id)
    }

    pub(in crate::swf::player) fn collect_avm2_instance_slot_defaults(
        &self,
        c_abc: usize,
        c_idx: usize,
    ) -> HashMap<String, Avm1Value> {
        let mut chain = Vec::new();
        let mut cur = Some((c_abc, c_idx));
        for _ in 0..32 {
            let Some((a, c)) = cur else { break };
            chain.push((a, c));
            cur = self.avm2_superclass(a, c);
        }
        chain.reverse();

        let mut props = HashMap::new();
        for &(a, c) in &chain {
            if let Some(loaded) = self.abc_tags.get(a)
                && let Some(inst) = loaded.abc.instances.get(c)
            {
                for tr in &inst.traits {
                    if let AbcTraitData::Slot {
                        type_name,
                        vindex,
                        vkind,
                        ..
                    } = tr.data
                    {
                        let (_, t_prop) = loaded.abc.split_qname(tr.name);
                        let init_val = if vindex != 0 || vkind != 0 {
                            abc_const_to_avm_value(&loaded.abc, vindex, vkind)
                        } else {
                            let (_, ty) = loaded.abc.split_qname(type_name);
                            match ty.as_str() {
                                "int" | "uint" => Avm1Value::Number(0.0),
                                "Number" => Avm1Value::Number(f64::NAN),
                                "Boolean" => Avm1Value::Bool(false),
                                "*" | "" => Avm1Value::Undefined,
                                _ => Avm1Value::Null,
                            }
                        };
                        props.insert(t_prop, init_val);
                    }
                }
            }
        }
        props
    }

    pub(in crate::swf::player) fn avm2_display_type_for_class(
        &self,
        c_abc: usize,
        c_idx: usize,
    ) -> Option<(&'static str, String)> {
        let short = self
            .abc_tags
            .get(c_abc)
            .and_then(|l| l.abc.instances.get(c_idx))
            .map(|inst| self.abc_tags[c_abc].abc.split_qname(inst.name).1)?;
        for dt in [
            "MovieClip",
            "SimpleButton",
            "TextField",
            "Shape",
            "Bitmap",
            "Video",
            "Sprite",
            "DisplayObjectContainer",
            "InteractiveObject",
            "DisplayObject",
        ] {
            if self.avm2_class_extends_builtin(c_abc, c_idx, dt) {
                return Some((dt, short));
            }
        }
        None
    }

    pub(in crate::swf::player) fn next_avm2_instance_name(&mut self) -> String {
        let id = self.avm2_next_instance_id.max(1);
        self.avm2_next_instance_id = id.saturating_add(1);
        format!("instance{id}")
    }

    pub(in crate::swf::player) fn instantiate_avm2_user_class(
        &mut self,
        c_abc: usize,
        c_idx: usize,
        cargs: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Some(iface_qname) = self.abc_tags.get(c_abc).and_then(|l| {
            let inst = l.abc.instances.get(c_idx)?;
            if (inst.flags & 0x04) == 0 {
                return None;
            }
            let (ipkg, iname) = l.abc.split_qname(inst.name);
            Some(if ipkg.is_empty() {
                iname
            } else {
                format!("{ipkg}::{iname}")
            })
        }) {
            self.throw_avm2_builtin_error(
                "VerifyError",
                1001,
                &format!("The method {iface_qname}() is not implemented."),
            );
            return Avm1Value::Undefined;
        }
        let mut props = self.collect_avm2_instance_slot_defaults(c_abc, c_idx);
        let short_cls = self
            .abc_tags
            .get(c_abc)
            .and_then(|l| l.abc.instances.get(c_idx))
            .map(|inst| self.abc_tags[c_abc].abc.split_qname(inst.name).1)
            .unwrap_or_else(|| "Object".to_owned());
        props.insert(
            "__avm2_short_class__".to_owned(),
            Avm1Value::String(short_cls),
        );

        let avm2_display =
            if let Some((dt, short_name)) = self.avm2_display_type_for_class(c_abc, c_idx) {
                let inst_name = self.next_avm2_instance_name();
                let cid = self.resolve_avm2_symbol_character_id(c_abc, c_idx);
                let total_frames = if cid != 0 {
                    self.sprites.get(&cid).map_or(1, |s| s.total_frames.max(1))
                } else {
                    1
                };
                props
                    .entry("x".to_owned())
                    .or_insert(Avm1Value::Number(0.0));
                props
                    .entry("y".to_owned())
                    .or_insert(Avm1Value::Number(0.0));
                props
                    .entry("scaleX".to_owned())
                    .or_insert(Avm1Value::Number(1.0));
                props
                    .entry("scaleY".to_owned())
                    .or_insert(Avm1Value::Number(1.0));
                props
                    .entry("alpha".to_owned())
                    .or_insert(Avm1Value::Number(1.0));
                props
                    .entry("visible".to_owned())
                    .or_insert(Avm1Value::Bool(true));
                props
                    .entry("rotation".to_owned())
                    .or_insert(Avm1Value::Number(0.0));
                props
                    .entry("name".to_owned())
                    .or_insert(Avm1Value::String(inst_name));
                Some(Avm2DisplayMeta {
                    display_type: dt.to_owned(),
                    short_class_name: short_name,
                    character_id: cid,
                    constructed: true,
                    super_constructed: false,
                    total_frames,
                    playing: true,
                    ..Default::default()
                })
            } else {
                None
            };

        let has_disp = avm2_display.is_some();
        let obj_id = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object {
            props,
            avm2_class: Some((c_abc, c_idx)),
            avm2_display,
            graphics: has_disp.then(RuntimeGraphicsState::default),
            ..Default::default()
        });
        if let Some(iinit) = self
            .abc_tags
            .get(c_abc)
            .and_then(|l| l.abc.instances.get(c_idx))
            .map(|inst| inst.iinit)
        {
            let _ = self.run_avm2_method_with_this(
                c_abc,
                iinit,
                Avm1Value::Object(obj_id),
                cargs,
                steps,
                depth + 1,
            );
        }
        Avm1Value::Object(obj_id)
    }

    pub(in crate::swf::player) fn resolve_avm2_activation_or_slot_key(
        &self,
        obj: &Avm1Value,
        slot_idx: usize,
        cur_abc: usize,
        cur_method: usize,
    ) -> (String, Option<(usize, usize)>) {
        if matches!(obj, Avm1Value::MovieClip(m) if m == "_root")
            && let Some(loaded) = self.abc_tags.get(cur_abc)
        {
            let resolve_in_traits = |traits: &[crate::swf::avm2::AbcTrait]| -> Option<(String, Option<(usize, usize)>)> {
                for (ti, tr) in traits.iter().enumerate() {
                    match tr.data {
                        AbcTraitData::Slot {
                            slot_id, type_name, ..
                        } => {
                            let rslot = if slot_id == 0 { ti + 1 } else { slot_id };
                            if rslot == slot_idx {
                                let (_, sname) = loaded.abc.split_qname(tr.name);
                                if !sname.is_empty() {
                                    return Some((sname, Some((cur_abc, type_name))));
                                }
                            }
                        }
                        AbcTraitData::Class { .. } => {
                            let rslot = ti + 1;
                            if rslot == slot_idx {
                                let (_, sname) = loaded.abc.split_qname(tr.name);
                                if !sname.is_empty() {
                                    return Some((sname, None));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                None
            };
            if let Some((_, traits)) = loaded
                .abc
                .scripts
                .iter()
                .find(|(init, _)| *init == cur_method)
                && let Some(hit) = resolve_in_traits(traits)
            {
                return hit;
            }
            for (_, traits) in loaded.abc.scripts.iter().rev() {
                if let Some(hit) = resolve_in_traits(traits) {
                    return hit;
                }
            }
        }
        if let Avm1Value::Object(oid) = obj {
            if slot_idx == 1
                && let Some((vname, c_abc, c_ty)) = self
                    .avm1_objects
                    .get(*oid)
                    .and_then(|o| o.avm2_catch_var.as_ref())
            {
                let key = if vname.is_empty() {
                    "__slot_1".to_owned()
                } else {
                    vname.clone()
                };
                return (key, Some((*c_abc, *c_ty)));
            }
            if let Some((a_idx, m_idx)) = self
                .avm1_objects
                .get(*oid)
                .and_then(|o| o.avm2_activation_method)
                && let Some(loaded) = self.abc_tags.get(a_idx)
                && let Some(body) = loaded.abc.bodies_by_method.get(&m_idx)
            {
                for (ti, tr) in body.traits.iter().enumerate() {
                    if let AbcTraitData::Slot {
                        slot_id, type_name, ..
                    } = tr.data
                    {
                        let resolved_slot = if slot_id == 0 { ti + 1 } else { slot_id };
                        if resolved_slot == slot_idx {
                            let (_, sname) = loaded.abc.split_qname(tr.name);
                            if !sname.is_empty() {
                                return (sname, Some((a_idx, type_name)));
                            }
                        }
                    }
                }
            }
        }
        (format!("__slot_{slot_idx}"), None)
    }

    pub(in crate::swf::player) fn resolve_avm2_slot_name(
        &self,
        c_abc: usize,
        c_idx: usize,
        slot_key: &str,
    ) -> Option<String> {
        let slot_num: usize = slot_key.strip_prefix("__slot_")?.parse().ok()?;
        let mut cur = Some((c_abc, c_idx));
        for _ in 0..32 {
            let Some((a, c)) = cur else { break };
            if let Some(loaded) = self.abc_tags.get(a)
                && let Some(inst) = loaded.abc.instances.get(c)
            {
                for tr in &inst.traits {
                    if let AbcTraitData::Slot { slot_id, .. } = tr.data
                        && slot_id == slot_num
                    {
                        return Some(loaded.abc.split_qname(tr.name).1);
                    }
                }
            }
            cur = self.avm2_superclass(a, c);
        }
        None
    }

    pub(in crate::swf::player) fn invoke_avm2_callable_with_this(
        &mut self,
        fn_val: &Avm1Value,
        receiver: Avm1Value,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::String(s) = fn_val {
            if let Some((cid, a_idx, m_idx)) = parse_avm2_closure_token(s) {
                let (exec_abc, exec_method, outer) = self
                    .avm2_closures
                    .get(cid)
                    .map(|c| (c.abc_idx, c.method_idx, c.outer_scopes.clone()))
                    .unwrap_or_else(|| {
                        (a_idx, m_idx, vec![Avm1Value::MovieClip("_root".to_owned())])
                    });
                let this_arg = if matches!(receiver, Avm1Value::Undefined | Avm1Value::Null) {
                    Avm1Value::MovieClip("_root".to_owned())
                } else {
                    receiver
                };
                return self.run_avm2_method_with_outer_scopes(
                    exec_abc,
                    exec_method,
                    this_arg,
                    outer,
                    args,
                    steps,
                    depth + 1,
                );
            }
            if let Some((oid, a_idx, m_idx)) = parse_avm2_bound_token(s) {
                return self.run_avm2_method_with_this(
                    a_idx,
                    m_idx,
                    Avm1Value::Object(oid),
                    args,
                    steps,
                    depth + 1,
                );
            }
            if let Some((a_idx, m_idx)) = parse_avm2_fn_token(s) {
                let this_arg = if matches!(receiver, Avm1Value::Undefined | Avm1Value::Null) {
                    self.avm2_root_oid.map_or_else(
                        || Avm1Value::MovieClip("_root".to_owned()),
                        Avm1Value::Object,
                    )
                } else {
                    receiver
                };
                return self.run_avm2_method_with_this(
                    a_idx,
                    m_idx,
                    this_arg,
                    args,
                    steps,
                    depth + 1,
                );
            }
            if let Some(rest) = s.strip_prefix("__avm2_bound_vec:")
                && let Some((oid_str, mname)) = rest.split_once(':')
                && let Ok(oid) = oid_str.parse::<usize>()
            {
                return self
                    .call_avm2_vector_method(oid, mname, &args, 0, steps, depth)
                    .unwrap_or(Avm1Value::Undefined);
            }
        }
        if let Avm1Value::MovieClip(mc) = fn_val {
            if let Some(vec_tok) = mc.strip_prefix("__avm2_vec_type:") {
                let elem_ty = Avm2VectorElemType::decode_token(vec_tok);
                return self.call_avm2_vector_cast(elem_ty, &args, 0, steps, depth);
            }
            if mc == "__avm2_type:XML" {
                return self.construct_avm2_xml(&args, true, 0, steps, depth);
            }
            if mc == "__avm2_type:XMLList" {
                return self.construct_avm2_xml_list(&args, true, 0, steps, depth);
            }
        }
        if let Avm1Value::MovieClip(mc) = fn_val
            && let Some((c_abc, c_idx)) = parse_avm2_class_token(mc)
        {
            return self.coerce_avm2_class_cast(c_abc, c_idx, &args);
        }
        if let Avm1Value::MovieClip(mc) = fn_val
            && let Some(tname) = mc.strip_prefix("__avm2_type:")
        {
            if args.is_empty() {
                return match tname {
                    "int" | "uint" => Avm1Value::Number(0.0),
                    "Number" => Avm1Value::Number(0.0),
                    "String" => Avm1Value::String(String::new()),
                    "Boolean" => Avm1Value::Bool(false),
                    "Object" => {
                        let obj_id = self.avm1_objects.len();
                        self.avm1_objects.push(Avm1Object::default());
                        Avm1Value::Object(obj_id)
                    }
                    "Array" => {
                        let obj_id = self.avm1_objects.len();
                        self.avm1_objects.push(Avm1Object {
                            array_elements: Some(Vec::new()),
                            ..Default::default()
                        });
                        Avm1Value::Object(obj_id)
                    }
                    _ => Avm1Value::Undefined,
                };
            }
            if args.len() == 1 {
                let v = &args[0];
                return match tname {
                    "int" => Avm1Value::Number(f64::from(ecma_f64_to_i32(avm2_to_number(v)))),
                    "uint" => Avm1Value::Number(f64::from(ecma_f64_to_u32(avm2_to_number(v)))),
                    "Number" => Avm1Value::Number(avm2_to_number(v)),
                    "String" => {
                        let s = self.avm2_coerce_to_string_eval(v, 0, steps, depth);
                        Avm1Value::String(s)
                    }
                    "Boolean" => Avm1Value::Bool(v.to_bool(9)),
                    "Object" => match v {
                        Avm1Value::Undefined | Avm1Value::Null => {
                            let obj_id = self.avm1_objects.len();
                            self.avm1_objects.push(Avm1Object::default());
                            Avm1Value::Object(obj_id)
                        }
                        other => other.clone(),
                    },
                    _ => {
                        if matches!(v, Avm1Value::Undefined | Avm1Value::Null) {
                            Avm1Value::Null
                        } else if self.avm2_is_type_name(0, v, "", tname) {
                            v.clone()
                        } else {
                            self.throw_avm2_builtin_error(
                                "TypeError",
                                1034,
                                &format!("Type Coercion failed: cannot convert value to {tname}."),
                            );
                            Avm1Value::Null
                        }
                    }
                };
            }
        }
        Avm1Value::Undefined
    }
}
