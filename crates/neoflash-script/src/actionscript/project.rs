//! Project source units and a deliberately bounded AS2/AS3 class linker.
//! No filesystem/network access: every dependency comes from the Project snapshot.
use super::*;
use neoflash_core::Project;
use std::collections::{BTreeMap, BTreeSet};

const BUILTINS: &[&str] = &[
    "Object",
    "Array",
    "flash.display.DisplayObject",
    "flash.display.DisplayObjectContainer",
    "flash.display.MovieClip",
    "flash.display.Sprite",
    "flash.display.Stage",
    "flash.text.TextField",
    "flash.geom.Point",
    "flash.geom.Rectangle",
    "flash.geom.Matrix",
    "flash.utils.Timer",
    "flash.events.TimerEvent",
    "flash.events.EventDispatcher",
    "flash.events.Event",
    "flash.events.MouseEvent",
    "flash.events.KeyboardEvent",
    "flash.events.EventPhase",
];
const BUILTIN_FUNCTIONS: &[&str] = &["flash.utils.getTimer"];
fn is_display_method(name: &str) -> bool {
    matches!(
        name,
        "addEventListener"
            | "removeEventListener"
            | "hasEventListener"
            | "willTrigger"
            | "dispatchEvent"
            | "getBounds"
            | "getRect"
            | "localToGlobal"
            | "globalToLocal"
            | "getChildAt"
            | "getChildByName"
            | "getChildIndex"
            | "contains"
            | "stopPropagation"
            | "stopImmediatePropagation"
            | "preventDefault"
            | "isDefaultPrevented"
    )
}

pub(super) fn is_host_method(name: &str) -> bool {
    is_display_method(name)
        || matches!(
            name,
            "clone"
                | "copyFrom"
                | "setTo"
                | "add"
                | "subtract"
                | "equals"
                | "offset"
                | "normalize"
                | "containsPoint"
                | "containsRect"
                | "intersection"
                | "intersects"
                | "union"
                | "offsetPoint"
                | "inflate"
                | "inflatePoint"
                | "isEmpty"
                | "setEmpty"
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
                | "start"
                | "reset"
                | "updateAfterEvent"
        )
}

/// `code` is a function body taking __nfBuiltins and returning the program API.
pub struct ProjectProgram {
    pub code: String,
    pub class_names: Vec<String>,
    names: BTreeSet<String>,
    languages: BTreeMap<String, ScriptLanguage>,
}

struct Header {
    script: Script,
    path: String,
    package: String,
    name: String,
    base: Option<String>,
    imports: Vec<String>,
    tokens: Vec<Token>,
    body: usize,
    packaged: bool,
}
struct Field {
    name: String,
    ty: Option<String>,
    value: Option<Expr>,
    constant: bool,
    is_static: bool,
}
struct Method {
    function: Function,
    is_static: bool,
    override_: bool,
}
struct Class {
    header: Header,
    context: CompileContext,
    base: Option<String>,
    fields: Vec<Field>,
    methods: Vec<Method>,
}

fn visible_names(
    names: &BTreeSet<String>,
    languages: &BTreeMap<String, ScriptLanguage>,
    language: ScriptLanguage,
) -> BTreeSet<String> {
    names
        .iter()
        .filter(|name| {
            languages.get(*name).map_or_else(
                || {
                    language == ScriptLanguage::ActionScript3
                        || matches!(name.as_str(), "Object" | "Array")
                        || name.starts_with("flash.geom.")
                },
                |owner| *owner == language,
            )
        })
        .cloned()
        .collect()
}

fn scalar_initializer(value: &Expr) -> bool {
    match value {
        Expr::Literal(_) => true,
        Expr::Name(name) => matches!(
            name.as_str(),
            "true" | "false" | "null" | "undefined" | "NaN" | "Infinity"
        ),
        Expr::Unary(op, value) if matches!(op.as_str(), "+" | "-") => scalar_initializer(value),
        _ => false,
    }
}

fn frontend(script: &Script, tokens: Vec<Token>, context: CompileContext) -> Frontend<'_> {
    Frontend {
        script,
        tokens,
        pos: 0,
        depth: 0,
        controls: vec![],
        function_depth: 0,
        next_function: 1,
        context,
    }
}
fn qualified(p: &mut Frontend<'_>) -> Result<String, Diagnostic> {
    let mut name = p.ident()?;
    while p.eat(".") {
        name.push('.');
        name.push_str(&p.ident()?);
    }
    Ok(name)
}
fn imports(p: &mut Frontend<'_>) -> Result<Vec<String>, Diagnostic> {
    let mut imports = vec![];
    while p.eat("import") {
        imports.push(qualified(p)?);
        p.expect(";")?;
    }
    Ok(imports)
}
fn header(source: &neoflash_core::SourceUnit) -> Result<Header, Diagnostic> {
    let script = Script {
        language: source.language,
        source: source.source.clone(),
    };
    if !matches!(
        script.language,
        ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript3
    ) {
        return Err(Diagnostic::at(
            &script,
            0,
            "Project source units require AS2 or AS3 classes; JS/TS modules are not implemented yet.",
        ));
    }
    if script.source.len() > 1_048_576 {
        return Err(Diagnostic::at(
            &script,
            0,
            "A source unit is limited to 1 MiB.",
        ));
    }
    let tokens = Lexer {
        script: &script,
        offset: 0,
    }
    .tokens()?;
    let mut p = frontend(&script, tokens, CompileContext::default());
    let as2 = script.language == ScriptLanguage::ActionScript2;
    if as2 && p.is("package") {
        return Err(p.error("AS2 uses a qualified class name, not an AS3 package block."));
    }
    let packaged = p.eat("package");
    let mut package = if packaged {
        let name = if p.is("{") {
            String::new()
        } else {
            qualified(&mut p)?
        };
        p.expect("{")?;
        name
    } else {
        String::new()
    };
    let imports = imports(&mut p)?;
    p.eat("public");
    if as2 {
        p.eat("dynamic");
    }
    if p.is("final") || p.is("dynamic") || p.is("internal") {
        return Err(p.error(
            "Class modifiers final/dynamic/internal are outside the first public-class profile.",
        ));
    }
    p.expect("class")?;
    let short = if as2 { qualified(&mut p)? } else { p.ident()? };
    if invalid_binding(short.rsplit('.').next().unwrap(), false) {
        return Err(p.error("Invalid or reserved class name."));
    }
    let name = if package.is_empty() {
        short
    } else {
        format!("{package}.{short}")
    };
    if as2 {
        package = name
            .rsplit_once('.')
            .map_or("", |(owner, _)| owner)
            .to_owned();
        if source.path != format!("{}.as", name.replace('.', "/")) {
            return Err(p.error("AS2 source path must match its qualified class name (app/Widget.as for app.Widget)."));
        }
    }
    let base = if p.eat("extends") {
        Some(qualified(&mut p)?)
    } else {
        None
    };
    p.expect("{")?;
    let body = p.pos;
    let tokens = p.tokens;
    Ok(Header {
        script,
        path: source.path.clone(),
        package,
        name,
        base,
        imports,
        tokens,
        body,
        packaged,
    })
}
fn context(
    names: &BTreeSet<String>,
    package: &str,
    imports: &[String],
    language: ScriptLanguage,
) -> Result<CompileContext, String> {
    let mut c = CompileContext {
        project: true,
        ..Default::default()
    };
    for name in names {
        if BUILTIN_FUNCTIONS.contains(&name.as_str()) {
            c.functions.insert(name.clone(), name.clone());
        } else {
            c.types.insert(name.clone(), name.clone());
        }
    }
    if language == ScriptLanguage::ActionScript2 {
        c.functions
            .insert("getTimer".into(), "flash.utils.getTimer".into());
        c.types
            .insert("MovieClip".into(), "flash.display.MovieClip".into());
        c.types
            .insert("TextField".into(), "flash.text.TextField".into());
    }
    for name in ["Object", "Array"] {
        c.types.insert(name.into(), name.into());
    }
    for name in names {
        let (owner, short) = name.rsplit_once('.').unwrap_or(("", name));
        if owner == package {
            if BUILTIN_FUNCTIONS.contains(&name.as_str()) {
                c.functions.insert(short.into(), name.clone());
            } else {
                c.types.insert(short.into(), name.clone());
            }
        }
    }
    for name in imports {
        if !names.contains(name) {
            return Err(format!(
                "Unresolved import '{name}'. Add its source unit or use a supported built-in type."
            ));
        }
        let short = name.rsplit('.').next().unwrap();
        if c.types
            .get(short)
            .or_else(|| c.functions.get(short))
            .is_some_and(|old| old != name)
        {
            return Err(format!("Ambiguous imported name '{short}'."));
        }
        if BUILTIN_FUNCTIONS.contains(&name.as_str()) {
            c.functions.insert(short.into(), name.clone());
        } else {
            c.types.insert(short.into(), name.clone());
        }
    }
    Ok(c)
}
fn parse_class(header: Header, names: &BTreeSet<String>) -> Result<Class, Diagnostic> {
    let mut c = context(
        names,
        &header.package,
        &header.imports,
        header.script.language,
    )
    .map_err(|m| Diagnostic::at(&header.script, 0, m))?;
    c.class_name = Some(header.name.clone());
    let base = header
        .base
        .as_ref()
        .map(|b| {
            c.types.get(b).cloned().ok_or_else(|| {
                Diagnostic::at(&header.script, 0, format!("Unknown base class '{b}'."))
            })
        })
        .transpose()?;
    let as2 = header.script.language == ScriptLanguage::ActionScript2;
    let mut p = frontend(&header.script, header.tokens.clone(), c.clone());
    p.pos = header.body;
    let mut fields = vec![];
    let mut methods = vec![];
    let mut seen = BTreeSet::new();
    while !p.is("}") && p.token().kind != Kind::End {
        if p.eat(";") {
            continue;
        }
        let mut is_static = false;
        let mut override_ = false;
        loop {
            if p.eat("public") {
            } else if p.eat("static") {
                is_static = true;
            } else if p.eat("override") {
                if as2 {
                    return Err(
                        p.error("AS2 overrides methods by name without the AS3 override keyword.")
                    );
                }
                override_ = true;
            } else {
                break;
            }
        }
        if p.is("private") || p.is("protected") || p.is("internal") {
            return Err(p.error("This class profile supports public members; private/protected/internal access control is not yet implemented."));
        }
        if p.is("var") || p.is("const") {
            let constant = p.eat("const");
            if as2 && constant {
                return Err(p.error(
                    "AS2 class const fields are not supported; use a static var or read-only API.",
                ));
            }
            if !constant {
                p.expect("var")?;
            }
            if override_ {
                return Err(p.error("Fields cannot use override."));
            }
            let Stmt::Var(vars) = p.variables()? else {
                unreachable!()
            };
            p.semicolon()?;
            for (name, ty, value) in vars {
                if !seen.insert(name.clone()) {
                    return Err(p.error(format!("Duplicate class member '{name}'.")));
                }
                if constant && value.is_none() {
                    return Err(
                        p.error("Class const fields require an initializer in this profile.")
                    );
                }
                if as2
                    && value
                        .as_ref()
                        .is_some_and(|value| !scalar_initializer(value))
                {
                    return Err(p.error("AS2 inline field initializers currently require scalar constants. Initialize arrays, objects and computed values in the constructor."));
                }
                fields.push(Field {
                    name,
                    ty,
                    value,
                    constant,
                    is_static,
                });
            }
        } else if p.is("function") {
            let f = p.function(true)?;
            let name = f.name.as_ref().unwrap();
            if !seen.insert(name.clone()) {
                return Err(p.error(format!("Duplicate class member '{name}'.")));
            }
            methods.push(Method {
                function: f,
                is_static,
                override_,
            });
        } else {
            return Err(p.error("Expected a public var/const field or function; accessors, interfaces and namespaces are not yet supported."));
        }
    }
    p.expect("}")?;
    if header.packaged {
        p.expect("}")?;
    }
    if p.token().kind != Kind::End {
        return Err(p.error("A source unit must contain exactly one class."));
    }
    Ok(Class {
        header,
        context: c,
        base,
        fields,
        methods,
    })
}
fn prefix(mut e: Diagnostic, path: &str) -> Diagnostic {
    e.message = format!("{path}: {}", e.message);
    e
}

// Native proxy APIs retain priority over class-owned descriptors. Redeclaration
// would therefore create a field/method that its author could never read back.
// This bounded profile rejects those declarations instead of hiding them.
fn native_member(base: &str, name: &str) -> bool {
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
                    | "stage"
                    | "root"
                    | "parent"
                    | "currentFrame"
                    | "totalFrames"
                    | "play"
                    | "stop"
                    | "gotoAndPlay"
                    | "gotoAndStop"
                    | "nextFrame"
                    | "prevFrame"
                    | "trace"
                    | "toString"
                    | "toJSON"
            ))
}

pub fn compile_project(project: &Project) -> Result<ProjectProgram, Vec<Diagnostic>> {
    let mut headers = vec![];
    let mut errors = vec![];
    let mut names: BTreeSet<String> = BUILTINS
        .iter()
        .chain(BUILTIN_FUNCTIONS)
        .map(|s| (*s).into())
        .collect();
    for source in &project.sources {
        match header(source) {
            Ok(h) => {
                if !names.insert(h.name.clone()) {
                    errors.push(Diagnostic::at(
                        &h.script,
                        0,
                        format!("{}: duplicate class {}", h.path, h.name),
                    ));
                }
                headers.push(h);
            }
            Err(e) => errors.push(prefix(e, &source.path)),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let languages: BTreeMap<_, _> = headers
        .iter()
        .map(|h| (h.name.clone(), h.script.language))
        .collect();
    let mut classes = BTreeMap::new();
    for h in headers {
        let path = h.path.clone();
        let visible = visible_names(&names, &languages, h.script.language);
        match parse_class(h, &visible) {
            Ok(c) => {
                classes.insert(c.header.name.clone(), c);
            }
            Err(e) => errors.push(prefix(e, &path)),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut order = vec![];
    let mut pending: BTreeSet<_> = classes.keys().cloned().collect();
    while !pending.is_empty() {
        let before = pending.len();
        for name in pending.clone() {
            if classes[&name]
                .base
                .as_ref()
                .is_none_or(|b| !pending.contains(b))
            {
                pending.remove(&name);
                order.push(name);
            }
        }
        if pending.len() == before {
            let c = &classes[pending.first().unwrap()];
            return Err(vec![Diagnostic::at(
                &c.header.script,
                0,
                format!("{}: inheritance cycle", c.header.path),
            )]);
        }
    }
    // Resolve inherited declarations before compiling method bodies.
    let mut resolved = BTreeMap::<String, BTreeMap<String, Option<String>>>::new();
    let mut static_members = BTreeMap::<String, BTreeMap<String, String>>::new();
    let mut native_bases = BTreeMap::<String, String>::new();
    let mut registrations = String::new();
    for name in &order {
        let c = classes.get_mut(name).unwrap();
        let native_base = c
            .base
            .as_ref()
            .map(|base| native_bases.get(base).unwrap_or(base).clone());
        if let Some(base) = &native_base {
            native_bases.insert(name.clone(), base.clone());
            for member in c
                .fields
                .iter()
                .filter(|f| !f.is_static)
                .map(|f| f.name.as_str())
                .chain(
                    c.methods
                        .iter()
                        .filter(|m| !m.is_static)
                        .filter_map(|m| m.function.name.as_deref()),
                )
            {
                if native_member(base, member) {
                    errors.push(Diagnostic::at(&c.header.script, 0, format!("{}: redeclaring native member '{member}' is not supported by this class profile", c.header.path)));
                }
            }
        }
        let mut members = c
            .base
            .as_ref()
            .and_then(|b| resolved.get(b))
            .cloned()
            .unwrap_or_default();
        for f in &c.fields {
            if members.contains_key(&f.name) {
                errors.push(Diagnostic::at(
                    &c.header.script,
                    0,
                    format!(
                        "{}: field '{}' redeclares an inherited member",
                        c.header.path, f.name
                    ),
                ));
            }
            members.insert(f.name.clone(), f.ty.clone());
        }
        let short = c.header.name.rsplit('.').next().unwrap();
        for m in &c.methods {
            let n = m.function.name.as_ref().unwrap();
            if n != short {
                if m.override_ && !members.contains_key(n) {
                    errors.push(Diagnostic::at(
                        &c.header.script,
                        m.function.offset,
                        format!(
                            "{}: override target '{n}' not found in a project base",
                            c.header.path
                        ),
                    ));
                }
                members.insert(n.clone(), Some("Function".into()));
            }
        }
        let mut statics = c
            .base
            .as_ref()
            .and_then(|base| static_members.get(base))
            .cloned()
            .unwrap_or_default();
        for field in c.fields.iter().filter(|f| f.is_static) {
            statics.insert(field.name.clone(), name.clone());
        }
        for method in c.methods.iter().filter(|m| m.is_static) {
            statics.insert(method.function.name.clone().unwrap(), name.clone());
        }
        c.context.static_members = statics.clone();
        static_members.insert(name.clone(), statics);
        c.context.members = members.clone();
        resolved.insert(name.clone(), members);
        match emit_class(c) {
            Ok(code) => registrations.push_str(&code),
            Err(e) => errors.push(prefix(e, &c.header.path)),
        }
    }
    for linked in project
        .document_class
        .iter()
        .chain(project.symbols.iter().filter_map(|s| s.class_name.as_ref()))
    {
        if !classes.contains_key(linked) {
            errors.push(Diagnostic {
                language: ScriptLanguage::ActionScript3,
                line: 1,
                column: 1,
                message: format!("Unknown linked project class '{linked}'."),
            });
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let code = format!(
        "{}\n{}\n{}\n__nfProgram.initializeStatics();\nreturn __nfProgram;",
        embedded_helpers(),
        include_str!("project-runtime.js"),
        registrations
    );
    // Common intrinsic captures precede helper declarations.
    let code = format!(
        "const __nfNumber=Number,__nfString=String,__nfBoolean=Boolean,__nfTypeError=TypeError;\nconst __nfAs2Method=__nfBuiltins.__nfAs2Method,__nfAs2Value=__nfBuiltins.__nfAs2Value,__nfDisplay=__nfBuiltins.__nfDisplay,__nfLegacy=__nfBuiltins.__nfLegacy;\nconst __nfTrace=(...args)=>{{if(typeof __nfBuiltins.__nfTrace !== 'function')throw new Error('Project trace host binding is missing');return __nfBuiltins.__nfTrace(...args);}};\n{code}"
    );
    Ok(ProjectProgram {
        code,
        class_names: order,
        names,
        languages,
    })
}

pub fn compile_with_program(
    script: &Script,
    program: &ProjectProgram,
) -> Result<String, Vec<Diagnostic>> {
    if !matches!(
        script.language,
        ScriptLanguage::ActionScript3 | ScriptLanguage::ActionScript2
    ) {
        return crate::compile(script);
    }
    let tokens = Lexer { script, offset: 0 }.tokens().map_err(|e| vec![e])?;
    let mut p = frontend(script, tokens, CompileContext::default());
    let imported = imports(&mut p).map_err(|e| vec![e])?;
    let c = context(
        &visible_names(&program.names, &program.languages, script.language),
        "",
        &imported,
        script.language,
    )
    .map_err(|m| vec![Diagnostic::at(script, 0, m)])?;
    // Keep original byte positions/line numbers when removing import statements.
    let end = p.token().offset;
    let mut replaced = script.clone();
    replaced.source = script.source[..end]
        .chars()
        .map(|c| if c == '\n' || c == '\r' { c } else { ' ' })
        .collect::<String>()
        + &script.source[end..];
    super::compile_context(&replaced, c).map_err(|e| vec![e])
}

fn emit_class(c: &Class) -> Result<String, Diagnostic> {
    let as3 = c.header.script.language == ScriptLanguage::ActionScript3;
    let mut fields = vec![];
    let mut methods = vec![];
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
    };
    for f in &c.fields {
        let coerce = emitter.coerce_type(f.ty.as_deref(), "value".into());
        let initial = f
            .value
            .as_ref()
            .map(|v| {
                format!(
                    "function(){{const __nfFrame=this;return {};}}",
                    emitter.expression(v)
                )
            })
            .unwrap_or_else(|| "null".into());
        fields.push(format!("{{name:{:?},static:{},constant:{},defaultValue:{},coerce:function(value){{return {coerce};}},init:{initial}}}",f.name,f.is_static,f.constant,emitter.default_value(f.ty.as_deref())));
    }
    for method in &c.methods {
        let f = &method.function;
        let name = f.name.as_ref().unwrap();
        let mut context = c.context.clone();
        context.method_id = Some(f.id);
        context.static_method = method.is_static;
        let mut f = f.clone();
        f.name = Some(format!("__nfMethodBody{}", f.id));
        let is_ctor = name == short;
        if is_ctor {
            if method.is_static || method.override_ || f.returns.is_some() {
                return Err(Diagnostic::at(
                    &c.header.script,
                    f.offset,
                    "A constructor cannot be static, override or have a return annotation.",
                ));
            }
            let explicit = matches!(f.body.first(),Some(Stmt::Expr(Expr::Call(callee,_,_))) if matches!(callee.as_ref(),Expr::Name(n) if n=="super"));
            if !explicit {
                f.body.insert(
                    0,
                    Stmt::Expr(Expr::Call(
                        Box::new(Expr::Name("super".into())),
                        vec![],
                        f.offset,
                    )),
                );
            }
            // Field initializers are executed after the selected base constructor.
            f.body.insert(
                1,
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
        let body = vec![Stmt::Function(f.clone())];
        let analysis = scope::Analysis::with_context(&c.header.script, &body, context)?;
        let emitter = Emitter {
            as3,
            analysis: &analysis,
            scope: 0,
        };
        let function = emitter.function(&f);
        if is_ctor {
            constructor = function;
        } else {
            methods.push(format!(
                "{{name:{name:?},static:{},fn:{function}}}",
                method.is_static
            ));
        }
    }
    Ok(format!(
        "__nfProgram.define({:?},{},[{}],[{}],{},{:?});\n",
        c.header.name,
        c.base
            .as_ref()
            .map(|s| format!("{s:?}"))
            .unwrap_or_else(|| "null".into()),
        fields.join(","),
        methods.join(","),
        constructor,
        if as3 { "as3" } else { "as2" }
    ))
}
