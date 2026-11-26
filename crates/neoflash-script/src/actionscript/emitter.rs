//! JavaScript code generator for ActionScript statements, functions, and references.

use super::*;

pub(super) struct Emitter<'a> {
    pub(super) with_targets: Vec<(usize, String)>,
    pub(super) tell_target: Option<String>,
    pub(super) as3: bool,
    pub(super) analysis: &'a scope::Analysis<'a>,
    pub(super) scope: usize,
}

impl Emitter<'_> {
    pub(super) fn marker(&self, statement: &Stmt) -> String {
        match (&self.analysis.context.mapping_prefix, statement) {
            (Some(prefix), Stmt::Located(offset, _)) => format!("{prefix}{offset}*/"),
            _ => String::new(),
        }
    }

    pub(super) fn legacy_case(&self) -> bool {
        self.analysis.context.legacy_case
    }
    pub(super) fn container_tail(&self) -> String {
        if self.legacy_case() {
            ", undefined, true".into()
        } else {
            self.caller_argument()
        }
    }
    pub(super) fn index_tail(&self) -> &'static str {
        if self.legacy_case() {
            ", undefined, true"
        } else {
            ""
        }
    }
    pub(super) fn legacy_receiver(&self) -> String {
        let fallback = if self.analysis.context.class_name.is_some()
            && self.analysis.context.method_id.is_none()
        {
            "this"
        } else {
            "__nfFrame"
        };
        let target = self.tell_target.as_deref().unwrap_or(fallback);
        if self.legacy_case() {
            format!("{{__nfTimeline: {target}, __nfLegacyCase: true}}")
        } else {
            target.to_owned()
        }
    }
    pub(super) fn dotted_value_expr(&self, name: &str) -> String {
        let mut parts = name.split('.');
        let first = parts.next().expect("non-empty type name");
        let mut expr = Expr::Name(first.to_owned());
        let mut has_dot = false;
        for part in parts {
            has_dot = true;
            expr = Expr::Member(Box::new(expr), part.to_owned());
        }
        let primary = self.expression(&expr);
        if !has_dot
            && self.legacy_case()
            && self
                .analysis
                .resolve(self.scope, first)
                .is_none_or(|(id, _)| id == 0)
        {
            format!(
                "((__nfVal) => __nfVal !== undefined ? __nfVal : (typeof __nfLegacy !== 'undefined' && __nfLegacy?._global ? __nfFamilyPropertyValue(__nfLegacy._global, {first:?}, false, undefined, true) : undefined))({primary})"
            )
        } else {
            primary
        }
    }
    pub(super) fn active_timeline(&self) -> &str {
        self.tell_target
            .as_deref()
            .unwrap_or(if self.scope == 0 { "this" } else { "__nfFrame" })
    }
    pub(super) fn coerce(&self, name: &str, value: String) -> String {
        self.coerce_type(
            self.analysis
                .resolve(self.scope, name)
                .and_then(|(_, b)| b.ty.as_deref()),
            value,
        )
    }
    pub(super) fn coerce_type(&self, ty: Option<&str>, value: String) -> String {
        if !self.as3 {
            return value;
        }
        let number = self.intrinsic("Number");
        let boolean = self.intrinsic("Boolean");
        let string = self.intrinsic("String");
        match ty {
            Some(ty) if vector_element(ty).is_some() => {
                format!("__nfVectorCast({value}, {})", self.vector_type(ty))
            }
            Some("Number") => format!("{number}({value})"),
            Some("Boolean") => format!("{boolean}({value})"),
            Some("int") => format!("({number}({value}) | 0)"),
            Some("uint") => format!("({number}({value}) >>> 0)"),
            Some("String") => {
                format!("((__nfValue) => __nfValue == null ? null : {string}(__nfValue))({value})")
            }
            Some("Function") => format!(
                "((__nfValue) => {{ if (__nfValue == null) return null; if (typeof __nfValue !== 'function' || __nfNumericClassName(__nfValue)!==null) throw new __nfTypeError('AS3 Function value required'); return __nfValue; }})({value})"
            ),
            Some("Class") => format!("__nfClassCast({value})"),
            Some("Object") => {
                format!("((__nfValue) => __nfValue == null ? null : __nfValue)({value})")
            }
            Some("Array") => format!(
                "((__nfValue) => {{ if (__nfValue == null || __nfArrayIsArray(__nfValue)) return __nfValue == null ? null : __nfValue; throw new __nfTypeError('AS3 Array value required'); }})({value})"
            ),
            Some(ty @ ("XML" | "XMLList" | "Namespace" | "QName"))
                if self.analysis.context.intrinsic_type(ty, true) =>
            {
                format!("__nfXMLCast({ty:?}, {value})")
            }
            Some(ty @ ("Date" | "RegExp")) if self.analysis.context.intrinsic_type(ty, true) => {
                format!(
                    "((__nfValue) => {{ if (__nfValue == null) return null; if (__nf{ty}Is(__nfValue)) return __nfValue; throw new __nfTypeError('AS3 {ty} value required'); }})({value})"
                )
            }
            Some(ty) if is_error_type(ty) => format!(
                "((__nfValue) => {{ if (__nfValue == null) return null; if (__nfCatchType(__nfValue, {ty:?})) return __nfValue; throw new __nfTypeError('AS3 {ty} value required'); }})({value})"
            ),
            Some(ty) if self.analysis.context.types.contains_key(ty) => format!(
                "__nfProgram.cast({:?}, {value})",
                self.analysis.context.types[ty]
            ),
            Some(ty) if !self.analysis.context.project && is_as3_wave7_type(ty) => {
                format!("__nfWave7Cast({ty:?}, {value})")
            }
            _ => value,
        }
    }
    pub(super) fn intrinsic(&self, name: &str) -> String {
        format!("__nf{name}")
    }
    pub(super) fn type_descriptor(&self, ty: &str) -> String {
        if let Some(element) = vector_element(ty) {
            return format!("__nfVectorDescriptor({})", self.type_descriptor(element));
        }
        if self.analysis.context.types.contains_key(ty)
            && !matches!(ty, "Object" | "Array")
            && !self.analysis.context.intrinsic_type(ty, true)
        {
            return format!(
                "__nfProgram.elementDescriptor({:?})",
                self.analysis.context.types[ty]
            );
        }
        format!("__nfPrimitiveDescriptor({ty:?})")
    }
    pub(super) fn vector_type(&self, ty: &str) -> String {
        format!(
            "__nfVectorType({})",
            self.type_descriptor(vector_element(ty).expect("Vector type"))
        )
    }
    pub(super) fn typed(&self, name: &str) -> bool {
        self.analysis
            .resolve(self.scope, name)
            .is_some_and(|(_, b)| b.ty.is_some())
    }
    pub(super) fn name(&self, name: &str) -> String {
        if let Some(reference) = self.with_reference(name) {
            return format!("({reference}).get()");
        }
        self.unscoped_name(name)
    }
    pub(super) fn without_with(&self) -> Emitter<'_> {
        Emitter {
            as3: self.as3,
            analysis: self.analysis,
            scope: self.scope,
            with_targets: Vec::new(),
            tell_target: self.tell_target.clone(),
        }
    }
    pub(super) fn scope_at_or_inside(&self, inner: usize, outer: usize) -> bool {
        let mut cur = Some(inner);
        while let Some(id) = cur {
            if id == outer {
                return true;
            }
            cur = self.analysis.scopes[&id].parent;
        }
        false
    }
    pub(super) fn with_reference(&self, name: &str) -> Option<String> {
        if self.with_targets.is_empty()
            || matches!(name, "this" | "super" | "eval" | "set")
            || (!self.as3 && parse_as2_level(name).is_some())
        {
            return None;
        }
        let min_scope = self
            .analysis
            .resolve(self.scope, name)
            .map_or(0, |(id, _)| id);
        let active_targets: Vec<&str> = self
            .with_targets
            .iter()
            .filter(|(with_scope, _)| self.scope_at_or_inside(*with_scope, min_scope))
            .map(|(_, target)| target.as_str())
            .collect();
        if active_targets.is_empty() {
            return None;
        }
        let plain = self.without_with();
        let timeline = plain.active_timeline().to_owned();
        let expression = Expr::Name(name.to_owned());
        let receiver = plain.bare_member_receiver(name).or_else(|| {
            let raw = plain.unscoped_name(name);
            if raw == format!("{timeline}.{name}") {
                Some(timeline)
            } else {
                None
            }
        });
        let fallback = receiver
            .as_ref()
            .map(|owner| {
                format!(
                    "__nfContainerRef({owner},{name:?},{}{})",
                    self.as3,
                    self.container_tail()
                )
            })
            .or_else(|| plain.reference(&expression))
            .unwrap_or_else(|| {
                let value = plain.unscoped_name(name);
                let resolved = self.analysis.resolve(self.scope, name);
                let write = if resolved.is_some()
                    || (!is_global(name) && !self.analysis.context.types.contains_key(name))
                {
                    format!("__nfAssigned=>({value}=__nfAssigned)")
                } else {
                    "()=>{throw new __nfTypeError('Cannot replace this built-in binding.');}".into()
                };
                let has_expr = if resolved.is_some_and(|(id, _)| id != 0)
                    || is_global(name)
                    || self.analysis.context.types.contains_key(name)
                {
                    "true".into()
                } else {
                    format!("({value} !== undefined)")
                };
                format!("{{get:()=>{value},set:{write},delete:()=>false,has:()=>{has_expr}}}")
            });
        let receiver = receiver.unwrap_or_else(|| "undefined".into());
        Some(format!(
            "__nfWithRef([{}],{name:?},()=>({fallback}),{receiver},{}{})",
            active_targets
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(","),
            self.as3,
            self.container_tail()
        ))
    }
    pub(super) fn unscoped_name(&self, name: &str) -> String {
        if name == "arguments" {
            return "__nfArgumentsValue".into();
        }
        if self.as3
            && matches!(name, "Number" | "int" | "uint" | "Boolean")
            && self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.core_type(name)
        {
            return format!("__nfNumericClass({name:?})");
        }
        if self.as3
            && name == "Class"
            && self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.core_type(name)
        {
            return "__nfClassMetaclass".into();
        }
        if self.as3
            && name == "Function"
            && self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.core_type(name)
        {
            return "__nfFunctionClass".into();
        }
        if self.as3
            && is_error_type(name)
            && self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.core_type(name)
        {
            return format!("__nfErrorClass({name:?}, true)");
        }
        if self.as3
            && self.analysis.resolve(self.scope, name).is_none()
            && let Some(uri) = self.analysis.context.namespaces.get(name)
        {
            return format!("__nfXML.namespace(undefined, {uri:?})");
        }
        if name == "Math"
            && self.analysis.resolve(self.scope, name).is_none()
            && !self.analysis.context.types.contains_key(name)
        {
            return format!("__nfMath({})", self.as3);
        }
        if !self.as3
            && matches!(
                name,
                "setInterval" | "setTimeout" | "clearInterval" | "clearTimeout"
            )
            && self.analysis.resolve(self.scope, name).is_none()
            && !self.analysis.context.functions.contains_key(name)
        {
            return format!("__nfLegacy.{name}");
        }
        if !self.as3
            && is_as2_frame_global(name)
            && self.analysis.resolve(self.scope, name).is_none()
            && !self.analysis.context.types.contains_key(name)
        {
            return format!("__nfLegacy.{name}.bind({})", self.legacy_receiver());
        }
        if self.analysis.resolve(self.scope, name).is_none() && (!self.as3 && name == "getTimer") {
            return "__nfProgram.type(\"flash.utils.getTimer\")".into();
        }
        if self.analysis.resolve(self.scope, name).is_none()
            && let Some(path) = self.analysis.context.functions.get(name)
        {
            return format!("__nfProgram.type({path:?})");
        }
        if !self.as3
            && self.analysis.resolve(self.scope, name).is_none()
            && let Some(level) = parse_as2_level(name)
        {
            return format!("__nfLegacy.level({level})");
        }
        if !self.as3
            && self.analysis.resolve(self.scope, name).is_none()
            && is_as2_global_prop(name)
        {
            return format!("__nfLegacy.globalPropRef({name:?}).get()");
        }
        if !self.as3
            && self.analysis.resolve(self.scope, name).is_none()
            && (matches!(
                name,
                "Key"
                    | "Mouse"
                    | "Stage"
                    | "LoadVars"
                    | "TextFormat"
                    | "TextField"
                    | "Accessibility"
                    | "_global"
                    | "newline"
                    | "Sound"
                    | "System"
                    | "flash"
                    | "targetPath"
                    | "getProperty"
                    | "setProperty"
                    | "getVersion"
                    | "updateAfterEvent"
                    | "stopAllSounds"
                    | "toggleHighQuality"
                    | "showRedrawRegions"
                    | "duplicateMovieClip"
                    | "removeMovieClip"
                    | "startDrag"
                    | "stopDrag"
            ) || (matches!(
                name,
                "MovieClip"
                    | "Button"
                    | "TextSnapshot"
                    | "Video"
                    | "MovieClipLoader"
                    | "AsBroadcaster"
                    | "Color"
                    | "SharedObject"
                    | "Selection"
                    | "Camera"
                    | "Microphone"
                    | "ContextMenu"
                    | "ContextMenuItem"
                    | "PrintJob"
                    | "LocalConnection"
                    | "XMLSocket"
                    | "NetConnection"
                    | "NetStream"
                    | "FileReference"
                    | "FileReferenceList"
                    | "TextRenderer"
            ) && !self.analysis.context.types.contains_key(name)))
        {
            return format!("__nfLegacy.{name}");
        }
        if self.as3
            && !self.analysis.context.project
            && self.analysis.resolve(self.scope, name).is_none()
            && (is_as3_wave7_type(name) || name == "flash")
        {
            return format!("__nfWave7Class({name:?})");
        }
        if self
            .analysis
            .resolve(self.scope, name)
            .is_some_and(|(id, binding)| id == 0 && binding.kind == scope::BindingKind::Field)
            && let Some(owner) = self.analysis.context.static_members.get(name)
        {
            return format!("__nfProgram.type({owner:?}).{name}");
        }
        if !self.as3
            && name == "Boolean"
            && self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.core_type(name)
        {
            return "__nfBoolean2".into();
        }
        if !self.as3
            && name == "Number"
            && self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.core_type(name)
        {
            return if self.legacy_case() {
                "__nfNumber1"
            } else {
                "__nfNumber2"
            }
            .into();
        }
        if self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.utility_function(name, self.as3)
        {
            return if self.as3 {
                format!("__nfUriFunctions.{name}")
            } else {
                match name {
                    "escape" => "__nfEscape2",
                    "unescape" => "__nfUnescape2",
                    "chr" | "ord" | "length" | "substring" | "mbchr" | "mbord" | "mblength"
                    | "mbsubstring" => return format!("__nfLegacyStringGlobals.{name}"),
                    _ => "__nfRandom2",
                }
                .into()
            };
        }
        if name == "Object"
            && self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.core_type(name)
        {
            return format!("__nfObjectClass({})", self.as3);
        }
        if self.analysis.resolve(self.scope, name).is_none() && matches!(name, "Array" | "String") {
            return format!("__nf{name}Class({})", self.as3);
        }
        if self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.intrinsic_type(name, self.as3)
        {
            return if matches!(name, "XML" | "XMLList" | "XMLNode" | "Namespace" | "QName") {
                format!("__nfXMLClass({name:?}, {})", self.as3)
            } else if name == "JSON" {
                "__nfJSON".into()
            } else {
                format!("__nf{name}Class({})", self.as3)
            };
        }
        if name == "__nfProgram" && self.analysis.context.project {
            return name.into();
        }
        if name == "super"
            && let Some(class) = &self.analysis.context.class_name
        {
            return format!("__nfProgram.superView({class:?}, __nfFrame)");
        }
        if self.analysis.resolve(self.scope, name).is_none()
            && let Some(qname) = self.analysis.context.types.get(name)
        {
            if !self.as3 && qname == "flash.external.ExternalInterface" {
                return "__nfLegacy.ExternalInterface".into();
            }
            return format!("__nfProgram.type({qname:?})");
        }
        if self.as3
            && self.analysis.resolve(self.scope, name).is_none()
            && let Some(uri) = self.analysis.context.namespaces.get(name)
        {
            return format!("(__nfXML?.namespace?.({name:?}, {uri:?}) ?? {uri:?})");
        }
        if name == "parseInt" && self.analysis.resolve(self.scope, name).is_none() {
            return if self.as3 {
                "__nfParseInt3"
            } else {
                "__nfParseInt2"
            }
            .into();
        }
        if let Some((id, binding)) = self.analysis.resolve(self.scope, name)
            && id != 0
        {
            return binding.canonical.clone();
        }
        if self.as3
            && self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.core_type(name)
        {
            if name == "Function" {
                return "__nfFunctionClass".into();
            }
            if name == "Class" {
                return "__nfClassMetaclass".into();
            }
        }
        if name == "this" || (is_global(name) && !is_timeline_method(name) && name != "trace") {
            name.into()
        } else if name == "trace" {
            "__nfTrace".into()
        } else {
            format!("{}.{name}", self.active_timeline())
        }
    }
    /// Lexical access travels with emitted code, never with an ambient runtime
    /// current-class stack. Computed/indexed access deliberately omits it.
    pub(super) fn caller_argument(&self) -> String {
        if self.as3 && !self.analysis.context.open_namespaces.is_empty() {
            let cls = self
                .analysis
                .context
                .class_name
                .as_ref()
                .map_or_else(|| "null".to_owned(), |name| format!("{name:?}"));
            let ns = self
                .analysis
                .context
                .open_namespaces
                .iter()
                .map(|u| format!("{u:?}"))
                .collect::<Vec<_>>()
                .join(",");
            return format!(", {{class:{cls},ns:[{ns}]}}");
        }
        self.analysis
            .context
            .class_name
            .as_ref()
            .map_or_else(String::new, |name| format!(", {name:?}"))
    }
    pub(super) fn open_namespace_uri_for_member(&self, name: &str) -> Option<&str> {
        if !self.as3 {
            return None;
        }
        for uri in &self.analysis.context.open_namespaces {
            if self
                .analysis
                .context
                .namespace_members
                .contains(&(uri.clone(), name.to_owned()))
            {
                return Some(uri.as_str());
            }
        }
        None
    }
    pub(super) fn bare_member_receiver(&self, name: &str) -> Option<String> {
        if self.analysis.context.class_name.is_none() && !self.analysis.context.legacy_case {
            return None;
        }
        if self
            .analysis
            .resolve(self.scope, name)
            .is_some_and(|(id, b)| id == 0 && b.kind == scope::BindingKind::Field)
        {
            return Some(
                self.analysis
                    .context
                    .static_members
                    .get(name)
                    .map(|owner| format!("__nfProgram.type({owner:?})"))
                    .unwrap_or_else(|| {
                        if self.scope == 0 {
                            "this".into()
                        } else {
                            "__nfFrame".into()
                        }
                    }),
            );
        }
        // Unqualified dynamic/native members also need the sealed-object gate.
        // Locals, built-in globals and type names retain their existing binding.
        let timeline = self.active_timeline();
        let raw = self.unscoped_name(name);
        if raw == format!("{timeline}.{name}") {
            Some(timeline.into())
        } else {
            None
        }
    }
    pub(super) fn as2_lexical_reference(&self) -> String {
        let mut names = std::collections::BTreeMap::new();
        let mut scope = Some(self.scope);
        while let Some(id) = scope {
            if id != 0 || self.tell_target.is_none() {
                for (key, binding) in &self.analysis.scopes[&id].bindings {
                    names
                        .entry(key.clone())
                        .or_insert_with(|| binding.canonical.clone());
                }
            }
            scope = self.analysis.scopes[&id].parent;
        }
        let cases = names
            .into_iter()
            .map(|(key, canonical)| {
                let expression = Expr::Name(canonical.clone());
                let value = self.name(&canonical);
                let reference = self.reference(&expression).unwrap_or_else(|| {
                    let timeline = self.active_timeline();
                    let delete_fn = if self.unscoped_name(&canonical) == format!("{timeline}.{canonical}") {
                        format!(
                            "()=>__nfContainerRef({timeline}, {canonical:?}, false{}).delete()",
                            self.container_tail()
                        )
                    } else {
                        "()=>false".into()
                    };
                    format!("{{get:()=>{value},set:__nfAssigned=>({value}=__nfAssigned),delete:{delete_fn}}}")
                });
                format!("case {key:?}: return {reference};")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let selector = if self.legacy_case() {
            "__nfAs2Strings().invoke(\"toLowerCase\", __nfString(__nfName), [])"
        } else {
            "__nfName"
        };
        let with_lookup = if self.with_targets.is_empty() || self.tell_target.is_some() {
            String::new()
        } else {
            let targets = self
                .with_targets
                .iter()
                .rev()
                .map(|(_, t)| t.as_str())
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "for(const __nfWithObj of [{targets}]){{const __nfRef=__nfContainerRef(__nfWithObj,__nfName,false{});if(__nfRef.has())return __nfRef;}}",
                self.container_tail()
            )
        };
        format!("(__nfName)=>{{{with_lookup}switch({selector}){{{cases}}}return null;}}")
    }
    pub(super) fn default_value(&self, ty: Option<&str>) -> &'static str {
        if !self.as3 {
            return "undefined";
        }
        match ty {
            Some(ty) if vector_element(ty).is_some() => "null",
            Some("Number") => "NaN",
            Some("Boolean") => "false",
            Some("String" | "Function" | "Class" | "Object" | "Array") => "null",
            Some(ty @ ("Date" | "RegExp" | "XML" | "XMLList" | "Namespace" | "QName"))
                if self.analysis.context.intrinsic_type(ty, true) =>
            {
                "null"
            }
            Some(ty) if is_error_type(ty) => "null",
            Some("int" | "uint") => "0",
            Some(ty) if self.analysis.context.types.contains_key(ty) => "null",
            Some(ty) if !self.analysis.context.project && is_as3_wave7_type(ty) => "null",
            _ => "undefined",
        }
    }
    pub(super) fn function(&self, f: &Function) -> String {
        let inner = Emitter {
            as3: self.as3,
            analysis: self.analysis,
            scope: f.id,
            with_targets: self.with_targets.clone(),
            tell_target: None,
        };
        let mut prologue = String::new();
        if self.analysis.context.method_id == Some(f.id) {
            prologue.push_str("const __nfFrame = this;\n");
        }
        if !f.declaration
            && let Some(name) = &f.name
        {
            prologue.push_str(&format!("const {name} = __nfActivation.callee;\n"));
        }
        for (index, (name, ty)) in f.params.iter().enumerate() {
            let value = format!("__nfActivation.values[{index}]");
            let value = if let Some(default) = &f.defaults[index] {
                format!(
                    "(__nfActivation.values.length > {index} ? {value} : {})",
                    inner.expression(default)
                )
            } else {
                value
            };
            prologue.push_str(&format!("var {name} = {value};\n"));
            if self.as3 && ty.is_some() {
                prologue.push_str(&format!("{name} = {};\n", inner.coerce(name, name.clone())));
            }
        }
        if let Some(name) = &f.rest {
            prologue.push_str(&format!(
                "var {name} = __nfActivation.values.slice({});\n",
                f.params.len()
            ));
        }
        let uses_arguments = self.analysis.scopes[&f.id].uses_arguments;
        if uses_arguments {
            prologue.push_str(&format!(
                "const __nfArgumentsValue = __nfFunctionArguments(__nfActivation, [{}]);\n",
                f.params
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        for binding in self.analysis.scopes[&f.id].bindings.values() {
            if binding.kind == scope::BindingKind::Variable {
                prologue.push_str(&format!(
                    "var {} = {};\n",
                    binding.canonical,
                    inner.default_value(binding.ty.as_deref())
                ));
            }
        }
        let required = f
            .defaults
            .iter()
            .take_while(|value| value.is_none())
            .count();
        let fn_body = if self.as3 && has_default_xml_ns(&f.body) {
            format!(
                "const __nfPrevXmlNs = typeof __nfXML !== 'undefined' && __nfXML?.getDefaultNamespace ? __nfXML.getDefaultNamespace() : '';\ntry {{\n{}\n}} finally {{\nif (typeof __nfXML !== 'undefined' && __nfXML?.setDefaultNamespace) __nfXML.setDefaultNamespace(__nfPrevXmlNs);\n}}",
                inner.body(&f.body)
            )
        } else {
            inner.body(&f.body)
        };
        let constructible = !self.as3
            || !(f.declaration
                && f.returns
                    .as_deref()
                    .is_some_and(|r| !matches!(r, "void" | "*" | "Object")));
        format!(
            "__nfSourceFunction(function(__nfActivation) {{\n{prologue}{fn_body}\n}}, {{as3:{}, length:{}, required:{required}, allowExtra:{}, constructible:{constructible}, defaultReceiver:typeof __nfFrame === 'undefined' ? undefined : __nfFrame}})",
            self.as3,
            f.params.len(),
            !self.as3 || f.rest.is_some() || uses_arguments,
        )
    }
    pub(super) fn body(&self, body: &[Stmt]) -> String {
        let mut output = String::new();
        for statement in body {
            if let Stmt::Function(f) = statement.unlocated() {
                output.push_str(&self.marker(statement));
                let code = self.function(f);
                let fn_name = f.name.as_deref().unwrap();
                if self.scope == 0 {
                    if self.legacy_case() {
                        output.push_str(&format!(
                            "__nfContainerRef(this, {fn_name:?}, false, undefined, true).set({code});\n"
                        ));
                    } else {
                        output.push_str(&format!(
                            "this.{fn_name} = {};\n",
                            if self.as3 {
                                format!("__nfBindFunction({code}, __nfFrame)")
                            } else {
                                code
                            }
                        ));
                    }
                } else {
                    let canonical = self
                        .analysis
                        .resolve(self.scope, fn_name)
                        .map_or(fn_name, |(_, b)| b.canonical.as_str());
                    output.push_str(&format!("var {canonical} = {code};\n"));
                }
            }
        }
        output.push_str(&self.statements(body));
        output
    }
    pub(super) fn lvalue(&self, expr: &Expr) -> String {
        match expr {
            Expr::Name(name) => self.name(name),
            Expr::Member(base, name) => format!("({}).{name}", self.expression(base)),
            Expr::Index(base, index) => {
                format!("({})[{}]", self.expression(base), self.expression(index))
            }
            _ => unreachable!("The parser only accepts assignable expression targets"),
        }
    }
    pub(super) fn as2_var_scroll_ref(&self, base: &Expr, prop: &str) -> Option<String> {
        if self.as3 || !matches!(prop, "scroll" | "maxscroll") {
            return None;
        }
        let caller = self.container_tail();
        match base {
            Expr::Name(var_name)
                if !is_global(var_name)
                    && parse_as2_level(var_name).is_none()
                    && !is_as2_global_prop(var_name)
                    && self
                        .analysis
                        .resolve(self.scope, var_name)
                        .is_none_or(|(id, b)| {
                            id == 0
                                && (b.kind != scope::BindingKind::Field
                                    || !self.analysis.context.static_members.contains_key(var_name))
                        }) =>
            {
                let is_instance_field = self
                    .analysis
                    .resolve(self.scope, var_name)
                    .is_some_and(|(id, b)| id == 0 && b.kind == scope::BindingKind::Field);
                if is_instance_field {
                    Some(format!(
                        "__nfVarScrollRef({}, this, {var_name:?}, {prop:?}{caller})",
                        self.name(var_name)
                    ))
                } else if let Some(wref) = self.with_reference(var_name) {
                    Some(format!(
                        "((__nfRef)=>__nfVarScrollRef(__nfRef.get(),__nfRef.receiver,{var_name:?},{prop:?}{caller}))({wref})"
                    ))
                } else {
                    let read_expr = if self.legacy_case() {
                        format!(
                            "__nfFamilyPropertyValue({}, {var_name:?}, false, undefined, true)",
                            self.active_timeline()
                        )
                    } else {
                        self.unscoped_name(var_name)
                    };
                    Some(format!(
                        "__nfVarScrollRef({read_expr}, {}, {var_name:?}, {prop:?}{caller})",
                        self.active_timeline()
                    ))
                }
            }
            Expr::Member(owner_expr, var_name) => Some(format!(
                "((__nfOwner)=>__nfVarScrollRef(__nfFamilyPropertyValue(__nfOwner,{var_name:?},false{caller}),__nfOwner,{var_name:?},{prop:?}{caller}))({})",
                self.expression(owner_expr)
            )),
            _ => None,
        }
    }
    pub(super) fn reference(&self, expr: &Expr) -> Option<String> {
        if let Expr::Name(name) = expr
            && let Some(reference) = self.with_reference(name)
        {
            return Some(reference);
        }
        match expr {
            Expr::Name(name)
                if !self.as3
                    && self.analysis.resolve(self.scope, name).is_none()
                    && let Some(level) = parse_as2_level(name) =>
            {
                Some(format!("__nfLegacy.levelRef({level})"))
            }
            Expr::Name(name)
                if !self.as3
                    && self.analysis.resolve(self.scope, name).is_none()
                    && is_as2_global_prop(name) =>
            {
                Some(format!("__nfLegacy.globalPropRef({name:?})"))
            }
            Expr::Name(name) => self.bare_member_receiver(name).map(|receiver| {
                if let Some(uri) = self.open_namespace_uri_for_member(name) {
                    format!(
                        "__nfQualifiedRef({receiver}, {uri:?}, {name:?}, {}{})",
                        self.as3,
                        self.container_tail()
                    )
                } else {
                    format!(
                        "__nfContainerRef({receiver}, {name:?}, {}{})",
                        self.as3,
                        self.container_tail()
                    )
                }
            }),
            Expr::Member(base, name) => self.as2_var_scroll_ref(base, name).or_else(|| {
                Some(format!(
                    "__nfContainerRef({}, {name:?}, {}{})",
                    self.expression(base),
                    self.as3,
                    self.container_tail()
                ))
            }),
            Expr::XmlSelect {
                base,
                local,
                namespace,
                attribute,
                descendant,
            } => {
                if self.as3
                    && !*attribute
                    && !*descendant
                    && let Some(namespace) = namespace
                {
                    return Some(format!(
                        "__nfQualifiedRef({}, {}, {}, {}{})",
                        self.expression(base),
                        self.expression(namespace),
                        self.expression(local),
                        self.as3,
                        self.container_tail()
                    ));
                }
                let ns_expr = namespace
                    .as_ref()
                    .map_or_else(|| "undefined".into(), |ns| self.expression(ns));
                let local_expr = self.expression(local);
                let selector = format!(
                    "((__nfNamespace,__nfLocal)=>__nfXML.selector(__nfLocal,__nfNamespace,{attribute},{descendant}))({ns_expr}, {local_expr})"
                );
                Some(format!(
                    "__nfXmlOrProxySelectRef({}, ()=>{selector}, {ns_expr}, {local_expr}, {attribute}, {descendant}, {})",
                    self.expression(base),
                    self.as3
                ))
            }
            Expr::XmlWithAttr {
                local,
                namespace,
                attribute,
                descendant,
            } => {
                let ns_expr = namespace
                    .as_ref()
                    .map_or_else(|| "undefined".into(), |ns| self.expression(ns));
                let local_expr = self.expression(local);
                let selector = format!(
                    "((__nfNamespace,__nfLocal)=>__nfXML.selector(__nfLocal,__nfNamespace,{attribute},{descendant}))({ns_expr}, {local_expr})"
                );
                let targets = self
                    .with_targets
                    .iter()
                    .rev()
                    .map(|(_, t)| t.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                Some(format!(
                    "__nfWithXmlAttr([{targets}], this, {selector}, {ns_expr}, {local_expr}, {attribute}, {descendant}{})",
                    self.caller_argument()
                ))
            }
            Expr::Index(base, key) => Some(format!(
                "__nfContainerRef({}, {}, {}{})",
                self.expression(base),
                self.expression(key),
                self.as3,
                self.index_tail()
            )),
            _ => None,
        }
    }
    pub(super) fn binary(&self, op: &str, left: String, right: String) -> String {
        if !self.as3
            && matches!(
                op,
                "add" | "eq" | "ne" | "lt" | "le" | "gt" | "ge" | "and" | "or"
            )
        {
            if self.legacy_case() {
                format!("__nfLegacyStringOperator({op:?}, {left}, {right}, true)")
            } else {
                format!("__nfLegacyStringOperator({op:?}, {left}, {right})")
            }
        } else if !self.as3 && op == "<>" {
            format!("__nfLegacyStringOperator(\"unequal\", {left}, {right})")
        } else if self.as3 && matches!(op, "+" | "==" | "!=" | "===" | "!==") {
            format!("__nfXMLBinary({op:?}, {left}, {right})")
        } else if !self.as3
            && (matches!(op, "+" | "-" | "*" | "/" | "%")
                || (self.legacy_case() && matches!(op, "<" | "<=" | ">" | ">=")))
        {
            format!(
                "__nfLegacyBinary({op:?}, {left}, {right}, {})",
                self.legacy_case()
            )
        } else {
            format!("({left} {op} {right})")
        }
    }
    pub(super) fn switch_value(&self, expr: &Expr) -> String {
        let value = self.expression(expr);
        if self.as3 {
            format!("__nfXMLSwitch({value})")
        } else {
            value
        }
    }
    pub(super) fn statement(&self, stmt: &Stmt) -> String {
        match stmt {
            Stmt::Located(offset, inner) => {
                let code = self.statement(inner);
                if code.is_empty() { return code; }
                self.analysis.context.mapping_prefix.as_ref().map_or(code.clone(), |prefix| {
                    format!("{prefix}{offset}*/{code}")
                })
            }
            Stmt::Empty => ";".into(),
            Stmt::Expr(expr) => format!("{};", self.expression(expr)),
            Stmt::DefaultXmlNamespace(expr) => {
                format!("__nfXML.setDefaultNamespace({});", self.expression(expr))
            }
            Stmt::LegacyEvent(triggers, func) => {
                let clean = Emitter {
                    as3: false,
                    analysis: self.analysis,
                    scope: 0,
                    with_targets: Vec::new(),
                    tell_target: None,
                };
                let code = format!(
                    "(function() {{\nconst __nfFrame = this, _parent = this._parent;\n{}\n}})",
                    clean.body(&func.body)
                );
                let mut out = format!("{{\nconst __nfEventFn = {code};\n");
                for trigger in triggers {
                    let kind = if trigger.clip_event { "clip" } else { "button" };
                    let key = trigger
                        .key_press
                        .as_ref()
                        .map_or_else(|| "null".into(), |k| format!("{k:?}"));
                    out.push_str(&format!(
                        "__nfDisplay.addLegacyEvent(this, {kind:?}, {:?}, {key}, __nfEventFn);\n",
                        trigger.event
                    ));
                }
                out.push('}');
                out
            }
            Stmt::Var(variables) => format!(
                "{};",
                variables
                    .iter()
                    .map(|(name, ty, value)| {
                        let target = self.name(name);
                        if self.analysis.scopes[&self.scope].function != 0 && value.is_none() {
                            return "undefined".into();
                        }
                        let initialized = value.is_some();
                        let value = value
                            .as_ref()
                            .map(|v| self.coerce(name, self.expression(v)))
                            .unwrap_or_else(|| self.default_value(ty.as_deref()).into());
                        if let Some(reference) = self.with_reference(name) {
                            return if initialized {
                                format!("({reference}).set({value})")
                            } else {
                                "undefined".into()
                            };
                        }
                        if self.analysis.scopes[&self.scope].function == 0 && self.legacy_case() {
                            let cref = format!(
                                "__nfContainerRef({}, {name:?}, false, undefined, true)",
                                self.active_timeline()
                            );
                            return if initialized {
                                format!("{cref}.set({value})")
                            } else {
                                format!(
                                    "((__nfRef)=>__nfRef.get()===undefined?__nfRef.set({value}):__nfRef.get())({cref})"
                                )
                            };
                        }
                        if initialized {
                            format!("{target} = {value}")
                        } else {
                            format!("({target} === undefined ? ({target} = {value}) : {target})")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Stmt::With(target, body, id) => {
                let name = format!("__nfWith{id}");
                let mut targets = self.with_targets.clone();
                targets.push((self.scope, name.clone()));
                let inner = Emitter {
                    as3: self.as3,
                    analysis: self.analysis,
                    scope: self.scope,
                    with_targets: targets,
                    tell_target: self.tell_target.clone(),
                };
                format!(
                    "{{const {name}=__nfWithTarget({}, {});\n{}\n}}",
                    self.expression(target),
                    self.as3,
                    inner.statement(body)
                )
            }
            Stmt::TellTarget(target, body, id) => {
                let name = format!("__nfWith{id}");
                let mut targets = self.with_targets.clone();
                targets.push((self.scope, name.clone()));
                let inner = Emitter {
                    as3: self.as3,
                    analysis: self.analysis,
                    scope: self.scope,
                    with_targets: targets,
                    tell_target: Some(name.clone()),
                };
                format!(
                    "{{const {name}=__nfTellTarget({}, {}, __nfFrame{});\n{}\n}}",
                    self.active_timeline(),
                    self.expression(target),
                    if self.legacy_case() { ", true" } else { "" },
                    inner.statement(body)
                )
            }
            Stmt::IfFrameLoaded(args, body) => format!(
                "if (__nfLegacy.isFrameLoaded({}, [{}])) {{\n{}\n}}",
                self.active_timeline(),
                args.iter()
                    .map(|arg| self.expression(arg))
                    .collect::<Vec<_>>()
                    .join(", "),
                self.statement(body)
            ),
            Stmt::InitClip(_, body, id) => {
                let key = format!("{:016x}:{id}", fnv1a64(&self.analysis.script.source));
                let clean = Emitter {
                    as3: false,
                    analysis: self.analysis,
                    scope: 0,
                    with_targets: Vec::new(),
                    tell_target: None,
                };
                format!(
                    "__nfRunInitClip({key:?}, (__nfLegacy?.level?.(0) ?? (typeof _root !== 'undefined' ? _root : this)), function(__nfInitRoot){{ const __nfFrame = __nfInitRoot;\n(function(){{\n{}\n}}).call(__nfInitRoot);\n}});",
                    clean.body(body)
                )
            }
            Stmt::Block(body) => format!("{{\n{}\n}}", self.statements(body)),
            Stmt::If(condition, yes, no) => {
                let mut output = format!(
                    "if ({}) {{\n{}\n}}",
                    self.expression(condition),
                    self.statement(yes)
                );
                let mut cur = no.as_deref();
                while let Some(node) = cur {
                    if let Stmt::If(next_cond, next_yes, next_no) = node.unlocated() {
                        output.push_str(&format!(
                            " {}else if ({}) {{\n{}\n}}",
                            self.marker(node),
                            self.expression(next_cond),
                            self.statement(next_yes)
                        ));
                        cur = next_no.as_deref();
                    } else {
                        output.push_str(&format!(" else {{\n{}\n}}", self.statement(node)));
                        break;
                    }
                }
                output
            }
            Stmt::While(condition, body) => format!(
                "while ({}) {{ if ((++__nfLoopSteps & 63) === 0) __nfStepOverflow();\n{}\n}}",
                self.expression(condition),
                self.statement(body)
            ),
            Stmt::For(init, condition, update, body) => format!(
                "for ({} {}; {}) {{ if ((++__nfLoopSteps & 63) === 0) __nfStepOverflow();\n{}\n}}",
                self.statement(init),
                condition
                    .as_ref()
                    .map(|x| self.expression(x))
                    .unwrap_or_default(),
                update
                    .as_ref()
                    .map(|x| self.expression(x))
                    .unwrap_or_default(),
                self.statement(body)
            ),
            Stmt::ForIn(binding, collection, values, body) => {
                let (target, initial) = match binding.unlocated() {
                    Stmt::Var(vars) => {
                        (Expr::Name(vars[0].0.clone()), Some(self.statement(binding)))
                    }
                    Stmt::Expr(value) => (value.clone(), None),
                    _ => unreachable!("validated enumeration target"),
                };
                let assign = Expr::Assign(
                    "=".into(),
                    Box::new(target),
                    Box::new(Expr::Literal("__nfItem".into())),
                );
                let iterator = format!(
                    "__nfEnumerate({}, {values}, {}{})",
                    self.expression(collection),
                    self.as3,
                    if self.legacy_case() { ", true" } else { "" }
                );
                let iterator = initial.map_or(iterator.clone(), |initial| {
                    format!("({}, {iterator})", initial.trim_end_matches(';'))
                });
                format!(
                    "for (const __nfItem of {iterator}) {{ if ((++__nfLoopSteps & 63) === 0) __nfStepOverflow();\n{};\n{}\n}}",
                    self.expression(&assign),
                    self.statement(body)
                )
            }
            Stmt::Break(label) => format!(
                "break{};",
                label.as_ref().map_or(String::new(), |l| format!(" {l}"))
            ),
            Stmt::Continue(label) => format!(
                "continue{};",
                label.as_ref().map_or(String::new(), |l| format!(" {l}"))
            ),
            Stmt::Label(name, body) => format!("{name}: {}", self.statement(body)),
            Stmt::DoWhile(body, test) => format!(
                "do {{ if ((++__nfLoopSteps & 63) === 0) __nfStepOverflow();\n{}\n}} while ({});",
                self.statement(body),
                self.expression(test)
            ),
            Stmt::Switch(value, cases) => format!(
                "switch ({}) {{\n{}\n}}",
                self.switch_value(value),
                cases
                    .iter()
                    .map(|c| format!(
                        "{}:\n{}",
                        c.test.as_ref().map_or_else(
                            || "default".into(),
                            |e| format!("case {}", self.switch_value(e))
                        ),
                        self.statements(&c.body)
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            Stmt::Throw(value) => {
                if self.as3 {
                    format!("throw {};", self.expression(value))
                } else {
                    format!(
                        "throw {{ [__nfThrownBrand]: true, value: {} }};",
                        self.expression(value)
                    )
                }
            }
            Stmt::Try(body, catches, finally) => {
                let mut out = format!("try {{\n{}\n}}", self.statements(body));
                if !catches.is_empty() {
                    // Each nested catch owns its generated temporaries and lexical binding.
                    let id = catches[0].id;
                    let caught = format!("__nfCaught{id}");
                    let value = format!("__nfValue{id}");
                    out.push_str(&format!(" catch ({caught}) {{\n"));
                    out.push_str(&format!(
                        "const {value} = __nfUnwrapCaught({caught}, {});\n",
                        self.as3
                    ));
                    for (index, catch) in catches.iter().enumerate() {
                        let inner = Emitter {
                            as3: self.as3,
                            analysis: self.analysis,
                            scope: catch.id,
                            with_targets: self.with_targets.clone(),
                            tell_target: self.tell_target.clone(),
                        };
                        let condition = catch.ty.as_deref().filter(|t| *t != "*").map_or_else(
                            || "true".into(),
                            |ty| {
                                if self.as3
                                    && self.analysis.context.project
                                    && !is_error_type(ty)
                                    && !matches!(
                                        ty,
                                        "Object"
                                            | "Number"
                                            | "String"
                                            | "Boolean"
                                            | "Function"
                                            | "int"
                                            | "uint"
                                    )
                                    && let Some(qname) = self.analysis.context.types.get(ty)
                                {
                                    format!("__nfProgram.is({value}, {qname:?})")
                                } else {
                                    format!("__nfCatchType({value}, {ty:?})")
                                }
                            },
                        );
                        out.push_str(&format!(
                            "{}if ({condition}) {{ let {} = {value};\n{}\n}} ",
                            if index == 0 { "" } else { "else " },
                            catch.name,
                            inner.statements(&catch.body)
                        ));
                    }
                    out.push_str(&format!("else {{ throw {caught}; }}\n}}"));
                }
                if let Some(finally) = finally {
                    out.push_str(&format!(" finally {{\n{}\n}}", self.statements(finally)));
                }
                out
            }
            Stmt::Function(_) => String::new(),
            Stmt::Return(value, _) => value.as_ref().map_or_else(
                || "return;".into(),
                |value| {
                    format!(
                        "return {};",
                        self.coerce_type(
                            self.analysis.scopes[&self.scope].returns.as_deref(),
                            self.expression(value)
                        )
                    )
                },
            ),
        }
    }
    pub(super) fn statements(&self, statements: &[Stmt]) -> String {
        statements
            .iter()
            .map(|s| self.statement(s))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
