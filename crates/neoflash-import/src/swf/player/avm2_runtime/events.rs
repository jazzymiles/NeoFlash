//! AVM2 display-list stage attach/detach lifecycle and event dispatch.

use super::super::*;

impl SwfBytecodePlayer {
    pub(in crate::swf::player) fn resolve_avm2_symbol_character_id(
        &self,
        c_abc: usize,
        c_idx: usize,
    ) -> u16 {
        let mut cur = Some((c_abc, c_idx));
        for _ in 0..32 {
            let Some((a, c)) = cur else { break };
            if let Some(loaded) = self.abc_tags.get(a)
                && let Some(inst) = loaded.abc.instances.get(c)
            {
                let (pkg, cls_name) = loaded.abc.split_qname(inst.name);
                let qname = if pkg.is_empty() {
                    cls_name.clone()
                } else {
                    format!("{pkg}.{cls_name}")
                };
                if let Some((&cid, _)) = self.symbol_classes.iter().find(|(cid, bound)| {
                    **cid != 0 && (bound.as_str() == qname || bound.as_str() == cls_name)
                }) {
                    return cid;
                }
            }
            cur = self.avm2_superclass(a, c);
        }
        0
    }

    pub(in crate::swf::player) fn avm2_add_child_to_target(
        &mut self,
        parent_target: &str,
        child_arg: &Avm1Value,
    ) -> Avm1Value {
        let next_script_depth = if parent_target == "_root" || parent_target.is_empty() {
            self.display_list
                .keys()
                .copied()
                .filter(|&d| d >= 16384)
                .max()
                .map_or(0, |max_d| i32::from(max_d) - 16384 + 1)
        } else if let Some(pslot) = self.target_slot(parent_target) {
            pslot
                .children
                .keys()
                .copied()
                .filter(|&d| d >= 16384)
                .max()
                .map_or(0, |max_d| i32::from(max_d) - 16384 + 1)
        } else {
            return Avm1Value::Undefined;
        };
        match child_arg {
            Avm1Value::Object(oid) => {
                let existing_target = self
                    .avm1_objects
                    .get(*oid)
                    .and_then(|o| o.props.get("__display_target__").cloned())
                    .and_then(|v| match v {
                        Avm1Value::MovieClipIdentity(t) => Some(t),
                        _ => None,
                    });
                if let Some(old_t) = existing_target
                    && !old_t.is_empty()
                    && old_t != "_root"
                {
                    let _ = self.remove_display_child_with_tombstone(&old_t, true);
                }
                let (name, x, y, sx, sy, alpha, visible, rotation, char_id, gfx_opt) = self
                    .avm1_objects
                    .get(*oid)
                    .map(|o| {
                        let nm = o
                            .props
                            .get("name")
                            .map(|v| avm2_convert_string(v, &self.avm1_objects))
                            .unwrap_or_default();
                        let px = o.props.get("x").map_or(0.0, avm2_to_number);
                        let py = o.props.get("y").map_or(0.0, avm2_to_number);
                        let psx = o.props.get("scaleX").map_or(1.0, avm2_to_number) * 100.0;
                        let psy = o.props.get("scaleY").map_or(1.0, avm2_to_number) * 100.0;
                        let pa = o.props.get("alpha").map_or(1.0, avm2_to_number) * 100.0;
                        let pv = o.props.get("visible").is_none_or(|v| v.to_bool(9));
                        let pr = o.props.get("rotation").map_or(0.0, avm2_to_number);
                        let cid = o
                            .avm2_class
                            .map_or(0, |(ca, cc)| self.resolve_avm2_symbol_character_id(ca, cc));
                        (nm, px, py, psx, psy, pa, pv, pr, cid, o.graphics.clone())
                    })
                    .unwrap_or_else(|| {
                        (
                            String::new(),
                            0.0,
                            0.0,
                            100.0,
                            100.0,
                            100.0,
                            true,
                            0.0,
                            0,
                            None,
                        )
                    });
                let created =
                    self.create_empty_movie_clip_in_target(parent_target, &name, next_script_depth);
                if let Avm1Value::MovieClipIdentity(ref new_id) = created {
                    let sprite_total = if char_id != 0 {
                        self.sprites
                            .get(&char_id)
                            .map_or(1, |s| s.total_frames.max(1))
                    } else {
                        1
                    };
                    if let Some(slot) = self.target_slot_mut(new_id) {
                        slot.character_id = char_id;
                        slot.total_frames = sprite_total;
                        slot.avm2_object_id = Some(*oid);
                        if char_id != 0 {
                            slot.last_applied_frame = None;
                        }
                        slot.x = if x.is_finite() { x } else { 0.0 };
                        slot.y = if y.is_finite() { y } else { 0.0 };
                        slot.scale_x = if sx.is_finite() { sx } else { 100.0 };
                        slot.scale_y = if sy.is_finite() { sy } else { 100.0 };
                        slot.alpha = if alpha.is_finite() { alpha } else { 100.0 };
                        slot.visible = visible;
                        slot.rotation = if rotation.is_finite() { rotation } else { 0.0 };
                        if let Some(gfx) = gfx_opt {
                            slot.graphics = gfx;
                        }
                        slot.sync_matrix_from_props();
                    }
                    if char_id != 0 {
                        let mut pending = Vec::new();
                        let mut diags = Vec::new();
                        let sprites = self.sprites.clone();
                        let swf_ver = self.swf_version;
                        let mut next_id = self.next_slot_instance_id;
                        if let Some(slot) = self.target_slot_mut(new_id) {
                            Self::sync_sprite_slot_recursive(
                                slot,
                                &sprites,
                                swf_ver,
                                false,
                                0,
                                new_id,
                                &mut pending,
                                &mut diags,
                                &mut next_id,
                            );
                        }
                        self.next_slot_instance_id = next_id;
                        for d in diags {
                            self.record_unsupported_diagnostic(d);
                        }
                        self.execute_avm1_work_list(pending);
                    }
                    if let Some(obj) = self.avm1_objects.get_mut(*oid) {
                        obj.props.insert(
                            "__display_target__".to_owned(),
                            Avm1Value::MovieClipIdentity(new_id.clone()),
                        );
                    }
                    let parent_oid = if parent_target == "_root" || parent_target.is_empty() {
                        self.avm2_root_oid
                    } else {
                        self.target_slot(parent_target)
                            .and_then(|s| s.avm2_object_id)
                    };
                    if let Some(p_oid) = parent_oid {
                        self.attach_avm2_child_oid_to_parent(p_oid, *oid);
                        let mut ev_steps = 0usize;
                        self.dispatch_avm2_event_on_oid(
                            *oid,
                            "added",
                            true,
                            false,
                            &mut ev_steps,
                            0,
                        );
                        let is_on_stage = self
                            .avm1_objects
                            .get(*oid)
                            .and_then(|o| o.avm2_display.as_ref())
                            .is_some_and(|m| m.on_stage);
                        if is_on_stage {
                            self.dispatch_avm2_subtree_stage_event(
                                *oid,
                                "addedToStage",
                                &mut ev_steps,
                                0,
                            );
                        }
                    }
                }
                child_arg.clone()
            }
            Avm1Value::MovieClip(_) | Avm1Value::MovieClipIdentity(_) => child_arg.clone(),
            _ => Avm1Value::Undefined,
        }
    }

    pub(in crate::swf::player) fn avm2_remove_child_value(
        &mut self,
        child_arg: &Avm1Value,
    ) -> Avm1Value {
        let resolved = match child_arg {
            Avm1Value::MovieClipIdentity(id) => id.clone(),
            Avm1Value::MovieClip(mc) => self.canonical_movie_target(mc),
            Avm1Value::Object(oid) => self
                .avm1_objects
                .get(*oid)
                .and_then(|o| o.props.get("__display_target__"))
                .and_then(|v| match v {
                    Avm1Value::MovieClipIdentity(id) => Some(id.clone()),
                    _ => None,
                })
                .unwrap_or_default(),
            _ => String::new(),
        };
        let target_oid = match child_arg {
            Avm1Value::Object(oid) => Some(*oid),
            _ if !resolved.is_empty() && resolved != "_root" => {
                self.target_slot(&resolved).and_then(|s| s.avm2_object_id)
            }
            _ => None,
        };
        if !resolved.is_empty() && resolved != "_root" {
            if let Some(c_oid) = target_oid {
                self.detach_avm2_child_oid(c_oid);
            }
            let _ = self.remove_display_child_with_tombstone(&resolved, true);
            if let Some(c_oid) = target_oid
                && let Some(obj) = self.avm1_objects.get_mut(c_oid)
            {
                obj.props.remove("__display_target__");
            }
            return child_arg.clone();
        }
        if let Some(c_oid) = target_oid {
            let had_parent = self
                .avm1_objects
                .get(c_oid)
                .and_then(|o| o.avm2_display.as_ref())
                .is_some_and(|m| m.parent_oid.is_some());
            if had_parent {
                self.detach_avm2_child_oid(c_oid);
                return child_arg.clone();
            }
        }
        Avm1Value::Null
    }

    pub(in crate::swf::player) fn has_active_avm2_tick_listeners(&self) -> bool {
        if self.vm_kind != SwfVmKind::Avm2 || !self.avm2_initialized {
            return false;
        }
        self.avm1_objects.iter().any(|obj| {
            obj.avm2_display.as_ref().is_some_and(|m| {
                m.on_stage
                    && (m.listeners.get("enterFrame").is_some_and(|v| !v.is_empty())
                        || m.listeners
                            .get("frameConstructed")
                            .is_some_and(|v| !v.is_empty())
                        || m.listeners.get("exitFrame").is_some_and(|v| !v.is_empty()))
            })
        })
    }

    pub(in crate::swf::player) fn set_avm2_subtree_on_stage(
        &mut self,
        root_oid: usize,
        on_stage: bool,
    ) {
        let mut stack = vec![root_oid];
        let mut visited = BTreeSet::new();
        while let Some(oid) = stack.pop() {
            if !visited.insert(oid) {
                continue;
            }
            if let Some(meta) = self
                .avm1_objects
                .get_mut(oid)
                .and_then(|o| o.avm2_display.as_mut())
            {
                meta.on_stage = on_stage;
                for &child_oid in meta.children_oids.iter().rev() {
                    stack.push(child_oid);
                }
            }
        }
    }

    pub(in crate::swf::player) fn attach_avm2_child_oid_to_parent(
        &mut self,
        parent_oid: usize,
        child_oid: usize,
    ) {
        if let Some(old_parent) = self
            .avm1_objects
            .get(child_oid)
            .and_then(|o| o.avm2_display.as_ref())
            .and_then(|m| m.parent_oid)
            && let Some(om) = self
                .avm1_objects
                .get_mut(old_parent)
                .and_then(|o| o.avm2_display.as_mut())
        {
            om.children_oids.retain(|&c| c != child_oid);
        }
        let parent_on_stage = self
            .avm1_objects
            .get(parent_oid)
            .and_then(|o| o.avm2_display.as_ref())
            .is_some_and(|m| m.on_stage);
        if let Some(pm) = self
            .avm1_objects
            .get_mut(parent_oid)
            .and_then(|o| o.avm2_display.as_mut())
            && !pm.children_oids.contains(&child_oid)
        {
            pm.children_oids.push(child_oid);
        }
        if let Some(cm) = self
            .avm1_objects
            .get_mut(child_oid)
            .and_then(|o| o.avm2_display.as_mut())
        {
            cm.parent_oid = Some(parent_oid);
        }
        self.set_avm2_subtree_on_stage(child_oid, parent_on_stage);
    }

    pub(in crate::swf::player) fn detach_avm2_child_oid(&mut self, child_oid: usize) {
        let (old_parent, was_on_stage, explicit_name) = self
            .avm1_objects
            .get(child_oid)
            .and_then(|o| o.avm2_display.as_ref())
            .map_or((None, false, None), |m| {
                (m.parent_oid, m.on_stage, m.explicit_timeline_name.clone())
            });
        let mut steps = 0usize;
        if old_parent.is_some() {
            self.dispatch_avm2_event_on_oid(child_oid, "removed", true, false, &mut steps, 0);
        }
        if was_on_stage {
            self.dispatch_avm2_subtree_stage_event(child_oid, "removedFromStage", &mut steps, 0);
        }
        if let Some(p_oid) = old_parent {
            if let Some(pm) = self
                .avm1_objects
                .get_mut(p_oid)
                .and_then(|o| o.avm2_display.as_mut())
            {
                pm.children_oids.retain(|&c| c != child_oid);
            }
            if let Some(nm) = explicit_name
                && self
                    .avm1_objects
                    .get(p_oid)
                    .and_then(|o| o.props.get(&nm))
                    .is_some_and(|v| matches!(v, Avm1Value::Object(id) if *id == child_oid))
            {
                self.set_avm2_property_eval(
                    &Avm1Value::Object(p_oid),
                    &nm,
                    Avm1Value::Null,
                    &mut steps,
                    0,
                );
            }
        }
        if let Some(cm) = self
            .avm1_objects
            .get_mut(child_oid)
            .and_then(|o| o.avm2_display.as_mut())
        {
            cm.parent_oid = None;
        }
        self.set_avm2_subtree_on_stage(child_oid, false);
    }

    pub(in crate::swf::player) fn dispatch_avm2_subtree_stage_event(
        &mut self,
        root_oid: usize,
        event_type: &str,
        steps: &mut usize,
        depth: usize,
    ) {
        let mut order = Vec::new();
        self.collect_avm2_constructed_preorder(root_oid, &mut order, &mut BTreeSet::new());
        for oid in order {
            self.dispatch_avm2_event_on_oid(oid, event_type, false, false, steps, depth);
        }
    }

    pub(in crate::swf::player) fn collect_avm2_constructed_preorder(
        &self,
        oid: usize,
        out: &mut Vec<usize>,
        visited: &mut BTreeSet<usize>,
    ) {
        if !visited.insert(oid) {
            return;
        }
        let Some(meta) = self
            .avm1_objects
            .get(oid)
            .and_then(|o| o.avm2_display.as_ref())
        else {
            return;
        };
        if !meta.constructed {
            return;
        }
        out.push(oid);
        for &child_oid in &meta.children_oids {
            self.collect_avm2_constructed_preorder(child_oid, out, visited);
        }
    }

    pub(in crate::swf::player) fn dispatch_avm2_broadcast_event(&mut self, event_type: &str) {
        let Some(stage_oid) = self.avm2_stage_oid else {
            return;
        };
        let mut order = Vec::new();
        self.collect_avm2_constructed_preorder(stage_oid, &mut order, &mut BTreeSet::new());
        let mut steps = 0usize;
        for oid in order {
            self.dispatch_avm2_event_on_oid(oid, event_type, false, false, &mut steps, 0);
        }
    }

    pub(in crate::swf::player) fn dispatch_avm2_event_on_oid(
        &mut self,
        target_oid: usize,
        event_type: &str,
        bubbles: bool,
        cancelable: bool,
        steps: &mut usize,
        depth: usize,
    ) {
        if depth > MAX_CALL_DEPTH {
            return;
        }
        let mut chain = vec![(target_oid, 2u8)];
        if bubbles {
            let mut cur = self
                .avm1_objects
                .get(target_oid)
                .and_then(|o| o.avm2_display.as_ref())
                .and_then(|m| m.parent_oid);
            for _ in 0..64 {
                let Some(anc) = cur else { break };
                chain.push((anc, 3u8));
                cur = self
                    .avm1_objects
                    .get(anc)
                    .and_then(|o| o.avm2_display.as_ref())
                    .and_then(|m| m.parent_oid);
            }
        }
        let any_listener = chain.iter().any(|&(oid, _)| {
            self.avm1_objects
                .get(oid)
                .and_then(|o| o.avm2_display.as_ref())
                .and_then(|m| m.listeners.get(event_type))
                .is_some_and(|list| !list.is_empty())
        });
        if !any_listener {
            return;
        }

        let ev_oid = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object {
            avm2_event: Some((
                event_type.to_owned(),
                bubbles,
                cancelable,
                target_oid,
                target_oid,
                2,
            )),
            ..Default::default()
        });

        for (cur_oid, phase) in chain {
            let callbacks = self
                .avm1_objects
                .get(cur_oid)
                .and_then(|o| o.avm2_display.as_ref())
                .and_then(|m| m.listeners.get(event_type))
                .cloned()
                .unwrap_or_default();
            if callbacks.is_empty() {
                continue;
            }
            if let Some(ev_obj) = self.avm1_objects.get_mut(ev_oid) {
                ev_obj.avm2_event = Some((
                    event_type.to_owned(),
                    bubbles,
                    cancelable,
                    target_oid,
                    cur_oid,
                    phase,
                ));
            }
            for cb in callbacks {
                let _ = self.invoke_avm2_callable_with_this(
                    &cb,
                    Avm1Value::Object(cur_oid),
                    vec![Avm1Value::Object(ev_oid)],
                    steps,
                    depth + 1,
                );
            }
        }
    }

    pub(in crate::swf::player) fn run_avm2_frame_phase(&mut self, steps: &mut usize, depth: usize) {
        if depth > MAX_CALL_DEPTH {
            return;
        }
        let Some(stage_oid) = self.avm2_stage_oid else {
            // Fallback for synthetic ABC tests without Stage
            if let Some(callable) = self.avm2_frame_scripts.get(&self.current_frame).cloned() {
                let _ = self.invoke_avm2_callable_with_this(
                    &callable,
                    Avm1Value::MovieClip("_root".to_owned()),
                    Vec::new(),
                    steps,
                    depth + 1,
                );
            }
            return;
        };

        // 1. Event.FRAME_CONSTRUCTED
        let mut fc_order = Vec::new();
        self.collect_avm2_constructed_preorder(stage_oid, &mut fc_order, &mut BTreeSet::new());
        for oid in fc_order {
            self.dispatch_avm2_event_on_oid(oid, "frameConstructed", false, false, steps, depth);
        }

        // 2. Pending addFrameScript callbacks on constructed stage MovieClips
        let mut fs_order = Vec::new();
        self.collect_avm2_constructed_preorder(stage_oid, &mut fs_order, &mut BTreeSet::new());
        for oid in fs_order {
            let callable_opt = {
                let Some(meta) = self
                    .avm1_objects
                    .get_mut(oid)
                    .and_then(|o| o.avm2_display.as_mut())
                else {
                    continue;
                };
                if !meta.constructed || !meta.on_stage || meta.display_type != "MovieClip" {
                    continue;
                }
                let cur_f = if Some(oid) == self.avm2_root_oid {
                    self.current_frame
                } else {
                    meta.current_frame
                };
                if meta.last_ran_frame_script == Some(cur_f) {
                    continue;
                }
                let found = meta.frame_scripts.get(&cur_f).cloned().or_else(|| {
                    (Some(oid) == self.avm2_root_oid)
                        .then(|| self.avm2_frame_scripts.get(&cur_f).cloned())
                        .flatten()
                });
                if found.is_some() {
                    meta.last_ran_frame_script = Some(cur_f);
                }
                found
            };
            if let Some(callable) = callable_opt {
                let _ = self.invoke_avm2_callable_with_this(
                    &callable,
                    Avm1Value::Object(oid),
                    Vec::new(),
                    steps,
                    depth + 1,
                );
            }
        }

        // 3. Event.EXIT_FRAME
        let mut ef_order = Vec::new();
        self.collect_avm2_constructed_preorder(stage_oid, &mut ef_order, &mut BTreeSet::new());
        for oid in ef_order {
            self.dispatch_avm2_event_on_oid(oid, "exitFrame", false, false, steps, depth);
        }
    }
}
