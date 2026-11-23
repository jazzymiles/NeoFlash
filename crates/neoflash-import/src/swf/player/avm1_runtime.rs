//! AVM1 display target resolution, clip mutation, variable lookup, and member access.

use super::*;

mod calls;

impl SwfBytecodePlayer {
    pub(super) fn resolve_avm1_target_value(&self, val: &Avm1Value) -> String {
        match val {
            Avm1Value::MovieClipIdentity(id) => id.clone(),
            Avm1Value::MovieClip(mc) => self.canonical_movie_target(mc),
            Avm1Value::Object(oid) => {
                if let Some(Avm1Value::MovieClipIdentity(id)) = self
                    .avm1_objects
                    .get(*oid)
                    .and_then(|o| o.props.get("__display_target__"))
                {
                    return id.clone();
                }
                let s = val.to_trace_string(self.swf_version, &self.avm1_objects);
                self.canonical_movie_target(&s)
            }
            _ => {
                let s = val.to_trace_string(self.swf_version, &self.avm1_objects);
                self.canonical_movie_target(&s)
            }
        }
    }

    pub(super) fn target_dot_path(&self, target: &str) -> Option<String> {
        if target.is_empty() || target == "_root" || target == "_level0" {
            return Some("_level0".to_owned());
        }
        let mut cur_map = &self.display_list;
        let mut names = Vec::new();
        for seg in target.split('/').filter(|s| !s.is_empty()) {
            let depth: u16 = seg.strip_prefix("depth")?.parse().ok()?;
            let slot = cur_map.get(&depth)?;
            if slot.name.is_empty() {
                names.push(format!("instance{depth}"));
            } else {
                names.push(slot.name.clone());
            }
            cur_map = &slot.children;
        }
        Some(format!("_level0.{}", names.join(".")))
    }

    pub(super) fn target_slash_path(&self, target: &str) -> String {
        if target.is_empty() || target == "_root" || target == "_level0" {
            return "/".to_owned();
        }
        let mut cur_map = &self.display_list;
        let mut names = Vec::new();
        for seg in target.split('/').filter(|s| !s.is_empty()) {
            if let Some(d_str) = seg.strip_prefix("depth")
                && let Ok(depth) = d_str.parse::<u16>()
                && let Some(slot) = cur_map.get(&depth)
            {
                if slot.name.is_empty() {
                    names.push(format!("instance{depth}"));
                } else {
                    names.push(slot.name.clone());
                }
                cur_map = &slot.children;
            } else {
                names.push(seg.to_owned());
            }
        }
        format!("/{}", names.join("/"))
    }

    pub(super) fn apply_dynamic_mask(&mut self, maskee_target: &str, mask_arg: Option<&Avm1Value>) {
        if maskee_target.is_empty() || maskee_target == "_root" {
            return;
        }
        let Some(arg) = mask_arg else {
            return;
        };
        if matches!(arg, Avm1Value::Null | Avm1Value::Undefined)
            || matches!(arg, Avm1Value::String(s) if s.is_empty())
        {
            if let Some(slot) = self.target_slot_mut(maskee_target) {
                slot.masked_by = None;
            }
            return;
        }
        let mask_target = self.resolve_avm1_target_value(arg);
        if mask_target.is_empty()
            || mask_target == "_root"
            || mask_target == maskee_target
            || self.target_slot(&mask_target).is_none()
        {
            return;
        }
        // Flash 1-to-1 dynamic mask invariant: clear any previous maskee using `mask_target`.
        clear_mask_source_in_map(&mut self.display_list, &mask_target);
        if let Some(mask_slot) = self.target_slot_mut(&mask_target) {
            mask_slot.masked_by = None;
        }
        if let Some(slot) = self.target_slot_mut(maskee_target) {
            slot.masked_by = Some(mask_target);
        }
    }

    pub(super) fn create_empty_movie_clip_in_target(
        &mut self,
        parent_target: &str,
        name: &str,
        script_depth: i32,
    ) -> Avm1Value {
        let swf_depth = script_depth_to_swf(script_depth);
        let instance_id = alloc_instance_id(&mut self.next_slot_instance_id);
        let new_slot = LiveSlot {
            instance_id,
            character_id: 0,
            name: name.to_owned(),
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            x: 0.0,
            y: 0.0,
            scale_x: 100.0,
            scale_y: 100.0,
            rotation: 0.0,
            skew: 0.0,
            alpha: 100.0,
            color_transform: ColorTransform::default(),
            visible: true,
            blend_mode: BlendMode::Normal,
            filters: Vec::new(),
            clip_depth: None,
            masked_by: None,
            is_dynamic: true,
            origin_timeline_depth: None,
            ratio: None,
            cache_as_bitmap: false,
            current_frame: 0,
            total_frames: 1,
            playing: true,
            last_applied_frame: Some(0),
            pending_audio_frames: Vec::new(),
            looped_this_step: false,
            entered_this_step: false,
            removed_timeline_depths: BTreeSet::new(),
            children: BTreeMap::new(),
            props: HashMap::new(),
            clip_handlers: Vec::new(),
            graphics: RuntimeGraphicsState::default(),
            placed_this_step: false,
            avm2_object_id: None,
        };
        let seg = format!("depth{swf_depth}");
        let mut unload_work = None;
        if parent_target == "_root" || parent_target.is_empty() {
            if let Some(old) = self.display_list.get(&swf_depth) {
                unload_work = detached_unload_work(old, &seg);
            }
            self.display_list.insert(swf_depth, new_slot);
            self.prune_dangling_masks();
            if let Some(w) = unload_work {
                self.execute_avm1_work(w);
            }
            Avm1Value::MovieClipIdentity(seg)
        } else {
            let new_id = join_target_path(parent_target, &seg);
            if let Some(parent_slot) = self.target_slot_mut(parent_target) {
                if let Some(old) = parent_slot.children.get(&swf_depth) {
                    unload_work = detached_unload_work(old, &new_id);
                }
                parent_slot.children.insert(swf_depth, new_slot);
            } else {
                return Avm1Value::Undefined;
            }
            self.prune_dangling_masks();
            if let Some(w) = unload_work {
                self.execute_avm1_work(w);
            }
            Avm1Value::MovieClipIdentity(new_id)
        }
    }

    pub(super) fn attach_movie_in_target(
        &mut self,
        parent_target: &str,
        linkage_id: &str,
        new_name: &str,
        script_depth: i32,
        init_obj: Option<&Avm1Value>,
    ) -> Avm1Value {
        let Some(&character_id) = self.exports.get(linkage_id) else {
            return Avm1Value::Undefined;
        };
        let swf_depth = script_depth_to_swf(script_depth);
        let total_frames = self
            .sprites
            .get(&character_id)
            .map_or(1, |s| s.total_frames.max(1));
        let instance_id = alloc_instance_id(&mut self.next_slot_instance_id);
        let new_slot = LiveSlot {
            instance_id,
            character_id,
            name: new_name.to_owned(),
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            x: 0.0,
            y: 0.0,
            scale_x: 100.0,
            scale_y: 100.0,
            rotation: 0.0,
            skew: 0.0,
            alpha: 100.0,
            color_transform: ColorTransform::default(),
            visible: true,
            blend_mode: BlendMode::Normal,
            filters: Vec::new(),
            clip_depth: None,
            masked_by: None,
            is_dynamic: true,
            origin_timeline_depth: None,
            ratio: None,
            cache_as_bitmap: false,
            current_frame: 0,
            total_frames,
            playing: true,
            last_applied_frame: None,
            pending_audio_frames: Vec::new(),
            looped_this_step: false,
            entered_this_step: false,
            removed_timeline_depths: BTreeSet::new(),
            children: BTreeMap::new(),
            props: HashMap::new(),
            clip_handlers: Vec::new(),
            graphics: RuntimeGraphicsState::default(),
            placed_this_step: false,
            avm2_object_id: None,
        };
        let seg = format!("depth{swf_depth}");
        let new_id = if parent_target == "_root" || parent_target.is_empty() {
            seg.clone()
        } else {
            join_target_path(parent_target, &seg)
        };
        let mut unload_work = None;
        if parent_target == "_root" || parent_target.is_empty() {
            if let Some(old) = self.display_list.get(&swf_depth) {
                unload_work = detached_unload_work(old, &seg);
            }
            self.display_list.insert(swf_depth, new_slot);
        } else if let Some(parent_slot) = self.target_slot_mut(parent_target) {
            if let Some(old) = parent_slot.children.get(&swf_depth) {
                unload_work = detached_unload_work(old, &new_id);
            }
            parent_slot.children.insert(swf_depth, new_slot);
        } else {
            return Avm1Value::Undefined;
        }
        self.prune_dangling_masks();
        if let Some(w) = unload_work {
            self.execute_avm1_work(w);
        }
        if let Some(Avm1Value::Object(init_id)) = init_obj
            && let Some(props) = self.avm1_objects.get(*init_id).map(|o| o.props.clone())
        {
            let mc_ref = Avm1Value::MovieClipIdentity(new_id.clone());
            for (k, v) in props {
                self.set_avm1_member(&mc_ref, &k, v);
            }
        }
        let mut pending = Vec::new();
        let mut diags = Vec::new();
        let sprites = self.sprites.clone();
        let swf_ver = self.swf_version;
        let mut next_id = self.next_slot_instance_id;
        if let Some(slot) = self.target_slot_mut(&new_id) {
            Self::sync_sprite_slot_recursive(
                slot,
                &sprites,
                swf_ver,
                false,
                0,
                &new_id,
                &mut pending,
                &mut diags,
                &mut next_id,
            );
        }
        self.next_slot_instance_id = next_id;
        for d in diags {
            self.record_unsupported_diagnostic(d);
        }
        if let Some(&ctor_id) = self.avm1_registered_classes.get(linkage_id) {
            let proto = self.get_avm1_member(&Avm1Value::Object(ctor_id), "prototype");
            if let Some(slot) = self.target_slot_mut(&new_id) {
                slot.props.insert("__proto__".to_owned(), proto);
            }
            if let Some(func) = self
                .avm1_objects
                .get(ctor_id)
                .and_then(|o| o.function.clone())
            {
                let super_ctor = self
                    .avm1_objects
                    .get(ctor_id)
                    .and_then(|o| o.props.get("__super_ctor__"))
                    .and_then(|v| match v {
                        Avm1Value::Object(sid) => Some(*sid),
                        _ => None,
                    });
                let mut steps = 0usize;
                let _ = self.invoke_avm1_func_with_this(
                    &func,
                    Avm1Value::MovieClipIdentity(new_id.clone()),
                    super_ctor,
                    Vec::new(),
                    &mut steps,
                    0,
                );
            }
        }
        self.execute_avm1_work_list(pending);
        Avm1Value::MovieClipIdentity(new_id)
    }

    pub(super) fn duplicate_movie_clip_at_swf_depth(
        &mut self,
        src_target: &str,
        new_name: &str,
        swf_depth: u16,
    ) -> Avm1Value {
        if src_target.is_empty() || src_target == "_root" {
            return Avm1Value::Undefined;
        }
        let Some(src_slot) = self.target_slot(src_target).cloned() else {
            return Avm1Value::Undefined;
        };
        let parent_target = src_target
            .rsplit_once('/')
            .map_or("_root", |(parent, _)| parent)
            .to_owned();
        let instance_id = alloc_instance_id(&mut self.next_slot_instance_id);
        let cloned = LiveSlot {
            instance_id,
            character_id: src_slot.character_id,
            name: new_name.to_owned(),
            matrix: src_slot.matrix,
            x: src_slot.x,
            y: src_slot.y,
            scale_x: src_slot.scale_x,
            scale_y: src_slot.scale_y,
            rotation: src_slot.rotation,
            skew: src_slot.skew,
            alpha: src_slot.alpha,
            color_transform: src_slot.color_transform,
            visible: src_slot.visible,
            blend_mode: src_slot.blend_mode,
            filters: src_slot.filters,
            clip_depth: None,
            masked_by: None,
            is_dynamic: true,
            origin_timeline_depth: None,
            ratio: src_slot.ratio,
            cache_as_bitmap: src_slot.cache_as_bitmap,
            current_frame: 0,
            total_frames: src_slot.total_frames,
            playing: true,
            last_applied_frame: if src_slot.character_id == 0 {
                Some(0)
            } else {
                None
            },
            pending_audio_frames: Vec::new(),
            looped_this_step: false,
            entered_this_step: false,
            removed_timeline_depths: BTreeSet::new(),
            children: BTreeMap::new(),
            props: HashMap::new(),
            clip_handlers: src_slot.clip_handlers.clone(),
            graphics: src_slot.graphics.clone(),
            placed_this_step: false,
            avm2_object_id: None,
        };
        let seg = format!("depth{swf_depth}");
        let new_id = if parent_target == "_root" {
            seg.clone()
        } else {
            join_target_path(&parent_target, &seg)
        };
        let mut unload_work = None;
        if parent_target == "_root" {
            if let Some(old) = self.display_list.get(&swf_depth) {
                unload_work = detached_unload_work(old, &seg);
            }
            self.display_list.insert(swf_depth, cloned);
        } else if let Some(parent_slot) = self.target_slot_mut(&parent_target) {
            if let Some(old) = parent_slot.children.get(&swf_depth) {
                unload_work = detached_unload_work(old, &new_id);
            }
            parent_slot.children.insert(swf_depth, cloned);
        } else {
            return Avm1Value::Undefined;
        }
        self.prune_dangling_masks();
        if let Some(w) = unload_work {
            self.execute_avm1_work(w);
        }
        let mut load_work = Vec::new();
        for h in &src_slot.clip_handlers {
            if (h.event_flags & CLIP_EVENT_INITIALIZE) != 0 {
                load_work.push(pending_action(new_id.clone(), h.actions.clone()));
            }
        }
        for h in &src_slot.clip_handlers {
            if (h.event_flags & CLIP_EVENT_CONSTRUCT) != 0 {
                load_work.push(pending_action(new_id.clone(), h.actions.clone()));
            }
        }
        for h in &src_slot.clip_handlers {
            if (h.event_flags & CLIP_EVENT_LOAD) != 0 {
                load_work.push(pending_action(new_id.clone(), h.actions.clone()));
            }
        }
        self.execute_avm1_work_list(load_work);
        let mut pending = Vec::new();
        let mut diags = Vec::new();
        let sprites = self.sprites.clone();
        let swf_ver = self.swf_version;
        let mut next_id = self.next_slot_instance_id;
        if let Some(slot) = self.target_slot_mut(&new_id) {
            Self::sync_sprite_slot_recursive(
                slot,
                &sprites,
                swf_ver,
                false,
                0,
                &new_id,
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
        Avm1Value::MovieClipIdentity(new_id)
    }

    pub(super) fn remove_movie_clip_identity(&mut self, target: &str) {
        let _ = self.remove_display_child_with_tombstone(target, false);
    }

    pub(super) fn remove_display_child_with_tombstone(
        &mut self,
        target: &str,
        allow_timeline_depth: bool,
    ) -> bool {
        if target.is_empty() || target == "_root" {
            return false;
        }
        let (parent_target, last_seg) = target
            .rsplit_once('/')
            .map_or(("_root", target), |(p, s)| (p, s));
        let Some(depth) = last_seg
            .strip_prefix("depth")
            .and_then(|d| d.parse::<u16>().ok())
        else {
            return false;
        };
        let mut unload_work = None;
        let removed = if parent_target == "_root" {
            if self
                .display_list
                .get(&depth)
                .is_some_and(|s| allow_timeline_depth || s.is_dynamic || depth >= 16384)
            {
                if let Some(slot) = self.display_list.remove(&depth) {
                    if let Some(orig_d) = slot.origin_timeline_depth {
                        self.removed_timeline_depths.insert(orig_d);
                    }
                    unload_work = detached_unload_work(&slot, target);
                }
                true
            } else {
                false
            }
        } else if let Some(parent_slot) = self.target_slot_mut(parent_target) {
            if parent_slot
                .children
                .get(&depth)
                .is_some_and(|s| allow_timeline_depth || s.is_dynamic || depth >= 16384)
            {
                if let Some(slot) = parent_slot.children.remove(&depth) {
                    if let Some(orig_d) = slot.origin_timeline_depth {
                        parent_slot.removed_timeline_depths.insert(orig_d);
                    }
                    unload_work = detached_unload_work(&slot, target);
                }
                true
            } else {
                false
            }
        } else {
            false
        };
        if removed {
            self.prune_dangling_masks();
            if let Some(w) = unload_work {
                self.execute_avm1_work(w);
            }
        }
        removed
    }

    pub(super) fn target_slot(&self, target: &str) -> Option<&LiveSlot> {
        if let Some((root_target, root_slot)) = &self.avm1_detached_target
            && let Some(slot) = find_slot_in_detached(root_target, root_slot, target)
        {
            return Some(slot);
        }
        find_slot_by_identity(&self.display_list, target)
    }

    pub(super) fn target_slot_mut(&mut self, target: &str) -> Option<&mut LiveSlot> {
        if let Some((root_target, root_slot)) = &mut self.avm1_detached_target
            && (target == root_target || target.starts_with(&format!("{root_target}/")))
        {
            return find_slot_in_detached_mut(root_target, root_slot, target);
        }
        find_slot_by_identity_mut(&mut self.display_list, target)
    }

    pub(super) fn canonical_movie_target(&self, target: &str) -> String {
        let target = target.trim();
        if target.is_empty() || target == "this" {
            return if self.avm1_target.is_empty() {
                "_root".to_owned()
            } else {
                self.avm1_target.clone()
            };
        }
        if target == "_root" || target == "_level0" || target == "/" {
            return "_root".to_owned();
        }
        if target == "_parent" || target == ".." {
            let cur = if self.avm1_target.is_empty() {
                "_root"
            } else {
                &self.avm1_target
            };
            return if cur == "_root" {
                String::new()
            } else if let Some((parent, _)) = cur.rsplit_once('/') {
                parent.to_owned()
            } else {
                "_root".to_owned()
            };
        }
        let explicitly_rooted = target.starts_with("_root.")
            || target.starts_with("_level0.")
            || target.starts_with('/');
        let normalized = target
            .trim_start_matches("_root.")
            .trim_start_matches("_level0.")
            .trim_start_matches('/');
        if let Some((root_target, root_slot)) = &self.avm1_detached_target
            && find_slot_in_detached(root_target, root_slot, normalized).is_some()
        {
            return normalized.to_owned();
        }
        let segments = target_segments(normalized);
        if !explicitly_rooted
            && !self.avm1_target.is_empty()
            && self.avm1_target != "_root"
            && let Some(current) = self.target_slot(&self.avm1_target)
            && let Some(identity) =
                resolve_target_identity_by_segments(&current.children, &segments, &self.avm1_target)
        {
            return identity;
        }
        if let Some(identity) = resolve_target_identity(&self.display_list, normalized) {
            return identity;
        }
        normalized.to_owned()
    }

    pub(super) fn current_target_path(&self) -> String {
        self.canonical_movie_target("")
    }

    pub(super) fn target_total_frames(&self, target: &str) -> u32 {
        if target == "_root" || target.is_empty() {
            self.root_timeline.frames.len().max(1) as u32
        } else {
            self.target_slot(target).map_or(1, |slot| slot.total_frames)
        }
    }

    pub(super) fn target_label_frame(&self, target: &str, label: &str) -> Option<u32> {
        if target == "_root" || target.is_empty() {
            self.root_timeline.resolve_avm1_label(label)
        } else {
            let character_id = self.target_slot(target)?.character_id;
            self.sprites.get(&character_id)?.resolve_avm1_label(label)
        }
    }

    pub(super) fn set_target_goto_value_identity(
        &mut self,
        target: &str,
        value: &Avm1Value,
        playing: bool,
    ) {
        let total = self.target_total_frames(target);
        let resolved = match value {
            Avm1Value::Number(number) if *number >= 1.0 => {
                Some((*number as i64).saturating_sub(1).max(0) as u32)
            }
            Avm1Value::String(label) => {
                if let Ok(number) = label.trim().parse::<u32>()
                    && number >= 1
                {
                    Some(number.saturating_sub(1))
                } else {
                    self.target_label_frame(target, label)
                }
            }
            _ => None,
        };
        let Some(frame) = resolved.map(|frame| frame.min(total.saturating_sub(1))) else {
            return;
        };
        self.set_target_frame_zero_based(target, frame, playing);
    }

    pub(super) fn set_target_frame_zero_based(&mut self, target: &str, frame: u32, playing: bool) {
        if target == "_root" || target.is_empty() {
            self.current_frame = frame;
            self.playing = playing;
        } else if let Some(slot) = self.target_slot_mut(target) {
            slot.current_frame = frame;
            slot.playing = playing;
        }
    }

    pub(super) fn avm1_has_member(&self, target: &Avm1Value, member: &str) -> bool {
        match target {
            Avm1Value::Object(id) => {
                let mut cur_id = *id;
                for _ in 0..32 {
                    let Some(obj) = self.avm1_objects.get(cur_id) else {
                        break;
                    };
                    if obj.props.contains_key(member) {
                        return true;
                    }
                    if let Some(elems) = &obj.array_elements
                        && (member == "length"
                            || member.parse::<usize>().is_ok_and(|idx| {
                                idx < elems.len() && !obj.deleted_indices.contains(&idx)
                            }))
                    {
                        return true;
                    }
                    if let Some(Avm1Value::Object(proto_id)) = obj.props.get("__proto__") {
                        cur_id = *proto_id;
                    } else {
                        break;
                    }
                }
                false
            }
            Avm1Value::MovieClip(mc) | Avm1Value::MovieClipIdentity(mc) => {
                if matches!(
                    member,
                    "_x" | "_y"
                        | "_xscale"
                        | "_yscale"
                        | "_currentframe"
                        | "_totalframes"
                        | "_framesloaded"
                        | "_alpha"
                        | "_visible"
                        | "_rotation"
                        | "_name"
                        | "_target"
                        | "_parent"
                ) {
                    return true;
                }
                let canonical = self.canonical_movie_target(mc);
                if canonical == "_root" {
                    self.avm1_globals.contains_key(member)
                        || self
                            .display_list
                            .iter()
                            .any(|(d, s)| slot_matches_segment(*d, s, member))
                } else if let Some(slot) = self.target_slot(&canonical) {
                    slot.props.contains_key(member)
                        || slot
                            .children
                            .iter()
                            .any(|(d, s)| slot_matches_segment(*d, s, member))
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    pub(super) fn get_avm1_variable(
        &self,
        name: &str,
        locals: &HashMap<String, Avm1Value>,
    ) -> Avm1Value {
        for scope in self.avm1_with_scopes.iter().rev() {
            if self.avm1_has_member(scope, name) {
                return self.get_avm1_member(scope, name);
            }
        }
        for &scope_id in self.avm1_scope_chain.iter().rev() {
            if let Some(v) = self
                .avm1_objects
                .get(scope_id)
                .and_then(|o| o.props.get(name))
            {
                return v.clone();
            }
        }
        if let Some(v) = locals.get(name) {
            return v.clone();
        }
        let cur_target = self.current_target_path();
        if cur_target != "_root" {
            if let Some(slot) = self.target_slot(&cur_target)
                && let Some(v) = slot.props.get(name)
            {
                return v.clone();
            }
            if let Some(v) = self.avm1_global_obj.get(name) {
                return v.clone();
            }
        }
        if let Some(v) = self.avm1_globals.get(name) {
            return v.clone();
        }
        if let Some(v) = self.avm1_global_obj.get(name) {
            return v.clone();
        }
        if let Some(func) = self.avm1_functions.get(name)
            && let Some(fn_id) = func.self_obj_id
        {
            return Avm1Value::Object(fn_id);
        }
        match name {
            "this" => {
                if cur_target == "_root" {
                    Avm1Value::MovieClip("_root".to_owned())
                } else {
                    Avm1Value::MovieClipIdentity(cur_target)
                }
            }
            "_root" | "_level0" => Avm1Value::MovieClip("_root".to_owned()),
            "_global" => Avm1Value::MovieClip("_global".to_owned()),
            "_parent" => {
                if cur_target == "_root" {
                    Avm1Value::Undefined
                } else if let Some((parent, _)) = cur_target.rsplit_once('/') {
                    Avm1Value::MovieClipIdentity(parent.to_owned())
                } else {
                    Avm1Value::MovieClip("_root".to_owned())
                }
            }
            "Math" => Avm1Value::MovieClip("Math".to_owned()),
            "Object" => Avm1Value::MovieClip("__builtin_Object".to_owned()),
            "Array" => Avm1Value::MovieClip("__builtin_Array".to_owned()),
            "Function" => Avm1Value::MovieClip("__builtin_Function".to_owned()),
            "MovieClip" => Avm1Value::MovieClip("__builtin_MovieClip".to_owned()),
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError"
            | "URIError" | "EvalError" | "ArgumentError" => {
                Avm1Value::MovieClip(format!("__builtin_{name}"))
            }
            "String" => Avm1Value::MovieClip("__builtin_String".to_owned()),
            "Number" => Avm1Value::MovieClip("__builtin_Number".to_owned()),
            "Boolean" => Avm1Value::MovieClip("__builtin_Boolean".to_owned()),
            "NaN" => Avm1Value::Number(f64::NAN),
            "Infinity" => Avm1Value::Number(f64::INFINITY),
            "_x" => self.get_mc_property("", 0),
            "_y" => self.get_mc_property("", 1),
            "_xscale" => self.get_mc_property("", 2),
            "_yscale" => self.get_mc_property("", 3),
            "_currentframe" => self.get_mc_property("", 4),
            "_totalframes" => self.get_mc_property("", 5),
            "_alpha" => self.get_mc_property("", 6),
            "_visible" => self.get_mc_property("", 7),
            "_rotation" => self.get_mc_property("", 10),
            "_target" => self.get_mc_property("", 11),
            "_framesloaded" => self.get_mc_property("", 12),
            "_name" => self.get_mc_property("", 13),
            _ => {
                let target = self.canonical_movie_target(name);
                if self.target_slot(&target).is_some() {
                    Avm1Value::MovieClipIdentity(target)
                } else {
                    Avm1Value::Undefined
                }
            }
        }
    }

    pub(super) fn set_avm1_variable(
        &mut self,
        name: String,
        val: Avm1Value,
        locals: &mut HashMap<String, Avm1Value>,
    ) {
        for scope in self.avm1_with_scopes.clone().into_iter().rev() {
            if self.avm1_has_member(&scope, &name) {
                self.set_avm1_member(&scope, &name, val);
                return;
            }
        }
        for &scope_id in self.avm1_scope_chain.iter().rev() {
            if self
                .avm1_objects
                .get(scope_id)
                .is_some_and(|o| o.props.contains_key(&name))
            {
                if let Some(o) = self.avm1_objects.get_mut(scope_id) {
                    o.props.insert(name, val);
                }
                return;
            }
        }
        if let Some(slot) = locals.get_mut(&name) {
            *slot = val;
            return;
        }
        let cur_target = self.current_target_path();
        match name.as_str() {
            "_x" => return self.set_mc_property_identity(&cur_target, 0, val),
            "_y" => return self.set_mc_property_identity(&cur_target, 1, val),
            "_xscale" => return self.set_mc_property_identity(&cur_target, 2, val),
            "_yscale" => return self.set_mc_property_identity(&cur_target, 3, val),
            "_alpha" => return self.set_mc_property_identity(&cur_target, 6, val),
            "_visible" => return self.set_mc_property_identity(&cur_target, 7, val),
            "_rotation" => return self.set_mc_property_identity(&cur_target, 10, val),
            "_name" => return self.set_mc_property_identity(&cur_target, 13, val),
            _ => {}
        }
        if cur_target != "_root"
            && let Some(slot) = self.target_slot_mut(&cur_target)
        {
            slot.props.insert(name, val);
        } else {
            self.avm1_globals.insert(name, val);
        }
    }

    pub(super) fn get_mc_property(&self, target: &str, prop_idx: i32) -> Avm1Value {
        let target = self.canonical_movie_target(target);
        self.get_mc_property_identity(&target, prop_idx)
    }

    pub(super) fn get_mc_property_identity(&self, target: &str, prop_idx: i32) -> Avm1Value {
        if target == "_root" || target.is_empty() {
            return match prop_idx {
                0 => Avm1Value::Number(self.root_props.x),
                1 => Avm1Value::Number(self.root_props.y),
                2 => Avm1Value::Number(self.root_props.scale_x),
                3 => Avm1Value::Number(self.root_props.scale_y),
                4 => Avm1Value::Number(f64::from(self.current_frame + 1)),
                5 | 12 => Avm1Value::Number(f64::from(self.root_timeline.total_frames)),
                6 => Avm1Value::Number(self.root_props.alpha),
                7 => Avm1Value::Bool(self.root_props.visible),
                8 => Avm1Value::Number(self.root_props.scale_x.abs()),
                9 => Avm1Value::Number(self.root_props.scale_y.abs()),
                10 => Avm1Value::Number(self.root_props.rotation),
                11 => Avm1Value::String("/".to_owned()),
                13 => Avm1Value::String(self.root_props.name.clone()),
                14 | 15 => Avm1Value::String(String::new()),
                16 => Avm1Value::Number(1.0),
                17 => Avm1Value::Bool(true),
                18 => Avm1Value::Number(5.0),
                19 => Avm1Value::String("HIGH".to_owned()),
                20 | 21 => Avm1Value::Number(0.0),
                _ => Avm1Value::Undefined,
            };
        }
        let slot = self.target_slot(target);
        match prop_idx {
            0 => Avm1Value::Number(slot.map_or(0.0, |s| s.x)),
            1 => Avm1Value::Number(slot.map_or(0.0, |s| s.y)),
            2 => Avm1Value::Number(slot.map_or(100.0, |s| s.scale_x)),
            3 => Avm1Value::Number(slot.map_or(100.0, |s| s.scale_y)),
            4 => Avm1Value::Number(f64::from(
                slot.map_or(self.current_frame + 1, |s| s.current_frame + 1),
            )),
            5 | 12 => Avm1Value::Number(f64::from(
                slot.map_or(self.root_timeline.total_frames, |s| s.total_frames),
            )),
            6 => Avm1Value::Number(slot.map_or(100.0, |s| s.alpha)),
            7 => Avm1Value::Bool(slot.is_none_or(|s| s.visible)),
            8 => Avm1Value::Number(slot.map_or(0.0, |s| s.scale_x.abs())),
            9 => Avm1Value::Number(slot.map_or(0.0, |s| s.scale_y.abs())),
            10 => Avm1Value::Number(slot.map_or(0.0, |s| s.rotation)),
            11 => Avm1Value::String(self.target_slash_path(target)),
            13 => Avm1Value::String(slot.map_or_else(String::new, |s| s.name.clone())),
            14 | 15 => Avm1Value::String(String::new()),
            16 => Avm1Value::Number(1.0),
            17 => Avm1Value::Bool(true),
            18 => Avm1Value::Number(5.0),
            19 => Avm1Value::String("HIGH".to_owned()),
            20 | 21 => Avm1Value::Number(0.0),
            _ => Avm1Value::Undefined,
        }
    }

    pub(super) fn set_mc_property(&mut self, target: &str, prop_idx: i32, val: Avm1Value) {
        let target = self.canonical_movie_target(target);
        self.set_mc_property_identity(&target, prop_idx, val);
    }

    pub(super) fn set_mc_property_identity(&mut self, target: &str, prop_idx: i32, val: Avm1Value) {
        let num = val.to_number();
        let bval = val.to_bool(self.swf_version);
        let sval = val.to_trace_string(self.swf_version, &self.avm1_objects);
        if target == "_root" || target.is_empty() {
            match prop_idx {
                0 if num.is_finite() => self.root_props.x = num,
                1 if num.is_finite() => self.root_props.y = num,
                2 if num.is_finite() => self.root_props.scale_x = num,
                3 if num.is_finite() => self.root_props.scale_y = num,
                6 if num.is_finite() => self.root_props.alpha = num,
                7 => self.root_props.visible = bval,
                10 if num.is_finite() => self.root_props.rotation = num,
                13 => self.root_props.name = sval,
                _ => {}
            }
            return;
        }
        if let Some(slot) = self.target_slot_mut(target) {
            match prop_idx {
                0 if num.is_finite() => {
                    slot.x = num;
                    slot.sync_matrix_from_props();
                }
                1 if num.is_finite() => {
                    slot.y = num;
                    slot.sync_matrix_from_props();
                }
                2 if num.is_finite() => {
                    slot.scale_x = num;
                    slot.sync_matrix_from_props();
                }
                3 if num.is_finite() => {
                    slot.scale_y = num;
                    slot.sync_matrix_from_props();
                }
                6 if num.is_finite() => slot.alpha = num,
                7 => slot.visible = bval,
                10 if num.is_finite() => {
                    slot.rotation = num;
                    slot.sync_matrix_from_props();
                }
                13 => slot.name = sval,
                _ => {}
            }
        }
    }

    pub(super) fn get_avm1_object_prop_chain(
        &self,
        start_id: usize,
        member: &str,
    ) -> Option<Avm1Value> {
        let is_array = self
            .avm1_objects
            .get(start_id)
            .is_some_and(|o| o.array_elements.is_some());
        let mut cur_id = start_id;
        for _ in 0..32 {
            let obj = self.avm1_objects.get(cur_id)?;
            if let Some(elems) = &obj.array_elements {
                if member == "length" {
                    return Some(Avm1Value::Number(elems.len() as f64));
                }
                if let Ok(idx) = member.parse::<usize>()
                    && idx < elems.len()
                    && !obj.deleted_indices.contains(&idx)
                {
                    return Some(elems[idx].clone());
                }
            }
            if let Some(v) = obj.props.get(member) {
                return Some(v.clone());
            }
            if let Some(Avm1Value::Object(proto_id)) = obj.props.get("__proto__") {
                cur_id = *proto_id;
            } else {
                break;
            }
        }
        if self.vm_kind == SwfVmKind::Avm2 {
            if is_array
                && let Some(v) = self
                    .avm2_globals
                    .get(&format!("__avm2_type:Array.prototype.{member}"))
            {
                return Some(v.clone());
            }
            if let Some(v) = self
                .avm2_globals
                .get(&format!("__avm2_type:Object.prototype.{member}"))
            {
                return Some(v.clone());
            }
        }
        None
    }

    pub(super) fn get_avm1_member(&self, target: &Avm1Value, member: &str) -> Avm1Value {
        match target {
            Avm1Value::Object(id) => {
                if member == "length"
                    && let Some(Avm1Value::String(s)) = self
                        .avm1_objects
                        .get(*id)
                        .and_then(|o| o.boxed_primitive.as_deref())
                {
                    return Avm1Value::Number(s.chars().count() as f64);
                }
                self.get_avm1_object_prop_chain(*id, member)
                    .unwrap_or(Avm1Value::Undefined)
            }
            Avm1Value::Super { super_ctor, .. } => {
                if let Some(sup_id) = super_ctor
                    && let Some(Avm1Value::Object(proto_id)) = self
                        .avm1_objects
                        .get(*sup_id)
                        .and_then(|o| o.props.get("prototype"))
                {
                    self.get_avm1_object_prop_chain(*proto_id, member)
                        .unwrap_or(Avm1Value::Undefined)
                } else {
                    Avm1Value::Undefined
                }
            }
            Avm1Value::String(s) => {
                if member == "length" {
                    Avm1Value::Number(s.chars().count() as f64)
                } else {
                    Avm1Value::Undefined
                }
            }
            Avm1Value::MovieClipIdentity(target) => self.get_avm1_mc_member(target, member),
            Avm1Value::MovieClip(mc) => {
                if mc == "_global" {
                    if let Some(v) = self
                        .avm1_global_obj
                        .get(member)
                        .or_else(|| self.avm1_globals.get(member))
                    {
                        return v.clone();
                    }
                    return match member {
                        "Object" | "Array" | "Function" | "MovieClip" | "String" | "Number"
                        | "Boolean" | "Error" | "TypeError" | "RangeError" | "ReferenceError"
                        | "SyntaxError" | "URIError" | "EvalError" | "ArgumentError" => {
                            Avm1Value::MovieClip(format!("__builtin_{member}"))
                        }
                        "Math" => Avm1Value::MovieClip("Math".to_owned()),
                        _ => Avm1Value::Undefined,
                    };
                }
                if let Some(builtin_name) = mc.strip_prefix("__builtin_")
                    && !builtin_name.starts_with("proto_")
                    && member == "prototype"
                {
                    return Avm1Value::MovieClip(format!("__builtin_proto_{builtin_name}"));
                }
                if mc == "Math" {
                    return match member {
                        "PI" => Avm1Value::Number(std::f64::consts::PI),
                        "E" => Avm1Value::Number(std::f64::consts::E),
                        "SQRT2" => Avm1Value::Number(std::f64::consts::SQRT_2),
                        "SQRT1_2" => Avm1Value::Number(std::f64::consts::FRAC_1_SQRT_2),
                        "LN2" => Avm1Value::Number(std::f64::consts::LN_2),
                        "LN10" => Avm1Value::Number(std::f64::consts::LN_10),
                        _ => Avm1Value::Undefined,
                    };
                }
                let canonical = self.canonical_movie_target(mc);
                self.get_avm1_mc_member(&canonical, member)
            }
            _ => Avm1Value::Undefined,
        }
    }

    pub(super) fn get_avm1_mc_member(&self, target: &str, member: &str) -> Avm1Value {
        match member {
            "_x" => self.get_mc_property_identity(target, 0),
            "_y" => self.get_mc_property_identity(target, 1),
            "_xscale" => self.get_mc_property_identity(target, 2),
            "_yscale" => self.get_mc_property_identity(target, 3),
            "_currentframe" => self.get_mc_property_identity(target, 4),
            "_totalframes" => self.get_mc_property_identity(target, 5),
            "_alpha" => self.get_mc_property_identity(target, 6),
            "_visible" => self.get_mc_property_identity(target, 7),
            "_width" => self.get_mc_property_identity(target, 8),
            "_height" => self.get_mc_property_identity(target, 9),
            "_rotation" => self.get_mc_property_identity(target, 10),
            "_target" => self.get_mc_property_identity(target, 11),
            "_framesloaded" => self.get_mc_property_identity(target, 12),
            "_name" => self.get_mc_property_identity(target, 13),
            "_quality" => self.get_mc_property_identity(target, 19),
            "_xmouse" => self.get_mc_property_identity(target, 20),
            "_ymouse" => self.get_mc_property_identity(target, 21),
            "_root" | "_level0" => Avm1Value::MovieClip("_root".to_owned()),
            "_global" => Avm1Value::MovieClip("_global".to_owned()),
            "_parent" => {
                if target == "_root" || target.is_empty() {
                    Avm1Value::Undefined
                } else if let Some((parent, _)) = target.rsplit_once('/') {
                    Avm1Value::MovieClipIdentity(parent.to_owned())
                } else {
                    Avm1Value::MovieClip("_root".to_owned())
                }
            }
            _ => {
                if target == "_root" {
                    if let Some(v) = self.avm1_globals.get(member) {
                        return v.clone();
                    }
                    if let Some(func) = self.avm1_functions.get(member)
                        && let Some(fn_id) = func.self_obj_id
                    {
                        return Avm1Value::Object(fn_id);
                    }
                    if let Some((depth, slot)) =
                        self.display_list.iter().find_map(|(depth, slot)| {
                            slot_matches_segment(*depth, slot, member).then_some((*depth, slot))
                        })
                    {
                        return Avm1Value::MovieClipIdentity(live_slot_segment(depth, slot));
                    }
                    return Avm1Value::Undefined;
                }
                if let Some(slot) = self.target_slot(target) {
                    if let Some(value) = slot.props.get(member) {
                        return value.clone();
                    }
                    if let Some(Avm1Value::Object(proto_id)) = slot.props.get("__proto__")
                        && let Some(val) = self.get_avm1_object_prop_chain(*proto_id, member)
                    {
                        return val;
                    }
                    if let Some((depth, child)) = slot.children.iter().find_map(|(depth, child)| {
                        slot_matches_segment(*depth, child, member).then_some((*depth, child))
                    }) {
                        return Avm1Value::MovieClipIdentity(join_target_path(
                            target,
                            &live_slot_segment(depth, child),
                        ));
                    }
                }
                Avm1Value::Undefined
            }
        }
    }

    pub(super) fn set_avm1_member(&mut self, target: &Avm1Value, member: &str, val: Avm1Value) {
        match target {
            Avm1Value::Object(id) => {
                if let Some(obj) = self.avm1_objects.get_mut(*id) {
                    if let Some(elems) = &mut obj.array_elements {
                        if member == "length" {
                            let new_len = val.to_i32().clamp(0, 4096) as usize;
                            elems.resize(new_len, Avm1Value::Undefined);
                            obj.deleted_indices.retain(|&i| i < new_len);
                            return;
                        }
                        if let Ok(idx) = member.parse::<usize>()
                            && idx < 4096
                        {
                            if idx >= elems.len() {
                                let prev_len = elems.len();
                                elems.resize(idx + 1, Avm1Value::Undefined);
                                for hole in prev_len..idx {
                                    obj.deleted_indices.insert(hole);
                                }
                            }
                            obj.deleted_indices.remove(&idx);
                            elems[idx] = val;
                            return;
                        }
                    }
                    obj.props.insert(member.to_owned(), val);
                }
            }
            Avm1Value::MovieClipIdentity(target) => self.set_avm1_mc_member(target, member, val),
            Avm1Value::MovieClip(mc) => {
                if mc == "_global" {
                    self.avm1_global_obj.insert(member.to_owned(), val);
                    return;
                }
                let canonical = self.canonical_movie_target(mc);
                self.set_avm1_mc_member(&canonical, member, val);
            }
            _ => {}
        }
    }

    pub(super) fn set_avm1_mc_member(&mut self, target: &str, member: &str, val: Avm1Value) {
        match member {
            "_x" => self.set_mc_property_identity(target, 0, val),
            "_y" => self.set_mc_property_identity(target, 1, val),
            "_xscale" => self.set_mc_property_identity(target, 2, val),
            "_yscale" => self.set_mc_property_identity(target, 3, val),
            "_alpha" => self.set_mc_property_identity(target, 6, val),
            "_visible" => self.set_mc_property_identity(target, 7, val),
            "_rotation" => self.set_mc_property_identity(target, 10, val),
            "_name" => self.set_mc_property_identity(target, 13, val),
            _ => {
                if target == "_root" {
                    self.avm1_globals.insert(member.to_owned(), val);
                } else if let Some(slot) = self.target_slot_mut(target) {
                    slot.props.insert(member.to_owned(), val);
                }
            }
        }
    }

    pub(super) fn set_target_playing(&mut self, playing: bool) {
        let target = self.current_target_path();
        if target != "_root" {
            if let Some(slot) = self.target_slot_mut(&target) {
                slot.playing = playing;
            }
            return;
        }
        self.playing = playing;
    }

    pub(super) fn set_target_frame_step(&mut self, delta: i32) {
        let target = self.current_target_path();
        if target != "_root" {
            if let Some(slot) = self.target_slot_mut(&target) {
                let next = (slot.current_frame as i32 + delta)
                    .clamp(0, slot.total_frames.saturating_sub(1) as i32)
                    as u32;
                slot.current_frame = next;
                slot.playing = false;
            }
            return;
        }
        let total = self.root_timeline.frames.len().max(1) as u32;
        let next =
            (self.current_frame as i32 + delta).clamp(0, total.saturating_sub(1) as i32) as u32;
        self.current_frame = next;
        self.playing = false;
    }
}
