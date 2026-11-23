//! AVM2 class hierarchy lookup, type checking, coercions, equality, and enumeration.

use super::super::*;

impl SwfBytecodePlayer {
    pub(in crate::swf::player) fn find_avm2_class(
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

    pub(in crate::swf::player) fn avm2_superclass(
        &self,
        abc_idx: usize,
        cls_idx: usize,
    ) -> Option<(usize, usize)> {
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

    pub(in crate::swf::player) fn avm2_class_matches_or_extends(
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
                    && self.avm2_class_matches_or_extends(ia, ic, target_abc, target_cls)
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

    pub(in crate::swf::player) fn resolve_avm2_method_owner(
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

    pub(in crate::swf::player) fn avm2_matches_exception(
        &self,
        abc_idx: usize,
        thrown: &Avm1Value,
        exc_type_mn: usize,
    ) -> bool {
        if exc_type_mn == 0 {
            return true;
        }
        if let Some(vec_elem) =
            self.resolve_avm2_vector_elem_type_from_multiname(abc_idx, exc_type_mn)
        {
            return matches!(
                thrown,
                Avm1Value::Object(oid)
                    if self
                        .avm1_objects
                        .get(*oid)
                        .and_then(|o| o.avm2_vector.as_ref())
                        .is_some_and(|vd| vd.elem_type.matches_target(&vec_elem))
            );
        }
        let (pkg, tname) = self.abc_tags[abc_idx].abc.split_qname(exc_type_mn);
        if tname == "*" || tname.is_empty() {
            return true;
        }
        self.avm2_is_type_name(abc_idx, thrown, &pkg, &tname)
    }

    pub(in crate::swf::player) fn avm2_builtin_super_matches(
        base_name: &str,
        target_name: &str,
    ) -> bool {
        if base_name == target_name || target_name == "Object" {
            return true;
        }
        match base_name {
            "Stage" => matches!(
                target_name,
                "DisplayObjectContainer"
                    | "InteractiveObject"
                    | "DisplayObject"
                    | "EventDispatcher"
            ),
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
            "SimpleButton" | "TextField" => matches!(
                target_name,
                "InteractiveObject" | "DisplayObject" | "EventDispatcher"
            ),
            "Shape" | "Bitmap" | "Video" => {
                matches!(target_name, "DisplayObject" | "EventDispatcher")
            }
            "Event" | "MouseEvent" | "KeyboardEvent" => target_name == "Event",
            "TypeError" | "RangeError" | "ReferenceError" | "ArgumentError" | "VerifyError"
            | "SyntaxError" | "URIError" | "EvalError" | "SecurityError" | "Error" => {
                target_name == "Error"
            }
            _ => false,
        }
    }

    pub(in crate::swf::player) fn avm2_class_extends_builtin(
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

    pub(in crate::swf::player) fn avm2_is_type_name(
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
            "Dictionary" => matches!(
                val,
                Avm1Value::Object(id)
                    if self
                        .avm1_objects
                        .get(*id)
                        .is_some_and(|o| o.avm2_dictionary.is_some())
            ),
            "XML" => matches!(
                val,
                Avm1Value::Object(id)
                    if self
                        .avm1_objects
                        .get(*id)
                        .and_then(|o| o.avm2_xml.as_ref())
                        .is_some_and(|xd| matches!(xd.kind, Avm2XmlKind::Xml(_)))
            ),
            "XMLList" => matches!(
                val,
                Avm1Value::Object(id)
                    if self
                        .avm1_objects
                        .get(*id)
                        .and_then(|o| o.avm2_xml.as_ref())
                        .is_some_and(|xd| matches!(xd.kind, Avm2XmlKind::XmlList { .. }))
            ),
            "QName" => matches!(
                val,
                Avm1Value::Object(id)
                    if self
                        .avm1_objects
                        .get(*id)
                        .and_then(|o| o.props.get("__avm2_short_class__"))
                        .is_some_and(|v| matches!(v, Avm1Value::String(s) if s == "QName"))
            ),
            "Class" => matches!(
                val,
                Avm1Value::MovieClip(mc)
                    if mc.starts_with("__avm2_class:")
                        || mc.starts_with("__avm2_type:")
                        || mc.starts_with("__avm2_vec_type:")
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
            "Stage"
            | "MovieClip"
            | "Sprite"
            | "Shape"
            | "SimpleButton"
            | "TextField"
            | "Bitmap"
            | "Video"
            | "DisplayObject"
            | "DisplayObjectContainer"
            | "InteractiveObject"
            | "EventDispatcher" => match val {
                Avm1Value::MovieClipIdentity(_) => !matches!(
                    tname,
                    "Stage" | "Shape" | "SimpleButton" | "TextField" | "Bitmap"
                ),
                Avm1Value::MovieClip(mc) => {
                    (!matches!(
                        tname,
                        "Stage" | "Shape" | "SimpleButton" | "TextField" | "Bitmap"
                    )) && (mc == "_root"
                        || (!mc.starts_with("__avm2_")
                            && mc != "Math"
                            && self.target_slot(&self.canonical_movie_target(mc)).is_some()))
                }
                Avm1Value::Object(id) => {
                    if let Some(o) = self.avm1_objects.get(*id) {
                        if let Some(meta) = &o.avm2_display
                            && Self::avm2_builtin_super_matches(&meta.display_type, tname)
                        {
                            return true;
                        }
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
            "Event" | "MouseEvent" | "KeyboardEvent" => {
                if let Avm1Value::Object(id) = val
                    && let Some(o) = self.avm1_objects.get(*id)
                {
                    if o.avm2_event.is_some() && tname == "Event" {
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
            _ => {
                if let Some(inner) = tname
                    .strip_prefix("Vector.<")
                    .and_then(|r| r.strip_suffix('>'))
                {
                    let req = self.resolve_avm2_vector_elem_type_from_name(abc_idx, pkg, inner);
                    return matches!(
                        val,
                        Avm1Value::Object(oid)
                            if self
                                .avm1_objects
                                .get(*oid)
                                .and_then(|o| o.avm2_vector.as_ref())
                                .is_some_and(|vd| vd.elem_type.matches_target(&req))
                    );
                }
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

    pub(in crate::swf::player) fn avm2_is_type_value(
        &self,
        abc_idx: usize,
        val: &Avm1Value,
        type_val: &Avm1Value,
    ) -> bool {
        match type_val {
            Avm1Value::MovieClip(s) | Avm1Value::String(s) => {
                if let Some(vec_tok) = s.strip_prefix("__avm2_vec_type:") {
                    let req = Avm2VectorElemType::decode_token(vec_tok);
                    return matches!(
                        val,
                        Avm1Value::Object(oid)
                            if self
                                .avm1_objects
                                .get(*oid)
                                .and_then(|o| o.avm2_vector.as_ref())
                                .is_some_and(|vd| vd.elem_type.matches_target(&req))
                    );
                }
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

    pub(in crate::swf::player) fn avm2_class_extends_non_interface(
        &self,
        mut cur_abc: usize,
        mut cur_cls: usize,
        target_abc: usize,
        target_cls: usize,
    ) -> bool {
        let is_target_iface = self
            .abc_tags
            .get(target_abc)
            .and_then(|l| l.abc.instances.get(target_cls))
            .is_some_and(|inst| (inst.flags & 0x04) != 0);
        if is_target_iface {
            return false;
        }
        for _ in 0..32 {
            if cur_abc == target_abc && cur_cls == target_cls {
                return true;
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

    pub(in crate::swf::player) fn avm2_instanceof_value(
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
        if let Avm1Value::MovieClip(s) | Avm1Value::String(s) = type_val {
            if let Some((t_abc, t_cls)) = parse_avm2_class_token(s) {
                if let Avm1Value::Object(id) = val
                    && let Some((c_abc, c_cls)) =
                        self.avm1_objects.get(*id).and_then(|o| o.avm2_class)
                {
                    return self.avm2_class_extends_non_interface(c_abc, c_cls, t_abc, t_cls);
                }
                return false;
            }
            let tname = s.strip_prefix("__avm2_type:").unwrap_or(s.as_str());
            if let Some((t_abc, t_cls)) = self.find_avm2_class(abc_idx, "", tname)
                && self
                    .abc_tags
                    .get(t_abc)
                    .and_then(|l| l.abc.instances.get(t_cls))
                    .is_some_and(|inst| (inst.flags & 0x04) != 0)
            {
                return false;
            }
        }
        self.avm2_is_type_value(abc_idx, val, type_val)
    }

    pub(in crate::swf::player) fn avm2_coerce_to_string_eval(
        &mut self,
        value: &Avm1Value,
        abc: usize,
        steps: &mut usize,
        depth: usize,
    ) -> String {
        if let Avm1Value::MovieClip(tok) = value {
            if let Some(rest) = tok.strip_prefix("__avm2_class:") {
                let mut parts = rest.split(':');
                if let (Some(Ok(ca)), Some(Ok(cc))) = (
                    parts.next().map(str::parse::<usize>),
                    parts.next().map(str::parse::<usize>),
                ) && let Some(inst) = self.abc_tags.get(ca).and_then(|l| l.abc.instances.get(cc))
                {
                    let (_, cname) = self.abc_tags[ca].abc.split_qname(inst.name);
                    return format!("[class {cname}]");
                }
            } else if let Some(tname) = tok.strip_prefix("__avm2_type:") {
                return format!("[class {tname}]");
            }
        }
        if let Avm1Value::Object(oid) = value
            && let Some(xd) = self
                .avm1_objects
                .get(*oid)
                .and_then(|o| o.avm2_xml.as_ref())
        {
            return xd.format_string();
        }
        if matches!(value, Avm1Value::Object(_)) {
            for name in ["toString", "valueOf"] {
                if !self.avm2_has_property(value, name) {
                    continue;
                }
                let method = self.get_avm2_property_eval(value, name, steps, depth);
                if self.avm2_thrown.is_some() {
                    return String::new();
                }
                if matches!(&method, Avm1Value::String(s) if is_avm2_fn_token(s)) {
                    let v = self.invoke_avm2_callable_with_this(
                        &method,
                        value.clone(),
                        Vec::new(),
                        steps,
                        depth,
                    );
                    if self.avm2_thrown.is_some() {
                        return String::new();
                    }
                    if matches!(
                        &v,
                        Avm1Value::Number(_)
                            | Avm1Value::Null
                            | Avm1Value::Undefined
                            | Avm1Value::Bool(_)
                    ) || matches!(&v, Avm1Value::String(s) if !is_avm2_fn_token(s))
                    {
                        return avm2_convert_string(&v, &self.avm1_objects);
                    }
                }
            }
            if self.avm2_has_property(value, "toString") || self.avm2_has_property(value, "valueOf")
            {
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
                return String::new();
            }
        }
        avm2_convert_string(value, &self.avm1_objects)
    }

    pub(in crate::swf::player) fn avm2_coerce_multiname(
        &mut self,
        abc_idx: usize,
        mn_idx: usize,
        val: Avm1Value,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if mn_idx == 0 {
            return val;
        }
        if let Some(vec_elem) = self.resolve_avm2_vector_elem_type_from_multiname(abc_idx, mn_idx) {
            if matches!(val, Avm1Value::Undefined | Avm1Value::Null) {
                return Avm1Value::Null;
            }
            if let Avm1Value::Object(oid) = &val
                && self
                    .avm1_objects
                    .get(*oid)
                    .and_then(|o| o.avm2_vector.as_ref())
                    .is_some_and(|vd| vd.elem_type.matches_target(&vec_elem))
            {
                return val;
            }
            let target_q = format!("__AS3__.vec::Vector.<{}>", vec_elem.full_as3_type_name());
            let val_desc = avm2_convert_string(&val, &self.avm1_objects);
            self.throw_avm2_builtin_error(
                "TypeError",
                1034,
                &format!("Type Coercion failed: cannot convert {val_desc} to {target_q}."),
            );
            return Avm1Value::Null;
        }
        let (pkg, tname) = self.abc_tags[abc_idx].abc.split_qname(mn_idx);
        match tname.as_str() {
            "" | "*" => val,
            "void" => Avm1Value::Undefined,
            "String" => match val {
                Avm1Value::Undefined | Avm1Value::Null => Avm1Value::Null,
                other => {
                    let s = self.avm2_coerce_to_string_eval(&other, abc_idx, steps, depth);
                    Avm1Value::String(s)
                }
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
                    || matches!(
                        tname.as_str(),
                        "Array"
                            | "Function"
                            | "Dictionary"
                            | "XML"
                            | "XMLList"
                            | "QName"
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
                    )
                {
                    let val_desc = match &val {
                        Avm1Value::String(s) if !is_avm2_fn_token(s) => format!("\"{s}\""),
                        Avm1Value::Object(oid) => {
                            if let Some((ca, cc)) =
                                self.avm1_objects.get(*oid).and_then(|o| o.avm2_class)
                            {
                                let (cpkg, cname) = self.abc_tags[ca]
                                    .abc
                                    .split_qname(self.abc_tags[ca].abc.instances[cc].name);
                                let q = if cpkg.is_empty() {
                                    cname
                                } else {
                                    format!("{cpkg}.{cname}")
                                };
                                format!("{q}@00000000000")
                            } else {
                                "Object@00000000000".to_owned()
                            }
                        }
                        other => avm2_convert_string(other, &self.avm1_objects),
                    };
                    let target_q = if pkg.is_empty() {
                        tname.clone()
                    } else {
                        format!("{pkg}.{tname}")
                    };
                    let err = self.construct_avm2_named(
                        abc_idx,
                        "",
                        "TypeError",
                        vec![Avm1Value::String(format!(
                            "Error #1034: Type Coercion failed: cannot convert {val_desc} to {target_q}."
                        ))],
                        steps,
                        depth,
                    );
                    self.avm2_thrown = Some(err);
                    Avm1Value::Null
                } else {
                    val
                }
            }
        }
    }

    pub(in crate::swf::player) fn avm2_typeof(&self, val: &Avm1Value) -> &'static str {
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
                if let Some(o) = self.avm1_objects.get(*id) {
                    if o.avm2_xml.is_some() {
                        return "xml";
                    }
                    if o.function.is_some() {
                        return "function";
                    }
                }
                "object"
            }
            Avm1Value::MovieClip(_) | Avm1Value::MovieClipIdentity(_) | Avm1Value::Super { .. } => {
                "object"
            }
        }
    }

    pub(in crate::swf::player) fn avm2_equals(&self, a: &Avm1Value, b: &Avm1Value) -> bool {
        // ECMA-357 §11.5.1 abstract equality for XML / XMLList
        if let (Avm1Value::Object(oa), Avm1Value::Object(ob)) = (a, b) {
            if oa == ob {
                return true;
            }
            if let (Some((ia, sa)), Some((ib, sb))) =
                (self.xml_items_of_oid(*oa), self.xml_items_of_oid(*ob))
            {
                if sa.is_none() && ia.is_empty() && sb.is_none() && ib.is_empty() {
                    return true;
                }
                if ia.len() == 1 && ib.len() == 1 {
                    return self.e4x_arena.borrow().nodes_equal(ia[0], ib[0]);
                }
                return false;
            }
        }
        if let Avm1Value::Object(ox) = a
            && let Some(xd) = self.avm1_objects.get(*ox).and_then(|o| o.avm2_xml.as_ref())
        {
            let has_simple = {
                let arena = xd.arena.borrow();
                match &xd.kind {
                    Avm2XmlKind::Xml(nid) => arena.node_has_simple_content(*nid),
                    Avm2XmlKind::XmlList { items, .. } => {
                        items.is_empty()
                            || (items.len() == 1 && arena.node_has_simple_content(items[0]))
                    }
                }
            };
            if has_simple && !matches!(b, Avm1Value::Undefined | Avm1Value::Null) {
                return self.avm2_equals(&Avm1Value::String(xd.format_string()), b);
            }
        }
        if let Avm1Value::Object(ox) = b
            && let Some(xd) = self.avm1_objects.get(*ox).and_then(|o| o.avm2_xml.as_ref())
        {
            let has_simple = {
                let arena = xd.arena.borrow();
                match &xd.kind {
                    Avm2XmlKind::Xml(nid) => arena.node_has_simple_content(*nid),
                    Avm2XmlKind::XmlList { items, .. } => {
                        items.is_empty()
                            || (items.len() == 1 && arena.node_has_simple_content(items[0]))
                    }
                }
            };
            if has_simple && !matches!(a, Avm1Value::Undefined | Avm1Value::Null) {
                return self.avm2_equals(a, &Avm1Value::String(xd.format_string()));
            }
        }
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
            (Avm1Value::Object(x), Avm1Value::Object(y)) => {
                if x == y {
                    return true;
                }
                if let (Some(ox), Some(oy)) = (self.avm1_objects.get(*x), self.avm1_objects.get(*y))
                    && let (Some(xx), Some(xy)) = (&ox.avm2_xml, &oy.avm2_xml)
                    && let (Avm2XmlKind::Xml(nx), Avm2XmlKind::Xml(ny)) = (&xx.kind, &xy.kind)
                {
                    return nx == ny && std::rc::Rc::ptr_eq(&xx.arena, &xy.arena);
                }
                false
            }
            (
                Avm1Value::MovieClip(x) | Avm1Value::MovieClipIdentity(x),
                Avm1Value::MovieClip(y) | Avm1Value::MovieClipIdentity(y),
            ) => self.avm2_mc_eq(x, y),
            _ => false,
        }
    }

    pub(in crate::swf::player) fn avm2_strict_equals(&self, a: &Avm1Value, b: &Avm1Value) -> bool {
        match (a, b) {
            (Avm1Value::Undefined, Avm1Value::Undefined) => true,
            (Avm1Value::Null, Avm1Value::Null) => true,
            (Avm1Value::Bool(x), Avm1Value::Bool(y)) => x == y,
            (Avm1Value::Number(x), Avm1Value::Number(y)) => x == y,
            (Avm1Value::String(x), Avm1Value::String(y)) => x == y,
            (Avm1Value::Object(x), Avm1Value::Object(y)) => {
                if x == y {
                    return true;
                }
                if let (Some(ox), Some(oy)) = (self.avm1_objects.get(*x), self.avm1_objects.get(*y))
                    && let (Some(xx), Some(xy)) = (&ox.avm2_xml, &oy.avm2_xml)
                    && let (Avm2XmlKind::Xml(nx), Avm2XmlKind::Xml(ny)) = (&xx.kind, &xy.kind)
                {
                    return nx == ny && std::rc::Rc::ptr_eq(&xx.arena, &xy.arena);
                }
                false
            }
            (
                Avm1Value::MovieClip(x) | Avm1Value::MovieClipIdentity(x),
                Avm1Value::MovieClip(y) | Avm1Value::MovieClipIdentity(y),
            ) => self.avm2_mc_eq(x, y),
            _ => false,
        }
    }

    pub(in crate::swf::player) fn avm2_mc_eq(&self, a: &str, b: &str) -> bool {
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

    pub(in crate::swf::player) fn avm2_enumerable_len(&self, obj: &Avm1Value) -> usize {
        if let Avm1Value::Object(id) = obj
            && let Some(o) = self.avm1_objects.get(*id)
        {
            if let Some(dict) = &o.avm2_dictionary {
                return dict.entries.len();
            }
            if let Some(vd) = &o.avm2_vector {
                return vd.elements.len();
            }
            if let Some(xd) = &o.avm2_xml {
                return match &xd.kind {
                    Avm2XmlKind::Xml(_) => 1,
                    Avm2XmlKind::XmlList { items, .. } => items.len(),
                };
            }
        }
        self.avm2_enumerable_keys(obj).len()
    }

    pub(in crate::swf::player) fn avm2_nextname_at(
        &self,
        obj: &Avm1Value,
        k_idx: usize,
    ) -> Avm1Value {
        if let Avm1Value::Object(id) = obj
            && let Some(o) = self.avm1_objects.get(*id)
        {
            if let Some(dict) = &o.avm2_dictionary {
                return dict
                    .entries
                    .get(k_idx)
                    .map_or(Avm1Value::Undefined, |(_, raw_k, _)| raw_k.clone());
            }
            if let Some(vd) = &o.avm2_vector {
                return if k_idx < vd.elements.len() {
                    Avm1Value::Number(k_idx as f64)
                } else {
                    Avm1Value::Undefined
                };
            }
            if let Some(xd) = &o.avm2_xml {
                let len = match &xd.kind {
                    Avm2XmlKind::Xml(_) => 1,
                    Avm2XmlKind::XmlList { items, .. } => items.len(),
                };
                return if k_idx < len {
                    Avm1Value::String(k_idx.to_string())
                } else {
                    Avm1Value::Undefined
                };
            }
        }
        let keys = self.avm2_enumerable_keys(obj);
        keys.get(k_idx)
            .cloned()
            .map_or(Avm1Value::Undefined, Avm1Value::String)
    }

    pub(in crate::swf::player) fn avm2_nextvalue_at(
        &mut self,
        obj: &Avm1Value,
        k_idx: usize,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::Object(id) = obj {
            if let Some(val) = self
                .avm1_objects
                .get(*id)
                .and_then(|o| o.avm2_dictionary.as_ref())
                .and_then(|d| d.entries.get(k_idx))
                .map(|(_, _, v)| v.clone())
            {
                return val;
            }
            if let Some(val) = self
                .avm1_objects
                .get(*id)
                .and_then(|o| o.avm2_vector.as_ref())
                .and_then(|vd| vd.elements.get(k_idx))
                .cloned()
            {
                return val;
            }
            if self
                .avm1_objects
                .get(*id)
                .is_some_and(|o| o.avm2_xml.is_some())
            {
                return self
                    .get_avm2_xml_property(*id, &k_idx.to_string())
                    .unwrap_or(Avm1Value::Undefined);
            }
        }
        let keys = self.avm2_enumerable_keys(obj);
        if let Some(k) = keys.get(k_idx) {
            self.get_avm2_property_eval(obj, k, steps, depth)
        } else {
            Avm1Value::Undefined
        }
    }

    pub(in crate::swf::player) fn avm2_runtime_class_label(&self, val: &Avm1Value) -> String {
        match val {
            Avm1Value::Undefined => "void".to_owned(),
            Avm1Value::Null => "null".to_owned(),
            Avm1Value::Bool(_) => "Boolean".to_owned(),
            Avm1Value::Number(_) => "Number".to_owned(),
            Avm1Value::String(s) => {
                if is_avm2_fn_token(s) {
                    "Function".to_owned()
                } else {
                    "String".to_owned()
                }
            }
            Avm1Value::Object(oid) => {
                if let Some(o) = self.avm1_objects.get(*oid) {
                    if let Some(xd) = &o.avm2_xml {
                        return match xd.kind {
                            Avm2XmlKind::Xml(_) => "XML".to_owned(),
                            Avm2XmlKind::XmlList { .. } => "XMLList".to_owned(),
                        };
                    }
                    if o.avm2_dictionary.is_some() {
                        return "flash.utils::Dictionary".to_owned();
                    }
                    if let Some(vd) = &o.avm2_vector {
                        return format!(
                            "__AS3__.vec::Vector.<{}>",
                            vd.elem_type.full_as3_type_name()
                        );
                    }
                    if o.array_elements.is_some() {
                        return "Array".to_owned();
                    }
                    if let Some(err) = &o.avm2_builtin_error {
                        return err.clone();
                    }
                    if o.avm2_event.is_some() {
                        return "flash.events::Event".to_owned();
                    }
                    if let Some((ca, cc)) = o.avm2_class
                        && let Some(loaded) = self.abc_tags.get(ca)
                        && let Some(inst) = loaded.abc.instances.get(cc)
                    {
                        let (pkg, short) = loaded.abc.split_qname(inst.name);
                        return if pkg.is_empty() {
                            short
                        } else {
                            format!("{pkg}::{short}")
                        };
                    }
                    if let Some(meta) = &o.avm2_display {
                        return format!("flash.display::{}", meta.display_type);
                    }
                    if let Some(Avm1Value::String(sc)) = o.props.get("__avm2_short_class__") {
                        return sc.clone();
                    }
                }
                "Object".to_owned()
            }
            Avm1Value::MovieClip(mc) => {
                if let Some(vt) = mc.strip_prefix("__avm2_vec_type:") {
                    return format!(
                        "__AS3__.vec::Vector.<{}>",
                        Avm2VectorElemType::decode_token(vt).full_as3_type_name()
                    );
                }
                if let Some((ca, cc)) = parse_avm2_class_token(mc)
                    && let Some(loaded) = self.abc_tags.get(ca)
                    && let Some(inst) = loaded.abc.instances.get(cc)
                {
                    let (pkg, short) = loaded.abc.split_qname(inst.name);
                    return if pkg.is_empty() {
                        short
                    } else {
                        format!("{pkg}::{short}")
                    };
                }
                if let Some(tn) = mc.strip_prefix("__avm2_type:") {
                    return tn.to_owned();
                }
                "flash.display::MovieClip".to_owned()
            }
            Avm1Value::MovieClipIdentity(_) => "flash.display::MovieClip".to_owned(),
            Avm1Value::Super { .. } => "Object".to_owned(),
        }
    }

    pub(in crate::swf::player) fn avm2_enumerable_keys(&self, obj: &Avm1Value) -> Vec<String> {
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

    pub(in crate::swf::player) fn coerce_avm2_class_cast(
        &mut self,
        c_abc: usize,
        c_idx: usize,
        args: &[Avm1Value],
    ) -> Avm1Value {
        if args.len() != 1 {
            self.throw_avm2_builtin_error(
                "ArgumentError",
                1112,
                &format!(
                    "Argument count mismatch on class coercion. Expected 1, got {}.",
                    args.len()
                ),
            );
            return Avm1Value::Null;
        }
        let v = &args[0];
        if matches!(v, Avm1Value::Undefined | Avm1Value::Null) {
            return Avm1Value::Null;
        }
        if let Avm1Value::Object(oid) = v
            && let Some((va, vc)) = self.avm1_objects.get(*oid).and_then(|o| o.avm2_class)
            && self.avm2_class_matches_or_extends(va, vc, c_abc, c_idx)
        {
            return v.clone();
        }
        let target_q = self
            .abc_tags
            .get(c_abc)
            .and_then(|l| {
                let inst = l.abc.instances.get(c_idx)?;
                let (cpkg, cname) = l.abc.split_qname(inst.name);
                Some(if cpkg.is_empty() {
                    cname
                } else {
                    format!("{cpkg}.{cname}")
                })
            })
            .unwrap_or_else(|| "Object".to_owned());
        let val_desc = avm2_convert_string(v, &self.avm1_objects);
        self.throw_avm2_builtin_error(
            "TypeError",
            1034,
            &format!("Type Coercion failed: cannot convert {val_desc} to {target_q}."),
        );
        Avm1Value::Null
    }

    pub(in crate::swf::player) fn resolve_avm2_definition_by_name(
        &self,
        abc_idx: usize,
        raw: &str,
    ) -> Option<Avm1Value> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let norm = trimmed
            .strip_prefix("__AS3__.vec::")
            .or_else(|| trimmed.strip_prefix("__AS3__.vec."))
            .unwrap_or(trimmed);
        if let Some(inner) = norm
            .strip_prefix("Vector.<")
            .and_then(|r| r.strip_suffix('>'))
        {
            let elem_ty = self.resolve_avm2_vector_elem_type_from_name(abc_idx, "", inner);
            return Some(Avm1Value::MovieClip(format!(
                "__avm2_vec_type:{}",
                elem_ty.encode_token()
            )));
        }
        let (pkg, short) = if let Some((p, s)) = trimmed.rsplit_once("::") {
            (p, s)
        } else if let Some((p, s)) = trimmed.rsplit_once('.') {
            (p, s)
        } else {
            ("", trimmed)
        };
        if let Some((ca, cc)) = self.find_avm2_class(abc_idx, pkg, short) {
            return Some(Avm1Value::MovieClip(format!("__avm2_class:{ca}:{cc}")));
        }
        if matches!(
            short,
            "int"
                | "uint"
                | "Number"
                | "Boolean"
                | "String"
                | "Object"
                | "Array"
                | "Function"
                | "Dictionary"
                | "XML"
                | "XMLList"
                | "QName"
                | "Namespace"
                | "Error"
                | "TypeError"
                | "RangeError"
                | "ReferenceError"
                | "ArgumentError"
                | "MovieClip"
                | "Sprite"
        ) {
            return Some(Avm1Value::MovieClip(format!("__avm2_type:{short}")));
        }
        None
    }
}
