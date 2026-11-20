//! Project package, import, namespace, header, and class body parser.

use super::*;

pub(super) fn visible_names(
    names: &BTreeSet<String>,
    languages: &BTreeMap<String, ScriptLanguage>,
    language: ScriptLanguage,
    public_types: &BTreeSet<String>,
    package: &str,
) -> BTreeSet<String> {
    names
        .iter()
        .filter(|name| {
            languages.get(*name).map_or_else(
                || {
                    (language == ScriptLanguage::ActionScript3
                        && !name.starts_with("mx.transitions.")
                        && (!AS2_BUILTINS.contains(&name.as_str())
                            || matches!(
                                name.as_str(),
                                "flash.net.FileReference" | "flash.net.FileReferenceList"
                            )))
                        || matches!(name.as_str(), "Object" | "Array")
                        || (language == ScriptLanguage::ActionScript2
                            && (name.starts_with("mx.transitions.")
                                || AS2_BUILTINS.contains(&name.as_str())))
                        || (name.starts_with("flash.geom.")
                            && (language == ScriptLanguage::ActionScript3
                                || *name != "flash.geom.Orientation3D"))
                        || (name.starts_with("flash.filters.")
                            && !matches!(
                                name.as_str(),
                                "flash.filters.BitmapFilterQuality"
                                    | "flash.filters.DisplacementMapFilterMode"
                            )
                            && (language == ScriptLanguage::ActionScript2
                                || !AS2_BUILTINS.contains(&name.as_str())))
                        || *name == "flash.display.BitmapData"
                        || *name == "flash.external.ExternalInterface"
                        || *name == "flash.net.FileReference"
                        || *name == "flash.net.FileReferenceList"
                        || (language == ScriptLanguage::ActionScript2
                            && matches!(
                                name.as_str(),
                                "flash.media.Video" | "flash.text.TextRenderer"
                            ))
                },
                |owner| {
                    *owner == language
                        && (language == ScriptLanguage::ActionScript2
                            || public_types.contains(*name)
                            || type_package(name) == package)
                },
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

pub(super) fn frontend(
    script: &Script,
    tokens: TokenCursor,
    context: CompileContext,
) -> Frontend<'_> {
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
        with_depth: 0,
        in_initclip: false,
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
pub(super) fn imports(p: &mut Frontend<'_>) -> Result<Vec<String>, Diagnostic> {
    let mut imports = vec![];
    while p.eat("import") {
        let mut name = p.ident()?;
        while p.eat(".") {
            name.push('.');
            if p.eat("*") {
                name.push('*');
                break;
            }
            name.push_str(&p.ident()?);
        }
        imports.push(name);
        p.expect(";")?;
    }
    Ok(imports)
}
fn parse_namespace_uri_literal(
    p: &mut Frontend<'_>,
    default_uri: String,
    known: &BTreeMap<String, String>,
) -> Result<String, Diagnostic> {
    if !p.eat("=") {
        return Ok(default_uri);
    }
    match p.token().kind.clone() {
        Kind::String(_) => {
            let lit = p.token_text();
            p.pos += 1;
            let unquoted = lit
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .or_else(|| lit.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
                .unwrap_or(&lit)
                .to_owned();
            Ok(unquoted)
        }
        Kind::Ident(_) => {
            let ref_name = qualified(p)?;
            known
                .get(&ref_name)
                .cloned()
                .or_else(|| builtin_namespace_uri(&ref_name).map(str::to_owned))
                .ok_or_else(|| p.error(format!("Unresolved namespace initializer '{ref_name}'.")))
        }
        _ => Err(p.error("Expected a namespace URI string literal.")),
    }
}

pub(super) fn header(
    source: &neoflash_core::SourceUnit,
    includes: &BTreeMap<String, neoflash_core::ResourceUnit>,
) -> Result<Header, Diagnostic> {
    let raw_script = Script {
        language: source.language,
        source: source.source.clone(),
    };
    if raw_script.language == ScriptLanguage::ActionScript1 {
        return Err(Diagnostic::at(
            &raw_script,
            0,
            "ActionScript 1.0 does not support AS2/AS3 class or interface source units; use timeline/object prototype functions or #include resource files.",
        ));
    }
    if !matches!(
        raw_script.language,
        ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript3
    ) {
        return Err(Diagnostic::at(
            &raw_script,
            0,
            "Project source units require AS2 or AS3 classes; JS/TS modules are not implemented yet.",
        ));
    }
    if raw_script.source.len() > 1_048_576 {
        return Err(Diagnostic::at(
            &raw_script,
            0,
            "A source unit is limited to 1 MiB.",
        ));
    }
    let script = if (raw_script.language == ScriptLanguage::ActionScript2
        && raw_script.source.contains("#include"))
        || (raw_script.language == ScriptLanguage::ActionScript3
            && (raw_script.source.contains("include ")
                || raw_script.source.contains("include\"")
                || raw_script.source.contains("include'")
                || raw_script.source.contains("#include")))
    {
        super::includes::expand_at(&raw_script, &source.path, includes)?.script
    } else {
        raw_script
    };
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
    let mut imports = imports(&mut p)?;
    let mut local_namespaces = BTreeMap::new();
    let mut open_namespaces = Vec::new();
    let mut public = as2;
    let mut access_seen = false;
    let mut dynamic = false;
    let mut final_ = false;
    let mut intrinsic = false;
    loop {
        if !as2
            && !access_seen
            && !dynamic
            && !final_
            && p.is("use")
            && p.peek(p.pos + 1)
                .is_some_and(|t| matches!(&t.kind, Kind::Ident(s) if s == "namespace"))
        {
            p.pos += 2;
            loop {
                open_namespaces.push(qualified(&mut p)?);
                if !p.eat(",") {
                    break;
                }
            }
            p.semicolon()?;
            imports.extend(self::imports(&mut p)?);
            continue;
        }
        if p.is("public") || p.is("internal") {
            let value = p.eat("public");
            if !value {
                p.expect("internal")?;
            }
            if access_seen || (as2 && !value) {
                return Err(p.error("Duplicate or unsupported class access modifier."));
            }
            access_seen = true;
            public = value;
        } else if p.eat("dynamic") {
            if dynamic {
                return Err(p.error("Duplicate dynamic modifier."));
            }
            dynamic = true;
        } else if p.eat("final") {
            if as2 || final_ {
                return Err(
                    p.error("The final class modifier requires AS3 and cannot be repeated.")
                );
            }
            final_ = true;
        } else if p.eat("intrinsic") {
            if !as2 || intrinsic {
                return Err(p.error(
                    "The intrinsic declaration modifier requires AS2 and cannot be repeated.",
                ));
            }
            intrinsic = true;
        } else if !as2 && !dynamic && !final_ && p.eat("namespace") {
            let short = p.ident()?;
            if invalid_binding(&short, false) {
                return Err(p.error("Invalid or reserved namespace name."));
            }
            let default_uri = if package.is_empty() {
                short.clone()
            } else {
                format!("{package}:{short}")
            };
            let uri = parse_namespace_uri_literal(&mut p, default_uri, &local_namespaces)?;
            p.semicolon()?;
            if p.is("}") || p.token().kind == Kind::End {
                if packaged {
                    p.expect("}")?;
                }
                if p.token().kind != Kind::End {
                    return Err(p.error(
                        "A source unit must contain a single primary package declaration.",
                    ));
                }
                let name = if package.is_empty() {
                    short
                } else {
                    format!("{package}.{short}")
                };
                let body = p.pos;
                let tokens = p.tokens;
                return Ok(Header {
                    script,
                    path: source.path.clone(),
                    package,
                    name,
                    base: None,
                    interfaces: Vec::new(),
                    is_interface: false,
                    is_namespace: true,
                    namespace_uri: Some(uri),
                    local_namespaces,
                    open_namespaces,
                    imports,
                    tokens,
                    body,
                    packaged,
                    public,
                    dynamic: false,
                    final_: false,
                    intrinsic: false,
                });
            }
            local_namespaces.insert(short, uri);
            public = false;
            access_seen = false;
            imports.extend(self::imports(&mut p)?);
        } else {
            break;
        }
    }
    let is_interface = p.eat("interface");
    if is_interface && (dynamic || final_) {
        return Err(p.error("Interfaces cannot be dynamic or final."));
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
                if as2 {
                    return Err(
                        p.error("AS2 interfaces extend one parent interface in this profile.")
                    );
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
        if is_interface {
            return Err(
                p.error("An interface extends a parent contract; only classes use implements.")
            );
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
        is_namespace: false,
        namespace_uri: None,
        local_namespaces,
        open_namespaces,
        imports,
        tokens,
        body,
        packaged,
        public,
        dynamic,
        final_,
        intrinsic,
    })
}
pub(super) fn context(
    names: &BTreeSet<String>,
    package: &str,
    imports: &[String],
    language: ScriptLanguage,
    interfaces: &BTreeSet<String>,
    namespace_uris: &BTreeMap<String, String>,
) -> Result<CompileContext, String> {
    let resolve_ns = |qualified_name: &str| -> Option<String> {
        builtin_namespace_uri(qualified_name)
            .map(str::to_owned)
            .or_else(|| namespace_uris.get(qualified_name).cloned())
    };
    let mut c = CompileContext {
        project: true,
        interfaces: interfaces.intersection(names).cloned().collect(),
        ..Default::default()
    };
    if language == ScriptLanguage::ActionScript3 {
        c.namespaces
            .insert("AS3".into(), "http://adobe.com/AS3/2006/builtin".into());
    }
    for name in names {
        if language == ScriptLanguage::ActionScript3
            && let Some(uri) = resolve_ns(name)
        {
            c.namespaces.insert(name.clone(), uri);
        } else if BUILTIN_FUNCTIONS.contains(&name.as_str()) {
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
            .insert("Button".into(), "flash.display.SimpleButton".into());
        c.types
            .insert("TextField".into(), "flash.text.TextField".into());
        c.types
            .insert("TextFormat".into(), "flash.text.TextFormat".into());
        c.types
            .insert("TextField.StyleSheet".into(), "TextField.StyleSheet".into());
        c.types.insert("Video".into(), "flash.media.Video".into());
    }
    for name in ["Object", "Array"] {
        c.types.insert(name.into(), name.into());
    }
    for name in names {
        let (owner, short) = name.rsplit_once('.').unwrap_or(("", name));
        if owner == package {
            if language == ScriptLanguage::ActionScript3
                && let Some(uri) = resolve_ns(name)
            {
                c.namespaces.insert(short.into(), uri);
            } else if BUILTIN_FUNCTIONS.contains(&name.as_str()) {
                c.functions.insert(short.into(), name.clone());
            } else {
                c.types.insert(short.into(), name.clone());
            }
        }
    }
    // Exact imports resolve first; wildcard imports cover one package level only.
    // A conflicting wildcard short name is rejected deterministically, even if unused.
    for import in imports.iter().filter(|name| !name.ends_with(".*")) {
        if !names.contains(import) {
            return Err(format!(
                "Unresolved or inaccessible import '{import}'. Add an accessible source unit or supported built-in type."
            ));
        }
        let short = import.rsplit('.').next().unwrap();
        if language == ScriptLanguage::ActionScript3
            && let Some(uri) = resolve_ns(import)
        {
            c.namespaces.insert(short.into(), uri);
            continue;
        }
        if c.types
            .get(short)
            .or_else(|| c.functions.get(short))
            .is_some_and(|old| old != import)
        {
            return Err(format!("Ambiguous imported name '{short}'."));
        }
        if BUILTIN_FUNCTIONS.contains(&import.as_str()) {
            c.functions.insert(short.into(), import.clone());
        } else {
            c.types.insert(short.into(), import.clone());
        }
    }
    let explicit: BTreeSet<_> = imports
        .iter()
        .filter(|name| !name.ends_with(".*"))
        .map(|name| name.rsplit('.').next().unwrap())
        .collect();
    let mut wildcard_names = BTreeMap::<String, String>::new();
    for import in imports.iter().filter(|name| name.ends_with(".*")) {
        let package = &import[..import.len() - 2];
        let selected: Vec<_> = names
            .iter()
            .filter(|name| type_package(name) == package)
            .collect();
        if selected.is_empty() {
            return Err(format!(
                "Wildcard import '{import}' has no accessible immediate-package declarations."
            ));
        }
        for name in selected {
            let short = name.rsplit('.').next().unwrap();
            if explicit.contains(short) {
                continue;
            }
            if wildcard_names.get(short).is_some_and(|old| old != name) {
                return Err(format!(
                    "Ambiguous wildcard imported name '{short}'. Use an exact import or qualified name."
                ));
            }
            wildcard_names.insert(short.into(), name.clone());
        }
    }
    for (short, name) in wildcard_names {
        if language == ScriptLanguage::ActionScript3
            && let Some(uri) = resolve_ns(&name)
        {
            c.namespaces.entry(short).or_insert(uri);
            continue;
        }
        // Same-package declarations take precedence over wildcard imports.
        if c.types
            .get(&short)
            .or_else(|| c.functions.get(&short))
            .is_some_and(|old| type_package(old) == package)
        {
            continue;
        }
        if BUILTIN_FUNCTIONS.contains(&name.as_str()) {
            c.functions.insert(short, name);
        } else {
            c.types.insert(short, name);
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
pub(super) fn parse_class(
    header: Header,
    names: &BTreeSet<String>,
    interface_names: &BTreeSet<String>,
    namespace_uris: &BTreeMap<String, String>,
) -> Result<Class, Diagnostic> {
    let mut c = context(
        names,
        &header.package,
        &header.imports,
        header.script.language,
        interface_names,
        namespace_uris,
    )
    .map_err(|m| Diagnostic::at(&header.script, 0, m))?;
    for (k, v) in &header.local_namespaces {
        c.namespaces.insert(k.clone(), v.clone());
    }
    for ns_name in &header.open_namespaces {
        let uri = c.namespaces.get(ns_name).cloned().ok_or_else(|| {
            Diagnostic::at(
                &header.script,
                0,
                format!("Unresolved namespace '{ns_name}' in use namespace directive."),
            )
        })?;
        if !c.open_namespaces.contains(&uri) {
            c.open_namespaces.push(uri);
        }
    }
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
        .as_deref()
        .is_some_and(|name| matches!(name, "mx.utils.Delegate" | "mx.events.EventDispatcher"))
    {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "AS2 utility classes provide static factory/initialize methods and cannot be subclassed.",
        ));
    }
    if base.as_deref() == Some("flash.external.ExternalInterface") {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "ExternalInterface supplies static browser integration methods and cannot be subclassed.",
        ));
    }
    if matches!(
        base.as_deref(),
        Some("flash.display.FrameLabel" | "flash.display.Scene")
    ) {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "FrameLabel and Scene are final metadata types and cannot be subclassed.",
        ));
    }
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
        Some(
            "flash.geom.Transform"
                | "flash.geom.ColorTransform"
                | "flash.geom.Vector3D"
                | "flash.geom.Matrix3D"
                | "flash.geom.Orientation3D"
                | "flash.geom.Utils3D"
                | "flash.geom.PerspectiveProjection"
                | "fl.motion.MatrixTransformer"
                | "fl.motion.Color"
                | "fl.motion.CustomEase"
                | "flash.utils.Dictionary"
        )
    ) {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "Native subclass initialization for geometry/motion values and utilities, and Dictionary is outside this profile; use native instances through composition.",
        ));
    }
    if base.as_deref() == Some("flash.display.InteractiveObject") {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "InteractiveObject is an abstract native base class and cannot be extended directly; extend Sprite, MovieClip or SimpleButton.",
        ));
    }
    if matches!(
        base.as_deref(),
        Some(
            "flash.utils.ByteArray"
                | "flash.utils.Endian"
                | "flash.utils.CompressionAlgorithm"
                | "flash.display.Graphics"
        )
    ) {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "ByteArray indexed storage cannot initialize an existing native subclass receiver; Endian and CompressionAlgorithm supply constants and Graphics comes from its display owner. Inheritance from these built-ins is unsupported.",
        ));
    }
    if matches!(
        base.as_deref(),
        Some(
            "flash.display.Bitmap"
                | "flash.display.BitmapData"
                | "flash.display.BitmapDataChannel"
                | "flash.display.PixelSnapping"
                | "flash.display.PNGEncoderOptions"
                | "flash.display.JPEGEncoderOptions"
                | "flash.display.JPEGXREncoderOptions"
                | "flash.display.GraphicsSolidFill"
                | "flash.display.GraphicsGradientFill"
                | "flash.display.GraphicsBitmapFill"
                | "flash.display.GraphicsEndFill"
                | "flash.display.GraphicsStroke"
                | "flash.display.GraphicsPath"
                | "flash.display.GraphicsTrianglePath"
                | "flash.display.Stage"
                | "flash.display.StageScaleMode"
                | "flash.display.StageAlign"
                | "flash.display.StageDisplayState"
                | "flash.display.StageQuality"
        )
    ) {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "Bitmap, BitmapData and Stage native subclass initialization is outside this profile; construct or read the native facade and use composition. PixelSnapping, BitmapDataChannel and Stage enum classes provide constants; encoder options and IGraphicsData command classes are final.",
        ));
    }
    if base
        .as_ref()
        .is_some_and(|base| base.starts_with("flash.xml."))
    {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "flash.xml XMLNode, XMLDocument and XMLNodeType native subclass initialization is outside this profile; construct XMLNode or XMLDocument and use composition.",
        ));
    }
    if base
        .as_ref()
        .is_some_and(|base| base.starts_with("flash.ui."))
    {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "flash.ui Keyboard and Mouse classes are final and cannot be extended.",
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
        .is_some_and(|base| base.starts_with("flash.text.") && base != "flash.text.Font")
    {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "TextField, TextFormat, StyleSheet, TextRenderer and CSMSettings native subclass initialization is outside this profile; construct the native facade and use composition. Text enum classes supply constants only.",
        ));
    }
    if base
        .as_ref()
        .is_some_and(|base| base.starts_with("flash.media."))
    {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "Video and Sound-family native subclass initialization is outside this profile; construct native objects and use composition.",
        ));
    }
    if base.as_ref().is_some_and(|base| {
        base.starts_with("flash.net.")
            || base.starts_with("flash.globalization.")
            || matches!(
                base.as_str(),
                "flash.events.ProgressEvent"
                    | "flash.events.IOErrorEvent"
                    | "flash.events.HTTPStatusEvent"
                    | "flash.events.NetStatusEvent"
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
        Some(
            "flash.events.FocusEvent"
                | "flash.events.TextEvent"
                | "flash.events.ErrorEvent"
                | "flash.events.SecurityErrorEvent"
                | "flash.events.AsyncErrorEvent"
                | "flash.events.DataEvent"
                | "flash.events.StatusEvent"
                | "flash.events.ActivityEvent"
                | "flash.events.FullScreenEvent"
                | "flash.events.SyncEvent"
                | "flash.events.ContextMenuEvent"
                | "flash.events.IMEEvent"
                | "flash.events.UncaughtErrorEvent"
                | "flash.events.SampleDataEvent"
                | "flash.events.AccelerometerEvent"
                | "flash.events.GestureEvent"
                | "flash.events.TransformGestureEvent"
                | "flash.events.PressAndTapGestureEvent"
                | "flash.events.TouchEvent"
                | "flash.events.GesturePhase"
                | "flash.events.GeolocationEvent"
                | "flash.events.NetDataEvent"
                | "flash.events.StageOrientationEvent"
                | "flash.events.ThrottleEvent"
                | "flash.events.SoftKeyboardEvent"
                | "flash.events.ThrottleType"
                | "flash.events.SoftKeyboardTrigger"
                | "flash.events.TouchEventIntent"
                | "flash.events.InvokeEvent"
                | "flash.events.BrowserInvokeEvent"
                | "flash.events.FileListEvent"
                | "flash.events.OutputProgressEvent"
                | "flash.events.NativeWindowBoundsEvent"
                | "flash.events.NativeWindowDisplayStateEvent"
                | "flash.events.NativeDragEvent"
                | "flash.events.ScreenMouseEvent"
                | "flash.events.SQLEvent"
                | "flash.events.SQLErrorEvent"
                | "flash.events.SQLUpdateEvent"
                | "flash.events.StorageVolumeChangeEvent"
                | "flash.events.NativeProcessExitEvent"
                | "flash.events.LocationChangeEvent"
                | "flash.events.HTMLUncaughtScriptExceptionEvent"
                | "flash.events.MediaEvent"
                | "flash.events.DatagramSocketDataEvent"
                | "flash.events.ServerSocketConnectEvent"
                | "flash.events.DNSResolverEvent"
                | "flash.events.DRMAuthenticateEvent"
                | "flash.events.DeviceRotationEvent"
                | "flash.events.PermissionEvent"
                | "flash.events.RemoteNotificationEvent"
                | "flash.events.GameInputEvent"
                | "flash.events.VsyncStateChangeAvailabilityEvent"
                | "flash.events.AVDictionaryDataEvent"
                | "flash.events.AVHTTPStatusEvent"
                | "flash.events.AudioOutputChangeEvent"
                | "flash.events.DRMAuthenticationCompleteEvent"
                | "flash.events.DRMAuthenticationErrorEvent"
                | "flash.events.DRMDeviceGroupErrorEvent"
                | "flash.events.DRMDeviceGroupEvent"
                | "flash.events.DRMErrorEvent"
                | "flash.events.DRMLicenseRequestEvent"
                | "flash.events.DRMMetadataEvent"
                | "flash.events.DRMReturnVoucherCompleteEvent"
                | "flash.events.DRMReturnVoucherErrorEvent"
                | "flash.events.DRMStatusEvent"
                | "flash.events.NetFilterEvent"
                | "flash.events.NetMonitorEvent"
                | "flash.events.ShaderEvent"
                | "flash.events.StageVideoAvailabilityEvent"
                | "flash.events.StageVideoEvent"
                | "flash.events.UncaughtErrorEvents"
                | "flash.events.VideoEvent"
                | "flash.events.VideoTextureEvent"
        )
    ) {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "This native event family does not support subclass initialization; construct its event facade and use typed listeners through composition. GesturePhase supplies constants only.",
        ));
    }
    if base.as_ref().is_some_and(|base| {
        base.starts_with("flash.filesystem.")
            || (base.starts_with("flash.desktop.") && base != "flash.desktop.IFilePromise")
            || base.starts_with("flash.data.")
            || base.starts_with("flash.errors.")
            || base.starts_with("flash.permissions.")
            || base.starts_with("flash.notifications.")
            || base.starts_with("flash.sensors.")
            || base.starts_with("flash.concurrent.")
            || base.starts_with("flash.printing.")
            || base.starts_with("flash.profiler.")
            || base.starts_with("flash.sampler.")
            || (base.starts_with("flash.security.") && base != "flash.security.IURIDereferencer")
            || (base.starts_with("flash.html.") && base != "flash.html.HTMLHost")
            || base.starts_with("flash.net.dns.")
            || base.starts_with("flash.net.drm.")
            || base.starts_with("flash.system.")
            || base.starts_with("flash.display3D.")
            || base.starts_with("flash.ui.")
            || matches!(
                base.as_str(),
                "flash.accessibility.Accessibility"
                    | "flash.accessibility.AccessibilityProperties"
                    | "flash.display.FocusDirection"
                    | "flash.display.StageAspectRatio"
                    | "flash.display.StageOrientation"
                    | "flash.display.ScreenMode"
                    | "flash.display.Screen"
                    | "flash.display.NativeMenuItem"
                    | "flash.display.NativeMenu"
                    | "flash.display.NativeWindowType"
                    | "flash.display.NativeWindowSystemChrome"
                    | "flash.display.NativeWindowDisplayState"
                    | "flash.display.NativeWindowResize"
                    | "flash.display.NativeWindowRenderMode"
                    | "flash.display.NativeWindowInitOptions"
                    | "flash.display.NativeWindow"
                    | "flash.display.ActionScriptVersion"
                    | "flash.display.SWFVersion"
                    | "flash.display.ColorCorrection"
                    | "flash.display.ColorCorrectionSupport"
                    | "flash.display.MorphShape"
                    | "flash.display.AVM1Movie"
                    | "flash.display.Shader"
                    | "flash.display.ShaderData"
                    | "flash.display.ShaderInput"
                    | "flash.display.ShaderParameter"
                    | "flash.display.ShaderParameterType"
                    | "flash.display.ShaderPrecision"
                    | "flash.display.ShaderJob"
                    | "flash.display.GraphicsShaderFill"
                    | "flash.display.Stage3D"
                    | "flash.external.ExtensionContext"
                    | "flash.media.AudioPlaybackMode"
                    | "flash.media.CameraPosition"
                    | "flash.media.CameraRoll"
                    | "flash.media.CameraRollBrowseOptions"
                    | "flash.media.CameraUI"
                    | "flash.media.MediaPromise"
                    | "flash.media.MediaType"
                    | "flash.media.StageWebView"
                    | "flash.net.DatagramSocket"
                    | "flash.net.IPVersion"
                    | "flash.net.InterfaceAddress"
                    | "flash.net.NetworkInfo"
                    | "flash.net.NetworkInterface"
                    | "flash.net.ServerSocket"
                    | "flash.net.URLRequestDefaults"
                    | "flash.system.ImageDecodingPolicy"
                    | "flash.text.AutoCapitalize"
                    | "flash.text.ReturnKeyLabel"
                    | "flash.text.SoftKeyboardType"
                    | "flash.text.StageText"
                    | "flash.text.StageTextClearButtonMode"
                    | "flash.text.StageTextInitOptions"
                    | "flash.ui.GameInput"
                    | "flash.ui.GameInputControl"
                    | "flash.ui.GameInputDevice"
            )
    }) {
        return Err(Diagnostic::at(
            &header.script,
            0,
            "AIR native runtime, window, filesystem, SQL, sensor and constant classes do not support native subclass initialization in this profile; construct or obtain the native facade and use composition.",
        ));
    }
    let as2 = header.script.language == ScriptLanguage::ActionScript2;
    let mut p = frontend(&header.script, header.tokens.clone(), c.clone());
    p.pos = header.body;
    let mut fields = vec![];
    let mut methods = vec![];
    let mut seen = BTreeSet::new();
    while !p.is("}") && p.token().kind != Kind::End {
        let member_offset = p.token().offset;
        if p.eat(";") {
            continue;
        }
        if !as2
            && !header.is_interface
            && p.is("use")
            && p.peek(p.pos + 1)
                .is_some_and(|t| matches!(&t.kind, Kind::Ident(s) if s == "namespace"))
        {
            p.pos += 2;
            loop {
                let ns_name = qualified(&mut p)?;
                let uri = c.namespaces.get(&ns_name).cloned().ok_or_else(|| {
                    p.error(format!(
                        "Unresolved namespace '{ns_name}' in use namespace directive."
                    ))
                })?;
                if !c.open_namespaces.contains(&uri) {
                    c.open_namespaces.push(uri);
                }
                if !p.eat(",") {
                    break;
                }
            }
            p.semicolon()?;
            p.context = c.clone();
            continue;
        }
        let mut is_static = false;
        let mut override_ = false;
        let mut final_ = false;
        let mut native_ = false;
        let mut member_ns: Option<String> = None;
        let mut access = if as2 || header.is_interface {
            Access::Public
        } else {
            Access::Internal
        };
        let mut access_seen = false;
        if header.is_interface
            && !(as2 && p.is("public"))
            && matches!(
                p.token_text().as_str(),
                "public"
                    | "static"
                    | "override"
                    | "private"
                    | "protected"
                    | "internal"
                    | "final"
                    | "native"
            )
        {
            return Err(p.error(
                "Interface signatures cannot have access, static, final or override modifiers.",
            ));
        }
        loop {
            let next_access = match p.token_text().as_str() {
                "public" => Some(Access::Public),
                "private" => Some(Access::Private),
                "protected" => Some(Access::Protected),
                "internal" => Some(Access::Internal),
                _ => None,
            };
            if let Some(next) = next_access {
                if access_seen || (as2 && matches!(next, Access::Protected | Access::Internal)) {
                    return Err(p.error("Duplicate or unsupported member access modifier."));
                }
                p.pos += 1;
                access = next;
                access_seen = true;
            } else if p.eat("static") {
                if is_static {
                    return Err(p.error("Duplicate static modifier."));
                }
                is_static = true;
            } else if p.eat("override") {
                if as2 || override_ {
                    return Err(p.error("AS3 override cannot be repeated; AS2 overrides by name without the keyword."));
                }
                override_ = true;
            } else if p.eat("final") {
                if as2 || final_ {
                    return Err(p.error("AS3 final cannot be repeated and is not an AS2 modifier."));
                }
                final_ = true;
            } else if !as2 && p.eat("native") {
                if native_ {
                    return Err(p.error("Duplicate native modifier."));
                }
                native_ = true;
            } else if !as2
                && !header.is_interface
                && matches!(p.token().kind, Kind::Ident(_))
                && !matches!(
                    p.token_text().as_str(),
                    "var" | "const" | "function" | "namespace" | "use"
                )
                && p.peek(p.pos + 1).is_some_and(|next_tok| {
                    matches!(
                        &next_tok.kind,
                        Kind::Ident(s)
                            if matches!(
                                s.as_str(),
                                "var"
                                    | "const"
                                    | "function"
                                    | "static"
                                    | "override"
                                    | "final"
                                    | "native"
                                    | "namespace"
                            )
                    )
                })
            {
                if access_seen || member_ns.is_some() {
                    return Err(p.error("Duplicate or unsupported member access modifier."));
                }
                let ns_ident = p.ident()?;
                let Some(uri) = c.namespaces.get(&ns_ident).cloned() else {
                    return Err(p.error(format!("Unresolved namespace '{ns_ident}'.")));
                };
                member_ns = Some(uri);
                access = Access::Public;
                access_seen = true;
            } else {
                break;
            }
        }
        if header.is_interface && (is_static || access != Access::Public) {
            return Err(p.error("Interface methods must be public instance signatures."));
        }
        if !as2 && !header.is_interface && p.eat("namespace") {
            if override_ || final_ || native_ {
                return Err(p.error("Namespace declarations cannot use override, final or native."));
            }
            let ns_name = p.ident()?;
            if invalid_binding(&ns_name, false) {
                return Err(p.error("Invalid or reserved namespace name."));
            }
            let default_uri = format!("{}:{}/{ns_name}", header.package, header.name);
            let uri = parse_namespace_uri_literal(&mut p, default_uri, &c.namespaces)?;
            p.semicolon()?;
            c.namespaces.insert(ns_name.clone(), uri.clone());
            p.context = c.clone();
            if seen
                .iter()
                .any(|(old, old_ns, _)| old == &ns_name && old_ns == &member_ns)
                || !seen.insert((ns_name.clone(), member_ns.clone(), MethodKind::Method))
            {
                return Err(p.error(format!("Duplicate class member '{ns_name}'.")));
            }
            fields.push(Field {
                offset: member_offset,
                name: ns_name,
                ty: Some("Namespace".into()),
                value: Some(Expr::Call(
                    Box::new(Expr::Name("Namespace".into())),
                    vec![Expr::Literal(format!("{uri:?}"))],
                    p.token().offset,
                )),
                constant: true,
                is_static: true,
                access,
                namespace: member_ns,
            });
        } else if p.is("var") || p.is("const") {
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
            if override_ || final_ || native_ {
                return Err(p.error("Fields cannot use override, final or native."));
            }
            let Stmt::Var(vars) = p.variables()? else {
                unreachable!()
            };
            p.semicolon()?;
            for (name, ty, value) in vars {
                if seen
                    .iter()
                    .any(|(old, old_ns, _)| old == &name && old_ns == &member_ns)
                    || !seen.insert((name.clone(), member_ns.clone(), MethodKind::Method))
                {
                    return Err(p.error(format!("Duplicate class member '{name}'.")));
                }
                if header.intrinsic && value.is_some() {
                    return Err(p.error(
                        "AS2 intrinsic class declarations cannot include field initializers.",
                    ));
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
                    offset: member_offset,
                    name,
                    ty,
                    value,
                    constant,
                    is_static,
                    access,
                    namespace: member_ns.clone(),
                });
            }
        } else if p.is("function") {
            let saved_open = p.context.open_namespaces.clone();
            let (mut f, accessor) = p.project_function(
                true,
                true,
                header.is_interface || header.intrinsic,
                (header.intrinsic && !header.is_interface) || native_,
            )?;
            let method_open_namespaces = p.context.open_namespaces.clone();
            p.context.open_namespaces = saved_open;
            let kind = match accessor.as_deref() {
                Some("get") => MethodKind::Get,
                Some("set") => MethodKind::Set,
                _ => MethodKind::Method,
            };
            let name = f.name.as_ref().unwrap();
            if header.is_interface && name == header.name.rsplit('.').next().unwrap() {
                return Err(p.error("Interfaces cannot declare constructors."));
            }
            if name == header.name.rsplit('.').next().unwrap() && kind == MethodKind::Method {
                if (access_seen && access != Access::Public) || member_ns.is_some() {
                    return Err(p.error("Constructors must be public."));
                }
                if final_ {
                    return Err(p.error("Constructors cannot be final."));
                }
                access = Access::Public;
            }
            if final_ && is_static {
                return Err(p.error("Static methods cannot use final in this profile."));
            }
            if kind == MethodKind::Get
                && (!f.params.is_empty()
                    || f.rest.is_some()
                    || (f.returns.as_deref() == Some("void")
                        || (as2 && f.returns.as_deref() == Some("Void"))))
            {
                return Err(p.error("A getter requires no parameters and cannot return void."));
            }
            if kind == MethodKind::Set {
                if f.params.len() != 1
                    || f.rest.is_some()
                    || f.defaults.iter().any(Option::is_some)
                    || f.returns
                        .as_deref()
                        .is_some_and(|ty| ty != "void" && !(as2 && ty == "Void"))
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
            if seen.contains(&(name.clone(), member_ns.clone(), kind))
                || seen.iter().any(|(old, old_ns, old_kind)| {
                    old == name
                        && old_ns == &member_ns
                        && (*old_kind == MethodKind::Method || kind == MethodKind::Method)
                })
            {
                return Err(p.error(format!("Duplicate class member '{name}'.")));
            }
            seen.insert((name.clone(), member_ns.clone(), kind));
            methods.push(Method {
                function: f,
                is_static,
                override_,
                access,
                namespace: member_ns,
                open_namespaces: method_open_namespaces,
                final_,
                kind,
            });
        } else {
            return Err(p.error(
                "Expected a supported field, method, get/set accessor or namespace declaration.",
            ));
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
pub(super) fn prefix(mut e: Diagnostic, path: &str) -> Diagnostic {
    if e.path.is_none() {
        e.path = Some(path.to_owned());
    }
    e.message = format!("{path}: {}", e.message);
    e
}
