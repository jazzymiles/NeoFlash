//! AVM2 property lookup, mutation, deletion, `in`/`hasOwnProperty`, and `super` access.

use super::super::*;

impl SwfBytecodePlayer {
    pub(in crate::swf::player) fn avm2_class_token_has_static_prop(
        &self,
        mc: &str,
        prop: &str,
    ) -> bool {
        let Some((c_abc, c_idx)) = parse_avm2_class_token(mc) else {
            return false;
        };
        let mut cur = Some((c_abc, c_idx));
        for _ in 0..32 {
            let Some((a, c)) = cur else { break };
            if self
                .avm2_globals
                .contains_key(&format!("__avm2_class:{a}:{c}.{prop}"))
            {
                return true;
            }
            if let Some(cls) = self.abc_tags.get(a).and_then(|l| l.abc.classes.get(c))
                && cls
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

    pub(in crate::swf::player) fn avm2_object_has_prop(&self, obj_id: usize, prop: &str) -> bool {
        let Some(obj) = self.avm1_objects.get(obj_id) else {
            return false;
        };
        if let Some((vname, _, _)) = &obj.avm2_catch_var
            && !vname.is_empty()
            && vname == prop
        {
            return true;
        }
        if obj.avm2_xml.is_some() {
            return self.avm2_xml_has_property(obj_id, prop);
        }
        if let Some(dict) = &obj.avm2_dictionary {
            return dict.contains(&Avm2DictKey::String(prop.to_owned()));
        }
        if let Some(vd) = &obj.avm2_vector {
            return matches!(prop, "length" | "fixed")
                || matches!(
                    Self::parse_avm2_vector_index(prop),
                    Some(Ok(i)) if i >= 0 && (i as usize) < vd.elements.len()
                );
        }
        if obj.props.contains_key(prop) {
            return true;
        }
        if obj.avm2_display.is_some()
            && matches!(
                prop,
                "x" | "y"
                    | "scaleX"
                    | "scaleY"
                    | "alpha"
                    | "visible"
                    | "rotation"
                    | "name"
                    | "mask"
                    | "stage"
                    | "parent"
                    | "root"
                    | "currentFrame"
                    | "totalFrames"
                    | "numChildren"
                    | "graphics"
                    | "addEventListener"
                    | "removeEventListener"
                    | "hasEventListener"
                    | "willTrigger"
                    | "dispatchEvent"
                    | "addFrameScript"
                    | "getChildAt"
                    | "getChildByName"
                    | "getChildIndex"
                    | "contains"
                    | "addChild"
                    | "removeChild"
                    | "stop"
                    | "play"
                    | "nextFrame"
                    | "prevFrame"
                    | "gotoAndStop"
                    | "gotoAndPlay"
            )
        {
            return true;
        }
        if let Some(Avm1Value::MovieClipIdentity(disp_target)) = obj.props.get("__display_target__")
        {
            if disp_target == "_root" {
                if self
                    .display_list
                    .iter()
                    .any(|(d, s)| slot_matches_segment(*d, s, prop))
                {
                    return true;
                }
            } else if let Some(slot) = self.target_slot(disp_target)
                && slot
                    .children
                    .iter()
                    .any(|(d, s)| slot_matches_segment(*d, s, prop))
            {
                return true;
            }
        }
        if let Some(elems) = &obj.array_elements
            && (prop == "length"
                || prop
                    .parse::<usize>()
                    .is_ok_and(|idx| idx < elems.len() && !obj.deleted_indices.contains(&idx)))
        {
            return true;
        }
        if let Some((c_abc, c_idx)) = obj.avm2_class {
            let mut cur = Some((c_abc, c_idx));
            for _ in 0..32 {
                let Some((a, c)) = cur else { break };
                if let Some(inst) = self.abc_tags.get(a).and_then(|l| l.abc.instances.get(c)) {
                    for tr in &inst.traits {
                        if self.abc_tags[a].abc.split_qname(tr.name).1 == prop {
                            return true;
                        }
                    }
                }
                cur = self.avm2_superclass(a, c);
            }
        }
        false
    }

    pub(in crate::swf::player) fn avm2_has_property(&self, obj: &Avm1Value, prop: &str) -> bool {
        match obj {
            Avm1Value::Object(id) => self.avm2_object_has_prop(*id, prop),
            Avm1Value::MovieClip(mc) | Avm1Value::MovieClipIdentity(mc) => {
                let target = self.canonical_movie_target(mc);
                if target == "_root" {
                    self.avm2_globals.contains_key(prop)
                        || self
                            .display_list
                            .iter()
                            .any(|(d, s)| slot_matches_segment(*d, s, prop))
                } else if let Some(slot) = self.target_slot(&target) {
                    matches!(
                        prop,
                        "x" | "y"
                            | "scaleX"
                            | "scaleY"
                            | "alpha"
                            | "visible"
                            | "rotation"
                            | "name"
                            | "mask"
                            | "currentFrame"
                            | "totalFrames"
                    ) || slot.props.contains_key(prop)
                        || slot
                            .children
                            .iter()
                            .any(|(d, s)| slot_matches_segment(*d, s, prop))
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    pub(in crate::swf::player) fn avm2_in_operator(
        &self,
        raw_key: &Avm1Value,
        obj: &Avm1Value,
    ) -> bool {
        if let Avm1Value::Object(oid) = obj {
            if let Some(dict) = self
                .avm1_objects
                .get(*oid)
                .and_then(|o| o.avm2_dictionary.as_ref())
            {
                let (dk, _) = Avm2DictionaryData::key_from_value(raw_key, &self.avm1_objects);
                return dict.contains(&dk);
            }
            if self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_xml.is_some())
            {
                let key_str = avm2_convert_string(raw_key, &self.avm1_objects);
                if matches!(
                    key_str.as_str(),
                    "hasOwnProperty"
                        | "propertyIsEnumerable"
                        | "isPrototypeOf"
                        | "setPropertyIsEnumerable"
                        | "toString"
                        | "toLocaleString"
                        | "valueOf"
                ) {
                    return true;
                }
                return self.avm2_xml_has_property(*oid, &key_str);
            }
        }
        let key_str = avm2_convert_string(raw_key, &self.avm1_objects);
        self.avm2_has_property(obj, &key_str)
    }

    pub(in crate::swf::player) fn get_avm2_property_by_key(
        &mut self,
        obj: &Avm1Value,
        mn_key: Avm2MultinameKey,
        abc_idx: usize,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::Object(oid) = obj
            && let Avm2MultinameKey::Runtime {
                ref key,
                is_attr: false,
            } = mn_key
            && self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_dictionary.is_some())
        {
            let (dk, _) = Avm2DictionaryData::key_from_value(key, &self.avm1_objects);
            return self
                .avm1_objects
                .get(*oid)
                .and_then(|o| o.avm2_dictionary.as_ref())
                .and_then(|d| d.get(&dk))
                .cloned()
                .unwrap_or(Avm1Value::Undefined);
        }
        let prop = mn_key.to_prop_string(&self.avm1_objects);
        let _ = abc_idx;
        self.get_avm2_property_eval(obj, &prop, steps, depth)
    }

    pub(in crate::swf::player) fn set_avm2_property_by_key(
        &mut self,
        obj: &Avm1Value,
        mn_key: Avm2MultinameKey,
        val: Avm1Value,
        abc_idx: usize,
        steps: &mut usize,
        depth: usize,
    ) {
        if let Avm1Value::Object(oid) = obj
            && self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_dictionary.is_some())
        {
            let (dk, raw_k) = match mn_key {
                Avm2MultinameKey::Runtime {
                    key,
                    is_attr: false,
                } => Avm2DictionaryData::key_from_value(&key, &self.avm1_objects),
                other => {
                    let s = other.to_prop_string(&self.avm1_objects);
                    (Avm2DictKey::String(s.clone()), Avm1Value::String(s))
                }
            };
            if let Some(dict) = self
                .avm1_objects
                .get_mut(*oid)
                .and_then(|o| o.avm2_dictionary.as_mut())
            {
                dict.set(dk, raw_k, val);
            }
            return;
        }
        let prop = mn_key.to_prop_string(&self.avm1_objects);
        if let Avm1Value::Object(oid) = obj {
            if self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_vector.is_some())
            {
                let _ = self.set_avm2_vector_property(*oid, &prop, val, abc_idx, steps, depth);
                return;
            }
            if self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_xml.is_some())
            {
                let _ = self.set_avm2_xml_property(*oid, &prop, val, abc_idx, steps, depth);
                return;
            }
            if let Some((vname, c_abc, c_ty)) = self
                .avm1_objects
                .get(*oid)
                .and_then(|o| o.avm2_catch_var.clone())
                && !vname.is_empty()
                && vname == prop
            {
                let coerced = if c_ty != 0 {
                    self.avm2_coerce_multiname(c_abc, c_ty, val, steps, depth)
                } else {
                    val
                };
                if self.avm2_thrown.is_none() {
                    self.set_avm1_member(obj, &prop, coerced);
                }
                return;
            }
        }
        if let Avm1Value::MovieClip(mc) = obj
            && mc == "__avm2_type:XML"
        {
            let b = val.to_bool(9);
            let mut arena = self.e4x_arena.borrow_mut();
            match prop.as_str() {
                "ignoreComments" => arena.settings.ignore_comments = b,
                "ignoreProcessingInstructions" => arena.settings.ignore_processing_instructions = b,
                "ignoreWhitespace" => arena.settings.ignore_whitespace = b,
                "prettyPrinting" => arena.settings.pretty_printing = b,
                "prettyIndent" => {
                    arena.settings.pretty_indent =
                        ecma_f64_to_i32(avm2_to_number(&val)).clamp(0, 64) as usize;
                }
                _ => {}
            }
            return;
        }
        self.set_avm2_property_eval(obj, &prop, val, steps, depth);
    }

    pub(in crate::swf::player) fn delete_avm2_property_by_key(
        &mut self,
        obj: &Avm1Value,
        mn_key: Avm2MultinameKey,
    ) -> bool {
        if let Avm1Value::Object(oid) = obj {
            if self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_dictionary.is_some())
            {
                let (dk, _) = match mn_key {
                    Avm2MultinameKey::Runtime {
                        key,
                        is_attr: false,
                    } => Avm2DictionaryData::key_from_value(&key, &self.avm1_objects),
                    other => {
                        let s = other.to_prop_string(&self.avm1_objects);
                        (Avm2DictKey::String(s.clone()), Avm1Value::String(s))
                    }
                };
                if let Some(d) = self
                    .avm1_objects
                    .get_mut(*oid)
                    .and_then(|o| o.avm2_dictionary.as_mut())
                {
                    let _ = d.remove(&dk);
                    return true;
                }
                return false;
            }
            let prop = mn_key.to_prop_string(&self.avm1_objects);
            if self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_vector.is_some())
            {
                return false;
            }
            if let Some(del) = self.delete_avm2_xml_property(*oid, &prop) {
                return del;
            }
            return self.delete_avm2_property(obj, &prop);
        }
        let prop = mn_key.to_prop_string(&self.avm1_objects);
        self.delete_avm2_property(obj, &prop)
    }

    pub(in crate::swf::player) fn delete_avm2_property(
        &mut self,
        obj: &Avm1Value,
        prop: &str,
    ) -> bool {
        match obj {
            Avm1Value::Object(id) => {
                if let Some((c_abc, c_idx)) = self.avm1_objects.get(*id).and_then(|o| o.avm2_class)
                {
                    // Sealed/fixed traits cannot be deleted in AVM2
                    let mut cur = Some((c_abc, c_idx));
                    for _ in 0..32 {
                        let Some((a, c)) = cur else { break };
                        if let Some(inst) =
                            self.abc_tags.get(a).and_then(|l| l.abc.instances.get(c))
                        {
                            for tr in &inst.traits {
                                if self.abc_tags[a].abc.split_qname(tr.name).1 == prop {
                                    return false;
                                }
                            }
                        }
                        cur = self.avm2_superclass(a, c);
                    }
                }
                if let Some(o) = self.avm1_objects.get_mut(*id) {
                    if let Some(elems) = &mut o.array_elements
                        && let Ok(idx) = prop.parse::<usize>()
                    {
                        if idx < elems.len() {
                            elems[idx] = Avm1Value::Undefined;
                            o.deleted_indices.insert(idx);
                        }
                        return true;
                    }
                    let _ = o.props.remove(prop);
                    return true;
                }
                false
            }
            Avm1Value::MovieClip(mc) | Avm1Value::MovieClipIdentity(mc) => {
                let target = self.canonical_movie_target(mc);
                if target != "_root"
                    && let Some(slot) = self.target_slot_mut(&target)
                {
                    let _ = slot.props.remove(prop);
                    true
                } else {
                    let _ = self.avm2_globals.remove(prop);
                    true
                }
            }
            _ => false,
        }
    }

    pub(in crate::swf::player) fn get_avm2_object_graphics_proxy(
        &mut self,
        oid: usize,
    ) -> Avm1Value {
        if let Some(Avm1Value::Object(pid)) = self
            .avm1_objects
            .get(oid)
            .and_then(|o| o.props.get("__graphics_proxy__").cloned())
        {
            return Avm1Value::Object(pid);
        }
        if let Some(o) = self.avm1_objects.get_mut(oid)
            && o.graphics.is_none()
        {
            o.graphics = Some(RuntimeGraphicsState::default());
        }
        let proxy_id = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object {
            avm2_graphics_owner: Some(oid),
            ..Default::default()
        });
        if let Some(o) = self.avm1_objects.get_mut(oid) {
            o.props
                .insert("__graphics_proxy__".to_owned(), Avm1Value::Object(proxy_id));
        }
        Avm1Value::Object(proxy_id)
    }

    pub(in crate::swf::player) fn get_avm2_property_eval(
        &mut self,
        obj: &Avm1Value,
        prop: &str,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::MovieClip(mc) = obj {
            if mc == "__avm2_type:XML" || mc == "XML" {
                let arena = self.e4x_arena.borrow();
                match prop {
                    "ignoreComments" => return Avm1Value::Bool(arena.settings.ignore_comments),
                    "ignoreProcessingInstructions" => {
                        return Avm1Value::Bool(arena.settings.ignore_processing_instructions);
                    }
                    "ignoreWhitespace" => return Avm1Value::Bool(arena.settings.ignore_whitespace),
                    "prettyPrinting" => return Avm1Value::Bool(arena.settings.pretty_printing),
                    "prettyIndent" => {
                        return Avm1Value::Number(arena.settings.pretty_indent as f64);
                    }
                    _ => {}
                }
            }
            if mc == "__avm2_type:ApplicationDomain" && prop == "currentDomain" {
                return Avm1Value::MovieClip("__avm2_type:ApplicationDomain".to_owned());
            }
        }
        if let Avm1Value::Object(id) = obj {
            if self
                .avm1_objects
                .get(*id)
                .is_some_and(|o| o.props.contains_key("__avm2_global_scope__"))
            {
                return self.get_avm2_property_eval(
                    &Avm1Value::MovieClip("_root".to_owned()),
                    prop,
                    steps,
                    depth,
                );
            }
            if self
                .avm1_objects
                .get(*id)
                .is_some_and(|o| o.avm2_vector.is_some())
                && let Some(val) = self.get_avm2_vector_property(*id, prop)
            {
                return val;
            }
            if self
                .avm1_objects
                .get(*id)
                .is_some_and(|o| o.avm2_xml.is_some())
                && let Some(val) = self.get_avm2_xml_property(*id, prop)
            {
                return val;
            }
            if let Some(val) = self
                .avm1_objects
                .get(*id)
                .and_then(|o| o.avm2_dictionary.as_ref())
                .map(|d| {
                    d.get(&Avm2DictKey::String(prop.to_owned()))
                        .cloned()
                        .unwrap_or(Avm1Value::Undefined)
                })
            {
                return val;
            }
            if let Some((ev_ty, bubbles, cancelable, target_oid, current_oid, phase)) = self
                .avm1_objects
                .get(*id)
                .and_then(|o| o.avm2_event.clone())
            {
                match prop {
                    "type" => return Avm1Value::String(ev_ty),
                    "bubbles" => return Avm1Value::Bool(bubbles),
                    "cancelable" => return Avm1Value::Bool(cancelable),
                    "target" => return Avm1Value::Object(target_oid),
                    "currentTarget" => return Avm1Value::Object(current_oid),
                    "eventPhase" => return Avm1Value::Number(f64::from(phase)),
                    _ => {}
                }
            }
            if prop == "graphics" {
                let is_disp = self.avm1_objects.get(*id).is_some_and(|o| {
                    o.graphics.is_some()
                        || o.avm2_display.is_some()
                        || o.props.contains_key("__avm2_display_type__")
                        || o.avm2_class.is_some_and(|(ca, cc)| {
                            self.avm2_class_extends_builtin(ca, cc, "Sprite")
                                || self.avm2_class_extends_builtin(ca, cc, "Shape")
                        })
                });
                if is_disp {
                    return self.get_avm2_object_graphics_proxy(*id);
                }
            }
            if let Some(meta) = self
                .avm1_objects
                .get(*id)
                .and_then(|o| o.avm2_display.clone())
            {
                match prop {
                    "stage" => {
                        return if meta.on_stage {
                            self.avm2_stage_oid
                                .map_or(Avm1Value::Null, Avm1Value::Object)
                        } else {
                            Avm1Value::Null
                        };
                    }
                    "parent" => {
                        return meta.parent_oid.map_or(Avm1Value::Null, Avm1Value::Object);
                    }
                    "root" => {
                        return if meta.on_stage {
                            self.avm2_root_oid
                                .map_or(Avm1Value::Null, Avm1Value::Object)
                        } else {
                            Avm1Value::Null
                        };
                    }
                    "currentFrame" => {
                        let f = if Some(*id) == self.avm2_root_oid {
                            self.current_frame + 1
                        } else {
                            meta.current_frame + 1
                        };
                        return Avm1Value::Number(f64::from(f));
                    }
                    "totalFrames" => {
                        let tf = if Some(*id) == self.avm2_root_oid {
                            self.root_timeline.total_frames
                        } else {
                            meta.total_frames.max(1)
                        };
                        return Avm1Value::Number(f64::from(tf));
                    }
                    "numChildren" => {
                        let count = meta
                            .children_oids
                            .iter()
                            .filter(|&&cid| {
                                self.avm1_objects
                                    .get(cid)
                                    .and_then(|co| co.avm2_display.as_ref())
                                    .is_some_and(|cm| cm.constructed)
                            })
                            .count();
                        return Avm1Value::Number(count as f64);
                    }
                    _ => {}
                }
            }
            if let Some(Avm1Value::MovieClipIdentity(disp_target)) = self
                .avm1_objects
                .get(*id)
                .and_then(|o| o.props.get("__display_target__").cloned())
                && disp_target != "_root"
            {
                if prop == "mask" {
                    return match self
                        .target_slot(&disp_target)
                        .and_then(|s| s.masked_by.clone())
                    {
                        None => Avm1Value::Null,
                        Some(mask_target) => self
                            .target_slot(&mask_target)
                            .and_then(|s| s.avm2_object_id)
                            .map_or(Avm1Value::MovieClipIdentity(mask_target), Avm1Value::Object),
                    };
                }
                if matches!(
                    prop,
                    "x" | "y" | "scaleX" | "scaleY" | "alpha" | "visible" | "rotation"
                ) {
                    return self
                        .get_avm2_property(&Avm1Value::MovieClipIdentity(disp_target), prop);
                }
            }
            if let Some((c_abc, c_idx)) = self.avm1_objects.get(*id).and_then(|o| o.avm2_class) {
                let effective_prop = self
                    .resolve_avm2_slot_name(c_abc, c_idx, prop)
                    .unwrap_or_else(|| prop.to_owned());
                let mut cur = Some((c_abc, c_idx));
                for _ in 0..32 {
                    let Some((a, c)) = cur else { break };
                    let found_trait = self
                        .abc_tags
                        .get(a)
                        .and_then(|l| l.abc.instances.get(c))
                        .and_then(|inst| {
                            inst.traits
                                .iter()
                                .find(|tr| {
                                    !matches!(tr.data, AbcTraitData::Setter { .. })
                                        && self.abc_tags[a].abc.split_qname(tr.name).1
                                            == effective_prop
                                })
                                .map(|tr| tr.data.clone())
                        });
                    if let Some(td) = found_trait {
                        match td {
                            AbcTraitData::Getter { method } => {
                                return self.run_avm2_method_with_this(
                                    a,
                                    method,
                                    Avm1Value::Object(*id),
                                    Vec::new(),
                                    steps,
                                    depth + 1,
                                );
                            }
                            AbcTraitData::Method { method } | AbcTraitData::Function { method } => {
                                return Avm1Value::String(format!(
                                    "__avm2_bound:{id}:{a}:{method}"
                                ));
                            }
                            _ => break,
                        }
                    }
                    cur = self.avm2_superclass(a, c);
                }
                if self
                    .avm1_objects
                    .get(*id)
                    .is_none_or(|o| !o.props.contains_key(&effective_prop))
                    && let Some(Avm1Value::MovieClipIdentity(disp_target)) = self
                        .avm1_objects
                        .get(*id)
                        .and_then(|o| o.props.get("__display_target__").cloned())
                {
                    if disp_target == "_root" {
                        if let Some((d, s)) = self.display_list.iter().find_map(|(d, s)| {
                            slot_matches_segment(*d, s, &effective_prop).then_some((*d, s))
                        }) {
                            if let Some(c_oid) = s.avm2_object_id {
                                if self
                                    .avm1_objects
                                    .get(c_oid)
                                    .and_then(|co| co.avm2_display.as_ref())
                                    .is_some_and(|cm| cm.constructed)
                                {
                                    return Avm1Value::Object(c_oid);
                                }
                            } else {
                                return Avm1Value::MovieClipIdentity(live_slot_segment(d, s));
                            }
                        }
                    } else if let Some(pslot) = self.target_slot(&disp_target)
                        && let Some((d, s)) = pslot.children.iter().find_map(|(d, s)| {
                            slot_matches_segment(*d, s, &effective_prop).then_some((*d, s))
                        })
                    {
                        if let Some(c_oid) = s.avm2_object_id {
                            if self
                                .avm1_objects
                                .get(c_oid)
                                .and_then(|co| co.avm2_display.as_ref())
                                .is_some_and(|cm| cm.constructed)
                            {
                                return Avm1Value::Object(c_oid);
                            }
                        } else {
                            return Avm1Value::MovieClipIdentity(join_target_path(
                                &disp_target,
                                &live_slot_segment(d, s),
                            ));
                        }
                    }
                }
                return self.get_avm1_member(obj, &effective_prop);
            }
            return self.get_avm1_member(obj, prop);
        }
        if let Avm1Value::String(s) = obj
            && !is_avm2_fn_token(s)
            && prop == "length"
        {
            return Avm1Value::Number(s.encode_utf16().count() as f64);
        }
        self.get_avm2_property(obj, prop)
    }

    pub(in crate::swf::player) fn get_avm2_property(
        &self,
        obj: &Avm1Value,
        prop: &str,
    ) -> Avm1Value {
        if let Avm1Value::Object(id) = obj {
            if self
                .avm1_objects
                .get(*id)
                .is_some_and(|o| o.props.contains_key("__avm2_global_scope__"))
            {
                return self.get_avm2_property(&Avm1Value::MovieClip("_root".to_owned()), prop);
            }
            return self.get_avm1_member(obj, prop);
        }
        if let Avm1Value::MovieClip(mc) | Avm1Value::MovieClipIdentity(mc) = obj {
            if let Some((c_abc, c_idx)) = parse_avm2_class_token(mc) {
                let mut cur = Some((c_abc, c_idx));
                for _ in 0..32 {
                    let Some((a, c)) = cur else { break };
                    if let Some(v) = self
                        .avm2_globals
                        .get(&format!("__avm2_class:{a}:{c}.{prop}"))
                    {
                        return v.clone();
                    }
                    if let Some(loaded) = self.abc_tags.get(a)
                        && let Some(cls) = loaded.abc.classes.get(c)
                    {
                        for tr in &cls.traits {
                            let (_, tname) = loaded.abc.split_qname(tr.name);
                            if tname == prop {
                                match tr.data {
                                    AbcTraitData::Slot { vindex, vkind, .. } => {
                                        return if vindex != 0 || vkind != 0 {
                                            abc_const_to_avm_value(&loaded.abc, vindex, vkind)
                                        } else {
                                            Avm1Value::Undefined
                                        };
                                    }
                                    AbcTraitData::Method { method }
                                    | AbcTraitData::Function { method } => {
                                        return Avm1Value::String(format!(
                                            "__avm2_fn:{a}:{method}"
                                        ));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    cur = self.avm2_superclass(a, c);
                }
                return Avm1Value::Undefined;
            }
            if mc == "Math" || mc == "__avm2_type:Math" {
                return match prop {
                    "PI" => Avm1Value::Number(std::f64::consts::PI),
                    "E" => Avm1Value::Number(std::f64::consts::E),
                    "SQRT2" => Avm1Value::Number(std::f64::consts::SQRT_2),
                    "LN2" => Avm1Value::Number(std::f64::consts::LN_2),
                    "LN10" => Avm1Value::Number(std::f64::consts::LN_10),
                    _ => Avm1Value::Undefined,
                };
            }
            if mc == "__avm2_type:Event" || mc == "Event" {
                return match prop {
                    "ADDED" => Avm1Value::String("added".to_owned()),
                    "ADDED_TO_STAGE" => Avm1Value::String("addedToStage".to_owned()),
                    "REMOVED" => Avm1Value::String("removed".to_owned()),
                    "REMOVED_FROM_STAGE" => Avm1Value::String("removedFromStage".to_owned()),
                    "ENTER_FRAME" => Avm1Value::String("enterFrame".to_owned()),
                    "FRAME_CONSTRUCTED" => Avm1Value::String("frameConstructed".to_owned()),
                    "EXIT_FRAME" => Avm1Value::String("exitFrame".to_owned()),
                    "COMPLETE" => Avm1Value::String("complete".to_owned()),
                    "INIT" => Avm1Value::String("init".to_owned()),
                    "OPEN" => Avm1Value::String("open".to_owned()),
                    "RENDER" => Avm1Value::String("render".to_owned()),
                    _ => Avm1Value::Undefined,
                };
            }
            if mc == "__avm2_type:MouseEvent" || mc == "MouseEvent" {
                return match prop {
                    "CLICK" => Avm1Value::String("click".to_owned()),
                    "MOUSE_DOWN" => Avm1Value::String("mouseDown".to_owned()),
                    "MOUSE_UP" => Avm1Value::String("mouseUp".to_owned()),
                    "MOUSE_MOVE" => Avm1Value::String("mouseMove".to_owned()),
                    "MOUSE_OVER" => Avm1Value::String("mouseOver".to_owned()),
                    "MOUSE_OUT" => Avm1Value::String("mouseOut".to_owned()),
                    "ROLL_OVER" => Avm1Value::String("rollOver".to_owned()),
                    "ROLL_OUT" => Avm1Value::String("rollOut".to_owned()),
                    _ => Avm1Value::Undefined,
                };
            }
            if mc == "__avm2_type:KeyboardEvent" || mc == "KeyboardEvent" {
                return match prop {
                    "KEY_DOWN" => Avm1Value::String("keyDown".to_owned()),
                    "KEY_UP" => Avm1Value::String("keyUp".to_owned()),
                    _ => Avm1Value::Undefined,
                };
            }
            if mc == "__avm2_type:Number" {
                return match prop {
                    "NaN" => Avm1Value::Number(f64::NAN),
                    "POSITIVE_INFINITY" => Avm1Value::Number(f64::INFINITY),
                    "NEGATIVE_INFINITY" => Avm1Value::Number(f64::NEG_INFINITY),
                    "MAX_VALUE" => Avm1Value::Number(f64::MAX),
                    "MIN_VALUE" => Avm1Value::Number(f64::MIN_POSITIVE),
                    _ => Avm1Value::Undefined,
                };
            }
            if mc == "__avm2_type:int" {
                return match prop {
                    "MAX_VALUE" => Avm1Value::Number(f64::from(i32::MAX)),
                    "MIN_VALUE" => Avm1Value::Number(f64::from(i32::MIN)),
                    _ => Avm1Value::Undefined,
                };
            }
            if mc == "__avm2_type:uint" {
                return match prop {
                    "MAX_VALUE" => Avm1Value::Number(f64::from(u32::MAX)),
                    "MIN_VALUE" => Avm1Value::Number(0.0),
                    _ => Avm1Value::Undefined,
                };
            }
            let target = self.canonical_movie_target(mc);
            if target != "_root" && !target.is_empty() && target != "Math" && mc != prop {
                if self.target_slot(&target).is_some() {
                    match prop {
                        "x" => return self.get_mc_property_identity(&target, 0),
                        "y" => return self.get_mc_property_identity(&target, 1),
                        "scaleX" => {
                            return Avm1Value::Number(
                                self.get_mc_property_identity(&target, 2).to_number() / 100.0,
                            );
                        }
                        "scaleY" => {
                            return Avm1Value::Number(
                                self.get_mc_property_identity(&target, 3).to_number() / 100.0,
                            );
                        }
                        "currentFrame" => return self.get_mc_property_identity(&target, 4),
                        "totalFrames" => return self.get_mc_property_identity(&target, 5),
                        "alpha" => {
                            return Avm1Value::Number(
                                self.get_mc_property_identity(&target, 6).to_number() / 100.0,
                            );
                        }
                        "visible" => return self.get_mc_property_identity(&target, 7),
                        "rotation" => return self.get_mc_property_identity(&target, 10),
                        "name" => return self.get_mc_property_identity(&target, 13),
                        "numChildren" => {
                            return Avm1Value::Number(
                                self.target_slot(&target)
                                    .map_or(0.0, |s| s.children.len() as f64),
                            );
                        }
                        "graphics" => {
                            return Avm1Value::MovieClip(format!("__avm2_graphics:{target}"));
                        }
                        "mask" => {
                            return self
                                .target_slot(&target)
                                .and_then(|s| s.masked_by.clone())
                                .map_or(Avm1Value::Null, Avm1Value::MovieClipIdentity);
                        }
                        _ => {
                            if let Some(slot) = self.target_slot(&target) {
                                if let Some(v) = slot.props.get(prop) {
                                    return v.clone();
                                }
                                if let Some((depth, child)) =
                                    slot.children.iter().find_map(|(depth, child)| {
                                        slot_matches_segment(*depth, child, prop)
                                            .then_some((*depth, child))
                                    })
                                {
                                    return Avm1Value::MovieClipIdentity(join_target_path(
                                        &target,
                                        &live_slot_segment(depth, child),
                                    ));
                                }
                            }
                            return Avm1Value::MovieClip(format!("{target}.{prop}"));
                        }
                    }
                }
                let dotted = format!("{mc}.{prop}");
                if let Some(v) = self.avm2_globals.get(&dotted) {
                    return v.clone();
                }
                return Avm1Value::MovieClip(dotted);
            }
        }
        if let Some(v) = self.avm2_globals.get(prop) {
            return v.clone();
        }
        // Check if `prop` is a method on the bound root document/timeline class first, then any timeline class
        if let Some(doc_cls) = self.symbol_classes.get(&0) {
            let (dpkg, dshort) = match doc_cls.rsplit_once('.') {
                Some((p, s)) => (p, s),
                None => ("", doc_cls.as_str()),
            };
            for abc_idx in 0..self.abc_tags.len() {
                if let Some((ca, cc)) = self.find_avm2_class(abc_idx, dpkg, dshort) {
                    let mut cur = Some((ca, cc));
                    for _ in 0..32 {
                        let Some((a, c)) = cur else { break };
                        if let Some(m_idx) = self
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
                            })
                        {
                            return Avm1Value::String(format!("__avm2_fn:{a}:{m_idx}"));
                        }
                        cur = self.avm2_superclass(a, c);
                    }
                }
            }
        }
        for (abc_idx, loaded) in self.abc_tags.iter().enumerate() {
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
                        return Avm1Value::String(format!("__avm2_fn:{abc_idx}:{method}"));
                    }
                }
            }
        }
        match prop {
            "stage" => self
                .avm2_stage_oid
                .map_or(Avm1Value::Null, Avm1Value::Object),
            "root" => self
                .avm2_root_oid
                .map_or(Avm1Value::Null, Avm1Value::Object),
            "currentFrame" => Avm1Value::Number(f64::from(self.current_frame + 1)),
            "totalFrames" => Avm1Value::Number(f64::from(self.root_timeline.total_frames)),
            "numChildren" => Avm1Value::Number(self.display_list.len() as f64),
            "graphics" => Avm1Value::MovieClip("__avm2_graphics:_root".into()),
            "Math" => Avm1Value::MovieClip("Math".into()),
            _ => {
                if let Some((depth, slot)) = self.display_list.iter().find_map(|(depth, slot)| {
                    slot_matches_segment(*depth, slot, prop).then_some((*depth, slot))
                }) {
                    if let Some(c_oid) = slot.avm2_object_id {
                        Avm1Value::Object(c_oid)
                    } else {
                        Avm1Value::MovieClipIdentity(live_slot_segment(depth, slot))
                    }
                } else if let Some((ca, cc)) =
                    (0..self.abc_tags.len()).find_map(|ai| self.find_avm2_class(ai, "", prop))
                {
                    Avm1Value::MovieClip(format!("__avm2_class:{ca}:{cc}"))
                } else if matches!(
                    prop,
                    "Object"
                        | "Class"
                        | "Function"
                        | "Array"
                        | "String"
                        | "Number"
                        | "int"
                        | "uint"
                        | "Boolean"
                        | "Dictionary"
                        | "XML"
                        | "XMLList"
                        | "QName"
                        | "Namespace"
                ) {
                    Avm1Value::MovieClip(format!("__avm2_type:{prop}"))
                } else {
                    Avm1Value::MovieClip(prop.to_owned())
                }
            }
        }
    }

    pub(in crate::swf::player) fn mirror_avm2_object_prop_to_display(
        &mut self,
        oid: usize,
        prop: &str,
        val: &Avm1Value,
    ) {
        if let Some(Avm1Value::MovieClipIdentity(disp_target)) = self
            .avm1_objects
            .get(oid)
            .and_then(|o| o.props.get("__display_target__").cloned())
            && matches!(
                prop,
                "x" | "y"
                    | "scaleX"
                    | "scaleY"
                    | "alpha"
                    | "visible"
                    | "rotation"
                    | "name"
                    | "mask"
            )
        {
            self.set_avm2_property(
                &Avm1Value::MovieClipIdentity(disp_target),
                prop,
                val.clone(),
            );
        }
    }

    pub(in crate::swf::player) fn set_avm2_property_eval(
        &mut self,
        obj: &Avm1Value,
        prop: &str,
        val: Avm1Value,
        steps: &mut usize,
        depth: usize,
    ) {
        if let Avm1Value::Object(id) = obj
            && let Some((c_abc, c_idx)) = self.avm1_objects.get(*id).and_then(|o| o.avm2_class)
        {
            let effective_prop = self
                .resolve_avm2_slot_name(c_abc, c_idx, prop)
                .unwrap_or_else(|| prop.to_owned());
            let mut cur = Some((c_abc, c_idx));
            for _ in 0..32 {
                let Some((a, c)) = cur else { break };
                let setter_or_slot = self
                    .abc_tags
                    .get(a)
                    .and_then(|l| l.abc.instances.get(c))
                    .and_then(|inst| {
                        inst.traits
                            .iter()
                            .find(|tr| {
                                !matches!(tr.data, AbcTraitData::Getter { .. })
                                    && self.abc_tags[a].abc.split_qname(tr.name).1 == effective_prop
                            })
                            .map(|tr| tr.data.clone())
                    });
                if let Some(td) = setter_or_slot {
                    match td {
                        AbcTraitData::Setter { method } => {
                            let _ = self.run_avm2_method_with_this(
                                a,
                                method,
                                Avm1Value::Object(*id),
                                vec![val],
                                steps,
                                depth + 1,
                            );
                            return;
                        }
                        AbcTraitData::Slot { type_name, .. } => {
                            let coerced =
                                self.avm2_coerce_multiname(a, type_name, val, steps, depth);
                            if self.avm2_thrown.is_none() {
                                self.mirror_avm2_object_prop_to_display(
                                    *id,
                                    &effective_prop,
                                    &coerced,
                                );
                                self.set_avm1_member(obj, &effective_prop, coerced);
                            }
                            return;
                        }
                        _ => break,
                    }
                }
                cur = self.avm2_superclass(a, c);
            }
            self.mirror_avm2_object_prop_to_display(*id, &effective_prop, &val);
            self.set_avm1_member(obj, &effective_prop, val);
            return;
        }
        self.set_avm2_property(obj, prop, val);
    }

    pub(in crate::swf::player) fn set_avm2_property(
        &mut self,
        obj: &Avm1Value,
        prop: &str,
        val: Avm1Value,
    ) {
        match obj {
            Avm1Value::Object(id) => {
                self.mirror_avm2_object_prop_to_display(*id, prop, &val);
                self.set_avm1_member(obj, prop, val);
            }
            Avm1Value::MovieClip(mc) | Avm1Value::MovieClipIdentity(mc) => {
                if mc.starts_with("__avm2_class:") {
                    self.avm2_globals.insert(format!("{mc}.{prop}"), val);
                    return;
                }
                let target = self.canonical_movie_target(mc);
                if target != "_root" && !target.is_empty() && target != "Math" && mc != prop {
                    if self.target_slot(&target).is_some() {
                        match prop {
                            "x" => self.set_mc_property_identity(
                                &target,
                                0,
                                Avm1Value::Number(avm2_to_number(&val)),
                            ),
                            "y" => self.set_mc_property_identity(
                                &target,
                                1,
                                Avm1Value::Number(avm2_to_number(&val)),
                            ),
                            "scaleX" => self.set_mc_property_identity(
                                &target,
                                2,
                                Avm1Value::Number(avm2_to_number(&val) * 100.0),
                            ),
                            "scaleY" => self.set_mc_property_identity(
                                &target,
                                3,
                                Avm1Value::Number(avm2_to_number(&val) * 100.0),
                            ),
                            "alpha" => self.set_mc_property_identity(
                                &target,
                                6,
                                Avm1Value::Number(avm2_to_number(&val) * 100.0),
                            ),
                            "visible" => self.set_mc_property_identity(&target, 7, val),
                            "rotation" => self.set_mc_property_identity(
                                &target,
                                10,
                                Avm1Value::Number(avm2_to_number(&val)),
                            ),
                            "name" => self.set_mc_property_identity(&target, 13, val),
                            "mask" => self.apply_dynamic_mask(&target, Some(&val)),
                            _ => {
                                if let Some(slot) = self.target_slot_mut(&target) {
                                    slot.props.insert(prop.to_owned(), val);
                                }
                            }
                        }
                    } else {
                        self.avm2_globals.insert(format!("{mc}.{prop}"), val);
                    }
                } else {
                    self.avm2_globals.insert(prop.to_owned(), val);
                }
            }
            _ => {
                self.avm2_globals.insert(prop.to_owned(), val);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::swf::player) fn get_avm2_super_property(
        &mut self,
        caller_abc: usize,
        caller_method: usize,
        receiver: &Avm1Value,
        prop: &str,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::Object(oid) = receiver
            && let Some((owner_abc, owner_cls)) =
                self.resolve_avm2_method_owner(caller_abc, caller_method, *oid)
        {
            let mut cur = self.avm2_superclass(owner_abc, owner_cls);
            for _ in 0..32 {
                let Some((a, c)) = cur else { break };
                let found = self
                    .abc_tags
                    .get(a)
                    .and_then(|l| l.abc.instances.get(c))
                    .and_then(|inst| {
                        inst.traits
                            .iter()
                            .find(|tr| {
                                !matches!(tr.data, AbcTraitData::Setter { .. })
                                    && self.abc_tags[a].abc.split_qname(tr.name).1 == prop
                            })
                            .map(|tr| tr.data.clone())
                    });
                if let Some(td) = found {
                    match td {
                        AbcTraitData::Getter { method } => {
                            return self.run_avm2_method_with_this(
                                a,
                                method,
                                Avm1Value::Object(*oid),
                                Vec::new(),
                                steps,
                                depth + 1,
                            );
                        }
                        AbcTraitData::Method { method } | AbcTraitData::Function { method } => {
                            return Avm1Value::String(format!("__avm2_bound:{oid}:{a}:{method}"));
                        }
                        _ => break,
                    }
                }
                cur = self.avm2_superclass(a, c);
            }
            return self.get_avm1_member(receiver, prop);
        }
        Avm1Value::Undefined
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::swf::player) fn set_avm2_super_property(
        &mut self,
        caller_abc: usize,
        caller_method: usize,
        receiver: &Avm1Value,
        prop: &str,
        val: Avm1Value,
        steps: &mut usize,
        depth: usize,
    ) {
        if let Avm1Value::Object(oid) = receiver
            && let Some((owner_abc, owner_cls)) =
                self.resolve_avm2_method_owner(caller_abc, caller_method, *oid)
        {
            let mut cur = self.avm2_superclass(owner_abc, owner_cls);
            for _ in 0..32 {
                let Some((a, c)) = cur else { break };
                let found = self
                    .abc_tags
                    .get(a)
                    .and_then(|l| l.abc.instances.get(c))
                    .and_then(|inst| {
                        inst.traits
                            .iter()
                            .find(|tr| {
                                !matches!(tr.data, AbcTraitData::Getter { .. })
                                    && self.abc_tags[a].abc.split_qname(tr.name).1 == prop
                            })
                            .map(|tr| tr.data.clone())
                    });
                if let Some(td) = found {
                    match td {
                        AbcTraitData::Setter { method } => {
                            let _ = self.run_avm2_method_with_this(
                                a,
                                method,
                                Avm1Value::Object(*oid),
                                vec![val],
                                steps,
                                depth + 1,
                            );
                            return;
                        }
                        AbcTraitData::Slot { type_name, .. } => {
                            let coerced =
                                self.avm2_coerce_multiname(a, type_name, val, steps, depth);
                            if self.avm2_thrown.is_none() {
                                self.mirror_avm2_object_prop_to_display(*oid, prop, &coerced);
                                self.set_avm1_member(receiver, prop, coerced);
                            }
                            return;
                        }
                        _ => break,
                    }
                }
                cur = self.avm2_superclass(a, c);
            }
            self.mirror_avm2_object_prop_to_display(*oid, prop, &val);
            self.set_avm1_member(receiver, prop, val);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::swf::player) fn call_avm2_super_method(
        &mut self,
        caller_abc: usize,
        caller_method: usize,
        receiver: &Avm1Value,
        prop: &str,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        let owner_opt = if let Avm1Value::Object(oid) = receiver {
            self.resolve_avm2_method_owner(caller_abc, caller_method, *oid)
                .map(|(a, c)| (a, c, Avm1Value::Object(*oid)))
        } else if matches!(receiver, Avm1Value::MovieClip(mc) if mc == "_root") {
            self.abc_tags
                .get(caller_abc)
                .and_then(|l| {
                    l.abc.instances.iter().position(|inst| {
                        inst.iinit == caller_method
                            || inst.traits.iter().any(|tr| {
                                matches!(
                                    tr.data,
                                    AbcTraitData::Method { method }
                                        | AbcTraitData::Function { method }
                                        if method == caller_method
                                )
                            })
                    })
                })
                .map(|c| (caller_abc, c, receiver.clone()))
        } else {
            None
        };
        if let Some((owner_abc, owner_cls, recv_val)) = owner_opt {
            let mut cur = self.avm2_superclass(owner_abc, owner_cls);
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
                        recv_val,
                        args,
                        steps,
                        depth + 1,
                    );
                }
                cur = self.avm2_superclass(a, c);
            }
            if let Avm1Value::Object(oid) = recv_val {
                if self
                    .avm1_objects
                    .get(oid)
                    .is_some_and(|o| o.avm2_display.is_some())
                    && let Some(ret) =
                        self.call_avm2_display_builtin_method(oid, prop, &args, steps, depth)
                {
                    return ret;
                }
                if prop == "toString" {
                    let full = self.avm2_runtime_class_label(&Avm1Value::Object(oid));
                    let short = full.rsplit_once("::").map_or(full.as_str(), |(_, s)| s);
                    return Avm1Value::String(format!("[object {short}]"));
                }
                if prop == "valueOf" {
                    return Avm1Value::Object(oid);
                }
            }
        }
        Avm1Value::Undefined
    }
}
