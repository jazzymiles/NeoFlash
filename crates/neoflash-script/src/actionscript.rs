//! A tokenizing/recursive-descent + Pratt frontend, not a text substitution pass.
pub(crate) mod analysis;
mod includes;
pub(crate) mod semantic;

use crate::Diagnostic;
use neoflash_core::{Script, ScriptLanguage};
use std::collections::HashMap;
pub(crate) mod ast;
mod completion;
#[cfg(test)]
mod control_tests;
pub(crate) mod diagnostics;
mod emitter;
mod emitter_expr;
pub(crate) mod lexer;
pub(crate) mod parser;
mod parser_expr;
pub(crate) mod project;
pub(crate) mod scope;
mod xml_literals;

use self::{
    ast::{
        Case, CatchClause, ControlTarget, Expr, Function, LegacyTrigger, PropertyKey, Stmt,
        constant_default,
    },
    emitter::Emitter,
    lexer::{Kind, Lexer, Token, TokenCursor},
    parser::Frontend,
};

#[derive(Clone, Default)]
pub(crate) struct CompileContext {
    pub(crate) legacy_case: bool,
    pub(crate) mapping_prefix: Option<String>,
    pub(crate) types: std::collections::BTreeMap<String, String>,
    pub(crate) intrinsic_types: std::collections::BTreeSet<String>,
    pub(crate) functions: std::collections::BTreeMap<String, String>,
    pub(crate) interfaces: std::collections::BTreeSet<String>,
    pub(crate) namespaces: std::collections::BTreeMap<String, String>,
    pub(crate) open_namespaces: Vec<String>,
    pub(crate) namespace_members: std::collections::BTreeSet<(String, String)>,
    pub(crate) members: std::collections::BTreeMap<String, Option<String>>,
    pub(crate) static_members: std::collections::BTreeMap<String, String>,
    pub(crate) as2_private: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    pub(crate) as3_class_consts: std::collections::BTreeMap<(String, String), Expr>,
    pub(crate) static_method: bool,
    pub(crate) class_name: Option<String>,
    pub(crate) method_id: Option<usize>,
    pub(crate) project: bool,
}

impl CompileContext {
    fn intrinsic_type(&self, name: &str, as3: bool) -> bool {
        (matches!(name, "Date" | "XML")
            || (as3
                && matches!(
                    name,
                    "RegExp" | "JSON" | "Vector" | "XMLList" | "Namespace" | "QName"
                ))
            || (!as3 && name == "XMLNode"))
            && (!self.project || self.intrinsic_types.contains(name))
    }
    fn core_type(&self, name: &str) -> bool {
        (matches!(
            name,
            "Number"
                | "String"
                | "Boolean"
                | "int"
                | "uint"
                | "Function"
                | "Class"
                | "Object"
                | "Array"
        ) || is_error_type(name))
            && self.types.get(name).is_none_or(|resolved| resolved == name)
    }
    fn utility_function(&self, name: &str, as3: bool) -> bool {
        ((as3
            && matches!(
                name,
                "encodeURI"
                    | "encodeURIComponent"
                    | "decodeURI"
                    | "decodeURIComponent"
                    | "escape"
                    | "unescape"
                    | "isXMLName"
            ))
            || (!as3
                && matches!(
                    name,
                    "random"
                        | "escape"
                        | "unescape"
                        | "chr"
                        | "ord"
                        | "length"
                        | "substring"
                        | "mbchr"
                        | "mbord"
                        | "mblength"
                        | "mbsubstring"
                )))
            && !self.types.contains_key(name)
            && !self.functions.contains_key(name)
    }
    fn numeric_conversion(&self, name: &str, as3: bool) -> bool {
        (name == "int" || (as3 && name == "uint")) && !self.types.contains_key(name)
    }
    fn canonical_type(&self, name: &str) -> String {
        if let Some(element) = vector_element(name) {
            format!("Vector.<{}>", self.canonical_type(element))
        } else {
            self.types.get(name).cloned().unwrap_or_else(|| name.into())
        }
    }
}

fn vector_element(name: &str) -> Option<&str> {
    name.strip_prefix("Vector.<")
        .and_then(|name| name.strip_suffix('>'))
}

fn is_timeline_method(name: &str) -> bool {
    matches!(
        name,
        "play" | "stop" | "gotoAndPlay" | "gotoAndStop" | "nextFrame" | "prevFrame"
    )
}

fn is_as2_global_prop(name: &str) -> bool {
    matches!(
        name,
        "_quality" | "_highquality" | "_focusrect" | "_soundbuftime"
    )
}

pub(super) fn is_as2_frame_global(name: &str) -> bool {
    matches!(
        name,
        "getURL"
            | "fscommand"
            | "loadMovie"
            | "loadMovieNum"
            | "unloadMovie"
            | "unloadMovieNum"
            | "loadVariables"
            | "loadVariablesNum"
            | "print"
            | "printNum"
            | "printAsBitmap"
            | "printAsBitmapNum"
            | "asfunction"
            | "MMExecute"
    )
}

fn qualified_expr_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(n) => Some(n.clone()),
        Expr::Member(base, n) => Some(format!("{}.{}", qualified_expr_name(base)?, n)),
        _ => None,
    }
}

fn is_as2_legacy_instanceof_type(name: &str) -> bool {
    matches!(
        name,
        "MovieClip"
            | "Button"
            | "TextField"
            | "TextField.StyleSheet"
            | "TextFormat"
            | "TextSnapshot"
            | "Video"
            | "LoadVars"
            | "Color"
            | "Sound"
            | "MovieClipLoader"
            | "SharedObject"
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
    )
}

fn is_as3_wave7_type(name: &str) -> bool {
    matches!(
        name,
        "LoaderContext"
            | "flash.system.LoaderContext"
            | "JPEGLoaderContext"
            | "flash.system.JPEGLoaderContext"
            | "ApplicationDomain"
            | "flash.system.ApplicationDomain"
            | "SecurityDomain"
            | "flash.system.SecurityDomain"
            | "Capabilities"
            | "flash.system.Capabilities"
            | "Context3D"
            | "flash.display3D.Context3D"
            | "VideoTexture"
            | "flash.display3D.textures.VideoTexture"
            | "Texture"
            | "flash.display3D.textures.Texture"
            | "CubeTexture"
            | "flash.display3D.textures.CubeTexture"
            | "VideoStreamSettings"
            | "flash.media.VideoStreamSettings"
            | "H264VideoStreamSettings"
            | "flash.media.H264VideoStreamSettings"
            | "VP6VideoStreamSettings"
            | "flash.media.VP6VideoStreamSettings"
            | "H264Level"
            | "flash.media.H264Level"
            | "H264Profile"
            | "flash.media.H264Profile"
            | "VideoCodec"
            | "flash.media.VideoCodec"
            | "AudioDecoder"
            | "flash.media.AudioDecoder"
            | "AudioDeviceManager"
            | "flash.media.AudioDeviceManager"
            | "AudioOutputChangeReason"
            | "flash.media.AudioOutputChangeReason"
            | "AudioOutputChangeEvent"
            | "flash.events.AudioOutputChangeEvent"
            | "Responder"
            | "flash.net.Responder"
            | "NetConnection"
            | "flash.net.NetConnection"
            | "NetStream"
            | "flash.net.NetStream"
            | "NetStreamPlayOptions"
            | "flash.net.NetStreamPlayOptions"
            | "NetStreamPlayTransitions"
            | "flash.net.NetStreamPlayTransitions"
            | "SharedObject"
            | "flash.net.SharedObject"
            | "ColorCorrection"
            | "flash.display.ColorCorrection"
            | "ColorCorrectionSupport"
            | "flash.display.ColorCorrectionSupport"
            | "AVM1Movie"
            | "flash.display.AVM1Movie"
            | "InteractiveObject"
            | "flash.display.InteractiveObject"
            | "Stage"
            | "flash.display.Stage"
            | "Rectangle"
            | "flash.geom.Rectangle"
            | "ByteArray"
            | "flash.utils.ByteArray"
            | "Proxy"
            | "flash.utils.Proxy"
            | "flash_proxy"
            | "flash.utils.flash_proxy"
            | "object_proxy"
            | "flash.utils.object_proxy"
            | "mx.utils.object_proxy"
            | "ObjectProxy"
            | "mx.utils.ObjectProxy"
    )
}

fn is_as3_wave7_constructible(name: &str) -> bool {
    matches!(
        name,
        "LoaderContext"
            | "flash.system.LoaderContext"
            | "JPEGLoaderContext"
            | "flash.system.JPEGLoaderContext"
            | "ApplicationDomain"
            | "flash.system.ApplicationDomain"
            | "SecurityDomain"
            | "flash.system.SecurityDomain"
            | "Context3D"
            | "flash.display3D.Context3D"
            | "VideoTexture"
            | "flash.display3D.textures.VideoTexture"
            | "Texture"
            | "flash.display3D.textures.Texture"
            | "CubeTexture"
            | "flash.display3D.textures.CubeTexture"
            | "VideoStreamSettings"
            | "flash.media.VideoStreamSettings"
            | "H264VideoStreamSettings"
            | "flash.media.H264VideoStreamSettings"
            | "VP6VideoStreamSettings"
            | "flash.media.VP6VideoStreamSettings"
            | "AudioOutputChangeEvent"
            | "flash.events.AudioOutputChangeEvent"
            | "Responder"
            | "flash.net.Responder"
            | "NetConnection"
            | "flash.net.NetConnection"
            | "NetStream"
            | "flash.net.NetStream"
            | "NetStreamPlayOptions"
            | "flash.net.NetStreamPlayOptions"
            | "AVM1Movie"
            | "flash.display.AVM1Movie"
            | "InteractiveObject"
            | "flash.display.InteractiveObject"
            | "Rectangle"
            | "flash.geom.Rectangle"
            | "ByteArray"
            | "flash.utils.ByteArray"
            | "Proxy"
            | "flash.utils.Proxy"
            | "ObjectProxy"
            | "mx.utils.ObjectProxy"
    )
}

pub(super) fn parse_as2_level(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("_level")?;
    if rest == "0" {
        return Some(0);
    }
    if rest.is_empty() || rest.starts_with('0') || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse::<u32>().ok()
}

pub(super) fn fnv1a64(text: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

const FAMILY_METHODS: &[&str] = &[
    "call",
    "apply",
    "push",
    "pop",
    "shift",
    "unshift",
    "join",
    "slice",
    "concat",
    "reverse",
    "splice",
    "sort",
    "sortOn",
    "indexOf",
    "lastIndexOf",
    "map",
    "filter",
    "forEach",
    "every",
    "some",
    "charAt",
    "charCodeAt",
    "substring",
    "substr",
    "split",
    "toLowerCase",
    "toUpperCase",
    "toLocaleLowerCase",
    "toLocaleUpperCase",
    "localeCompare",
    "replace",
    "toString",
    "valueOf",
    "fromCharCode",
    "toFixed",
    "toExponential",
    "toPrecision",
    "match",
    "search",
    "exec",
    "test",
    "UTC",
    "parse",
    "stringify",
    "getDate",
    "getDay",
    "getFullYear",
    "getHours",
    "getMilliseconds",
    "getMinutes",
    "getMonth",
    "getSeconds",
    "getTime",
    "getTimezoneOffset",
    "getUTCDate",
    "getUTCDay",
    "getUTCFullYear",
    "getUTCHours",
    "getUTCMilliseconds",
    "getUTCMinutes",
    "getUTCMonth",
    "getUTCSeconds",
    "getUTCYear",
    "getYear",
    "setDate",
    "setFullYear",
    "setHours",
    "setMilliseconds",
    "setMinutes",
    "setMonth",
    "setSeconds",
    "setTime",
    "setUTCDate",
    "setUTCFullYear",
    "setUTCHours",
    "setUTCMilliseconds",
    "setUTCMinutes",
    "setUTCMonth",
    "setUTCSeconds",
    "setYear",
    "toDateString",
    "toTimeString",
    "toUTCString",
    "toLocaleDateString",
    "toLocaleTimeString",
    "toLocaleString",
    "toJSON",
    "length",
    "child",
    "children",
    "elements",
    "descendants",
    "attribute",
    "attributes",
    "text",
    "parent",
    "localName",
    "nodeKind",
    "hasSimpleContent",
    "hasComplexContent",
    "copy",
    "appendChild",
    "prependChild",
    "setChildren",
    "normalize",
    "toXMLString",
    "settings",
    "setSettings",
    "defaultSettings",
    "parseXML",
    "insertAt",
    "removeAt",
    "getStackTrace",
    "hasOwnProperty",
    "propertyIsEnumerable",
    "setPropertyIsEnumerable",
    "isPrototypeOf",
    "childIndex",
    "comments",
    "processingInstructions",
    "addNamespace",
    "removeNamespace",
    "inScopeNamespaces",
    "namespaceDeclarations",
    "namespace",
    "name",
    "setLocalName",
    "setName",
    "setNamespace",
    "insertChildAfter",
    "insertChildBefore",
    "contains",
    "createElement",
    "createTextNode",
    "hasChildNodes",
    "insertBefore",
    "removeNode",
    "cloneNode",
    "load",
    "getBytesLoaded",
    "getBytesTotal",
];

fn is_family_method(name: &str) -> bool {
    FAMILY_METHODS.contains(&name)
}

fn is_family_method_for(name: &str, legacy_case: bool) -> bool {
    is_family_method(name)
        || (legacy_case && FAMILY_METHODS.iter().any(|m| m.eq_ignore_ascii_case(name)))
}

fn resolve_legacy_event_name(
    clip_event: bool,
    name: &str,
    legacy_case: bool,
) -> Option<&'static str> {
    const CLIP_EVENTS: &[&str] = &[
        "load",
        "enterFrame",
        "unload",
        "data",
        "mouseDown",
        "mouseMove",
        "mouseUp",
        "keyDown",
        "keyUp",
    ];
    const BUTTON_EVENTS: &[&str] = &[
        "press",
        "release",
        "releaseOutside",
        "rollOver",
        "rollOut",
        "dragOver",
        "dragOut",
        "keyPress",
    ];
    let table = if clip_event {
        CLIP_EVENTS
    } else {
        BUTTON_EVENTS
    };
    table.iter().copied().find(|candidate| {
        if legacy_case {
            candidate.eq_ignore_ascii_case(name)
        } else {
            *candidate == name
        }
    })
}

fn validate_flash_key_press(spec: &str, legacy_case: bool) -> Option<String> {
    const SPECIAL_KEYS: &[&str] = &[
        "<Left>",
        "<Right>",
        "<Up>",
        "<Down>",
        "<Enter>",
        "<Space>",
        "<Tab>",
        "<Escape>",
        "<Backspace>",
        "<Delete>",
        "<Home>",
        "<End>",
        "<PageUp>",
        "<PageDown>",
        "<Insert>",
    ];
    if let Some(&canonical) = SPECIAL_KEYS.iter().find(|token| {
        if legacy_case {
            token.eq_ignore_ascii_case(spec)
        } else {
            **token == spec
        }
    }) {
        return Some(canonical.to_owned());
    }
    if spec.len() >= 2 && spec.starts_with('<') && spec.ends_with('>') {
        return None;
    }
    if spec.chars().count() == 1 {
        Some(spec.to_owned())
    } else {
        None
    }
}

fn flash4_property_index(name: &str, legacy_case: bool) -> Option<u32> {
    (0..22).find(|&index| {
        neoflash_core::as2_strings::property_name(index).is_some_and(|prop| {
            if legacy_case {
                prop.eq_ignore_ascii_case(name)
            } else {
                prop == name
            }
        })
    })
}

fn canonical_legacy_callback(name: &str, legacy_case: bool) -> Option<&'static str> {
    const LEGACY_CALLBACKS: &[&str] = &[
        "onEnterFrame",
        "onPress",
        "onRelease",
        "onReleaseOutside",
        "onRollOver",
        "onRollOut",
        "onMouseDown",
        "onMouseMove",
        "onMouseUp",
        "onKeyDown",
        "onKeyUp",
        "onDragOver",
        "onDragOut",
        "onUnload",
    ];
    LEGACY_CALLBACKS.iter().copied().find(|candidate| {
        if legacy_case {
            candidate.eq_ignore_ascii_case(name)
        } else {
            *candidate == name
        }
    })
}

fn literal_property_name(expr: &Expr) -> Option<String> {
    let Expr::Literal(source) = expr else {
        return None;
    };
    let allocator = oxc_allocator::Allocator::default();
    let value = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::cjs())
        .parse_expression()
        .ok()?;
    match value {
        oxc_ast::ast::Expression::StringLiteral(literal) if !literal.lone_surrogates => {
            Some(literal.value.to_string())
        }
        _ => None,
    }
}

fn is_global(name: &str) -> bool {
    matches!(
        name,
        "Math"
            | "Number"
            | "String"
            | "Array"
            | "Boolean"
            | "parseInt"
            | "parseFloat"
            | "isNaN"
            | "isFinite"
            | "NaN"
            | "Infinity"
            | "undefined"
            | "true"
            | "false"
            | "null"
            | "trace"
    ) || is_timeline_method(name)
        || is_error_type(name)
}
fn is_error_type(name: &str) -> bool {
    matches!(
        name,
        "Error"
            | "TypeError"
            | "RangeError"
            | "ReferenceError"
            | "SyntaxError"
            | "EvalError"
            | "URIError"
            | "ArgumentError"
            | "DefinitionError"
            | "SecurityError"
            | "VerifyError"
    )
}
fn is_unsupported_keyword(name: &str) -> bool {
    matches!(
        name,
        "package"
            | "class"
            | "import"
            | "interface"
            | "extends"
            | "implements"
            | "public"
            | "private"
            | "protected"
            | "internal"
            | "namespace"
            | "use"
            | "with"
            | "try"
            | "catch"
            | "finally"
            | "throw"
            | "switch"
            | "case"
            | "default"
            | "new"
            | "delete"
            | "void"
            | "is"
            | "as"
            | "instanceof"
            | "in"
            | "each"
            | "on"
            | "onClipEvent"
            | "const"
            | "let"
            | "do"
            | "super"
            | "get"
            | "set"
    )
}

fn invalid_binding(name: &str, allow_globals: bool) -> bool {
    (is_global(name)
        && (!allow_globals
            || matches!(
                name,
                "NaN" | "Infinity" | "undefined" | "true" | "false" | "null"
            )))
        || is_unsupported_keyword(name)
        || name.starts_with("__nf")
        || matches!(
            name,
            "this"
                | "function"
                | "return"
                | "var"
                | "if"
                | "else"
                | "while"
                | "for"
                | "break"
                | "continue"
                | "arguments"
                | "eval"
                | "await"
                | "yield"
        )
}

fn binding_power(operator: &str) -> Option<(u8, u8, bool)> {
    Some(match operator {
        "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>=" | ">>>="
        | "&&=" | "||=" => (1, 1, true),
        "||" | "or" => (3, 4, false),
        "&&" | "and" => (5, 6, false),
        "|" => (7, 8, false),
        "^" => (9, 10, false),
        "&" => (11, 12, false),
        "==" | "!=" | "===" | "!==" | "eq" | "ne" | "<>" => (13, 14, false),
        "<" | "<=" | ">" | ">=" | "in" | "lt" | "le" | "gt" | "ge" => (15, 16, false),
        "<<" | ">>" | ">>>" => (17, 18, false),
        "+" | "-" | "add" => (19, 20, false),
        "*" | "/" | "%" => (21, 22, false),
        _ => return None,
    })
}

pub(crate) fn compile(script: &Script) -> Result<String, Diagnostic> {
    if script.language == ScriptLanguage::ActionScript1 {
        let mut s = script.clone();
        s.language = ScriptLanguage::ActionScript2;
        return compile_legacy(&s).map_err(|mut d| {
            d.language = ScriptLanguage::ActionScript1;
            d
        });
    }
    compile_context(script, CompileContext::default())
}
pub(crate) fn compile_mapped(script: &Script, prefix: &str) -> Result<String, Diagnostic> {
    let mut script = script.clone();
    let legacy = script.language == ScriptLanguage::ActionScript1;
    if legacy {
        script.language = ScriptLanguage::ActionScript2;
    }
    compile_context(
        &script,
        CompileContext {
            legacy_case: legacy,
            mapping_prefix: Some(prefix.to_owned()),
            ..CompileContext::default()
        },
    )
}
pub(crate) fn compile_legacy(script: &Script) -> Result<String, Diagnostic> {
    compile_context(
        script,
        CompileContext {
            legacy_case: true,
            ..CompileContext::default()
        },
    )
}

// One helper profile for standalone handlers and project methods. Helpers are
// lexically captured per compiled unit; JavaScript/TypeScript remain untouched.
pub(crate) fn embedded_helpers() -> String {
    let mut reflection_types = project::BUILTINS
        .iter()
        .filter(|name| {
            !name.starts_with("mx.transitions.")
                && (!project::AS2_BUILTINS.contains(name)
                    || matches!(
                        **name,
                        "flash.net.FileReference" | "flash.net.FileReferenceList"
                    ))
        })
        .copied()
        .collect::<Vec<_>>();
    reflection_types.extend([
        "Date",
        "RegExp",
        "JSON",
        "XML",
        "XMLList",
        "Namespace",
        "QName",
    ]);
    let default_native_types = format!("const __nfDefaultNativeTypes = {reflection_types:?};");
    [
        default_native_types.as_str(),
        include_str!("actionscript/exceptions.js"),
        include_str!("actionscript/intrinsics.js"),
        include_str!("actionscript/functions.js"),
        include_str!("actionscript/collections.js"),
        include_str!("actionscript/dates.js"),
        include_str!("actionscript/regexp.js"),
        include_str!("actionscript/strings.js"),
        include_str!("actionscript/vectors.js"),
        include_str!("actionscript/dictionary.js"),
        include_str!("actionscript/containers.js"),
        include_str!("actionscript/json.js"),
        include_str!("actionscript/project-runtime.js"),
        r#"
// XML state/settings belong to the publication host, shared by every source unit.
function __nfXMLClass(name,as3) {
    if(typeof __nfXML==='undefined'||!__nfXML)throw new __nfTypeError('XML service is not available in this publication.');
    const key=!as3&&name==='XML'?'LegacyXML':name;
    const ctor=__nfXML[key];
    if(typeof ctor!=='function')throw new __nfTypeError('Unsupported XML native type '+name);
    return ctor;
}
function __nfXMLCast(name,value) {
    if(value==null)return null;
    if(__nfXMLClass(name,true).__nfIs(value))return value;
    throw new __nfTypeError('AS3 '+name+' value required; use the XML/XMLList constructor for conversion.');
}
function __nfXMLSwitch(value) {
    if(typeof __nfXML!=='undefined'&&__nfXML) {
        const kind=__nfXML.kind(value);
        if(kind==='xml'||kind==='list'||kind==='namespace'||kind==='qname')throw new __nfTypeError('E4X XML switch equality is not implemented; explicitly convert the selector and case values.');
    }
    return value;
}
function __nfXMLBinary(op,left,right) {
    if(typeof __nfXML!=='undefined'&&__nfXML) {
        const a=__nfXML.kind(left),b=__nfXML.kind(right);
        if(op==='+') {
            if((a==='xml'||a==='list')&&(b==='xml'||b==='list'))return __nfXML.concat(left,right);
            if(a==='xml'||a==='list'||b==='xml'||b==='list')return __nfString(left)+__nfString(right);
        } else if(a==='xml'||a==='list'||b==='xml'||b==='list'||a==='namespace'||a==='qname'||b==='namespace'||b==='qname') {
            if(op==='=='||op==='!=') {
                const eq=__nfXML.equals(left,right);
                return op==='=='?eq:!eq;
            }
            if(op==='==='||op==='!==') {
                const eq=__nfXML.strictEquals(left,right);
                return op==='==='?eq:!eq;
            }
        }
    }
    switch(op){case '+':return __nfArrayDefaultString(left,true,false)+__nfArrayDefaultString(right,true,false);case '==':return left==right;case '!=':return left!=right;case '===':return left===right;case '!==':return left!==right;}
    throw new __nfTypeError('Unsupported XML operator.');
}
const __nfPrimitiveTypes = new Map();
const __nfObjectType=Object, __nfFunctionType=Function, __nfArrayType=Array;
function __nfPrimitiveDescriptor(name) {
    if (__nfPrimitiveTypes.has(name)) return __nfPrimitiveTypes.get(name);
    const nullable=value=>value==null?null:value;
    let token,coerce,defaultValue=null;
    switch(name) {
      case 'int':token=Symbol.for('neoflash.vector.element.int');coerce=value=>__nfNumber(value)|0;defaultValue=0;break;
      case 'uint':token=Symbol.for('neoflash.vector.element.uint');coerce=value=>__nfNumber(value)>>>0;defaultValue=0;break;
      case 'Number':token=__nfNumber;coerce=__nfNumber;defaultValue=0;break;
      case 'Boolean':token=__nfBoolean;coerce=__nfBoolean;defaultValue=false;break;
      case 'String':token=__nfString;coerce=value=>value==null?null:__nfString(value);break;
      case '*':token=Symbol.for('neoflash.vector.element.any');coerce=nullable;break;
      case 'Object':token=__nfObjectType;coerce=nullable;break;
      case 'Function':token=__nfFunctionType;coerce=value=>{if(value==null)return null;if(typeof value==='function'&&__nfNumericClassName(value)===null)return value;throw new __nfTypeError('AS3 Function value required');};break;
      case 'Class':token=Symbol.for('neoflash.vector.element.Class');coerce=__nfClassCast;break;
      case 'Array':token=__nfArrayType;coerce=value=>{if(value==null)return null;if(__nfArrayIsArray(value))return value;throw new __nfTypeError('AS3 Array value required');};break;
      case 'XML':case 'XMLList':case 'Namespace':case 'QName':token=__nfXMLClass(name,true);coerce=value=>__nfXMLCast(name,value);break;
      case 'Date':token=__nfDateNative;coerce=__nfDateCast;break;
      case 'RegExp':token=Symbol.for('neoflash.vector.element.RegExp');coerce=value=>{if(value==null)return null;if(__nfRegExpIs(value))return value;throw new __nfTypeError('AS3 RegExp value required');};break;
      default:
        if(!Object.hasOwn(__nfErrorTypes,name))throw new __nfTypeError('Unsupported Vector element type '+name);
        token=Symbol.for('neoflash.vector.element.error.'+name);
        coerce=value=>{if(value==null)return null;if(__nfCatchType(value,name))return value;throw new __nfTypeError('AS3 '+name+' value required');};
    }
    const descriptor=Object.freeze({token,coerce,defaultValue,name});__nfPrimitiveTypes.set(name,descriptor);return descriptor;
}
function __nfVectorDescriptor(element) {
    const token=__nfVectorType(element);
    return Object.freeze({token,coerce:value=>__nfVectorCast(value,token),defaultValue:null,name:'Vector.<'+element.name+'>'});
}
function __nfFamilyValue(value, as3) {
    value = __nfFunctionMethodValue(value);
    value = __nfArrayMethodValue(value);
    value = __nfStringMethodValue(value);
    value = __nfDateMethodValue(value);
    return as3 ? __nfMethodValue(value) : value;
}
function __nfFamilyPropertyValue(receiver, key, as3, caller, legacyCase) {
    return __nfFamilyValue(__nfContainerRef(receiver,key,as3,caller,legacyCase).get(), as3);
}
function __nfMemberMethod(receiver, key, as3, caller, legacyCase) {
    return __nfContainerMethod(receiver,key,as3,caller,legacyCase);
}
const __nfNumber1 = (() => {
    const fn = function Number(value) {
        if (new.target) return __nfBoxCreate('Number', arguments.length ? [value] : []);
        if (!arguments.length || value === undefined || value === null) return 0;
        return __nfNumber(value);
    };
    Object.defineProperties(fn, Object.getOwnPropertyDescriptors(__nfNumber));
    return fn;
})();
const __nfNumber2 = (() => {
    const fn = function Number(value) {
        if (new.target) return __nfBoxCreate('Number', arguments.length ? [value] : []);
        if (!arguments.length) return 0;
        if (value === undefined || value === null) return NaN;
        return __nfNumber(value);
    };
    Object.defineProperties(fn, Object.getOwnPropertyDescriptors(__nfNumber));
    return fn;
})();
function __nfLegacyNumOperand(val, legacyCase) {
    if (val === undefined || val === null) return legacyCase ? 0 : NaN;
    if (typeof val === 'object') return val;
    return __nfNumber(val);
}
function __nfLegacyBinary(op, left, right, legacyCase) {
    const isAs1 = Boolean(legacyCase) || (legacyCase === undefined && typeof __nfLegacyCase !== 'undefined' && Boolean(__nfLegacyCase));
    if (op === '+') {
        const lArr = __nfArrayDefaultString(left, false, isAs1);
        const rArr = __nfArrayDefaultString(right, false, isAs1);
        if (typeof lArr === 'string' || typeof rArr === 'string') {
            if (isAs1) {
                return (lArr === undefined ? '' : lArr) + (rArr === undefined ? '' : rArr);
            }
            return lArr + rArr;
        }
        if ((lArr !== null && typeof lArr === 'object') || (rArr !== null && typeof rArr === 'object')) {
            return lArr + rArr;
        }
        return __nfLegacyNumOperand(lArr, isAs1) + __nfLegacyNumOperand(rArr, isAs1);
    }
    if (op === '<' || op === '<=' || op === '>' || op === '>=') {
        if ((typeof left === 'string' && typeof right === 'string')
            || (left !== null && typeof left === 'object')
            || (right !== null && typeof right === 'object')) {
            switch (op) {
                case '<': return left < right;
                case '<=': return left <= right;
                case '>': return left > right;
                case '>=': return left >= right;
            }
        }
        const a = __nfLegacyNumOperand(left, isAs1);
        const b = __nfLegacyNumOperand(right, isAs1);
        switch (op) {
            case '<': return a < b;
            case '<=': return a <= b;
            case '>': return a > b;
            case '>=': return a >= b;
        }
    }
    const a = __nfLegacyNumOperand(left, isAs1);
    const b = __nfLegacyNumOperand(right, isAs1);
    switch (op) {
        case '-': return a - b;
        case '*': return a * b;
        case '/': return a / b;
        case '%': return a % b;
    }
    throw new __nfTypeError('Unsupported legacy binary operator.');
}
"#,
    ]
    .join("\n")
}

fn has_default_xml_ns(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s.unlocated() {
        Stmt::DefaultXmlNamespace(_) => true,
        Stmt::Block(body) => has_default_xml_ns(body),
        Stmt::If(_, yes, no) => {
            has_default_xml_ns(std::slice::from_ref(yes))
                || no
                    .as_ref()
                    .is_some_and(|n| has_default_xml_ns(std::slice::from_ref(n)))
        }
        Stmt::While(_, body)
        | Stmt::With(_, body, _)
        | Stmt::TellTarget(_, body, _)
        | Stmt::IfFrameLoaded(_, body)
        | Stmt::For(_, _, _, body)
        | Stmt::ForIn(_, _, _, body)
        | Stmt::DoWhile(body, _)
        | Stmt::Label(_, body) => has_default_xml_ns(std::slice::from_ref(body)),
        Stmt::Switch(_, cases) => cases.iter().any(|c| has_default_xml_ns(&c.body)),
        Stmt::Try(body, catches, finally) => {
            has_default_xml_ns(body)
                || catches.iter().any(|c| has_default_xml_ns(&c.body))
                || finally.as_ref().is_some_and(|f| has_default_xml_ns(f))
        }
        _ => false,
    })
}

pub(crate) fn builtin_as3_const_expr(parts: &[String]) -> Option<Expr> {
    match parts {
        [single] if single == "NaN" => Some(Expr::Literal("NaN".into())),
        [single] if single == "Infinity" => Some(Expr::Literal("Infinity".into())),
        [cls, prop] if cls == "Number" && prop == "NaN" => Some(Expr::Literal("NaN".into())),
        [cls, prop] if cls == "Number" && prop == "POSITIVE_INFINITY" => {
            Some(Expr::Literal("Infinity".into()))
        }
        [cls, prop] if cls == "Number" && prop == "NEGATIVE_INFINITY" => Some(Expr::Unary(
            "-".into(),
            Box::new(Expr::Literal("Infinity".into())),
        )),
        [cls, prop] if cls == "int" && prop == "MAX_VALUE" => {
            Some(Expr::Literal("2147483647".into()))
        }
        [cls, prop] if cls == "int" && prop == "MIN_VALUE" => Some(Expr::Unary(
            "-".into(),
            Box::new(Expr::Literal("2147483648".into())),
        )),
        [cls, prop] if cls == "uint" && prop == "MAX_VALUE" => {
            Some(Expr::Literal("4294967295".into()))
        }
        [cls, prop] if cls == "uint" && prop == "MIN_VALUE" => Some(Expr::Literal("0".into())),
        _ => None,
    }
}

pub(crate) fn resolve_const_expr_from_map(
    expr: &Expr,
    types: &std::collections::BTreeMap<String, String>,
    class_consts: &std::collections::BTreeMap<(String, String), Expr>,
) -> Option<Expr> {
    if constant_default(expr) {
        return Some(expr.clone());
    }
    let parts = ast::dotted_const_ref(expr)?;
    if let Some(b) = builtin_as3_const_expr(&parts) {
        return Some(b);
    }
    if parts.len() == 1 {
        return class_consts
            .get(&(String::new(), parts[0].clone()))
            .cloned();
    }
    let cls_raw = parts[..parts.len() - 1].join(".");
    let prop = &parts[parts.len() - 1];
    if let Some(mapped) = types.get(&cls_raw)
        && let Some(val) = class_consts.get(&(mapped.clone(), prop.clone()))
    {
        return Some(val.clone());
    }
    class_consts.get(&(cls_raw, prop.clone())).cloned()
}

pub(crate) fn resolve_function_defaults_in_function(
    script: &Script,
    func: &mut Function,
    types: &std::collections::BTreeMap<String, String>,
    class_consts: &std::collections::BTreeMap<(String, String), Expr>,
) -> Result<(), Diagnostic> {
    for (idx, def_opt) in func.defaults.iter_mut().enumerate() {
        if let Some(def) = def_opt
            && !constant_default(def)
        {
            let param_off = func
                .default_offsets
                .get(idx)
                .copied()
                .unwrap_or(func.offset);
            let param_name = func
                .params
                .get(idx)
                .map(|(n, _)| n.as_str())
                .unwrap_or("param");
            if let Some(resolved) = resolve_const_expr_from_map(def, types, class_consts) {
                *def_opt = Some(resolved);
            } else {
                let raw = ast::dotted_const_ref(def)
                    .map(|p| p.join("."))
                    .unwrap_or_else(|| "expression".to_owned());
                return Err(Diagnostic::at(
                    script,
                    param_off,
                    format!(
                        "Unresolved or non-constant default value '{raw}' for optional parameter '{param_name}'."
                    ),
                ));
            }
        }
    }
    resolve_function_defaults_in_stmts(script, &mut func.body, types, class_consts)
}

pub(crate) fn resolve_function_defaults_in_expr(
    script: &Script,
    expr: &mut Expr,
    types: &std::collections::BTreeMap<String, String>,
    class_consts: &std::collections::BTreeMap<(String, String), Expr>,
) -> Result<(), Diagnostic> {
    match expr {
        Expr::Function(f) => resolve_function_defaults_in_function(script, f, types, class_consts),
        Expr::Member(a, _)
        | Expr::Unary(_, a)
        | Expr::Update(_, a, _)
        | Expr::TypeTest(_, a, _, _)
        | Expr::Delete(a, _) => resolve_function_defaults_in_expr(script, a, types, class_consts),
        Expr::Index(a, b)
        | Expr::Binary(_, a, b)
        | Expr::Assign(_, a, b)
        | Expr::XmlFilter(a, b, _)
        | Expr::InstanceOf(a, b) => {
            resolve_function_defaults_in_expr(script, a, types, class_consts)?;
            resolve_function_defaults_in_expr(script, b, types, class_consts)
        }
        Expr::Conditional(a, b, c) => {
            resolve_function_defaults_in_expr(script, a, types, class_consts)?;
            resolve_function_defaults_in_expr(script, b, types, class_consts)?;
            resolve_function_defaults_in_expr(script, c, types, class_consts)
        }
        Expr::Array(items) => {
            for item in items.iter_mut().flatten() {
                resolve_function_defaults_in_expr(script, item, types, class_consts)?;
            }
            Ok(())
        }
        Expr::Object(props) => {
            for (_, v) in props {
                resolve_function_defaults_in_expr(script, v, types, class_consts)?;
            }
            Ok(())
        }
        Expr::Call(callee, args, _) => {
            resolve_function_defaults_in_expr(script, callee, types, class_consts)?;
            for a in args {
                resolve_function_defaults_in_expr(script, a, types, class_consts)?;
            }
            Ok(())
        }
        Expr::NewError(_, args, _)
        | Expr::NewClass(_, args, _)
        | Expr::NewValue(_, args, _)
        | Expr::NewBuiltin(_, args, _)
        | Expr::NewVector(_, args, _)
        | Expr::XmlLiteral(_, args) => {
            for a in args {
                resolve_function_defaults_in_expr(script, a, types, class_consts)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub(crate) fn resolve_function_defaults_in_stmts(
    script: &Script,
    stmts: &mut [Stmt],
    types: &std::collections::BTreeMap<String, String>,
    class_consts: &std::collections::BTreeMap<(String, String), Expr>,
) -> Result<(), Diagnostic> {
    for stmt in stmts {
        match stmt {
            Stmt::Located(_, inner) => {
                resolve_function_defaults_in_stmts(
                    script,
                    std::slice::from_mut(inner.as_mut()),
                    types,
                    class_consts,
                )?;
            }
            Stmt::Function(f) | Stmt::LegacyEvent(_, f) => {
                resolve_function_defaults_in_function(script, f, types, class_consts)?;
            }
            Stmt::Block(body) | Stmt::InitClip(_, body, _) => {
                resolve_function_defaults_in_stmts(script, body, types, class_consts)?;
            }
            Stmt::If(cond, yes, no) => {
                resolve_function_defaults_in_expr(script, cond, types, class_consts)?;
                resolve_function_defaults_in_stmts(
                    script,
                    std::slice::from_mut(yes.as_mut()),
                    types,
                    class_consts,
                )?;
                if let Some(no_branch) = no {
                    resolve_function_defaults_in_stmts(
                        script,
                        std::slice::from_mut(no_branch.as_mut()),
                        types,
                        class_consts,
                    )?;
                }
            }
            Stmt::While(cond, body) | Stmt::DoWhile(body, cond) => {
                resolve_function_defaults_in_expr(script, cond, types, class_consts)?;
                resolve_function_defaults_in_stmts(
                    script,
                    std::slice::from_mut(body.as_mut()),
                    types,
                    class_consts,
                )?;
            }
            Stmt::For(init, cond, upd, body) => {
                resolve_function_defaults_in_stmts(
                    script,
                    std::slice::from_mut(init.as_mut()),
                    types,
                    class_consts,
                )?;
                if let Some(c) = cond {
                    resolve_function_defaults_in_expr(script, c, types, class_consts)?;
                }
                if let Some(u) = upd {
                    resolve_function_defaults_in_expr(script, u, types, class_consts)?;
                }
                resolve_function_defaults_in_stmts(
                    script,
                    std::slice::from_mut(body.as_mut()),
                    types,
                    class_consts,
                )?;
            }
            Stmt::ForIn(init, coll, _, body) => {
                resolve_function_defaults_in_stmts(
                    script,
                    std::slice::from_mut(init.as_mut()),
                    types,
                    class_consts,
                )?;
                resolve_function_defaults_in_expr(script, coll, types, class_consts)?;
                resolve_function_defaults_in_stmts(
                    script,
                    std::slice::from_mut(body.as_mut()),
                    types,
                    class_consts,
                )?;
            }
            Stmt::Switch(val, cases) => {
                resolve_function_defaults_in_expr(script, val, types, class_consts)?;
                for case in cases {
                    if let Some(t) = &mut case.test {
                        resolve_function_defaults_in_expr(script, t, types, class_consts)?;
                    }
                    resolve_function_defaults_in_stmts(
                        script,
                        &mut case.body,
                        types,
                        class_consts,
                    )?;
                }
            }
            Stmt::Try(body, catches, fin) => {
                resolve_function_defaults_in_stmts(script, body, types, class_consts)?;
                for c in catches {
                    resolve_function_defaults_in_stmts(script, &mut c.body, types, class_consts)?;
                }
                if let Some(f) = fin {
                    resolve_function_defaults_in_stmts(script, f, types, class_consts)?;
                }
            }
            Stmt::With(obj, body, _) | Stmt::TellTarget(obj, body, _) => {
                resolve_function_defaults_in_expr(script, obj, types, class_consts)?;
                resolve_function_defaults_in_stmts(
                    script,
                    std::slice::from_mut(body.as_mut()),
                    types,
                    class_consts,
                )?;
            }
            Stmt::Label(_, body) => {
                resolve_function_defaults_in_stmts(
                    script,
                    std::slice::from_mut(body.as_mut()),
                    types,
                    class_consts,
                )?;
            }
            Stmt::Var(vars) => {
                for (_, _, init) in vars {
                    if let Some(e) = init {
                        resolve_function_defaults_in_expr(script, e, types, class_consts)?;
                    }
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::DefaultXmlNamespace(e) => {
                resolve_function_defaults_in_expr(script, e, types, class_consts)?;
            }
            Stmt::Return(Some(e), _) => {
                resolve_function_defaults_in_expr(script, e, types, class_consts)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn compile_context(script: &Script, mut context: CompileContext) -> Result<String, Diagnostic> {
    if script.language == ScriptLanguage::ActionScript3 {
        context
            .namespaces
            .entry("AS3".into())
            .or_insert_with(|| "http://adobe.com/AS3/2006/builtin".into());
        context
            .namespaces
            .entry("flash_proxy".into())
            .or_insert_with(|| "http://www.adobe.com/2006/actionscript/flash/proxy".into());
        context
            .namespaces
            .entry("object_proxy".into())
            .or_insert_with(|| "http://www.adobe.com/2006/flex/mx/internal".into());
    }
    let legacy_case = context.legacy_case;
    let tokens = TokenCursor::default();
    let mut parser = Frontend {
        script,
        tokens,
        pos: 0,
        depth: 0,
        controls: Vec::new(),
        function_depth: 0,
        next_function: 1,
        context,
        no_in: false,
        with_depth: 0,
        in_initclip: false,
    };
    let mut statements = parser.statements(false)?;
    if script.language == ScriptLanguage::ActionScript3 {
        resolve_function_defaults_in_stmts(
            script,
            &mut statements,
            &parser.context.types,
            &parser.context.as3_class_consts,
        )?;
    }
    let analysis = scope::Analysis::with_context(script, &statements, parser.context)?;
    let raw_body = Emitter {
        as3: script.language == ScriptLanguage::ActionScript3,
        analysis: &analysis,
        scope: 0,
        with_targets: Vec::new(),
        tell_target: None,
    }
    .body(&statements);
    let body = if script.language == ScriptLanguage::ActionScript3
        && has_default_xml_ns(&statements)
    {
        format!(
            "const __nfPrevXmlNs = typeof __nfXML !== 'undefined' && __nfXML?.getDefaultNamespace ? __nfXML.getDefaultNamespace() : '';\ntry {{\n{raw_body}\n}} finally {{\nif (typeof __nfXML !== 'undefined' && __nfXML?.setDefaultNamespace) __nfXML.setDefaultNamespace(__nfPrevXmlNs);\n}}"
        )
    } else {
        raw_body
    };
    let legacy_prefix = if legacy_case {
        "const __nfLegacyCase = true;\n"
    } else {
        ""
    };
    let output = format!(
        "{legacy_prefix}const __nfFrame = this, __nfNumber = Number, __nfString = String, __nfBoolean = Boolean, __nfTypeError = TypeError;\nconst __nfTrace = trace;\n{}\n{body}",
        embedded_helpers()
    );
    // Validate literal spelling and the generated function body with an independent parser.
    let generated = Script {
        language: ScriptLanguage::JavaScript,
        source: output.clone(),
    };
    if let Err(errors) = crate::compile_web(&generated, false) {
        return Err(Diagnostic::at(
            script,
            0,
            format!(
                "This ActionScript syntax cannot be lowered safely yet: {}",
                errors[0].message
            ),
        ));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn compile_as(source: &str) -> Result<String, Diagnostic> {
        compile(&Script {
            language: ScriptLanguage::ActionScript3,
            source: source.into(),
        })
    }

    #[test]
    fn timeline_methods_and_strings_are_distinct_tokens() {
        let output =
            compile_as("// stop();\ntrace('stop() _x'); child.gotoAndPlay(2); stop();").unwrap();
        assert!(output.contains("__nfTrace('stop() _x')"));
        assert!(output.contains("const __nfTrace = trace;"));
        assert!(!output.contains("console.log"));
        assert!(output.contains("__nfContainerMethod(this.child, \"gotoAndPlay\", true)(2)"));
        assert!(output.contains("this.stop()"));
    }
    #[test]
    fn operators_preserve_precedence() {
        let output = compile_as("x = 2 + 3 * 4; y = (2 + 3) * 4; flag = x > 3 && y < 50;").unwrap();
        assert!(output.contains("this.x = __nfXMLBinary(\"+\", 2, (3 * 4))"));
        assert!(output.contains("this.y = (__nfXMLBinary(\"+\", 2, 3) * 4)"));
    }
    #[test]
    fn assignments_are_right_associative_and_require_lvalues() {
        assert!(
            compile_as("x = y = 4;")
                .unwrap()
                .contains("this.x = (this.y = 4)")
        );
        assert!(compile_as("(x + 2) = 4;").is_err());
    }
    #[test]
    fn control_flow_and_persistent_variables() {
        let output = compile_as(
            "var speed:Number = 2; if (_x < 10) { _x += speed; } else { gotoAndStop(1); }",
        )
        .unwrap();
        assert!(output.contains("this.speed = __nfNumber(2)"));
        assert!(output.contains("this._x = __nfXMLBinary(\"+\", this._x, this.speed)"));
        assert!(output.contains("this.gotoAndStop(1)"));
    }
    #[test]
    fn typed_int_assignments_wrap_and_defaults_are_defined() {
        let output = compile_as("var count:int; count = 4294967295; var text:String;").unwrap();
        assert!(output.contains("this.count = 0"));
        assert!(output.contains("Number(4294967295) | 0"));
        assert!(output.contains("this.text = null"));
    }
    #[test]
    fn as2_annotations_are_not_as3_runtime_coercions() {
        let output = compile(&Script {
            language: ScriptLanguage::ActionScript2,
            source: "var n:Number = 2; _x += n;".into(),
        })
        .unwrap();
        assert!(output.contains("this.n = 2"));
        assert!(!output.contains("Number(2)"));
        let events = compile(&Script {language:ScriptLanguage::ActionScript2,source:"on(press,release) { trace(this); } onClipEvent(enterFrame) { this.tick=1; } var a=[]; trace(a instanceof Array);".into()}).unwrap();
        assert!(events.contains("addLegacyEvent"));
        assert!(events.contains("__nfArrayIsArray(__nfValue)"));
    }
    #[test]
    fn loops_and_math_compile() {
        let output =
            compile_as("for (var i:int = 0; i < 10; i++) { x += Math.sin(i); if (x > 4) break; }")
                .unwrap();
        assert!(output.contains("for ("));
        assert!(output.contains("__nfContainerMethod(__nfMath(true), \"sin\", true)(this.i)"));
    }
    #[test]
    fn missing_compatibility_features_fail_explicitly() {
        assert!(compile_as("var x:XML = <a/>; x.(@id==1);").is_ok());
        for source in [
            "x = y is MissingType;",
            "on(release) { play(); }",
            "getURL('https://example.com');",
            "import flash.display.Sprite;",
            "foo.addEventListener('click', handler);",
        ] {
            assert!(compile_as(source).is_err(), "must reject {source}");
        }
    }
    #[test]
    fn errors_point_to_user_line() {
        let error = compile_as("play();\n  package sample {}").unwrap_err();
        assert_eq!((error.line, error.column), (2, 3));
    }
    #[test]
    fn no_silent_remainder_and_unterminated_comments() {
        assert!(compile_as("play(); garbage ???").is_err());
        assert!(compile_as("/* unclosed").is_err());
        assert!(compile_as("trace('unclosed)").is_err());
        assert!(compile_as("x = 1e;").is_err());
    }

    #[test]
    fn rejects_extreme_expression_depth_without_overflowing() {
        let source = format!("x = {};", vec!["1"; 1000].join(" + "));
        let error = compile_as(&source).unwrap_err();
        assert!(error.message.contains("128 nested operations"));
    }

    #[test]
    fn functions_are_resolved_after_declarations_and_locals_are_lexical() {
        let code=compile_as("result = twice(4); function twice(value:int):int { var local:int = value * 2; return local; }").unwrap();
        assert!(code.contains("var local = 0"));
        assert!(!code.contains("this.local"));
        assert!(code.find("this.twice =").unwrap() < code.find("this.result =").unwrap());
        assert!(compile_as("function make(x:int):Function { return function(y:int):int { return x + y; }; } var f:Function = make(2); answer = f(3);").is_ok());
    }

    #[test]
    fn function_semantics_outside_the_supported_boundary_are_diagnostics() {
        for source in [
            "return 1;",
            "function f(a:int = 2, b:int):int { return a+b; }",
            "function f(a:int = Number(2)):int { return a; }",
            "function f(a:Object = {}):Object { return a; }",
            "function f(...args:int) {}",
            "function f(...args, later) {}",
            "function f(...args) { return arguments.length; }",
            "function f(arguments) {}",
            "function f(){ var arguments = []; }",
            "arguments[0];",
            "function f(a, a) {}",
            "function f():void { return 1; }",
            "function f():int { return; }",
            "function f(x:int):int { if (x > 0) return x; }",
            "while (true) { var f = function(){ break; }; }",
            "if (true) { function f() {} }",
            "var f = function named(){ named = 2; };",
            "function f(){ var fn:Number = 2; fn(); }",
            "var n:Number = function() {}; n();",
            "function f(){ return 1; } child.f();",
            "var obj = {}; obj.unavailable();",
            "function f(trace:Number) { trace(); }",
            "function f(Math) { Math.sin(1); }",
            "function __nfFrame() {}",
            "function f(__nfNumber) {}",
            "function f(){ var __nfValue; }",
            "Number = 3;",
            "trace++;",
        ] {
            assert!(compile_as(source).is_err(), "must reject {source}");
        }
        for source in [
            "function f(a:int = 2):int { return a; }",
            "function f(a:String = null,...args:Array):int { return args.length; }",
            "function f(){ return arguments[0]; }",
            "var f=function(){return this.x;}; f.call(this); f['apply'](this,[]);",
            "var f=function(n:int):int {if(n<=1)return 1;return n*arguments.callee(n-1);};",
        ] {
            assert!(compile_as(source).is_ok(), "must accept {source}");
        }
        for source in ["function f(a=2){}", "function f(...rest){}"] {
            assert!(
                compile(&Script {
                    language: ScriptLanguage::ActionScript2,
                    source: source.into()
                })
                .is_err()
            );
        }
        let error = compile_as("function ok() {}\nmissing();").unwrap_err();
        assert_eq!(error.line, 2);
    }

    #[test]
    fn explicit_user_function_members_do_not_unlock_unrelated_apis() {
        assert!(
            compile_as("this.custom = function(x:int):int { return x; }; this.custom(2);").is_ok()
        );
        assert!(compile_as("var tools = { go: function() {} }; tools.go();").is_ok());
        assert!(compile_as("var tools = { go: function() {} }; other.go();").is_err());
        assert!(compile_as(r#"var tools = {"r\u0075n": function() {} }; tools.run();"#).is_ok());
        assert!(compile_as(r#"var tools = {"\uZZZZ": function() {} };"#).is_err());
        assert!(
            compile_as(
                "function a() { var tools = { go: function() {} }; } function b() { tools.go(); }"
            )
            .is_err()
        );
    }

    #[test]
    fn as2_function_types_erase_but_as3_primitive_returns_require_values() {
        assert!(
            compile(&Script {
                language: ScriptLanguage::ActionScript2,
                source: "function f(x:Number):Number { return; } f();".into()
            })
            .is_ok()
        );
        assert!(
            compile(&Script {
                language: ScriptLanguage::ActionScript2,
                source: "function f():Void { return; }".into()
            })
            .is_ok()
        );
        assert!(compile_as("function f():int { return; }").is_err());
        assert!(compile_as("function f():int { if (true) return 1; else return 2; }").is_ok());
    }

    #[test]
    fn t240_compiler_and_runtime_semantics() {
        // 1. AS1 rejects :Type annotations
        assert!(
            compile(&Script {
                language: ScriptLanguage::ActionScript1,
                source: "var score:Number = 10;".into(),
            })
            .is_err()
        );

        // 2. AS1/AS2 var and function inside on(...) / onClipEvent(...) bind to MovieClip scope
        let clip_src = [
            "onClipEvent(load) { var counter = 10; function bump(d) { counter += d; return counter; } }",
            "onClipEvent(enterFrame) { counter = bump(5); }",
            "on(press) { trace(counter); }",
        ]
        .join("\n");
        let compiled_clip = compile(&Script {
            language: ScriptLanguage::ActionScript2,
            source: clip_src,
        })
        .expect("onClipEvent/on shared instance scope must compile");
        assert!(
            compiled_clip.contains("__nfFrame.counter") || compiled_clip.contains("this.counter"),
            "clip handler var must bind to timeline scope:\n{compiled_clip}"
        );
        assert!(
            !compiled_clip.contains("let counter"),
            "clip handler var must not be trapped in ephemeral JS local:\n{compiled_clip}"
        );

        // 3. AS1 undefined coercions and AS2 null coercions
        let as1_coercions = compile(&Script {
            language: ScriptLanguage::ActionScript1,
            source: "var u; var a = u + 5; var b = 'x' + u; var c = Number(u);".into(),
        })
        .expect("AS1 coercions compile");
        assert!(as1_coercions.contains("__nfLegacyBinary(\"+\""));
        assert!(as1_coercions.contains("__nfNumber1"));

        let as2_coercions = compile(&Script {
            language: ScriptLanguage::ActionScript2,
            source: "var a = null + 1; var b = Number(null);".into(),
        })
        .expect("AS2 coercions compile");
        assert!(as2_coercions.contains("__nfLegacyBinary(\"+\""));
        assert!(as2_coercions.contains("__nfNumber2"));

        // 4. AS3 constructor statements before super(...), custom namespace isolation,
        //    typed member assignment return value, and native-base super.method()
        let mut proj = neoflash_core::Project::blank();
        proj.sources = vec![
            neoflash_core::SourceUnit {
                id: "ns".into(),
                path: "secretNs.as".into(),
                language: ScriptLanguage::ActionScript3,
                source: "package { public namespace secretNs = \"urn:test:secret\"; }".into(),
            },
            neoflash_core::SourceUnit {
                id: "base".into(),
                path: "BaseBox.as".into(),
                language: ScriptLanguage::ActionScript3,
                source: "package { import flash.display.MovieClip; public class BaseBox extends MovieClip { public var superCalls:int = 0; public function BaseBox(step:int = 1) { superCalls += step; } public function stopBase():void { super.stop(); } } }".into(),
            },
            neoflash_core::SourceUnit {
                id: "child".into(),
                path: "ChildBox.as".into(),
                language: ScriptLanguage::ActionScript3,
                source: "package { public dynamic class ChildBox extends BaseBox { public var count:int = 0; secretNs var hidden:String = \"top-secret\"; public function ChildBox() { var step:int = 2; super(step); } public function readOpen():String { use namespace secretNs; return this.hidden; } } }".into(),
            },
        ];
        let program = crate::compile_project(&proj).expect("AS3 project compiles");
        // Verify superConstruct is only emitted once in ChildBox constructor despite statement before super(step)
        let child_ctor_idx = program.code.find("define(\"ChildBox\"").unwrap();
        let child_section = &program.code[child_ctor_idx..];
        assert_eq!(
            child_section.matches("superConstruct(\"ChildBox\"").count(),
            1,
            "ChildBox constructor must not inject duplicate superConstruct:\n{child_section}"
        );
    }
}
