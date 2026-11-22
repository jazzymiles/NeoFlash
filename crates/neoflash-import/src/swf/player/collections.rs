use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Avm2VectorElemType {
    Int,
    Uint,
    Number,
    Boolean,
    String,
    Any,
    Object,
    Class {
        abc_idx: usize,
        class_idx: Option<usize>,
        pkg: String,
        name: String,
    },
    Vector(Box<Avm2VectorElemType>),
}

impl Avm2VectorElemType {
    pub(super) fn default_element(&self) -> Avm1Value {
        match self {
            Self::Int | Self::Uint | Self::Number => Avm1Value::Number(0.0),
            Self::Boolean => Avm1Value::Bool(false),
            Self::String | Self::Any | Self::Object | Self::Class { .. } | Self::Vector(_) => {
                Avm1Value::Null
            }
        }
    }

    pub(super) fn empty_pop_shift_value(&self) -> Avm1Value {
        match self {
            Self::Int | Self::Uint | Self::Number => Avm1Value::Number(0.0),
            Self::Boolean
            | Self::String
            | Self::Any
            | Self::Object
            | Self::Class { .. }
            | Self::Vector(_) => Avm1Value::Undefined,
        }
    }

    pub(super) fn full_as3_type_name(&self) -> String {
        match self {
            Self::Int => "int".to_owned(),
            Self::Uint => "uint".to_owned(),
            Self::Number => "Number".to_owned(),
            Self::Boolean => "Boolean".to_owned(),
            Self::String => "String".to_owned(),
            Self::Any => "*".to_owned(),
            Self::Object => "Object".to_owned(),
            Self::Class { pkg, name, .. } => {
                if pkg.is_empty() {
                    name.clone()
                } else {
                    format!("{pkg}::{name}")
                }
            }
            Self::Vector(inner) => format!("__AS3__.vec::Vector.<{}>", inner.full_as3_type_name()),
        }
    }

    pub(super) fn class_display_string(&self) -> String {
        format!("[class Vector.<{}>]", self.full_as3_type_name())
    }

    pub(super) fn encode_token(&self) -> String {
        match self {
            Self::Int => "int".to_owned(),
            Self::Uint => "uint".to_owned(),
            Self::Number => "Number".to_owned(),
            Self::Boolean => "Boolean".to_owned(),
            Self::String => "String".to_owned(),
            Self::Any => "*".to_owned(),
            Self::Object => "Object".to_owned(),
            Self::Class {
                abc_idx,
                class_idx,
                pkg,
                name,
            } => {
                let ci = class_idx.map_or_else(|| "none".to_owned(), |c| c.to_string());
                format!("cls|{abc_idx}|{ci}|{pkg}|{name}")
            }
            Self::Vector(inner) => format!("vec<{}", inner.encode_token()),
        }
    }

    pub(super) fn decode_token(raw: &str) -> Self {
        if let Some(rest) = raw.strip_prefix("vec<") {
            return Self::Vector(Box::new(Self::decode_token(rest)));
        }
        if let Some(rest) = raw.strip_prefix("cls|") {
            let parts: Vec<&str> = rest.splitn(4, '|').collect();
            if parts.len() == 4 {
                let abc_idx = parts[0].parse::<usize>().unwrap_or(0);
                let class_idx = parts[1].parse::<usize>().ok();
                return Self::Class {
                    abc_idx,
                    class_idx,
                    pkg: parts[2].to_owned(),
                    name: parts[3].to_owned(),
                };
            }
        }
        match raw {
            "int" => Self::Int,
            "uint" => Self::Uint,
            "Number" => Self::Number,
            "Boolean" => Self::Boolean,
            "String" => Self::String,
            "*" => Self::Any,
            "Object" => Self::Object,
            other => {
                let (pkg, name) = if let Some((p, s)) = other.rsplit_once("::") {
                    (p.to_owned(), s.to_owned())
                } else if let Some((p, s)) = other.rsplit_once('.') {
                    (p.to_owned(), s.to_owned())
                } else {
                    (String::new(), other.to_owned())
                };
                Self::Class {
                    abc_idx: 0,
                    class_idx: None,
                    pkg,
                    name,
                }
            }
        }
    }

    pub(super) fn matches_target(&self, target: &Self) -> bool {
        match (self, target) {
            (Self::Int, Self::Int)
            | (Self::Uint, Self::Uint)
            | (Self::Number, Self::Number)
            | (Self::Boolean, Self::Boolean)
            | (Self::String, Self::String)
            | (Self::Any, Self::Any)
            | (Self::Object, Self::Object) => true,
            // In Tamarin/AS3, `Vector.<*>` is an alias for `Vector.<Object>` or matches reference vectors when target is `*`.
            (_, Self::Any) => !matches!(self, Self::Int | Self::Uint | Self::Number),
            (
                Self::Class {
                    class_idx: ca,
                    abc_idx: aa,
                    pkg: pa,
                    name: na,
                },
                Self::Class {
                    class_idx: cb,
                    abc_idx: ab,
                    pkg: pb,
                    name: nb,
                },
            ) => {
                if let (Some(ia), Some(ib)) = (ca, cb) {
                    aa == ab && ia == ib
                } else {
                    pa == pb && na == nb
                }
            }
            (Self::Vector(ia), Self::Vector(ib)) => ia == ib,
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Avm2VectorData {
    pub(super) elem_type: Avm2VectorElemType,
    pub(super) fixed: bool,
    pub(super) elements: Vec<Avm1Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Avm2DictKey {
    String(String),
    Object(usize),
    Closure(usize),
    ClassToken(String),
    MovieClip(String),
}

#[derive(Clone, Debug, Default)]
pub(super) struct Avm2DictionaryData {
    #[allow(dead_code)]
    pub(super) weak_keys: bool,
    pub(super) entries: Vec<(Avm2DictKey, Avm1Value, Avm1Value)>,
}

impl Avm2DictionaryData {
    pub(super) fn key_from_value(
        val: &Avm1Value,
        objects: &[Avm1Object],
    ) -> (Avm2DictKey, Avm1Value) {
        match val {
            Avm1Value::Object(oid) => (Avm2DictKey::Object(*oid), Avm1Value::Object(*oid)),
            Avm1Value::String(s) if s.starts_with("__avm2_closure:") => {
                let cid = s
                    .strip_prefix("__avm2_closure:")
                    .and_then(|r| r.split(':').next())
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(0);
                (Avm2DictKey::Closure(cid), val.clone())
            }
            Avm1Value::MovieClip(mc)
                if mc.starts_with("__avm2_class:")
                    || mc.starts_with("__avm2_type:")
                    || mc.starts_with("__avm2_vec_type:") =>
            {
                (Avm2DictKey::ClassToken(mc.clone()), val.clone())
            }
            Avm1Value::MovieClip(mc) | Avm1Value::MovieClipIdentity(mc) => {
                (Avm2DictKey::MovieClip(mc.clone()), val.clone())
            }
            other => {
                let k = avm2_convert_string(other, objects);
                (Avm2DictKey::String(k.clone()), Avm1Value::String(k))
            }
        }
    }

    pub(super) fn get(&self, key: &Avm2DictKey) -> Option<&Avm1Value> {
        self.entries
            .iter()
            .find(|(k, _, _)| k == key)
            .map(|(_, _, v)| v)
    }

    pub(super) fn contains(&self, key: &Avm2DictKey) -> bool {
        self.entries.iter().any(|(k, _, _)| k == key)
    }

    pub(super) fn set(&mut self, key: Avm2DictKey, raw_key: Avm1Value, val: Avm1Value) {
        if let Some(existing) = self.entries.iter_mut().find(|(k, _, _)| *k == key) {
            existing.2 = val;
        } else if self.entries.len() < 65_536 {
            self.entries.push((key, raw_key, val));
        }
    }

    pub(super) fn remove(&mut self, key: &Avm2DictKey) -> bool {
        if let Some(pos) = self.entries.iter().position(|(k, _, _)| k == key) {
            self.entries.remove(pos);
            true
        } else {
            false
        }
    }
}

impl SwfBytecodePlayer {
    pub(super) fn throw_avm2_builtin_error(
        &mut self,
        err_class: &str,
        error_id: i32,
        detail: &str,
    ) {
        let msg = format!("Error #{error_id}: {detail}");
        let mut props = HashMap::new();
        props.insert("name".to_owned(), Avm1Value::String(err_class.to_owned()));
        props.insert("message".to_owned(), Avm1Value::String(msg));
        props.insert("errorID".to_owned(), Avm1Value::Number(f64::from(error_id)));
        let oid = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object {
            props,
            avm2_builtin_error: Some(err_class.to_owned()),
            ..Default::default()
        });
        self.avm2_thrown = Some(Avm1Value::Object(oid));
    }

    pub(super) fn resolve_avm2_vector_elem_type_from_value(
        &self,
        abc_idx: usize,
        tval: &Avm1Value,
    ) -> Avm2VectorElemType {
        match tval {
            Avm1Value::Null => Avm2VectorElemType::Any,
            Avm1Value::MovieClip(s) | Avm1Value::String(s) => {
                if let Some(vec_tok) = s.strip_prefix("__avm2_vec_type:") {
                    return Avm2VectorElemType::Vector(Box::new(Avm2VectorElemType::decode_token(
                        vec_tok,
                    )));
                }
                if let Some((ca, cc)) = parse_avm2_class_token(s) {
                    let (pkg, name) = self
                        .abc_tags
                        .get(ca)
                        .and_then(|l| {
                            l.abc
                                .instances
                                .get(cc)
                                .map(|inst| l.abc.split_qname(inst.name))
                        })
                        .unwrap_or_else(|| (String::new(), "Object".to_owned()));
                    return Avm2VectorElemType::Class {
                        abc_idx: ca,
                        class_idx: Some(cc),
                        pkg,
                        name,
                    };
                }
                let raw = s.strip_prefix("__avm2_type:").unwrap_or(s.as_str());
                self.resolve_avm2_vector_elem_type_from_name(abc_idx, "", raw)
            }
            _ => Avm2VectorElemType::Any,
        }
    }

    pub(super) fn resolve_avm2_vector_elem_type_from_multiname(
        &self,
        abc_idx: usize,
        mn_idx: usize,
    ) -> Option<Avm2VectorElemType> {
        let abc = &self.abc_tags.get(abc_idx)?.abc;
        if let Some(super::super::avm2::AbcMultiname::TypeName { base, params }) =
            abc.multinames.get(mn_idx)
        {
            let (_, bname) = abc.split_qname(*base);
            if bname == "Vector" {
                let elem = if let Some(&p0) = params.first() {
                    if p0 == 0 {
                        Avm2VectorElemType::Any
                    } else if let Some(inner_vec) =
                        self.resolve_avm2_vector_elem_type_from_multiname(abc_idx, p0)
                    {
                        Avm2VectorElemType::Vector(Box::new(inner_vec))
                    } else {
                        let (ppkg, pname) = abc.split_qname(p0);
                        self.resolve_avm2_vector_elem_type_from_name(abc_idx, &ppkg, &pname)
                    }
                } else {
                    Avm2VectorElemType::Any
                };
                return Some(elem);
            }
        }
        let (pkg, tname) = abc.split_qname(mn_idx);
        tname
            .strip_prefix("Vector.<")
            .and_then(|r| r.strip_suffix('>'))
            .map(|inner| self.resolve_avm2_vector_elem_type_from_name(abc_idx, &pkg, inner))
    }

    pub(super) fn resolve_avm2_vector_elem_type_from_name(
        &self,
        abc_idx: usize,
        pkg: &str,
        name: &str,
    ) -> Avm2VectorElemType {
        let trimmed = name.trim();
        let stripped = trimmed
            .strip_prefix("__AS3__.vec::")
            .or_else(|| trimmed.strip_prefix("__AS3__.vec."))
            .unwrap_or(trimmed);
        if let Some(inner) = stripped
            .strip_prefix("Vector.<")
            .and_then(|r| r.strip_suffix('>'))
        {
            return Avm2VectorElemType::Vector(Box::new(
                self.resolve_avm2_vector_elem_type_from_name(abc_idx, "", inner),
            ));
        }
        match stripped {
            "int" => Avm2VectorElemType::Int,
            "uint" => Avm2VectorElemType::Uint,
            "Number" => Avm2VectorElemType::Number,
            "Boolean" => Avm2VectorElemType::Boolean,
            "String" => Avm2VectorElemType::String,
            "*" | "" => Avm2VectorElemType::Any,
            "Object" => Avm2VectorElemType::Object,
            other => {
                let (epkg, ename) = if let Some((p, s)) = other.rsplit_once("::") {
                    (p, s)
                } else if let Some((p, s)) = other.rsplit_once('.') {
                    (p, s)
                } else {
                    (pkg, other)
                };
                let found = self.find_avm2_class(abc_idx, epkg, ename);
                let resolved_pkg = if !epkg.is_empty() {
                    epkg.to_owned()
                } else if let Some((fa, fc)) = found
                    && let Some(loaded) = self.abc_tags.get(fa)
                    && let Some(inst) = loaded.abc.instances.get(fc)
                {
                    loaded.abc.split_qname(inst.name).0
                } else {
                    match ename {
                        "Sprite"
                        | "MovieClip"
                        | "Shape"
                        | "SimpleButton"
                        | "Stage"
                        | "Bitmap"
                        | "DisplayObject"
                        | "DisplayObjectContainer"
                        | "InteractiveObject" => "flash.display".to_owned(),
                        "TextField" => "flash.text".to_owned(),
                        "Dictionary" => "flash.utils".to_owned(),
                        "EventDispatcher" | "Event" | "MouseEvent" | "KeyboardEvent" => {
                            "flash.events".to_owned()
                        }
                        "ApplicationDomain" => "flash.system".to_owned(),
                        _ => String::new(),
                    }
                };
                Avm2VectorElemType::Class {
                    abc_idx: found.map_or(abc_idx, |(a, _)| a),
                    class_idx: found.map(|(_, c)| c),
                    pkg: resolved_pkg,
                    name: ename.to_owned(),
                }
            }
        }
    }

    pub(super) fn coerce_avm2_vector_element(
        &mut self,
        elem_type: &Avm2VectorElemType,
        val: Avm1Value,
        abc_idx: usize,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        match elem_type {
            Avm2VectorElemType::Int => {
                let n = self.avm2_to_number_eval(&val, abc_idx, steps, depth);
                Avm1Value::Number(f64::from(ecma_f64_to_i32(n)))
            }
            Avm2VectorElemType::Uint => {
                let n = self.avm2_to_number_eval(&val, abc_idx, steps, depth);
                Avm1Value::Number(f64::from(ecma_f64_to_u32(n)))
            }
            Avm2VectorElemType::Number => {
                let n = self.avm2_to_number_eval(&val, abc_idx, steps, depth);
                Avm1Value::Number(n)
            }
            Avm2VectorElemType::Boolean => Avm1Value::Bool(val.to_bool(9)),
            Avm2VectorElemType::String => match val {
                Avm1Value::Undefined | Avm1Value::Null => Avm1Value::Null,
                other => {
                    let s = self.avm2_coerce_to_string_eval(&other, abc_idx, steps, depth);
                    Avm1Value::String(s)
                }
            },
            Avm2VectorElemType::Any | Avm2VectorElemType::Object => {
                if matches!(val, Avm1Value::Undefined) {
                    Avm1Value::Null
                } else {
                    val
                }
            }
            Avm2VectorElemType::Vector(inner) => {
                if matches!(val, Avm1Value::Undefined | Avm1Value::Null) {
                    return Avm1Value::Null;
                }
                if let Avm1Value::Object(oid) = &val
                    && let Some(vec_data) = self
                        .avm1_objects
                        .get(*oid)
                        .and_then(|o| o.avm2_vector.as_ref())
                    && &vec_data.elem_type == inner.as_ref()
                {
                    return val;
                }
                let target = elem_type.full_as3_type_name();
                let desc = avm2_convert_string(&val, &self.avm1_objects);
                self.throw_avm2_builtin_error(
                    "TypeError",
                    1034,
                    &format!("Type Coercion failed: cannot convert {desc} to {target}."),
                );
                Avm1Value::Null
            }
            Avm2VectorElemType::Class {
                abc_idx: c_abc,
                pkg,
                name,
                ..
            } => {
                if matches!(val, Avm1Value::Undefined | Avm1Value::Null) {
                    return Avm1Value::Null;
                }
                let check_abc = if *c_abc < self.abc_tags.len() {
                    *c_abc
                } else {
                    abc_idx
                };
                if self.avm2_is_type_name(check_abc, &val, pkg, name) {
                    return val;
                }
                let target = elem_type.full_as3_type_name();
                let desc = avm2_convert_string(&val, &self.avm1_objects);
                self.throw_avm2_builtin_error(
                    "TypeError",
                    1034,
                    &format!("Type Coercion failed: cannot convert {desc} to {target}."),
                );
                Avm1Value::Null
            }
        }
    }

    pub(super) fn avm2_to_number_eval(
        &mut self,
        val: &Avm1Value,
        abc_idx: usize,
        steps: &mut usize,
        depth: usize,
    ) -> f64 {
        if matches!(val, Avm1Value::Object(_)) {
            for name in ["valueOf", "toString"] {
                if !self.avm2_has_property(val, name) {
                    continue;
                }
                let method = self.get_avm2_property_eval(val, name, steps, depth);
                if self.avm2_thrown.is_some() {
                    return f64::NAN;
                }
                if matches!(&method, Avm1Value::String(s) if is_avm2_fn_token(s)) {
                    let res = self.invoke_avm2_callable_with_this(
                        &method,
                        val.clone(),
                        Vec::new(),
                        steps,
                        depth,
                    );
                    if self.avm2_thrown.is_some() {
                        return f64::NAN;
                    }
                    if !matches!(res, Avm1Value::Object(_)) {
                        return avm2_to_number(&res);
                    }
                }
            }
        }
        let _ = abc_idx;
        avm2_to_number(val)
    }

    pub(super) fn alloc_avm2_vector(
        &mut self,
        elem_type: Avm2VectorElemType,
        fixed: bool,
        elements: Vec<Avm1Value>,
    ) -> Avm1Value {
        let oid = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object {
            avm2_vector: Some(Avm2VectorData {
                elem_type,
                fixed,
                elements,
            }),
            ..Default::default()
        });
        Avm1Value::Object(oid)
    }

    pub(super) fn construct_avm2_vector(
        &mut self,
        elem_type: Avm2VectorElemType,
        args: &[Avm1Value],
        abc_idx: usize,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        let len = if let Some(first) = args.first() {
            let n = self.avm2_to_number_eval(first, abc_idx, steps, depth);
            if self.avm2_thrown.is_some() {
                return Avm1Value::Undefined;
            }
            ecma_f64_to_u32(n).min(65_536) as usize
        } else {
            0
        };
        let fixed = args.get(1).is_some_and(|v| v.to_bool(9));
        let def = elem_type.default_element();
        self.alloc_avm2_vector(elem_type, fixed, vec![def; len])
    }

    pub(super) fn call_avm2_vector_cast(
        &mut self,
        elem_type: Avm2VectorElemType,
        args: &[Avm1Value],
        abc_idx: usize,
        steps: &mut usize,
        depth: usize,
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
            return Avm1Value::Undefined;
        }
        let src = &args[0];
        if let Avm1Value::Object(oid) = src {
            if let Some(vd) = self
                .avm1_objects
                .get(*oid)
                .and_then(|o| o.avm2_vector.clone())
            {
                if vd.elem_type == elem_type {
                    return src.clone();
                }
                let mut out = Vec::with_capacity(vd.elements.len());
                for item in vd.elements {
                    let c =
                        self.coerce_avm2_vector_element(&elem_type, item, abc_idx, steps, depth);
                    if self.avm2_thrown.is_some() {
                        return Avm1Value::Undefined;
                    }
                    out.push(c);
                }
                return self.alloc_avm2_vector(elem_type, false, out);
            }
            if let Some(arr) = self
                .avm1_objects
                .get(*oid)
                .and_then(|o| o.array_elements.clone())
            {
                let deleted = self
                    .avm1_objects
                    .get(*oid)
                    .map(|o| o.deleted_indices.clone())
                    .unwrap_or_default();
                let mut out = Vec::with_capacity(arr.len());
                for (idx, item) in arr.into_iter().enumerate() {
                    let raw = if deleted.contains(&idx) {
                        Avm1Value::Undefined
                    } else {
                        item
                    };
                    let c = self.coerce_avm2_vector_element(&elem_type, raw, abc_idx, steps, depth);
                    if self.avm2_thrown.is_some() {
                        return Avm1Value::Undefined;
                    }
                    out.push(c);
                }
                return self.alloc_avm2_vector(elem_type, false, out);
            }
        }
        let target = elem_type.full_as3_type_name();
        let desc = avm2_convert_string(src, &self.avm1_objects);
        self.throw_avm2_builtin_error(
            "TypeError",
            1034,
            &format!("Type Coercion failed: cannot convert {desc} to {target}."),
        );
        Avm1Value::Null
    }

    pub(super) fn parse_avm2_vector_index(prop: &str) -> Option<Result<isize, f64>> {
        let trimmed = prop.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Ok(i) = trimmed.parse::<isize>() {
            return Some(Ok(i));
        }
        if let Ok(n) = trimmed.parse::<f64>() {
            if n.is_finite()
                && n.fract() == 0.0
                && n >= (isize::MIN as f64)
                && n <= (isize::MAX as f64)
            {
                return Some(Ok(n as isize));
            }
            return Some(Err(n));
        }
        None
    }

    pub(super) fn get_avm2_vector_property(&mut self, oid: usize, prop: &str) -> Option<Avm1Value> {
        let vd = self.avm1_objects.get(oid)?.avm2_vector.as_ref()?;
        match prop {
            "length" => return Some(Avm1Value::Number(vd.elements.len() as f64)),
            "fixed" => return Some(Avm1Value::Bool(vd.fixed)),
            _ => {}
        }
        if let Some(parsed) = Self::parse_avm2_vector_index(prop) {
            let len = vd.elements.len();
            match parsed {
                Ok(idx) if idx >= 0 && (idx as usize) < len => {
                    return Some(vd.elements[idx as usize].clone());
                }
                Ok(idx) => {
                    self.throw_avm2_builtin_error(
                        "RangeError",
                        1125,
                        &format!("The index {idx} is out of range {len}."),
                    );
                    return Some(Avm1Value::Undefined);
                }
                Err(n) => {
                    self.throw_avm2_builtin_error(
                        "ReferenceError",
                        1069,
                        &format!("Property {n} not found on Vector and there is no default value."),
                    );
                    return Some(Avm1Value::Undefined);
                }
            }
        }
        if matches!(
            prop,
            "push"
                | "pop"
                | "shift"
                | "unshift"
                | "insertAt"
                | "removeAt"
                | "slice"
                | "splice"
                | "concat"
                | "join"
                | "toString"
                | "toLocaleString"
                | "indexOf"
                | "lastIndexOf"
                | "reverse"
                | "sort"
                | "every"
                | "some"
                | "forEach"
                | "map"
                | "filter"
                | "hasOwnProperty"
        ) {
            return Some(Avm1Value::String(format!("__avm2_bound_vec:{oid}:{prop}")));
        }
        self.throw_avm2_builtin_error(
            "ReferenceError",
            1069,
            &format!("Property {prop} not found on Vector and there is no default value."),
        );
        Some(Avm1Value::Undefined)
    }

    pub(super) fn set_avm2_vector_property(
        &mut self,
        oid: usize,
        prop: &str,
        val: Avm1Value,
        abc_idx: usize,
        steps: &mut usize,
        depth: usize,
    ) -> bool {
        let Some((elem_type, fixed, len)) = self
            .avm1_objects
            .get(oid)
            .and_then(|o| o.avm2_vector.as_ref())
            .map(|vd| (vd.elem_type.clone(), vd.fixed, vd.elements.len()))
        else {
            return false;
        };
        match prop {
            "fixed" => {
                let f = val.to_bool(9);
                if let Some(vd) = self
                    .avm1_objects
                    .get_mut(oid)
                    .and_then(|o| o.avm2_vector.as_mut())
                {
                    vd.fixed = f;
                }
                return true;
            }
            "length" => {
                let n = self.avm2_to_number_eval(&val, abc_idx, steps, depth);
                if self.avm2_thrown.is_some() {
                    return true;
                }
                let new_len = ecma_f64_to_u32(n).min(65_536) as usize;
                if fixed && new_len != len {
                    self.throw_avm2_builtin_error(
                        "RangeError",
                        1126,
                        "Cannot change the length of a fixed Vector.",
                    );
                    return true;
                }
                let def = elem_type.default_element();
                if let Some(vd) = self
                    .avm1_objects
                    .get_mut(oid)
                    .and_then(|o| o.avm2_vector.as_mut())
                {
                    vd.elements.resize(new_len, def);
                }
                return true;
            }
            _ => {}
        }
        if let Some(parsed) = Self::parse_avm2_vector_index(prop) {
            match parsed {
                Ok(idx) => {
                    if idx < 0 || (idx as usize) > len {
                        self.throw_avm2_builtin_error(
                            "RangeError",
                            1125,
                            &format!("The index {idx} is out of range {len}."),
                        );
                        return true;
                    }
                    let u = idx as usize;
                    if u == len && fixed {
                        self.throw_avm2_builtin_error(
                            "RangeError",
                            1126,
                            "Cannot change the length of a fixed Vector.",
                        );
                        return true;
                    }
                    let coerced =
                        self.coerce_avm2_vector_element(&elem_type, val, abc_idx, steps, depth);
                    if self.avm2_thrown.is_some() {
                        return true;
                    }
                    if let Some(vd) = self
                        .avm1_objects
                        .get_mut(oid)
                        .and_then(|o| o.avm2_vector.as_mut())
                    {
                        if u < vd.elements.len() {
                            vd.elements[u] = coerced;
                        } else if u == vd.elements.len() && vd.elements.len() < 65_536 {
                            vd.elements.push(coerced);
                        }
                    }
                    return true;
                }
                Err(n) => {
                    self.throw_avm2_builtin_error(
                        "ReferenceError",
                        1056,
                        &format!("Cannot create property {n} on Vector."),
                    );
                    return true;
                }
            }
        }
        self.throw_avm2_builtin_error(
            "ReferenceError",
            1056,
            &format!("Cannot create property {prop} on Vector."),
        );
        true
    }

    pub(super) fn call_avm2_vector_method(
        &mut self,
        oid: usize,
        prop: &str,
        args: &[Avm1Value],
        abc_idx: usize,
        steps: &mut usize,
        depth: usize,
    ) -> Option<Avm1Value> {
        let (elem_type, fixed, snapshot) = {
            let vd = self.avm1_objects.get(oid)?.avm2_vector.as_ref()?;
            (vd.elem_type.clone(), vd.fixed, vd.elements.clone())
        };
        let len = snapshot.len();
        match prop {
            "hasOwnProperty" => {
                let key = args
                    .first()
                    .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                let has = matches!(key.as_str(), "length" | "fixed")
                    || matches!(
                        Self::parse_avm2_vector_index(&key),
                        Some(Ok(i)) if i >= 0 && (i as usize) < len
                    );
                Some(Avm1Value::Bool(has))
            }
            "push" => {
                if fixed {
                    self.throw_avm2_builtin_error(
                        "RangeError",
                        1126,
                        "Cannot change the length of a fixed Vector.",
                    );
                    return Some(Avm1Value::Undefined);
                }
                let mut coerced = Vec::with_capacity(args.len());
                for a in args {
                    let c = self.coerce_avm2_vector_element(
                        &elem_type,
                        a.clone(),
                        abc_idx,
                        steps,
                        depth,
                    );
                    if self.avm2_thrown.is_some() {
                        return Some(Avm1Value::Undefined);
                    }
                    coerced.push(c);
                }
                let new_len = if let Some(vd) = self
                    .avm1_objects
                    .get_mut(oid)
                    .and_then(|o| o.avm2_vector.as_mut())
                {
                    for c in coerced {
                        if vd.elements.len() < 65_536 {
                            vd.elements.push(c);
                        }
                    }
                    vd.elements.len()
                } else {
                    len
                };
                Some(Avm1Value::Number(new_len as f64))
            }
            "pop" => {
                if fixed {
                    self.throw_avm2_builtin_error(
                        "RangeError",
                        1126,
                        "Cannot change the length of a fixed Vector.",
                    );
                    return Some(Avm1Value::Undefined);
                }
                let out = self
                    .avm1_objects
                    .get_mut(oid)
                    .and_then(|o| o.avm2_vector.as_mut())
                    .and_then(|vd| vd.elements.pop())
                    .unwrap_or_else(|| elem_type.empty_pop_shift_value());
                Some(out)
            }
            "shift" => {
                if fixed {
                    self.throw_avm2_builtin_error(
                        "RangeError",
                        1126,
                        "Cannot change the length of a fixed Vector.",
                    );
                    return Some(Avm1Value::Undefined);
                }
                let out = self
                    .avm1_objects
                    .get_mut(oid)
                    .and_then(|o| o.avm2_vector.as_mut())
                    .and_then(|vd| {
                        if vd.elements.is_empty() {
                            None
                        } else {
                            Some(vd.elements.remove(0))
                        }
                    })
                    .unwrap_or_else(|| elem_type.empty_pop_shift_value());
                Some(out)
            }
            "unshift" => {
                if fixed {
                    self.throw_avm2_builtin_error(
                        "RangeError",
                        1126,
                        "Cannot change the length of a fixed Vector.",
                    );
                    return Some(Avm1Value::Undefined);
                }
                let mut coerced = Vec::with_capacity(args.len());
                for a in args {
                    let c = self.coerce_avm2_vector_element(
                        &elem_type,
                        a.clone(),
                        abc_idx,
                        steps,
                        depth,
                    );
                    if self.avm2_thrown.is_some() {
                        return Some(Avm1Value::Undefined);
                    }
                    coerced.push(c);
                }
                let new_len = if let Some(vd) = self
                    .avm1_objects
                    .get_mut(oid)
                    .and_then(|o| o.avm2_vector.as_mut())
                {
                    if vd.elements.len() + coerced.len() <= 65_536 {
                        coerced.append(&mut vd.elements);
                        vd.elements = coerced;
                    }
                    vd.elements.len()
                } else {
                    len
                };
                Some(Avm1Value::Number(new_len as f64))
            }
            "insertAt" => {
                if fixed {
                    self.throw_avm2_builtin_error(
                        "RangeError",
                        1126,
                        "Cannot change the length of a fixed Vector.",
                    );
                    return Some(Avm1Value::Undefined);
                }
                let raw_idx = args
                    .first()
                    .map_or(0, |v| ecma_f64_to_i32(avm2_to_number(v)));
                let ins_val = args.get(1).cloned().unwrap_or(Avm1Value::Undefined);
                let c = self.coerce_avm2_vector_element(&elem_type, ins_val, abc_idx, steps, depth);
                if self.avm2_thrown.is_some() {
                    return Some(Avm1Value::Undefined);
                }
                if let Some(vd) = self
                    .avm1_objects
                    .get_mut(oid)
                    .and_then(|o| o.avm2_vector.as_mut())
                {
                    let cur_len = vd.elements.len();
                    let pos = if raw_idx < 0 {
                        (cur_len as isize + raw_idx as isize).max(0) as usize
                    } else {
                        (raw_idx as usize).min(cur_len)
                    };
                    if cur_len < 65_536 {
                        vd.elements.insert(pos, c);
                    }
                }
                Some(Avm1Value::Undefined)
            }
            "removeAt" => {
                if fixed {
                    self.throw_avm2_builtin_error(
                        "RangeError",
                        1126,
                        "Cannot change the length of a fixed Vector.",
                    );
                    return Some(Avm1Value::Undefined);
                }
                let raw_idx = args
                    .first()
                    .map_or(0, |v| ecma_f64_to_i32(avm2_to_number(v)));
                let pos = if raw_idx < 0 {
                    len as isize + raw_idx as isize
                } else {
                    raw_idx as isize
                };
                if pos < 0 || (pos as usize) >= len {
                    self.throw_avm2_builtin_error(
                        "RangeError",
                        1125,
                        &format!("The index {pos} is out of range {len}."),
                    );
                    return Some(Avm1Value::Undefined);
                }
                let removed = self
                    .avm1_objects
                    .get_mut(oid)
                    .and_then(|o| o.avm2_vector.as_mut())
                    .map(|vd| vd.elements.remove(pos as usize))
                    .unwrap_or(Avm1Value::Undefined);
                Some(removed)
            }
            "reverse" => {
                if let Some(vd) = self
                    .avm1_objects
                    .get_mut(oid)
                    .and_then(|o| o.avm2_vector.as_mut())
                {
                    vd.elements.reverse();
                }
                Some(Avm1Value::Object(oid))
            }
            "slice" => {
                let start_raw = args.first().map_or(0.0, avm2_to_number);
                let end_raw = args.get(1).map_or(len as f64, avm2_to_number);
                let norm = |v: f64| -> usize {
                    if v.is_nan() {
                        0
                    } else if v < 0.0 {
                        ((len as f64) + v.trunc()).max(0.0) as usize
                    } else {
                        (v.trunc() as usize).min(len)
                    }
                };
                let s = norm(start_raw);
                let e = norm(end_raw);
                let sub = if e > s {
                    snapshot[s..e].to_vec()
                } else {
                    Vec::new()
                };
                Some(self.alloc_avm2_vector(elem_type, false, sub))
            }
            "splice" => {
                if fixed {
                    self.throw_avm2_builtin_error(
                        "RangeError",
                        1126,
                        "Cannot change the length of a fixed Vector.",
                    );
                    return Some(Avm1Value::Undefined);
                }
                let start_raw = args.first().map_or(0.0, avm2_to_number);
                let start = if start_raw.is_nan() {
                    0
                } else if start_raw < 0.0 {
                    ((len as f64) + start_raw.trunc()).max(0.0) as usize
                } else {
                    (start_raw.trunc() as usize).min(len)
                };
                let del_cnt = if let Some(dc) = args.get(1) {
                    let n = avm2_to_number(dc);
                    if n.is_nan() || n <= 0.0 {
                        0
                    } else {
                        (n.trunc() as usize).min(len.saturating_sub(start))
                    }
                } else {
                    len.saturating_sub(start)
                };
                let mut ins = Vec::new();
                for item in args.iter().skip(2) {
                    let c = self.coerce_avm2_vector_element(
                        &elem_type,
                        item.clone(),
                        abc_idx,
                        steps,
                        depth,
                    );
                    if self.avm2_thrown.is_some() {
                        return Some(Avm1Value::Undefined);
                    }
                    ins.push(c);
                }
                let removed = if let Some(vd) = self
                    .avm1_objects
                    .get_mut(oid)
                    .and_then(|o| o.avm2_vector.as_mut())
                {
                    vd.elements.splice(start..start + del_cnt, ins).collect()
                } else {
                    Vec::new()
                };
                Some(self.alloc_avm2_vector(elem_type, false, removed))
            }
            "concat" => {
                let mut out = snapshot;
                for a in args {
                    if let Avm1Value::Object(a_oid) = a
                        && let Some(other_vd) = self
                            .avm1_objects
                            .get(*a_oid)
                            .and_then(|o| o.avm2_vector.clone())
                    {
                        for item in other_vd.elements {
                            let c = self.coerce_avm2_vector_element(
                                &elem_type, item, abc_idx, steps, depth,
                            );
                            if self.avm2_thrown.is_some() {
                                return Some(Avm1Value::Undefined);
                            }
                            if out.len() < 65_536 {
                                out.push(c);
                            }
                        }
                    } else {
                        self.throw_avm2_builtin_error(
                            "TypeError",
                            1034,
                            "Type Coercion failed: cannot concat non-Vector into Vector.",
                        );
                        return Some(Avm1Value::Undefined);
                    }
                }
                Some(self.alloc_avm2_vector(elem_type, false, out))
            }
            "join" | "toString" | "toLocaleString" => {
                let sep = if prop == "join" {
                    args.first()
                        .filter(|v| !matches!(v, Avm1Value::Undefined))
                        .map_or_else(
                            || ",".to_owned(),
                            |v| avm2_convert_string(v, &self.avm1_objects),
                        )
                } else {
                    ",".to_owned()
                };
                let mut parts = Vec::with_capacity(snapshot.len());
                for el in &snapshot {
                    if matches!(el, Avm1Value::Undefined | Avm1Value::Null) {
                        parts.push(String::new());
                    } else {
                        let s = self.avm2_coerce_to_string_eval(el, abc_idx, steps, depth);
                        if self.avm2_thrown.is_some() {
                            return Some(Avm1Value::Undefined);
                        }
                        parts.push(s);
                    }
                }
                Some(Avm1Value::String(parts.join(&sep)))
            }
            "indexOf" => {
                let needle = args.first().cloned().unwrap_or(Avm1Value::Undefined);
                let from_raw = args
                    .get(1)
                    .map_or(0, |v| ecma_f64_to_i32(avm2_to_number(v)));
                let start = if from_raw < 0 {
                    ((len as isize) + (from_raw as isize)).max(0) as usize
                } else {
                    from_raw as usize
                };
                for (i, el) in snapshot.iter().enumerate().skip(start) {
                    if self.avm2_strict_equals(el, &needle) {
                        return Some(Avm1Value::Number(i as f64));
                    }
                }
                Some(Avm1Value::Number(-1.0))
            }
            "lastIndexOf" => {
                let needle = args.first().cloned().unwrap_or(Avm1Value::Undefined);
                if snapshot.is_empty() {
                    return Some(Avm1Value::Number(-1.0));
                }
                let start = if let Some(f) = args.get(1) {
                    let fr = ecma_f64_to_i32(avm2_to_number(f));
                    let p = if fr < 0 {
                        (len as isize) + (fr as isize)
                    } else {
                        (fr as isize).min(len.saturating_sub(1) as isize)
                    };
                    if p < 0 {
                        return Some(Avm1Value::Number(-1.0));
                    }
                    p as usize
                } else {
                    len - 1
                };
                for i in (0..=start).rev() {
                    if self.avm2_strict_equals(&snapshot[i], &needle) {
                        return Some(Avm1Value::Number(i as f64));
                    }
                }
                Some(Avm1Value::Number(-1.0))
            }
            "sort" => {
                let cmp_fn = args
                    .first()
                    .filter(|v| matches!(v, Avm1Value::String(s) if is_avm2_fn_token(s)))
                    .cloned();
                let mut sorted = snapshot;
                if let Some(cb) = cmp_fn {
                    // Insertion sort so we can invoke AVM2 comparator closure deterministically
                    for i in 1..sorted.len() {
                        let mut j = i;
                        while j > 0 {
                            let cmp_val = self.invoke_avm2_callable_with_this(
                                &cb,
                                Avm1Value::Null,
                                vec![sorted[j - 1].clone(), sorted[j].clone()],
                                steps,
                                depth + 1,
                            );
                            if self.avm2_thrown.is_some() {
                                return Some(Avm1Value::Undefined);
                            }
                            if avm2_to_number(&cmp_val) > 0.0 {
                                sorted.swap(j - 1, j);
                                j -= 1;
                            } else {
                                break;
                            }
                        }
                    }
                } else {
                    sorted.sort_by(|a, b| {
                        let sa = avm2_convert_string(a, &self.avm1_objects);
                        let sb = avm2_convert_string(b, &self.avm1_objects);
                        sa.cmp(&sb)
                    });
                }
                if let Some(vd) = self
                    .avm1_objects
                    .get_mut(oid)
                    .and_then(|o| o.avm2_vector.as_mut())
                {
                    vd.elements = sorted;
                }
                Some(Avm1Value::Object(oid))
            }
            "every" | "some" | "forEach" | "filter" | "map" => {
                let Some(cb) = args.first().cloned() else {
                    return Some(Avm1Value::Undefined);
                };
                if matches!(cb, Avm1Value::Null | Avm1Value::Undefined) {
                    return Some(match prop {
                        "every" => Avm1Value::Bool(true),
                        "some" => Avm1Value::Bool(false),
                        "filter" | "map" => self.alloc_avm2_vector(elem_type, false, Vec::new()),
                        _ => Avm1Value::Undefined,
                    });
                }
                let this_obj = args.get(1).cloned().unwrap_or(Avm1Value::Null);
                let mut out_items = Vec::new();
                for (i, item) in snapshot.iter().enumerate() {
                    let r = self.invoke_avm2_callable_with_this(
                        &cb,
                        this_obj.clone(),
                        vec![
                            item.clone(),
                            Avm1Value::Number(i as f64),
                            Avm1Value::Object(oid),
                        ],
                        steps,
                        depth + 1,
                    );
                    if self.avm2_thrown.is_some() {
                        return Some(Avm1Value::Undefined);
                    }
                    match prop {
                        "every" if !r.to_bool(9) => return Some(Avm1Value::Bool(false)),
                        "some" if r.to_bool(9) => return Some(Avm1Value::Bool(true)),
                        "filter" if r.to_bool(9) => out_items.push(item.clone()),
                        "map" => {
                            let c = self
                                .coerce_avm2_vector_element(&elem_type, r, abc_idx, steps, depth);
                            if self.avm2_thrown.is_some() {
                                return Some(Avm1Value::Undefined);
                            }
                            out_items.push(c);
                        }
                        _ => {}
                    }
                }
                Some(match prop {
                    "every" => Avm1Value::Bool(true),
                    "some" => Avm1Value::Bool(false),
                    "filter" | "map" => self.alloc_avm2_vector(elem_type, false, out_items),
                    _ => Avm1Value::Undefined,
                })
            }
            _ => None,
        }
    }
}
