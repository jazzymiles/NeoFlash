//! JavaScript expression code generator for ActionScript.

use super::*;

impl Emitter<'_> {
    pub(super) fn expression(&self, expr: &Expr) -> String {
        fn qualified_name(expr: &Expr) -> Option<String> {
            match expr {
                Expr::Name(n) => Some(n.clone()),
                Expr::Member(base, n) => Some(format!("{}.{}", qualified_name(base)?, n)),
                _ => None,
            }
        }
        if self.analysis.context.project
            && let Some(name) = qualified_name(expr)
            && !matches!(name.as_str(), "Array" | "String")
            && !(!self.as3 && name == "Object")
            && !self.analysis.context.intrinsic_type(&name, self.as3)
            && self
                .analysis
                .resolve(self.scope, name.split('.').next().unwrap())
                .is_none()
        {
            if let Some(qname) = self
                .analysis
                .context
                .types
                .get(&name)
                .or_else(|| self.analysis.context.functions.get(&name))
            {
                if !self.as3 && qname == "flash.external.ExternalInterface" {
                    return "__nfLegacy.ExternalInterface".into();
                }
                return format!("__nfProgram.type({qname:?})");
            }
            if self.as3
                && let Some(uri) = self.analysis.context.namespaces.get(&name)
            {
                let short = name.rsplit('.').next().unwrap_or(&name);
                return format!("(__nfXML?.namespace?.({short:?}, {uri:?}) ?? {uri:?})");
            }
        }
        match expr {
            Expr::XmlLiteral(template, holes) => format!(
                "__nfXML.literal({template}, [{}])",
                holes
                    .iter()
                    .map(|hole| format!("()=>({})", self.expression(hole)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::XmlSelect { .. } => format!(
                "({}).get()",
                self.reference(expr).expect("XML selector ref")
            ),
            Expr::XmlWithAttr { .. } => format!(
                "({}).get()",
                self.reference(expr).expect("XML with attribute ref")
            ),
            Expr::XmlFilter(base, predicate, id) => {
                let item = format!("__nfXmlFilter{id}");
                let mut targets = self.with_targets.clone();
                targets.push((self.scope, item.clone()));
                let inner = Emitter {
                    as3: self.as3,
                    analysis: self.analysis,
                    scope: self.scope,
                    with_targets: targets,
                    tell_target: self.tell_target.clone(),
                };
                format!(
                    "__nfXML.filter({}, ({item}) => Boolean({}))",
                    self.expression(base),
                    inner.expression(predicate)
                )
            }
            Expr::InstanceOf(lhs, rhs) => format!(
                "__nfAs3InstanceOf({}, {})",
                self.expression(lhs),
                self.expression(rhs)
            ),
            Expr::TypeValue(ty, _) => self.vector_type(ty),
            Expr::NewVector(ty, args, _) => format!(
                "new ({})({})",
                self.vector_type(ty),
                args.iter()
                    .map(|a| self.expression(a))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::Delete(target, _) => {
                if let Some(reference) = self.reference(target) {
                    format!("({reference}).delete()")
                } else if !self.as3
                    && let Expr::Name(name) = target.as_ref()
                {
                    let timeline = self.active_timeline();
                    if self.unscoped_name(name) == format!("{timeline}.{name}") {
                        format!(
                            "__nfContainerRef({timeline}, {name:?}, false{}).delete()",
                            self.container_tail()
                        )
                    } else {
                        "false".into()
                    }
                } else {
                    panic!("delete property")
                }
            }
            Expr::Literal(value) => value.clone(),
            Expr::Name(name) if self.with_reference(name).is_some() => self.name(name),
            Expr::Name(name)
                if !self.as3
                    && name == "scrollRect"
                    && self
                        .analysis
                        .resolve(self.scope, name)
                        .is_none_or(|(scope, _)| scope == 0)
                    && !self.analysis.context.types.contains_key(name)
                    && !self.analysis.context.static_members.contains_key(name) =>
            {
                format!(
                    "__nfContainerRef({}, \"scrollRect\", false{}).get()",
                    if self.scope == 0 { "this" } else { "__nfFrame" },
                    self.index_tail()
                )
            }
            Expr::Name(name) => self.bare_member_receiver(name).map_or_else(
                || self.name(name),
                |receiver| {
                    if let Some(uri) = self.open_namespace_uri_for_member(name) {
                        format!(
                            "__nfQualifiedRef({receiver}, {uri:?}, {name:?}, {}{}).get()",
                            self.as3,
                            self.container_tail()
                        )
                    } else {
                        format!(
                            "__nfFamilyPropertyValue({receiver}, {name:?}, {}{})",
                            self.as3,
                            self.container_tail()
                        )
                    }
                },
            ),
            Expr::RegExpLiteral(pattern, flags) => {
                format!("__nfRegExpCreate(true, [{pattern}, {flags}])")
            }
            Expr::Function(f) => format!("({})", self.function(f)),
            Expr::NewError(name, args, _) => format!(
                "__nfMakeError({name:?}, {}, [{}])",
                self.as3,
                args.iter()
                    .map(|a| self.expression(a))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::NewClass(name, args, _) => format!(
                "__nfProgram.construct({name:?}, [{}])",
                args.iter()
                    .map(|a| self.expression(a))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::NewValue(name, args, _) => {
                if !self.as3 {
                    format!(
                        "__nfLegacyConstruct({}, [{}])",
                        if self.legacy_case() {
                            self.dotted_value_expr(name)
                        } else {
                            self.name(name)
                        },
                        args.iter()
                            .map(|a| self.expression(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                } else {
                    format!(
                        "__nfClassConstruct({}, [{}])",
                        self.name(name),
                        args.iter()
                            .map(|a| self.expression(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            Expr::NewBuiltin(name, args, _)
                if matches!(
                    name.as_str(),
                    "XML" | "XMLList" | "XMLNode" | "Namespace" | "QName"
                ) =>
            {
                format!(
                    "new (__nfXMLClass({name:?}, {}))({})",
                    self.as3,
                    args.iter()
                        .map(|a| self.expression(a))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Expr::NewBuiltin(name, args, _)
                if self.as3
                    && !self.analysis.context.project
                    && is_as3_wave7_constructible(name) =>
            {
                format!(
                    "new (__nfWave7Class({name:?}))({})",
                    args.iter()
                        .map(|arg| self.expression(arg))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Expr::NewBuiltin(name, args, _)
                if matches!(
                    name.as_str(),
                    "TextField.StyleSheet"
                        | "LoadVars"
                        | "Color"
                        | "TextFormat"
                        | "Sound"
                        | "MovieClipLoader"
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
                        | "flash.net.FileReference"
                        | "flash.net.FileReferenceList"
                ) =>
            {
                format!(
                    "new __nfLegacy.{name}({})",
                    args.iter()
                        .map(|arg| self.expression(arg))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Expr::NewBuiltin(name, args, _)
                if matches!(name.as_str(), "Number" | "Boolean" | "int" | "uint") =>
            {
                let values = args
                    .iter()
                    .map(|a| self.expression(a))
                    .collect::<Vec<_>>()
                    .join(", ");
                if !self.as3 && matches!(name.as_str(), "Number" | "Boolean") {
                    format!("__nfBoxCreate({name:?}, [{values}])")
                } else {
                    format!("__nfScalarCreate({name:?}, [{values}])")
                }
            }
            Expr::NewBuiltin(name, args, _) if name == "Object" => format!(
                "__nfObjectCreate({}, [{}], true)",
                self.as3,
                args.iter()
                    .map(|a| self.expression(a))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::NewBuiltin(name, args, _) => format!(
                "__nf{name}Create({}, [{}]{})",
                self.as3,
                args.iter()
                    .map(|a| self.expression(a))
                    .collect::<Vec<_>>()
                    .join(", "),
                if name == "String" { ", true" } else { "" }
            ),
            Expr::Member(base, name) => {
                if let Some(reference) = self.as2_var_scroll_ref(base, name) {
                    return format!("({reference}).get()");
                }
                format!(
                    "__nfFamilyPropertyValue({}, {name:?}, {}{})",
                    self.expression(base),
                    self.as3,
                    self.container_tail()
                )
            }
            Expr::Index(base, index) => {
                format!(
                    "__nfFamilyPropertyValue({}, {}, {}{})",
                    self.expression(base),
                    self.expression(index),
                    self.as3,
                    self.index_tail()
                )
            }
            Expr::Array(values) => format!(
                "[{}{}]",
                values
                    .iter()
                    .map(|v| v.as_ref().map_or_else(String::new, |v| self.expression(v)))
                    .collect::<Vec<_>>()
                    .join(", "),
                if values.last().is_some_and(Option::is_none) {
                    ","
                } else {
                    ""
                }
            ),
            Expr::Object(values) => format!(
                "({{{}}})",
                values
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.source, self.expression(v)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::Unary(op, value) if op == "not" => format!(
                "__nfLegacyStringOperator(\"not\", {}, undefined)",
                self.expression(value)
            ),
            Expr::Unary(op, value) => format!("({op} {})", self.expression(value)),
            Expr::Binary(op, left, right) if op == "in" => format!(
                "((__nfKey,__nfReceiver)=>__nfContainerRef(__nfReceiver,__nfKey,{}{}).has())({}, {})",
                self.as3,
                self.index_tail(),
                self.expression(left),
                self.expression(right)
            ),
            Expr::Binary(op, left, right) => {
                self.binary(op, self.expression(left), self.expression(right))
            }
            Expr::TypeTest(op, value, ty, _) => {
                let predicate = if self.analysis.context.core_type(ty) {
                    if ty == "Class" {
                        "__nfClassIs(__nfValue)".into()
                    } else if self.as3 && ty == "Function" {
                        "(typeof __nfValue === 'function' && __nfNumericClassName(__nfValue)===null)".into()
                    } else if ty == "Array" {
                        "__nfArrayIsArray(__nfValue)".into()
                    } else if !self.as3
                        && op == "instanceof"
                        && matches!(ty.as_str(), "Number" | "String" | "Boolean")
                    {
                        format!(
                            "Boolean(typeof __nfLegacy !== 'undefined' && __nfLegacy.boxing?.is?.(__nfValue) && __nfLegacy.boxing.kind(__nfValue) === {ty:?})"
                        )
                    } else {
                        format!("__nfCatchType(__nfValue, {ty:?})")
                    }
                } else if vector_element(ty).is_some() {
                    format!("{}.__nfIs(__nfValue)", self.vector_type(ty))
                } else if matches!(
                    ty.as_str(),
                    "XML" | "XMLList" | "XMLNode" | "Namespace" | "QName"
                ) && self.analysis.context.intrinsic_type(ty, self.as3)
                {
                    format!("__nfXMLClass({ty:?}, {}).__nfIs(__nfValue)", self.as3)
                } else if self.analysis.context.intrinsic_type(ty, self.as3) {
                    format!("__nf{ty}Is(__nfValue)")
                } else if !self.as3
                    && !self.analysis.context.types.contains_key(ty)
                    && is_as2_legacy_instanceof_type(ty)
                {
                    format!("Boolean(__nfLegacy.{ty}?.__nfIs?.(__nfValue))")
                } else if self.as3 && !self.analysis.context.project && is_as3_wave7_type(ty) {
                    format!("Boolean(__nfWave7Class({ty:?})?.__nfIs?.(__nfValue))")
                } else if self.legacy_case() && !self.analysis.context.types.contains_key(ty) {
                    format!(
                        "__nfLegacyInstanceOf(__nfValue, {})",
                        self.dotted_value_expr(ty)
                    )
                } else {
                    format!(
                        "__nfProgram.is(__nfValue, {:?})",
                        self.analysis.context.types[ty]
                    )
                };
                format!(
                    "((__nfValue) => {} )({})",
                    if op == "instanceof" {
                        format!(
                            "(__nfValue !== null && (typeof __nfValue === 'object' || typeof __nfValue === 'function') && ({predicate}))"
                        )
                    } else if op == "is" {
                        predicate
                    } else {
                        format!("{predicate} ? __nfValue : null")
                    },
                    self.expression(value)
                )
            }
            Expr::Assign(op, left, right) => {
                let target = if matches!(
                    left.as_ref(),
                    Expr::XmlSelect { .. } | Expr::XmlWithAttr { .. }
                ) {
                    String::new()
                } else {
                    self.lvalue(left)
                };
                let value = self.expression(right);
                if !self.as3 && op == "=" {
                    let callback = match left.as_ref() {
                        Expr::Name(name)
                            if self.with_targets.is_empty()
                                && self.analysis.resolve(self.scope, name).is_none() =>
                        {
                            canonical_legacy_callback(name, self.legacy_case())
                                .map(|canonical| ("__nfFrame".into(), canonical))
                        }
                        Expr::Member(base, name) => {
                            canonical_legacy_callback(name, self.legacy_case())
                                .map(|canonical| (self.expression(base), canonical))
                        }
                        _ => None,
                    };
                    if let Some((receiver, name)) = callback {
                        return format!(
                            "__nfDisplay.setLegacyCallback({receiver}, {name:?}, {value})"
                        );
                    }
                }
                if let Expr::Name(name) = left.as_ref()
                    && self.as3
                    && self.typed(name)
                {
                    if let Some(reference) = self.reference(left) {
                        if op == "=" {
                            return format!("({reference}).set({})", self.coerce(name, value));
                        }
                        let operator = &op[..op.len() - 1];
                        let next = if matches!(op.as_str(), "&&=" | "||=") {
                            format!(
                                "__nfOld {operator} __nfRef.set({})",
                                self.coerce(name, value)
                            )
                        } else {
                            format!(
                                "__nfRef.set({})",
                                self.coerce(name, self.binary(operator, "__nfOld".into(), value))
                            )
                        };
                        return format!(
                            "((__nfRef)=>{{const __nfOld=__nfRef.get();return {next};}})({reference})"
                        );
                    }
                    if op == "&&=" || op == "||=" {
                        return format!(
                            "({target} {} ({target} = {}))",
                            &op[..2],
                            self.coerce(name, value)
                        );
                    }
                    let value = if op == "=" {
                        value
                    } else {
                        self.binary(&op[..op.len() - 1], target.clone(), value)
                    };
                    return format!("({target} = {})", self.coerce(name, value));
                }
                if let Some(reference) = self.reference(left) {
                    if op == "=" {
                        return format!("({reference}).set({value})");
                    }
                    let operator = &op[..op.len() - 1];
                    let expression = if matches!(op.as_str(), "&&=" | "||=") {
                        format!("__nfOld {operator} __nfRef.set({value})")
                    } else {
                        format!(
                            "__nfRef.set({})",
                            self.binary(operator, "__nfOld".into(), value.clone())
                        )
                    };
                    let assignment = format!(
                        "((__nfRef)=>{{const __nfOld=__nfRef.get();return {expression};}})({reference})"
                    );
                    return if self.as3 && matches!(op.as_str(), "&&=" | "||=") {
                        format!("__nfFamilyValue({assignment},true)")
                    } else {
                        assignment
                    };
                }
                let assignment = if (self.as3 && op == "+=")
                    || (!self.as3 && matches!(op.as_str(), "+=" | "-=" | "*=" | "/=" | "%="))
                {
                    format!(
                        "({target} = {})",
                        self.binary(&op[..op.len() - 1], target.clone(), value)
                    )
                } else {
                    format!("({target} {op} {value})")
                };
                if self.as3 && matches!(op.as_str(), "&&=" | "||=") {
                    // A short-circuited indexed ||= may itself extract the old
                    // function value without passing through an ordinary read.
                    format!("__nfFamilyValue({assignment}, true)")
                } else {
                    assignment
                }
            }
            Expr::Update(op, value, prefix) => {
                if let Expr::Name(name) = value.as_ref()
                    && self.as3
                    && self.typed(name)
                    && let Some(reference) = self.reference(value)
                {
                    let sign = if op == "++" { "+" } else { "-" };
                    let next = self.coerce(name, format!("(__nfOld {sign} 1)"));
                    return format!(
                        "((__nfRef)=>{{const __nfOld=__nfNumber(__nfRef.get()),__nfNext=__nfRef.set({next});return {};}})({reference})",
                        if *prefix { "__nfNext" } else { "__nfOld" }
                    );
                }
                if let Some(reference) = self.reference(value) {
                    let sign = if op == "++" { "+" } else { "-" };
                    let num_fn = if self.as3 {
                        "__nfNumber"
                    } else if self.legacy_case() {
                        "__nfNumber1"
                    } else {
                        "__nfNumber2"
                    };
                    return format!(
                        "((__nfRef)=>{{const __nfOld={num_fn}(__nfRef.get()),__nfNext=__nfRef.set(__nfOld {sign} 1);return {};}})({reference})",
                        if *prefix { "__nfNext" } else { "__nfOld" }
                    );
                }
                let target = self.lvalue(value);
                if let Expr::Name(name) = value.as_ref()
                    && self.as3
                    && self.typed(name)
                {
                    let sign = if op == "++" { "+" } else { "-" };
                    let number = self.intrinsic("Number");
                    if *prefix {
                        let converted = self.coerce(name, format!("({number}({target}) {sign} 1)"));
                        return format!("({target} = {converted})");
                    }
                    let converted = self.coerce(name, format!("(__nfOld {sign} 1)"));
                    return format!(
                        "((__nfOld) => ({target} = {converted}, __nfOld))({number}({target}))"
                    );
                }
                if !self.as3 {
                    let sign = if op == "++" { "+" } else { "-" };
                    let num_fn = if self.legacy_case() {
                        "__nfNumber1"
                    } else {
                        "__nfNumber2"
                    };
                    if *prefix {
                        return format!("({target} = {num_fn}({target}) {sign} 1)");
                    }
                    return format!(
                        "((__nfOld) => ({target} = __nfOld {sign} 1, __nfOld))({num_fn}({target}))"
                    );
                }
                if *prefix {
                    format!("({op}{target})")
                } else {
                    format!("({target}{op})")
                }
            }
            Expr::Conditional(test, yes, no) => format!(
                "({} ? {} : {})",
                self.expression(test),
                self.expression(yes),
                self.expression(no)
            ),
            Expr::Call(callee, args, _) => {
                if !self.as3
                    && let Expr::Name(name) = callee.as_ref()
                    && name == "call"
                    && self.analysis.resolve(self.scope, name).is_none()
                {
                    return format!(
                        "__nfLegacy.callFrame({}, {})",
                        self.active_timeline(),
                        self.expression(&args[0])
                    );
                }
                if !self.as3
                    && let Expr::Name(name) = callee.as_ref()
                    && matches!(name.as_str(), "eval" | "set")
                    && self.analysis.resolve(self.scope, name).is_none()
                {
                    let tail = match (name == "set", self.legacy_case()) {
                        (true, true) => format!(", {}, true", self.expression(&args[1])),
                        (true, false) => format!(", {}", self.expression(&args[1])),
                        (false, true) => ", undefined, true".into(),
                        (false, false) => String::new(),
                    };
                    return format!(
                        "__nfLegacy.objects.resolve({}, {}, {}, {}{tail})",
                        self.active_timeline(),
                        self.expression(&args[0]),
                        self.as2_lexical_reference(),
                        name == "set"
                    );
                }
                if !self.as3
                    && let Expr::Name(name) = callee.as_ref()
                    && self.analysis.resolve(self.scope, name).is_none()
                    && matches!(name.as_str(), "getProperty" | "setProperty" | "targetPath")
                {
                    let rendered_args = args
                        .iter()
                        .enumerate()
                        .map(|(index, arg)| {
                            if index == 1
                                && matches!(name.as_str(), "getProperty" | "setProperty")
                                && let Expr::Name(prop) = arg
                                && self
                                    .analysis
                                    .resolve(self.scope, prop)
                                    .is_none_or(|(id, _)| id == 0)
                                && let Some(prop_index) =
                                    flash4_property_index(prop, self.legacy_case())
                            {
                                return prop_index.to_string();
                            }
                            self.expression(arg)
                        })
                        .map(|arg| format!(", {arg}"))
                        .collect::<String>();
                    return format!(
                        "__nfLegacy.{name}.call({}{rendered_args})",
                        self.legacy_receiver()
                    );
                }
                if !self.as3
                    && (self.with_targets.is_empty() || self.tell_target.is_some())
                    && let Expr::Name(name) = callee.as_ref()
                    && self.analysis.resolve(self.scope, name).is_none()
                    && !self.analysis.context.types.contains_key(name)
                    && (matches!(
                        name.as_str(),
                        "duplicateMovieClip"
                            | "removeMovieClip"
                            | "startDrag"
                            | "stopDrag"
                            | "loadVariables"
                            | "loadVariablesNum"
                    ) || is_as2_frame_global(name))
                    && match name.as_str() {
                        "duplicateMovieClip" => args.len() >= 3 || self.with_targets.is_empty(),
                        "removeMovieClip" | "unloadMovie" | "startDrag" => {
                            !args.is_empty() || self.with_targets.is_empty()
                        }
                        "loadVariables" | "loadMovie" => {
                            args.len() >= 2 || self.with_targets.is_empty()
                        }
                        _ => true,
                    }
                {
                    return format!(
                        "__nfLegacy.{name}.call({}{})",
                        self.legacy_receiver(),
                        args.iter()
                            .map(|arg| format!(", {}", self.expression(arg)))
                            .collect::<String>()
                    );
                }
                if let Expr::Name(name) = callee.as_ref()
                    && let Some(reference) = self.with_reference(name)
                {
                    return format!(
                        "__nfWithMethod({reference})({})",
                        args.iter()
                            .map(|arg| self.expression(arg))
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                }
                if !self.as3
                    && let Expr::Name(name) = callee.as_ref()
                    && matches!(name.as_str(), "loadVariables" | "loadVariablesNum")
                    && self.analysis.resolve(self.scope, name).is_none()
                {
                    return format!(
                        "__nfLegacy.{name}.call({}, {})",
                        self.legacy_receiver(),
                        args.iter()
                            .map(|arg| self.expression(arg))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                if !self.as3
                    && let Expr::Name(name) = callee.as_ref()
                    && is_as2_frame_global(name)
                    && self.analysis.resolve(self.scope, name).is_none()
                    && !self.analysis.context.types.contains_key(name)
                {
                    return format!(
                        "__nfLegacy.{name}.call({}{})",
                        self.legacy_receiver(),
                        args.iter()
                            .map(|arg| format!(", {}", self.expression(arg)))
                            .collect::<String>()
                    );
                }
                if let Some(name) = qualified_name(callee)
                    && self
                        .analysis
                        .resolve(self.scope, name.split('.').next().unwrap())
                        .is_none()
                    && (self.analysis.context.functions.contains_key(&name)
                        || (!self.as3
                            && matches!(
                                name.as_str(),
                                "getTimer"
                                    | "setInterval"
                                    | "setTimeout"
                                    | "clearInterval"
                                    | "clearTimeout"
                            )))
                {
                    return format!(
                        "({})({})",
                        self.expression(callee),
                        args.iter()
                            .map(|a| self.expression(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                if !self.as3
                    && args.len() == 1
                    && let Some(name) = qualified_expr_name(callee)
                    && self
                        .analysis
                        .resolve(self.scope, name.split('.').next().unwrap())
                        .is_none()
                    && !matches!(name.as_str(), "Array" | "String")
                    && !(matches!(name.as_str(), "Object" | "Number" | "Boolean")
                        && self.analysis.context.core_type(&name))
                    && !(name == "Date" && self.analysis.context.intrinsic_type(&name, false))
                    && !self.analysis.context.utility_function(&name, false)
                    && !self.analysis.context.numeric_conversion(&name, false)
                    && !self.analysis.context.functions.contains_key(&name)
                {
                    if matches!(name.as_str(), "XML" | "XMLNode")
                        && self.analysis.context.intrinsic_type(&name, false)
                    {
                        return format!(
                            "__nfLegacyCast(__nfXMLClass({name:?}, false), {})",
                            self.expression(&args[0])
                        );
                    }
                    if !self.analysis.context.intrinsic_type(&name, false)
                        && let Some(qname) = self.analysis.context.types.get(&name)
                    {
                        return format!(
                            "__nfProgram.as2Cast({qname:?}, {})",
                            self.expression(&args[0])
                        );
                    }
                    if is_as2_legacy_instanceof_type(&name) {
                        let ctor = if matches!(
                            name.as_str(),
                            "TextField.StyleSheet"
                                | "flash.net.FileReference"
                                | "flash.net.FileReferenceList"
                        ) {
                            format!("__nfLegacy.{name}")
                        } else {
                            format!("__nfLegacy?.{name}")
                        };
                        return format!("__nfLegacyCast({ctor}, {})", self.expression(&args[0]));
                    }
                }
                if let Expr::Name(name) = callee.as_ref()
                    && self.analysis.resolve(self.scope, name).is_none()
                {
                    if self.analysis.context.utility_function(name, self.as3) {
                        return format!(
                            "{}({})",
                            self.name(name),
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if name == "Object" && self.analysis.context.core_type(name) {
                        return format!(
                            "__nfObjectCreate({}, [{}], false)",
                            self.as3,
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if self.as3 && name == "Class" && self.analysis.context.core_type(name) {
                        return format!(
                            "__nfClassMetaclass({})",
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if self.as3 && name == "Function" && self.analysis.context.core_type(name) {
                        return format!(
                            "__nfFunctionClass({})",
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if self.as3 && is_error_type(name) && self.analysis.context.core_type(name) {
                        return format!(
                            "__nfMakeError({name:?}, true, [{}])",
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if self.analysis.context.numeric_conversion(name, self.as3) {
                        let value = args
                            .first()
                            .map_or_else(|| "0".into(), |value| self.expression(value));
                        return format!(
                            "(__nfNumber({value}) {} 0)",
                            if name == "int" { "|" } else { ">>>" }
                        );
                    }
                    if matches!(
                        name.as_str(),
                        "XML" | "XMLList" | "XMLNode" | "Namespace" | "QName"
                    ) && self.analysis.context.intrinsic_type(name, self.as3)
                    {
                        return format!(
                            "(__nfXMLClass({name:?}, {}))({})",
                            self.as3,
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if matches!(name.as_str(), "Date" | "RegExp")
                        && self.analysis.context.intrinsic_type(name, self.as3)
                    {
                        return format!(
                            "__nf{name}Class({})({})",
                            self.as3,
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if matches!(name.as_str(), "Array" | "String") {
                        return format!(
                            "__nf{name}Create({}, [{}]{})",
                            self.as3,
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", "),
                            if name == "String" { ", false" } else { "" }
                        );
                    }
                    if name == "super" {
                        if let Some(class) = &self.analysis.context.class_name {
                            return format!(
                                "__nfProgram.superConstruct({class:?}, __nfFrame, [{}])",
                                args.iter()
                                    .map(|a| self.expression(a))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            );
                        }
                        if !self.as3 {
                            return format!(
                                "__nfLegacySuperConstruct(this, [{}])",
                                args.iter()
                                    .map(|a| self.expression(a))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            );
                        }
                    }
                    if let Some(qname) = self.analysis.context.types.get(name) {
                        return format!(
                            "__nfProgram.cast({qname:?}, {})",
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if self.as3 && !self.analysis.context.project && is_as3_wave7_type(name) {
                        return format!(
                            "__nfWave7Cast({name:?}, {})",
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                }
                if let Expr::Name(name) = callee.as_ref()
                    && self.as3
                    && let Some(receiver) = self.bare_member_receiver(name)
                {
                    if let Some(uri) = self.open_namespace_uri_for_member(name) {
                        return format!(
                            "__nfQualifiedMethod({receiver}, {uri:?}, {name:?}, {}{})({})",
                            self.as3,
                            self.caller_argument(),
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    return format!(
                        "__nfContainerMethod({receiver}, {name:?}, {}{})({})",
                        self.as3,
                        self.caller_argument(),
                        args.iter()
                            .map(|a| self.expression(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                let member = match callee.as_ref() {
                    Expr::Member(base, name) => Some((base.as_ref(), name.clone())),
                    Expr::Index(base, index) => {
                        literal_property_name(index).map(|name| (base.as_ref(), name))
                    }
                    _ => None,
                };
                if !self.as3
                    && self.analysis.context.class_name.is_none()
                    && let Some((Expr::Name(base_name), method_name)) = &member
                    && base_name == "super"
                {
                    return format!(
                        "__nfLegacySuperMethod(this, {method_name:?}, {})({})",
                        self.legacy_case(),
                        args.iter()
                            .map(|arg| self.expression(arg))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                if let Some((base, name)) = &member
                    && (matches!(name.as_str(), "call" | "apply")
                        || (self.legacy_case()
                            && (name.eq_ignore_ascii_case("call")
                                || name.eq_ignore_ascii_case("apply"))))
                {
                    let canonical = if name.eq_ignore_ascii_case("call") {
                        "call"
                    } else {
                        "apply"
                    };
                    if self.analysis.context.class_name.is_some() {
                        let caller = if matches!(callee.as_ref(), Expr::Member(_, _)) {
                            self.caller_argument()
                        } else {
                            String::new()
                        };
                        return format!(
                            "__nfContainerMethod({}, {name:?}, {}{caller})({})",
                            self.expression(base),
                            self.as3,
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if self.legacy_case() {
                        return format!(
                            "((__nfRecv)=>__nfFunctionMethod(__nfRecv, {canonical:?}, false, __nfContainerRef(__nfRecv, {name:?}, false, undefined, true).get()))({})({})",
                            self.expression(base),
                            args.iter()
                                .map(|arg| self.expression(arg))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    return format!(
                        "__nfFunctionMethod({}, {name:?}, {})({})",
                        self.expression(base),
                        self.as3,
                        args.iter()
                            .map(|arg| self.expression(arg))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                if let Some((base, name)) = &member
                    && is_family_method_for(name, self.legacy_case())
                {
                    let tail = if matches!(callee.as_ref(), Expr::Member(_, _)) {
                        self.container_tail()
                    } else {
                        self.index_tail().to_owned()
                    };
                    return format!(
                        "__nfMemberMethod({}, {name:?}, {}{tail})({})",
                        self.expression(base),
                        self.as3,
                        args.iter()
                            .map(|arg| self.expression(arg))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                let as2_method = if self.as3 {
                    None
                } else {
                    match callee.as_ref() {
                        Expr::Name(name)
                            if self.analysis.resolve(self.scope, name).is_none()
                                && !matches!(
                                    name.as_str(),
                                    "getProperty"
                                        | "setProperty"
                                        | "targetPath"
                                        | "getVersion"
                                        | "updateAfterEvent"
                                        | "stopAllSounds"
                                        | "toggleHighQuality"
                                        | "showRedrawRegions"
                                        | "duplicateMovieClip"
                                        | "removeMovieClip"
                                        | "startDrag"
                                        | "stopDrag"
                                )
                                && (!is_global(name)
                                    || matches!(name.as_str(), "gotoAndPlay" | "gotoAndStop")) =>
                        {
                            Some((self.active_timeline().to_owned(), format!("{name:?}")))
                        }
                        Expr::Member(base, name) => {
                            Some((self.expression(base), format!("{name:?}")))
                        }
                        Expr::Index(base, index) => Some((
                            self.expression(base),
                            member.as_ref().map_or_else(
                                || self.expression(index),
                                |(_, name)| format!("{name:?}"),
                            ),
                        )),
                        _ => None,
                    }
                };
                let callable = as2_method.map_or_else(
                    || {
                        if !self.as3
                            && let Expr::Name(name) = callee.as_ref()
                            && let Some((owner, _)) = self.analysis.resolve(self.scope, name)
                        {
                            return format!(
                                "__nfAs2Value({}, {})",
                                self.expression(callee),
                                if owner == 0 {
                                    self.analysis
                                        .context
                                        .static_members
                                        .get(name)
                                        .map(|class| format!("__nfProgram.type({class:?})"))
                                        .unwrap_or_else(|| "__nfFrame".into())
                                } else {
                                    "undefined".into()
                                }
                            );
                        }
                        // A direct member call retains the receiver. Ordinary
                        // value reads use the native-method identity guard, but
                        // calling a helper result here would erase this binding.
                        if self.as3
                            && let Expr::XmlSelect {
                                base,
                                local,
                                namespace: Some(namespace),
                                attribute: false,
                                descendant: false,
                            } = callee.as_ref()
                        {
                            format!(
                                "__nfQualifiedMethod({}, {}, {}, true{})",
                                self.expression(base),
                                self.expression(namespace),
                                self.expression(local),
                                self.container_tail()
                            )
                        } else if self.as3
                            && let Expr::Member(base, name) = callee.as_ref()
                        {
                            format!(
                                "__nfContainerMethod({}, {name:?}, true{})",
                                self.expression(base),
                                self.caller_argument()
                            )
                        } else if self.as3
                            && let Some((base, name)) = &member
                        {
                            format!(
                                "__nfContainerMethod({}, {name:?}, true)",
                                self.expression(base)
                            )
                        } else {
                            self.expression(callee)
                        }
                    },
                    |(receiver, name_expr)| {
                        if self.legacy_case() {
                            format!("__nfAs2Method({receiver}, {name_expr}, true)")
                        } else {
                            format!("__nfAs2Method({receiver}, {name_expr})")
                        }
                    },
                );
                let arg_list = args
                    .iter()
                    .map(|v| self.expression(v))
                    .collect::<Vec<_>>()
                    .join(", ");
                if !self.as3
                    && let Expr::Member(base, method) = callee.as_ref()
                    && method == "setFocus"
                    && matches!(
                        base.as_ref(),
                        Expr::Name(sel)
                            if sel == "Selection"
                                && self.analysis.resolve(self.scope, "Selection").is_none()
                    )
                {
                    return format!(
                        "(typeof __nfLegacy?.Selection?.__nfSetFocusWithCaller === \"function\" ? __nfLegacy.Selection.__nfSetFocusWithCaller({}, [{arg_list}]) : {callable}({arg_list}))",
                        self.legacy_receiver()
                    );
                }
                format!("{callable}({arg_list})")
            }
        }
    }
}
