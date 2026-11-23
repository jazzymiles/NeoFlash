//! AVM2 timeline child construction, sprite frame synchronization, and `gotoAndPlay`/`Stop`.

use super::super::*;

impl SwfBytecodePlayer {
    pub(in crate::swf::player) fn run_avm2_builtin_display_super(
        &mut self,
        oid: usize,
        steps: &mut usize,
        depth: usize,
    ) {
        let Some((dt, cid, already)) = self
            .avm1_objects
            .get(oid)
            .and_then(|o| o.avm2_display.as_ref())
            .map(|m| (m.display_type.clone(), m.character_id, m.super_constructed))
        else {
            return;
        };
        if already {
            return;
        }
        if let Some(meta) = self
            .avm1_objects
            .get_mut(oid)
            .and_then(|o| o.avm2_display.as_mut())
        {
            meta.super_constructed = true;
        }
        if dt == "MovieClip" || dt == "Sprite" {
            if Some(oid) == self.avm2_root_oid {
                self.construct_avm2_children_for_container(oid, "_root", steps, depth);
            } else if let Some(Avm1Value::MovieClipIdentity(target)) = self
                .avm1_objects
                .get(oid)
                .and_then(|o| o.props.get("__display_target__").cloned())
            {
                self.construct_avm2_children_for_container(oid, &target, steps, depth);
            } else if cid != 0 {
                self.construct_avm2_offtree_sprite_children(oid, cid, steps, depth);
            }
        }
    }

    pub(in crate::swf::player) fn allocate_avm2_timeline_object(
        &mut self,
        cid: u16,
        authored_name: &str,
        target_opt: Option<&str>,
        parent_oid: Option<usize>,
    ) -> usize {
        let bound_class = self.symbol_classes.get(&cid).and_then(|bound| {
            let (pkg, short) = match bound.rsplit_once('.') {
                Some((p, s)) => (p, s),
                None => ("", bound.as_str()),
            };
            (0..self.abc_tags.len()).find_map(|ai| self.find_avm2_class(ai, pkg, short))
        });

        let base_kind = self
            .char_kinds
            .get(&cid)
            .copied()
            .or_else(|| {
                bound_class
                    .and_then(|(a, c)| self.avm2_display_type_for_class(a, c))
                    .map(|(dt, _)| dt)
            })
            .unwrap_or("Shape");
        let short_class_name = bound_class
            .and_then(|(a, c)| {
                self.abc_tags
                    .get(a)
                    .and_then(|l| l.abc.instances.get(c))
                    .map(|inst| self.abc_tags[a].abc.split_qname(inst.name).1)
            })
            .unwrap_or_else(|| base_kind.to_owned());

        let inst_name = if !authored_name.is_empty() {
            authored_name.to_owned()
        } else {
            self.next_avm2_instance_name()
        };

        let mut props = if let Some((ca, cc)) = bound_class {
            self.collect_avm2_instance_slot_defaults(ca, cc)
        } else {
            HashMap::new()
        };
        let (x, y, sx, sy, alpha, visible, rotation) = target_opt
            .and_then(|t| self.target_slot(t))
            .map_or((0.0, 0.0, 1.0, 1.0, 1.0, true, 0.0), |s| {
                (
                    s.x,
                    s.y,
                    s.scale_x / 100.0,
                    s.scale_y / 100.0,
                    s.alpha / 100.0,
                    s.visible,
                    s.rotation,
                )
            });
        props.insert("name".to_owned(), Avm1Value::String(inst_name));
        props.insert("x".to_owned(), Avm1Value::Number(x));
        props.insert("y".to_owned(), Avm1Value::Number(y));
        props.insert("scaleX".to_owned(), Avm1Value::Number(sx));
        props.insert("scaleY".to_owned(), Avm1Value::Number(sy));
        props.insert("alpha".to_owned(), Avm1Value::Number(alpha));
        props.insert("visible".to_owned(), Avm1Value::Bool(visible));
        props.insert("rotation".to_owned(), Avm1Value::Number(rotation));
        if let Some(t) = target_opt {
            props.insert(
                "__display_target__".to_owned(),
                Avm1Value::MovieClipIdentity(t.to_owned()),
            );
        }

        let parent_on_stage = parent_oid
            .and_then(|pid| self.avm1_objects.get(pid)?.avm2_display.as_ref())
            .is_some_and(|m| m.on_stage);
        let total_frames = self.sprites.get(&cid).map_or(1, |s| s.total_frames.max(1));

        let oid = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object {
            props,
            avm2_class: bound_class,
            avm2_display: Some(Avm2DisplayMeta {
                display_type: base_kind.to_owned(),
                short_class_name,
                character_id: cid,
                parent_oid,
                on_stage: parent_on_stage,
                constructed: false,
                super_constructed: false,
                explicit_timeline_name: (!authored_name.is_empty())
                    .then(|| authored_name.to_owned()),
                current_frame: 0,
                total_frames,
                playing: true,
                ..Default::default()
            }),
            graphics: Some(RuntimeGraphicsState::default()),
            ..Default::default()
        });
        oid
    }

    fn ensure_avm2_sprite_slot_applied(&mut self, target: &str) {
        if target == "_root" || target.is_empty() {
            return;
        }
        let Some((cid, prev_applied, cur_frame)) = self
            .target_slot(target)
            .map(|s| (s.character_id, s.last_applied_frame, s.current_frame))
        else {
            return;
        };
        if prev_applied == Some(cur_frame) {
            return;
        }
        let Some(def) = self.sprites.get(&cid).cloned() else {
            if let Some(s) = self.target_slot_mut(target) {
                s.last_applied_frame = Some(cur_frame);
            }
            return;
        };
        let sprites = self.sprites.clone();
        let swf_ver = self.swf_version;
        let mut unmodeled_any = false;
        let mut next_id = self.next_slot_instance_id;
        if let Some(slot) = self.target_slot_mut(target) {
            slot.last_applied_frame = Some(cur_frame);
            slot.entered_this_step = true;
            if prev_applied.is_some_and(|prev| cur_frame <= prev) {
                slot.looped_this_step = true;
            }
            slot.pending_audio_frames.push(cur_frame);
            let start_frame = match prev_applied {
                Some(prev) if cur_frame > prev => prev + 1,
                _ => {
                    slot.children.clear();
                    slot.removed_timeline_depths.clear();
                    0
                }
            };
            let max_f = cur_frame.min(def.frames.len().saturating_sub(1) as u32);
            for f in start_frame..=max_f {
                if let Some(tags) = def.frames.get(f as usize) {
                    for tag in tags {
                        match tag.code {
                            4 | 26 | 70 => {
                                let (_, uf) = apply_place_tag_to_map(
                                    &mut slot.children,
                                    &mut slot.removed_timeline_depths,
                                    &sprites,
                                    swf_ver,
                                    tag.code,
                                    &tag.payload,
                                    &mut next_id,
                                );
                                unmodeled_any |= uf;
                            }
                            5 | 28 => {
                                let _ = apply_remove_tag_to_map(
                                    &mut slot.children,
                                    &mut slot.removed_timeline_depths,
                                    tag.code,
                                    &tag.payload,
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        self.next_slot_instance_id = next_id;
        if unmodeled_any {
            self.record_unsupported_diagnostic(
                "SWF PlaceObject3 filter list contained an unmodeled filter".to_owned(),
            );
        }
    }

    pub(in crate::swf::player) fn construct_avm2_children_for_container(
        &mut self,
        parent_oid: usize,
        parent_target: &str,
        steps: &mut usize,
        depth: usize,
    ) {
        if depth > MAX_CALL_DEPTH {
            return;
        }
        self.ensure_avm2_sprite_slot_applied(parent_target);

        let pending_slots: Vec<(u16, u16, String)> = if parent_target == "_root" {
            self.display_list
                .iter()
                .filter(|(_, s)| s.avm2_object_id.is_none())
                .map(|(&d, s)| (d, s.character_id, s.name.clone()))
                .collect()
        } else if let Some(pslot) = self.target_slot(parent_target) {
            pslot
                .children
                .iter()
                .filter(|(_, s)| s.avm2_object_id.is_none())
                .map(|(&d, s)| (d, s.character_id, s.name.clone()))
                .collect()
        } else {
            Vec::new()
        };

        if pending_slots.is_empty() {
            return;
        }

        // Phase 1: Allocate all unconstructed children at this frame with `constructed = false`
        let mut allocated = Vec::with_capacity(pending_slots.len());
        for (child_depth, cid, authored_name) in pending_slots {
            let child_target = if parent_target == "_root" {
                format!("depth{child_depth}")
            } else {
                join_target_path(parent_target, &format!("depth{child_depth}"))
            };
            let child_oid = self.allocate_avm2_timeline_object(
                cid,
                &authored_name,
                Some(&child_target),
                Some(parent_oid),
            );
            if parent_target == "_root" {
                if let Some(s) = self.display_list.get_mut(&child_depth) {
                    s.avm2_object_id = Some(child_oid);
                }
            } else if let Some(pslot) = self.target_slot_mut(parent_target)
                && let Some(s) = pslot.children.get_mut(&child_depth)
            {
                s.avm2_object_id = Some(child_oid);
            }
            if let Some(pm) = self
                .avm1_objects
                .get_mut(parent_oid)
                .and_then(|o| o.avm2_display.as_mut())
            {
                pm.children_oids.push(child_oid);
            }
            allocated.push((child_oid, child_target, cid, authored_name));
        }

        // Phase 2: Construct each child in depth order
        for (child_oid, child_target, cid, authored_name) in allocated {
            self.construct_avm2_timeline_child(
                parent_oid,
                child_oid,
                Some(&child_target),
                cid,
                &authored_name,
                true,
                steps,
                depth + 1,
            );
        }
    }

    pub(in crate::swf::player) fn construct_avm2_offtree_sprite_children(
        &mut self,
        parent_oid: usize,
        sprite_cid: u16,
        steps: &mut usize,
        depth: usize,
    ) {
        if depth > MAX_CALL_DEPTH {
            return;
        }
        let Some(def) = self.sprites.get(&sprite_cid).cloned() else {
            return;
        };
        let mut temp_map: BTreeMap<u16, LiveSlot> = BTreeMap::new();
        let mut temp_removed: BTreeSet<u16> = BTreeSet::new();
        let mut temp_next_id = 1u64;
        if let Some(tags) = def.frames.first() {
            for tag in tags {
                match tag.code {
                    4 | 26 | 70 => {
                        let _ = apply_place_tag_to_map(
                            &mut temp_map,
                            &mut temp_removed,
                            &self.sprites,
                            self.swf_version,
                            tag.code,
                            &tag.payload,
                            &mut temp_next_id,
                        );
                    }
                    5 | 28 => {
                        let _ = apply_remove_tag_to_map(
                            &mut temp_map,
                            &mut temp_removed,
                            tag.code,
                            &tag.payload,
                        );
                    }
                    _ => {}
                }
            }
        }
        let mut allocated = Vec::with_capacity(temp_map.len());
        for (_, s) in temp_map {
            let child_oid =
                self.allocate_avm2_timeline_object(s.character_id, &s.name, None, Some(parent_oid));
            if let Some(pm) = self
                .avm1_objects
                .get_mut(parent_oid)
                .and_then(|o| o.avm2_display.as_mut())
            {
                pm.children_oids.push(child_oid);
            }
            allocated.push((child_oid, s.character_id, s.name));
        }
        for (child_oid, cid, authored_name) in allocated {
            self.construct_avm2_timeline_child(
                parent_oid,
                child_oid,
                None,
                cid,
                &authored_name,
                true,
                steps,
                depth + 1,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::swf::player) fn construct_avm2_timeline_child(
        &mut self,
        parent_oid: usize,
        child_oid: usize,
        child_target_opt: Option<&str>,
        cid: u16,
        authored_name: &str,
        dispatch_added_on_self: bool,
        steps: &mut usize,
        depth: usize,
    ) {
        if depth > MAX_CALL_DEPTH {
            return;
        }
        let (dt, bound_cls) = {
            let Some(obj) = self.avm1_objects.get_mut(child_oid) else {
                return;
            };
            let bc = obj.avm2_class;
            let Some(meta) = obj.avm2_display.as_mut() else {
                return;
            };
            meta.constructed = true;
            (meta.display_type.clone(), bc)
        };

        if dt == "SimpleButton" {
            let records = self.button_records.get(&cid).cloned().unwrap_or_default();
            let mut instantiated_records = Vec::with_capacity(records.len());
            for (states_mask, rec_cid, _rec_depth) in records {
                let rec_oid =
                    self.allocate_avm2_timeline_object(rec_cid, "", None, Some(child_oid));
                self.construct_avm2_timeline_child(
                    child_oid,
                    rec_oid,
                    None,
                    rec_cid,
                    "",
                    false,
                    steps,
                    depth + 1,
                );
                instantiated_records.push((states_mask, rec_oid));
            }

            let mut up_oids = Vec::new();
            let mut over_oids = Vec::new();
            let mut down_oids = Vec::new();
            let mut hit_oids = Vec::new();
            for &(states_mask, rec_oid) in &instantiated_records {
                if (states_mask & 0x01) != 0 {
                    up_oids.push(rec_oid);
                } else {
                    if let Some(rm) = self
                        .avm1_objects
                        .get_mut(rec_oid)
                        .and_then(|o| o.avm2_display.as_mut())
                    {
                        rm.parent_oid = None;
                    }
                    self.set_avm2_subtree_on_stage(rec_oid, false);
                }
                if (states_mask & 0x02) != 0 {
                    over_oids.push(rec_oid);
                }
                if (states_mask & 0x04) != 0 {
                    down_oids.push(rec_oid);
                }
                if (states_mask & 0x08) != 0 {
                    hit_oids.push(rec_oid);
                }
            }

            if let Some(&first_up) = up_oids.first() {
                if let Some(bm) = self
                    .avm1_objects
                    .get_mut(child_oid)
                    .and_then(|o| o.avm2_display.as_mut())
                {
                    bm.children_oids.push(first_up);
                }
                if let Some(bo) = self.avm1_objects.get_mut(child_oid) {
                    bo.props
                        .insert("upState".to_owned(), Avm1Value::Object(first_up));
                }
                self.dispatch_avm2_event_on_oid(first_up, "added", true, false, steps, depth + 1);
                let btn_on_stage = self
                    .avm1_objects
                    .get(child_oid)
                    .and_then(|o| o.avm2_display.as_ref())
                    .is_some_and(|m| m.on_stage);
                if btn_on_stage {
                    self.dispatch_avm2_subtree_stage_event(
                        first_up,
                        "addedToStage",
                        steps,
                        depth + 1,
                    );
                }
            }
            for (prop_name, list) in [
                ("overState", over_oids),
                ("downState", down_oids),
                ("hitTestState", hit_oids),
            ] {
                if let Some(&first_oid) = list.first()
                    && let Some(bo) = self.avm1_objects.get_mut(child_oid)
                {
                    bo.props
                        .insert(prop_name.to_owned(), Avm1Value::Object(first_oid));
                }
            }

            // Flash Player executes a nested frame phase before running SimpleButton's iinit
            self.run_avm2_frame_phase(steps, depth + 1);

            if let Some((ca, cc)) = bound_cls {
                let iinit = self.abc_tags[ca].abc.instances[cc].iinit;
                let _ = self.run_avm2_method_with_this(
                    ca,
                    iinit,
                    Avm1Value::Object(child_oid),
                    Vec::new(),
                    steps,
                    depth + 1,
                );
            }
        } else if dt == "MovieClip" || dt == "Sprite" {
            if let Some((ca, cc)) = bound_cls {
                let iinit = self.abc_tags[ca].abc.instances[cc].iinit;
                let _ = self.run_avm2_method_with_this(
                    ca,
                    iinit,
                    Avm1Value::Object(child_oid),
                    Vec::new(),
                    steps,
                    depth + 1,
                );
            }
            let need_children = self
                .avm1_objects
                .get(child_oid)
                .and_then(|o| o.avm2_display.as_ref())
                .is_some_and(|m| !m.super_constructed);
            if need_children {
                if let Some(meta) = self
                    .avm1_objects
                    .get_mut(child_oid)
                    .and_then(|o| o.avm2_display.as_mut())
                {
                    meta.super_constructed = true;
                }
                if let Some(t) = child_target_opt {
                    self.construct_avm2_children_for_container(child_oid, t, steps, depth + 1);
                } else if cid != 0 {
                    self.construct_avm2_offtree_sprite_children(child_oid, cid, steps, depth + 1);
                }
            }
        } else if let Some((ca, cc)) = bound_cls {
            let iinit = self.abc_tags[ca].abc.instances[cc].iinit;
            let _ = self.run_avm2_method_with_this(
                ca,
                iinit,
                Avm1Value::Object(child_oid),
                Vec::new(),
                steps,
                depth + 1,
            );
        }

        if !authored_name.is_empty() {
            self.set_avm2_property_eval(
                &Avm1Value::Object(parent_oid),
                authored_name,
                Avm1Value::Object(child_oid),
                steps,
                depth + 1,
            );
        }

        if dispatch_added_on_self {
            self.dispatch_avm2_event_on_oid(child_oid, "added", true, false, steps, depth + 1);
            let is_on_stage = self
                .avm1_objects
                .get(child_oid)
                .and_then(|o| o.avm2_display.as_ref())
                .is_some_and(|m| m.on_stage);
            if is_on_stage {
                self.dispatch_avm2_event_on_oid(
                    child_oid,
                    "addedToStage",
                    false,
                    false,
                    steps,
                    depth + 1,
                );
            }
        }
    }

    pub(in crate::swf::player) fn sync_avm2_container_with_display_list(
        &mut self,
        parent_oid: usize,
        parent_target: &str,
        steps: &mut usize,
        depth: usize,
    ) {
        self.ensure_avm2_sprite_slot_applied(parent_target);

        let live_oids: BTreeSet<usize> = if parent_target == "_root" {
            self.display_list
                .values()
                .filter_map(|s| s.avm2_object_id)
                .collect()
        } else if let Some(pslot) = self.target_slot(parent_target) {
            pslot
                .children
                .values()
                .filter_map(|s| s.avm2_object_id)
                .collect()
        } else {
            BTreeSet::new()
        };

        let existing_children: Vec<usize> = self
            .avm1_objects
            .get(parent_oid)
            .and_then(|o| o.avm2_display.as_ref())
            .map_or_else(Vec::new, |m| m.children_oids.clone());

        for c_oid in existing_children {
            let has_disp_target = self
                .avm1_objects
                .get(c_oid)
                .is_some_and(|o| o.props.contains_key("__display_target__"));
            if has_disp_target && !live_oids.contains(&c_oid) {
                self.detach_avm2_child_oid(c_oid);
                if let Some(co) = self.avm1_objects.get_mut(c_oid) {
                    co.props.remove("__display_target__");
                }
            }
        }

        self.construct_avm2_children_for_container(parent_oid, parent_target, steps, depth);
    }

    pub(in crate::swf::player) fn avm2_goto_frame(
        &mut self,
        oid: usize,
        target_frame: u32,
        playing: bool,
        steps: &mut usize,
        depth: usize,
    ) {
        if Some(oid) == self.avm2_root_oid {
            let total = self.root_timeline.frames.len().max(1) as u32;
            let clamped = target_frame.min(total.saturating_sub(1));
            let prev = self.last_applied_frame.unwrap_or(self.current_frame);
            self.playing = playing;
            self.current_frame = clamped;
            if let Some(meta) = self
                .avm1_objects
                .get_mut(oid)
                .and_then(|o| o.avm2_display.as_mut())
            {
                meta.playing = playing;
                if meta.current_frame != clamped {
                    meta.current_frame = clamped;
                    meta.last_ran_frame_script = None;
                }
            }
            if prev == clamped {
                return;
            }
            let start_frame = if clamped < prev {
                self.display_list.clear();
                self.removed_timeline_depths.clear();
                0
            } else {
                prev + 1
            };
            for f in start_frame..=clamped {
                if let Some(tags) = self.root_timeline.frames.get(f as usize).cloned() {
                    for tag in tags {
                        match tag.code {
                            4 | 26 | 70 => {
                                let _ = self.apply_place_tag(tag.code, &tag.payload);
                            }
                            5 | 28 => {
                                let _ = self.apply_remove_tag(tag.code, &tag.payload);
                            }
                            _ => {}
                        }
                    }
                }
                if f < clamped {
                    let inter_depths: Vec<u16> = self.display_list.keys().copied().collect();
                    for d in inter_depths {
                        self.step_sprite_frame_actions(d, f > start_frame);
                    }
                }
            }
            self.apply_root_audio(clamped);
            self.last_applied_frame = Some(clamped);
            self.avm2_sync_goto_handled = true;
            self.sync_avm2_container_with_display_list(oid, "_root", steps, depth);
            self.run_avm2_frame_phase(steps, depth + 1);
            return;
        }

        let Some(disp_target) = self
            .avm1_objects
            .get(oid)
            .and_then(|o| o.props.get("__display_target__"))
            .and_then(|v| match v {
                Avm1Value::MovieClipIdentity(t) => Some(t.clone()),
                _ => None,
            })
        else {
            if let Some(meta) = self
                .avm1_objects
                .get_mut(oid)
                .and_then(|o| o.avm2_display.as_mut())
            {
                let total = meta.total_frames.max(1);
                let clamped = target_frame.min(total.saturating_sub(1));
                meta.playing = playing;
                if meta.current_frame != clamped {
                    meta.current_frame = clamped;
                    meta.last_ran_frame_script = None;
                }
            }
            return;
        };

        let Some((total, prev_frame)) = self
            .target_slot(&disp_target)
            .map(|s| (s.total_frames.max(1), s.current_frame))
        else {
            return;
        };
        let clamped = target_frame.min(total.saturating_sub(1));
        if let Some(slot) = self.target_slot_mut(&disp_target) {
            slot.playing = playing;
            slot.current_frame = clamped;
        }
        if let Some(meta) = self
            .avm1_objects
            .get_mut(oid)
            .and_then(|o| o.avm2_display.as_mut())
        {
            meta.playing = playing;
            if meta.current_frame != clamped {
                meta.current_frame = clamped;
                meta.last_ran_frame_script = None;
            }
        }
        if prev_frame == clamped {
            return;
        }
        self.sync_avm2_container_with_display_list(oid, &disp_target, steps, depth);
        self.run_avm2_frame_phase(steps, depth + 1);
    }

    pub(in crate::swf::player) fn advance_avm2_nested_timelines(&mut self) {
        let Some(stage_oid) = self.avm2_stage_oid else {
            return;
        };
        let mut order = Vec::new();
        self.collect_avm2_constructed_preorder(stage_oid, &mut order, &mut BTreeSet::new());
        let mut steps = 0usize;
        for oid in order {
            if Some(oid) == self.avm2_stage_oid || Some(oid) == self.avm2_root_oid {
                continue;
            }
            let Some((playing, total, cur, target_opt)) =
                self.avm1_objects.get(oid).and_then(|o| {
                    let m = o.avm2_display.as_ref()?;
                    if m.display_type != "MovieClip" || !m.on_stage || !m.constructed {
                        return None;
                    }
                    let t = o.props.get("__display_target__").and_then(|v| match v {
                        Avm1Value::MovieClipIdentity(s) => Some(s.clone()),
                        _ => None,
                    });
                    Some((m.playing, m.total_frames, m.current_frame, t))
                })
            else {
                continue;
            };
            if !playing || total <= 1 {
                continue;
            }
            if let Some(ref target) = target_opt
                && self
                    .target_slot(target)
                    .is_some_and(|slot| slot.placed_this_step)
            {
                continue;
            }
            let next_f = (cur + 1) % total;
            if let Some(meta) = self
                .avm1_objects
                .get_mut(oid)
                .and_then(|o| o.avm2_display.as_mut())
            {
                meta.current_frame = next_f;
                meta.last_ran_frame_script = None;
            }
            if let Some(target) = target_opt {
                if let Some(slot) = self.target_slot_mut(&target) {
                    slot.current_frame = next_f;
                }
                self.sync_avm2_container_with_display_list(oid, &target, &mut steps, 0);
            }
        }
    }

    pub(in crate::swf::player) fn call_avm2_display_builtin_method(
        &mut self,
        oid: usize,
        prop: &str,
        args: &[Avm1Value],
        steps: &mut usize,
        depth: usize,
    ) -> Option<Avm1Value> {
        match prop {
            "addEventListener" => {
                let ev_ty = args
                    .first()
                    .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                if !ev_ty.is_empty()
                    && let Some(listener) = args.get(1)
                    && !matches!(listener, Avm1Value::Undefined | Avm1Value::Null)
                    && let Some(meta) = self
                        .avm1_objects
                        .get_mut(oid)
                        .and_then(|o| o.avm2_display.as_mut())
                {
                    let list = meta.listeners.entry(ev_ty).or_default();
                    if !list.iter().any(|existing| match (existing, listener) {
                        (Avm1Value::String(a), Avm1Value::String(b)) => a == b,
                        (Avm1Value::Object(a), Avm1Value::Object(b)) => a == b,
                        _ => false,
                    }) {
                        list.push(listener.clone());
                    }
                }
                Some(Avm1Value::Undefined)
            }
            "removeEventListener" => {
                let ev_ty = args
                    .first()
                    .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                if let Some(listener) = args.get(1)
                    && let Some(meta) = self
                        .avm1_objects
                        .get_mut(oid)
                        .and_then(|o| o.avm2_display.as_mut())
                    && let Some(list) = meta.listeners.get_mut(&ev_ty)
                {
                    list.retain(|existing| match (existing, listener) {
                        (Avm1Value::String(a), Avm1Value::String(b)) => a != b,
                        (Avm1Value::Object(a), Avm1Value::Object(b)) => a != b,
                        _ => true,
                    });
                }
                Some(Avm1Value::Undefined)
            }
            "hasEventListener" | "willTrigger" => {
                let ev_ty = args
                    .first()
                    .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                let has = self
                    .avm1_objects
                    .get(oid)
                    .and_then(|o| o.avm2_display.as_ref())
                    .and_then(|m| m.listeners.get(&ev_ty))
                    .is_some_and(|l| !l.is_empty());
                Some(Avm1Value::Bool(has))
            }
            "dispatchEvent" => {
                if let Some(Avm1Value::Object(ev_oid)) = args.first()
                    && let Some((ev_ty, bubbles, cancelable, _, _, _)) = self
                        .avm1_objects
                        .get(*ev_oid)
                        .and_then(|o| o.avm2_event.clone())
                {
                    self.dispatch_avm2_event_on_oid(oid, &ev_ty, bubbles, cancelable, steps, depth);
                    return Some(Avm1Value::Bool(true));
                }
                Some(Avm1Value::Bool(false))
            }
            "addFrameScript" => {
                for pair in args.as_chunks::<2>().0 {
                    let f_idx = ecma_f64_to_i32(avm2_to_number(&pair[0])).max(0) as u32;
                    match &pair[1] {
                        Avm1Value::Null | Avm1Value::Undefined => {
                            if let Some(meta) = self
                                .avm1_objects
                                .get_mut(oid)
                                .and_then(|o| o.avm2_display.as_mut())
                            {
                                meta.frame_scripts.remove(&f_idx);
                            }
                            if Some(oid) == self.avm2_root_oid {
                                self.avm2_frame_scripts.remove(&f_idx);
                            }
                        }
                        callable => {
                            if let Some(meta) = self
                                .avm1_objects
                                .get_mut(oid)
                                .and_then(|o| o.avm2_display.as_mut())
                            {
                                meta.frame_scripts.insert(f_idx, callable.clone());
                            }
                        }
                    }
                }
                Some(Avm1Value::Undefined)
            }
            "getChildAt" => {
                let idx = args
                    .first()
                    .map_or(-1, |v| ecma_f64_to_i32(avm2_to_number(v)));
                if idx < 0 {
                    return Some(Avm1Value::Null);
                }
                let child_opt = self
                    .avm1_objects
                    .get(oid)
                    .and_then(|o| o.avm2_display.as_ref())
                    .and_then(|m| m.children_oids.get(idx as usize).copied())
                    .filter(|&c_oid| {
                        self.avm1_objects
                            .get(c_oid)
                            .and_then(|co| co.avm2_display.as_ref())
                            .is_some_and(|cm| cm.constructed)
                    });
                Some(child_opt.map_or(Avm1Value::Null, Avm1Value::Object))
            }
            "getChildByName" => {
                let want = args
                    .first()
                    .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                if want.is_empty() {
                    return Some(Avm1Value::Null);
                }
                let found = self
                    .avm1_objects
                    .get(oid)
                    .and_then(|o| o.avm2_display.as_ref())
                    .and_then(|m| {
                        m.children_oids.iter().copied().find(|&c_oid| {
                            self.avm1_objects.get(c_oid).is_some_and(|co| {
                                co.avm2_display.as_ref().is_some_and(|cm| cm.constructed)
                                    && matches!(co.props.get("name"), Some(Avm1Value::String(s)) if s == &want)
                            })
                        })
                    });
                Some(found.map_or(Avm1Value::Null, Avm1Value::Object))
            }
            "getChildIndex" => {
                if let Some(Avm1Value::Object(want_oid)) = args.first()
                    && let Some(pos) = self
                        .avm1_objects
                        .get(oid)
                        .and_then(|o| o.avm2_display.as_ref())
                        .and_then(|m| m.children_oids.iter().position(|&c| c == *want_oid))
                {
                    return Some(Avm1Value::Number(pos as f64));
                }
                Some(Avm1Value::Number(-1.0))
            }
            "contains" => {
                let Some(Avm1Value::Object(mut cur)) = args.first().cloned() else {
                    return Some(Avm1Value::Bool(false));
                };
                for _ in 0..64 {
                    if cur == oid {
                        return Some(Avm1Value::Bool(true));
                    }
                    let Some(next_p) = self
                        .avm1_objects
                        .get(cur)
                        .and_then(|o| o.avm2_display.as_ref())
                        .and_then(|m| m.parent_oid)
                    else {
                        break;
                    };
                    cur = next_p;
                }
                Some(Avm1Value::Bool(false))
            }
            "addChild" => {
                let Some(child_arg) = args.first() else {
                    return Some(Avm1Value::Undefined);
                };
                if Some(oid) == self.avm2_root_oid {
                    return Some(self.avm2_add_child_to_target("_root", child_arg));
                }
                if let Some(Avm1Value::MovieClipIdentity(disp_target)) = self
                    .avm1_objects
                    .get(oid)
                    .and_then(|o| o.props.get("__display_target__").cloned())
                {
                    return Some(self.avm2_add_child_to_target(&disp_target, child_arg));
                }
                if let Avm1Value::Object(c_oid) = child_arg {
                    self.attach_avm2_child_oid_to_parent(oid, *c_oid);
                    self.dispatch_avm2_event_on_oid(*c_oid, "added", true, false, steps, depth);
                    let on_st = self
                        .avm1_objects
                        .get(*c_oid)
                        .and_then(|o| o.avm2_display.as_ref())
                        .is_some_and(|m| m.on_stage);
                    if on_st {
                        self.dispatch_avm2_subtree_stage_event(
                            *c_oid,
                            "addedToStage",
                            steps,
                            depth,
                        );
                    }
                }
                Some(child_arg.clone())
            }
            "removeChild" => {
                let Some(child_arg) = args.first() else {
                    return Some(Avm1Value::Undefined);
                };
                Some(self.avm2_remove_child_value(child_arg))
            }
            "stop" => {
                if Some(oid) == self.avm2_root_oid {
                    self.playing = false;
                } else if let Some(Avm1Value::MovieClipIdentity(t)) = self
                    .avm1_objects
                    .get(oid)
                    .and_then(|o| o.props.get("__display_target__").cloned())
                    && let Some(slot) = self.target_slot_mut(&t)
                {
                    slot.playing = false;
                }
                if let Some(meta) = self
                    .avm1_objects
                    .get_mut(oid)
                    .and_then(|o| o.avm2_display.as_mut())
                {
                    meta.playing = false;
                }
                Some(Avm1Value::Undefined)
            }
            "play" => {
                if Some(oid) == self.avm2_root_oid {
                    self.playing = true;
                } else if let Some(Avm1Value::MovieClipIdentity(t)) = self
                    .avm1_objects
                    .get(oid)
                    .and_then(|o| o.props.get("__display_target__").cloned())
                    && let Some(slot) = self.target_slot_mut(&t)
                {
                    slot.playing = true;
                }
                if let Some(meta) = self
                    .avm1_objects
                    .get_mut(oid)
                    .and_then(|o| o.avm2_display.as_mut())
                {
                    meta.playing = true;
                }
                Some(Avm1Value::Undefined)
            }
            "nextFrame" | "prevFrame" => {
                let cur = if Some(oid) == self.avm2_root_oid {
                    self.current_frame
                } else {
                    self.avm1_objects
                        .get(oid)
                        .and_then(|o| o.avm2_display.as_ref())
                        .map_or(0, |m| m.current_frame)
                };
                let next_f = if prop == "nextFrame" {
                    cur.saturating_add(1)
                } else {
                    cur.saturating_sub(1)
                };
                self.avm2_goto_frame(oid, next_f, false, steps, depth);
                Some(Avm1Value::Undefined)
            }
            "gotoAndStop" | "gotoAndPlay" => {
                let want_play = prop == "gotoAndPlay";
                if let Some(first) = args.first() {
                    if Some(oid) == self.avm2_root_oid {
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
                        let resolved = match first {
                            Avm1Value::Number(n) if *n >= 1.0 => {
                                Some(scene_offset + ((*n as i64).saturating_sub(1).max(0) as u32))
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
                            self.avm2_goto_frame(oid, f, want_play, steps, depth);
                        }
                    } else {
                        let cid = self
                            .avm1_objects
                            .get(oid)
                            .and_then(|o| o.avm2_display.as_ref())
                            .map_or(0, |m| m.character_id);
                        let resolved = match first {
                            Avm1Value::Number(n) if *n >= 1.0 => {
                                Some((*n as i64).saturating_sub(1).max(0) as u32)
                            }
                            Avm1Value::String(lbl) => self
                                .sprites
                                .get(&cid)
                                .and_then(|def| def.resolve_avm2_label(lbl)),
                            _ => None,
                        };
                        if let Some(f) = resolved {
                            self.avm2_goto_frame(oid, f, want_play, steps, depth);
                        }
                    }
                }
                Some(Avm1Value::Undefined)
            }
            "toString" => Some(Avm1Value::String(
                Avm1Value::Object(oid).to_trace_string(9, &self.avm1_objects),
            )),
            _ => None,
        }
    }
}
