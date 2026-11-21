use super::*;

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
        let new_slot = LiveSlot {
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
        let new_slot = LiveSlot {
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
            );
        }
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
        let cloned = LiveSlot {
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
            );
        }
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
        if cur_target != "_root"
            && let Some(slot) = self.target_slot(&cur_target)
            && let Some(v) = slot.props.get(name)
        {
            return v.clone();
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
        let mut cur_id = start_id;
        for _ in 0..32 {
            let obj = self.avm1_objects.get(cur_id)?;
            if let Some(elems) = &obj.array_elements {
                if member == "length" {
                    return Some(Avm1Value::Number(elems.len() as f64));
                }
                if let Ok(idx) = member.parse::<usize>() {
                    return Some(elems.get(idx).cloned().unwrap_or(Avm1Value::Undefined));
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
        None
    }

    pub(super) fn get_avm1_member(&self, target: &Avm1Value, member: &str) -> Avm1Value {
        match target {
            Avm1Value::Object(id) => self
                .get_avm1_object_prop_chain(*id, member)
                .unwrap_or(Avm1Value::Undefined),
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
                    return self
                        .avm1_global_obj
                        .get(member)
                        .or_else(|| self.avm1_globals.get(member))
                        .cloned()
                        .unwrap_or(Avm1Value::Undefined);
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

    pub(super) fn call_avm1_function(
        &mut self,
        fn_name: &str,
        args: Vec<Avm1Value>,
        caller_locals: &mut HashMap<String, Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        let target = self.current_target_path();
        let this_val = if target == "_root" {
            Avm1Value::MovieClip("_root".to_owned())
        } else {
            Avm1Value::MovieClipIdentity(target.clone())
        };
        for scope in self.avm1_with_scopes.clone().into_iter().rev() {
            if self.avm1_has_member(&scope, fn_name)
                && let Avm1Value::Object(id) = self.get_avm1_member(&scope, fn_name)
                && let Some(func) = self.avm1_objects.get(id).and_then(|o| o.function.clone())
            {
                return self.invoke_avm1_func_with_this(&func, scope, None, args, steps, depth);
            }
        }
        for &scope_id in self.avm1_scope_chain.iter().rev() {
            if let Some(Avm1Value::Object(id)) = self
                .avm1_objects
                .get(scope_id)
                .and_then(|o| o.props.get(fn_name).cloned())
                && let Some(func) = self.avm1_objects.get(id).and_then(|o| o.function.clone())
            {
                return self.invoke_avm1_func_with_this(&func, this_val, None, args, steps, depth);
            }
        }
        if let Some(Avm1Value::Object(id)) = caller_locals.get(fn_name).cloned()
            && let Some(func) = self.avm1_objects.get(id).and_then(|o| o.function.clone())
        {
            return self.invoke_avm1_func_with_this(&func, this_val, None, args, steps, depth);
        }
        if target != "_root"
            && let Some(Avm1Value::Object(id)) = self
                .target_slot(&target)
                .and_then(|s| s.props.get(fn_name).cloned())
            && let Some(func) = self.avm1_objects.get(id).and_then(|o| o.function.clone())
        {
            return self.invoke_avm1_func_with_this(&func, this_val, None, args, steps, depth);
        }
        if let Some(func) = self.avm1_functions.get(fn_name).cloned() {
            return self.invoke_avm1_func_with_this(&func, this_val, None, args, steps, depth);
        }
        if let Some(Avm1Value::Object(id)) = self
            .avm1_globals
            .get(fn_name)
            .or_else(|| self.avm1_global_obj.get(fn_name))
            .cloned()
            && let Some(func) = self.avm1_objects.get(id).and_then(|o| o.function.clone())
        {
            return self.invoke_avm1_func_with_this(&func, this_val, None, args, steps, depth);
        }
        match fn_name {
            "trace" => {
                if let Some(first) = args.first() {
                    let s = first.to_trace_string(self.swf_version, &self.avm1_objects);
                    self.record_trace(s);
                }
                Avm1Value::Undefined
            }
            "int" => Avm1Value::Number(f64::from(
                args.first().unwrap_or(&Avm1Value::Undefined).to_i32(),
            )),
            "parseInt" => {
                let s = args
                    .first()
                    .unwrap_or(&Avm1Value::Undefined)
                    .to_trace_string(self.swf_version, &self.avm1_objects);
                let radix = args.get(1).map_or(0, Avm1Value::to_i32);
                Avm1Value::Number(parse_avm_int_radix(&s, radix))
            }
            "Number" | "parseFloat" => {
                Avm1Value::Number(args.first().unwrap_or(&Avm1Value::Undefined).to_number())
            }
            "isNaN" => Avm1Value::Bool(
                args.first()
                    .unwrap_or(&Avm1Value::Undefined)
                    .to_number()
                    .is_nan(),
            ),
            "isFinite" => Avm1Value::Bool(
                args.first()
                    .unwrap_or(&Avm1Value::Undefined)
                    .to_number()
                    .is_finite(),
            ),
            "String" => Avm1Value::String(
                args.first()
                    .unwrap_or(&Avm1Value::Undefined)
                    .to_trace_string(self.swf_version, &self.avm1_objects),
            ),
            "Boolean" => Avm1Value::Bool(
                args.first()
                    .unwrap_or(&Avm1Value::Undefined)
                    .to_bool(self.swf_version),
            ),
            "targetPath" => match args.first() {
                Some(Avm1Value::MovieClip(mc)) => {
                    let canonical = self.canonical_movie_target(mc);
                    self.target_dot_path(&canonical)
                        .map_or(Avm1Value::Undefined, Avm1Value::String)
                }
                Some(Avm1Value::MovieClipIdentity(t)) => self
                    .target_dot_path(t)
                    .map_or(Avm1Value::Undefined, Avm1Value::String),
                _ => Avm1Value::Undefined,
            },
            "stop" => {
                self.set_target_playing(false);
                Avm1Value::Undefined
            }
            "play" => {
                self.set_target_playing(true);
                Avm1Value::Undefined
            }
            "nextFrame" => {
                self.set_target_frame_step(1);
                Avm1Value::Undefined
            }
            "prevFrame" => {
                self.set_target_frame_step(-1);
                Avm1Value::Undefined
            }
            "gotoAndStop" | "gotoAndPlay" => {
                if let Some(first) = args.first() {
                    let target = self.current_target_path();
                    self.set_target_goto_value_identity(&target, first, fn_name == "gotoAndPlay");
                }
                Avm1Value::Undefined
            }
            "createEmptyMovieClip" => {
                let name = args.first().map_or_else(String::new, |v| {
                    v.to_trace_string(self.swf_version, &self.avm1_objects)
                });
                let d = args.get(1).map_or(0, Avm1Value::to_i32);
                self.create_empty_movie_clip_in_target(&target, &name, d)
            }
            "attachMovie" => {
                let id_name = args.first().map_or_else(String::new, |v| {
                    v.to_trace_string(self.swf_version, &self.avm1_objects)
                });
                let new_name = args.get(1).map_or_else(String::new, |v| {
                    v.to_trace_string(self.swf_version, &self.avm1_objects)
                });
                let d = args.get(2).map_or(0, Avm1Value::to_i32);
                self.attach_movie_in_target(&target, &id_name, &new_name, d, args.get(3))
            }
            "super" => {
                let sup = self.get_avm1_variable("super", caller_locals);
                if matches!(sup, Avm1Value::Super { .. }) {
                    self.call_avm1_method(&sup, "", args, caller_locals, steps, depth)
                } else {
                    Avm1Value::Undefined
                }
            }
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError"
            | "URIError" | "EvalError" | "ArgumentError" => self.construct_avm1_object(
                fn_name,
                &Avm1Value::MovieClip(format!("__builtin_{fn_name}")),
                args,
                steps,
                depth,
            ),
            "duplicateMovieClip" => {
                if let Some(first) = args.first() {
                    let src = self.resolve_avm1_target_value(first);
                    let new_name = args.get(1).map_or_else(String::new, |v| {
                        v.to_trace_string(self.swf_version, &self.avm1_objects)
                    });
                    let d = args.get(2).map_or(0, Avm1Value::to_i32);
                    self.duplicate_movie_clip_at_swf_depth(&src, &new_name, script_depth_to_swf(d))
                } else {
                    Avm1Value::Undefined
                }
            }
            "removeMovieClip" => {
                if let Some(first) = args.first() {
                    let resolved = self.resolve_avm1_target_value(first);
                    self.remove_movie_clip_identity(&resolved);
                } else {
                    self.remove_movie_clip_identity(&target);
                }
                Avm1Value::Undefined
            }
            "beginFill"
            | "endFill"
            | "lineStyle"
            | "moveTo"
            | "lineTo"
            | "curveTo"
            | "clear"
            | "drawRect"
            | "drawCircle"
            | "drawEllipse"
            | "getNextHighestDepth" => self.call_avm1_method(
                &Avm1Value::MovieClipIdentity(target),
                fn_name,
                args,
                caller_locals,
                steps,
                depth,
            ),
            _ => Avm1Value::Undefined,
        }
    }

    pub(super) fn resolve_avm1_object_method_with_super(
        &self,
        obj_id: usize,
        method_name: &str,
    ) -> Option<(Avm1Function, Option<usize>)> {
        let mut cur_id = obj_id;
        for _ in 0..32 {
            let obj = self.avm1_objects.get(cur_id)?;
            if let Some(Avm1Value::Object(fn_id)) = obj.props.get(method_name)
                && let Some(func) = self
                    .avm1_objects
                    .get(*fn_id)
                    .and_then(|o| o.function.clone())
            {
                let next_super = obj
                    .props
                    .get("__proto__")
                    .and_then(|v| match v {
                        Avm1Value::Object(pid) => self.avm1_objects.get(*pid),
                        _ => None,
                    })
                    .and_then(|p| {
                        p.props
                            .get("constructor")
                            .or_else(|| p.props.get("__constructor__"))
                    })
                    .and_then(|v| match v {
                        Avm1Value::Object(cid) => Some(*cid),
                        _ => None,
                    });
                return Some((func, next_super));
            }
            if let Some(Avm1Value::Object(proto_id)) = obj.props.get("__proto__") {
                cur_id = *proto_id;
            } else {
                break;
            }
        }
        None
    }

    pub(super) fn call_avm1_method(
        &mut self,
        receiver: &Avm1Value,
        method_name: &str,
        args: Vec<Avm1Value>,
        caller_locals: &mut HashMap<String, Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        match receiver {
            Avm1Value::Super {
                this_val,
                super_ctor,
            } => {
                let Some(sup_id) = super_ctor else {
                    return Avm1Value::Undefined;
                };
                if method_name.is_empty() {
                    if let Some(func) = self
                        .avm1_objects
                        .get(*sup_id)
                        .and_then(|o| o.function.clone())
                    {
                        let next_super = self
                            .avm1_objects
                            .get(*sup_id)
                            .and_then(|o| o.props.get("__super_ctor__"))
                            .and_then(|v| match v {
                                Avm1Value::Object(id) => Some(*id),
                                _ => None,
                            });
                        return self.invoke_avm1_func_with_this(
                            &func,
                            (**this_val).clone(),
                            next_super,
                            args,
                            steps,
                            depth,
                        );
                    }
                } else if let Some(Avm1Value::Object(proto_id)) = self
                    .avm1_objects
                    .get(*sup_id)
                    .and_then(|o| o.props.get("prototype").cloned())
                    && let Some((func, next_super)) =
                        self.resolve_avm1_object_method_with_super(proto_id, method_name)
                {
                    return self.invoke_avm1_func_with_this(
                        &func,
                        (**this_val).clone(),
                        next_super,
                        args,
                        steps,
                        depth,
                    );
                }
                Avm1Value::Undefined
            }
            Avm1Value::Object(id) => {
                if method_name.is_empty()
                    && let Some(func) = self.avm1_objects.get(*id).and_then(|o| o.function.clone())
                {
                    let cur_t = self.current_target_path();
                    let this_v = if cur_t == "_root" {
                        Avm1Value::MovieClip("_root".to_owned())
                    } else {
                        Avm1Value::MovieClipIdentity(cur_t)
                    };
                    return self
                        .invoke_avm1_func_with_this(&func, this_v, None, args, steps, depth);
                }
                if (method_name == "call" || method_name == "apply")
                    && let Some(func) = self.avm1_objects.get(*id).and_then(|o| o.function.clone())
                {
                    let this_arg = args
                        .first()
                        .cloned()
                        .unwrap_or_else(|| Avm1Value::MovieClip("_root".to_owned()));
                    let call_args = if method_name == "call" {
                        args.into_iter().skip(1).collect()
                    } else if let Some(Avm1Value::Object(arr_id)) = args.get(1) {
                        self.avm1_objects
                            .get(*arr_id)
                            .and_then(|o| o.array_elements.clone())
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    return self.invoke_avm1_func_with_this(
                        &func, this_arg, None, call_args, steps, depth,
                    );
                }
                if let Some((func, super_ctor)) =
                    self.resolve_avm1_object_method_with_super(*id, method_name)
                {
                    return self.invoke_avm1_func_with_this(
                        &func,
                        Avm1Value::Object(*id),
                        super_ctor,
                        args,
                        steps,
                        depth,
                    );
                }
                let swf_ver = self.swf_version;
                let has_array = self
                    .avm1_objects
                    .get(*id)
                    .is_some_and(|o| o.array_elements.is_some());
                if has_array {
                    match method_name {
                        "push" => {
                            if let Some(elems) = self
                                .avm1_objects
                                .get_mut(*id)
                                .and_then(|o| o.array_elements.as_mut())
                            {
                                for a in args {
                                    if elems.len() < 4096 {
                                        elems.push(a);
                                    }
                                }
                                return Avm1Value::Number(elems.len() as f64);
                            }
                        }
                        "pop" => {
                            if let Some(elems) = self
                                .avm1_objects
                                .get_mut(*id)
                                .and_then(|o| o.array_elements.as_mut())
                            {
                                return elems.pop().unwrap_or(Avm1Value::Undefined);
                            }
                        }
                        "shift" => {
                            if let Some(elems) = self
                                .avm1_objects
                                .get_mut(*id)
                                .and_then(|o| o.array_elements.as_mut())
                            {
                                return if elems.is_empty() {
                                    Avm1Value::Undefined
                                } else {
                                    elems.remove(0)
                                };
                            }
                        }
                        "unshift" => {
                            if let Some(elems) = self
                                .avm1_objects
                                .get_mut(*id)
                                .and_then(|o| o.array_elements.as_mut())
                            {
                                for (i, a) in args.into_iter().enumerate() {
                                    if elems.len() < 4096 {
                                        elems.insert(i, a);
                                    }
                                }
                                return Avm1Value::Number(elems.len() as f64);
                            }
                        }
                        "reverse" => {
                            if let Some(elems) = self
                                .avm1_objects
                                .get_mut(*id)
                                .and_then(|o| o.array_elements.as_mut())
                            {
                                elems.reverse();
                            }
                            return Avm1Value::Object(*id);
                        }
                        "slice" => {
                            if let Some(elems) = self
                                .avm1_objects
                                .get(*id)
                                .and_then(|o| o.array_elements.clone())
                            {
                                let len = elems.len() as i32;
                                let mut s = args.first().map_or(0, Avm1Value::to_i32);
                                let mut e = args.get(1).map_or(len, Avm1Value::to_i32);
                                if s < 0 {
                                    s = (len + s).max(0);
                                }
                                if e < 0 {
                                    e = (len + e).max(0);
                                }
                                let su = (s.clamp(0, len)) as usize;
                                let eu = (e.clamp(su as i32, len)) as usize;
                                let out_id = self.avm1_objects.len();
                                self.avm1_objects.push(Avm1Object {
                                    array_elements: Some(elems[su..eu].to_vec()),
                                    ..Default::default()
                                });
                                return Avm1Value::Object(out_id);
                            }
                        }
                        "splice" => {
                            if let Some(elems) = self
                                .avm1_objects
                                .get_mut(*id)
                                .and_then(|o| o.array_elements.as_mut())
                            {
                                let len = elems.len() as i32;
                                let mut s = args.first().map_or(0, Avm1Value::to_i32);
                                if s < 0 {
                                    s = (len + s).max(0);
                                }
                                let su = (s.clamp(0, len)) as usize;
                                let del_count =
                                    args.get(1)
                                        .map_or(len - su as i32, Avm1Value::to_i32)
                                        .max(0) as usize;
                                let eu = (su.saturating_add(del_count)).min(elems.len());
                                let insert_items: Vec<Avm1Value> = args
                                    .into_iter()
                                    .skip(2)
                                    .take(
                                        4096usize
                                            .saturating_sub(elems.len().saturating_sub(eu - su)),
                                    )
                                    .collect();
                                let removed: Vec<Avm1Value> =
                                    elems.splice(su..eu, insert_items).collect();
                                let out_id = self.avm1_objects.len();
                                self.avm1_objects.push(Avm1Object {
                                    array_elements: Some(removed),
                                    ..Default::default()
                                });
                                return Avm1Value::Object(out_id);
                            }
                        }
                        "concat" => {
                            if let Some(mut out) = self
                                .avm1_objects
                                .get(*id)
                                .and_then(|o| o.array_elements.clone())
                            {
                                for arg in args {
                                    if let Avm1Value::Object(aid) = &arg
                                        && let Some(sub) = self
                                            .avm1_objects
                                            .get(*aid)
                                            .and_then(|o| o.array_elements.clone())
                                    {
                                        for item in sub {
                                            if out.len() < 4096 {
                                                out.push(item);
                                            }
                                        }
                                    } else if out.len() < 4096 {
                                        out.push(arg);
                                    }
                                }
                                let out_id = self.avm1_objects.len();
                                self.avm1_objects.push(Avm1Object {
                                    array_elements: Some(out),
                                    ..Default::default()
                                });
                                return Avm1Value::Object(out_id);
                            }
                        }
                        "indexOf" | "lastIndexOf" => {
                            if let Some((elems, deleted)) =
                                self.avm1_objects.get(*id).and_then(|o| {
                                    Some((o.array_elements.clone()?, o.deleted_indices.clone()))
                                })
                            {
                                let needle = args.first().cloned().unwrap_or(Avm1Value::Undefined);
                                let len = elems.len() as i32;
                                if method_name == "indexOf" {
                                    let mut from = args.get(1).map_or(0, Avm1Value::to_i32);
                                    if from < 0 {
                                        from = (len + from).max(0);
                                    }
                                    for (idx, elem) in
                                        elems.iter().enumerate().skip(from.max(0) as usize)
                                    {
                                        if !deleted.contains(&idx)
                                            && self.avm2_strict_equals(elem, &needle)
                                        {
                                            return Avm1Value::Number(idx as f64);
                                        }
                                    }
                                } else {
                                    let mut from = args.get(1).map_or(len - 1, Avm1Value::to_i32);
                                    if from < 0 {
                                        from += len;
                                    }
                                    from = from.min(len - 1);
                                    if from >= 0 {
                                        for idx in (0..=(from as usize)).rev() {
                                            if !deleted.contains(&idx)
                                                && self.avm2_strict_equals(&elems[idx], &needle)
                                            {
                                                return Avm1Value::Number(idx as f64);
                                            }
                                        }
                                    }
                                }
                                return Avm1Value::Number(-1.0);
                            }
                        }
                        "join" | "toString" => {
                            let sep = if method_name == "join" {
                                args.first().map_or_else(
                                    || ",".to_owned(),
                                    |v| v.to_trace_string(swf_ver, &self.avm1_objects),
                                )
                            } else {
                                ",".to_owned()
                            };
                            if let Some(elems) = self
                                .avm1_objects
                                .get(*id)
                                .and_then(|o| o.array_elements.clone())
                            {
                                let parts: Vec<String> = elems
                                    .iter()
                                    .map(|v| v.to_trace_string(swf_ver, &self.avm1_objects))
                                    .collect();
                                return Avm1Value::String(parts.join(&sep));
                            }
                        }
                        _ => {}
                    }
                }
                if method_name == "hasOwnProperty" {
                    let key = args.first().map_or_else(String::new, |v| {
                        v.to_trace_string(swf_ver, &self.avm1_objects)
                    });
                    let has = self
                        .avm1_objects
                        .get(*id)
                        .is_some_and(|o| o.props.contains_key(&key));
                    return Avm1Value::Bool(has);
                }
                if method_name == "toString" {
                    let s = Avm1Value::Object(*id).to_trace_string(swf_ver, &self.avm1_objects);
                    return Avm1Value::String(s);
                }
                Avm1Value::Undefined
            }
            Avm1Value::MovieClipIdentity(target) => self.call_avm1_identity_method(
                target,
                method_name,
                args,
                caller_locals,
                steps,
                depth,
            ),
            Avm1Value::MovieClip(mc) => {
                if mc == "__builtin_Object" && method_name == "registerClass" {
                    let linkage = args.first().map_or_else(String::new, |v| {
                        v.to_trace_string(self.swf_version, &self.avm1_objects)
                    });
                    if linkage.is_empty() {
                        return Avm1Value::Bool(false);
                    }
                    match args.get(1) {
                        Some(Avm1Value::Object(ctor_id)) => {
                            self.avm1_registered_classes.insert(linkage, *ctor_id);
                            return Avm1Value::Bool(true);
                        }
                        Some(Avm1Value::Null | Avm1Value::Undefined) => {
                            let existed = self.avm1_registered_classes.remove(&linkage).is_some();
                            return Avm1Value::Bool(existed);
                        }
                        _ => return Avm1Value::Bool(false),
                    }
                }
                if mc == "Math" || mc == "__avm2_type:Math" {
                    let a = args.first().map_or(f64::NAN, Avm1Value::to_number);
                    let b = args.get(1).map_or(f64::NAN, Avm1Value::to_number);
                    return match method_name {
                        "abs" => Avm1Value::Number(a.abs()),
                        "floor" => Avm1Value::Number(a.floor()),
                        "ceil" => Avm1Value::Number(a.ceil()),
                        "round" => Avm1Value::Number(if a.is_nan() {
                            f64::NAN
                        } else {
                            (a + 0.5).floor()
                        }),
                        "sqrt" => Avm1Value::Number(a.sqrt()),
                        "sin" => Avm1Value::Number(a.sin()),
                        "cos" => Avm1Value::Number(a.cos()),
                        "tan" => Avm1Value::Number(a.tan()),
                        "atan2" => Avm1Value::Number(a.atan2(b)),
                        "asin" => Avm1Value::Number(a.asin()),
                        "acos" => Avm1Value::Number(a.acos()),
                        "atan" => Avm1Value::Number(a.atan()),
                        "exp" => Avm1Value::Number(a.exp()),
                        "log" => Avm1Value::Number(a.ln()),
                        "min" => {
                            if args.is_empty() {
                                Avm1Value::Number(f64::INFINITY)
                            } else {
                                let mut acc = f64::INFINITY;
                                for v in &args {
                                    let n = v.to_number();
                                    if n.is_nan() {
                                        return Avm1Value::Number(f64::NAN);
                                    }
                                    acc = acc.min(n);
                                }
                                Avm1Value::Number(acc)
                            }
                        }
                        "max" => {
                            if args.is_empty() {
                                Avm1Value::Number(f64::NEG_INFINITY)
                            } else {
                                let mut acc = f64::NEG_INFINITY;
                                for v in &args {
                                    let n = v.to_number();
                                    if n.is_nan() {
                                        return Avm1Value::Number(f64::NAN);
                                    }
                                    acc = acc.max(n);
                                }
                                Avm1Value::Number(acc)
                            }
                        }
                        "pow" => Avm1Value::Number(a.powf(b)),
                        "random" => {
                            self.rng_state = self
                                .rng_state
                                .wrapping_mul(1_664_525)
                                .wrapping_add(1_013_904_223);
                            Avm1Value::Number(f64::from(self.rng_state) / 4_294_967_296.0)
                        }
                        _ => Avm1Value::Undefined,
                    };
                }
                if (mc == "__builtin_String" || mc == "__avm2_type:String" || mc == "String")
                    && method_name == "fromCharCode"
                {
                    let s: String = args
                        .iter()
                        .filter_map(|v| char::from_u32(v.to_u32() & 0xFFFF))
                        .collect();
                    return Avm1Value::String(s);
                }
                let target = self.canonical_movie_target(mc);
                self.call_avm1_identity_method(
                    &target,
                    method_name,
                    args,
                    caller_locals,
                    steps,
                    depth,
                )
            }
            Avm1Value::String(s) => match method_name {
                "toUpperCase" => Avm1Value::String(s.to_uppercase()),
                "toLowerCase" => Avm1Value::String(s.to_lowercase()),
                "charAt" => {
                    let idx = args.first().map_or(0, Avm1Value::to_i32);
                    if idx < 0 {
                        Avm1Value::String(String::new())
                    } else {
                        Avm1Value::String(
                            s.chars()
                                .nth(idx as usize)
                                .map_or_else(String::new, |c| c.to_string()),
                        )
                    }
                }
                "charCodeAt" => {
                    let idx = args.first().map_or(0, Avm1Value::to_i32);
                    if idx < 0 {
                        Avm1Value::Number(f64::NAN)
                    } else {
                        s.chars()
                            .nth(idx as usize)
                            .map_or(Avm1Value::Number(f64::NAN), |c| {
                                Avm1Value::Number(f64::from(u32::from(c)))
                            })
                    }
                }
                "indexOf" => {
                    let needle = args.first().map_or_else(String::new, |v| {
                        v.to_trace_string(self.swf_version, &self.avm1_objects)
                    });
                    let start = args.get(1).map_or(0, Avm1Value::to_i32).max(0) as usize;
                    let chars: Vec<char> = s.chars().collect();
                    let nchars: Vec<char> = needle.chars().collect();
                    let found = if nchars.is_empty() {
                        Some(start.min(chars.len()))
                    } else if start + nchars.len() > chars.len() {
                        None
                    } else {
                        (start..=chars.len() - nchars.len())
                            .find(|&i| chars[i..i + nchars.len()] == nchars[..])
                    };
                    Avm1Value::Number(found.map_or(-1.0, |i| i as f64))
                }
                "lastIndexOf" => {
                    let needle = args.first().map_or_else(String::new, |v| {
                        v.to_trace_string(self.swf_version, &self.avm1_objects)
                    });
                    let chars: Vec<char> = s.chars().collect();
                    let nchars: Vec<char> = needle.chars().collect();
                    let max_start = if nchars.is_empty() {
                        chars.len() as i32
                    } else {
                        chars.len() as i32 - nchars.len() as i32
                    };
                    if max_start < 0 {
                        Avm1Value::Number(-1.0)
                    } else {
                        let pos = args
                            .get(1)
                            .map_or(max_start, |v| {
                                let n = v.to_number();
                                if n.is_nan() { max_start } else { v.to_i32() }
                            })
                            .min(max_start);
                        if pos < 0 {
                            Avm1Value::Number(-1.0)
                        } else if nchars.is_empty() {
                            Avm1Value::Number(f64::from(pos))
                        } else {
                            let found = (0..=(pos as usize))
                                .rev()
                                .find(|&i| chars[i..i + nchars.len()] == nchars[..]);
                            Avm1Value::Number(found.map_or(-1.0, |i| i as f64))
                        }
                    }
                }
                "concat" => {
                    let mut out = s.clone();
                    for a in args {
                        out.push_str(&a.to_trace_string(self.swf_version, &self.avm1_objects));
                    }
                    Avm1Value::String(out)
                }
                "toString" | "valueOf" => Avm1Value::String(s.clone()),
                "substr" => {
                    let chars: Vec<char> = s.chars().collect();
                    let len = chars.len() as i32;
                    let mut start = args.first().map_or(0, Avm1Value::to_i32);
                    if start < 0 {
                        start = (len + start).max(0);
                    }
                    let count = args.get(1).map_or(len, Avm1Value::to_i32).max(0);
                    let su = start.clamp(0, len) as usize;
                    let eu = (start.saturating_add(count)).clamp(su as i32, len) as usize;
                    Avm1Value::String(chars[su..eu].iter().collect())
                }
                "substring" | "slice" => {
                    let chars: Vec<char> = s.chars().collect();
                    let len = chars.len() as i32;
                    let mut a = args.first().map_or(0, Avm1Value::to_i32);
                    let mut b = args.get(1).map_or(len, Avm1Value::to_i32);
                    if method_name == "slice" {
                        if a < 0 {
                            a = (len + a).max(0);
                        }
                        if b < 0 {
                            b = (len + b).max(0);
                        }
                    } else {
                        a = a.clamp(0, len);
                        b = b.clamp(0, len);
                        if a > b {
                            std::mem::swap(&mut a, &mut b);
                        }
                    }
                    let su = a.clamp(0, len) as usize;
                    let eu = b.clamp(su as i32, len) as usize;
                    Avm1Value::String(chars[su..eu].iter().collect())
                }
                "split" => {
                    let sep = args.first().map_or_else(String::new, |v| {
                        v.to_trace_string(self.swf_version, &self.avm1_objects)
                    });
                    let elems: Vec<Avm1Value> = if sep.is_empty() {
                        s.chars()
                            .map(|c| Avm1Value::String(c.to_string()))
                            .collect()
                    } else {
                        s.split(&sep)
                            .map(|p| Avm1Value::String(p.to_owned()))
                            .collect()
                    };
                    let out_id = self.avm1_objects.len();
                    self.avm1_objects.push(Avm1Object {
                        array_elements: Some(elems),
                        ..Default::default()
                    });
                    Avm1Value::Object(out_id)
                }
                _ => Avm1Value::Undefined,
            },
            _ => self.call_avm1_function(method_name, args, caller_locals, steps, depth),
        }
    }

    pub(super) fn call_avm1_identity_method(
        &mut self,
        target: &str,
        method_name: &str,
        args: Vec<Avm1Value>,
        caller_locals: &mut HashMap<String, Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if target == "_root" {
            if let Some(func) = self.avm1_functions.get(method_name).cloned() {
                return self.invoke_avm1_func_with_this(
                    &func,
                    Avm1Value::MovieClip("_root".to_owned()),
                    None,
                    args,
                    steps,
                    depth,
                );
            }
            if let Some(Avm1Value::Object(fn_id)) = self.avm1_globals.get(method_name).cloned()
                && let Some(func) = self
                    .avm1_objects
                    .get(fn_id)
                    .and_then(|o| o.function.clone())
            {
                return self.invoke_avm1_func_with_this(
                    &func,
                    Avm1Value::MovieClip("_root".to_owned()),
                    None,
                    args,
                    steps,
                    depth,
                );
            }
        } else if let Some(slot_val) = self.target_slot(target).and_then(|slot| {
            slot.props.get(method_name).cloned().or_else(|| {
                if let Some(Avm1Value::Object(proto_id)) = slot.props.get("__proto__") {
                    self.resolve_avm1_object_method_with_super(*proto_id, method_name)
                        .map(|(f, sup)| {
                            let _ = &f;
                            Avm1Value::Super {
                                this_val: Box::new(Avm1Value::Object(*proto_id)),
                                super_ctor: sup,
                            }
                        })
                } else {
                    None
                }
            })
        }) {
            if let Avm1Value::Object(function_id) = slot_val
                && let Some(function) = self
                    .avm1_objects
                    .get(function_id)
                    .and_then(|object| object.function.clone())
            {
                return self.invoke_avm1_func_with_this(
                    &function,
                    Avm1Value::MovieClipIdentity(target.to_owned()),
                    None,
                    args,
                    steps,
                    depth,
                );
            }
            if let Avm1Value::Super {
                this_val,
                super_ctor,
            } = slot_val
                && let Avm1Value::Object(proto_id) = *this_val
                && let Some((function, _)) =
                    self.resolve_avm1_object_method_with_super(proto_id, method_name)
            {
                return self.invoke_avm1_func_with_this(
                    &function,
                    Avm1Value::MovieClipIdentity(target.to_owned()),
                    super_ctor,
                    args,
                    steps,
                    depth,
                );
            }
        }

        if matches!(
            method_name,
            "beginFill"
                | "endFill"
                | "lineStyle"
                | "moveTo"
                | "lineTo"
                | "curveTo"
                | "clear"
                | "drawRect"
                | "drawCircle"
                | "drawEllipse"
        ) {
            let rgb = args.first().map_or(0, Avm1Value::to_u32) & 0x00FF_FFFF;
            let a1 = args.get(1).map_or(100.0, Avm1Value::to_number);
            let a2 = args.get(2).map_or(100.0, Avm1Value::to_number);
            let x0 = args.first().map_or(0.0, Avm1Value::to_number);
            let y0 = args.get(1).map_or(0.0, Avm1Value::to_number);
            let x1 = args.get(2).map_or(0.0, Avm1Value::to_number);
            let y1 = args.get(3).map_or(0.0, Avm1Value::to_number);
            let th_opt = match args.first() {
                None | Some(Avm1Value::Undefined | Avm1Value::Null) => None,
                Some(v) => Some(v.to_number()),
            };
            let line_rgb = args.get(1).map_or(0, Avm1Value::to_u32) & 0x00FF_FFFF;
            if let Some(gfx) = if target == "_root" {
                Some(&mut self.root_graphics)
            } else {
                self.target_slot_mut(target).map(|s| &mut s.graphics)
            } {
                match method_name {
                    "beginFill" => gfx.begin_fill(rgb, a1 / 100.0),
                    "endFill" => gfx.end_fill(),
                    "lineStyle" => gfx.line_style(th_opt, line_rgb, a2 / 100.0),
                    "moveTo" => gfx.move_to(x0, y0),
                    "lineTo" => gfx.line_to(x0, y0),
                    "curveTo" => gfx.curve_to(x0, y0, x1, y1),
                    "drawRect" => gfx.draw_rect(x0, y0, x1, y1),
                    "drawCircle" => {
                        let r = x1.abs();
                        gfx.draw_ellipse(x0 - r, y0 - r, r * 2.0, r * 2.0);
                    }
                    "drawEllipse" => gfx.draw_ellipse(x0, y0, x1, y1),
                    "clear" => gfx.clear(),
                    _ => {}
                }
            }
            return Avm1Value::Undefined;
        }

        match method_name {
            "stop" => {
                if target == "_root" {
                    self.playing = false;
                } else if let Some(slot) = self.target_slot_mut(target) {
                    slot.playing = false;
                }
                return Avm1Value::Undefined;
            }
            "play" => {
                if target == "_root" {
                    self.playing = true;
                } else if let Some(slot) = self.target_slot_mut(target) {
                    slot.playing = true;
                }
                return Avm1Value::Undefined;
            }
            "nextFrame" => {
                if target == "_root" {
                    let total = self.root_timeline.frames.len().max(1) as u32;
                    self.current_frame = (self.current_frame + 1).min(total.saturating_sub(1));
                    self.playing = false;
                } else if let Some(slot) = self.target_slot_mut(target) {
                    slot.current_frame =
                        (slot.current_frame + 1).min(slot.total_frames.saturating_sub(1));
                    slot.playing = false;
                }
                return Avm1Value::Undefined;
            }
            "prevFrame" => {
                if target == "_root" {
                    self.current_frame = self.current_frame.saturating_sub(1);
                    self.playing = false;
                } else if let Some(slot) = self.target_slot_mut(target) {
                    slot.current_frame = slot.current_frame.saturating_sub(1);
                    slot.playing = false;
                }
                return Avm1Value::Undefined;
            }
            "gotoAndStop" | "gotoAndPlay" => {
                if let Some(first) = args.first() {
                    self.set_target_goto_value_identity(
                        target,
                        first,
                        method_name == "gotoAndPlay",
                    );
                }
                return Avm1Value::Undefined;
            }
            "setMask" => {
                self.apply_dynamic_mask(target, args.first());
                return Avm1Value::Undefined;
            }
            "createEmptyMovieClip" => {
                let name = args.first().map_or_else(String::new, |v| {
                    v.to_trace_string(self.swf_version, &self.avm1_objects)
                });
                let d = args.get(1).map_or(0, Avm1Value::to_i32);
                return self.create_empty_movie_clip_in_target(target, &name, d);
            }
            "attachMovie" => {
                let id_name = args.first().map_or_else(String::new, |v| {
                    v.to_trace_string(self.swf_version, &self.avm1_objects)
                });
                let new_name = args.get(1).map_or_else(String::new, |v| {
                    v.to_trace_string(self.swf_version, &self.avm1_objects)
                });
                let d = args.get(2).map_or(0, Avm1Value::to_i32);
                return self.attach_movie_in_target(target, &id_name, &new_name, d, args.get(3));
            }
            "duplicateMovieClip" => {
                let new_name = args.first().map_or_else(String::new, |v| {
                    v.to_trace_string(self.swf_version, &self.avm1_objects)
                });
                let d = args.get(1).map_or(0, Avm1Value::to_i32);
                let swf_depth = script_depth_to_swf(d);
                let res = self.duplicate_movie_clip_at_swf_depth(target, &new_name, swf_depth);
                if let (Avm1Value::MovieClipIdentity(new_id), Some(Avm1Value::Object(init_id))) =
                    (&res, args.get(2))
                    && let Some(props) = self.avm1_objects.get(*init_id).map(|o| o.props.clone())
                {
                    let mc_ref = Avm1Value::MovieClipIdentity(new_id.clone());
                    for (k, v) in props {
                        self.set_avm1_member(&mc_ref, &k, v);
                    }
                }
                return res;
            }
            "removeMovieClip" => {
                self.remove_movie_clip_identity(target);
                return Avm1Value::Undefined;
            }
            "getDepth" => {
                if target == "_root" {
                    return Avm1Value::Number(-16384.0);
                }
                let last_seg = target.rsplit_once('/').map_or(target, |(_, s)| s);
                if let Some(d) = last_seg
                    .strip_prefix("depth")
                    .and_then(|s| s.parse::<u16>().ok())
                {
                    return Avm1Value::Number(f64::from(i32::from(d) - 16384));
                }
                return Avm1Value::Undefined;
            }
            "getNextHighestDepth" => {
                let map_opt = if target == "_root" {
                    Some(&self.display_list)
                } else {
                    self.target_slot(target).map(|s| &s.children)
                };
                if let Some(map) = map_opt {
                    let next = map
                        .keys()
                        .copied()
                        .filter(|&d| d >= 16384)
                        .max()
                        .map_or(0, |max_d| i32::from(max_d) - 16384 + 1);
                    return Avm1Value::Number(f64::from(next));
                }
                return Avm1Value::Number(0.0);
            }
            "swapDepths" => {
                if target != "_root"
                    && let Some(first) = args.first()
                {
                    self.swap_clip_depths(target, first);
                }
                return Avm1Value::Undefined;
            }
            "startDrag" => {
                self.drag_target = Some(target.to_owned());
                return Avm1Value::Undefined;
            }
            "stopDrag" => {
                self.drag_target = None;
                return Avm1Value::Undefined;
            }
            _ => {}
        }

        if target == "_root" {
            let saved_target = std::mem::replace(&mut self.avm1_target, "_root".to_owned());
            let result = self.call_avm1_function(method_name, args, caller_locals, steps, depth);
            self.avm1_target = saved_target;
            return result;
        }
        Avm1Value::Undefined
    }

    pub(super) fn swap_clip_depths(&mut self, target: &str, arg: &Avm1Value) {
        let (parent_target, last_seg) = target
            .rsplit_once('/')
            .map_or(("_root", target), |(p, s)| (p, s));
        let Some(src_depth) = last_seg
            .strip_prefix("depth")
            .and_then(|s| s.parse::<u16>().ok())
        else {
            return;
        };
        let dst_depth = match arg {
            Avm1Value::Number(_) => script_depth_to_swf(arg.to_i32()),
            _ => {
                let other = self.resolve_avm1_target_value(arg);
                let (other_parent, other_seg) = other
                    .rsplit_once('/')
                    .map_or(("_root", other.as_str()), |(p, s)| (p, s));
                if other_parent != parent_target {
                    return;
                }
                let Some(d) = other_seg
                    .strip_prefix("depth")
                    .and_then(|s| s.parse::<u16>().ok())
                else {
                    return;
                };
                d
            }
        };
        if src_depth == dst_depth {
            return;
        }
        let (map, tombstones) = if parent_target == "_root" {
            (&mut self.display_list, &mut self.removed_timeline_depths)
        } else if let Some(slot) = self.target_slot_mut(parent_target) {
            (&mut slot.children, &mut slot.removed_timeline_depths)
        } else {
            return;
        };
        let Some(mut src_slot) = map.remove(&src_depth) else {
            return;
        };
        src_slot.is_dynamic = true;
        if let Some(orig_d) = src_slot.origin_timeline_depth {
            tombstones.insert(orig_d);
        }
        if let Some(mut dst_slot) = map.remove(&dst_depth) {
            dst_slot.is_dynamic = true;
            if let Some(orig_d) = dst_slot.origin_timeline_depth {
                tombstones.insert(orig_d);
            }
            map.insert(src_depth, dst_slot);
        }
        map.insert(dst_depth, src_slot);
    }

    pub(super) fn invoke_avm1_func_with_this(
        &mut self,
        func: &Avm1Function,
        this_val: Avm1Value,
        super_ctor: Option<usize>,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        let saved_pool =
            std::mem::replace(&mut self.avm1_constant_pool, func.constant_pool.clone());
        let saved_regs = self.avm1_registers.clone();
        let saved_target = std::mem::replace(&mut self.avm1_target, func.base_target.clone());
        let saved_with = std::mem::take(&mut self.avm1_with_scopes);

        for r in 0..usize::from(func.register_count) {
            self.avm1_registers[r] = Avm1Value::Undefined;
        }

        let mut fn_locals = HashMap::new();
        let super_val = Avm1Value::Super {
            this_val: Box::new(this_val.clone()),
            super_ctor,
        };

        if func.is_v2 {
            let flags = func.flags;
            let mut next_reg = 1usize;

            // 1. preload_this (0x0001) / suppress_this (0x0002)
            if (flags & 0x0001) != 0 {
                if next_reg < 256 {
                    self.avm1_registers[next_reg] = this_val.clone();
                }
                next_reg += 1;
            } else if (flags & 0x0002) == 0 {
                fn_locals.insert("this".to_owned(), this_val.clone());
            }

            // 2. preload_arguments (0x0004) / suppress_arguments (0x0008)
            if (flags & 0x0004) != 0 || (flags & 0x0008) == 0 {
                let mut arg_props = HashMap::new();
                if let Some(fn_id) = func.self_obj_id {
                    arg_props.insert("callee".to_owned(), Avm1Value::Object(fn_id));
                }
                let args_obj_id = self.avm1_objects.len();
                self.avm1_objects.push(Avm1Object {
                    props: arg_props,
                    array_elements: Some(args.clone()),
                    ..Default::default()
                });
                if (flags & 0x0004) != 0 {
                    if next_reg < 256 {
                        self.avm1_registers[next_reg] = Avm1Value::Object(args_obj_id);
                    }
                    next_reg += 1;
                } else {
                    fn_locals.insert("arguments".to_owned(), Avm1Value::Object(args_obj_id));
                }
            }

            // 3. preload_super (0x0010) / suppress_super (0x0020)
            if (flags & 0x0010) != 0 {
                if next_reg < 256 {
                    self.avm1_registers[next_reg] = super_val;
                }
                next_reg += 1;
            } else if (flags & 0x0020) == 0 {
                fn_locals.insert("super".to_owned(), super_val);
            }

            // 4. preload_root (0x0040)
            if (flags & 0x0040) != 0 {
                if next_reg < 256 {
                    self.avm1_registers[next_reg] = Avm1Value::MovieClip("_root".to_owned());
                }
                next_reg += 1;
            }

            // 5. preload_parent (0x0080)
            if (flags & 0x0080) != 0 {
                let parent_val = match &this_val {
                    Avm1Value::MovieClipIdentity(t) if t != "_root" && !t.is_empty() => {
                        if let Some((p, _)) = t.rsplit_once('/') {
                            Avm1Value::MovieClipIdentity(p.to_owned())
                        } else {
                            Avm1Value::MovieClip("_root".to_owned())
                        }
                    }
                    _ => {
                        let base = self.canonical_movie_target(&func.base_target);
                        if base == "_root" || base.is_empty() {
                            Avm1Value::Undefined
                        } else if let Some((p, _)) = base.rsplit_once('/') {
                            Avm1Value::MovieClipIdentity(p.to_owned())
                        } else {
                            Avm1Value::MovieClip("_root".to_owned())
                        }
                    }
                };
                if next_reg < 256 {
                    self.avm1_registers[next_reg] = parent_val;
                }
                next_reg += 1;
            }

            // 6. preload_global (0x0100)
            if (flags & 0x0100) != 0 && next_reg < 256 {
                self.avm1_registers[next_reg] = Avm1Value::MovieClip("_global".to_owned());
            }

            for (i, (reg, name)) in func.params.iter().enumerate() {
                let val = args.get(i).cloned().unwrap_or(Avm1Value::Undefined);
                if *reg != 0 {
                    self.avm1_registers[usize::from(*reg)] = val;
                } else if !name.is_empty() {
                    fn_locals.insert(name.clone(), val);
                }
            }
        } else {
            fn_locals.insert("this".to_owned(), this_val);
            fn_locals.insert("super".to_owned(), super_val);
            let mut arg_props = HashMap::new();
            if let Some(fn_id) = func.self_obj_id {
                arg_props.insert("callee".to_owned(), Avm1Value::Object(fn_id));
            }
            let args_obj_id = self.avm1_objects.len();
            self.avm1_objects.push(Avm1Object {
                props: arg_props,
                array_elements: Some(args.clone()),
                ..Default::default()
            });
            fn_locals.insert("arguments".to_owned(), Avm1Value::Object(args_obj_id));
            for (i, (_, name)) in func.params.iter().enumerate() {
                let val = args.get(i).cloned().unwrap_or(Avm1Value::Undefined);
                if !name.is_empty() {
                    fn_locals.insert(name.clone(), val);
                }
            }
        }

        let act_id = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object {
            props: fn_locals,
            ..Default::default()
        });
        let saved_scopes =
            std::mem::replace(&mut self.avm1_scope_chain, func.captured_scopes.clone());
        self.avm1_scope_chain.push(act_id);

        let mut fn_stack = Vec::new();
        let mut empty_locals = HashMap::new();
        let ret = self
            .run_avm1_code(
                &func.body,
                &mut fn_stack,
                &mut empty_locals,
                steps,
                depth + 1,
            )
            .unwrap_or(Avm1Value::Undefined);
        self.avm1_scope_chain = saved_scopes;
        self.avm1_constant_pool = saved_pool;
        self.avm1_registers = saved_regs;
        self.avm1_target = saved_target;
        self.avm1_with_scopes = saved_with;
        ret
    }
}
