//! Interface/override contract validation, reflection metadata, and class JS emitter.

use super::*;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Signature {
    pub(super) params: Vec<String>,
    pub(super) optional: Vec<bool>,
    // This bounded source profile requires the same rest shape. Default values
    // themselves are not part of override/interface signature compatibility.
    pub(super) rest: bool,
    pub(super) returns: String,
}
#[derive(Clone)]
pub(super) struct MethodContract {
    pub(super) signature: Signature,
    pub(super) access: Access,
    pub(super) owner: String,
    pub(super) final_: bool,
}
pub(super) fn method_contract(class: &Class, method: &Method) -> MethodContract {
    let canonical = |ty: Option<&str>| class.context.canonical_type(ty.unwrap_or("*"));
    MethodContract {
        signature: Signature {
            params: method
                .function
                .params
                .iter()
                .map(|(_, ty)| canonical(ty.as_deref()))
                .collect(),
            optional: method
                .function
                .defaults
                .iter()
                .map(Option::is_some)
                .collect(),
            rest: method.function.rest.is_some(),
            returns: canonical(method.function.returns.as_deref()),
        },
        access: method.access,
        owner: class.header.name.clone(),
        final_: method.final_,
    }
}
pub(super) fn check_accessor_pairs(
    class: &Class,
    contracts: &BTreeMap<(String, MethodKind), MethodContract>,
    errors: &mut Vec<Diagnostic>,
) {
    let member_offset = |trait_name: &str, kind: MethodKind| {
        let local = trait_name
            .rsplit_once("::")
            .map_or(trait_name, |(_, local)| local);
        class
            .methods
            .iter()
            .find(|m| m.function.name.as_deref() == Some(local) && m.kind == kind)
            .map_or(class.header.decl_offset, |m| m.function.offset)
    };
    for ((name, kind), getter) in contracts {
        if *kind == MethodKind::Get
            && let Some(setter) = contracts.get(&(name.clone(), MethodKind::Set))
            && (getter.signature.returns != setter.signature.params[0]
                || getter.access != setter.access)
        {
            errors.push(class.header.diag(
                member_offset(name, *kind),
                format!(
                    "getter and setter '{name}' must use the same property type and access namespace"
                ),
            ));
        }
        if *kind != MethodKind::Method
            && contracts.contains_key(&(name.clone(), MethodKind::Method))
        {
            errors.push(class.header.diag(
                member_offset(name, *kind),
                format!("accessor '{name}' conflicts with a method"),
            ));
        }
    }
}

// Native proxy APIs retain priority over class-owned descriptors. Redeclaration
// would therefore create a field/method that its author could never read back.
// This bounded profile rejects those declarations instead of hiding them.
pub(super) fn native_member(base: &str, name: &str) -> bool {
    let value_member = match base {
        "flash.utils.Timer" => matches!(
            name,
            "addEventListener"
                | "removeEventListener"
                | "hasEventListener"
                | "willTrigger"
                | "dispatchEvent"
                | "delay"
                | "repeatCount"
                | "currentCount"
                | "running"
                | "start"
                | "stop"
                | "reset"
        ),
        "flash.geom.Point" => matches!(
            name,
            "x" | "y"
                | "length"
                | "clone"
                | "copyFrom"
                | "setTo"
                | "add"
                | "subtract"
                | "equals"
                | "offset"
                | "normalize"
                | "toString"
        ),
        "flash.geom.Rectangle" => matches!(
            name,
            "x" | "y"
                | "width"
                | "height"
                | "left"
                | "right"
                | "top"
                | "bottom"
                | "topLeft"
                | "bottomRight"
                | "size"
                | "clone"
                | "copyFrom"
                | "setTo"
                | "setEmpty"
                | "isEmpty"
                | "equals"
                | "contains"
                | "containsPoint"
                | "containsRect"
                | "intersection"
                | "intersects"
                | "union"
                | "offset"
                | "offsetPoint"
                | "inflate"
                | "inflatePoint"
                | "toString"
        ),
        "flash.geom.Matrix" => matches!(
            name,
            "a" | "b"
                | "c"
                | "d"
                | "tx"
                | "ty"
                | "clone"
                | "copyFrom"
                | "setTo"
                | "identity"
                | "concat"
                | "transformPoint"
                | "deltaTransformPoint"
                | "invert"
                | "rotate"
                | "scale"
                | "translate"
                | "createBox"
                | "createGradientBox"
                | "toString"
        ),
        "flash.text.Font" => matches!(name, "fontName" | "fontStyle" | "fontType" | "hasGlyphs"),
        "flash.html.HTMLHost" => matches!(name, "htmlLoader"),
        _ => false,
    };
    if value_member {
        return true;
    }
    if (base == "flash.events.EventDispatcher"
        || base.starts_with("flash.display.")
        || base == "flash.text.TextField")
        && matches!(
            name,
            "addEventListener"
                | "removeEventListener"
                | "hasEventListener"
                | "willTrigger"
                | "dispatchEvent"
        )
    {
        return true;
    }
    (base.starts_with("flash.display.") || base == "flash.text.TextField")
        && (is_display_method(name)
            || matches!(
                name,
                "x" | "y"
                    | "_x"
                    | "_y"
                    | "_rotation"
                    | "_alpha"
                    | "_xscale"
                    | "_yscale"
                    | "_visible"
                    | "_name"
                    | "_width"
                    | "_height"
                    | "_root"
                    | "_parent"
                    | "_currentframe"
                    | "_totalframes"
                    | "rotation"
                    | "alpha"
                    | "scaleX"
                    | "scaleY"
                    | "visible"
                    | "name"
                    | "width"
                    | "height"
                    | "numChildren"
                    | "graphics"
                    | "transform"
                    | "mask"
                    | "filters"
                    | "scrollRect"
                    | "loaderInfo"
                    | "mouseX"
                    | "mouseY"
                    | "_xmouse"
                    | "_ymouse"
                    | "mouseEnabled"
                    | "mouseChildren"
                    | "buttonMode"
                    | "useHandCursor"
                    | "enabled"
                    | "hitArea"
                    | "dropTarget"
                    | "_droptarget"
                    | "stage"
                    | "root"
                    | "parent"
                    | "currentFrame"
                    | "totalFrames"
                    | "currentLabel"
                    | "currentFrameLabel"
                    | "currentLabels"
                    | "framesLoaded"
                    | "isPlaying"
                    | "currentScene"
                    | "scenes"
                    | "play"
                    | "stop"
                    | "gotoAndPlay"
                    | "gotoAndStop"
                    | "nextFrame"
                    | "prevFrame"
                    | "nextScene"
                    | "prevScene"
                    | "trace"
                    | "toString"
                    | "toJSON"
            ))
}

pub(super) fn builtin_interface_contracts()
-> BTreeMap<String, BTreeMap<(String, MethodKind), MethodContract>> {
    let entry =
        |owner: &str, name: &str, kind: MethodKind, params: &[(&str, bool)], returns: &str| {
            (
                (name.to_owned(), kind),
                MethodContract {
                    signature: Signature {
                        params: params.iter().map(|(ty, _)| (*ty).to_owned()).collect(),
                        optional: params.iter().map(|(_, opt)| *opt).collect(),
                        rest: false,
                        returns: returns.to_owned(),
                    },
                    access: Access::Public,
                    owner: owner.to_owned(),
                    final_: false,
                },
            )
        };
    let ev = "flash.events.IEventDispatcher";
    let di = "flash.utils.IDataInput";
    let do_ = "flash.utils.IDataOutput";
    let fp = "flash.desktop.IFilePromise";
    let ime = "flash.text.ime.IIMEClient";
    BTreeMap::from([
        (
            ev.to_owned(),
            BTreeMap::from([
                entry(
                    ev,
                    "addEventListener",
                    MethodKind::Method,
                    &[
                        ("String", false),
                        ("Function", false),
                        ("Boolean", true),
                        ("int", true),
                        ("Boolean", true),
                    ],
                    "void",
                ),
                entry(
                    ev,
                    "removeEventListener",
                    MethodKind::Method,
                    &[("String", false), ("Function", false), ("Boolean", true)],
                    "void",
                ),
                entry(
                    ev,
                    "dispatchEvent",
                    MethodKind::Method,
                    &[("flash.events.Event", false)],
                    "Boolean",
                ),
                entry(
                    ev,
                    "hasEventListener",
                    MethodKind::Method,
                    &[("String", false)],
                    "Boolean",
                ),
                entry(
                    ev,
                    "willTrigger",
                    MethodKind::Method,
                    &[("String", false)],
                    "Boolean",
                ),
            ]),
        ),
        (
            di.to_owned(),
            BTreeMap::from([
                entry(di, "bytesAvailable", MethodKind::Get, &[], "uint"),
                entry(di, "endian", MethodKind::Get, &[], "String"),
                entry(di, "endian", MethodKind::Set, &[("String", false)], "void"),
                entry(di, "objectEncoding", MethodKind::Get, &[], "uint"),
                entry(
                    di,
                    "objectEncoding",
                    MethodKind::Set,
                    &[("uint", false)],
                    "void",
                ),
                entry(di, "readBoolean", MethodKind::Method, &[], "Boolean"),
                entry(di, "readByte", MethodKind::Method, &[], "int"),
                entry(
                    di,
                    "readBytes",
                    MethodKind::Method,
                    &[
                        ("flash.utils.ByteArray", false),
                        ("uint", true),
                        ("uint", true),
                    ],
                    "void",
                ),
                entry(di, "readDouble", MethodKind::Method, &[], "Number"),
                entry(di, "readFloat", MethodKind::Method, &[], "Number"),
                entry(di, "readInt", MethodKind::Method, &[], "int"),
                entry(
                    di,
                    "readMultiByte",
                    MethodKind::Method,
                    &[("uint", false), ("String", false)],
                    "String",
                ),
                entry(di, "readObject", MethodKind::Method, &[], "*"),
                entry(di, "readShort", MethodKind::Method, &[], "int"),
                entry(di, "readUnsignedByte", MethodKind::Method, &[], "uint"),
                entry(di, "readUnsignedInt", MethodKind::Method, &[], "uint"),
                entry(di, "readUnsignedShort", MethodKind::Method, &[], "uint"),
                entry(di, "readUTF", MethodKind::Method, &[], "String"),
                entry(
                    di,
                    "readUTFBytes",
                    MethodKind::Method,
                    &[("uint", false)],
                    "String",
                ),
            ]),
        ),
        (
            do_.to_owned(),
            BTreeMap::from([
                entry(do_, "endian", MethodKind::Get, &[], "String"),
                entry(do_, "endian", MethodKind::Set, &[("String", false)], "void"),
                entry(do_, "objectEncoding", MethodKind::Get, &[], "uint"),
                entry(
                    do_,
                    "objectEncoding",
                    MethodKind::Set,
                    &[("uint", false)],
                    "void",
                ),
                entry(
                    do_,
                    "writeBoolean",
                    MethodKind::Method,
                    &[("Boolean", false)],
                    "void",
                ),
                entry(
                    do_,
                    "writeByte",
                    MethodKind::Method,
                    &[("int", false)],
                    "void",
                ),
                entry(
                    do_,
                    "writeBytes",
                    MethodKind::Method,
                    &[
                        ("flash.utils.ByteArray", false),
                        ("uint", true),
                        ("uint", true),
                    ],
                    "void",
                ),
                entry(
                    do_,
                    "writeDouble",
                    MethodKind::Method,
                    &[("Number", false)],
                    "void",
                ),
                entry(
                    do_,
                    "writeFloat",
                    MethodKind::Method,
                    &[("Number", false)],
                    "void",
                ),
                entry(
                    do_,
                    "writeInt",
                    MethodKind::Method,
                    &[("int", false)],
                    "void",
                ),
                entry(
                    do_,
                    "writeMultiByte",
                    MethodKind::Method,
                    &[("String", false), ("String", false)],
                    "void",
                ),
                entry(
                    do_,
                    "writeObject",
                    MethodKind::Method,
                    &[("*", false)],
                    "void",
                ),
                entry(
                    do_,
                    "writeShort",
                    MethodKind::Method,
                    &[("int", false)],
                    "void",
                ),
                entry(
                    do_,
                    "writeUnsignedInt",
                    MethodKind::Method,
                    &[("uint", false)],
                    "void",
                ),
                entry(
                    do_,
                    "writeUTF",
                    MethodKind::Method,
                    &[("String", false)],
                    "void",
                ),
                entry(
                    do_,
                    "writeUTFBytes",
                    MethodKind::Method,
                    &[("String", false)],
                    "void",
                ),
            ]),
        ),
        ("flash.display.IBitmapDrawable".to_owned(), BTreeMap::new()),
        ("flash.display.IGraphicsData".to_owned(), BTreeMap::new()),
        ("flash.display.IGraphicsFill".to_owned(), BTreeMap::new()),
        ("flash.display.IGraphicsPath".to_owned(), BTreeMap::new()),
        ("flash.display.IGraphicsStroke".to_owned(), BTreeMap::new()),
        (
            fp.to_owned(),
            BTreeMap::from([
                entry(fp, "relativePath", MethodKind::Get, &[], "String"),
                entry(fp, "isAsync", MethodKind::Get, &[], "Boolean"),
                entry(
                    fp,
                    "open",
                    MethodKind::Method,
                    &[],
                    "flash.utils.IDataInput",
                ),
                entry(fp, "close", MethodKind::Method, &[], "void"),
                entry(
                    fp,
                    "reportError",
                    MethodKind::Method,
                    &[("flash.events.ErrorEvent", false)],
                    "void",
                ),
            ]),
        ),
        (
            ime.to_owned(),
            BTreeMap::from([
                entry(ime, "compositionEndIndex", MethodKind::Get, &[], "int"),
                entry(ime, "compositionStartIndex", MethodKind::Get, &[], "int"),
                entry(ime, "selectionActiveIndex", MethodKind::Get, &[], "int"),
                entry(ime, "selectionAnchorIndex", MethodKind::Get, &[], "int"),
                entry(ime, "verticalTextLayout", MethodKind::Get, &[], "Boolean"),
                entry(
                    ime,
                    "confirmComposition",
                    MethodKind::Method,
                    &[("String", true), ("Boolean", true)],
                    "void",
                ),
                entry(
                    ime,
                    "getTextBounds",
                    MethodKind::Method,
                    &[("int", false), ("int", false)],
                    "flash.geom.Rectangle",
                ),
                entry(
                    ime,
                    "getTextInRange",
                    MethodKind::Method,
                    &[("int", false), ("int", false)],
                    "String",
                ),
                entry(
                    ime,
                    "selectRange",
                    MethodKind::Method,
                    &[("int", false), ("int", false)],
                    "void",
                ),
                entry(
                    ime,
                    "updateComposition",
                    MethodKind::Method,
                    &[
                        ("String", false),
                        ("Vector.<flash.text.ime.CompositionAttributeRange>", false),
                        ("int", false),
                        ("int", false),
                    ],
                    "void",
                ),
            ]),
        ),
        (
            "flash.accessibility.ISearchableText".to_owned(),
            BTreeMap::from([entry(
                "flash.accessibility.ISearchableText",
                "searchText",
                MethodKind::Get,
                &[],
                "String",
            )]),
        ),
        (
            "flash.accessibility.ISimpleTextSelection".to_owned(),
            BTreeMap::from([
                entry(
                    "flash.accessibility.ISimpleTextSelection",
                    "selectionActiveIndex",
                    MethodKind::Get,
                    &[],
                    "int",
                ),
                entry(
                    "flash.accessibility.ISimpleTextSelection",
                    "selectionAnchorIndex",
                    MethodKind::Get,
                    &[],
                    "int",
                ),
            ]),
        ),
        (
            "flash.security.IURIDereferencer".to_owned(),
            BTreeMap::from([entry(
                "flash.security.IURIDereferencer",
                "dereference",
                MethodKind::Method,
                &[("String", false)],
                "flash.utils.IDataInput",
            )]),
        ),
        (
            "flash.utils.IExternalizable".to_owned(),
            BTreeMap::from([
                entry(
                    "flash.utils.IExternalizable",
                    "writeExternal",
                    MethodKind::Method,
                    &[(do_, false)],
                    "void",
                ),
                entry(
                    "flash.utils.IExternalizable",
                    "readExternal",
                    MethodKind::Method,
                    &[(di, false)],
                    "void",
                ),
            ]),
        ),
    ])
}

pub(super) fn reflection_metadata(c: &Class) -> serde_json::Value {
    use serde_json::json;
    let canonical = |ty: Option<&str>| c.context.canonical_type(ty.unwrap_or("*"));
    let parameters = |f: &Function| {
        f.params.iter().enumerate().map(|(index, (_, ty))| json!({
        "type": canonical(ty.as_deref()), "optional": f.defaults.get(index).is_some_and(Option::is_some)
    })).collect::<Vec<_>>()
    };
    let short = c.header.name.rsplit('.').next().unwrap();
    let mut constructor = Vec::new();
    let mut constructor_rest = false;
    let mut members = c.fields.iter().map(|field| json!({
        "name": field.name, "kind": if field.constant { "constant" } else { "variable" },
        "static": field.is_static, "access": field.access.name(), "uri": field.namespace, "type": canonical(field.ty.as_deref())
    })).collect::<Vec<_>>();
    for method in &c.methods {
        let function = &method.function;
        if function.name.as_deref() == Some(short) && method.kind == MethodKind::Method {
            constructor = parameters(function);
            constructor_rest = function.rest.is_some();
            continue;
        }
        let ty = if method.kind == MethodKind::Set {
            function.params.first().and_then(|(_, ty)| ty.as_deref())
        } else {
            function.returns.as_deref()
        };
        members.push(json!({
            "name": function.name, "kind": match method.kind { MethodKind::Method => "method", MethodKind::Get => "get", MethodKind::Set => "set" },
            "static": method.is_static, "access": method.access.name(), "uri": method.namespace, "type": canonical(ty),
            "parameters": parameters(function), "rest": function.rest.is_some()
        }));
    }
    json!({"name": c.header.name, "base": c.base, "public": c.header.public,
        "dynamic": c.header.dynamic, "final": c.header.final_, "interface": c.header.is_interface,
        "interfaces": c.interfaces, "members": members, "constructor": constructor, "constructor_rest": constructor_rest})
}

pub(super) fn emit_class(c: &Class) -> Result<String, Diagnostic> {
    if c.header.intrinsic {
        return Ok(format!(
            "__nfProgram.declareIntrinsic({:?});\n",
            c.header.name
        ));
    }
    let as3 = c.header.script.language == ScriptLanguage::ActionScript3;
    let mut fields = vec![];
    let mut methods = vec![];
    let mut accessors =
        BTreeMap::<(String, bool, Access, Option<String>), (Option<String>, Option<String>)>::new();
    let mut constructor = "null".to_owned();
    let short = c.header.name.rsplit('.').next().unwrap();
    let initializers: Vec<_> = c
        .fields
        .iter()
        .filter_map(|f| f.value.clone())
        .map(Stmt::Expr)
        .collect();
    let field_analysis =
        scope::Analysis::with_context(&c.header.script, &initializers, c.context.clone())?;
    let emitter = Emitter {
        as3,
        analysis: &field_analysis,
        scope: 0,
        with_targets: Vec::new(),
        tell_target: None,
    };
    for f in &c.fields {
        let coerce = emitter.coerce_type(f.ty.as_deref(), "value".into());
        let initial = f
            .value
            .as_ref()
            .map(|v| {
                format!(
                    "function(){{const __nfFrame=this;{}return {};}}",
                    c.context
                        .mapping_prefix
                        .as_ref()
                        .map(|prefix| format!("{prefix}{}*/", f.offset))
                        .unwrap_or_default(),
                    emitter.expression(v)
                )
            })
            .unwrap_or_else(|| "null".into());
        let ns_lit = f
            .namespace
            .as_ref()
            .map_or_else(|| "null".into(), |u| format!("{u:?}"));
        fields.push(format!("{{name:{:?},static:{},access:{:?},ns:{ns_lit},constant:{},defaultValue:{},coerce:function(value){{return {coerce};}},init:{initial}}}",f.name,f.is_static,f.access.name(),f.constant,emitter.default_value(f.ty.as_deref())));
    }
    for method in &c.methods {
        let f = &method.function;
        let name = f.name.as_ref().unwrap();
        let mut context = c.context.clone();
        context.open_namespaces = method.open_namespaces.clone();
        for f_decl in &c.fields {
            if let Some(ns) = &f_decl.namespace
                && context.open_namespaces.contains(ns)
            {
                context
                    .members
                    .entry(f_decl.name.clone())
                    .or_insert_with(|| f_decl.ty.clone());
                if f_decl.is_static {
                    context
                        .static_members
                        .entry(f_decl.name.clone())
                        .or_insert_with(|| c.header.name.clone());
                }
            }
        }
        for m_decl in &c.methods {
            if let Some(ns) = &m_decl.namespace
                && context.open_namespaces.contains(ns)
                && let Some(m_name) = &m_decl.function.name
            {
                context.members.entry(m_name.clone()).or_insert_with(|| {
                    if m_decl.kind == MethodKind::Method {
                        Some("Function".into())
                    } else {
                        None
                    }
                });
                if m_decl.is_static {
                    context
                        .static_members
                        .entry(m_name.clone())
                        .or_insert_with(|| c.header.name.clone());
                }
            }
        }
        for (uri, ns_member) in &c.context.namespace_members {
            if context.open_namespaces.contains(uri) {
                context.members.entry(ns_member.clone()).or_insert(None);
            }
        }
        context.method_id = Some(f.id);
        context.static_method = method.is_static;
        let mut f = f.clone();
        f.name = Some(format!("__nfMethodBody{}", f.id));
        let is_ctor = name == short && method.kind == MethodKind::Method;
        if is_ctor {
            if method.is_static || method.override_ || f.returns.is_some() {
                return Err(Diagnostic::at(
                    &c.header.script,
                    f.offset,
                    "A constructor cannot be static, override or have a return annotation.",
                ));
            }
            let super_stmt_idx = f.body.iter().position(stmt_contains_super_call);
            let init_fields_idx = if let Some(idx) = super_stmt_idx {
                idx + 1
            } else {
                f.body.insert(
                    0,
                    Stmt::Expr(Expr::Call(
                        Box::new(Expr::Name("super".into())),
                        vec![],
                        f.offset,
                    )),
                );
                1
            };
            // Field initializers are executed after the selected base constructor.
            f.body.insert(
                init_fields_idx,
                Stmt::Expr(Expr::Call(
                    Box::new(Expr::Member(
                        Box::new(Expr::Name("__nfProgram".into())),
                        "initializeFields".into(),
                    )),
                    vec![
                        Expr::Literal(format!("{:?}", c.header.name)),
                        Expr::Name("this".into()),
                    ],
                    f.offset,
                )),
            );
        }
        let body = vec![Stmt::Function(Box::new(f.clone()))];
        let analysis = scope::Analysis::with_context(&c.header.script, &body, context)?;
        let emitter = Emitter {
            as3,
            analysis: &analysis,
            scope: 0,
            with_targets: Vec::new(),
            tell_target: None,
        };
        let function = emitter.function(&f);
        if is_ctor {
            constructor = function;
        } else if method.kind == MethodKind::Method {
            let ns_lit = method
                .namespace
                .as_ref()
                .map_or_else(|| "null".into(), |u| format!("{u:?}"));
            methods.push(format!(
                "{{name:{name:?},static:{},access:{:?},ns:{ns_lit},final:{},fn:{function}}}",
                method.is_static,
                method.access.name(),
                method.final_
            ));
        } else {
            let pair = accessors
                .entry((
                    name.clone(),
                    method.is_static,
                    method.access,
                    method.namespace.clone(),
                ))
                .or_default();
            if method.kind == MethodKind::Get {
                pair.0 = Some(function);
            } else {
                pair.1 = Some(function);
            }
        }
    }
    let accessors = accessors
        .into_iter()
        .map(|((name, is_static, access, ns), (get, set))| {
            let ns_lit = ns
                .as_ref()
                .map_or_else(|| "null".into(), |u| format!("{u:?}"));
            format!(
                "{{name:{name:?},static:{is_static},access:{:?},ns:{ns_lit},get:{},set:{}}}",
                access.name(),
                get.unwrap_or_else(|| "null".into()),
                set.unwrap_or_else(|| "null".into())
            )
        })
        .collect::<Vec<_>>();
    Ok(format!(
        "__nfProgram.define({:?},{},[{}],[{}],{},{:?},[{}],{:?},{{dynamic:{},final:{},package:{:?},openNamespaces:{:?},reflection:{}}});\n",
        c.header.name,
        c.base
            .as_ref()
            .map(|s| format!("{s:?}"))
            .unwrap_or_else(|| "null".into()),
        fields.join(","),
        methods.join(","),
        constructor,
        if as3 { "as3" } else { "as2" },
        accessors.join(","),
        c.interfaces,
        c.header.dynamic,
        c.header.final_,
        c.header.package,
        c.context.open_namespaces,
        reflection_metadata(c)
    ))
}

fn expr_contains_super_call(expr: &Expr) -> bool {
    if matches!(expr, Expr::Function(_)) {
        return false;
    }
    if let Expr::Call(callee, _, _) = expr
        && matches!(callee.as_ref(), Expr::Name(n) if n == "super")
    {
        return true;
    }
    scope::children(expr)
        .into_iter()
        .any(expr_contains_super_call)
}

fn stmt_contains_super_call(stmt: &Stmt) -> bool {
    match stmt.unlocated() {
        Stmt::Function(_) | Stmt::LegacyEvent(_, _) => false,
        Stmt::Try(body, catches, finally) => {
            body.iter().any(stmt_contains_super_call)
                || catches
                    .iter()
                    .any(|c| c.body.iter().any(stmt_contains_super_call))
                || finally
                    .as_ref()
                    .is_some_and(|f| f.iter().any(stmt_contains_super_call))
        }
        Stmt::Switch(e, cases) => {
            expr_contains_super_call(e)
                || cases.iter().any(|c| {
                    c.test.as_ref().is_some_and(expr_contains_super_call)
                        || c.body.iter().any(stmt_contains_super_call)
                })
        }
        other => {
            scope::expressions(other)
                .into_iter()
                .any(expr_contains_super_call)
                || scope::statements(other)
                    .into_iter()
                    .any(stmt_contains_super_call)
        }
    }
}
