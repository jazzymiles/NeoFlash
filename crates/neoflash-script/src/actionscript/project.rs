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
    "flash.display.Shape",
    "flash.display.Graphics",
    "flash.display.GradientType",
    "flash.display.SpreadMethod",
    "flash.display.InterpolationMethod",
    "flash.display.GraphicsPathCommand",
    "flash.display.GraphicsPathWinding",
    "flash.display.Bitmap",
    "flash.display.BitmapData",
    "flash.display.PixelSnapping",
    "flash.display.Loader",
    "flash.display.LoaderInfo",
    "flash.display.Stage",
    "flash.text.TextField",
    "flash.text.TextFormat",
    "flash.text.TextFormatAlign",
    "flash.text.TextFieldType",
    "flash.media.Sound",
    "flash.media.SoundChannel",
    "flash.media.SoundTransform",
    "flash.filters.BitmapFilter",
    "flash.filters.BitmapFilterQuality",
    "flash.filters.BlurFilter",
    "flash.filters.GlowFilter",
    "flash.filters.DropShadowFilter",
    "flash.filters.ColorMatrixFilter",
    "flash.geom.Point",
    "flash.geom.Rectangle",
    "flash.geom.Matrix",
    "flash.geom.Transform",
    "flash.geom.ColorTransform",
    "flash.utils.Timer",
    "flash.utils.ByteArray",
    "flash.utils.Endian",
    "flash.utils.Dictionary",
    "flash.net.URLRequest",
    "flash.net.URLRequestHeader",
    "flash.net.URLRequestMethod",
    "flash.net.URLVariables",
    "flash.net.URLLoader",
    "flash.net.URLLoaderDataFormat",
    "flash.net.SharedObject",
    "flash.net.SharedObjectFlushStatus",
    "flash.net.ObjectEncoding",
    "flash.events.ProgressEvent",
    "flash.events.IOErrorEvent",
    "flash.events.HTTPStatusEvent",
    "flash.events.TimerEvent",
    "flash.events.EventDispatcher",
    "flash.events.Event",
    "flash.events.MouseEvent",
    "flash.events.KeyboardEvent",
    "flash.events.FocusEvent",
    "flash.events.TextEvent",
    "flash.events.EventPhase",
];
const BUILTIN_FUNCTIONS: &[&str] = &["flash.utils.getTimer"];
pub(super) fn is_graphics_constants(name: &str) -> bool {
    matches!(
        name,
        "flash.display.GradientType"
            | "flash.display.SpreadMethod"
            | "flash.display.InterpolationMethod"
            | "flash.display.GraphicsPathCommand"
            | "flash.display.GraphicsPathWinding"
    )
}
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
            | "addChild"
            | "addChildAt"
            | "removeChild"
            | "removeChildAt"
            | "setChildIndex"
            | "setMask"
            | "swapChildren"
            | "swapChildrenAt"
            | "createEmptyMovieClip"
            | "createTextField"
            | "removeMovieClip"
            | "getNextHighestDepth"
            | "attachBitmap"
            | "beginFill"
            | "beginGradientFill"
            | "lineGradientStyle"
            | "endFill"
            | "lineStyle"
            | "moveTo"
            | "lineTo"
            | "curveTo"
            | "clear"
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
                | "cubicCurveTo"
                | "drawRect"
                | "drawRoundRect"
                | "drawRoundRectComplex"
                | "drawPath"
                | "drawEllipse"
                | "drawCircle"
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
                | "getRelativeMatrix"
                | "start"
                | "reset"
                | "updateAfterEvent"
                | "load"
                | "loadBytes"
                | "unload"
                | "getLocal"
                | "flush"
                | "setProperty"
                | "setDirty"
                | "readObject"
                | "writeObject"
                | "close"
                | "decode"
                | "getPixel"
                | "getPixel32"
                | "setPixel"
                | "setPixel32"
                | "fillRect"
                | "copyPixels"
                | "scroll"
                | "dispose"
                | "appendText"
                | "replaceText"
                | "replaceSelectedText"
                | "setSelection"
                | "getLineText"
                | "getLineLength"
                | "getLineOffset"
                | "getLineIndexOfChar"
                | "getTextFormat"
                | "setTextFormat"
                | "getNewTextFormat"
                | "setNewTextFormat"
        )
}

/// `code` is a function body taking __nfBuiltins and returning the program API.
pub struct ProjectProgram {
    pub code: String,
    pub class_names: Vec<String>,
    names: BTreeSet<String>,
    languages: BTreeMap<String, ScriptLanguage>,
    interfaces: BTreeSet<String>,
}

struct Header {
    script: Script,
    path: String,
    package: String,
    name: String,
    base: Option<String>,
    interfaces: Vec<String>,
    is_interface: bool,
    imports: Vec<String>,
    tokens: TokenCursor,
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
    is_public: bool,
    kind: MethodKind,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MethodKind {
    Method,
    Get,
    Set,
}
struct Class {
    header: Header,
    context: CompileContext,
    base: Option<String>,
    interfaces: Vec<String>,
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
                        || (name.starts_with("flash.filters.")
                            && name.as_str() != "flash.filters.BitmapFilterQuality")
                        || *name == "flash.display.BitmapData"
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

fn frontend(script: &Script, tokens: TokenCursor, context: CompileContext) -> Frontend<'_> {
    Frontend {
        script,
        tokens,
        pos: 0,
        depth: 0,
        controls: vec![],
        function_depth: 0,
        next_function: 1,
        context,
        no_in: false,
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
    let tokens = TokenCursor::default();
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
    let is_interface = p.eat("interface");
    if is_interface && as2 {
        return Err(p.error(
            "Project interfaces currently require AS3; AS2 interface semantics remain unsupported.",
        ));
    }
    if !is_interface {
        p.expect("class")?;
    }
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
    let mut interfaces = Vec::new();
    let base = if p.eat("extends") {
        if is_interface {
            loop {
                interfaces.push(qualified(&mut p)?);
                if !p.eat(",") {
                    break;
                }
            }
            None
        } else {
            Some(qualified(&mut p)?)
        }
    } else {
        None
    };
    if p.eat("implements") {
        if as2 || is_interface {
            return Err(p.error("Only AS3 project classes can use implements in this profile."));
        }
        loop {
            interfaces.push(qualified(&mut p)?);
            if !p.eat(",") {
                break;
            }
        }
    }
    p.expect("{")?;
    let body = p.pos;
    let tokens = p.tokens;
    Ok(Header {
        script,
        path: source.path.clone(),
        package,
        name,
        base,
        interfaces,
        is_interface,
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
    interfaces: &BTreeSet<String>,
) -> Result<CompileContext, String> {
    let mut c = CompileContext {
        project: true,
        interfaces: interfaces.intersection(names).cloned().collect(),
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
        c.types
            .insert("TextFormat".into(), "flash.text.TextFormat".into());
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
    // Source classes/imports win over these top-level native names. In
    // particular, AS2 projects may define their own JSON or RegExp classes.
    for name in if language == ScriptLanguage::ActionScript3 {
        &[
            "Date",
            "RegExp",
            "JSON",
            "Vector",
            "XML",
            "XMLList",
            "Namespace",
            "QName",
        ][..]
    } else {
        &["Date", "XML", "XMLNode"][..]
    } {
        if !c.types.contains_key(*name) {
            c.types.insert((*name).into(), (*name).into());
            c.intrinsic_types.insert((*name).into());
        }
    }
    Ok(c)
}
fn parse_class(
    header: Header,
    names: &BTreeSet<String>,
    interface_names: &BTreeSet<String>,
) -> Result<Class, Diagnostic> {
    let mut c = context(
        names,
        &header.package,
        &header.imports,
        header.script.language,
        interface_names,
    )
    .map_err(|m| Diagnostic::at(&header.script, 0, m))?;
    c.class_name = Some(header.name.clone());
    if header.base.as_ref().is_some_and(|name| {
        c.intrinsic_type(
            name,
            header.script.language == ScriptLanguage::ActionScript3,
        )
    }) {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "Native Date/RegExp/Vector/XML-family subclass initialization is outside this profile; use composition. JSON supplies static functions and cannot be a base class.",
        ));
    }
    let base = header
        .base
        .as_ref()
        .map(|b| {
            c.types.get(b).cloned().ok_or_else(|| {
                Diagnostic::at(&header.script, 0, format!("Unknown base class '{b}'."))
            })
        })
        .transpose()?;
    if base
        .as_ref()
        .is_some_and(|name| interface_names.contains(name))
    {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "A class cannot extend an interface; use implements.",
        ));
    }
    let mut interfaces = Vec::new();
    for name in &header.interfaces {
        let resolved = c
            .types
            .get(name)
            .filter(|name| interface_names.contains(*name))
            .ok_or_else(|| {
                Diagnostic::at(
                    &header.script,
                    0,
                    format!("'{name}' is not a resolved project interface."),
                )
            })?;
        if interfaces.contains(resolved) {
            return Err(Diagnostic::at(
                &header.script,
                0,
                format!("Duplicate interface '{name}'."),
            ));
        }
        interfaces.push(resolved.clone());
    }
    if base.as_deref().is_some_and(is_graphics_constants) {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "Graphics enum classes supply constants and cannot be native base classes.",
        ));
    }
    if matches!(
        base.as_deref(),
        Some("flash.geom.Transform" | "flash.geom.ColorTransform" | "flash.utils.Dictionary")
    ) {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "Transform, ColorTransform and Dictionary native subclass initialization is outside this profile; use native instances through composition.",
        ));
    }
    if matches!(
        base.as_deref(),
        Some("flash.utils.ByteArray" | "flash.utils.Endian" | "flash.display.Graphics")
    ) {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "ByteArray indexed storage cannot initialize an existing native subclass receiver; Endian supplies constants and Graphics comes from its display owner. Inheritance from these built-ins is unsupported.",
        ));
    }
    if matches!(
        base.as_deref(),
        Some("flash.display.Bitmap" | "flash.display.BitmapData" | "flash.display.PixelSnapping")
    ) {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "Bitmap and BitmapData native subclass initialization is outside this profile; construct the native facade and use composition. PixelSnapping provides constants and cannot be a base class.",
        ));
    }
    if matches!(
        base.as_deref(),
        Some("flash.display.Loader" | "flash.display.LoaderInfo")
    ) {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "Loader and LoaderInfo native subclass initialization is outside this profile; construct Loader() and use its contentLoaderInfo through composition.",
        ));
    }
    if base
        .as_ref()
        .is_some_and(|base| base.starts_with("flash.filters."))
    {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "Native filter subclasses are outside this profile; construct a supported concrete filter and use BitmapFilter-typed values through composition.",
        ));
    }
    if base
        .as_ref()
        .is_some_and(|base| base.starts_with("flash.text."))
    {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "TextField and TextFormat native subclass initialization is outside this profile; construct the native facade and use composition. TextFormatAlign and TextFieldType supply constants only.",
        ));
    }
    if base
        .as_ref()
        .is_some_and(|base| base.starts_with("flash.media."))
    {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "Sound, SoundChannel and SoundTransform native subclass initialization is outside this profile; use a Sound instance, its playback channel and a SoundTransform value.",
        ));
    }
    if base.as_ref().is_some_and(|base| {
        base.starts_with("flash.net.")
            || matches!(
                base.as_str(),
                "flash.events.ProgressEvent"
                    | "flash.events.IOErrorEvent"
                    | "flash.events.HTTPStatusEvent"
            )
    }) {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "Network/persistence request, loader, value and event subclasses are outside this profile; use the supported native facade through composition. Constant classes cannot be native bases.",
        ));
    }
    if matches!(
        base.as_deref(),
        Some("flash.events.FocusEvent" | "flash.events.TextEvent")
    ) {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "FocusEvent and TextEvent native subclass initialization is outside this profile; construct the event facade and use typed listeners through composition.",
        ));
    }
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
        let mut is_public = false;
        if header.is_interface
            && matches!(
                p.token_text().as_str(),
                "public" | "static" | "override" | "private" | "protected" | "internal"
            )
        {
            return Err(p.error(
                "Interface method signatures cannot have access, static or override modifiers.",
            ));
        }
        loop {
            if p.eat("public") {
                is_public = true;
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
            if header.is_interface {
                return Err(
                    p.error("Interfaces contain method/getter/setter signatures, not fields.")
                );
            }
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
                if seen.iter().any(|(old, _)| old == &name)
                    || !seen.insert((name.clone(), MethodKind::Method))
                {
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
            let (mut f, accessor) = p.project_function(true, true, header.is_interface)?;
            let kind = match accessor.as_deref() {
                Some("get") => MethodKind::Get,
                Some("set") => MethodKind::Set,
                _ => MethodKind::Method,
            };
            let name = f.name.as_ref().unwrap();
            if header.is_interface && name == header.name.rsplit('.').next().unwrap() {
                return Err(p.error("Interfaces cannot declare constructors."));
            }
            if kind != MethodKind::Method && !header.is_interface && !is_public {
                return Err(p.error(
                    "This accessor profile requires explicit public get/set declarations.",
                ));
            }
            if kind == MethodKind::Get
                && (!f.params.is_empty()
                    || f.rest.is_some()
                    || f.returns.as_deref() == Some("void"))
            {
                return Err(p.error("A getter requires no parameters and cannot return void."));
            }
            if kind == MethodKind::Set {
                if f.params.len() != 1
                    || f.rest.is_some()
                    || f.defaults.iter().any(Option::is_some)
                    || f.returns.as_deref().is_some_and(|ty| ty != "void")
                {
                    return Err(p.error("A setter requires exactly one required parameter, no rest parameter, and a void or omitted return type."));
                }
                f.returns = Some("void".into());
            }
            if kind != MethodKind::Method && name == header.name.rsplit('.').next().unwrap() {
                return Err(p.error("An accessor cannot use its containing type's constructor name in this profile."));
            }
            if f.params
                .iter()
                .map(|(name, _)| name)
                .chain(f.rest.iter())
                .collect::<BTreeSet<_>>()
                .len()
                != f.params.len() + usize::from(f.rest.is_some())
            {
                return Err(p.error("Duplicate parameter name in method signature."));
            }
            if seen.contains(&(name.clone(), kind))
                || seen.iter().any(|(old, old_kind)| {
                    old == name && (*old_kind == MethodKind::Method || kind == MethodKind::Method)
                })
            {
                return Err(p.error(format!("Duplicate class member '{name}'.")));
            }
            seen.insert((name.clone(), kind));
            methods.push(Method {
                function: f,
                is_static,
                override_,
                is_public,
                kind,
            });
        } else {
            return Err(p.error("Expected a supported public field, method or get/set accessor; namespaces remain unsupported."));
        }
    }
    p.expect("}")?;
    if header.packaged {
        p.expect("}")?;
    }
    if p.token().kind != Kind::End {
        return Err(p.error("A source unit must contain exactly one class or interface."));
    }
    Ok(Class {
        header,
        context: c,
        base,
        interfaces,
        fields,
        methods,
    })
}
fn prefix(mut e: Diagnostic, path: &str) -> Diagnostic {
    e.message = format!("{path}: {}", e.message);
    e
}

#[derive(Clone, PartialEq, Eq)]
struct Signature {
    params: Vec<String>,
    optional: Vec<bool>,
    // This bounded source profile requires the same rest shape. Default values
    // themselves are not part of override/interface signature compatibility.
    rest: bool,
    returns: String,
}
#[derive(Clone)]
struct MethodContract {
    signature: Signature,
    is_public: bool,
}
fn method_contract(class: &Class, method: &Method) -> MethodContract {
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
        is_public: class.header.is_interface || method.is_public,
    }
}
fn check_accessor_pairs(
    class: &Class,
    contracts: &BTreeMap<(String, MethodKind), MethodContract>,
    errors: &mut Vec<Diagnostic>,
) {
    for ((name, kind), getter) in contracts {
        if *kind == MethodKind::Get
            && let Some(setter) = contracts.get(&(name.clone(), MethodKind::Set))
            && getter.signature.returns != setter.signature.params[0]
        {
            errors.push(Diagnostic::at(
                &class.header.script,
                0,
                format!(
                    "{}: getter and setter '{name}' must use the same property type",
                    class.header.path
                ),
            ));
        }
        if *kind != MethodKind::Method
            && contracts.contains_key(&(name.clone(), MethodKind::Method))
        {
            errors.push(Diagnostic::at(
                &class.header.script,
                0,
                format!(
                    "{}: accessor '{name}' conflicts with a method",
                    class.header.path
                ),
            ));
        }
    }
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
                    | "graphics"
                    | "transform"
                    | "mask"
                    | "filters"
                    | "scrollRect"
                    | "loaderInfo"
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
    let interfaces: BTreeSet<_> = headers
        .iter()
        .filter(|h| h.is_interface)
        .map(|h| h.name.clone())
        .collect();
    let mut classes = BTreeMap::new();
    for h in headers {
        let path = h.path.clone();
        let visible = visible_names(&names, &languages, h.script.language);
        match parse_class(h, &visible, &interfaces) {
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
                .iter()
                .chain(&classes[&name].interfaces)
                .all(|dependency| !pending.contains(dependency))
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
    let mut contracts = BTreeMap::<String, BTreeMap<(String, MethodKind), MethodContract>>::new();
    let mut interface_closures = BTreeMap::<String, BTreeSet<String>>::new();
    let mut registrations = String::new();
    for name in &order {
        let c = classes.get_mut(name).unwrap();
        if c.header.is_interface {
            let mut inherited = BTreeMap::new();
            let mut closure = BTreeSet::from([name.clone()]);
            for parent in &c.interfaces {
                closure.extend(interface_closures[parent].iter().cloned());
                for (key, contract) in &contracts[parent] {
                    if inherited
                        .get(key)
                        .is_some_and(|old: &MethodContract| old.signature != contract.signature)
                    {
                        errors.push(Diagnostic::at(
                            &c.header.script,
                            0,
                            format!(
                                "{}: conflicting inherited interface signature '{}'",
                                c.header.path, key.0
                            ),
                        ));
                    }
                    inherited.insert(key.clone(), contract.clone());
                }
            }
            for method in &c.methods {
                let key = (method.function.name.clone().unwrap(), method.kind);
                let contract = method_contract(c, method);
                if inherited
                    .get(&key)
                    .is_some_and(|old| old.signature != contract.signature)
                {
                    errors.push(Diagnostic::at(
                        &c.header.script,
                        method.function.offset,
                        format!(
                            "{}: incompatible interface signature '{}'",
                            c.header.path, key.0
                        ),
                    ));
                }
                inherited.insert(key, contract);
            }
            check_accessor_pairs(c, &inherited, &mut errors);
            contracts.insert(name.clone(), inherited);
            interface_closures.insert(name.clone(), closure);
            registrations.push_str(&format!("__nfProgram.defineInterface({name:?});\n"));
            continue;
        }
        let mut contract = c
            .base
            .as_ref()
            .and_then(|base| contracts.get(base))
            .cloned()
            .unwrap_or_default();
        let inherited_contract = contract.clone();
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
        let inherited_members = members.clone();
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
            if n != short || m.kind != MethodKind::Method {
                let key = (n.clone(), m.kind);
                let own_contract = method_contract(c, m);
                let inherited = if m.is_static {
                    None
                } else {
                    inherited_contract.get(&key)
                };
                let same_name = inherited_contract.keys().any(|(name, kind)| {
                    name == n && (*kind == MethodKind::Method || m.kind == MethodKind::Method)
                });
                if m.override_ && inherited.is_none() {
                    errors.push(Diagnostic::at(
                        &c.header.script,
                        m.function.offset,
                        format!(
                            "{}: override target '{n}' not found in a project base",
                            c.header.path
                        ),
                    ));
                }
                if c.header.script.language == ScriptLanguage::ActionScript3 {
                    if let Some(base) = inherited {
                        if !m.override_
                            || base.signature != own_contract.signature
                            || base.is_public != own_contract.is_public
                        {
                            errors.push(Diagnostic::at(&c.header.script, m.function.offset, format!("{}: inherited member '{n}' requires override and an identical public signature", c.header.path)));
                        }
                    } else if !m.is_static
                        && (same_name
                            || inherited_members.contains_key(n)
                                && !inherited_contract.keys().any(|(name, _)| name == n))
                    {
                        errors.push(Diagnostic::at(
                            &c.header.script,
                            m.function.offset,
                            format!(
                                "{}: member '{n}' conflicts with an inherited field or method kind",
                                c.header.path
                            ),
                        ));
                    }
                }
                if !m.is_static {
                    contract.insert(key, own_contract);
                }
                // Accessor invocation owns coercion. A second emitter coercion
                // on a bare property would repeat observable conversions.
                members.insert(
                    n.clone(),
                    if m.kind == MethodKind::Method {
                        Some("Function".into())
                    } else {
                        None
                    },
                );
            }
        }
        check_accessor_pairs(c, &contract, &mut errors);
        let statics_contract = c
            .methods
            .iter()
            .filter(|m| m.is_static)
            .map(|m| {
                (
                    (m.function.name.clone().unwrap(), m.kind),
                    method_contract(c, m),
                )
            })
            .collect();
        check_accessor_pairs(c, &statics_contract, &mut errors);
        for getter in c.methods.iter().filter(|m| m.kind == MethodKind::Get) {
            if c.methods.iter().any(|m| {
                m.kind == MethodKind::Set
                    && m.function.name == getter.function.name
                    && m.is_static != getter.is_static
            }) {
                errors.push(Diagnostic::at(
                    &c.header.script,
                    getter.function.offset,
                    format!("{}: getter and setter must agree on static", c.header.path),
                ));
            }
        }
        let mut implemented = BTreeSet::new();
        for interface in &c.interfaces {
            implemented.extend(interface_closures[interface].iter().cloned());
            for (key, expected) in &contracts[interface] {
                if !contract.get(key).is_some_and(|actual| {
                    actual.is_public && actual.signature == expected.signature
                }) {
                    errors.push(Diagnostic::at(&c.header.script, 0, format!("{}: interface '{interface}' requires public member '{}' with the matching method/accessor signature", c.header.path, key.0)));
                }
            }
        }
        c.interfaces = implemented.into_iter().collect();
        contracts.insert(name.clone(), contract);
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
        if !classes
            .get(linked)
            .is_some_and(|class| !class.header.is_interface)
        {
            errors.push(Diagnostic {
                language: ScriptLanguage::ActionScript3,
                line: 1,
                column: 1,
                message: format!("Unknown linked project class '{linked}'."),
            });
        }
    }
    for symbol in project
        .symbols
        .iter()
        .filter(|symbol| symbol.linkage_id.is_some())
    {
        if let Some(class) = &symbol.class_name
            && native_bases.get(class).map(String::as_str) != Some("flash.display.MovieClip")
        {
            errors.push(Diagnostic {
                language: classes.get(class).map_or(ScriptLanguage::ActionScript3, |class| class.header.script.language),
                line: 1,
                column: 1,
                message: format!("Exported MovieClip '{}' requires class '{class}' to ultimately extend flash.display.MovieClip.",symbol.name),
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
        "const __nfNumber=Number,__nfString=String,__nfBoolean=Boolean,__nfTypeError=TypeError;\nconst __nfAs2Method=__nfBuiltins.__nfAs2Method,__nfAs2Value=__nfBuiltins.__nfAs2Value,__nfDisplay=__nfBuiltins.__nfDisplay,__nfLegacy=__nfBuiltins.__nfLegacy,__nfXML=__nfBuiltins.__nfXML;\nconst __nfTrace=(...args)=>{{if(typeof __nfBuiltins.__nfTrace !== 'function')throw new Error('Project trace host binding is missing');return __nfBuiltins.__nfTrace(...args);}};\n{code}"
    );
    Ok(ProjectProgram {
        code,
        class_names: order
            .into_iter()
            .filter(|name| !interfaces.contains(name))
            .collect(),
        names,
        languages,
        interfaces,
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
    let tokens = TokenCursor::default();
    let mut p = frontend(script, tokens, CompileContext::default());
    let imported = imports(&mut p).map_err(|e| vec![e])?;
    let c = context(
        &visible_names(&program.names, &program.languages, script.language),
        "",
        &imported,
        script.language,
        &program.interfaces,
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
    let mut accessors = BTreeMap::<(String, bool), (Option<String>, Option<String>)>::new();
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
        let is_ctor = name == short && method.kind == MethodKind::Method;
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
        let body = vec![Stmt::Function(Box::new(f.clone()))];
        let analysis = scope::Analysis::with_context(&c.header.script, &body, context)?;
        let emitter = Emitter {
            as3,
            analysis: &analysis,
            scope: 0,
        };
        let function = emitter.function(&f);
        if is_ctor {
            constructor = function;
        } else if method.kind == MethodKind::Method {
            methods.push(format!(
                "{{name:{name:?},static:{},fn:{function}}}",
                method.is_static
            ));
        } else {
            let pair = accessors
                .entry((name.clone(), method.is_static))
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
        .map(|((name, is_static), (get, set))| {
            format!(
                "{{name:{name:?},static:{is_static},get:{},set:{}}}",
                get.unwrap_or_else(|| "null".into()),
                set.unwrap_or_else(|| "null".into())
            )
        })
        .collect::<Vec<_>>();
    Ok(format!(
        "__nfProgram.define({:?},{},[{}],[{}],{},{:?},[{}],{:?});\n",
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
        c.interfaces
    ))
}
