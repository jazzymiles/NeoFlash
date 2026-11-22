use super::*;

enum Avm1RangeOutcome {
    NormalEnd,
    JumpedOut(usize),
    Returned(Avm1Value),
    Thrown,
}

impl SwfBytecodePlayer {
    // =========================================================================
    // AVM1 Bytecode Interpreter
    // =========================================================================

    pub(super) fn execute_avm1_slice(&mut self, code: &[u8]) {
        let mut stack: Vec<Avm1Value> = Vec::new();
        let mut locals: HashMap<String, Avm1Value> = HashMap::new();
        let mut steps = 0usize;
        self.avm1_thrown = None;
        let _ = self.run_avm1_code(code, &mut stack, &mut locals, &mut steps, 0);
        if let Some(uncaught) = self.avm1_thrown.take() {
            let desc = uncaught.to_trace_string(self.swf_version, &self.avm1_objects);
            self.record_unsupported_diagnostic(format!("Uncaught AVM1 exception: {desc}"));
        }
    }

    pub(super) fn execute_avm1_for_target(&mut self, target: &str, code: &[u8]) {
        let saved_target = std::mem::replace(&mut self.avm1_target, target.to_owned());
        self.execute_avm1_slice(code);
        self.avm1_target = saved_target;
    }

    pub(super) fn execute_avm1_callback_for_target(&mut self, target: &str, fn_obj_id: usize) {
        let Some(func) = self
            .avm1_objects
            .get(fn_obj_id)
            .and_then(|o| o.function.clone())
        else {
            return;
        };
        let saved_target = std::mem::replace(&mut self.avm1_target, target.to_owned());
        let this_val = Avm1Value::MovieClipIdentity(self.canonical_movie_target(target));
        let mut steps = 0usize;
        self.avm1_thrown = None;
        let _ = self.invoke_avm1_func_with_this(&func, this_val, None, Vec::new(), &mut steps, 0);
        self.avm1_thrown = None;
        self.avm1_target = saved_target;
    }

    pub(super) fn execute_avm1_work(&mut self, work: PendingAvm1Work) {
        match work {
            PendingAvm1Work::Action { target, actions } => {
                self.execute_avm1_for_target(&target, &actions);
            }
            PendingAvm1Work::Callback { target, fn_obj_id } => {
                self.execute_avm1_callback_for_target(&target, fn_obj_id);
            }
            PendingAvm1Work::Detached {
                root_target,
                root_slot,
                actions,
            } => {
                debug_assert!(self.avm1_detached_target.is_none());
                self.avm1_detached_target = Some((root_target, *root_slot));
                for item in actions {
                    match item {
                        DetachedAvm1Item::Bytes(target, code) => {
                            self.execute_avm1_for_target(&target, &code);
                        }
                        DetachedAvm1Item::Callback(target, fn_obj_id) => {
                            self.execute_avm1_callback_for_target(&target, fn_obj_id);
                        }
                    }
                }
                self.avm1_detached_target = None;
            }
        }
    }

    pub(super) fn execute_avm1_work_list(&mut self, work: Vec<PendingAvm1Work>) {
        for item in work {
            self.execute_avm1_work(item);
        }
    }

    pub(super) fn run_avm1_code(
        &mut self,
        code: &[u8],
        stack: &mut Vec<Avm1Value>,
        locals: &mut HashMap<String, Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Option<Avm1Value> {
        if depth > MAX_CALL_DEPTH {
            self.record_unsupported_diagnostic(format!(
                "AVM1 call depth exceeded bounded limit ({MAX_CALL_DEPTH})"
            ));
            return None;
        }
        let outcome = self.run_avm1_range(code, 0, code.len(), stack, locals, steps, depth);
        if *steps >= MAX_VM_STEPS_PER_FRAME {
            self.record_unsupported_diagnostic(format!(
                "AVM1 execution reached bounded per-frame step limit ({MAX_VM_STEPS_PER_FRAME})"
            ));
        }
        match outcome {
            Avm1RangeOutcome::Returned(v) => Some(v),
            Avm1RangeOutcome::NormalEnd
            | Avm1RangeOutcome::JumpedOut(_)
            | Avm1RangeOutcome::Thrown => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_avm1_range(
        &mut self,
        code: &[u8],
        start_pc: usize,
        end_pc: usize,
        stack: &mut Vec<Avm1Value>,
        locals: &mut HashMap<String, Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1RangeOutcome {
        let limit = end_pc.min(code.len());
        let mut pc = start_pc.min(limit);
        while pc >= start_pc
            && pc < limit
            && *steps < MAX_VM_STEPS_PER_FRAME
            && self.avm1_thrown.is_none()
        {
            *steps += 1;
            let insn_pc = pc;
            let op = code[pc];
            pc += 1;
            if op == 0x00 {
                break;
            }
            let payload: &[u8] = if (op & 0x80) != 0 {
                if pc + 2 > limit {
                    break;
                }
                let len = usize::from(u16::from_le_bytes([code[pc], code[pc + 1]]));
                pc += 2;
                let end = (pc + len).min(limit);
                let slice = &code[pc..end];
                pc = end;
                slice
            } else {
                &[]
            };

            match op {
                0x04 => {
                    // ActionNextFrame
                    self.set_target_frame_step(1);
                }
                0x05 => {
                    // ActionPrevFrame
                    self.set_target_frame_step(-1);
                }
                0x06 => {
                    // ActionPlay
                    self.set_target_playing(true);
                }
                0x07 => {
                    // ActionStop
                    self.set_target_playing(false);
                }
                0x08 => {
                    // ActionToggleQuality (visual hint; valid SWF4 no-op)
                }
                0x09 => {
                    // ActionStopSounds
                    self.stop_swf_sounds();
                }
                0x0A => {
                    // ActionAdd (numeric)
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Number(b + a));
                }
                0x0B => {
                    // ActionSubtract
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Number(b - a));
                }
                0x0C => {
                    // ActionMultiply
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Number(b * a));
                }
                0x0D => {
                    // ActionDivide
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Number(b / a));
                }
                0x0E => {
                    // ActionEquals (SWF 4 numeric)
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Bool(b == a));
                }
                0x0F => {
                    // ActionLess (SWF 4 numeric)
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Bool(b < a));
                }
                0x10 => {
                    // ActionAnd
                    let a = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    let b = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    self.push_avm_stack(stack, Avm1Value::Bool(b && a));
                }
                0x11 => {
                    // ActionOr
                    let a = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    let b = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    self.push_avm_stack(stack, Avm1Value::Bool(b || a));
                }
                0x12 => {
                    // ActionNot
                    let a = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    self.push_avm_stack(stack, Avm1Value::Bool(!a));
                }
                0x13 => {
                    // ActionStringEquals
                    let a = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let b = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    self.push_avm_stack(stack, Avm1Value::Bool(b == a));
                }
                0x14 | 0x31 => {
                    // ActionStringLength / ActionMBStringLength
                    let s = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    self.push_avm_stack(stack, Avm1Value::Number(s.chars().count() as f64));
                }
                0x15 | 0x35 => {
                    // ActionStringExtract (1-based index, count)
                    let count =
                        stack.pop().unwrap_or(Avm1Value::Undefined).to_i32().max(0) as usize;
                    let index = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .saturating_sub(1)
                        .max(0) as usize;
                    let s = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let sub: String = s.chars().skip(index).take(count).collect();
                    self.push_avm_stack(stack, Avm1Value::String(sub));
                }
                0x17 => {
                    // ActionPop
                    let _ = stack.pop();
                }
                0x18 => {
                    // ActionToInteger
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(v)));
                }
                0x1C => {
                    // ActionGetVariable
                    let name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let val = self.get_avm1_variable(&name, locals);
                    self.push_avm_stack(stack, val);
                }
                0x1D => {
                    // ActionSetVariable
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    self.set_avm1_variable(name, val, locals);
                }
                0x20 => {
                    // ActionSetTarget2
                    let target = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.avm1_target = if let Avm1Value::MovieClipIdentity(identity) = target {
                        identity
                    } else {
                        let authored = target.to_trace_string(self.swf_version, &self.avm1_objects);
                        if authored.is_empty() {
                            String::new()
                        } else {
                            self.canonical_movie_target(&authored)
                        }
                    };
                }
                0x21 => {
                    // ActionStringAdd
                    let a = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let b = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    self.push_avm_stack(stack, Avm1Value::String(format!("{b}{a}")));
                }
                0x22 => {
                    // ActionGetProperty
                    let prop_idx = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let target = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let val = if let Avm1Value::MovieClipIdentity(identity) = target {
                        self.get_mc_property_identity(&identity, prop_idx)
                    } else {
                        let target = target.to_trace_string(self.swf_version, &self.avm1_objects);
                        self.get_mc_property(&target, prop_idx)
                    };
                    self.push_avm_stack(stack, val);
                }
                0x23 => {
                    // ActionSetProperty
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let prop_idx = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let target = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if let Avm1Value::MovieClipIdentity(identity) = target {
                        self.set_mc_property_identity(&identity, prop_idx, val);
                    } else {
                        let target = target.to_trace_string(self.swf_version, &self.avm1_objects);
                        self.set_mc_property(&target, prop_idx, val);
                    }
                }
                0x24 => {
                    // ActionCloneSprite: [source, new_name, depth] (depth on top)
                    let depth_raw = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let new_name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let source = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let src_target = self.resolve_avm1_target_value(&source);
                    let swf_depth = depth_raw.clamp(0, 65535) as u16;
                    let _ =
                        self.duplicate_movie_clip_at_swf_depth(&src_target, &new_name, swf_depth);
                }
                0x25 => {
                    // ActionRemoveSprite: [target]
                    let target = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let resolved = self.resolve_avm1_target_value(&target);
                    self.remove_movie_clip_identity(&resolved);
                }
                0x26 => {
                    // ActionTrace
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let msg = v.to_trace_string(self.swf_version, &self.avm1_objects);
                    self.record_trace(msg);
                }
                0x27 => {
                    // ActionStartDrag: [x1, y1, x2, y2 (if constrain)], constrain, lockCenter, target
                    let target_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let _lock_center = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    let constrain = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    if constrain {
                        let _y2 = stack.pop();
                        let _x2 = stack.pop();
                        let _y1 = stack.pop();
                        let _x1 = stack.pop();
                    }
                    let resolved = self.resolve_avm1_target_value(&target_val);
                    self.drag_target = Some(resolved);
                }
                0x28 => {
                    // ActionEndDrag
                    self.drag_target = None;
                }
                0x29 => {
                    // ActionStringLess
                    let a = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let b = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    self.push_avm_stack(stack, Avm1Value::Bool(b < a));
                }
                0x2A => {
                    // ActionThrow
                    let err = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.avm1_thrown = Some(err);
                    return Avm1RangeOutcome::Thrown;
                }
                0x2B => {
                    // ActionCastOp: [constr, obj] (obj on top)
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let constr = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let out = if self.avm1_is_instance_of(&obj, &constr) {
                        obj
                    } else {
                        Avm1Value::Null
                    };
                    self.push_avm_stack(stack, out);
                }
                0x2C => {
                    // ActionImplementsOp: [iface_1..iface_n, count, ctor] (ctor on top)
                    let ctor = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let count = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .clamp(0, 256) as usize;
                    let mut iface_ids = Vec::with_capacity(count);
                    for _ in 0..count {
                        if let Some(Avm1Value::Object(iface_id)) = stack.pop() {
                            if let Some(Avm1Value::Object(proto_id)) = self
                                .avm1_objects
                                .get(iface_id)
                                .and_then(|o| o.props.get("prototype").cloned())
                            {
                                iface_ids.push(proto_id);
                            }
                            iface_ids.push(iface_id);
                        }
                    }
                    if let Avm1Value::Object(ctor_id) = ctor
                        && let Some(Avm1Value::Object(proto_id)) = self
                            .avm1_objects
                            .get(ctor_id)
                            .and_then(|o| o.props.get("prototype").cloned())
                        && let Some(proto_obj) = self.avm1_objects.get_mut(proto_id)
                    {
                        for iid in iface_ids {
                            if !proto_obj.interfaces.contains(&iid) {
                                proto_obj.interfaces.push(iid);
                            }
                        }
                    }
                }
                0x30 => {
                    // ActionRandomNumber (deterministic LCG)
                    let max = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    self.rng_state = self
                        .rng_state
                        .wrapping_mul(1_664_525)
                        .wrapping_add(1_013_904_223);
                    let r = if max > 0 {
                        (self.rng_state % (max as u32)) as i32
                    } else {
                        0
                    };
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(r)));
                }
                0x32 | 0x36 => {
                    // ActionCharToAscii / ActionMBCharToAscii
                    let s = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let code_pt = s.chars().next().map_or(0u32, u32::from);
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(code_pt)));
                }
                0x33 | 0x37 => {
                    // ActionAsciiToChar / ActionMBAsciiToChar
                    let code_pt = stack.pop().unwrap_or(Avm1Value::Undefined).to_u32();
                    let s = if code_pt == 0 {
                        String::new()
                    } else {
                        char::from_u32(code_pt).map_or_else(String::new, |c| c.to_string())
                    };
                    self.push_avm_stack(stack, Avm1Value::String(s));
                }
                0x34 => {
                    // ActionGetTime (deterministic frame-based elapsed milliseconds)
                    let ms = f64::from(self.executed_frames) * 40.0;
                    self.push_avm_stack(stack, Avm1Value::Number(ms));
                }
                0x3A => {
                    // ActionDelete
                    let prop = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let deleted = match obj {
                        Avm1Value::Object(id) => self.avm1_objects.get_mut(id).is_some_and(|o| {
                            if let Some(elems) = &mut o.array_elements
                                && let Ok(idx) = prop.parse::<usize>()
                            {
                                if idx < elems.len() {
                                    elems[idx] = Avm1Value::Undefined;
                                    o.deleted_indices.insert(idx);
                                }
                                return true;
                            }
                            let _ = o.props.remove(&prop);
                            true
                        }),
                        Avm1Value::MovieClip(mc) => {
                            let target = self.canonical_movie_target(&mc);
                            if mc == "_global" {
                                self.avm1_global_obj.remove(&prop);
                                true
                            } else if target == "_root" {
                                self.avm1_globals.remove(&prop);
                                true
                            } else if let Some(slot) = self.target_slot_mut(&target) {
                                slot.props.remove(&prop);
                                true
                            } else {
                                false
                            }
                        }
                        Avm1Value::MovieClipIdentity(target) => {
                            if target == "_root" {
                                self.avm1_globals.remove(&prop);
                                true
                            } else if let Some(slot) = self.target_slot_mut(&target) {
                                slot.props.remove(&prop);
                                true
                            } else {
                                false
                            }
                        }
                        _ => false,
                    };
                    self.push_avm_stack(stack, Avm1Value::Bool(deleted));
                }
                0x3B => {
                    // ActionDelete2
                    let name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let mut deleted = false;
                    for scope in self.avm1_with_scopes.clone().into_iter().rev() {
                        if let Avm1Value::Object(id) = scope
                            && let Some(o) = self.avm1_objects.get_mut(id)
                            && o.props.remove(&name).is_some()
                        {
                            deleted = true;
                            break;
                        }
                    }
                    if !deleted {
                        for &scope_id in self.avm1_scope_chain.iter().rev() {
                            if let Some(o) = self.avm1_objects.get_mut(scope_id)
                                && o.props.remove(&name).is_some()
                            {
                                deleted = true;
                                break;
                            }
                        }
                    }
                    if !deleted {
                        deleted |= locals.remove(&name).is_some();
                        let cur = self.current_target_path();
                        if cur != "_root"
                            && let Some(slot) = self.target_slot_mut(&cur)
                        {
                            deleted |= slot.props.remove(&name).is_some();
                        }
                        deleted |= self.avm1_globals.remove(&name).is_some();
                        deleted |= self.avm1_global_obj.remove(&name).is_some();
                        deleted |= self.avm1_functions.remove(&name).is_some();
                    }
                    self.push_avm_stack(stack, Avm1Value::Bool(deleted));
                }
                0x3C => {
                    // ActionDefineLocal
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    if let Some(&act_id) = self.avm1_scope_chain.last() {
                        if let Some(act_obj) = self.avm1_objects.get_mut(act_id) {
                            act_obj.props.insert(name, val);
                        }
                    } else {
                        locals.insert(name.clone(), val.clone());
                        if depth == 0 {
                            let cur = self.current_target_path();
                            if cur == "_root" {
                                self.avm1_globals.insert(name, val);
                            } else if let Some(slot) = self.target_slot_mut(&cur) {
                                slot.props.insert(name, val);
                            }
                        }
                    }
                }
                0x3D => {
                    // ActionCallFunction
                    let fn_name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let nargs = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .clamp(0, 256) as usize;
                    let mut args = Vec::with_capacity(nargs);
                    for _ in 0..nargs {
                        args.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    let ret = self.call_avm1_function(&fn_name, args, locals, steps, depth);
                    if self.avm1_thrown.is_some() {
                        return Avm1RangeOutcome::Thrown;
                    }
                    self.push_avm_stack(stack, ret);
                }
                0x3E => {
                    // ActionReturn
                    return Avm1RangeOutcome::Returned(stack.pop().unwrap_or(Avm1Value::Undefined));
                }
                0x3F => {
                    // ActionModulo
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Number(b % a));
                }
                0x40 => {
                    // ActionNewObject
                    let ctor_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let ctor_name = ctor_val.to_trace_string(self.swf_version, &self.avm1_objects);
                    let nargs = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .clamp(0, 256) as usize;
                    let mut args = Vec::with_capacity(nargs);
                    for _ in 0..nargs {
                        args.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    let resolved_ctor = if matches!(ctor_val, Avm1Value::Object(_)) {
                        ctor_val
                    } else {
                        self.get_avm1_variable(&ctor_name, locals)
                    };
                    let constructed =
                        self.construct_avm1_object(&ctor_name, &resolved_ctor, args, steps, depth);
                    if self.avm1_thrown.is_some() {
                        return Avm1RangeOutcome::Thrown;
                    }
                    self.push_avm_stack(stack, constructed);
                }
                0x41 => {
                    // ActionDefineLocal2
                    let name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    if let Some(&act_id) = self.avm1_scope_chain.last() {
                        if let Some(act_obj) = self.avm1_objects.get_mut(act_id) {
                            act_obj.props.entry(name).or_insert(Avm1Value::Undefined);
                        }
                    } else {
                        locals.entry(name.clone()).or_insert(Avm1Value::Undefined);
                        if depth == 0 {
                            let cur = self.current_target_path();
                            if cur == "_root" {
                                self.avm1_globals
                                    .entry(name)
                                    .or_insert(Avm1Value::Undefined);
                            } else if let Some(slot) = self.target_slot_mut(&cur) {
                                slot.props.entry(name).or_insert(Avm1Value::Undefined);
                            }
                        }
                    }
                }
                0x42 => {
                    // ActionInitArray
                    let n = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .clamp(0, 4096) as usize;
                    let mut elems = Vec::with_capacity(n);
                    for _ in 0..n {
                        elems.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    let obj_id = self.avm1_objects.len();
                    self.avm1_objects.push(Avm1Object {
                        array_elements: Some(elems),
                        ..Default::default()
                    });
                    self.push_avm_stack(stack, Avm1Value::Object(obj_id));
                }
                0x43 => {
                    // ActionInitObject
                    let n = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .clamp(0, 4096) as usize;
                    let mut props = HashMap::new();
                    for _ in 0..n {
                        let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                        let key = stack
                            .pop()
                            .unwrap_or(Avm1Value::Undefined)
                            .to_trace_string(self.swf_version, &self.avm1_objects);
                        props.insert(key, val);
                    }
                    let obj_id = self.avm1_objects.len();
                    self.avm1_objects.push(Avm1Object {
                        props,
                        ..Default::default()
                    });
                    self.push_avm_stack(stack, Avm1Value::Object(obj_id));
                }
                0x44 => {
                    // ActionTypeOf
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let t = match v {
                        Avm1Value::Undefined => "undefined",
                        Avm1Value::Null => "null",
                        Avm1Value::Bool(_) => "boolean",
                        Avm1Value::Number(_) => "number",
                        Avm1Value::String(_) => "string",
                        Avm1Value::MovieClip(ref mc) if mc.starts_with("__builtin_") => "function",
                        Avm1Value::MovieClip(ref mc) if mc == "Math" || mc == "_global" => "object",
                        Avm1Value::MovieClip(_) | Avm1Value::MovieClipIdentity(_) => "movieclip",
                        Avm1Value::Super { .. } => "object",
                        Avm1Value::Object(id) => {
                            if self
                                .avm1_objects
                                .get(id)
                                .is_some_and(|o| o.function.is_some())
                            {
                                "function"
                            } else {
                                "object"
                            }
                        }
                    };
                    self.push_avm_stack(stack, Avm1Value::String(t.to_owned()));
                }
                0x45 => {
                    // ActionTargetPath: returns _level0 dot-path ONLY for MovieClip values
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let out = match v {
                        Avm1Value::MovieClip(mc)
                            if mc != "Math" && mc != "_global" && !mc.starts_with("__builtin_") =>
                        {
                            let canonical = self.canonical_movie_target(&mc);
                            self.target_dot_path(&canonical)
                                .map_or(Avm1Value::Undefined, Avm1Value::String)
                        }
                        Avm1Value::MovieClipIdentity(target) => self
                            .target_dot_path(&target)
                            .map_or(Avm1Value::Undefined, Avm1Value::String),
                        _ => Avm1Value::Undefined,
                    };
                    self.push_avm_stack(stack, out);
                }
                0x46 => {
                    // ActionEnumerate (variable path string on stack)
                    let var_name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let obj = self.get_avm1_variable(&var_name, locals);
                    self.enumerate_avm1_value(&obj, stack);
                }
                0x47 => {
                    // ActionAdd2 (type-aware)
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if matches!(a, Avm1Value::String(_)) || matches!(b, Avm1Value::String(_)) {
                        let sa = a.to_trace_string(self.swf_version, &self.avm1_objects);
                        let sb = b.to_trace_string(self.swf_version, &self.avm1_objects);
                        self.push_avm_stack(stack, Avm1Value::String(format!("{sb}{sa}")));
                    } else {
                        self.push_avm_stack(
                            stack,
                            Avm1Value::Number(b.to_number() + a.to_number()),
                        );
                    }
                }
                0x48 => {
                    // ActionLess2
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let res = match (&b, &a) {
                        (Avm1Value::String(sb), Avm1Value::String(sa)) => Avm1Value::Bool(sb < sa),
                        _ => {
                            let nb = b.to_number();
                            let na = a.to_number();
                            if nb.is_nan() || na.is_nan() {
                                Avm1Value::Undefined
                            } else {
                                Avm1Value::Bool(nb < na)
                            }
                        }
                    };
                    self.push_avm_stack(stack, res);
                }
                0x49 => {
                    // ActionEquals2
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let eq = avm1_equals(&b, &a, self.swf_version, &self.avm1_objects);
                    self.push_avm_stack(stack, Avm1Value::Bool(eq));
                }
                0x4A => {
                    // ActionToNumber
                    let n = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Number(n));
                }
                0x4B => {
                    // ActionToString
                    let s = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    self.push_avm_stack(stack, Avm1Value::String(s));
                }
                0x4C => {
                    // ActionPushDuplicate
                    let top = stack.last().cloned().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(stack, top);
                }
                0x4D => {
                    // ActionSwap
                    let len = stack.len();
                    if len >= 2 {
                        stack.swap(len - 1, len - 2);
                    }
                }
                0x4E => {
                    // ActionGetMember
                    let member = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let target = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let val = self.get_avm1_member(&target, &member);
                    self.push_avm_stack(stack, val);
                }
                0x4F => {
                    // ActionSetMember
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let member = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let target = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.set_avm1_member(&target, &member, val);
                }
                0x50 => {
                    // ActionIncrement
                    let n = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Number(n + 1.0));
                }
                0x51 => {
                    // ActionDecrement
                    let n = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Number(n - 1.0));
                }
                0x52 => {
                    // ActionCallMethod
                    let method_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let method_name = if matches!(method_val, Avm1Value::Undefined) {
                        String::new()
                    } else {
                        method_val.to_trace_string(self.swf_version, &self.avm1_objects)
                    };
                    let receiver = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let nargs = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .clamp(0, 256) as usize;
                    let mut args = Vec::with_capacity(nargs);
                    for _ in 0..nargs {
                        args.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    let ret =
                        self.call_avm1_method(&receiver, &method_name, args, locals, steps, depth);
                    if self.avm1_thrown.is_some() {
                        return Avm1RangeOutcome::Thrown;
                    }
                    self.push_avm_stack(stack, ret);
                }
                0x53 => {
                    // ActionNewMethod
                    let method_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let method_name = if matches!(method_val, Avm1Value::Undefined) {
                        String::new()
                    } else {
                        method_val.to_trace_string(self.swf_version, &self.avm1_objects)
                    };
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let nargs = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .clamp(0, 256) as usize;
                    let mut args = Vec::with_capacity(nargs);
                    for _ in 0..nargs {
                        args.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    let ctor_val = if method_name.is_empty() {
                        obj
                    } else {
                        self.get_avm1_member(&obj, &method_name)
                    };
                    let constructed =
                        self.construct_avm1_object(&method_name, &ctor_val, args, steps, depth);
                    if self.avm1_thrown.is_some() {
                        return Avm1RangeOutcome::Thrown;
                    }
                    self.push_avm_stack(stack, constructed);
                }
                0x54 => {
                    // ActionInstanceOf: [obj, constr] (constr on top)
                    let constr = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let is_inst = self.avm1_is_instance_of(&obj, &constr);
                    self.push_avm_stack(stack, Avm1Value::Bool(is_inst));
                }
                0x55 => {
                    // ActionEnumerate2
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.enumerate_avm1_value(&obj, stack);
                }
                0x60 => {
                    // ActionBitAnd
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(b & a)));
                }
                0x61 => {
                    // ActionBitOr
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(b | a)));
                }
                0x62 => {
                    // ActionBitXor
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(b ^ a)));
                }
                0x63 => {
                    // ActionBitLShift
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_u32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(b.wrapping_shl(a))));
                }
                0x64 => {
                    // ActionBitRShift
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_u32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(b.wrapping_shr(a))));
                }
                0x65 => {
                    // ActionBitURShift
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_u32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_u32();
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(b.wrapping_shr(a))));
                }
                0x66 => {
                    // ActionStrictEquals
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let eq = match (&b, &a) {
                        (Avm1Value::Undefined, Avm1Value::Undefined)
                        | (Avm1Value::Null, Avm1Value::Null) => true,
                        (Avm1Value::Bool(x), Avm1Value::Bool(y)) => x == y,
                        (Avm1Value::Number(x), Avm1Value::Number(y)) => x == y,
                        (Avm1Value::String(x), Avm1Value::String(y)) => x == y,
                        (Avm1Value::Object(x), Avm1Value::Object(y)) => x == y,
                        (Avm1Value::MovieClip(x), Avm1Value::MovieClip(y))
                        | (Avm1Value::MovieClipIdentity(x), Avm1Value::MovieClipIdentity(y))
                        | (Avm1Value::MovieClip(x), Avm1Value::MovieClipIdentity(y))
                        | (Avm1Value::MovieClipIdentity(x), Avm1Value::MovieClip(y)) => {
                            self.canonical_movie_target(x) == self.canonical_movie_target(y)
                        }
                        _ => false,
                    };
                    self.push_avm_stack(stack, Avm1Value::Bool(eq));
                }
                0x67 => {
                    // ActionGreater
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let res = match (&b, &a) {
                        (Avm1Value::String(sb), Avm1Value::String(sa)) => Avm1Value::Bool(sb > sa),
                        _ => {
                            let nb = b.to_number();
                            let na = a.to_number();
                            if nb.is_nan() || na.is_nan() {
                                Avm1Value::Undefined
                            } else {
                                Avm1Value::Bool(nb > na)
                            }
                        }
                    };
                    self.push_avm_stack(stack, res);
                }
                0x68 => {
                    // ActionStringGreater
                    let a = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let b = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    self.push_avm_stack(stack, Avm1Value::Bool(b > a));
                }
                0x69 => {
                    // ActionExtends: [subclass, superclass] (superclass on top)
                    let sup = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let sub = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if let (Avm1Value::Object(sub_id), Avm1Value::Object(sup_id)) = (sub, sup) {
                        let sup_proto =
                            self.get_avm1_member(&Avm1Value::Object(sup_id), "prototype");
                        let new_proto_id = self.avm1_objects.len();
                        let mut proto_props = HashMap::new();
                        proto_props.insert("__proto__".to_owned(), sup_proto);
                        proto_props.insert("__constructor__".to_owned(), Avm1Value::Object(sup_id));
                        proto_props.insert("constructor".to_owned(), Avm1Value::Object(sub_id));
                        self.avm1_objects.push(Avm1Object {
                            props: proto_props,
                            ..Default::default()
                        });
                        if let Some(sub_obj) = self.avm1_objects.get_mut(sub_id) {
                            sub_obj
                                .props
                                .insert("prototype".to_owned(), Avm1Value::Object(new_proto_id));
                            sub_obj
                                .props
                                .insert("__super_ctor__".to_owned(), Avm1Value::Object(sup_id));
                        }
                    }
                }
                0x81 => {
                    // ActionGotoFrame (u16 zero-based frame index)
                    if payload.len() >= 2 {
                        let f = u32::from(u16::from_le_bytes([payload[0], payload[1]]));
                        let target = self.current_target_path();
                        self.set_target_goto_value_identity(
                            &target,
                            &Avm1Value::Number(f64::from(f + 1)),
                            false,
                        );
                    }
                }
                0x83 => {
                    // ActionGetURL (url, target c-strings — safe sandboxed no-op)
                }
                0x87 => {
                    // ActionStoreRegister
                    if let Some(&reg) = payload.first()
                        && let Some(top) = stack.last().cloned()
                    {
                        self.avm1_registers[usize::from(reg)] = top;
                    }
                }
                0x88 => {
                    // ActionConstantPool
                    let mut pr = SwfSlice::new(payload);
                    if let Some(count) = pr.u16() {
                        let mut pool = Vec::with_capacity(usize::from(count));
                        for _ in 0..count {
                            if let Some((s, _)) = pr.c_string(self.swf_version) {
                                pool.push(s);
                            } else {
                                break;
                            }
                        }
                        self.avm1_constant_pool = pool;
                    }
                }
                0x8A => {
                    // ActionWaitForFrame (all frames loaded in local SWF buffer)
                }
                0x8B => {
                    // ActionSetTarget
                    let mut pr = SwfSlice::new(payload);
                    if let Some((t, _)) = pr.c_string(self.swf_version) {
                        self.avm1_target = if t.is_empty() {
                            String::new()
                        } else {
                            self.canonical_movie_target(&t)
                        };
                    }
                }
                0x8C => {
                    // ActionGotoLabel
                    let mut pr = SwfSlice::new(payload);
                    if let Some((label, _)) = pr.c_string(self.swf_version) {
                        let target = self.current_target_path();
                        self.set_target_goto_value_identity(
                            &target,
                            &Avm1Value::String(label),
                            false,
                        );
                    }
                }
                0x8D => {
                    // ActionWaitForFrame2 (pops frame expression; local SWF is fully loaded)
                    let _ = stack.pop();
                }
                0x8E => {
                    // ActionDefineFunction2
                    let mut pr = SwfSlice::new(payload);
                    let Some((fn_name, _)) = pr.c_string(self.swf_version) else {
                        continue;
                    };
                    let Some(num_params) = pr.u16() else {
                        continue;
                    };
                    let Some(register_count) = pr.u8() else {
                        continue;
                    };
                    let Some(flags) = pr.u16() else {
                        continue;
                    };
                    let mut params = Vec::with_capacity(usize::from(num_params));
                    for _ in 0..num_params {
                        let Some(reg) = pr.u8() else { break };
                        let Some((pname, _)) = pr.c_string(self.swf_version) else {
                            break;
                        };
                        params.push((reg, pname));
                    }
                    let Some(code_size) = pr.u16().map(usize::from) else {
                        continue;
                    };
                    let body_end = (pc + code_size).min(code.len());
                    let body = code[pc..body_end].to_vec();
                    pc = body_end;
                    let fn_obj_id = self.register_avm1_function_object(
                        &fn_name,
                        params,
                        register_count,
                        flags,
                        true,
                        body,
                        locals,
                        depth,
                    );
                    if fn_name.is_empty() {
                        self.push_avm_stack(stack, Avm1Value::Object(fn_obj_id));
                    }
                }
                0x8F => {
                    // ActionTry — run directly against `code` with PC range bounds so
                    // `break`/`continue` jump offsets inside `try`/`catch`/`finally` address
                    // enclosing loops/switches accurately.
                    let mut pr = SwfSlice::new(payload);
                    let Some(flags) = pr.u8() else { continue };
                    let Some(try_size) = pr.u16().map(usize::from) else {
                        continue;
                    };
                    let Some(catch_size) = pr.u16().map(usize::from) else {
                        continue;
                    };
                    let Some(finally_size) = pr.u16().map(usize::from) else {
                        continue;
                    };
                    let has_catch = (flags & 0x01) != 0;
                    let has_finally = (flags & 0x02) != 0;
                    let catch_in_reg = (flags & 0x04) != 0;
                    let (catch_var, catch_reg) = if catch_in_reg {
                        (None, pr.u8())
                    } else {
                        (pr.c_string(self.swf_version).map(|(s, _)| s), None)
                    };
                    let try_start = pc;
                    let try_end = (try_start + try_size).min(limit);
                    let catch_start = try_end;
                    let catch_end = (catch_start + catch_size).min(limit);
                    let finally_start = catch_end;
                    let finally_end = (finally_start + finally_size).min(limit);
                    pc = finally_end;

                    let saved_stack_len = stack.len();
                    let mut body_outcome =
                        self.run_avm1_range(code, try_start, try_end, stack, locals, steps, depth);
                    if let Avm1RangeOutcome::JumpedOut(target) = body_outcome
                        && has_finally
                        && finally_size > 0
                        && target == finally_start
                    {
                        body_outcome = Avm1RangeOutcome::NormalEnd;
                    }
                    if (matches!(body_outcome, Avm1RangeOutcome::Thrown)
                        || self.avm1_thrown.is_some())
                        && let Some(thrown_val) = self.avm1_thrown.take()
                    {
                        stack.truncate(saved_stack_len);
                        if has_catch {
                            if let Some(reg) = catch_reg {
                                self.avm1_registers[usize::from(reg)] = thrown_val;
                            } else if let Some(var_name) = catch_var
                                && !var_name.is_empty()
                            {
                                if let Some(&act_id) = self.avm1_scope_chain.last() {
                                    if let Some(act_obj) = self.avm1_objects.get_mut(act_id) {
                                        act_obj.props.insert(var_name, thrown_val);
                                    }
                                } else {
                                    locals.insert(var_name.clone(), thrown_val.clone());
                                    if depth == 0 {
                                        self.avm1_globals.insert(var_name, thrown_val);
                                    }
                                }
                            }
                            body_outcome = self.run_avm1_range(
                                code,
                                catch_start,
                                catch_end,
                                stack,
                                locals,
                                steps,
                                depth,
                            );
                            if self.avm1_thrown.is_some() {
                                stack.truncate(saved_stack_len);
                                body_outcome = Avm1RangeOutcome::Thrown;
                            }
                        } else {
                            self.avm1_thrown = Some(thrown_val);
                            body_outcome = Avm1RangeOutcome::Thrown;
                        }
                    }

                    let should_run_finally = has_finally
                        && matches!(
                            body_outcome,
                            Avm1RangeOutcome::NormalEnd | Avm1RangeOutcome::Thrown
                        );
                    if should_run_finally {
                        let pending_throw = self.avm1_thrown.take();
                        let fin_outcome = self.run_avm1_range(
                            code,
                            finally_start,
                            finally_end,
                            stack,
                            locals,
                            steps,
                            depth,
                        );
                        if self.avm1_thrown.is_some() {
                            return Avm1RangeOutcome::Thrown;
                        }
                        match fin_outcome {
                            Avm1RangeOutcome::Returned(v) => return Avm1RangeOutcome::Returned(v),
                            Avm1RangeOutcome::JumpedOut(target) => {
                                if target == limit {
                                    return Avm1RangeOutcome::NormalEnd;
                                }
                                if target < start_pc || target > limit {
                                    return Avm1RangeOutcome::JumpedOut(target);
                                }
                                pc = target;
                                continue;
                            }
                            Avm1RangeOutcome::Thrown => return Avm1RangeOutcome::Thrown,
                            Avm1RangeOutcome::NormalEnd => {
                                if let Some(err) = pending_throw {
                                    self.avm1_thrown = Some(err);
                                    return Avm1RangeOutcome::Thrown;
                                }
                            }
                        }
                    }

                    match body_outcome {
                        Avm1RangeOutcome::NormalEnd => {}
                        Avm1RangeOutcome::Returned(v) => return Avm1RangeOutcome::Returned(v),
                        Avm1RangeOutcome::Thrown => return Avm1RangeOutcome::Thrown,
                        Avm1RangeOutcome::JumpedOut(target) => {
                            if target == limit {
                                return Avm1RangeOutcome::NormalEnd;
                            }
                            if target < start_pc || target > limit {
                                return Avm1RangeOutcome::JumpedOut(target);
                            }
                            pc = target;
                        }
                    }
                }
                0x94 => {
                    // ActionWith
                    let scope_obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if payload.len() >= 2 {
                        let with_size = usize::from(u16::from_le_bytes([payload[0], payload[1]]));
                        let with_start = pc;
                        let end = (with_start + with_size).min(limit);
                        pc = end;
                        let pushed = !matches!(scope_obj, Avm1Value::Undefined | Avm1Value::Null);
                        if pushed {
                            self.avm1_with_scopes.push(scope_obj);
                        }
                        let with_outcome =
                            self.run_avm1_range(code, with_start, end, stack, locals, steps, depth);
                        if pushed {
                            let _ = self.avm1_with_scopes.pop();
                        }
                        match with_outcome {
                            Avm1RangeOutcome::NormalEnd => {}
                            Avm1RangeOutcome::Returned(v) => return Avm1RangeOutcome::Returned(v),
                            Avm1RangeOutcome::Thrown => return Avm1RangeOutcome::Thrown,
                            Avm1RangeOutcome::JumpedOut(target) => {
                                if target == limit {
                                    return Avm1RangeOutcome::NormalEnd;
                                }
                                if target < start_pc || target > limit {
                                    return Avm1RangeOutcome::JumpedOut(target);
                                }
                                pc = target;
                            }
                        }
                    }
                }
                0x96 => {
                    // ActionPush
                    let mut pr = SwfSlice::new(payload);
                    while let Some(ptype) = pr.u8() {
                        match ptype {
                            0 => {
                                if let Some((s, _)) = pr.c_string(self.swf_version) {
                                    self.push_avm_stack(stack, Avm1Value::String(s));
                                } else {
                                    break;
                                }
                            }
                            1 => {
                                if let Some(b) = pr.take(4) {
                                    let f = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(f)));
                                } else {
                                    break;
                                }
                            }
                            2 => self.push_avm_stack(stack, Avm1Value::Null),
                            3 => self.push_avm_stack(stack, Avm1Value::Undefined),
                            4 => {
                                if let Some(reg) = pr.u8() {
                                    let v = self.avm1_registers[usize::from(reg)].clone();
                                    self.push_avm_stack(stack, v);
                                } else {
                                    break;
                                }
                            }
                            5 => {
                                if let Some(b) = pr.u8() {
                                    self.push_avm_stack(stack, Avm1Value::Bool(b != 0));
                                } else {
                                    break;
                                }
                            }
                            6 => {
                                // SWF double has the two 32-bit words swapped in little-endian!
                                if let Some(b) = pr.take(8) {
                                    let d = f64::from_le_bytes([
                                        b[4], b[5], b[6], b[7], b[0], b[1], b[2], b[3],
                                    ]);
                                    self.push_avm_stack(stack, Avm1Value::Number(d));
                                } else {
                                    break;
                                }
                            }
                            7 => {
                                if let Some(i) = pr.i32() {
                                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(i)));
                                } else {
                                    break;
                                }
                            }
                            8 => {
                                if let Some(idx) = pr.u8() {
                                    let s = self
                                        .avm1_constant_pool
                                        .get(usize::from(idx))
                                        .cloned()
                                        .unwrap_or_default();
                                    self.push_avm_stack(stack, Avm1Value::String(s));
                                } else {
                                    break;
                                }
                            }
                            9 => {
                                if let Some(idx) = pr.u16() {
                                    let s = self
                                        .avm1_constant_pool
                                        .get(usize::from(idx))
                                        .cloned()
                                        .unwrap_or_default();
                                    self.push_avm_stack(stack, Avm1Value::String(s));
                                } else {
                                    break;
                                }
                            }
                            _ => break,
                        }
                    }
                }
                0x99 => {
                    // ActionJump (i16 signed byte offset relative to end of instruction)
                    if payload.len() >= 2 {
                        let offset = i16::from_le_bytes([payload[0], payload[1]]) as isize;
                        let next_pc = (pc as isize) + offset;
                        if next_pc >= 0 && (next_pc as usize) <= code.len() {
                            let target = next_pc as usize;
                            if target == limit {
                                return Avm1RangeOutcome::NormalEnd;
                            }
                            if target < start_pc || target > limit {
                                return Avm1RangeOutcome::JumpedOut(target);
                            }
                            pc = target;
                        } else {
                            break;
                        }
                    }
                }
                0x9A => {
                    // ActionGetURL2 (pops target_window, url — safe sandboxed no-op)
                    let _target_window = stack.pop();
                    let _url = stack.pop();
                }
                0x9B => {
                    // ActionDefineFunction
                    let mut pr = SwfSlice::new(payload);
                    let Some((fn_name, _)) = pr.c_string(self.swf_version) else {
                        continue;
                    };
                    let Some(num_params) = pr.u16() else {
                        continue;
                    };
                    let mut params = Vec::with_capacity(usize::from(num_params));
                    for _ in 0..num_params {
                        let Some((pname, _)) = pr.c_string(self.swf_version) else {
                            break;
                        };
                        params.push((0, pname));
                    }
                    let Some(code_size) = pr.u16().map(usize::from) else {
                        continue;
                    };
                    let body_end = (pc + code_size).min(code.len());
                    let body = code[pc..body_end].to_vec();
                    pc = body_end;
                    let fn_obj_id = self.register_avm1_function_object(
                        &fn_name, params, 0, 0, false, body, locals, depth,
                    );
                    if fn_name.is_empty() {
                        self.push_avm_stack(stack, Avm1Value::Object(fn_obj_id));
                    }
                }
                0x9D => {
                    // ActionIf
                    let cond = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    if cond && payload.len() >= 2 {
                        let offset = i16::from_le_bytes([payload[0], payload[1]]) as isize;
                        let next_pc = (pc as isize) + offset;
                        if next_pc >= 0 && (next_pc as usize) <= code.len() {
                            let target = next_pc as usize;
                            if target == limit {
                                return Avm1RangeOutcome::NormalEnd;
                            }
                            if target < start_pc || target > limit {
                                return Avm1RangeOutcome::JumpedOut(target);
                            }
                            pc = target;
                        } else {
                            break;
                        }
                    }
                }
                0x9E => {
                    // ActionCall (Flash 4 call frame actions)
                    let frame_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let movie_target = self.current_target_path();
                    let resolved = match &frame_val {
                        Avm1Value::Number(n) if *n >= 1.0 => Some((*n as u32).saturating_sub(1)),
                        Avm1Value::String(s) => {
                            if let Ok(num) = s.trim().parse::<u32>()
                                && num >= 1
                            {
                                Some(num - 1)
                            } else {
                                self.target_label_frame(&movie_target, s)
                            }
                        }
                        _ => None,
                    };
                    if let Some(f_idx) = resolved {
                        let scripts: Vec<Vec<u8>> = if movie_target == "_root" {
                            self.root_timeline
                                .frames
                                .get(f_idx as usize)
                                .map(|tags| {
                                    tags.iter()
                                        .filter(|t| t.code == 12)
                                        .map(|t| t.payload.clone())
                                        .collect()
                                })
                                .unwrap_or_default()
                        } else if let Some(cid) =
                            self.target_slot(&movie_target).map(|s| s.character_id)
                        {
                            self.sprites
                                .get(&cid)
                                .and_then(|def| def.frames.get(f_idx as usize))
                                .map(|tags| {
                                    tags.iter()
                                        .filter(|t| t.code == 12)
                                        .map(|t| t.payload.clone())
                                        .collect()
                                })
                                .unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                        for script_bytes in scripts {
                            let mut sub_stack = Vec::new();
                            let _ = self.run_avm1_code(
                                &script_bytes,
                                &mut sub_stack,
                                locals,
                                steps,
                                depth + 1,
                            );
                        }
                    }
                }
                0x9F => {
                    // ActionGotoFrame2
                    let target = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let flags = payload.first().copied().unwrap_or(0);
                    let play_flag = (flags & 0x01) != 0;
                    let bias = if (flags & 0x02) != 0 && payload.len() >= 3 {
                        u32::from(u16::from_le_bytes([payload[1], payload[2]]))
                    } else {
                        0
                    };
                    let movie_target = self.current_target_path();
                    let total = self.target_total_frames(&movie_target);
                    let resolved = match target {
                        Avm1Value::Number(n) if n >= 1.0 => {
                            let f = (n as i64).saturating_sub(1).max(0) as u32 + bias;
                            Some(f.min(total.saturating_sub(1)))
                        }
                        Avm1Value::String(s) => {
                            if let Ok(num) = s.trim().parse::<u32>()
                                && num >= 1
                            {
                                let f = num.saturating_sub(1) + bias;
                                Some(f.min(total.saturating_sub(1)))
                            } else {
                                self.target_label_frame(&movie_target, &s)
                                    .map(|f| (f + bias).min(total.saturating_sub(1)))
                            }
                        }
                        _ => None,
                    };
                    if let Some(target_f) = resolved {
                        self.set_target_frame_zero_based(&movie_target, target_f, play_flag);
                    }
                }
                _ => {
                    let cur_target = self.current_target_path();
                    self.record_unsupported_diagnostic(format!(
                        "Unsupported AVM1 action opcode 0x{op:02X} at target={cur_target} frame={} offset=0x{insn_pc:x}",
                        self.current_frame
                    ));
                }
            }
        }
        if self.avm1_thrown.is_some() {
            Avm1RangeOutcome::Thrown
        } else {
            Avm1RangeOutcome::NormalEnd
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn register_avm1_function_object(
        &mut self,
        fn_name: &str,
        params: Vec<(u8, String)>,
        register_count: u8,
        flags: u16,
        is_v2: bool,
        body: Vec<u8>,
        locals: &mut HashMap<String, Avm1Value>,
        depth: usize,
    ) -> usize {
        let proto_id = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object::default());
        let fn_obj_id = self.avm1_objects.len();
        let func = Avm1Function {
            params,
            register_count,
            flags,
            is_v2,
            body,
            constant_pool: self.avm1_constant_pool.clone(),
            base_target: self.avm1_target.clone(),
            self_obj_id: Some(fn_obj_id),
            captured_scopes: self.avm1_scope_chain.clone(),
        };
        let mut fn_props = HashMap::new();
        fn_props.insert("prototype".to_owned(), Avm1Value::Object(proto_id));
        self.avm1_objects.push(Avm1Object {
            props: fn_props,
            function: Some(func.clone()),
            ..Default::default()
        });
        if let Some(proto_obj) = self.avm1_objects.get_mut(proto_id) {
            proto_obj
                .props
                .insert("constructor".to_owned(), Avm1Value::Object(fn_obj_id));
        }
        if !fn_name.is_empty() {
            let fn_val = Avm1Value::Object(fn_obj_id);
            if let Some(&act_id) = self.avm1_scope_chain.last() {
                if let Some(act_obj) = self.avm1_objects.get_mut(act_id) {
                    act_obj.props.insert(fn_name.to_owned(), fn_val);
                }
            } else if depth > 0 {
                locals.insert(fn_name.to_owned(), fn_val);
            } else {
                self.avm1_functions.insert(fn_name.to_owned(), func);
                let cur = self.current_target_path();
                if cur != "_root"
                    && let Some(slot) = self.target_slot_mut(&cur)
                {
                    slot.props.insert(fn_name.to_owned(), fn_val);
                } else {
                    self.avm1_globals.insert(fn_name.to_owned(), fn_val);
                }
            }
        }
        fn_obj_id
    }

    pub(super) fn construct_avm1_object(
        &mut self,
        ctor_name: &str,
        ctor_val: &Avm1Value,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::Object(ctor_id) = ctor_val
            && let Some(func) = self
                .avm1_objects
                .get(*ctor_id)
                .and_then(|o| o.function.clone())
        {
            let proto = self.get_avm1_member(&Avm1Value::Object(*ctor_id), "prototype");
            let super_ctor = self
                .avm1_objects
                .get(*ctor_id)
                .and_then(|o| o.props.get("__super_ctor__").cloned())
                .and_then(|v| match v {
                    Avm1Value::Object(id) => Some(id),
                    _ => None,
                });
            let inst_id = self.avm1_objects.len();
            let mut props = HashMap::new();
            props.insert("__proto__".to_owned(), proto);
            props.insert("__constructor__".to_owned(), Avm1Value::Object(*ctor_id));
            self.avm1_objects.push(Avm1Object {
                props,
                ..Default::default()
            });
            let inst_val = Avm1Value::Object(inst_id);
            let ret = self.invoke_avm1_func_with_this(
                &func,
                inst_val.clone(),
                super_ctor,
                args,
                steps,
                depth,
            );
            return if matches!(ret, Avm1Value::Object(_)) {
                ret
            } else {
                inst_val
            };
        }
        if !ctor_name.is_empty()
            && let Some(func) = self.avm1_functions.get(ctor_name).cloned()
        {
            let inst_id = self.avm1_objects.len();
            let mut props = HashMap::new();
            if let Some(fn_id) = func.self_obj_id {
                let proto = self.get_avm1_member(&Avm1Value::Object(fn_id), "prototype");
                props.insert("__proto__".to_owned(), proto);
                props.insert("__constructor__".to_owned(), Avm1Value::Object(fn_id));
            }
            self.avm1_objects.push(Avm1Object {
                props,
                ..Default::default()
            });
            let inst_val = Avm1Value::Object(inst_id);
            let ret =
                self.invoke_avm1_func_with_this(&func, inst_val.clone(), None, args, steps, depth);
            return if matches!(ret, Avm1Value::Object(_)) {
                ret
            } else {
                inst_val
            };
        }
        let obj_id = self.avm1_objects.len();
        let mut obj = Avm1Object::default();
        if ctor_name == "Array"
            || matches!(ctor_val, Avm1Value::MovieClip(s) if s == "__builtin_Array")
        {
            if args.len() == 1
                && let Some(Avm1Value::Number(n)) = args.first()
            {
                let len = (*n as usize).min(4096);
                obj.array_elements = Some(vec![Avm1Value::Undefined; len]);
            } else {
                obj.array_elements = Some(args);
            }
        } else if let Some(err_kind) = match ctor_val {
            Avm1Value::MovieClip(s) => s.strip_prefix("__builtin_").filter(|k| {
                matches!(
                    *k,
                    "Error"
                        | "TypeError"
                        | "RangeError"
                        | "ReferenceError"
                        | "SyntaxError"
                        | "URIError"
                        | "EvalError"
                        | "ArgumentError"
                )
            }),
            _ => None,
        }
        .or_else(|| {
            matches!(
                ctor_name,
                "Error"
                    | "TypeError"
                    | "RangeError"
                    | "ReferenceError"
                    | "SyntaxError"
                    | "URIError"
                    | "EvalError"
                    | "ArgumentError"
            )
            .then_some(ctor_name)
        }) {
            let msg = args.first().map_or_else(String::new, |v| {
                v.to_trace_string(self.swf_version, &self.avm1_objects)
            });
            obj.avm2_builtin_error = Some(err_kind.to_owned());
            obj.props
                .insert("name".to_owned(), Avm1Value::String(err_kind.to_owned()));
            obj.props
                .insert("message".to_owned(), Avm1Value::String(msg));
        } else if ctor_name == "Object"
            && args.len() == 1
            && matches!(args[0], Avm1Value::Object(_))
        {
            return args[0].clone();
        }
        self.avm1_objects.push(obj);
        Avm1Value::Object(obj_id)
    }

    pub(super) fn avm1_is_instance_of(&self, obj: &Avm1Value, constr: &Avm1Value) -> bool {
        match constr {
            Avm1Value::MovieClip(b) if b == "__builtin_MovieClip" => {
                matches!(
                    obj,
                    Avm1Value::MovieClip(mc) if !mc.starts_with("__builtin_") && mc != "Math" && mc != "_global"
                ) || matches!(obj, Avm1Value::MovieClipIdentity(_))
            }
            Avm1Value::MovieClip(b) if b == "__builtin_Array" => {
                matches!(obj, Avm1Value::Object(id) if self.avm1_objects.get(*id).is_some_and(|o| o.array_elements.is_some()))
            }
            Avm1Value::MovieClip(b)
                if b.strip_prefix("__builtin_").is_some_and(|k| {
                    matches!(
                        k,
                        "Error"
                            | "TypeError"
                            | "RangeError"
                            | "ReferenceError"
                            | "SyntaxError"
                            | "URIError"
                            | "EvalError"
                            | "ArgumentError"
                    )
                }) =>
            {
                let req_err = b.strip_prefix("__builtin_").unwrap_or("Error");
                if let Avm1Value::Object(mut cur_id) = *obj {
                    for _ in 0..32 {
                        let Some(o) = self.avm1_objects.get(cur_id) else {
                            break;
                        };
                        if let Some(actual) = o.avm2_builtin_error.as_deref()
                            && (req_err == "Error" || actual == req_err)
                        {
                            return true;
                        }
                        if let Some(Avm1Value::Object(pid)) = o.props.get("__proto__") {
                            cur_id = *pid;
                        } else {
                            break;
                        }
                    }
                }
                false
            }
            Avm1Value::MovieClip(b) if b == "__builtin_Function" => {
                matches!(obj, Avm1Value::Object(id) if self.avm1_objects.get(*id).is_some_and(|o| o.function.is_some()))
                    || matches!(obj, Avm1Value::MovieClip(mc) if mc.starts_with("__builtin_"))
            }
            Avm1Value::MovieClip(b) if b == "__builtin_Object" => {
                matches!(
                    obj,
                    Avm1Value::Object(_)
                        | Avm1Value::MovieClip(_)
                        | Avm1Value::MovieClipIdentity(_)
                )
            }
            Avm1Value::Object(ctor_id) => {
                let Avm1Value::Object(mut cur_id) = *obj else {
                    return false;
                };
                let target_proto_id = self
                    .avm1_objects
                    .get(*ctor_id)
                    .and_then(|o| o.props.get("prototype"))
                    .and_then(|v| match v {
                        Avm1Value::Object(pid) => Some(*pid),
                        _ => None,
                    });
                for _ in 0..32 {
                    let Some(cur_obj) = self.avm1_objects.get(cur_id) else {
                        break;
                    };
                    if cur_obj.interfaces.contains(ctor_id)
                        || target_proto_id.is_some_and(|tp| cur_obj.interfaces.contains(&tp))
                    {
                        return true;
                    }
                    let Some(Avm1Value::Object(proto_id)) = cur_obj.props.get("__proto__") else {
                        break;
                    };
                    if Some(*proto_id) == target_proto_id || proto_id == ctor_id {
                        return true;
                    }
                    if let Some(proto_obj) = self.avm1_objects.get(*proto_id)
                        && (proto_obj.interfaces.contains(ctor_id)
                            || target_proto_id.is_some_and(|tp| proto_obj.interfaces.contains(&tp)))
                    {
                        return true;
                    }
                    cur_id = *proto_id;
                }
                false
            }
            _ => false,
        }
    }

    pub(super) fn enumerate_avm1_value(&mut self, obj: &Avm1Value, stack: &mut Vec<Avm1Value>) {
        self.push_avm_stack(stack, Avm1Value::Null);
        match obj {
            Avm1Value::Object(id) => {
                let mut keys: Vec<String> = Vec::new();
                if let Some(o) = self.avm1_objects.get(*id) {
                    if let Some(elems) = &o.array_elements {
                        for idx in (0..elems.len()).rev() {
                            if !o.deleted_indices.contains(&idx) {
                                keys.push(idx.to_string());
                            }
                        }
                    }
                    let mut prop_keys: Vec<String> = o
                        .props
                        .keys()
                        .filter(|k| {
                            !matches!(
                                k.as_str(),
                                "__proto__"
                                    | "__constructor__"
                                    | "__super_ctor__"
                                    | "prototype"
                                    | "constructor"
                                    | "callee"
                            )
                        })
                        .cloned()
                        .collect();
                    prop_keys.sort();
                    keys.extend(prop_keys);
                }
                for k in keys {
                    self.push_avm_stack(stack, Avm1Value::String(k));
                }
            }
            Avm1Value::MovieClip(mc) | Avm1Value::MovieClipIdentity(mc) => {
                let target = self.canonical_movie_target(mc);
                let mut keys: Vec<String> = Vec::new();
                if target == "_root" {
                    keys.extend(self.avm1_globals.keys().cloned());
                    for (&d, s) in &self.display_list {
                        keys.push(if s.name.is_empty() {
                            format!("instance{d}")
                        } else {
                            s.name.clone()
                        });
                    }
                } else if let Some(slot) = self.target_slot(&target) {
                    keys.extend(slot.props.keys().cloned());
                    for (&d, s) in &slot.children {
                        keys.push(if s.name.is_empty() {
                            format!("instance{d}")
                        } else {
                            s.name.clone()
                        });
                    }
                }
                keys.sort();
                keys.dedup();
                for k in keys {
                    self.push_avm_stack(stack, Avm1Value::String(k));
                }
            }
            _ => {}
        }
    }
}

pub(super) fn format_avm_number(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_owned()
    } else if n.is_infinite() {
        if n.is_sign_positive() {
            "Infinity".to_owned()
        } else {
            "-Infinity".to_owned()
        }
    } else if n == 0.0 {
        "0".to_owned()
    } else if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

pub(super) fn avm1_string_to_number(s: &str, swf_version: u8) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return if swf_version <= 4 { 0.0 } else { f64::NAN };
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).map_or(f64::NAN, |v| v as f64);
    }
    if let Some(hex) = t.strip_prefix("-0x").or_else(|| t.strip_prefix("-0X")) {
        return u64::from_str_radix(hex, 16).map_or(f64::NAN, |v| -(v as f64));
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

pub(super) fn avm1_equals(
    a: &Avm1Value,
    b: &Avm1Value,
    swf_version: u8,
    _objects: &[Avm1Object],
) -> bool {
    match (a, b) {
        (Avm1Value::Undefined | Avm1Value::Null, Avm1Value::Undefined | Avm1Value::Null) => true,
        (Avm1Value::Undefined | Avm1Value::Null, _)
        | (_, Avm1Value::Undefined | Avm1Value::Null) => false,
        (Avm1Value::Number(x), Avm1Value::Number(y)) => x == y,
        (Avm1Value::Bool(x), Avm1Value::Bool(y)) => x == y,
        (Avm1Value::String(x), Avm1Value::String(y)) => x == y,
        (Avm1Value::Bool(x), other) => avm1_equals(
            &Avm1Value::Number(if *x { 1.0 } else { 0.0 }),
            other,
            swf_version,
            _objects,
        ),
        (other, Avm1Value::Bool(y)) => avm1_equals(
            other,
            &Avm1Value::Number(if *y { 1.0 } else { 0.0 }),
            swf_version,
            _objects,
        ),
        (Avm1Value::Number(x), Avm1Value::String(y))
        | (Avm1Value::String(y), Avm1Value::Number(x)) => {
            *x == avm1_string_to_number(y, swf_version)
        }
        (Avm1Value::Object(x), Avm1Value::Object(y)) => x == y,
        (Avm1Value::MovieClip(x), Avm1Value::MovieClip(y))
        | (Avm1Value::MovieClipIdentity(x), Avm1Value::MovieClipIdentity(y))
        | (Avm1Value::MovieClip(x), Avm1Value::MovieClipIdentity(y))
        | (Avm1Value::MovieClipIdentity(x), Avm1Value::MovieClip(y)) => x == y,
        _ => false,
    }
}

pub(super) fn parse_avm_int_radix(s: &str, radix: i32) -> f64 {
    let mut t = s.trim();
    if t.is_empty() {
        return f64::NAN;
    }
    let mut neg = false;
    if let Some(rest) = t.strip_prefix('-') {
        neg = true;
        t = rest;
    } else if let Some(rest) = t.strip_prefix('+') {
        t = rest;
    }
    let base: u32 = if radix == 0 {
        if let Some(rest) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
            t = rest;
            16
        } else {
            10
        }
    } else if (2..=36).contains(&radix) {
        let b = radix as u32;
        if b == 16
            && let Some(rest) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X"))
        {
            t = rest;
        }
        b
    } else {
        return f64::NAN;
    };
    let mut val = 0.0_f64;
    let mut any = false;
    for ch in t.chars() {
        let Some(d) = ch.to_digit(base) else {
            break;
        };
        any = true;
        val = val * f64::from(base) + f64::from(d);
    }
    if !any {
        f64::NAN
    } else if neg {
        -val
    } else {
        val
    }
}
