//! AS3 Object property policy and native + source-class reflection.
//! Original Rust implementation from Adobe's public Object and flash.utils contracts:
//! https://help.adobe.com/en_US/FlashPlatform/reference/actionscript/3/Object.html
//! https://help.adobe.com/en_US/FlashPlatform/reference/actionscript/3/flash/utils/package.html
//! Default labels were checked against Adobe avmplus 858d034a3bd3a54d9b70909386435cf4aec81d21,
//! core/ScriptObject.cpp::implToString and core/ClassClosure.cpp::implToString.
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_OBJECT_JSON_BYTES: usize = 8 * 1024 * 1024;
const MAX_CLASSES: usize = 4096;
const MAX_MEMBERS: usize = 32768;

#[derive(Clone, Deserialize)]
struct Parameter {
    #[serde(rename = "type")]
    ty: String,
    optional: bool,
}
#[derive(Clone, Deserialize)]
struct Member {
    name: String,
    kind: String,
    access: String,
    #[serde(rename = "static")]
    is_static: bool,
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    parameters: Vec<Parameter>,
    #[serde(default)]
    #[allow(dead_code)]
    rest: bool,
}
#[derive(Clone, Deserialize)]
struct Class {
    name: String,
    base: Option<String>,
    public: bool,
    dynamic: bool,
    #[serde(rename = "final")]
    is_final: bool,
    #[serde(default)]
    interface: bool,
    #[serde(default)]
    interfaces: Vec<String>,
    members: Vec<Member>,
    #[serde(default)]
    constructor: Vec<Parameter>,
    #[serde(default)]
    #[allow(dead_code)]
    constructor_rest: bool,
}

fn param(ty: &str, optional: bool) -> Parameter {
    Parameter {
        ty: ty.into(),
        optional,
    }
}
fn m_var(name: &str, ty: &str, is_static: bool) -> Member {
    Member {
        name: name.into(),
        kind: "variable".into(),
        access: "public".into(),
        is_static,
        ty: ty.into(),
        parameters: vec![],
        rest: false,
    }
}
fn m_const(name: &str, ty: &str, is_static: bool) -> Member {
    Member {
        name: name.into(),
        kind: "constant".into(),
        access: "public".into(),
        is_static,
        ty: ty.into(),
        parameters: vec![],
        rest: false,
    }
}
fn m_get(name: &str, ty: &str) -> Member {
    Member {
        name: name.into(),
        kind: "get".into(),
        access: "public".into(),
        is_static: false,
        ty: ty.into(),
        parameters: vec![],
        rest: false,
    }
}
fn m_rw(name: &str, ty: &str) -> [Member; 2] {
    [
        m_get(name, ty),
        Member {
            name: name.into(),
            kind: "set".into(),
            access: "public".into(),
            is_static: false,
            ty: ty.into(),
            parameters: vec![param(ty, false)],
            rest: false,
        },
    ]
}
fn m_fn(name: &str, ret: &str, params: &[(&str, bool)], is_static: bool) -> Member {
    Member {
        name: name.into(),
        kind: "method".into(),
        access: "public".into(),
        is_static,
        ty: ret.into(),
        parameters: params.iter().map(|(t, o)| param(t, *o)).collect(),
        rest: false,
    }
}

const NATIVE_INTERFACES: &[&str] = &[
    "flash.accessibility.ISearchableText",
    "flash.accessibility.ISimpleTextSelection",
    "flash.desktop.IFilePromise",
    "flash.display.IBitmapDrawable",
    "flash.display.IGraphicsData",
    "flash.display.IGraphicsFill",
    "flash.display.IGraphicsPath",
    "flash.display.IGraphicsStroke",
    "flash.events.IEventDispatcher",
    "flash.filters.IBitmapFilter",
    "flash.security.IURIDereferencer",
    "flash.text.ime.IIMEClient",
    "flash.utils.IDataInput",
    "flash.utils.IDataOutput",
    "flash.utils.IExternalizable",
];

const NATIVE_BASES: &[(&str, &str)] = &[
    ("ArgumentError", "Error"),
    ("DefinitionError", "Error"),
    ("EvalError", "Error"),
    ("RangeError", "Error"),
    ("ReferenceError", "Error"),
    ("SecurityError", "Error"),
    ("SyntaxError", "Error"),
    ("TypeError", "Error"),
    ("URIError", "Error"),
    ("VerifyError", "Error"),
    ("flash.data.SQLConnection", "flash.events.EventDispatcher"),
    ("flash.data.SQLIndexSchema", "flash.data.SQLSchema"),
    ("flash.data.SQLStatement", "flash.events.EventDispatcher"),
    ("flash.data.SQLTableSchema", "flash.data.SQLSchema"),
    ("flash.data.SQLTriggerSchema", "flash.data.SQLSchema"),
    ("flash.data.SQLViewSchema", "flash.data.SQLTableSchema"),
    ("flash.desktop.DockIcon", "flash.desktop.InteractiveIcon"),
    ("flash.desktop.Icon", "flash.events.EventDispatcher"),
    ("flash.desktop.InteractiveIcon", "flash.desktop.Icon"),
    (
        "flash.desktop.NativeApplication",
        "flash.events.EventDispatcher",
    ),
    (
        "flash.desktop.NativeProcess",
        "flash.events.EventDispatcher",
    ),
    (
        "flash.desktop.SystemTrayIcon",
        "flash.desktop.InteractiveIcon",
    ),
    ("flash.display.AVM1Movie", "flash.display.DisplayObject"),
    ("flash.display.Bitmap", "flash.display.DisplayObject"),
    (
        "flash.display.DisplayObject",
        "flash.events.EventDispatcher",
    ),
    (
        "flash.display.DisplayObjectContainer",
        "flash.display.InteractiveObject",
    ),
    ("flash.display.FrameLabel", "flash.events.EventDispatcher"),
    (
        "flash.display.InteractiveObject",
        "flash.display.DisplayObject",
    ),
    (
        "flash.display.Loader",
        "flash.display.DisplayObjectContainer",
    ),
    ("flash.display.LoaderInfo", "flash.events.EventDispatcher"),
    ("flash.display.MorphShape", "flash.display.DisplayObject"),
    ("flash.display.MovieClip", "flash.display.Sprite"),
    ("flash.display.NativeMenu", "flash.events.EventDispatcher"),
    (
        "flash.display.NativeMenuItem",
        "flash.events.EventDispatcher",
    ),
    ("flash.display.NativeWindow", "flash.events.EventDispatcher"),
    ("flash.display.Screen", "flash.events.EventDispatcher"),
    ("flash.display.ShaderJob", "flash.events.EventDispatcher"),
    ("flash.display.Shape", "flash.display.DisplayObject"),
    (
        "flash.display.SimpleButton",
        "flash.display.InteractiveObject",
    ),
    (
        "flash.display.Sprite",
        "flash.display.DisplayObjectContainer",
    ),
    (
        "flash.display.Stage",
        "flash.display.DisplayObjectContainer",
    ),
    ("flash.display.Stage3D", "flash.events.EventDispatcher"),
    ("flash.display3D.Context3D", "flash.events.EventDispatcher"),
    (
        "flash.display3D.textures.CubeTexture",
        "flash.display3D.textures.TextureBase",
    ),
    (
        "flash.display3D.textures.RectangleTexture",
        "flash.display3D.textures.TextureBase",
    ),
    (
        "flash.display3D.textures.Texture",
        "flash.display3D.textures.TextureBase",
    ),
    (
        "flash.display3D.textures.TextureBase",
        "flash.events.EventDispatcher",
    ),
    (
        "flash.display3D.textures.VideoTexture",
        "flash.display3D.textures.TextureBase",
    ),
    ("flash.errors.DRMManagerError", "Error"),
    ("flash.errors.EOFError", "flash.errors.IOError"),
    ("flash.errors.IOError", "Error"),
    ("flash.errors.IllegalOperationError", "Error"),
    ("flash.errors.InvalidSWFError", "Error"),
    ("flash.errors.MemoryError", "Error"),
    ("flash.errors.PermissionError", "Error"),
    ("flash.errors.SQLError", "Error"),
    ("flash.errors.ScriptTimeoutError", "Error"),
    ("flash.errors.StackOverflowError", "Error"),
    ("flash.events.AccelerometerEvent", "flash.events.Event"),
    ("flash.events.ActivityEvent", "flash.events.Event"),
    ("flash.events.AsyncErrorEvent", "flash.events.ErrorEvent"),
    ("flash.events.AudioOutputChangeEvent", "flash.events.Event"),
    ("flash.events.BrowserInvokeEvent", "flash.events.Event"),
    ("flash.events.ContextMenuEvent", "flash.events.Event"),
    ("flash.events.DNSResolverEvent", "flash.events.Event"),
    ("flash.events.DataEvent", "flash.events.TextEvent"),
    ("flash.events.DatagramSocketDataEvent", "flash.events.Event"),
    ("flash.events.DeviceRotationEvent", "flash.events.Event"),
    ("flash.events.ErrorEvent", "flash.events.TextEvent"),
    ("flash.events.FileListEvent", "flash.events.Event"),
    ("flash.events.FocusEvent", "flash.events.Event"),
    ("flash.events.FullScreenEvent", "flash.events.ActivityEvent"),
    ("flash.events.GameInputEvent", "flash.events.Event"),
    ("flash.events.GeolocationEvent", "flash.events.Event"),
    ("flash.events.GestureEvent", "flash.events.Event"),
    ("flash.events.HTTPStatusEvent", "flash.events.Event"),
    ("flash.events.IMEEvent", "flash.events.TextEvent"),
    ("flash.events.IOErrorEvent", "flash.events.ErrorEvent"),
    ("flash.events.InvokeEvent", "flash.events.Event"),
    ("flash.events.KeyboardEvent", "flash.events.Event"),
    ("flash.events.LocationChangeEvent", "flash.events.Event"),
    ("flash.events.MediaEvent", "flash.events.Event"),
    ("flash.events.MouseEvent", "flash.events.Event"),
    ("flash.events.NativeDragEvent", "flash.events.MouseEvent"),
    ("flash.events.NativeProcessExitEvent", "flash.events.Event"),
    ("flash.events.NativeWindowBoundsEvent", "flash.events.Event"),
    (
        "flash.events.NativeWindowDisplayStateEvent",
        "flash.events.Event",
    ),
    ("flash.events.NetDataEvent", "flash.events.Event"),
    ("flash.events.NetMonitorEvent", "flash.events.Event"),
    ("flash.events.NetStatusEvent", "flash.events.Event"),
    ("flash.events.OutputProgressEvent", "flash.events.Event"),
    ("flash.events.PermissionEvent", "flash.events.Event"),
    (
        "flash.events.PressAndTapGestureEvent",
        "flash.events.GestureEvent",
    ),
    ("flash.events.ProgressEvent", "flash.events.Event"),
    ("flash.events.RemoteNotificationEvent", "flash.events.Event"),
    ("flash.events.SQLErrorEvent", "flash.events.ErrorEvent"),
    ("flash.events.SQLEvent", "flash.events.Event"),
    ("flash.events.SQLUpdateEvent", "flash.events.Event"),
    ("flash.events.SampleDataEvent", "flash.events.Event"),
    ("flash.events.ScreenMouseEvent", "flash.events.MouseEvent"),
    ("flash.events.SecurityErrorEvent", "flash.events.ErrorEvent"),
    (
        "flash.events.ServerSocketConnectEvent",
        "flash.events.Event",
    ),
    ("flash.events.ShaderEvent", "flash.events.Event"),
    ("flash.events.SoftKeyboardEvent", "flash.events.Event"),
    ("flash.events.StageOrientationEvent", "flash.events.Event"),
    (
        "flash.events.StageVideoAvailabilityEvent",
        "flash.events.Event",
    ),
    ("flash.events.StageVideoEvent", "flash.events.Event"),
    ("flash.events.StatusEvent", "flash.events.Event"),
    (
        "flash.events.StorageVolumeChangeEvent",
        "flash.events.Event",
    ),
    ("flash.events.SyncEvent", "flash.events.Event"),
    ("flash.events.TextEvent", "flash.events.Event"),
    ("flash.events.ThrottleEvent", "flash.events.Event"),
    ("flash.events.TimerEvent", "flash.events.Event"),
    ("flash.events.TouchEvent", "flash.events.Event"),
    (
        "flash.events.TransformGestureEvent",
        "flash.events.GestureEvent",
    ),
    ("flash.events.UncaughtErrorEvent", "flash.events.ErrorEvent"),
    (
        "flash.events.UncaughtErrorEvents",
        "flash.events.EventDispatcher",
    ),
    ("flash.events.VideoEvent", "flash.events.Event"),
    ("flash.events.VideoTextureEvent", "flash.events.Event"),
    (
        "flash.external.ExtensionContext",
        "flash.events.EventDispatcher",
    ),
    ("flash.filesystem.File", "flash.net.FileReference"),
    (
        "flash.filesystem.FileStream",
        "flash.events.EventDispatcher",
    ),
    (
        "flash.filesystem.StorageVolumeInfo",
        "flash.events.EventDispatcher",
    ),
    ("flash.filters.BevelFilter", "flash.filters.BitmapFilter"),
    ("flash.filters.BlurFilter", "flash.filters.BitmapFilter"),
    (
        "flash.filters.ColorMatrixFilter",
        "flash.filters.BitmapFilter",
    ),
    (
        "flash.filters.ConvolutionFilter",
        "flash.filters.BitmapFilter",
    ),
    (
        "flash.filters.DisplacementMapFilter",
        "flash.filters.BitmapFilter",
    ),
    (
        "flash.filters.DropShadowFilter",
        "flash.filters.BitmapFilter",
    ),
    ("flash.filters.GlowFilter", "flash.filters.BitmapFilter"),
    (
        "flash.filters.GradientBevelFilter",
        "flash.filters.BitmapFilter",
    ),
    (
        "flash.filters.GradientGlowFilter",
        "flash.filters.BitmapFilter",
    ),
    ("flash.filters.ShaderFilter", "flash.filters.BitmapFilter"),
    ("flash.html.HTMLLoader", "flash.display.Sprite"),
    ("flash.media.AVURLLoader", "flash.net.URLLoader"),
    ("flash.media.AVURLStream", "flash.net.URLStream"),
    (
        "flash.media.AudioDeviceManager",
        "flash.events.EventDispatcher",
    ),
    ("flash.media.Camera", "flash.events.EventDispatcher"),
    ("flash.media.CameraRoll", "flash.events.EventDispatcher"),
    ("flash.media.CameraUI", "flash.events.EventDispatcher"),
    (
        "flash.media.H264VideoStreamSettings",
        "flash.media.VideoStreamSettings",
    ),
    ("flash.media.MediaPromise", "flash.events.EventDispatcher"),
    ("flash.media.Microphone", "flash.events.EventDispatcher"),
    ("flash.media.Sound", "flash.events.EventDispatcher"),
    ("flash.media.SoundChannel", "flash.events.EventDispatcher"),
    ("flash.media.StageVideo", "flash.events.EventDispatcher"),
    ("flash.media.StageWebView", "flash.events.EventDispatcher"),
    ("flash.media.Video", "flash.display.DisplayObject"),
    ("flash.net.DatagramSocket", "flash.events.EventDispatcher"),
    ("flash.net.FileReference", "flash.events.EventDispatcher"),
    (
        "flash.net.FileReferenceList",
        "flash.events.EventDispatcher",
    ),
    ("flash.net.LocalConnection", "flash.events.EventDispatcher"),
    ("flash.net.NetConnection", "flash.events.EventDispatcher"),
    ("flash.net.NetGroup", "flash.events.EventDispatcher"),
    ("flash.net.NetMonitor", "flash.events.EventDispatcher"),
    ("flash.net.NetStream", "flash.events.EventDispatcher"),
    (
        "flash.net.NetStreamPlayOptions",
        "flash.events.EventDispatcher",
    ),
    ("flash.net.NetworkInfo", "flash.events.EventDispatcher"),
    ("flash.net.SecureSocket", "flash.net.Socket"),
    ("flash.net.ServerSocket", "flash.events.EventDispatcher"),
    ("flash.net.SharedObject", "flash.events.EventDispatcher"),
    ("flash.net.Socket", "flash.events.EventDispatcher"),
    ("flash.net.URLLoader", "flash.events.EventDispatcher"),
    ("flash.net.URLStream", "flash.events.EventDispatcher"),
    ("flash.net.XMLSocket", "flash.events.EventDispatcher"),
    ("flash.net.dns.AAAARecord", "flash.net.dns.ResourceRecord"),
    ("flash.net.dns.ARecord", "flash.net.dns.ResourceRecord"),
    ("flash.net.dns.DNSResolver", "flash.events.EventDispatcher"),
    ("flash.net.dns.MXRecord", "flash.net.dns.ResourceRecord"),
    ("flash.net.dns.PTRRecord", "flash.net.dns.ResourceRecord"),
    ("flash.net.dns.SRVRecord", "flash.net.dns.ResourceRecord"),
    ("flash.printing.PrintJob", "flash.events.EventDispatcher"),
    (
        "flash.sensors.Accelerometer",
        "flash.events.EventDispatcher",
    ),
    (
        "flash.sensors.DeviceRotation",
        "flash.events.EventDispatcher",
    ),
    ("flash.sensors.Geolocation", "flash.events.EventDispatcher"),
    ("flash.system.IME", "flash.events.EventDispatcher"),
    (
        "flash.system.JPEGLoaderContext",
        "flash.system.LoaderContext",
    ),
    (
        "flash.system.MessageChannel",
        "flash.events.EventDispatcher",
    ),
    ("flash.system.Worker", "flash.events.EventDispatcher"),
    ("flash.text.StageText", "flash.events.EventDispatcher"),
    ("flash.text.StaticText", "flash.display.DisplayObject"),
    ("flash.text.StyleSheet", "flash.events.EventDispatcher"),
    ("flash.text.TextField", "flash.display.InteractiveObject"),
    (
        "flash.text.engine.EastAsianJustifier",
        "flash.text.engine.TextJustifier",
    ),
    (
        "flash.text.engine.GraphicElement",
        "flash.text.engine.ContentElement",
    ),
    (
        "flash.text.engine.GroupElement",
        "flash.text.engine.ContentElement",
    ),
    (
        "flash.text.engine.SpaceJustifier",
        "flash.text.engine.TextJustifier",
    ),
    (
        "flash.text.engine.TextElement",
        "flash.text.engine.ContentElement",
    ),
    (
        "flash.text.engine.TextLine",
        "flash.display.DisplayObjectContainer",
    ),
    ("flash.ui.ContextMenu", "flash.display.NativeMenu"),
    ("flash.ui.ContextMenuItem", "flash.display.NativeMenuItem"),
    ("flash.ui.GameInput", "flash.events.EventDispatcher"),
    ("flash.ui.GameInputControl", "flash.events.EventDispatcher"),
    ("flash.utils.Timer", "flash.events.EventDispatcher"),
    ("flash.xml.XMLDocument", "flash.xml.XMLNode"),
];

fn native_base_lookup(name: &str) -> Option<&'static str> {
    if name == "Object" || name == "null" || name == "void" || NATIVE_INTERFACES.contains(&name) {
        return None;
    }
    if name.starts_with("Vector.<") {
        return Some("Object");
    }
    for (cls, base) in NATIVE_BASES {
        if *cls == name {
            return Some(*base);
        }
    }
    Some("Object")
}

fn native_interfaces_for(name: &str) -> Vec<String> {
    match name {
        "flash.events.EventDispatcher" => vec!["flash.events.IEventDispatcher".into()],
        "flash.display.DisplayObject" | "flash.display.BitmapData" => {
            vec!["flash.display.IBitmapDrawable".into()]
        }
        "flash.utils.ByteArray"
        | "flash.net.Socket"
        | "flash.net.URLStream"
        | "flash.filesystem.FileStream" => vec![
            "flash.utils.IDataInput".into(),
            "flash.utils.IDataOutput".into(),
        ],
        "flash.display.GraphicsSolidFill" => vec![
            "flash.display.IGraphicsData".into(),
            "flash.display.IGraphicsFill".into(),
        ],
        "flash.display.GraphicsGradientFill"
        | "flash.display.GraphicsBitmapFill"
        | "flash.display.GraphicsShaderFill"
        | "flash.display.GraphicsEndFill" => vec![
            "flash.display.IGraphicsData".into(),
            "flash.display.IGraphicsFill".into(),
        ],
        "flash.display.GraphicsPath" | "flash.display.GraphicsTrianglePath" => vec![
            "flash.display.IGraphicsData".into(),
            "flash.display.IGraphicsPath".into(),
        ],
        "flash.display.GraphicsStroke" => vec![
            "flash.display.IGraphicsData".into(),
            "flash.display.IGraphicsStroke".into(),
        ],
        _ => vec![],
    }
}

fn native_is_dynamic(name: &str) -> bool {
    matches!(
        name,
        "Object"
            | "Array"
            | "Date"
            | "RegExp"
            | "Function"
            | "Class"
            | "Error"
            | "ArgumentError"
            | "DefinitionError"
            | "EvalError"
            | "RangeError"
            | "ReferenceError"
            | "SecurityError"
            | "SyntaxError"
            | "TypeError"
            | "URIError"
            | "VerifyError"
            | "XML"
            | "XMLList"
            | "flash.display.MovieClip"
            | "flash.utils.Dictionary"
            | "flash.net.URLVariables"
            | "flash.net.NetConnection"
            | "flash.text.StyleSheet"
    )
}

fn native_is_final(name: &str) -> bool {
    name.starts_with("Vector.<")
        || matches!(
            name,
            "String"
                | "Number"
                | "int"
                | "uint"
                | "Boolean"
                | "Math"
                | "JSON"
                | "XML"
                | "XMLList"
                | "Namespace"
                | "QName"
                | "Class"
                | "flash.display.FrameLabel"
                | "flash.display.Scene"
                | "flash.display.Graphics"
                | "flash.display.StageQuality"
                | "flash.display.StageScaleMode"
                | "flash.display.StageAlign"
                | "flash.display.StageDisplayState"
                | "flash.display.BlendMode"
                | "flash.display.CapsStyle"
                | "flash.display.JointStyle"
                | "flash.display.LineScaleMode"
                | "flash.display.PixelSnapping"
                | "flash.display.BitmapDataChannel"
                | "flash.utils.Endian"
        )
}

fn build_native_class(name: &str) -> Class {
    if NATIVE_INTERFACES.contains(&name) {
        let members = match name {
            "flash.events.IEventDispatcher" => vec![
                m_fn(
                    "addEventListener",
                    "void",
                    &[
                        ("String", false),
                        ("Function", false),
                        ("Boolean", true),
                        ("int", true),
                        ("Boolean", true),
                    ],
                    false,
                ),
                m_fn(
                    "removeEventListener",
                    "void",
                    &[("String", false), ("Function", false), ("Boolean", true)],
                    false,
                ),
                m_fn(
                    "dispatchEvent",
                    "Boolean",
                    &[("flash.events.Event", false)],
                    false,
                ),
                m_fn("hasEventListener", "Boolean", &[("String", false)], false),
                m_fn("willTrigger", "Boolean", &[("String", false)], false),
            ],
            "flash.utils.IDataInput" => vec![
                m_get("bytesAvailable", "uint"),
                m_rw("endian", "String")[0].clone(),
                m_rw("endian", "String")[1].clone(),
                m_rw("objectEncoding", "uint")[0].clone(),
                m_rw("objectEncoding", "uint")[1].clone(),
                m_fn("readBoolean", "Boolean", &[], false),
                m_fn("readByte", "int", &[], false),
                m_fn("readDouble", "Number", &[], false),
                m_fn("readInt", "int", &[], false),
                m_fn("readObject", "*", &[], false),
                m_fn("readUTF", "String", &[], false),
            ],
            "flash.utils.IDataOutput" => vec![
                m_rw("endian", "String")[0].clone(),
                m_rw("endian", "String")[1].clone(),
                m_rw("objectEncoding", "uint")[0].clone(),
                m_rw("objectEncoding", "uint")[1].clone(),
                m_fn("writeBoolean", "void", &[("Boolean", false)], false),
                m_fn("writeByte", "void", &[("int", false)], false),
                m_fn("writeDouble", "void", &[("Number", false)], false),
                m_fn("writeInt", "void", &[("int", false)], false),
                m_fn("writeObject", "void", &[("*", false)], false),
                m_fn("writeUTF", "void", &[("String", false)], false),
            ],
            _ => vec![],
        };
        return Class {
            name: name.into(),
            base: None,
            public: true,
            dynamic: false,
            is_final: false,
            interface: true,
            interfaces: vec![],
            members,
            constructor: vec![],
            constructor_rest: false,
        };
    }

    if let Some(elem) = name
        .strip_prefix("Vector.<")
        .and_then(|s| s.strip_suffix('>'))
    {
        let mut members = Vec::new();
        members.extend(m_rw("length", "uint"));
        members.extend(m_rw("fixed", "Boolean"));
        members.push(m_fn("push", "uint", &[], false));
        members.push(m_fn("pop", elem, &[], false));
        members.push(m_fn("shift", elem, &[], false));
        members.push(m_fn("unshift", "uint", &[], false));
        members.push(m_fn("slice", name, &[("int", true), ("int", true)], false));
        members.push(m_fn(
            "splice",
            name,
            &[("int", false), ("uint", true)],
            false,
        ));
        members.push(m_fn("concat", name, &[], false));
        members.push(m_fn("join", "String", &[("String", true)], false));
        members.push(m_fn(
            "indexOf",
            "int",
            &[(elem, false), ("int", true)],
            false,
        ));
        members.push(m_fn(
            "lastIndexOf",
            "int",
            &[(elem, false), ("int", true)],
            false,
        ));
        members.push(m_fn("reverse", name, &[], false));
        members.push(m_fn("sort", name, &[("*", false)], false));
        members.push(m_fn(
            "forEach",
            "void",
            &[("Function", false), ("Object", true)],
            false,
        ));
        members.push(m_fn(
            "map",
            name,
            &[("Function", false), ("Object", true)],
            false,
        ));
        members.push(m_fn(
            "filter",
            name,
            &[("Function", false), ("Object", true)],
            false,
        ));
        members.push(m_fn(
            "every",
            "Boolean",
            &[("Function", false), ("Object", true)],
            false,
        ));
        members.push(m_fn(
            "some",
            "Boolean",
            &[("Function", false), ("Object", true)],
            false,
        ));
        return Class {
            name: name.into(),
            base: Some("Object".into()),
            public: true,
            dynamic: false,
            is_final: true,
            interface: false,
            interfaces: vec![],
            members,
            constructor: vec![param("uint", true), param("Boolean", true)],
            constructor_rest: false,
        };
    }

    let mut members = Vec::new();
    let mut ctor = Vec::new();
    match name {
        "Number" => {
            ctor.push(param("*", true));
            for c in [
                "MAX_VALUE",
                "MIN_VALUE",
                "NaN",
                "NEGATIVE_INFINITY",
                "POSITIVE_INFINITY",
            ] {
                members.push(m_const(c, "Number", true));
            }
        }
        "int" | "uint" => {
            ctor.push(param("*", true));
            members.push(m_const("MAX_VALUE", name, true));
            members.push(m_const("MIN_VALUE", name, true));
        }
        "Boolean" => ctor.push(param("*", true)),
        "String" => {
            ctor.push(param("*", true));
            members.push(m_get("length", "int"));
            members.push(m_fn("fromCharCode", "String", &[], true));
        }
        "Array" => {
            for c in [
                "CASEINSENSITIVE",
                "DESCENDING",
                "UNIQUESORT",
                "RETURNINDEXEDARRAY",
                "NUMERIC",
            ] {
                members.push(m_const(c, "uint", true));
            }
            members.extend(m_rw("length", "uint"));
            members.push(m_fn("push", "uint", &[], false));
            members.push(m_fn("pop", "*", &[], false));
            members.push(m_fn("slice", "Array", &[("*", true), ("*", true)], false));
            members.push(m_fn("splice", "Array", &[], false));
            members.push(m_fn("sort", "*", &[], false));
            members.push(m_fn("sortOn", "*", &[("*", false), ("*", true)], false));
            members.push(m_fn("join", "String", &[("*", true)], false));
        }
        "Error" | "TypeError" | "RangeError" | "ReferenceError" | "ArgumentError"
        | "SecurityError" | "SyntaxError" | "URIError" | "VerifyError" | "EvalError" => {
            ctor.push(param("*", true));
            ctor.push(param("*", true));
            if name == "Error" {
                members.push(m_var("message", "String", false));
                members.push(m_var("name", "String", false));
                members.push(m_get("errorID", "int"));
                members.push(m_fn("getStackTrace", "String", &[], false));
            }
        }
        "XML" => {
            ctor.push(param("*", true));
            for p in [
                "ignoreComments",
                "ignoreProcessingInstructions",
                "ignoreWhitespace",
                "prettyPrinting",
            ] {
                members.push(m_var(p, "Boolean", true));
            }
            members.push(m_var("prettyIndent", "int", true));
            members.push(m_fn("children", "XMLList", &[], false));
            members.push(m_fn("elements", "XMLList", &[("*", true)], false));
            members.push(m_fn("attributes", "XMLList", &[], false));
            members.push(m_fn("attribute", "XMLList", &[("*", false)], false));
            members.push(m_fn("appendChild", "XML", &[("Object", false)], false));
            members.push(m_fn("copy", "XML", &[], false));
            members.push(m_fn("length", "int", &[], false));
            members.push(m_fn("toXMLString", "String", &[], false));
        }
        "XMLList" => {
            ctor.push(param("*", true));
            members.push(m_fn("children", "XMLList", &[], false));
            members.push(m_fn("elements", "XMLList", &[("*", true)], false));
            members.push(m_fn("attributes", "XMLList", &[], false));
            members.push(m_fn("copy", "XMLList", &[], false));
            members.push(m_fn("length", "int", &[], false));
            members.push(m_fn("toXMLString", "String", &[], false));
        }
        "Namespace" => {
            ctor.push(param("*", true));
            ctor.push(param("*", true));
            members.push(m_get("prefix", "String"));
            members.push(m_get("uri", "String"));
        }
        "QName" => {
            ctor.push(param("*", true));
            ctor.push(param("*", true));
            members.push(m_get("localName", "String"));
            members.push(m_get("uri", "String"));
        }
        "flash.events.Event" => {
            ctor.push(param("String", false));
            ctor.push(param("Boolean", true));
            ctor.push(param("Boolean", true));
            members.push(m_get("type", "String"));
            members.push(m_get("bubbles", "Boolean"));
            members.push(m_get("cancelable", "Boolean"));
            members.push(m_get("target", "Object"));
            members.push(m_get("currentTarget", "Object"));
            members.push(m_get("eventPhase", "uint"));
            members.push(m_fn("clone", "flash.events.Event", &[], false));
            members.push(m_fn("preventDefault", "void", &[], false));
            members.push(m_fn("isDefaultPrevented", "Boolean", &[], false));
            members.push(m_fn("stopPropagation", "void", &[], false));
            members.push(m_fn("stopImmediatePropagation", "void", &[], false));
        }
        "flash.events.EventDispatcher" => {
            ctor.push(param("flash.events.IEventDispatcher", true));
            members.push(m_fn(
                "addEventListener",
                "void",
                &[
                    ("String", false),
                    ("Function", false),
                    ("Boolean", true),
                    ("int", true),
                    ("Boolean", true),
                ],
                false,
            ));
            members.push(m_fn(
                "removeEventListener",
                "void",
                &[("String", false), ("Function", false), ("Boolean", true)],
                false,
            ));
            members.push(m_fn(
                "dispatchEvent",
                "Boolean",
                &[("flash.events.Event", false)],
                false,
            ));
            members.push(m_fn(
                "hasEventListener",
                "Boolean",
                &[("String", false)],
                false,
            ));
            members.push(m_fn("willTrigger", "Boolean", &[("String", false)], false));
        }
        "flash.display.DisplayObject" => {
            for p in [
                "x",
                "y",
                "z",
                "width",
                "height",
                "scaleX",
                "scaleY",
                "scaleZ",
                "rotation",
                "rotationX",
                "rotationY",
                "rotationZ",
                "alpha",
            ] {
                members.extend(m_rw(p, "Number"));
            }
            members.extend(m_rw("visible", "Boolean"));
            members.extend(m_rw("cacheAsBitmap", "Boolean"));
            members.extend(m_rw("name", "String"));
            members.extend(m_rw("blendMode", "String"));
            members.extend(m_rw("filters", "Array"));
            members.extend(m_rw("mask", "flash.display.DisplayObject"));
            members.extend(m_rw("scrollRect", "flash.geom.Rectangle"));
            members.extend(m_rw("scale9Grid", "flash.geom.Rectangle"));
            members.extend(m_rw("transform", "flash.geom.Transform"));
            members.push(m_get("parent", "flash.display.DisplayObjectContainer"));
            members.push(m_get("root", "flash.display.DisplayObject"));
            members.push(m_get("stage", "flash.display.Stage"));
            members.push(m_get("loaderInfo", "flash.display.LoaderInfo"));
            members.push(m_get("mouseX", "Number"));
            members.push(m_get("mouseY", "Number"));
            members.push(m_fn(
                "getBounds",
                "flash.geom.Rectangle",
                &[("flash.display.DisplayObject", false)],
                false,
            ));
            members.push(m_fn(
                "getRect",
                "flash.geom.Rectangle",
                &[("flash.display.DisplayObject", false)],
                false,
            ));
            members.push(m_fn(
                "globalToLocal",
                "flash.geom.Point",
                &[("flash.geom.Point", false)],
                false,
            ));
            members.push(m_fn(
                "localToGlobal",
                "flash.geom.Point",
                &[("flash.geom.Point", false)],
                false,
            ));
            members.push(m_fn(
                "hitTestObject",
                "Boolean",
                &[("flash.display.DisplayObject", false)],
                false,
            ));
            members.push(m_fn(
                "hitTestPoint",
                "Boolean",
                &[("Number", false), ("Number", false), ("Boolean", true)],
                false,
            ));
        }
        "flash.display.InteractiveObject" => {
            members.extend(m_rw("mouseEnabled", "Boolean"));
            members.extend(m_rw("doubleClickEnabled", "Boolean"));
            members.extend(m_rw("tabEnabled", "Boolean"));
            members.extend(m_rw("tabIndex", "int"));
            members.extend(m_rw("focusRect", "Object"));
            members.extend(m_rw("contextMenu", "flash.display.NativeMenu"));
        }
        "flash.display.DisplayObjectContainer" => {
            members.push(m_get("numChildren", "int"));
            members.extend(m_rw("mouseChildren", "Boolean"));
            members.extend(m_rw("tabChildren", "Boolean"));
            members.push(m_fn(
                "addChild",
                "flash.display.DisplayObject",
                &[("flash.display.DisplayObject", false)],
                false,
            ));
            members.push(m_fn(
                "addChildAt",
                "flash.display.DisplayObject",
                &[("flash.display.DisplayObject", false), ("int", false)],
                false,
            ));
            members.push(m_fn(
                "removeChild",
                "flash.display.DisplayObject",
                &[("flash.display.DisplayObject", false)],
                false,
            ));
            members.push(m_fn(
                "removeChildAt",
                "flash.display.DisplayObject",
                &[("int", false)],
                false,
            ));
            members.push(m_fn(
                "removeChildren",
                "void",
                &[("int", true), ("int", true)],
                false,
            ));
            members.push(m_fn(
                "getChildAt",
                "flash.display.DisplayObject",
                &[("int", false)],
                false,
            ));
            members.push(m_fn(
                "getChildByName",
                "flash.display.DisplayObject",
                &[("String", false)],
                false,
            ));
            members.push(m_fn(
                "getChildIndex",
                "int",
                &[("flash.display.DisplayObject", false)],
                false,
            ));
            members.push(m_fn(
                "setChildIndex",
                "void",
                &[("flash.display.DisplayObject", false), ("int", false)],
                false,
            ));
            members.push(m_fn(
                "swapChildren",
                "void",
                &[
                    ("flash.display.DisplayObject", false),
                    ("flash.display.DisplayObject", false),
                ],
                false,
            ));
            members.push(m_fn(
                "swapChildrenAt",
                "void",
                &[("int", false), ("int", false)],
                false,
            ));
            members.push(m_fn(
                "contains",
                "Boolean",
                &[("flash.display.DisplayObject", false)],
                false,
            ));
            members.push(m_fn(
                "getObjectsUnderPoint",
                "Array",
                &[("flash.geom.Point", false)],
                false,
            ));
        }
        "flash.display.Sprite" => {
            members.push(m_get("graphics", "flash.display.Graphics"));
            members.extend(m_rw("buttonMode", "Boolean"));
            members.extend(m_rw("useHandCursor", "Boolean"));
            members.extend(m_rw("hitArea", "flash.display.Sprite"));
            members.extend(m_rw("soundTransform", "flash.media.SoundTransform"));
            members.push(m_get("dropTarget", "flash.display.DisplayObject"));
            members.push(m_fn(
                "startDrag",
                "void",
                &[("Boolean", true), ("flash.geom.Rectangle", true)],
                false,
            ));
            members.push(m_fn("stopDrag", "void", &[], false));
        }
        "flash.display.MovieClip" => {
            members.push(m_get("currentFrame", "int"));
            members.push(m_get("framesLoaded", "int"));
            members.push(m_get("totalFrames", "int"));
            members.push(m_get("currentLabel", "String"));
            members.push(m_get("currentFrameLabel", "String"));
            members.push(m_get("currentLabels", "Array"));
            members.push(m_get("currentScene", "flash.display.Scene"));
            members.push(m_get("scenes", "Array"));
            members.push(m_get("isPlaying", "Boolean"));
            members.extend(m_rw("enabled", "Boolean"));
            members.extend(m_rw("trackAsMenu", "Boolean"));
            members.push(m_fn("play", "void", &[], false));
            members.push(m_fn("stop", "void", &[], false));
            members.push(m_fn("nextFrame", "void", &[], false));
            members.push(m_fn("prevFrame", "void", &[], false));
            members.push(m_fn("nextScene", "void", &[], false));
            members.push(m_fn("prevScene", "void", &[], false));
            members.push(m_fn(
                "gotoAndPlay",
                "void",
                &[("Object", false), ("String", true)],
                false,
            ));
            members.push(m_fn(
                "gotoAndStop",
                "void",
                &[("Object", false), ("String", true)],
                false,
            ));
            members.push(m_fn("addFrameScript", "void", &[], false));
        }
        "flash.display.Shape" => {
            members.push(m_get("graphics", "flash.display.Graphics"));
        }
        "flash.display.Bitmap" => {
            ctor.push(param("flash.display.BitmapData", true));
            ctor.push(param("String", true));
            ctor.push(param("Boolean", true));
            members.extend(m_rw("bitmapData", "flash.display.BitmapData"));
            members.extend(m_rw("pixelSnapping", "String"));
            members.extend(m_rw("smoothing", "Boolean"));
        }
        "flash.display.BitmapData" => {
            ctor.push(param("int", false));
            ctor.push(param("int", false));
            ctor.push(param("Boolean", true));
            ctor.push(param("uint", true));
            members.push(m_get("width", "int"));
            members.push(m_get("height", "int"));
            members.push(m_get("transparent", "Boolean"));
            members.push(m_get("rect", "flash.geom.Rectangle"));
            members.push(m_fn("clone", "flash.display.BitmapData", &[], false));
            members.push(m_fn("dispose", "void", &[], false));
            members.push(m_fn(
                "getPixel",
                "uint",
                &[("int", false), ("int", false)],
                false,
            ));
            members.push(m_fn(
                "getPixel32",
                "uint",
                &[("int", false), ("int", false)],
                false,
            ));
            members.push(m_fn(
                "setPixel",
                "void",
                &[("int", false), ("int", false), ("uint", false)],
                false,
            ));
            members.push(m_fn(
                "setPixel32",
                "void",
                &[("int", false), ("int", false), ("uint", false)],
                false,
            ));
            members.push(m_fn(
                "fillRect",
                "void",
                &[("flash.geom.Rectangle", false), ("uint", false)],
                false,
            ));
            members.push(m_fn(
                "draw",
                "void",
                &[
                    ("flash.display.IBitmapDrawable", false),
                    ("flash.geom.Matrix", true),
                    ("flash.geom.ColorTransform", true),
                    ("String", true),
                    ("flash.geom.Rectangle", true),
                    ("Boolean", true),
                ],
                false,
            ));
            members.push(m_fn(
                "drawWithQuality",
                "void",
                &[
                    ("flash.display.IBitmapDrawable", false),
                    ("flash.geom.Matrix", true),
                    ("flash.geom.ColorTransform", true),
                    ("String", true),
                    ("flash.geom.Rectangle", true),
                    ("Boolean", true),
                    ("String", true),
                ],
                false,
            ));
            members.push(m_fn(
                "copyPixels",
                "void",
                &[
                    ("flash.display.BitmapData", false),
                    ("flash.geom.Rectangle", false),
                    ("flash.geom.Point", false),
                    ("flash.display.BitmapData", true),
                    ("flash.geom.Point", true),
                    ("Boolean", true),
                ],
                false,
            ));
        }
        "flash.display.SimpleButton" => {
            ctor.push(param("flash.display.DisplayObject", true));
            ctor.push(param("flash.display.DisplayObject", true));
            ctor.push(param("flash.display.DisplayObject", true));
            ctor.push(param("flash.display.DisplayObject", true));
            for s in ["upState", "overState", "downState", "hitTestState"] {
                members.extend(m_rw(s, "flash.display.DisplayObject"));
            }
            members.extend(m_rw("enabled", "Boolean"));
            members.extend(m_rw("useHandCursor", "Boolean"));
            members.extend(m_rw("trackAsMenu", "Boolean"));
            members.extend(m_rw("soundTransform", "flash.media.SoundTransform"));
        }
        "flash.display.Stage" => {
            members.push(m_get("stageWidth", "int"));
            members.push(m_get("stageHeight", "int"));
            members.extend(m_rw("scaleMode", "String"));
            members.extend(m_rw("align", "String"));
            members.extend(m_rw("quality", "String"));
            members.extend(m_rw("frameRate", "Number"));
            members.extend(m_rw("color", "uint"));
            members.extend(m_rw("displayState", "String"));
            members.extend(m_rw("focus", "flash.display.InteractiveObject"));
            members.extend(m_rw("stageFocusRect", "Boolean"));
            members.extend(m_rw("showDefaultContextMenu", "Boolean"));
            members.push(m_fn("invalidate", "void", &[], false));
        }
        "flash.display.Loader" => {
            members.push(m_get("content", "flash.display.DisplayObject"));
            members.push(m_get("contentLoaderInfo", "flash.display.LoaderInfo"));
            members.push(m_fn(
                "load",
                "void",
                &[
                    ("flash.net.URLRequest", false),
                    ("flash.system.LoaderContext", true),
                ],
                false,
            ));
            members.push(m_fn(
                "loadBytes",
                "void",
                &[
                    ("flash.utils.ByteArray", false),
                    ("flash.system.LoaderContext", true),
                ],
                false,
            ));
            members.push(m_fn("close", "void", &[], false));
            members.push(m_fn("unload", "void", &[], false));
            members.push(m_fn("unloadAndStop", "void", &[("Boolean", true)], false));
        }
        "flash.display.LoaderInfo" => {
            members.push(m_get("bytesLoaded", "uint"));
            members.push(m_get("bytesTotal", "uint"));
            members.push(m_get("content", "flash.display.DisplayObject"));
            members.push(m_get("loader", "flash.display.Loader"));
            members.push(m_get("url", "String"));
            members.push(m_get("width", "int"));
            members.push(m_get("height", "int"));
            members.push(m_get("frameRate", "Number"));
            members.push(m_get("parameters", "Object"));
        }
        "flash.text.TextField" => {
            for p in [
                "text",
                "htmlText",
                "type",
                "autoSize",
                "antiAliasType",
                "gridFitType",
                "restrict",
            ] {
                members.extend(m_rw(p, "String"));
            }
            for p in [
                "wordWrap",
                "multiline",
                "selectable",
                "border",
                "background",
                "embedFonts",
                "displayAsPassword",
            ] {
                members.extend(m_rw(p, "Boolean"));
            }
            for p in ["textColor", "borderColor", "backgroundColor"] {
                members.extend(m_rw(p, "uint"));
            }
            members.extend(m_rw("maxChars", "int"));
            members.extend(m_rw("scrollV", "int"));
            members.extend(m_rw("scrollH", "int"));
            members.extend(m_rw("defaultTextFormat", "flash.text.TextFormat"));
            members.push(m_get("length", "int"));
            members.push(m_get("textWidth", "Number"));
            members.push(m_get("textHeight", "Number"));
            members.push(m_get("numLines", "int"));
            members.push(m_get("selectionBeginIndex", "int"));
            members.push(m_get("selectionEndIndex", "int"));
            members.push(m_fn("appendText", "void", &[("String", false)], false));
            members.push(m_fn(
                "setSelection",
                "void",
                &[("int", false), ("int", false)],
                false,
            ));
            members.push(m_fn(
                "getTextFormat",
                "flash.text.TextFormat",
                &[("int", true), ("int", true)],
                false,
            ));
            members.push(m_fn(
                "setTextFormat",
                "void",
                &[
                    ("flash.text.TextFormat", false),
                    ("int", true),
                    ("int", true),
                ],
                false,
            ));
        }
        "flash.media.Video" => {
            ctor.push(param("int", true));
            ctor.push(param("int", true));
            members.push(m_get("videoWidth", "int"));
            members.push(m_get("videoHeight", "int"));
            members.extend(m_rw("smoothing", "Boolean"));
            members.extend(m_rw("deblocking", "int"));
            members.push(m_fn(
                "attachNetStream",
                "void",
                &[("flash.net.NetStream", false)],
                false,
            ));
            members.push(m_fn("clear", "void", &[], false));
        }
        "flash.media.Sound" => {
            ctor.push(param("flash.net.URLRequest", true));
            ctor.push(param("flash.media.SoundLoaderContext", true));
            members.push(m_get("bytesLoaded", "uint"));
            members.push(m_get("bytesTotal", "int"));
            members.push(m_get("length", "Number"));
            members.push(m_get("url", "String"));
            members.push(m_fn(
                "load",
                "void",
                &[
                    ("flash.net.URLRequest", false),
                    ("flash.media.SoundLoaderContext", true),
                ],
                false,
            ));
            members.push(m_fn(
                "play",
                "flash.media.SoundChannel",
                &[
                    ("Number", true),
                    ("int", true),
                    ("flash.media.SoundTransform", true),
                ],
                false,
            ));
            members.push(m_fn("close", "void", &[], false));
        }
        "flash.utils.ByteArray" => {
            members.extend(m_rw("length", "uint"));
            members.extend(m_rw("position", "uint"));
            members.push(m_get("bytesAvailable", "uint"));
            members.extend(m_rw("endian", "String"));
            members.extend(m_rw("objectEncoding", "uint"));
            members.push(m_fn("clear", "void", &[], false));
            members.push(m_fn("compress", "void", &[("String", true)], false));
            members.push(m_fn("uncompress", "void", &[("String", true)], false));
            members.push(m_fn("readByte", "int", &[], false));
            members.push(m_fn("writeByte", "void", &[("int", false)], false));
            members.push(m_fn("readInt", "int", &[], false));
            members.push(m_fn("writeInt", "void", &[("int", false)], false));
            members.push(m_fn("readUTF", "String", &[], false));
            members.push(m_fn("writeUTF", "void", &[("String", false)], false));
            members.push(m_fn("readUTFBytes", "String", &[("uint", false)], false));
            members.push(m_fn("writeUTFBytes", "void", &[("String", false)], false));
            members.push(m_fn("readObject", "*", &[], false));
            members.push(m_fn("writeObject", "void", &[("*", false)], false));
        }
        "flash.utils.Dictionary" => {
            ctor.push(param("Boolean", true));
        }
        "flash.utils.Timer" => {
            ctor.push(param("Number", false));
            ctor.push(param("int", true));
            members.extend(m_rw("delay", "Number"));
            members.extend(m_rw("repeatCount", "int"));
            members.push(m_get("currentCount", "int"));
            members.push(m_get("running", "Boolean"));
            members.push(m_fn("start", "void", &[], false));
            members.push(m_fn("stop", "void", &[], false));
            members.push(m_fn("reset", "void", &[], false));
        }
        "flash.geom.Point" => {
            ctor.push(param("Number", true));
            ctor.push(param("Number", true));
            members.push(m_var("x", "Number", false));
            members.push(m_var("y", "Number", false));
            members.push(m_get("length", "Number"));
            members.push(m_fn(
                "add",
                "flash.geom.Point",
                &[("flash.geom.Point", false)],
                false,
            ));
            members.push(m_fn(
                "subtract",
                "flash.geom.Point",
                &[("flash.geom.Point", false)],
                false,
            ));
            members.push(m_fn("clone", "flash.geom.Point", &[], false));
            members.push(m_fn(
                "equals",
                "Boolean",
                &[("flash.geom.Point", false)],
                false,
            ));
            members.push(m_fn("normalize", "void", &[("Number", false)], false));
            members.push(m_fn(
                "offset",
                "void",
                &[("Number", false), ("Number", false)],
                false,
            ));
            members.push(m_fn(
                "distance",
                "Number",
                &[("flash.geom.Point", false), ("flash.geom.Point", false)],
                true,
            ));
            members.push(m_fn(
                "interpolate",
                "flash.geom.Point",
                &[
                    ("flash.geom.Point", false),
                    ("flash.geom.Point", false),
                    ("Number", false),
                ],
                true,
            ));
            members.push(m_fn(
                "polar",
                "flash.geom.Point",
                &[("Number", false), ("Number", false)],
                true,
            ));
        }
        "flash.geom.Rectangle" => {
            for _ in 0..4 {
                ctor.push(param("Number", true));
            }
            for f in ["x", "y", "width", "height"] {
                members.push(m_var(f, "Number", false));
            }
            for p in ["left", "right", "top", "bottom"] {
                members.extend(m_rw(p, "Number"));
            }
            for p in ["topLeft", "bottomRight", "size"] {
                members.extend(m_rw(p, "flash.geom.Point"));
            }
            members.push(m_fn("clone", "flash.geom.Rectangle", &[], false));
            members.push(m_fn(
                "contains",
                "Boolean",
                &[("Number", false), ("Number", false)],
                false,
            ));
            members.push(m_fn(
                "containsPoint",
                "Boolean",
                &[("flash.geom.Point", false)],
                false,
            ));
            members.push(m_fn(
                "containsRect",
                "Boolean",
                &[("flash.geom.Rectangle", false)],
                false,
            ));
            members.push(m_fn(
                "intersects",
                "Boolean",
                &[("flash.geom.Rectangle", false)],
                false,
            ));
            members.push(m_fn(
                "intersection",
                "flash.geom.Rectangle",
                &[("flash.geom.Rectangle", false)],
                false,
            ));
            members.push(m_fn(
                "union",
                "flash.geom.Rectangle",
                &[("flash.geom.Rectangle", false)],
                false,
            ));
            members.push(m_fn("isEmpty", "Boolean", &[], false));
            members.push(m_fn("setEmpty", "void", &[], false));
        }
        "flash.geom.Matrix" => {
            for _ in 0..6 {
                ctor.push(param("Number", true));
            }
            for f in ["a", "b", "c", "d", "tx", "ty"] {
                members.push(m_var(f, "Number", false));
            }
            members.push(m_fn("clone", "flash.geom.Matrix", &[], false));
            members.push(m_fn(
                "concat",
                "void",
                &[("flash.geom.Matrix", false)],
                false,
            ));
            members.push(m_fn("identity", "void", &[], false));
            members.push(m_fn("invert", "void", &[], false));
            members.push(m_fn("rotate", "void", &[("Number", false)], false));
            members.push(m_fn(
                "scale",
                "void",
                &[("Number", false), ("Number", false)],
                false,
            ));
            members.push(m_fn(
                "translate",
                "void",
                &[("Number", false), ("Number", false)],
                false,
            ));
            members.push(m_fn(
                "transformPoint",
                "flash.geom.Point",
                &[("flash.geom.Point", false)],
                false,
            ));
            members.push(m_fn(
                "deltaTransformPoint",
                "flash.geom.Point",
                &[("flash.geom.Point", false)],
                false,
            ));
        }
        _ => {}
    }

    Class {
        name: name.into(),
        base: native_base_lookup(name).map(str::to_owned),
        public: true,
        dynamic: native_is_dynamic(name),
        is_final: native_is_final(name),
        interface: false,
        interfaces: native_interfaces_for(name),
        members,
        constructor: ctor,
        constructor_rest: false,
    }
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Object metadata requires string {key}"))
}
fn boolean(value: &Value, key: &str) -> Result<bool, String> {
    value
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("Object metadata requires Boolean {key}"))
}
fn qualified(name: &str) -> String {
    if let Some(element) = name
        .strip_prefix("Vector.<")
        .and_then(|name| name.strip_suffix('>'))
    {
        format!("__AS3__.vec::Vector.<{}>", qualified(element))
    } else if name.contains("::") {
        name.into()
    } else if let Some((package, local)) = name.rsplit_once('.') {
        format!("{package}::{local}")
    } else {
        name.into()
    }
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn attr(name: &str, value: &str) -> String {
    format!(" {name}=\"{}\"", escape(value))
}
fn property(request: &Value) -> Result<Value, String> {
    let method = string(request, "method")?;
    if string(request, "name")?.len() > 1_000_000 {
        return Err("Object property name exceeds 1000000 bytes".into());
    }
    if !boolean(request, "supported")? {
        return Err("Object property queries support plain records and ordinary AS3 project classes; native and AS2 trait reflection is unsupported".into());
    }
    let own = boolean(request, "own")?;
    let fixed = boolean(request, "fixed")?;
    match method {
        "hasOwnProperty" => Ok(json!({"value": own || fixed, "change_enumerable": null})),
        "propertyIsEnumerable" => Ok(
            json!({"value": own && !fixed && boolean(request, "enumerable")?, "change_enumerable": null}),
        ),
        "setPropertyIsEnumerable" => {
            let desired = boolean(request, "is_enum")?;
            let change = if own && !fixed {
                if !boolean(request, "configurable")? {
                    return Err(
                        "Cannot change enumerability of this nonconfigurable host property".into(),
                    );
                }
                Some(desired)
            } else {
                None
            };
            Ok(json!({"value": null, "change_enumerable": change}))
        }
        _ => Err(format!("Unsupported Object property operation {method}")),
    }
}
fn classes(request: &Value) -> Result<BTreeMap<String, Class>, String> {
    let values: Vec<Class> =
        serde_json::from_value(request.get("classes").cloned().unwrap_or_else(|| json!([])))
            .map_err(|e| format!("Invalid source reflection metadata: {e}"))?;
    if values.len() > MAX_CLASSES
        || values.iter().map(|c| c.members.len()).sum::<usize>() > MAX_MEMBERS
    {
        return Err("Source reflection exceeds 4096 classes or 32768 traits".into());
    }
    let mut result = BTreeMap::new();
    for class in values {
        if result.insert(class.name.clone(), class).is_some() {
            return Err("Duplicate source reflection class".into());
        }
    }
    if let Some(native_extra) = request.get("native_classes").and_then(Value::as_array) {
        for raw in native_extra {
            if let Ok(extra) = serde_json::from_value::<Class>(raw.clone())
                && !result.contains_key(&extra.name)
            {
                let mut merged = build_native_class(&extra.name);
                let existing: BTreeSet<(String, bool)> = merged
                    .members
                    .iter()
                    .map(|m| (m.name.clone(), m.is_static))
                    .collect();
                for m in extra.members {
                    if !existing.contains(&(m.name.clone(), m.is_static)) {
                        merged.members.push(m);
                    }
                }
                result.insert(merged.name.clone(), merged);
            }
        }
    }
    Ok(result)
}
fn value_name(request: &Value) -> Result<String, String> {
    let value = request
        .get("value")
        .ok_or("Missing reflection value metadata")?;
    Ok(match string(value, "kind")? {
        "project" | "class" | "native" => string(value, "name")?.into(),
        "undefined" => "void".into(),
        "null" => "null".into(),
        "boolean" => "Boolean".into(),
        "string" => "String".into(),
        "number" => value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Number")
            .into(),
        "array" => "Array".into(),
        "date" => "Date".into(),
        "regexp" => "RegExp".into(),
        "function" => "Function".into(),
        "object" => "Object".into(),
        _ => {
            return Err("Reflection of this foreign value is unsupported in ActionScript 3".into());
        }
    })
}
fn base_name(name: &str, classes: &BTreeMap<String, Class>) -> Result<Option<String>, String> {
    if let Some(class) = classes.get(name) {
        return Ok(if class.interface || class.name == "Object" {
            None
        } else {
            Some(class.base.clone().unwrap_or_else(|| "Object".into()))
        });
    }
    Ok(native_base_lookup(name).map(str::to_owned))
}
fn ensure_lineage_populated(
    name: &str,
    classes: &mut BTreeMap<String, Class>,
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    let mut current = name.to_owned();
    loop {
        if current == "Object" || current == "null" || current == "void" {
            break;
        }
        if !seen.insert(current.clone()) {
            return Err("Cyclic reflection inheritance".into());
        }
        if !classes.contains_key(&current) {
            classes.insert(current.clone(), build_native_class(&current));
        }
        let class = &classes[&current];
        if class.interface {
            break;
        }
        current = class.base.clone().unwrap_or_else(|| "Object".into());
    }
    Ok(())
}
fn lineage<'a>(name: &str, classes: &'a BTreeMap<String, Class>) -> Result<Vec<&'a Class>, String> {
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current = name;
    while current != "Object" {
        if !seen.insert(current) {
            return Err("Cyclic reflection inheritance".into());
        }
        let class = classes
            .get(current)
            .ok_or_else(|| format!("describeType cannot resolve class {current}"))?;
        result.push(class);
        if class.interface {
            break;
        }
        current = class.base.as_deref().unwrap_or("Object");
    }
    Ok(result)
}
fn parameters(out: &mut String, values: &[Parameter]) {
    for (index, parameter) in values.iter().enumerate() {
        out.push_str(&format!(
            "<parameter{}{}{}/>",
            attr("index", &(index + 1).to_string()),
            attr("type", &qualified(&parameter.ty)),
            attr(
                "optional",
                if parameter.optional { "true" } else { "false" }
            )
        ));
    }
}
fn traits(out: &mut String, chain: &[&Class], is_static: bool) -> Result<(), String> {
    let mut members = BTreeMap::<String, (&Member, &str, bool, bool)>::new();
    let definitions: Vec<_> = if is_static {
        chain.first().copied().into_iter().collect()
    } else {
        chain.iter().copied().rev().collect()
    };
    for class in definitions {
        for member in &class.members {
            if member.access != "public" || member.is_static != is_static {
                continue;
            }
            let previous = members.get(&member.name);
            let read = member.kind == "get" || previous.is_some_and(|p| p.2);
            let write = member.kind == "set" || previous.is_some_and(|p| p.3);
            members.insert(member.name.clone(), (member, &class.name, read, write));
        }
    }
    for (member, owner, read, write) in members.values() {
        let kind = match member.kind.as_str() {
            "get" | "set" => "accessor",
            "method" => "method",
            "constant" => "constant",
            "variable" => "variable",
            _ => return Err("Unknown reflection trait kind".into()),
        };
        out.push('<');
        out.push_str(kind);
        out.push_str(&attr("name", &member.name));
        out.push_str(&attr(
            if kind == "method" {
                "returnType"
            } else {
                "type"
            },
            &qualified(&member.ty),
        ));
        if matches!(kind, "method" | "accessor") {
            out.push_str(&attr("declaredBy", &qualified(owner)));
        }
        if kind == "accessor" {
            out.push_str(&attr(
                "access",
                match (*read, *write) {
                    (true, true) => "readwrite",
                    (true, false) => "readonly",
                    _ => "writeonly",
                },
            ));
        }
        if kind == "method" && !member.parameters.is_empty() {
            out.push('>');
            parameters(out, &member.parameters);
            out.push_str("</method>");
        } else if kind == "method" {
            out.push_str("></method>");
        } else {
            out.push_str("/>");
        }
    }
    Ok(())
}
fn instance_body(
    out: &mut String,
    chain: &[&Class],
    include_constructor: bool,
) -> Result<(), String> {
    if chain.is_empty() {
        return Ok(());
    }
    let head = chain[0];
    if head.interface {
        for interface in &head.interfaces {
            out.push_str(&format!(
                "<extendsInterface{}/>",
                attr("type", &qualified(interface))
            ));
        }
        return traits(out, chain, false);
    }
    for class in chain.iter().skip(1) {
        out.push_str(&format!(
            "<extendsClass{}/>",
            attr("type", &qualified(&class.name))
        ));
    }
    out.push_str("<extendsClass type=\"Object\"/>");
    let interfaces: BTreeSet<_> = chain
        .iter()
        .flat_map(|class| class.interfaces.iter())
        .collect();
    for interface in interfaces {
        out.push_str(&format!(
            "<implementsInterface{}/>",
            attr("type", &qualified(interface))
        ));
    }
    if include_constructor && !head.constructor.is_empty() {
        out.push_str("<constructor>");
        parameters(out, &head.constructor);
        out.push_str("</constructor>");
    }
    traits(out, chain, false)
}
fn describe(
    request: &Value,
    name: &str,
    classes: &mut BTreeMap<String, Class>,
) -> Result<String, String> {
    let is_static = string(&request["value"], "kind")? == "class";
    if matches!(name, "null" | "void") {
        return Ok(format!(
            "<type name=\"{name}\" isDynamic=\"false\" isFinal=\"true\" isStatic=\"false\"/>"
        ));
    }
    if name == "Object" {
        if is_static {
            return Ok("<type name=\"Object\" base=\"Class\" isDynamic=\"true\" isFinal=\"true\" isStatic=\"true\"><extendsClass type=\"Class\"/><extendsClass type=\"Object\"/><factory type=\"Object\"/></type>".into());
        }
        return Ok(
            "<type name=\"Object\" base=\"\" isDynamic=\"true\" isFinal=\"false\" isStatic=\"false\"/>"
                .into(),
        );
    }
    ensure_lineage_populated(name, classes)?;
    let chain = lineage(name, classes)?;
    let class = chain
        .first()
        .ok_or("describeType could not resolve target type")?;
    let base = if is_static {
        "Class".into()
    } else if class.interface {
        String::new()
    } else {
        class
            .base
            .as_deref()
            .map(qualified)
            .unwrap_or_else(|| "Object".into())
    };
    let mut out = format!(
        "<type{}{}{}{}{}>",
        attr("name", &qualified(name)),
        attr("base", &base),
        attr(
            "isDynamic",
            if is_static || class.dynamic {
                "true"
            } else {
                "false"
            }
        ),
        attr(
            "isFinal",
            if is_static || class.is_final {
                "true"
            } else {
                "false"
            }
        ),
        attr("isStatic", if is_static { "true" } else { "false" })
    );
    if is_static {
        out.push_str("<extendsClass type=\"Class\"/><extendsClass type=\"Object\"/>");
        traits(&mut out, &chain, true)?;
        out.push_str(&format!("<factory{}>", attr("type", &qualified(name))));
        instance_body(&mut out, &chain, !class.interface)?;
        out.push_str("</factory>");
    } else {
        instance_body(&mut out, &chain, !class.constructor.is_empty())?;
    }
    out.push_str("</type>");
    Ok(out)
}
pub fn execute(request: Value) -> Result<Value, String> {
    match string(&request, "op")? {
        "property" => property(&request),
        "default_string" => {
            let name = string(&request, "name")?;
            if name.is_empty() || name.len() > 4096 {
                return Err("Object class label requires 1 through 4096 bytes".into());
            }
            let short = name
                .rsplit("::")
                .next()
                .unwrap()
                .rsplit('.')
                .next()
                .unwrap();
            Ok(json!(format!(
                "[{} {short}]",
                if boolean(&request, "class_object")? {
                    "class"
                } else {
                    "object"
                }
            )))
        }
        "getDefinitionByName" => {
            let name = string(&request, "name")?;
            if name.len() > 4096 {
                return Err("Definition name exceeds 4096 bytes".into());
            }
            let raw_canonical = name.replace("::", ".");
            let canonical = raw_canonical
                .strip_prefix("__AS3__.vec.")
                .map(str::to_owned)
                .unwrap_or(raw_canonical);
            let classes = classes(&request)?;
            let names = request["definitions"]
                .as_array()
                .ok_or("Missing public definition list")?;
            if names.len() > MAX_CLASSES {
                return Err("Definition lookup exceeds 4096 names".into());
            }
            if (canonical.starts_with("Vector.<") && canonical.ends_with('>'))
                || classes.get(&canonical).is_some_and(|class| class.public)
                || names.iter().any(|value| value.as_str() == Some(&canonical))
            {
                Ok(json!(canonical))
            } else {
                Err(format!("No public definition exists with the name {name}"))
            }
        }
        "getQualifiedClassName" => Ok(json!(qualified(&value_name(&request)?))),
        "getQualifiedSuperclassName" => Ok(base_name(&value_name(&request)?, &classes(&request)?)?
            .map(|b| qualified(&b))
            .map(Value::String)
            .unwrap_or(Value::Null)),
        "describeType" => {
            let mut cls_map = classes(&request)?;
            let target_name = value_name(&request)?;
            Ok(json!(describe(&request, &target_name, &mut cls_map)?))
        }
        operation => Err(format!("Unsupported AS3 Object operation {operation}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_traits_and_dynamic_flags_are_distinct() {
        let mut request = json!({"op":"property","method":"setPropertyIsEnumerable","name":"count","own":true,"fixed":true,"enumerable":true,"configurable":true,"supported":true,"is_enum":true});
        assert_eq!(
            execute(request.clone()).unwrap()["change_enumerable"],
            Value::Null
        );
        request["method"] = json!("propertyIsEnumerable");
        assert_eq!(execute(request.clone()).unwrap()["value"], false);
        request["fixed"] = json!(false);
        request["method"] = json!("setPropertyIsEnumerable");
        request["is_enum"] = json!(false);
        assert_eq!(execute(request).unwrap()["change_enumerable"], false);
    }
    #[test]
    fn default_labels_use_local_trait_names() {
        assert_eq!(
            execute(json!({"op":"default_string","name":"app.Item","class_object":false})).unwrap(),
            "[object Item]"
        );
        assert_eq!(
            execute(json!({"op":"default_string","name":"app::Item","class_object":true})).unwrap(),
            "[class Item]"
        );
    }
    #[test]
    fn reflection_uses_public_traits_without_reading_values() {
        let classes = json!([
            {"name":"model.Base","base":null,"public":true,"dynamic":false,"final":false,"members":[{"name":"secret","kind":"variable","access":"private","static":false,"type":"String"},{"name":"score","kind":"get","access":"public","static":false,"type":"int"}]},
            {"name":"model.Item","base":"model.Base","public":true,"dynamic":true,"final":false,"members":[{"name":"score","kind":"set","access":"public","static":false,"type":"int"},{"name":"make","kind":"method","access":"public","static":true,"type":"model.Item","parameters":[{"type":"int","optional":true}]}]}
        ]);
        let output = execute(json!({"op":"describeType","value":{"kind":"class","name":"model.Item"},"classes":classes})).unwrap();
        let xml = output.as_str().unwrap();
        assert!(xml.contains("name=\"model::Item\""));
        assert!(xml.contains("<factory type=\"model::Item\""));
        assert!(xml.contains("access=\"readwrite\""));
        assert!(xml.contains("optional=\"true\""));
        assert!(!xml.contains("secret"));
    }
    #[test]
    fn native_display_and_runtime_reflection_chains() {
        assert_eq!(
            execute(json!({"op":"getQualifiedClassName","value":{"kind":"native","name":"flash.display.MovieClip"}})).unwrap(),
            "flash.display::MovieClip"
        );
        assert_eq!(
            execute(json!({"op":"getQualifiedSuperclassName","value":{"kind":"native","name":"flash.display.MovieClip"}})).unwrap(),
            "flash.display::Sprite"
        );
        assert_eq!(
            execute(json!({"op":"getQualifiedSuperclassName","value":{"kind":"class","name":"flash.display.Sprite"}})).unwrap(),
            "flash.display::DisplayObjectContainer"
        );
        assert_eq!(
            execute(json!({"op":"getQualifiedSuperclassName","value":{"kind":"class","name":"flash.events.IEventDispatcher"}})).unwrap(),
            Value::Null
        );
        assert_eq!(
            execute(json!({"op":"getQualifiedClassName","value":{"kind":"native","name":"Vector.<int>"}})).unwrap(),
            "__AS3__.vec::Vector.<int>"
        );
        let mc_xml = execute(
            json!({"op":"describeType","value":{"kind":"native","name":"flash.display.MovieClip"}}),
        )
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
        assert!(mc_xml.contains("name=\"flash.display::MovieClip\""));
        assert!(mc_xml.contains("base=\"flash.display::Sprite\""));
        assert!(mc_xml.contains("isDynamic=\"true\""));
        assert!(mc_xml.contains("<extendsClass type=\"flash.display::Sprite\"/>"));
        assert!(mc_xml.contains("<extendsClass type=\"flash.display::DisplayObjectContainer\"/>"));
        assert!(mc_xml.contains("<extendsClass type=\"flash.display::InteractiveObject\"/>"));
        assert!(mc_xml.contains("<extendsClass type=\"flash.display::DisplayObject\"/>"));
        assert!(mc_xml.contains("<extendsClass type=\"flash.events::EventDispatcher\"/>"));
        assert!(mc_xml.contains("<extendsClass type=\"Object\"/>"));
        assert!(mc_xml.contains("<implementsInterface type=\"flash.display::IBitmapDrawable\"/>"));
        assert!(mc_xml.contains("<implementsInterface type=\"flash.events::IEventDispatcher\"/>"));
        assert!(mc_xml.contains("<accessor name=\"currentFrame\""));
        assert!(mc_xml.contains("<method name=\"gotoAndStop\""));
    }
}
