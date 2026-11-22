use super::*;

impl SwfBytecodePlayer {
    // =========================================================================
    // AVM2 Bytecode Interpreter
    // =========================================================================

    pub(super) fn initialize_avm2(&mut self) {
        let abc_count = self.abc_tags.len();
        for abc_idx in 0..abc_count {
            // 1. Run script initializers (`script.init`)
            let script_inits: Vec<usize> = self.abc_tags[abc_idx]
                .abc
                .scripts
                .iter()
                .map(|(init, _)| *init)
                .collect();
            for init_m in script_inits {
                self.avm2_thrown = None;
                let mut steps = 0usize;
                let _ = self.run_avm2_method_with_outer_scopes(
                    abc_idx,
                    init_m,
                    Avm1Value::MovieClip("_root".to_owned()),
                    Vec::new(),
                    Vec::new(),
                    &mut steps,
                    0,
                );
                self.avm2_thrown = None;
            }
        }

        // 2. Run class static initializers (`cinit`) across all ABC tags before constructing root
        let mut total_classes = 0usize;
        for abc_idx in 0..abc_count {
            let class_count = self.abc_tags[abc_idx].abc.classes.len();
            total_classes += class_count;
            for c_idx in 0..class_count {
                let cinit = self.abc_tags[abc_idx].abc.classes[c_idx].cinit;
                let mut cinit_steps = 0usize;
                let cinit_scopes = self.avm2_captured_class_scopes(abc_idx, c_idx, false);
                self.avm2_thrown = None;
                let _ = self.run_avm2_method_with_outer_scopes(
                    abc_idx,
                    cinit,
                    Avm1Value::MovieClip(format!("__avm2_class:{abc_idx}:{c_idx}")),
                    cinit_scopes,
                    Vec::new(),
                    &mut cinit_steps,
                    0,
                );
                self.avm2_thrown = None;
            }
        }

        // 3. Resolve root document class (`SymbolClass[0]`, `MainTimeline`, or single-class SWF without child bindings)
        let doc_class: Option<(usize, usize)> = if let Some(bound0) = self.symbol_classes.get(&0) {
            let (pkg, short) = match bound0.rsplit_once('.') {
                Some((p, s)) => (p, s),
                None => ("", bound0.as_str()),
            };
            (0..abc_count).find_map(|ai| self.find_avm2_class(ai, pkg, short))
        } else {
            (0..abc_count)
                .find_map(|ai| {
                    self.abc_tags[ai]
                        .abc
                        .instances
                        .iter()
                        .position(|inst| {
                            self.abc_tags[ai].abc.split_qname(inst.name).1 == "MainTimeline"
                        })
                        .map(|ci| (ai, ci))
                })
                .or_else(|| {
                    (total_classes == 1 && self.symbol_classes.is_empty()).then(|| {
                        (0..abc_count)
                            .find(|&ai| !self.abc_tags[ai].abc.classes.is_empty())
                            .map(|ai| (ai, 0))
                    })?
                })
        };

        // 4. Allocate Stage and root MovieClip (`"root1"`), then construct root and its Frame-0 children
        let stage_oid = self.avm1_objects.len();
        let mut stage_props = HashMap::new();
        stage_props.insert("name".to_owned(), Avm1Value::Null);
        self.avm1_objects.push(Avm1Object {
            props: stage_props,
            avm2_display: Some(Avm2DisplayMeta {
                display_type: "Stage".to_owned(),
                short_class_name: "Stage".to_owned(),
                on_stage: true,
                constructed: true,
                super_constructed: true,
                total_frames: 1,
                ..Default::default()
            }),
            ..Default::default()
        });
        self.avm2_stage_oid = Some(stage_oid);

        let root_short = doc_class
            .map(|(a, c)| {
                self.abc_tags[a]
                    .abc
                    .split_qname(self.abc_tags[a].abc.instances[c].name)
                    .1
            })
            .unwrap_or_else(|| "MovieClip".to_owned());
        let mut root_props = if let Some((da, dc)) = doc_class {
            self.collect_avm2_instance_slot_defaults(da, dc)
        } else {
            HashMap::new()
        };
        root_props.insert("name".to_owned(), Avm1Value::String("root1".to_owned()));
        root_props.insert("x".to_owned(), Avm1Value::Number(0.0));
        root_props.insert("y".to_owned(), Avm1Value::Number(0.0));
        root_props.insert("scaleX".to_owned(), Avm1Value::Number(1.0));
        root_props.insert("scaleY".to_owned(), Avm1Value::Number(1.0));
        root_props.insert("alpha".to_owned(), Avm1Value::Number(1.0));
        root_props.insert("visible".to_owned(), Avm1Value::Bool(true));
        root_props.insert("rotation".to_owned(), Avm1Value::Number(0.0));
        root_props.insert(
            "__display_target__".to_owned(),
            Avm1Value::MovieClipIdentity("_root".to_owned()),
        );
        let root_oid = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object {
            props: root_props,
            avm2_class: doc_class,
            avm2_display: Some(Avm2DisplayMeta {
                display_type: "MovieClip".to_owned(),
                short_class_name: root_short,
                character_id: 0,
                parent_oid: Some(stage_oid),
                on_stage: true,
                constructed: true,
                super_constructed: false,
                current_frame: 0,
                total_frames: self.root_timeline.total_frames.max(1),
                playing: true,
                ..Default::default()
            }),
            graphics: Some(RuntimeGraphicsState::default()),
            ..Default::default()
        });
        if let Some(st_meta) = self
            .avm1_objects
            .get_mut(stage_oid)
            .and_then(|o| o.avm2_display.as_mut())
        {
            st_meta.children_oids.push(root_oid);
        }
        self.avm2_root_oid = Some(root_oid);

        let mut steps = 0usize;
        if let Some((da, dc)) = doc_class {
            let iinit = self.abc_tags[da].abc.instances[dc].iinit;
            self.avm2_thrown = None;
            let _ = self.run_avm2_method_with_this(
                da,
                iinit,
                Avm1Value::Object(root_oid),
                Vec::new(),
                &mut steps,
                0,
            );
            self.avm2_thrown = None;
        }
        let need_children = self
            .avm1_objects
            .get(root_oid)
            .and_then(|o| o.avm2_display.as_ref())
            .is_some_and(|m| !m.super_constructed);
        if need_children {
            if let Some(meta) = self
                .avm1_objects
                .get_mut(root_oid)
                .and_then(|o| o.avm2_display.as_mut())
            {
                meta.super_constructed = true;
            }
            self.construct_avm2_children_for_container(root_oid, "_root", &mut steps, 0);
        }
        self.dispatch_avm2_event_on_oid(root_oid, "added", true, false, &mut steps, 0);
        self.dispatch_avm2_event_on_oid(root_oid, "addedToStage", false, false, &mut steps, 0);
    }

    pub(super) fn run_avm2_method_bounded(
        &mut self,
        abc_idx: usize,
        method_idx: usize,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        let this_val = self.avm2_root_oid.map_or_else(
            || Avm1Value::MovieClip("_root".to_owned()),
            Avm1Value::Object,
        );
        self.run_avm2_method_with_this(abc_idx, method_idx, this_val, args, steps, depth)
    }

    fn avm2_captured_class_scopes(
        &self,
        abc_idx: usize,
        class_idx: usize,
        include_class: bool,
    ) -> Vec<Avm1Value> {
        let key = format!("__avm2_cscope:{abc_idx}:{class_idx}");
        let mut scopes = if let Some(Avm1Value::Object(oid)) = self.avm2_globals.get(&key)
            && let Some(elems) = self
                .avm1_objects
                .get(*oid)
                .and_then(|o| o.array_elements.as_ref())
        {
            elems.clone()
        } else {
            vec![Avm1Value::MovieClip("_root".to_owned())]
        };
        if include_class {
            scopes.push(Avm1Value::MovieClip(format!(
                "__avm2_class:{abc_idx}:{class_idx}"
            )));
        }
        scopes
    }

    fn avm2_method_outer_scopes(
        &self,
        abc_idx: usize,
        method_idx: usize,
        this_val: &Avm1Value,
    ) -> Vec<Avm1Value> {
        if let Some(loaded) = self.abc_tags.get(abc_idx) {
            if loaded
                .abc
                .scripts
                .iter()
                .any(|&(init, _)| init == method_idx)
            {
                return Vec::new();
            }
            for (c_idx, cls) in loaded.abc.classes.iter().enumerate() {
                if cls.cinit == method_idx {
                    return self.avm2_captured_class_scopes(abc_idx, c_idx, false);
                }
                for tr in &cls.traits {
                    let matches_tr = matches!(
                        tr.data,
                        AbcTraitData::Method { method }
                            | AbcTraitData::Getter { method }
                            | AbcTraitData::Setter { method }
                            | AbcTraitData::Function { method }
                            if method == method_idx
                    );
                    if matches_tr {
                        return self.avm2_captured_class_scopes(abc_idx, c_idx, true);
                    }
                }
            }
            for (c_idx, inst) in loaded.abc.instances.iter().enumerate() {
                if inst.iinit == method_idx {
                    return self.avm2_captured_class_scopes(abc_idx, c_idx, true);
                }
                for tr in &inst.traits {
                    let matches_tr = matches!(
                        tr.data,
                        AbcTraitData::Method { method }
                            | AbcTraitData::Getter { method }
                            | AbcTraitData::Setter { method }
                            | AbcTraitData::Function { method }
                            if method == method_idx
                    );
                    if matches_tr {
                        return self.avm2_captured_class_scopes(abc_idx, c_idx, true);
                    }
                }
            }
        }
        if let Avm1Value::Object(oid) = this_val
            && let Some((c_abc, c_idx)) = self.avm1_objects.get(*oid).and_then(|o| o.avm2_class)
        {
            return self.avm2_captured_class_scopes(c_abc, c_idx, true);
        }
        vec![Avm1Value::MovieClip("_root".to_owned())]
    }

    fn avm2_class_is_display_object(&self, abc_idx: usize, class_idx: usize) -> bool {
        let mut cur = Some((abc_idx, class_idx));
        for _ in 0..32 {
            let Some((a, c)) = cur else { break };
            if let Some(inst) = self.abc_tags.get(a).and_then(|l| l.abc.instances.get(c)) {
                let (_, sname) = self.abc_tags[a].abc.split_qname(inst.super_name);
                if matches!(
                    sname.as_str(),
                    "MovieClip"
                        | "Sprite"
                        | "DisplayObjectContainer"
                        | "InteractiveObject"
                        | "DisplayObject"
                        | "SimpleButton"
                        | "Shape"
                        | "Bitmap"
                        | "TextField"
                        | "EventDispatcher"
                ) {
                    return true;
                }
            }
            cur = self.avm2_superclass(a, c);
        }
        false
    }

    fn is_avm2_builtin_display_member(prop: &str) -> bool {
        matches!(
            prop,
            "x" | "y"
                | "scaleX"
                | "scaleY"
                | "alpha"
                | "visible"
                | "rotation"
                | "name"
                | "width"
                | "height"
                | "stage"
                | "parent"
                | "root"
                | "currentFrame"
                | "totalFrames"
                | "numChildren"
                | "graphics"
                | "upState"
                | "overState"
                | "downState"
                | "hitTestState"
                | "stop"
                | "play"
                | "nextFrame"
                | "prevFrame"
                | "gotoAndStop"
                | "gotoAndPlay"
                | "addFrameScript"
                | "addChild"
                | "removeChild"
                | "getChildAt"
                | "getChildByName"
                | "getChildIndex"
                | "contains"
                | "addEventListener"
                | "removeEventListener"
                | "dispatchEvent"
                | "hasEventListener"
                | "willTrigger"
        )
    }

    fn avm2_mc_has_instance_trait(&self, mc: &str, prop: &str) -> bool {
        let target = self.canonical_movie_target(mc);
        let cid_opt = if target == "_root" {
            Some(0u16)
        } else {
            self.target_slot(&target).map(|s| s.character_id)
        };
        let Some(cid) = cid_opt else {
            return false;
        };
        let class_pos = if let Some(bound_cls) = self.symbol_classes.get(&cid) {
            let (bpkg, bshort) = match bound_cls.rsplit_once('.') {
                Some((p, s)) => (p, s),
                None => ("", bound_cls.as_str()),
            };
            (0..self.abc_tags.len()).find_map(|ai| self.find_avm2_class(ai, bpkg, bshort))
        } else if cid == 0 {
            self.abc_tags.iter().enumerate().find_map(|(ai, loaded)| {
                loaded
                    .abc
                    .instances
                    .iter()
                    .position(|inst| {
                        let (_, s) = loaded.abc.split_qname(inst.name);
                        s == "MainTimeline"
                    })
                    .map(|ci| (ai, ci))
            })
        } else {
            None
        };
        let mut cur = class_pos;
        for _ in 0..32 {
            let Some((a, c)) = cur else { break };
            if let Some(inst) = self.abc_tags.get(a).and_then(|l| l.abc.instances.get(c))
                && inst
                    .traits
                    .iter()
                    .any(|tr| self.abc_tags[a].abc.split_qname(tr.name).1 == prop)
            {
                return true;
            }
            cur = self.avm2_superclass(a, c);
        }
        false
    }

    fn avm2_scope_owns_prop(&self, sc: &Avm1Value, prop: &str) -> bool {
        match sc {
            Avm1Value::Object(oid) => {
                if self.avm2_object_has_prop(*oid, prop) {
                    return true;
                }
                if self
                    .avm1_objects
                    .get(*oid)
                    .is_some_and(|o| o.avm2_display.is_some())
                    && Self::is_avm2_builtin_display_member(prop)
                {
                    return true;
                }
                if let Some((c_abc, c_idx)) = self.avm1_objects.get(*oid).and_then(|o| o.avm2_class)
                    && self.avm2_class_is_display_object(c_abc, c_idx)
                    && Self::is_avm2_builtin_display_member(prop)
                {
                    return true;
                }
                false
            }
            Avm1Value::MovieClip(mc) => {
                if mc.starts_with("__avm2_class:") {
                    self.avm2_class_token_has_static_prop(mc, prop)
                } else if mc.starts_with("__avm2_type:") {
                    false
                } else {
                    self.avm2_has_property(sc, prop)
                        || self.avm2_mc_has_instance_trait(mc, prop)
                        || (mc != "_root" && Self::is_avm2_builtin_display_member(prop))
                        || (mc == "_root"
                            && matches!(
                                prop,
                                "x" | "y"
                                    | "scaleX"
                                    | "scaleY"
                                    | "alpha"
                                    | "visible"
                                    | "rotation"
                                    | "name"
                                    | "currentFrame"
                                    | "totalFrames"
                                    | "numChildren"
                                    | "graphics"
                            ))
                }
            }
            Avm1Value::MovieClipIdentity(mc) => {
                self.avm2_has_property(sc, prop)
                    || self.avm2_mc_has_instance_trait(mc, prop)
                    || Self::is_avm2_builtin_display_member(prop)
            }
            _ => false,
        }
    }

    pub(super) fn run_avm2_method_with_this(
        &mut self,
        abc_idx: usize,
        method_idx: usize,
        this_val: Avm1Value,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        let outer_scopes = self.avm2_method_outer_scopes(abc_idx, method_idx, &this_val);
        self.run_avm2_method_with_outer_scopes(
            abc_idx,
            method_idx,
            this_val,
            outer_scopes,
            args,
            steps,
            depth,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_avm2_method_with_outer_scopes(
        &mut self,
        abc_idx: usize,
        method_idx: usize,
        this_val: Avm1Value,
        outer_scopes: Vec<Avm1Value>,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if depth > MAX_CALL_DEPTH {
            self.record_unsupported_diagnostic(format!(
                "AVM2 call depth exceeded bounded limit ({MAX_CALL_DEPTH})"
            ));
            return Avm1Value::Undefined;
        }
        let Some((code, declared_locals, param_count, opt_defaults, exceptions)) =
            self.abc_tags.get(abc_idx).and_then(|l| {
                let body = l.abc.bodies_by_method.get(&method_idx)?;
                let (pcount, opts) = l.abc.methods.get(method_idx).map_or_else(
                    || (0, Vec::new()),
                    |m| {
                        let defs: Vec<Avm1Value> = m
                            .optional_values
                            .iter()
                            .map(|&(vidx, vkind)| abc_const_to_avm_value(&l.abc, vidx, vkind))
                            .collect();
                        (m.param_types.len(), defs)
                    },
                );
                Some((
                    body.code.clone(),
                    body.local_count,
                    pcount,
                    opts,
                    body.exceptions.clone(),
                ))
            })
        else {
            return Avm1Value::Undefined;
        };

        let local_slots = declared_locals
            .max(param_count + 1)
            .max(args.len() + 1)
            .clamp(64, 1024);
        let mut locals: Vec<Avm1Value> = vec![Avm1Value::Undefined; local_slots];
        locals[0] = this_val.clone();
        let given_len = args.len();
        for (i, arg) in args.into_iter().enumerate() {
            if i + 1 < locals.len() {
                locals[i + 1] = arg;
            }
        }
        if given_len < param_count && !opt_defaults.is_empty() {
            let first_opt = param_count.saturating_sub(opt_defaults.len());
            for i in given_len.max(first_opt)..param_count {
                if i + 1 < locals.len()
                    && let Some(def_val) = opt_defaults.get(i - first_opt)
                {
                    locals[i + 1] = def_val.clone();
                }
            }
        }

        let mut stack: Vec<Avm1Value> = Vec::new();
        let mut scope_stack: Vec<Avm1Value> = Vec::new();
        let mut r = AbcReader::new(&code);

        while r.pos < code.len() && *steps < MAX_VM_STEPS_PER_FRAME {
            *steps += 1;
            let insn_start = r.pos;
            let Some(op) = r.u8() else { break };
            match op {
                0x02 => {} // nop
                0x03 => {
                    // throw
                    let thrown = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.avm2_thrown = Some(thrown);
                }
                0x04 => {
                    // getsuper
                    let Some(mn_idx) = r.u30() else { break };
                    let prop_name = self.pop_avm2_multiname(abc_idx, mn_idx, &mut stack);
                    let receiver = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let val = self.get_avm2_super_property(
                        abc_idx, method_idx, &receiver, &prop_name, steps, depth,
                    );
                    self.push_avm_stack(&mut stack, val);
                }
                0x05 => {
                    // setsuper
                    let Some(mn_idx) = r.u30() else { break };
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let prop_name = self.pop_avm2_multiname(abc_idx, mn_idx, &mut stack);
                    let receiver = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.set_avm2_super_property(
                        abc_idx, method_idx, &receiver, &prop_name, val, steps, depth,
                    );
                }
                0x08 => {
                    // kill
                    let Some(reg) = r.u30() else { break };
                    if reg < locals.len() {
                        locals[reg] = Avm1Value::Undefined;
                    }
                }
                0x09 => {} // label
                0x0C..=0x0F => {
                    // ifnlt (0x0C), ifnle (0x0D), ifngt (0x0E), ifnge (0x0F)
                    let Some(rel) = r.s24() else { break };
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let cond = match op {
                        0x0C => avm2_less_than(&a, &b) != Some(true),
                        0x0D => avm2_less_than(&b, &a) != Some(false),
                        0x0E => avm2_less_than(&b, &a) != Some(true),
                        _ => avm2_less_than(&a, &b) != Some(false),
                    };
                    if cond {
                        let target = (r.pos as isize) + (rel as isize);
                        if target >= 0 && (target as usize) <= code.len() {
                            r.pos = target as usize;
                        }
                    }
                }
                0x10 => {
                    // jump
                    let Some(rel) = r.s24() else { break };
                    let target = (r.pos as isize) + (rel as isize);
                    if target >= 0 && (target as usize) <= code.len() {
                        r.pos = target as usize;
                    } else {
                        break;
                    }
                }
                0x11 => {
                    // iftrue
                    let Some(rel) = r.s24() else { break };
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined).to_bool(9);
                    if v {
                        let target = (r.pos as isize) + (rel as isize);
                        if target >= 0 && (target as usize) <= code.len() {
                            r.pos = target as usize;
                        }
                    }
                }
                0x12 => {
                    // iffalse
                    let Some(rel) = r.s24() else { break };
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined).to_bool(9);
                    if !v {
                        let target = (r.pos as isize) + (rel as isize);
                        if target >= 0 && (target as usize) <= code.len() {
                            r.pos = target as usize;
                        }
                    }
                }
                0x13..=0x1A => {
                    // ifeq (0x13), ifne (0x14), iflt (0x15), ifle (0x16), ifgt (0x17), ifge (0x18), ifstricteq (0x19), ifstrictne (0x1A)
                    let Some(rel) = r.s24() else { break };
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let cond = match op {
                        0x13 => self.avm2_equals(&a, &b),
                        0x14 => !self.avm2_equals(&a, &b),
                        0x15 => avm2_less_than(&a, &b) == Some(true),
                        0x16 => avm2_less_than(&b, &a) == Some(false),
                        0x17 => avm2_less_than(&b, &a) == Some(true),
                        0x18 => avm2_less_than(&a, &b) == Some(false),
                        0x19 => self.avm2_strict_equals(&a, &b),
                        _ => !self.avm2_strict_equals(&a, &b),
                    };
                    if cond {
                        let target = (r.pos as isize) + (rel as isize);
                        if target >= 0 && (target as usize) <= code.len() {
                            r.pos = target as usize;
                        }
                    }
                }
                0x1B => {
                    // lookupswitch (offsets relative to `insn_start`)
                    let Some(default_off) = r.s24() else { break };
                    let Some(max_idx) = r.u30() else { break };
                    let mut case_offs = Vec::with_capacity(max_idx.saturating_add(1).min(4096));
                    let mut ok = true;
                    for _ in 0..=max_idx.min(4096) {
                        if let Some(off) = r.s24() {
                            case_offs.push(off);
                        } else {
                            ok = false;
                            break;
                        }
                    }
                    if !ok {
                        break;
                    }
                    let idx = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let rel = if idx >= 0 && (idx as usize) < case_offs.len() {
                        case_offs[idx as usize]
                    } else {
                        default_off
                    };
                    let target = (insn_start as isize) + (rel as isize);
                    if target >= 0 && (target as usize) <= code.len() {
                        r.pos = target as usize;
                    } else {
                        break;
                    }
                }
                0x1C | 0x30 => {
                    // pushwith / pushscope
                    let sc = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if scope_stack.len() < 64 {
                        scope_stack.push(sc);
                    }
                }
                0x1D => {
                    // popscope
                    let _ = scope_stack.pop();
                }
                0x1E => {
                    // nextname
                    let idx_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let keys = self.avm2_enumerable_keys(&obj);
                    let k_idx = ecma_f64_to_i32(avm2_to_number(&idx_val))
                        .saturating_sub(1)
                        .max(0) as usize;
                    let val = keys
                        .get(k_idx)
                        .cloned()
                        .map_or(Avm1Value::Undefined, Avm1Value::String);
                    self.push_avm_stack(&mut stack, val);
                }
                0x1F => {
                    // hasnext
                    let idx_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let keys = self.avm2_enumerable_keys(&obj);
                    let cur_idx = ecma_f64_to_i32(avm2_to_number(&idx_val)).max(0) as usize;
                    let next = if cur_idx < keys.len() {
                        (cur_idx + 1) as f64
                    } else {
                        0.0
                    };
                    self.push_avm_stack(&mut stack, Avm1Value::Number(next));
                }
                0x20 => self.push_avm_stack(&mut stack, Avm1Value::Null),
                0x21 => self.push_avm_stack(&mut stack, Avm1Value::Undefined),
                0x23 => {
                    // nextvalue
                    let idx_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let keys = self.avm2_enumerable_keys(&obj);
                    let k_idx = ecma_f64_to_i32(avm2_to_number(&idx_val))
                        .saturating_sub(1)
                        .max(0) as usize;
                    let val = if let Some(k) = keys.get(k_idx) {
                        self.get_avm2_property_eval(&obj, k, steps, depth)
                    } else {
                        Avm1Value::Undefined
                    };
                    self.push_avm_stack(&mut stack, val);
                }
                0x24 => {
                    // pushbyte
                    let Some(b) = r.i8() else { break };
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(b)));
                }
                0x25 => {
                    // pushshort (signed s30 / i16)
                    let Some(v) = r.s32() else { break };
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(v as i16)));
                }
                0x26 => self.push_avm_stack(&mut stack, Avm1Value::Bool(true)),
                0x27 => self.push_avm_stack(&mut stack, Avm1Value::Bool(false)),
                0x28 => self.push_avm_stack(&mut stack, Avm1Value::Number(f64::NAN)),
                0x29 => {
                    let _ = stack.pop();
                }
                0x2A => {
                    let top = stack.last().cloned().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(&mut stack, top);
                }
                0x2B => {
                    let len = stack.len();
                    if len >= 2 {
                        stack.swap(len - 1, len - 2);
                    }
                }
                0x2C => {
                    // pushstring
                    let Some(idx) = r.u30() else { break };
                    let s = self.abc_tags[abc_idx].abc.string(idx).to_owned();
                    self.push_avm_stack(&mut stack, Avm1Value::String(s));
                }
                0x2D => {
                    // pushint
                    let Some(idx) = r.u30() else { break };
                    let v = self.abc_tags[abc_idx]
                        .abc
                        .ints
                        .get(idx)
                        .copied()
                        .unwrap_or(0);
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(v)));
                }
                0x2E => {
                    // pushuint
                    let Some(idx) = r.u30() else { break };
                    let v = self.abc_tags[abc_idx]
                        .abc
                        .uints
                        .get(idx)
                        .copied()
                        .unwrap_or(0);
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(v)));
                }
                0x2F => {
                    // pushdouble
                    let Some(idx) = r.u30() else { break };
                    let v = self.abc_tags[abc_idx]
                        .abc
                        .doubles
                        .get(idx)
                        .copied()
                        .unwrap_or(f64::NAN);
                    self.push_avm_stack(&mut stack, Avm1Value::Number(v));
                }
                0x32 => {
                    // hasnext2
                    let Some(obj_reg) = r.u30() else { break };
                    let Some(idx_reg) = r.u30() else { break };
                    let obj = locals.get(obj_reg).cloned().unwrap_or(Avm1Value::Undefined);
                    let cur_idx = locals
                        .get(idx_reg)
                        .map_or(0, |v| ecma_f64_to_i32(avm2_to_number(v)).max(0) as usize);
                    let keys = self.avm2_enumerable_keys(&obj);
                    if cur_idx < keys.len() {
                        if let Some(slot) = locals.get_mut(idx_reg) {
                            *slot = Avm1Value::Number((cur_idx + 1) as f64);
                        }
                        self.push_avm_stack(&mut stack, Avm1Value::Bool(true));
                    } else {
                        if let Some(slot) = locals.get_mut(obj_reg) {
                            *slot = Avm1Value::Null;
                        }
                        if let Some(slot) = locals.get_mut(idx_reg) {
                            *slot = Avm1Value::Number(0.0);
                        }
                        self.push_avm_stack(&mut stack, Avm1Value::Bool(false));
                    }
                }
                0x40 => {
                    // newfunction
                    let Some(midx) = r.u30() else { break };
                    let mut captured = outer_scopes.clone();
                    captured.extend(scope_stack.iter().cloned());
                    let cid = self.avm2_closures.len();
                    self.avm2_closures.push(Avm2Closure {
                        abc_idx,
                        method_idx: midx,
                        outer_scopes: captured,
                    });
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::String(format!("__avm2_closure:{cid}:{abc_idx}:{midx}")),
                    );
                }
                0x41 => {
                    // call
                    let Some(argc) = r.u30() else { break };
                    let mut cargs = Vec::with_capacity(argc.min(64));
                    for _ in 0..argc {
                        cargs.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    cargs.reverse();
                    let receiver = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let fn_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let ret =
                        self.invoke_avm2_callable_with_this(&fn_val, receiver, cargs, steps, depth);
                    self.push_avm_stack(&mut stack, ret);
                }
                0x42 => {
                    // construct
                    let Some(argc) = r.u30() else { break };
                    let mut cargs = Vec::with_capacity(argc.min(64));
                    for _ in 0..argc {
                        cargs.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    cargs.reverse();
                    let ctor = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let constructed =
                        self.construct_avm2_value(abc_idx, &ctor, cargs, steps, depth);
                    self.push_avm_stack(&mut stack, constructed);
                }
                0x45 | 0x4E => {
                    // callsuper (0x45), callsupervoid (0x4E)
                    let Some(mn_idx) = r.u30() else { break };
                    let Some(argc) = r.u30() else { break };
                    let mut cargs = Vec::with_capacity(argc.min(64));
                    for _ in 0..argc {
                        cargs.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    cargs.reverse();
                    let prop_name = self.pop_avm2_multiname(abc_idx, mn_idx, &mut stack);
                    let receiver = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let ret = self.call_avm2_super_method(
                        abc_idx, method_idx, &receiver, &prop_name, cargs, steps, depth,
                    );
                    if op == 0x45 {
                        self.push_avm_stack(&mut stack, ret);
                    }
                }
                0x46 | 0x4C | 0x4F => {
                    // callproperty (0x46), callproplex (0x4C), callpropvoid (0x4F)
                    let Some(mn_idx) = r.u30() else { break };
                    let Some(argc) = r.u30() else { break };
                    let mut cargs = Vec::with_capacity(argc.min(64));
                    for _ in 0..argc {
                        cargs.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    cargs.reverse();
                    let prop_name = self.pop_avm2_multiname(abc_idx, mn_idx, &mut stack);
                    let receiver = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let ret = self
                        .call_avm2_property(abc_idx, &receiver, &prop_name, cargs, steps, depth);
                    if op != 0x4F {
                        self.push_avm_stack(&mut stack, ret);
                    }
                }
                0x47 => {
                    // returnvoid
                    return Avm1Value::Undefined;
                }
                0x48 => {
                    // returnvalue
                    return stack.pop().unwrap_or(Avm1Value::Undefined);
                }
                0x49 => {
                    // constructsuper
                    let Some(argc) = r.u30() else { break };
                    let mut cargs = Vec::with_capacity(argc.min(64));
                    for _ in 0..argc {
                        cargs.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    cargs.reverse();
                    let receiver = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if let Avm1Value::Object(oid) = receiver {
                        if let Some((owner_abc, owner_cls)) =
                            self.resolve_avm2_method_owner(abc_idx, method_idx, oid)
                        {
                            if let Some((sup_abc, sup_cls)) =
                                self.avm2_superclass(owner_abc, owner_cls)
                            {
                                let sup_iinit = self.abc_tags[sup_abc].abc.instances[sup_cls].iinit;
                                let _ = self.run_avm2_method_with_this(
                                    sup_abc,
                                    sup_iinit,
                                    Avm1Value::Object(oid),
                                    cargs,
                                    steps,
                                    depth + 1,
                                );
                            } else {
                                let inst = &self.abc_tags[owner_abc].abc.instances[owner_cls];
                                let (_, super_short) =
                                    self.abc_tags[owner_abc].abc.split_qname(inst.super_name);
                                let (_, cls_short) =
                                    self.abc_tags[owner_abc].abc.split_qname(inst.name);
                                if super_short == "Error" || super_short.ends_with("Error") {
                                    let msg = cargs
                                        .first()
                                        .filter(|v| {
                                            !matches!(v, Avm1Value::Undefined | Avm1Value::Null)
                                        })
                                        .map(|v| v.to_trace_string(9, &self.avm1_objects))
                                        .unwrap_or_default();
                                    let err_id =
                                        cargs.get(1).map(|v| v.to_number() as i32).unwrap_or(0);
                                    if let Some(obj) = self.avm1_objects.get_mut(oid) {
                                        obj.props.insert("message".into(), Avm1Value::String(msg));
                                        obj.props.insert(
                                            "errorID".into(),
                                            Avm1Value::Number(f64::from(err_id)),
                                        );
                                        obj.props
                                            .entry("name".into())
                                            .or_insert_with(|| Avm1Value::String(cls_short));
                                    }
                                } else {
                                    self.run_avm2_builtin_display_super(oid, steps, depth + 1);
                                }
                            }
                        } else {
                            self.run_avm2_builtin_display_super(oid, steps, depth + 1);
                        }
                    } else if matches!(&receiver, Avm1Value::MovieClip(mc) if mc == "_root")
                        && let Some(owner_cls) = self.abc_tags.get(abc_idx).and_then(|l| {
                            l.abc
                                .instances
                                .iter()
                                .position(|inst| inst.iinit == method_idx)
                        })
                        && let Some((sup_abc, sup_cls)) = self.avm2_superclass(abc_idx, owner_cls)
                    {
                        let sup_iinit = self.abc_tags[sup_abc].abc.instances[sup_cls].iinit;
                        let _ = self.run_avm2_method_with_this(
                            sup_abc,
                            sup_iinit,
                            receiver,
                            cargs,
                            steps,
                            depth + 1,
                        );
                    }
                }
                0x4A => {
                    // constructprop
                    let Some(mn_idx) = r.u30() else { break };
                    let Some(argc) = r.u30() else { break };
                    let mut cargs = Vec::with_capacity(argc.min(64));
                    for _ in 0..argc {
                        cargs.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    cargs.reverse();
                    let (pkg, cls_name) = self.pop_avm2_qname(abc_idx, mn_idx, &mut stack);
                    let _receiver = stack.pop();
                    let constructed =
                        self.construct_avm2_named(abc_idx, &pkg, &cls_name, cargs, steps, depth);
                    self.push_avm_stack(&mut stack, constructed);
                }
                0x55 => {
                    // newobject
                    let Some(count) = r.u30() else { break };
                    let mut props = HashMap::new();
                    for _ in 0..count {
                        let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                        let key = stack
                            .pop()
                            .unwrap_or(Avm1Value::Undefined)
                            .to_trace_string(9, &self.avm1_objects);
                        props.insert(key, val);
                    }
                    let obj_id = self.avm1_objects.len();
                    self.avm1_objects.push(Avm1Object {
                        props,
                        ..Default::default()
                    });
                    self.push_avm_stack(&mut stack, Avm1Value::Object(obj_id));
                }
                0x56 => {
                    // newarray
                    let Some(count) = r.u30() else { break };
                    let mut elems = Vec::with_capacity(count.min(1024));
                    for _ in 0..count {
                        elems.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    elems.reverse();
                    let obj_id = self.avm1_objects.len();
                    self.avm1_objects.push(Avm1Object {
                        array_elements: Some(elems),
                        ..Default::default()
                    });
                    self.push_avm_stack(&mut stack, Avm1Value::Object(obj_id));
                }
                0x57 => {
                    // newactivation
                    let mut act_props = HashMap::new();
                    if let Some(loaded) = self.abc_tags.get(abc_idx)
                        && let Some(body) = loaded.abc.bodies_by_method.get(&method_idx)
                    {
                        for tr in &body.traits {
                            if let AbcTraitData::Slot {
                                type_name,
                                vindex,
                                vkind,
                                ..
                            } = tr.data
                            {
                                let (_, sname) = loaded.abc.split_qname(tr.name);
                                if sname.is_empty() {
                                    continue;
                                }
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
                                act_props.insert(sname, init_val);
                            }
                        }
                    }
                    let obj_id = self.avm1_objects.len();
                    self.avm1_objects.push(Avm1Object {
                        props: act_props,
                        avm2_activation_method: Some((abc_idx, method_idx)),
                        ..Default::default()
                    });
                    self.push_avm_stack(&mut stack, Avm1Value::Object(obj_id));
                }
                0x58 => {
                    // newclass — capture the active outer + local scope chain for cinit & class methods
                    let Some(class_idx) = r.u30() else { break };
                    let _base = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let mut captured_scopes = outer_scopes.clone();
                    captured_scopes.extend(scope_stack.iter().cloned());
                    let scope_obj_id = self.avm1_objects.len();
                    self.avm1_objects.push(Avm1Object {
                        array_elements: Some(captured_scopes),
                        ..Default::default()
                    });
                    self.avm2_globals.insert(
                        format!("__avm2_cscope:{abc_idx}:{class_idx}"),
                        Avm1Value::Object(scope_obj_id),
                    );
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::MovieClip(format!("__avm2_class:{abc_idx}:{class_idx}")),
                    );
                }
                0x5A => {
                    // newcatch
                    let Some(exc_idx) = r.u30() else { break };
                    let catch_var = exceptions.get(exc_idx).map(|exc| {
                        let (_, vname) = self.abc_tags[abc_idx].abc.split_qname(exc.var_name);
                        (vname, abc_idx, exc.exc_type)
                    });
                    let obj_id = self.avm1_objects.len();
                    self.avm1_objects.push(Avm1Object {
                        avm2_catch_var: catch_var,
                        ..Default::default()
                    });
                    self.push_avm_stack(&mut stack, Avm1Value::Object(obj_id));
                }
                0x5B | 0x5C => {
                    // findpropglobalstrict / findpropglobal
                    let Some(mn_idx) = r.u30() else { break };
                    let name = self.pop_avm2_multiname(abc_idx, mn_idx, &mut stack);
                    self.push_avm_stack(&mut stack, Avm1Value::MovieClip(name));
                }
                0x5D | 0x5E => {
                    // findpropstrict / findproperty
                    let Some(mn_idx) = r.u30() else { break };
                    let name = self.pop_avm2_multiname(abc_idx, mn_idx, &mut stack);
                    let mut resolved = None;
                    for sc in scope_stack.iter().rev().chain(outer_scopes.iter().rev()) {
                        if self.avm2_scope_owns_prop(sc, &name) {
                            resolved = Some(sc.clone());
                            break;
                        }
                    }
                    if resolved.is_none() && self.avm2_scope_owns_prop(&this_val, &name) {
                        resolved = Some(this_val.clone());
                    }
                    if resolved.is_none()
                        && let Avm1Value::Object(oid) = &this_val
                        && let Some((ca, cc)) =
                            self.avm1_objects.get(*oid).and_then(|o| o.avm2_class)
                    {
                        let cls_tok = format!("__avm2_class:{ca}:{cc}");
                        if self.avm2_class_token_has_static_prop(&cls_tok, &name) {
                            resolved = Some(Avm1Value::MovieClip(cls_tok));
                        }
                    }
                    if resolved.is_none()
                        && op == 0x5E
                        && let Some(Avm1Value::Object(oid)) = scope_stack.first()
                        && self
                            .avm1_objects
                            .get(*oid)
                            .is_some_and(|o| o.avm2_class.is_some())
                    {
                        resolved = Some(Avm1Value::Object(*oid));
                    }
                    self.push_avm_stack(&mut stack, resolved.unwrap_or(Avm1Value::MovieClip(name)));
                }
                0x60 => {
                    // getlex
                    let Some(mn_idx) = r.u30() else { break };
                    let (pkg, name) = self.abc_tags[abc_idx].abc.split_qname(mn_idx);
                    let mut val_opt = None;
                    for sc in scope_stack.iter().rev().chain(outer_scopes.iter().rev()) {
                        if self.avm2_scope_owns_prop(sc, &name) {
                            val_opt = Some(self.get_avm2_property_eval(sc, &name, steps, depth));
                            break;
                        }
                    }
                    if val_opt.is_none() && self.avm2_scope_owns_prop(&this_val, &name) {
                        val_opt = Some(self.get_avm2_property_eval(&this_val, &name, steps, depth));
                    }
                    if val_opt.is_none()
                        && let Avm1Value::Object(oid) = &this_val
                        && let Some((ca, cc)) =
                            self.avm1_objects.get(*oid).and_then(|o| o.avm2_class)
                    {
                        let cls_tok = format!("__avm2_class:{ca}:{cc}");
                        if self.avm2_class_token_has_static_prop(&cls_tok, &name) {
                            val_opt = Some(self.get_avm2_property_eval(
                                &Avm1Value::MovieClip(cls_tok),
                                &name,
                                steps,
                                depth,
                            ));
                        }
                    }
                    let val = if let Some(v) = val_opt {
                        v
                    } else if let Some((c_abc, c_idx)) = self.find_avm2_class(abc_idx, &pkg, &name)
                    {
                        Avm1Value::MovieClip(format!("__avm2_class:{c_abc}:{c_idx}"))
                    } else if matches!(
                        name.as_str(),
                        "Object"
                            | "Array"
                            | "String"
                            | "Number"
                            | "int"
                            | "uint"
                            | "Boolean"
                            | "Function"
                            | "MovieClip"
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
                            | "MouseEvent"
                            | "KeyboardEvent"
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
                        Avm1Value::MovieClip(format!("__avm2_type:{name}"))
                    } else if name == "NaN" {
                        Avm1Value::Number(f64::NAN)
                    } else if name == "Infinity" {
                        Avm1Value::Number(f64::INFINITY)
                    } else if pkg.is_empty() && name == "undefined" {
                        Avm1Value::Undefined
                    } else {
                        self.get_avm2_property_eval(
                            &Avm1Value::MovieClip("_root".into()),
                            &name,
                            steps,
                            depth,
                        )
                    };
                    self.push_avm_stack(&mut stack, val);
                }
                0x61 | 0x68 => {
                    // setproperty / initproperty
                    let Some(mn_idx) = r.u30() else { break };
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let prop_name = self.pop_avm2_multiname(abc_idx, mn_idx, &mut stack);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.set_avm2_property_eval(&obj, &prop_name, val, steps, depth);
                }
                0x62 => {
                    // getlocal
                    let Some(reg) = r.u30() else { break };
                    let val = locals.get(reg).cloned().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(&mut stack, val);
                }
                0x63 => {
                    // setlocal
                    let Some(reg) = r.u30() else { break };
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if reg < locals.len() {
                        locals[reg] = val;
                    }
                }
                0x64 => {
                    // getglobalscope
                    let gs = outer_scopes
                        .first()
                        .or_else(|| scope_stack.first())
                        .cloned()
                        .unwrap_or_else(|| Avm1Value::MovieClip("_root".into()));
                    self.push_avm_stack(&mut stack, gs);
                }
                0x65 => {
                    // getscopeobject
                    let Some(idx) = r.u8() else { break };
                    let sc = scope_stack
                        .get(usize::from(idx))
                        .cloned()
                        .unwrap_or_else(|| Avm1Value::MovieClip("_root".into()));
                    self.push_avm_stack(&mut stack, sc);
                }
                0x66 => {
                    // getproperty
                    let Some(mn_idx) = r.u30() else { break };
                    let prop_name = self.pop_avm2_multiname(abc_idx, mn_idx, &mut stack);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let val = self.get_avm2_property_eval(&obj, &prop_name, steps, depth);
                    self.push_avm_stack(&mut stack, val);
                }
                0x67 => {
                    // getouterscope
                    let Some(idx) = r.u30() else { break };
                    let sc = outer_scopes
                        .get(idx)
                        .cloned()
                        .unwrap_or_else(|| Avm1Value::MovieClip("_root".into()));
                    self.push_avm_stack(&mut stack, sc);
                }
                0x6A => {
                    // deleteproperty
                    let Some(mn_idx) = r.u30() else { break };
                    let prop_name = self.pop_avm2_multiname(abc_idx, mn_idx, &mut stack);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let deleted = self.delete_avm2_property(&obj, &prop_name);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(deleted));
                }
                0x6C => {
                    // getslot
                    let Some(slot_idx) = r.u30() else { break };
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let (key, _) = self.resolve_avm2_activation_or_slot_key(&obj, slot_idx);
                    let val = self.get_avm2_property_eval(&obj, &key, steps, depth);
                    self.push_avm_stack(&mut stack, val);
                }
                0x6D => {
                    // setslot
                    let Some(slot_idx) = r.u30() else { break };
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let (key, coerce_ty) = self.resolve_avm2_activation_or_slot_key(&obj, slot_idx);
                    let coerced = if let Some((t_abc, t_mn)) = coerce_ty
                        && t_mn != 0
                    {
                        self.avm2_coerce_multiname(t_abc, t_mn, val)
                    } else {
                        val
                    };
                    self.set_avm2_property_eval(&obj, &key, coerced, steps, depth);
                }
                0x6E => {
                    // getglobalslot
                    let Some(slot_idx) = r.u30() else { break };
                    let key = format!("__gslot_{slot_idx}");
                    let val = self
                        .avm2_globals
                        .get(&key)
                        .cloned()
                        .unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(&mut stack, val);
                }
                0x6F => {
                    // setglobalslot
                    let Some(slot_idx) = r.u30() else { break };
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let key = format!("__gslot_{slot_idx}");
                    self.avm2_globals.insert(key, val);
                }
                0x70 => {
                    // convert_s: null -> "null", undefined -> "undefined"
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let s = avm2_convert_string(&v, &self.avm1_objects);
                    self.push_avm_stack(&mut stack, Avm1Value::String(s));
                }
                0x73 | 0x83 => {
                    // convert_i / coerce_i
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let i = ecma_f64_to_i32(avm2_to_number(&v));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(i)));
                }
                0x74 | 0x88 => {
                    // convert_u / coerce_u
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let u = ecma_f64_to_u32(avm2_to_number(&v));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(u)));
                }
                0x75 | 0x84 => {
                    // convert_d / coerce_d
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(&mut stack, Avm1Value::Number(avm2_to_number(&v)));
                }
                0x76 | 0x81 => {
                    // convert_b / coerce_b
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_bool(9);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(b));
                }
                0x77 => {
                    // convert_o
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if matches!(v, Avm1Value::Undefined | Avm1Value::Null) {
                        self.record_unsupported_diagnostic(format!(
                            "AVM2 convert_o on null/undefined in abc#{abc_idx} method#{method_idx} offset=0x{insn_start:x}"
                        ));
                        self.push_avm_stack(&mut stack, Avm1Value::Null);
                    } else {
                        self.push_avm_stack(&mut stack, v);
                    }
                }
                0x80 => {
                    // coerce(multiname)
                    let Some(mn_idx) = r.u30() else { break };
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let out = self.avm2_coerce_multiname(abc_idx, mn_idx, v);
                    self.push_avm_stack(&mut stack, out);
                }
                0x82 => {
                    // coerce_a (any `*`: preserve exact value including undefined/null)
                }
                0x85 => {
                    // coerce_s: null/undefined -> null; otherwise convert to String
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let out = match v {
                        Avm1Value::Undefined | Avm1Value::Null => Avm1Value::Null,
                        other => Avm1Value::String(avm2_convert_string(&other, &self.avm1_objects)),
                    };
                    self.push_avm_stack(&mut stack, out);
                }
                0x86 => {
                    // astype(multiname)
                    let Some(mn_idx) = r.u30() else { break };
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let (pkg, tname) = self.abc_tags[abc_idx].abc.split_qname(mn_idx);
                    let out = if self.avm2_is_type_name(abc_idx, &v, &pkg, &tname) {
                        v
                    } else {
                        Avm1Value::Null
                    };
                    self.push_avm_stack(&mut stack, out);
                }
                0x87 => {
                    // astypelate
                    let type_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let out = if self.avm2_is_type_value(abc_idx, &v, &type_val) {
                        v
                    } else {
                        Avm1Value::Null
                    };
                    self.push_avm_stack(&mut stack, out);
                }
                0x89 => {
                    // coerce_o: undefined -> null, else preserve
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let out = if matches!(v, Avm1Value::Undefined) {
                        Avm1Value::Null
                    } else {
                        v
                    };
                    self.push_avm_stack(&mut stack, out);
                }
                0x90 => {
                    // negate
                    let n = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(-n));
                }
                0xC4 => {
                    // negate_i
                    let n = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(n.wrapping_neg())));
                }
                0x91 => {
                    // increment
                    let n = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(n + 1.0));
                }
                0xC0 => {
                    // increment_i
                    let n = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Number(f64::from(n.wrapping_add(1))),
                    );
                }
                0x92 => {
                    // inclocal
                    let Some(reg) = r.u30() else { break };
                    if let Some(slot) = locals.get_mut(reg) {
                        *slot = Avm1Value::Number(avm2_to_number(slot) + 1.0);
                    }
                }
                0xC2 => {
                    // inclocal_i
                    let Some(reg) = r.u30() else { break };
                    if let Some(slot) = locals.get_mut(reg) {
                        let next = ecma_f64_to_i32(avm2_to_number(slot)).wrapping_add(1);
                        *slot = Avm1Value::Number(f64::from(next));
                    }
                }
                0x93 => {
                    // decrement
                    let n = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(n - 1.0));
                }
                0xC1 => {
                    // decrement_i
                    let n = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Number(f64::from(n.wrapping_sub(1))),
                    );
                }
                0x94 => {
                    // declocal
                    let Some(reg) = r.u30() else { break };
                    if let Some(slot) = locals.get_mut(reg) {
                        *slot = Avm1Value::Number(avm2_to_number(slot) - 1.0);
                    }
                }
                0xC3 => {
                    // declocal_i
                    let Some(reg) = r.u30() else { break };
                    if let Some(slot) = locals.get_mut(reg) {
                        let next = ecma_f64_to_i32(avm2_to_number(slot)).wrapping_sub(1);
                        *slot = Avm1Value::Number(f64::from(next));
                    }
                }
                0x95 => {
                    // typeof
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let ty = self.avm2_typeof(&v);
                    self.push_avm_stack(&mut stack, Avm1Value::String(ty.to_owned()));
                }
                0x96 => {
                    // not
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_bool(9);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(!b));
                }
                0x97 => {
                    // bitnot
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let i = ecma_f64_to_i32(avm2_to_number(&v));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(!i)));
                }
                0xA0 => {
                    // add
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = self.avm2_add_operand(a, abc_idx, steps, depth);
                    let b = if self.avm2_thrown.is_none() {
                        self.avm2_add_operand(b, abc_idx, steps, depth)
                    } else {
                        Avm1Value::Undefined
                    };
                    if matches!(a, Avm1Value::String(_)) || matches!(b, Avm1Value::String(_)) {
                        let sa = avm2_convert_string(&a, &self.avm1_objects);
                        let sb = avm2_convert_string(&b, &self.avm1_objects);
                        self.push_avm_stack(&mut stack, Avm1Value::String(format!("{sa}{sb}")));
                    } else {
                        self.push_avm_stack(
                            &mut stack,
                            Avm1Value::Number(avm2_to_number(&a) + avm2_to_number(&b)),
                        );
                    }
                }
                0xA1 => {
                    // subtract
                    let b = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    let a = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(a - b));
                }
                0xA2 => {
                    // multiply
                    let b = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    let a = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(a * b));
                }
                0xA3 => {
                    // divide
                    let b = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    let a = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(a / b));
                }
                0xA4 => {
                    // modulo
                    let b = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    let a = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(a % b));
                }
                0xA5 => {
                    let b = ecma_f64_to_u32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Number(f64::from(a.wrapping_shl(b))),
                    );
                }
                0xA6 => {
                    let b = ecma_f64_to_u32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Number(f64::from(a.wrapping_shr(b))),
                    );
                }
                0xA7 => {
                    let b = ecma_f64_to_u32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_u32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Number(f64::from(a.wrapping_shr(b))),
                    );
                }
                0xA8 => {
                    let b = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(a & b)));
                }
                0xA9 => {
                    let b = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(a | b)));
                }
                0xAA => {
                    let b = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(a ^ b)));
                }
                0xAB => {
                    // equals (ECMA-357 / AS3 abstract equality)
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let eq = self.avm2_equals(&a, &b);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(eq));
                }
                0xAC => {
                    // strictequals (AS3 strict equality)
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let eq = self.avm2_strict_equals(&a, &b);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(eq));
                }
                0xAD => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Bool(avm2_less_than(&a, &b) == Some(true)),
                    );
                }
                0xAE => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Bool(avm2_less_than(&b, &a) == Some(false)),
                    );
                }
                0xAF => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Bool(avm2_less_than(&b, &a) == Some(true)),
                    );
                }
                0xB0 => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Bool(avm2_less_than(&a, &b) == Some(false)),
                    );
                }
                0xB1 => {
                    // instanceof
                    let type_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let res = self.avm2_instanceof_value(abc_idx, &v, &type_val);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(res));
                }
                0xB2 => {
                    // istype(multiname)
                    let Some(mn_idx) = r.u30() else { break };
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let (pkg, tname) = self.abc_tags[abc_idx].abc.split_qname(mn_idx);
                    let is_t = self.avm2_is_type_name(abc_idx, &v, &pkg, &tname);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(is_t));
                }
                0xB3 => {
                    // istypelate
                    let type_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let is_t = self.avm2_is_type_value(abc_idx, &v, &type_val);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(is_t));
                }
                0xB4 => {
                    // in
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let key = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(9, &self.avm1_objects);
                    let has = self.avm2_has_property(&obj, &key);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(has));
                }
                0xC5 => {
                    // add_i
                    let b = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Number(f64::from(a.wrapping_add(b))),
                    );
                }
                0xC6 => {
                    // subtract_i
                    let b = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Number(f64::from(a.wrapping_sub(b))),
                    );
                }
                0xC7 => {
                    // multiply_i
                    let b = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Number(f64::from(a.wrapping_mul(b))),
                    );
                }
                0xD0..=0xD3 => {
                    let idx = usize::from(op - 0xD0);
                    let val = locals.get(idx).cloned().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(&mut stack, val);
                }
                0xD4..=0xD7 => {
                    let idx = usize::from(op - 0xD4);
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if idx < locals.len() {
                        locals[idx] = val;
                    }
                }
                0xEF => {
                    // debug
                    let _ = r.u8();
                    let _ = r.u30();
                    let _ = r.u8();
                    let _ = r.u30();
                }
                0xF0 | 0xF1 => {
                    // debugline / debugfile
                    let _ = r.u30();
                }
                _ => {
                    self.record_unsupported_diagnostic(format!(
                        "Unsupported AVM2 ABC opcode 0x{op:02X} in abc#{abc_idx} method#{method_idx} offset=0x{insn_start:x}"
                    ));
                    break;
                }
            }
            if let Some(thrown) = self.avm2_thrown.take() {
                if let Some(exc) = exceptions.iter().find(|e| {
                    insn_start >= e.from
                        && insn_start < e.to
                        && e.target <= code.len()
                        && self.avm2_matches_exception(abc_idx, &thrown, e.exc_type)
                }) {
                    stack.clear();
                    self.push_avm_stack(&mut stack, thrown);
                    scope_stack.clear();
                    r.pos = exc.target;
                    continue;
                }
                self.avm2_thrown = Some(thrown);
                return Avm1Value::Undefined;
            }
        }
        if *steps >= MAX_VM_STEPS_PER_FRAME {
            self.record_unsupported_diagnostic(format!(
                "AVM2 execution reached bounded per-frame step limit ({MAX_VM_STEPS_PER_FRAME})"
            ));
        }
        Avm1Value::Undefined
    }

    pub(super) fn pop_avm2_multiname(
        &self,
        abc_idx: usize,
        mn_idx: usize,
        stack: &mut Vec<Avm1Value>,
    ) -> String {
        let (has_rt_ns, has_rt_name) = self.abc_tags[abc_idx].abc.runtime_multiname_parts(mn_idx);
        let prop_name = if has_rt_name {
            avm2_convert_string(
                &stack.pop().unwrap_or(Avm1Value::Undefined),
                &self.avm1_objects,
            )
        } else {
            self.abc_tags[abc_idx].abc.split_qname(mn_idx).1
        };
        if has_rt_ns {
            let _ = stack.pop();
        }
        prop_name
    }

    pub(super) fn pop_avm2_qname(
        &self,
        abc_idx: usize,
        mn_idx: usize,
        stack: &mut Vec<Avm1Value>,
    ) -> (String, String) {
        let (has_rt_ns, has_rt_name) = self.abc_tags[abc_idx].abc.runtime_multiname_parts(mn_idx);
        let (pkg, short) = if has_rt_name {
            let n = avm2_convert_string(
                &stack.pop().unwrap_or(Avm1Value::Undefined),
                &self.avm1_objects,
            );
            (String::new(), n)
        } else {
            self.abc_tags[abc_idx].abc.split_qname(mn_idx)
        };
        if has_rt_ns {
            let _ = stack.pop();
        }
        (pkg, short)
    }
}
