use super::*;

#[derive(Clone, Debug)]
pub(super) enum Avm2MultinameKey {
    Static { name: String, is_attr: bool },
    Runtime { key: Avm1Value, is_attr: bool },
}

impl Avm2MultinameKey {
    pub(super) fn to_prop_string(&self, objects: &[Avm1Object]) -> String {
        match self {
            Self::Static { name, is_attr } => {
                if *is_attr && !name.starts_with('@') {
                    format!("@{name}")
                } else {
                    name.clone()
                }
            }
            Self::Runtime { key, is_attr } => {
                let mut s = if let Avm1Value::Object(qoid) = key
                    && let Some(qo) = objects.get(*qoid)
                    && let Some(Avm1Value::String(ln)) = qo.props.get("localName")
                {
                    ln.clone()
                } else {
                    avm2_convert_string(key, objects)
                };
                if *is_attr && !s.starts_with('@') {
                    s = format!("@{s}");
                }
                s
            }
        }
    }
}

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

    pub(super) fn avm2_builtin_super_matches(base_name: &str, target_name: &str) -> bool {
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

    pub(super) fn avm2_is_type_value(
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

    pub(super) fn avm2_coerce_to_string_eval(
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

    pub(super) fn avm2_coerce_multiname(
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

    pub(super) fn avm2_equals(&self, a: &Avm1Value, b: &Avm1Value) -> bool {
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

    pub(super) fn avm2_strict_equals(&self, a: &Avm1Value, b: &Avm1Value) -> bool {
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

    pub(super) fn avm2_enumerable_len(&self, obj: &Avm1Value) -> usize {
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

    pub(super) fn avm2_nextname_at(&self, obj: &Avm1Value, k_idx: usize) -> Avm1Value {
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

    pub(super) fn avm2_nextvalue_at(
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

    pub(super) fn avm2_runtime_class_label(&self, val: &Avm1Value) -> String {
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

    pub(super) fn avm2_in_operator(&self, raw_key: &Avm1Value, obj: &Avm1Value) -> bool {
        if let Avm1Value::Object(oid) = obj
            && let Some(dict) = self
                .avm1_objects
                .get(*oid)
                .and_then(|o| o.avm2_dictionary.as_ref())
        {
            let (dk, _) = Avm2DictionaryData::key_from_value(raw_key, &self.avm1_objects);
            return dict.contains(&dk);
        }
        let key_str = avm2_convert_string(raw_key, &self.avm1_objects);
        self.avm2_has_property(obj, &key_str)
    }

    pub(super) fn get_avm2_property_by_key(
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

    pub(super) fn set_avm2_property_by_key(
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

    pub(super) fn delete_avm2_property_by_key(
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
                return self
                    .avm1_objects
                    .get_mut(*oid)
                    .and_then(|o| o.avm2_dictionary.as_mut())
                    .is_some_and(|d| d.remove(&dk));
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

    pub(super) fn collect_avm2_instance_slot_defaults(
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

    pub(super) fn avm2_display_type_for_class(
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

    pub(super) fn next_avm2_instance_name(&mut self) -> String {
        let id = self.avm2_next_instance_id.max(1);
        self.avm2_next_instance_id = id.saturating_add(1);
        format!("instance{id}")
    }

    pub(super) fn instantiate_avm2_user_class(
        &mut self,
        c_abc: usize,
        c_idx: usize,
        cargs: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
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

    pub(super) fn resolve_avm2_activation_or_slot_key(
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

    /// AVM2 Add converts objects before choosing numeric addition or concatenation.
    pub(super) fn avm2_add_operand(
        &mut self,
        value: Avm1Value,
        abc: usize,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if !matches!(value, Avm1Value::Object(_)) {
            return match &value {
                Avm1Value::MovieClip(_)
                | Avm1Value::MovieClipIdentity(_)
                | Avm1Value::Super { .. } => {
                    Avm1Value::String(self.avm2_coerce_to_string_eval(&value, abc, steps, depth))
                }
                Avm1Value::String(s) if is_avm2_fn_token(s) => {
                    Avm1Value::String(avm2_convert_string(&value, &self.avm1_objects))
                }
                _ => value,
            };
        }
        for name in ["valueOf", "toString"] {
            if !self.avm2_has_property(&value, name) {
                if name == "toString" {
                    return Avm1Value::String(avm2_convert_string(&value, &self.avm1_objects));
                }
                continue;
            }
            let method = self.get_avm2_property_eval(&value, name, steps, depth);
            if self.avm2_thrown.is_some() {
                return Avm1Value::Undefined;
            }
            if matches!(&method, Avm1Value::String(s) if is_avm2_fn_token(s)) {
                let v = self.invoke_avm2_callable_with_this(
                    &method,
                    value.clone(),
                    Vec::new(),
                    steps,
                    depth,
                );
                if self.avm2_thrown.is_some()
                    || matches!(
                        &v,
                        Avm1Value::Number(_)
                            | Avm1Value::Null
                            | Avm1Value::Undefined
                            | Avm1Value::Bool(_)
                    )
                    || matches!(&v, Avm1Value::String(s) if !is_avm2_fn_token(s))
                {
                    return v;
                }
            }
        }
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
        Avm1Value::Undefined
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

        if let Avm1Value::MovieClip(mc) = receiver {
            if mc == "__avm2_type:XML" || mc == "XML" {
                match prop {
                    "settings" | "defaultSettings" => {
                        let s = if prop == "defaultSettings" {
                            E4xSettings::default()
                        } else {
                            self.e4x_arena.borrow().settings.clone()
                        };
                        let mut props = HashMap::new();
                        props.insert(
                            "ignoreComments".to_owned(),
                            Avm1Value::Bool(s.ignore_comments),
                        );
                        props.insert(
                            "ignoreProcessingInstructions".to_owned(),
                            Avm1Value::Bool(s.ignore_processing_instructions),
                        );
                        props.insert(
                            "ignoreWhitespace".to_owned(),
                            Avm1Value::Bool(s.ignore_whitespace),
                        );
                        props.insert(
                            "prettyPrinting".to_owned(),
                            Avm1Value::Bool(s.pretty_printing),
                        );
                        props.insert(
                            "prettyIndent".to_owned(),
                            Avm1Value::Number(s.pretty_indent as f64),
                        );
                        let oid = self.avm1_objects.len();
                        self.avm1_objects.push(Avm1Object {
                            props,
                            ..Default::default()
                        });
                        return Avm1Value::Object(oid);
                    }
                    "setSettings" => {
                        if let Some(Avm1Value::Object(soid)) = args.first() {
                            if let Some(so) = self.avm1_objects.get(*soid).cloned() {
                                let mut arena = self.e4x_arena.borrow_mut();
                                if let Some(v) = so.props.get("ignoreComments") {
                                    arena.settings.ignore_comments = v.to_bool(9);
                                }
                                if let Some(v) = so.props.get("ignoreProcessingInstructions") {
                                    arena.settings.ignore_processing_instructions = v.to_bool(9);
                                }
                                if let Some(v) = so.props.get("ignoreWhitespace") {
                                    arena.settings.ignore_whitespace = v.to_bool(9);
                                }
                                if let Some(v) = so.props.get("prettyPrinting") {
                                    arena.settings.pretty_printing = v.to_bool(9);
                                }
                                if let Some(v) = so.props.get("prettyIndent") {
                                    arena.settings.pretty_indent =
                                        ecma_f64_to_i32(avm2_to_number(v)).clamp(0, 64) as usize;
                                }
                            }
                        } else {
                            self.e4x_arena.borrow_mut().settings = E4xSettings::default();
                        }
                        return Avm1Value::Undefined;
                    }
                    _ => {}
                }
            }
            if mc == "__avm2_type:ApplicationDomain" {
                match prop {
                    "hasDefinition" => {
                        let q = args.first().map_or_else(String::new, |v| {
                            avm2_convert_string(v, &self.avm1_objects)
                        });
                        return Avm1Value::Bool(
                            self.resolve_avm2_definition_by_name(abc_idx, &q).is_some(),
                        );
                    }
                    "getDefinition" => {
                        let q = args.first().map_or_else(String::new, |v| {
                            avm2_convert_string(v, &self.avm1_objects)
                        });
                        if let Some(def) = self.resolve_avm2_definition_by_name(abc_idx, &q) {
                            return def;
                        }
                        self.throw_avm2_builtin_error(
                            "ReferenceError",
                            1065,
                            &format!("Variable {q} is not defined."),
                        );
                        return Avm1Value::Undefined;
                    }
                    _ => {}
                }
            }
        }

        if let Avm1Value::Object(oid) = receiver {
            if self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_catch_var.is_some())
            {
                self.throw_avm2_builtin_error(
                    "ReferenceError",
                    1069,
                    &format!(
                        "Property {prop} not found on catch scope and there is no default value."
                    ),
                );
                return Avm1Value::Undefined;
            }
            if self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_vector.is_some())
                && let Some(ret) =
                    self.call_avm2_vector_method(*oid, prop, &args, abc_idx, steps, depth)
            {
                return ret;
            }
            if self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_xml.is_some())
                && let Some(ret) =
                    self.call_avm2_xml_method(*oid, prop, &args, abc_idx, steps, depth)
            {
                return ret;
            }
            if self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_dictionary.is_some())
            {
                match prop {
                    "hasOwnProperty" => {
                        let s = if let Some(first) = args.first() {
                            self.avm2_coerce_to_string_eval(first, abc_idx, steps, depth)
                        } else {
                            String::new()
                        };
                        if self.avm2_thrown.is_some() {
                            return Avm1Value::Undefined;
                        }
                        let has = self
                            .avm1_objects
                            .get(*oid)
                            .and_then(|o| o.avm2_dictionary.as_ref())
                            .is_some_and(|d| d.contains(&Avm2DictKey::String(s)));
                        return Avm1Value::Bool(has);
                    }
                    "propertyIsEnumerable" => {
                        let has = if let Some(first) = args.first() {
                            let (dk, _) =
                                Avm2DictionaryData::key_from_value(first, &self.avm1_objects);
                            self.avm1_objects
                                .get(*oid)
                                .and_then(|o| o.avm2_dictionary.as_ref())
                                .is_some_and(|d| d.contains(&dk))
                        } else {
                            false
                        };
                        return Avm1Value::Bool(has);
                    }
                    "setPropertyIsEnumerable" => return Avm1Value::Undefined,
                    "toString" => return Avm1Value::String("[object Dictionary]".to_owned()),
                    _ => {}
                }
            }
            if let Some(Avm1Value::String(sc)) = self
                .avm1_objects
                .get(*oid)
                .and_then(|o| o.props.get("__avm2_short_class__"))
                && (sc == "QName" || sc == "Namespace")
            {
                match prop {
                    "valueOf" => return receiver.clone(),
                    "toString" => {
                        if let Some(s) = self
                            .avm1_objects
                            .get(*oid)
                            .and_then(|o| o.props.get("qname_formatted"))
                            .cloned()
                        {
                            return s;
                        }
                    }
                    _ => {}
                }
            }
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
                if let Some(t) = disp_target {
                    if t == "_root" {
                        let _ = Self::apply_avm2_graphics_op(&mut self.root_graphics, prop, &args);
                    } else if let Some(slot) = self.target_slot_mut(&t) {
                        let _ = Self::apply_avm2_graphics_op(&mut slot.graphics, prop, &args);
                    }
                }
                return Avm1Value::Undefined;
            }
            if prop == "toString"
                && self
                    .avm1_objects
                    .get(*oid)
                    .is_some_and(|o| o.avm2_event.is_some())
            {
                return Avm1Value::String(
                    Avm1Value::Object(*oid).to_trace_string(9, &self.avm1_objects),
                );
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
            if self
                .avm1_objects
                .get(*oid)
                .is_some_and(|o| o.avm2_display.is_some())
                && let Some(ret) =
                    self.call_avm2_display_builtin_method(*oid, prop, &args, steps, depth)
            {
                return ret;
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
                                    .and_then(|def| def.resolve_avm2_label(lbl)),
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

        if let Avm1Value::MovieClip(mc) = receiver {
            if (mc == "__avm2_type:Object.prototype.toString" || mc == "Object.prototype.toString")
                && prop == "call"
            {
                let this_arg = args.first().unwrap_or(&Avm1Value::Undefined);
                let full = self.avm2_runtime_class_label(this_arg);
                let short = full
                    .strip_prefix("__AS3__.vec::")
                    .unwrap_or_else(|| full.rsplit_once("::").map_or(full.as_str(), |(_, s)| s));
                return Avm1Value::String(format!("[object {short}]"));
            }
            if (mc == "__avm2_type:Object.prototype.valueOf" || mc == "Object.prototype.valueOf")
                && prop == "call"
            {
                return args.first().cloned().unwrap_or(Avm1Value::Undefined);
            }
            if mc == "__avm2_type:XML.prototype" && prop == "toString" {
                return Avm1Value::String(String::new());
            }
            if let Some(mname) = mc.strip_prefix("__avm2_type:XML.prototype.")
                && prop == "call"
                && let Some(Avm1Value::Object(oid)) = args.first()
                && let Some(ret) =
                    self.call_avm2_xml_method(*oid, mname, &args[1..], abc_idx, steps, depth)
            {
                return ret;
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
                    let s = match args.first() {
                        None => String::new(),
                        Some(v) => self.avm2_coerce_to_string_eval(v, abc_idx, steps, depth),
                    };
                    return Avm1Value::String(s);
                }
                "Boolean" => {
                    let b = args.first().is_some_and(|v| v.to_bool(9));
                    return Avm1Value::Bool(b);
                }
                "Object" => {
                    return match args.first() {
                        None | Some(Avm1Value::Undefined | Avm1Value::Null) => {
                            let obj_id = self.avm1_objects.len();
                            self.avm1_objects.push(Avm1Object::default());
                            Avm1Value::Object(obj_id)
                        }
                        Some(v) => v.clone(),
                    };
                }
                "MovieClip"
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
                | "Array"
                | "Function" => {
                    if args.len() == 1 {
                        let v = &args[0];
                        return if matches!(v, Avm1Value::Undefined | Avm1Value::Null) {
                            Avm1Value::Null
                        } else if self.avm2_is_type_name(abc_idx, v, "", prop) {
                            v.clone()
                        } else {
                            Avm1Value::Null
                        };
                    }
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
                "XML" => {
                    return self.construct_avm2_xml(&args, true, abc_idx, steps, depth);
                }
                "XMLList" => {
                    return self.construct_avm2_xml_list(&args, true, abc_idx, steps, depth);
                }
                "getQualifiedClassName" => {
                    let target = args.first().unwrap_or(&Avm1Value::Undefined);
                    return Avm1Value::String(self.avm2_runtime_class_label(target));
                }
                "getDefinitionByName" => {
                    let q = args
                        .first()
                        .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                    if let Some(def) = self.resolve_avm2_definition_by_name(abc_idx, &q) {
                        return def;
                    }
                    let short_q = q
                        .rsplit_once("::")
                        .or_else(|| q.rsplit_once('.'))
                        .map_or(q.as_str(), |(_, s)| s);
                    self.throw_avm2_builtin_error(
                        "ReferenceError",
                        1065,
                        &format!("Variable {short_q} is not defined."),
                    );
                    return Avm1Value::Undefined;
                }
                _ => {
                    if prop.starts_with("Vector.<") {
                        let elem_ty =
                            self.resolve_avm2_vector_elem_type_from_name(abc_idx, "", prop);
                        return self.call_avm2_vector_cast(elem_ty, &args, abc_idx, steps, depth);
                    }
                    if args.len() == 1
                        && let Some((c_abc, c_idx)) = self.find_avm2_class(abc_idx, "", prop)
                    {
                        let v = &args[0];
                        return if matches!(v, Avm1Value::Undefined | Avm1Value::Null) {
                            Avm1Value::Null
                        } else if let Avm1Value::Object(oid) = v
                            && let Some((va, vc)) =
                                self.avm1_objects.get(*oid).and_then(|o| o.avm2_class)
                            && self.avm2_class_matches_or_extends(va, vc, c_abc, c_idx)
                        {
                            v.clone()
                        } else {
                            Avm1Value::Null
                        };
                    }
                }
            }
        }

        match prop {
            "trace" => {
                let mut parts = Vec::with_capacity(args.len());
                for a in &args {
                    let s = self.avm2_coerce_to_string_eval(a, abc_idx, steps, depth);
                    if self.avm2_thrown.is_some() {
                        return Avm1Value::Undefined;
                    }
                    parts.push(s);
                }
                self.record_trace(parts.join(" "));
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
                        self.current_frame = f;
                        self.playing = prop == "gotoAndPlay";
                    }
                }
                return Avm1Value::Undefined;
            }
            "stopAll" => {
                self.stop_swf_sounds();
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
            && args.len() == 1
        {
            if let Some((c_abc, c_idx)) = parse_avm2_class_token(mc) {
                let v = &args[0];
                return if matches!(v, Avm1Value::Undefined | Avm1Value::Null) {
                    Avm1Value::Null
                } else if let Avm1Value::Object(oid) = v
                    && let Some((va, vc)) = self.avm1_objects.get(*oid).and_then(|o| o.avm2_class)
                    && self.avm2_class_matches_or_extends(va, vc, c_abc, c_idx)
                {
                    v.clone()
                } else {
                    Avm1Value::Null
                };
            }
            if let Some(tname) = mc.strip_prefix("__avm2_type:") {
                let v = &args[0];
                return match tname {
                    "int" => Avm1Value::Number(f64::from(ecma_f64_to_i32(avm2_to_number(v)))),
                    "uint" => Avm1Value::Number(f64::from(ecma_f64_to_u32(avm2_to_number(v)))),
                    "Number" => Avm1Value::Number(avm2_to_number(v)),
                    "String" => Avm1Value::String(avm2_convert_string(v, &self.avm1_objects)),
                    "Boolean" => Avm1Value::Bool(v.to_bool(9)),
                    "Object" => v.clone(),
                    _ => {
                        if matches!(v, Avm1Value::Undefined | Avm1Value::Null) {
                            Avm1Value::Null
                        } else if self.avm2_is_type_name(0, v, "", tname) {
                            v.clone()
                        } else {
                            Avm1Value::Null
                        }
                    }
                };
            }
        }
        Avm1Value::Undefined
    }

    pub(super) fn has_active_avm2_tick_listeners(&self) -> bool {
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

    pub(super) fn set_avm2_subtree_on_stage(&mut self, root_oid: usize, on_stage: bool) {
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

    pub(super) fn attach_avm2_child_oid_to_parent(&mut self, parent_oid: usize, child_oid: usize) {
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

    pub(super) fn detach_avm2_child_oid(&mut self, child_oid: usize) {
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

    pub(super) fn dispatch_avm2_subtree_stage_event(
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

    fn collect_avm2_constructed_preorder(
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

    pub(super) fn dispatch_avm2_broadcast_event(&mut self, event_type: &str) {
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

    pub(super) fn dispatch_avm2_event_on_oid(
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

    pub(super) fn run_avm2_frame_phase(&mut self, steps: &mut usize, depth: usize) {
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

    pub(super) fn run_avm2_builtin_display_super(
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

    pub(super) fn allocate_avm2_timeline_object(
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

    pub(super) fn construct_avm2_children_for_container(
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

    pub(super) fn construct_avm2_offtree_sprite_children(
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
    pub(super) fn construct_avm2_timeline_child(
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

    pub(super) fn sync_avm2_container_with_display_list(
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

    pub(super) fn avm2_goto_frame(
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

    pub(super) fn advance_avm2_nested_timelines(&mut self) {
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

    pub(super) fn call_avm2_display_builtin_method(
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
    pub(super) fn resolve_avm2_definition_by_name(
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

pub(super) fn is_avm2_fn_token(s: &str) -> bool {
    s.starts_with("__avm2_fn:")
        || s.starts_with("__avm2_bound:")
        || s.starts_with("__avm2_bound_vec:")
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
    parse_ecma_decimal_strict(t)
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
