use super::*;

impl SwfBytecodePlayer {
    pub(super) fn find_avm2_class(
        &self,
        hint_abc: usize,
        pkg: &str,
        cls_name: &str,
    ) -> Option<(usize, usize)> {
        if cls_name.is_empty() || cls_name == "*" {
            return None;
        }
        if let Some(loaded) = self.abc_tags.get(hint_abc) {
            for (c_idx, inst) in loaded.abc.instances.iter().enumerate() {
                let (ipkg, iname) = loaded.abc.split_qname(inst.name);
                if iname == cls_name && (pkg.is_empty() || ipkg == pkg) {
                    return Some((hint_abc, c_idx));
                }
            }
        }
        for (a_idx, loaded) in self.abc_tags.iter().enumerate() {
            for (c_idx, inst) in loaded.abc.instances.iter().enumerate() {
                let (ipkg, iname) = loaded.abc.split_qname(inst.name);
                if iname == cls_name && (pkg.is_empty() || ipkg == pkg) {
                    return Some((a_idx, c_idx));
                }
            }
        }
        None
    }

    pub(super) fn avm2_superclass(&self, abc_idx: usize, cls_idx: usize) -> Option<(usize, usize)> {
        let inst = self.abc_tags.get(abc_idx)?.abc.instances.get(cls_idx)?;
        if inst.super_name == 0 {
            return None;
        }
        let (spkg, sname) = self.abc_tags[abc_idx].abc.split_qname(inst.super_name);
        if sname == "Object" || sname == "*" || sname.is_empty() {
            return None;
        }
        self.find_avm2_class(abc_idx, &spkg, &sname)
    }

    pub(super) fn avm2_class_matches_or_extends(
        &self,
        mut cur_abc: usize,
        mut cur_cls: usize,
        target_abc: usize,
        target_cls: usize,
    ) -> bool {
        for _ in 0..32 {
            if cur_abc == target_abc && cur_cls == target_cls {
                return true;
            }
            let inst = match self
                .abc_tags
                .get(cur_abc)
                .and_then(|l| l.abc.instances.get(cur_cls))
            {
                Some(i) => i,
                None => break,
            };
            for &iface_mn in &inst.interfaces {
                let (ipkg, iname) = self.abc_tags[cur_abc].abc.split_qname(iface_mn);
                if let Some((ia, ic)) = self.find_avm2_class(cur_abc, &ipkg, &iname)
                    && ia == target_abc
                    && ic == target_cls
                {
                    return true;
                }
            }
            if let Some((sa, sc)) = self.avm2_superclass(cur_abc, cur_cls) {
                cur_abc = sa;
                cur_cls = sc;
            } else {
                break;
            }
        }
        false
    }

    pub(super) fn resolve_avm2_method_owner(
        &self,
        caller_abc: usize,
        caller_method: usize,
        obj_id: usize,
    ) -> Option<(usize, usize)> {
        let base_cls = self.avm1_objects.get(obj_id).and_then(|o| o.avm2_class)?;
        let mut cur = Some(base_cls);
        for _ in 0..32 {
            let Some((a, c)) = cur else { break };
            if a == caller_abc
                && let Some(inst) = self.abc_tags.get(a).and_then(|l| l.abc.instances.get(c))
            {
                if inst.iinit == caller_method {
                    return Some((a, c));
                }
                for tr in &inst.traits {
                    if matches!(
                        tr.data,
                        AbcTraitData::Method { method }
                            | AbcTraitData::Getter { method }
                            | AbcTraitData::Setter { method }
                            | AbcTraitData::Function { method }
                            if method == caller_method
                    ) {
                        return Some((a, c));
                    }
                }
            }
            cur = self.avm2_superclass(a, c);
        }
        Some(base_cls)
    }

    pub(super) fn avm2_matches_exception(
        &self,
        abc_idx: usize,
        thrown: &Avm1Value,
        exc_type_mn: usize,
    ) -> bool {
        if exc_type_mn == 0 {
            return true;
        }
        let (pkg, tname) = self.abc_tags[abc_idx].abc.split_qname(exc_type_mn);
        if tname == "*" || tname.is_empty() {
            return true;
        }
        self.avm2_is_type_name(abc_idx, thrown, &pkg, &tname)
    }

    pub(super) fn avm2_builtin_super_matches(base_name: &str, target_name: &str) -> bool {
        if base_name == target_name || target_name == "Object" {
            return true;
        }
        match base_name {
            "MovieClip" => matches!(
                target_name,
                "Sprite"
                    | "DisplayObjectContainer"
                    | "InteractiveObject"
                    | "DisplayObject"
                    | "EventDispatcher"
            ),
            "Sprite" => matches!(
                target_name,
                "DisplayObjectContainer"
                    | "InteractiveObject"
                    | "DisplayObject"
                    | "EventDispatcher"
            ),
            "SimpleButton" => matches!(
                target_name,
                "InteractiveObject" | "DisplayObject" | "EventDispatcher"
            ),
            "Shape" => matches!(target_name, "DisplayObject" | "EventDispatcher"),
            "TypeError" | "RangeError" | "ReferenceError" | "ArgumentError" | "VerifyError"
            | "SyntaxError" | "URIError" | "EvalError" | "SecurityError" | "Error" => {
                target_name == "Error"
            }
            _ => false,
        }
    }

    pub(super) fn avm2_class_extends_builtin(
        &self,
        mut cur_abc: usize,
        mut cur_cls: usize,
        target_name: &str,
    ) -> bool {
        for _ in 0..32 {
            let Some(inst) = self
                .abc_tags
                .get(cur_abc)
                .and_then(|l| l.abc.instances.get(cur_cls))
            else {
                break;
            };
            let (_, own_name) = self.abc_tags[cur_abc].abc.split_qname(inst.name);
            if own_name == target_name {
                return true;
            }
            if inst.super_name == 0 {
                break;
            }
            let (spkg, sname) = self.abc_tags[cur_abc].abc.split_qname(inst.super_name);
            if Self::avm2_builtin_super_matches(&sname, target_name) {
                return true;
            }
            if let Some((sa, sc)) = self.find_avm2_class(cur_abc, &spkg, &sname) {
                cur_abc = sa;
                cur_cls = sc;
            } else {
                break;
            }
        }
        false
    }

    pub(super) fn avm2_is_type_name(
        &self,
        abc_idx: usize,
        val: &Avm1Value,
        pkg: &str,
        tname: &str,
    ) -> bool {
        if tname == "*" {
            return true;
        }
        if matches!(val, Avm1Value::Undefined | Avm1Value::Null) {
            return false;
        }
        match tname {
            "Object" => true,
            "Boolean" => matches!(val, Avm1Value::Bool(_)),
            "Number" => matches!(val, Avm1Value::Number(_)),
            "int" => matches!(
                val,
                Avm1Value::Number(n)
                    if n.is_finite()
                        && n.fract() == 0.0
                        && *n >= f64::from(i32::MIN)
                        && *n <= f64::from(i32::MAX)
            ),
            "uint" => matches!(
                val,
                Avm1Value::Number(n)
                    if n.is_finite()
                        && n.fract() == 0.0
                        && *n >= 0.0
                        && *n <= f64::from(u32::MAX)
            ),
            "String" => matches!(val, Avm1Value::String(s) if !is_avm2_fn_token(s)),
            "Function" => match val {
                Avm1Value::String(s) => is_avm2_fn_token(s),
                Avm1Value::Object(id) => self
                    .avm1_objects
                    .get(*id)
                    .is_some_and(|o| o.function.is_some()),
                _ => false,
            },
            "Array" => matches!(
                val,
                Avm1Value::Object(id)
                    if self
                        .avm1_objects
                        .get(*id)
                        .is_some_and(|o| o.array_elements.is_some())
            ),
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "ArgumentError"
            | "VerifyError" | "SyntaxError" | "URIError" | "EvalError" | "SecurityError" => {
                if let Avm1Value::Object(id) = val
                    && let Some(o) = self.avm1_objects.get(*id)
                {
                    if let Some(err_cls) = &o.avm2_builtin_error
                        && Self::avm2_builtin_super_matches(err_cls, tname)
                    {
                        return true;
                    }
                    if let Some((c_abc, c_idx)) = o.avm2_class {
                        if let Some((t_abc, t_idx)) = self.find_avm2_class(abc_idx, pkg, tname)
                            && self.avm2_class_matches_or_extends(c_abc, c_idx, t_abc, t_idx)
                        {
                            return true;
                        }
                        if self.avm2_class_extends_builtin(c_abc, c_idx, tname) {
                            return true;
                        }
                    }
                }
                false
            }
            "MovieClip"
            | "Sprite"
            | "Shape"
            | "SimpleButton"
            | "DisplayObject"
            | "DisplayObjectContainer"
            | "InteractiveObject"
            | "EventDispatcher" => match val {
                Avm1Value::MovieClipIdentity(_) => tname != "Shape" && tname != "SimpleButton",
                Avm1Value::MovieClip(mc) => {
                    (tname != "Shape" && tname != "SimpleButton")
                        && (mc == "_root"
                            || (!mc.starts_with("__avm2_")
                                && mc != "Math"
                                && self.target_slot(&self.canonical_movie_target(mc)).is_some()))
                }
                Avm1Value::Object(id) => {
                    if let Some(o) = self.avm1_objects.get(*id) {
                        if let Some(Avm1Value::String(dt)) = o.props.get("__avm2_display_type__")
                            && Self::avm2_builtin_super_matches(dt, tname)
                        {
                            return true;
                        }
                        if let Some((c_abc, c_idx)) = o.avm2_class {
                            if let Some((t_abc, t_idx)) = self.find_avm2_class(abc_idx, pkg, tname)
                                && self.avm2_class_matches_or_extends(c_abc, c_idx, t_abc, t_idx)
                            {
                                return true;
                            }
                            return self.avm2_class_extends_builtin(c_abc, c_idx, tname);
                        }
                    }
                    false
                }
                _ => false,
            },
            _ => {
                if let Avm1Value::Object(id) = val
                    && let Some((c_abc, c_idx)) =
                        self.avm1_objects.get(*id).and_then(|o| o.avm2_class)
                    && let Some((t_abc, t_idx)) = self.find_avm2_class(abc_idx, pkg, tname)
                {
                    return self.avm2_class_matches_or_extends(c_abc, c_idx, t_abc, t_idx);
                }
                false
            }
        }
    }

    pub(super) fn avm2_is_type_value(
        &self,
        abc_idx: usize,
        val: &Avm1Value,
        type_val: &Avm1Value,
    ) -> bool {
        match type_val {
            Avm1Value::MovieClip(s) | Avm1Value::String(s) => {
                if let Some((t_abc, t_cls)) = parse_avm2_class_token(s) {
                    if let Avm1Value::Object(id) = val
                        && let Some((c_abc, c_cls)) =
                            self.avm1_objects.get(*id).and_then(|o| o.avm2_class)
                    {
                        return self.avm2_class_matches_or_extends(c_abc, c_cls, t_abc, t_cls);
                    }
                    return false;
                }
                let tname = s.strip_prefix("__avm2_type:").unwrap_or(s.as_str());
                self.avm2_is_type_name(abc_idx, val, "", tname)
            }
            _ => false,
        }
    }

    pub(super) fn avm2_instanceof_value(
        &self,
        abc_idx: usize,
        val: &Avm1Value,
        type_val: &Avm1Value,
    ) -> bool {
        if matches!(
            val,
            Avm1Value::Undefined | Avm1Value::Null | Avm1Value::Bool(_) | Avm1Value::Number(_)
        ) {
            return false;
        }
        if let Avm1Value::String(s) = val
            && !is_avm2_fn_token(s)
        {
            return false;
        }
        self.avm2_is_type_value(abc_idx, val, type_val)
    }

    pub(super) fn avm2_coerce_multiname(
        &mut self,
        abc_idx: usize,
        mn_idx: usize,
        val: Avm1Value,
    ) -> Avm1Value {
        let (pkg, tname) = self.abc_tags[abc_idx].abc.split_qname(mn_idx);
        match tname.as_str() {
            "*" => val,
            "String" => match val {
                Avm1Value::Undefined | Avm1Value::Null => Avm1Value::Null,
                other => Avm1Value::String(avm2_convert_string(&other, &self.avm1_objects)),
            },
            "int" => Avm1Value::Number(f64::from(ecma_f64_to_i32(avm2_to_number(&val)))),
            "uint" => Avm1Value::Number(f64::from(ecma_f64_to_u32(avm2_to_number(&val)))),
            "Number" => Avm1Value::Number(avm2_to_number(&val)),
            "Boolean" => Avm1Value::Bool(val.to_bool(9)),
            "Object" => {
                if matches!(val, Avm1Value::Undefined) {
                    Avm1Value::Null
                } else {
                    val
                }
            }
            _ => {
                if matches!(val, Avm1Value::Undefined | Avm1Value::Null) {
                    return Avm1Value::Null;
                }
                if self.avm2_is_type_name(abc_idx, &val, &pkg, &tname) {
                    return val;
                }
                if self.find_avm2_class(abc_idx, &pkg, &tname).is_some()
                    || matches!(tname.as_str(), "Array" | "Function")
                {
                    self.record_unsupported_diagnostic(format!(
                        "AVM2 type coercion mismatch to {tname}"
                    ));
                    Avm1Value::Null
                } else {
                    val
                }
            }
        }
    }

    pub(super) fn avm2_typeof(&self, val: &Avm1Value) -> &'static str {
        match val {
            Avm1Value::Undefined => "undefined",
            Avm1Value::Null => "object",
            Avm1Value::Bool(_) => "boolean",
            Avm1Value::Number(_) => "number",
            Avm1Value::String(s) => {
                if is_avm2_fn_token(s) {
                    "function"
                } else {
                    "string"
                }
            }
            Avm1Value::Object(id) => {
                if self
                    .avm1_objects
                    .get(*id)
                    .is_some_and(|o| o.function.is_some())
                {
                    "function"
                } else {
                    "object"
                }
            }
            Avm1Value::MovieClip(_) | Avm1Value::MovieClipIdentity(_) | Avm1Value::Super { .. } => {
                "object"
            }
        }
    }

    pub(super) fn avm2_equals(&self, a: &Avm1Value, b: &Avm1Value) -> bool {
        match (a, b) {
            (Avm1Value::Undefined | Avm1Value::Null, Avm1Value::Undefined | Avm1Value::Null) => {
                true
            }
            (Avm1Value::Undefined | Avm1Value::Null, _)
            | (_, Avm1Value::Undefined | Avm1Value::Null) => false,
            (Avm1Value::Number(x), Avm1Value::Number(y)) => x == y,
            (Avm1Value::Bool(x), Avm1Value::Bool(y)) => x == y,
            (Avm1Value::String(x), Avm1Value::String(y)) => x == y,
            (Avm1Value::Bool(x), other) => {
                self.avm2_equals(&Avm1Value::Number(if *x { 1.0 } else { 0.0 }), other)
            }
            (other, Avm1Value::Bool(y)) => {
                self.avm2_equals(other, &Avm1Value::Number(if *y { 1.0 } else { 0.0 }))
            }
            (Avm1Value::Number(x), Avm1Value::String(y))
            | (Avm1Value::String(y), Avm1Value::Number(x)) => {
                if is_avm2_fn_token(y) {
                    false
                } else {
                    *x == avm2_string_to_number(y)
                }
            }
            (Avm1Value::Object(x), Avm1Value::Object(y)) => x == y,
            (
                Avm1Value::MovieClip(x) | Avm1Value::MovieClipIdentity(x),
                Avm1Value::MovieClip(y) | Avm1Value::MovieClipIdentity(y),
            ) => self.avm2_mc_eq(x, y),
            _ => false,
        }
    }

    pub(super) fn avm2_strict_equals(&self, a: &Avm1Value, b: &Avm1Value) -> bool {
        match (a, b) {
            (Avm1Value::Undefined, Avm1Value::Undefined) => true,
            (Avm1Value::Null, Avm1Value::Null) => true,
            (Avm1Value::Bool(x), Avm1Value::Bool(y)) => x == y,
            (Avm1Value::Number(x), Avm1Value::Number(y)) => x == y,
            (Avm1Value::String(x), Avm1Value::String(y)) => x == y,
            (Avm1Value::Object(x), Avm1Value::Object(y)) => x == y,
            (
                Avm1Value::MovieClip(x) | Avm1Value::MovieClipIdentity(x),
                Avm1Value::MovieClip(y) | Avm1Value::MovieClipIdentity(y),
            ) => self.avm2_mc_eq(x, y),
            _ => false,
        }
    }

    pub(super) fn avm2_mc_eq(&self, a: &str, b: &str) -> bool {
        if a == b {
            return true;
        }
        if a.starts_with("__avm2_") || b.starts_with("__avm2_") {
            return false;
        }
        let ca = self.canonical_movie_target(a);
        let cb = self.canonical_movie_target(b);
        !ca.is_empty() && ca == cb && (ca == "_root" || self.target_slot(&ca).is_some())
    }

    pub(super) fn avm2_enumerable_keys(&self, obj: &Avm1Value) -> Vec<String> {
        match obj {
            Avm1Value::Object(id) => {
                let Some(o) = self.avm1_objects.get(*id) else {
                    return Vec::new();
                };
                let mut keys = Vec::new();
                if let Some(elems) = &o.array_elements {
                    for idx in 0..elems.len() {
                        if !o.deleted_indices.contains(&idx) {
                            keys.push(idx.to_string());
                        }
                    }
                }
                let mut prop_keys: Vec<String> = o
                    .props
                    .keys()
                    .filter(|k| {
                        !k.starts_with("__")
                            && (o.avm2_builtin_error.is_none()
                                || !matches!(k.as_str(), "name" | "message" | "errorID"))
                    })
                    .cloned()
                    .collect();
                prop_keys.sort();
                keys.extend(prop_keys);
                keys
            }
            _ => Vec::new(),
        }
    }

    pub(super) fn avm2_class_token_has_static_prop(&self, mc: &str, prop: &str) -> bool {
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

    pub(super) fn avm2_object_has_prop(&self, obj_id: usize, prop: &str) -> bool {
        let Some(obj) = self.avm1_objects.get(obj_id) else {
            return false;
        };
        if let Some((vname, _, _)) = &obj.avm2_catch_var
            && vname == prop
        {
            return true;
        }
        if obj.props.contains_key(prop) {
            return true;
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

    pub(super) fn avm2_has_property(&self, obj: &Avm1Value, prop: &str) -> bool {
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

    pub(super) fn delete_avm2_property(&mut self, obj: &Avm1Value, prop: &str) -> bool {
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

    pub(super) fn construct_avm2_value(
        &mut self,
        abc_idx: usize,
        ctor: &Avm1Value,
        cargs: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::MovieClip(s) | Avm1Value::String(s) = ctor {
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

    pub(super) fn construct_avm2_named(
        &mut self,
        abc_idx: usize,
        pkg: &str,
        cls_name: &str,
        cargs: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        match cls_name {
            "Array" => {
                let elems = if cargs.len() == 1
                    && let Avm1Value::Number(n) = cargs[0]
                    && n.is_finite()
                    && n.fract() == 0.0
                    && (0.0..=4096.0).contains(&n)
                {
                    vec![Avm1Value::Undefined; n as usize]
                } else {
                    cargs
                };
                let obj_id = self.avm1_objects.len();
                self.avm1_objects.push(Avm1Object {
                    array_elements: Some(elems),
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
            "Sprite" | "MovieClip" | "Shape" => {
                if let Some((c_abc, c_idx)) = self.find_avm2_class(abc_idx, pkg, cls_name) {
                    return self.instantiate_avm2_user_class(c_abc, c_idx, cargs, steps, depth);
                }
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
                props.insert("name".to_owned(), Avm1Value::String(String::new()));
                let obj_id = self.avm1_objects.len();
                self.avm1_objects.push(Avm1Object {
                    props,
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

    pub(super) fn instantiate_avm2_user_class(
        &mut self,
        c_abc: usize,
        c_idx: usize,
        cargs: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
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
                                "*" => Avm1Value::Undefined,
                                _ => Avm1Value::Null,
                            }
                        };
                        props.insert(t_prop, init_val);
                    }
                }
            }
        }

        let obj_id = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object {
            props,
            avm2_class: Some((c_abc, c_idx)),
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

    pub(super) fn resolve_avm2_activation_or_slot_key(
        &self,
        obj: &Avm1Value,
        slot_idx: usize,
    ) -> (String, Option<(usize, usize)>) {
        if let Avm1Value::Object(oid) = obj {
            if slot_idx == 1
                && let Some((vname, c_abc, c_ty)) = self
                    .avm1_objects
                    .get(*oid)
                    .and_then(|o| o.avm2_catch_var.as_ref())
                && !vname.is_empty()
            {
                return (vname.clone(), Some((*c_abc, *c_ty)));
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

    pub(super) fn resolve_avm2_slot_name(
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

    pub(super) fn get_avm2_object_graphics_proxy(&mut self, oid: usize) -> Avm1Value {
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

    pub(super) fn get_avm2_property_eval(
        &mut self,
        obj: &Avm1Value,
        prop: &str,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::Object(id) = obj {
            if prop == "graphics" {
                let is_disp = self.avm1_objects.get(*id).is_some_and(|o| {
                    o.graphics.is_some()
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
            if let Some(Avm1Value::MovieClipIdentity(disp_target)) = self
                .avm1_objects
                .get(*id)
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
                        | "numChildren"
                        | "currentFrame"
                        | "totalFrames"
                )
            {
                return self.get_avm2_property(&Avm1Value::MovieClipIdentity(disp_target), prop);
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
                return self.get_avm1_member(obj, &effective_prop);
            }
            return self.get_avm1_member(obj, prop);
        }
        if let Avm1Value::String(s) = obj
            && !is_avm2_fn_token(s)
            && prop == "length"
        {
            return Avm1Value::Number(s.chars().count() as f64);
        }
        self.get_avm2_property(obj, prop)
    }

    pub(super) fn get_avm2_property(&self, obj: &Avm1Value, prop: &str) -> Avm1Value {
        if let Avm1Value::Object(_id) = obj {
            return self.get_avm1_member(obj, prop);
        }
        if let Avm1Value::MovieClip(mc) | Avm1Value::MovieClipIdentity(mc) = obj {
            if let Some((c_abc, c_idx)) = parse_avm2_class_token(mc) {
                if let Some(loaded) = self.abc_tags.get(c_abc)
                    && let Some(cls) = loaded.abc.classes.get(c_idx)
                {
                    for tr in &cls.traits {
                        let (_, tname) = loaded.abc.split_qname(tr.name);
                        if tname == prop {
                            match tr.data {
                                AbcTraitData::Slot { vindex, vkind, .. }
                                    if vindex != 0 || vkind != 0 =>
                                {
                                    return abc_const_to_avm_value(&loaded.abc, vindex, vkind);
                                }
                                AbcTraitData::Method { method }
                                | AbcTraitData::Function { method } => {
                                    return Avm1Value::String(format!(
                                        "__avm2_fn:{c_abc}:{method}"
                                    ));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                if let Some(v) = self.avm2_globals.get(&format!("{mc}.{prop}")) {
                    return v.clone();
                }
                return Avm1Value::Undefined;
            }
            if mc == "Math" {
                return match prop {
                    "PI" => Avm1Value::Number(std::f64::consts::PI),
                    "E" => Avm1Value::Number(std::f64::consts::E),
                    "SQRT2" => Avm1Value::Number(std::f64::consts::SQRT_2),
                    "LN2" => Avm1Value::Number(std::f64::consts::LN_2),
                    "LN10" => Avm1Value::Number(std::f64::consts::LN_10),
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
            "currentFrame" => Avm1Value::Number(f64::from(self.current_frame + 1)),
            "totalFrames" => Avm1Value::Number(f64::from(self.root_timeline.total_frames)),
            "numChildren" => Avm1Value::Number(self.display_list.len() as f64),
            "graphics" => Avm1Value::MovieClip("__avm2_graphics:_root".into()),
            "Math" => Avm1Value::MovieClip("Math".into()),
            _ => {
                if let Some((depth, slot)) = self.display_list.iter().find_map(|(depth, slot)| {
                    slot_matches_segment(*depth, slot, prop).then_some((*depth, slot))
                }) {
                    Avm1Value::MovieClipIdentity(live_slot_segment(depth, slot))
                } else {
                    Avm1Value::MovieClip(prop.to_owned())
                }
            }
        }
    }

    pub(super) fn mirror_avm2_object_prop_to_display(
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

    pub(super) fn set_avm2_property_eval(
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
                            let coerced = self.avm2_coerce_multiname(a, type_name, val);
                            self.mirror_avm2_object_prop_to_display(*id, &effective_prop, &coerced);
                            self.set_avm1_member(obj, &effective_prop, coerced);
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

    pub(super) fn set_avm2_property(&mut self, obj: &Avm1Value, prop: &str, val: Avm1Value) {
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
    pub(super) fn get_avm2_super_property(
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
    pub(super) fn set_avm2_super_property(
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
                            let coerced = self.avm2_coerce_multiname(a, type_name, val);
                            self.mirror_avm2_object_prop_to_display(*oid, prop, &coerced);
                            self.set_avm1_member(receiver, prop, coerced);
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
    pub(super) fn call_avm2_super_method(
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
        }
        Avm1Value::Undefined
    }

    pub(super) fn apply_avm2_graphics_op(
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

    pub(super) fn call_avm2_property(
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
            && (prop == "call" || prop == "apply")
        {
            let this_arg = args.first().cloned().unwrap_or(Avm1Value::Null);
            let call_args = if prop == "call" {
                args.into_iter().skip(1).collect()
            } else if let Some(Avm1Value::Object(arr_id)) = args.get(1) {
                self.avm1_objects
                    .get(*arr_id)
                    .and_then(|o| o.array_elements.clone())
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            return self
                .invoke_avm2_callable_with_this(receiver, this_arg, call_args, steps, depth);
        }

        if let Avm1Value::Object(oid) = receiver {
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
                if let Some(t) = disp_target
                    && let Some(slot) = self.target_slot_mut(&t)
                {
                    let _ = Self::apply_avm2_graphics_op(&mut slot.graphics, prop, &args);
                }
                return Avm1Value::Undefined;
            }
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
                                    .and_then(|def| def.resolve_label(lbl)),
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
                    let s = args
                        .first()
                        .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                    return Avm1Value::String(s);
                }
                "Boolean" => {
                    let b = args.first().is_some_and(|v| v.to_bool(9));
                    return Avm1Value::Bool(b);
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
                    let s = args
                        .first()
                        .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                    let radix = args
                        .get(1)
                        .map_or(0, |v| ecma_f64_to_i32(avm2_to_number(v)));
                    return Avm1Value::Number(parse_avm_int_radix(&s, radix));
                }
                "parseFloat" => {
                    let s = args
                        .first()
                        .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                    return Avm1Value::Number(avm2_string_to_number(&s));
                }
                _ => {}
            }
        }

        match prop {
            "trace" => {
                let line = args
                    .iter()
                    .map(|a| avm2_convert_string(a, &self.avm1_objects))
                    .collect::<Vec<_>>()
                    .join(" ");
                self.record_trace(line);
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
                            self.root_timeline.resolve_label(lbl).or_else(|| {
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
                self.stream_sound_active = false;
                self.stop_all_sounds_pending = true;
                return Avm1Value::Undefined;
            }
            _ => {}
        }

        if let Some(Avm1Value::String(tok)) = self.avm2_globals.get(prop).cloned()
            && is_avm2_fn_token(&tok)
        {
            return self.invoke_avm2_callable_with_this(
                &Avm1Value::String(tok),
                Avm1Value::MovieClip("_root".into()),
                args,
                steps,
                depth,
            );
        }

        // Check if `prop` is a method on the class in `abc_idx`
        if let Some(loaded) = self.abc_tags.get(abc_idx) {
            for inst in &loaded.abc.instances {
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

    pub(super) fn resolve_avm2_symbol_character_id(&self, c_abc: usize, c_idx: usize) -> u16 {
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

    pub(super) fn avm2_add_child_to_target(
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
                            );
                        }
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
                }
                child_arg.clone()
            }
            Avm1Value::MovieClip(_) | Avm1Value::MovieClipIdentity(_) => child_arg.clone(),
            _ => Avm1Value::Undefined,
        }
    }

    pub(super) fn avm2_remove_child_value(&mut self, child_arg: &Avm1Value) -> Avm1Value {
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
        if !resolved.is_empty() && resolved != "_root" {
            let _ = self.remove_display_child_with_tombstone(&resolved, true);
            if let Avm1Value::Object(oid) = child_arg
                && let Some(obj) = self.avm1_objects.get_mut(*oid)
            {
                obj.props.remove("__display_target__");
            }
            return child_arg.clone();
        }
        Avm1Value::Null
    }

    pub(super) fn invoke_avm2_callable_with_this(
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
                    Avm1Value::MovieClip("_root".to_owned())
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
        }
        Avm1Value::Undefined
    }
}

pub(super) fn is_avm2_fn_token(s: &str) -> bool {
    s.starts_with("__avm2_fn:")
        || s.starts_with("__avm2_bound:")
        || s.starts_with("__avm2_closure:")
}

pub(super) fn parse_avm2_fn_token(s: &str) -> Option<(usize, usize)> {
    if let Some((_, a, m)) = parse_avm2_closure_token(s) {
        return Some((a, m));
    }
    if let Some((_, a, m)) = parse_avm2_bound_token(s) {
        return Some((a, m));
    }
    let rest = s.strip_prefix("__avm2_fn:")?;
    let (a, m) = rest.split_once(':')?;
    Some((a.parse().ok()?, m.parse().ok()?))
}

pub(super) fn parse_avm2_closure_token(s: &str) -> Option<(usize, usize, usize)> {
    let rest = s.strip_prefix("__avm2_closure:")?;
    let (cid, rem) = rest.split_once(':')?;
    let (a, m) = rem.split_once(':')?;
    Some((cid.parse().ok()?, a.parse().ok()?, m.parse().ok()?))
}

pub(super) fn parse_avm2_bound_token(s: &str) -> Option<(usize, usize, usize)> {
    let rest = s.strip_prefix("__avm2_bound:")?;
    let (oid, rem) = rest.split_once(':')?;
    let (a, m) = rem.split_once(':')?;
    Some((oid.parse().ok()?, a.parse().ok()?, m.parse().ok()?))
}

pub(super) fn parse_avm2_class_token(s: &str) -> Option<(usize, usize)> {
    let rest = s.strip_prefix("__avm2_class:")?;
    let (a, c) = rest.split_once(':')?;
    Some((a.parse().ok()?, c.parse().ok()?))
}

pub(super) fn abc_const_to_avm_value(abc: &AbcFile, vindex: usize, vkind: u8) -> Avm1Value {
    match vkind {
        0x03 => Avm1Value::Number(f64::from(abc.ints.get(vindex).copied().unwrap_or(0))),
        0x04 => Avm1Value::Number(f64::from(abc.uints.get(vindex).copied().unwrap_or(0))),
        0x06 => Avm1Value::Number(abc.doubles.get(vindex).copied().unwrap_or(f64::NAN)),
        0x01 => Avm1Value::String(abc.string(vindex).to_owned()),
        0x0B => Avm1Value::Bool(true),
        0x0A => Avm1Value::Bool(false),
        0x0C => Avm1Value::Null,
        0x00 => Avm1Value::Undefined,
        _ => Avm1Value::Null,
    }
}

pub(super) fn avm2_string_to_number(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).map_or(f64::NAN, |v| v as f64);
    }
    if let Some(hex) = t.strip_prefix("-0x").or_else(|| t.strip_prefix("-0X")) {
        return u64::from_str_radix(hex, 16).map_or(f64::NAN, |v| -(v as f64));
    }
    if let Some(hex) = t.strip_prefix("+0x").or_else(|| t.strip_prefix("+0X")) {
        return u64::from_str_radix(hex, 16).map_or(f64::NAN, |v| v as f64);
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

pub(super) fn avm2_to_number(val: &Avm1Value) -> f64 {
    match val {
        Avm1Value::Undefined => f64::NAN,
        Avm1Value::Null => 0.0,
        Avm1Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Avm1Value::Number(n) => *n,
        Avm1Value::String(s) => {
            if is_avm2_fn_token(s) {
                f64::NAN
            } else {
                avm2_string_to_number(s)
            }
        }
        Avm1Value::Object(_)
        | Avm1Value::MovieClip(_)
        | Avm1Value::MovieClipIdentity(_)
        | Avm1Value::Super { .. } => f64::NAN,
    }
}

pub(super) fn avm2_convert_string(val: &Avm1Value, objects: &[Avm1Object]) -> String {
    match val {
        Avm1Value::Undefined => "undefined".to_owned(),
        Avm1Value::Null => "null".to_owned(),
        Avm1Value::String(s) if is_avm2_fn_token(s) => "function Function() {}".to_owned(),
        other => other.to_trace_string(9, objects),
    }
}

pub(super) fn avm2_less_than(a: &Avm1Value, b: &Avm1Value) -> Option<bool> {
    if let (Avm1Value::String(sa), Avm1Value::String(sb)) = (a, b)
        && !is_avm2_fn_token(sa)
        && !is_avm2_fn_token(sb)
    {
        return Some(sa < sb);
    }
    let na = avm2_to_number(a);
    let nb = avm2_to_number(b);
    if na.is_nan() || nb.is_nan() {
        None
    } else {
        Some(na < nb)
    }
}
