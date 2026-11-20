//! Shared compiler language service, contextual token extraction, and Adobe-aligned reference
//! descriptions for Actions and Scripting Reference.
pub(crate) use neoflash_script::language_service::*;

use neoflash_core::ScriptLanguage;
use std::fmt::Write as _;

fn is_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$' | '#')
}

/// Extracts a clean identifier, directive, or dotted `Class.member` lookup token from `source`
/// using the active selection (`primary_char != secondary_char`) or caret position.
pub(crate) fn extract_reference_token(
    source: &str,
    primary_char: usize,
    secondary_char: usize,
) -> Option<String> {
    let chars: Vec<char> = source.chars().collect();
    if chars.is_empty() {
        return None;
    }

    if primary_char != secondary_char {
        let lo = primary_char.min(secondary_char).min(chars.len());
        let hi = primary_char.max(secondary_char).min(chars.len());
        let selected: String = chars[lo..hi].iter().collect();
        let trimmed = selected.trim();
        if !trimmed.is_empty()
            && trimmed.chars().count() <= 48
            && !trimmed.contains('\n')
            && trimmed
                .chars()
                .all(|c| is_token_char(c) || matches!(c, '.' | '<' | '>' | '/'))
        {
            return Some(trimmed.to_owned());
        }
        return None;
    }

    let idx = primary_char.min(chars.len());
    let anchor = if idx < chars.len() && is_token_char(chars[idx]) {
        idx
    } else if idx > 0 && is_token_char(chars[idx - 1]) {
        idx - 1
    } else {
        return None;
    };

    let mut start = anchor;
    while start > 0 && is_token_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = anchor + 1;
    while end < chars.len() && is_token_char(chars[end]) {
        end += 1;
    }
    let word: String = chars[start..end].iter().collect();
    if word.is_empty() || word.chars().all(|c| c.is_ascii_digit()) {
        None
    } else {
        Some(word)
    }
}

/// Returns a concise dialect-aware summary for core language statements, `Function` / `arguments`
/// constructs, and timeline/game primitives when looking up contextual reference help.
pub(crate) fn keyword_reference_summary(
    language: ScriptLanguage,
    token: &str,
) -> Option<&'static str> {
    let clean = token.trim();
    match (language, clean) {
        (ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2, "function") => Some(
            "AS1/AS2 function statement & expression: declares hoisted named functions or anonymous function literals with optional parameter/return type annotations.",
        ),
        (ScriptLanguage::ActionScript3, "function") => Some(
            "AS3 function statement & expression: declares typed functions, class methods (bound closures), default parameter values, and ...rest variadic signatures.",
        ),
        (ScriptLanguage::JavaScript | ScriptLanguage::TypeScript, "function") => Some(
            "JS/TS function declaration & expression: defines hoisted functions, generator/async functions, or typed TypeScript call signatures.",
        ),
        (ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2, "Function") => Some(
            "AS2 Function class: represents callable objects supporting fn.call(thisObj, ...), fn.apply(thisObj, argsArray), and constructor Function.prototype inheritance.",
        ),
        (ScriptLanguage::ActionScript3, "Function") => Some(
            "AS3 Function class: represents closures and bound class methods supporting fn.call(thisArg, ...), fn.apply(thisArg, argArray), and fn.length.",
        ),
        (ScriptLanguage::JavaScript | ScriptLanguage::TypeScript, "Function") => Some(
            "ECMAScript Function constructor & prototype: provides fn.call(thisArg, ...), fn.apply(thisArg, args), and fn.bind(thisArg, ...).",
        ),
        (
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2,
            "arguments" | "callee" | "caller",
        ) => Some(
            "AS1/AS2 arguments object: implicit local array-like call frame exposing arguments.length, arguments[i], arguments.callee (self recursion), and arguments.caller.",
        ),
        (ScriptLanguage::ActionScript3, "arguments" | "callee") => Some(
            "AS3 arguments object: local call parameter list (arguments.length, arguments.callee); prefer typed ...rest parameters in new AS3 code.",
        ),
        (_, "apply" | "Function.apply") => Some(
            "Function.apply(thisObject, argsArray): invokes a function with an explicit `this` receiver and forwards an Array of positional arguments.",
        ),
        (_, "call" | "Function.call") => Some(
            "Function.call(thisObject, arg1, ...): invokes a function with an explicit `this` receiver and comma-separated arguments.",
        ),
        (_, "return") => Some(
            "return [expression]: exits the active function immediately and yields a value to the caller (or undefined / void when omitted).",
        ),
        (
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2,
            "prototype" | "__proto__" | "registerClass",
        ) => Some(
            "AS1/AS2 prototype inheritance: attach shared methods on Constructor.prototype and link Library symbols with Object.registerClass(id, Constructor).",
        ),
        (_, "var") => Some(
            "var variable declaration: declares a function-scoped local variable on the activation object (captured by nested closures) or timeline property at top level.",
        ),
        (_, "trace") => Some(
            "trace(...): streams formatted diagnostic lines to the Output panel (F2) during Test Movie and in-editor Play.",
        ),
        (
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2,
            "onEnterFrame" | "onRelease" | "onPress" | "on" | "onClipEvent",
        ) => Some(
            "AS1/AS2 event handler callbacks: drive 1-frame game loops (this.onEnterFrame) and interactive Button/MovieClip pointer handlers.",
        ),
        (
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2,
            "setInterval" | "clearInterval",
        ) => Some(
            "AS1/AS2 interval timers: setInterval(fn, ms, ...args) schedules repeating callbacks and returns an ID cancelled by clearInterval(id).",
        ),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// ActionScript 2.0 & 3.0 Class and Member Reference Descriptions
// ---------------------------------------------------------------------------------------------

/// Splits an identifier (`gotoAndPlay`, `_xscale`, `ADDED_TO_STAGE`) into readable words.
fn humanize_identifier(raw: &str) -> String {
    let trimmed = raw
        .trim()
        .trim_start_matches('_')
        .trim_start_matches('#')
        .split('(')
        .next()
        .unwrap_or(raw)
        .trim();
    if trimmed.is_empty() {
        return raw.trim().to_owned();
    }
    if trimmed
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    {
        return trimmed
            .split('_')
            .filter(|part| !part.is_empty())
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>()
            .join(" ");
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let mut words = Vec::new();
    let mut current = String::new();
    for i in 0..chars.len() {
        let ch = chars[i];
        if ch == '_' || ch == '.' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        let prev_lower =
            i > 0 && (chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit());
        let acronym_end = i > 0
            && chars[i - 1].is_ascii_uppercase()
            && ch.is_ascii_uppercase()
            && i + 1 < chars.len()
            && chars[i + 1].is_ascii_lowercase();
        if ((ch.is_ascii_uppercase() && prev_lower) || acronym_end) && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
        .into_iter()
        .map(|w| {
            if w.len() >= 2
                && w.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            {
                w
            } else {
                w.to_ascii_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Curated Adobe-aligned class and interface descriptions for ActionScript 2.0 and 3.0.
pub(crate) fn describe_as_class(
    language: ScriptLanguage,
    key: &str,
    name: &str,
    package: &str,
    kind: &str,
    parents: &[String],
    implements: &[String],
) -> String {
    let as3 = language == ScriptLanguage::ActionScript3;
    if let Some(curated) = curated_class_description(as3, key, name) {
        return curated.to_owned();
    }
    synthesize_class_description(as3, key, name, package, kind, parents, implements)
}

fn curated_class_description(as3: bool, key: &str, name: &str) -> Option<&'static str> {
    match (as3, key, name) {
        // ── Display, Stage & Vector Graphics ──
        (false, _, "MovieClip") => Some(
            "Timeline-driven display container in ActionScript 2.0 supporting frame navigation, nested child instances, depth management, hit testing, dynamic vector drawing, and event callbacks.",
        ),
        (true, _, "MovieClip") => Some(
            "Dynamic display list container that extends Sprite with a multi-frame timeline, Scenes, frame labels, and playhead navigation methods.",
        ),
        (true, _, "Sprite") => Some(
            "Lightweight display list building block with a vector Graphics context, child display list, pointer drag support, and buttonMode cursor behavior without timeline frames.",
        ),
        (true, _, "DisplayObject") => Some(
            "Abstract base class for every visual element on the ActionScript 3.0 display list, providing 2D/3D transforms, bounds measurement, masks, blend modes, and live bitmap filters.",
        ),
        (true, _, "DisplayObjectContainer") => Some(
            "Abstract base class for display objects that manage an ordered child list (addChild, removeChild, getChildByName, swapChildren, and point hit queries).",
        ),
        (true, _, "InteractiveObject") => Some(
            "Abstract base class for display objects that receive mouse, keyboard, focus, accessibility, and context-menu interaction on the Stage.",
        ),
        (false, _, "Stage") => Some(
            "Global interface to the Flash Player viewport, controlling Stage alignment, scale mode, fullscreen display state, dimensions, and resize listener broadcasts.",
        ),
        (true, _, "Stage") => Some(
            "Root viewport container owning global keyboard/resize events, frame rate, scaleMode, align, quality, fullscreen displayState, and keyboard focus.",
        ),
        (true, _, "StageScaleMode") => Some(
            "Enumeration of Stage.scaleMode constants (SHOW_ALL, NO_SCALE, EXACT_FIT, NO_BORDER) governing how content scales inside the player viewport.",
        ),
        (true, _, "StageAlign") => Some(
            "Enumeration of Stage.align anchor constants (TOP_LEFT, TOP, BOTTOM, LEFT, RIGHT, etc.) pinning the Stage inside the viewport.",
        ),
        (true, _, "StageDisplayState") => Some(
            "Constants for Stage.displayState switching between NORMAL windowed rendering and FULL_SCREEN / FULL_SCREEN_INTERACTIVE modes.",
        ),
        (true, _, "StageQuality") => Some(
            "Constants for Stage.quality controlling vector anti-aliasing and bitmap smoothing fidelity (LOW, MEDIUM, HIGH, BEST).",
        ),
        (false, _, "Button") => Some(
            "Interactive 4-state (Up, Over, Down, Hit) button symbol instance supporting pointer/keyboard callbacks (onRelease, onPress, onRollOver) and tab focus.",
        ),
        (true, _, "SimpleButton") => Some(
            "Interactive 4-state button display object exposing upState, overState, downState, and hitTestState display subtrees with automatic pointer state switching.",
        ),
        (true, _, "Shape") => Some(
            "Lightweight non-interactive leaf display object wrapping a vector Graphics drawing surface with minimal memory overhead.",
        ),
        (true, _, "MorphShape") => Some(
            "Display object representing an authored shape-tween span interpolated by the timeline engine.",
        ),
        (true, _, "Graphics") => Some(
            "Vector drawing context owned by Shape, Sprite, and MovieClip for procedural strokes, solid/gradient/bitmap fills, primitives, quadratic/cubic curves, and path commands.",
        ),
        (true, _, "Bitmap") => Some(
            "Display object that renders a BitmapData pixel buffer on the display list with configurable pixelSnapping and bilinear smoothing.",
        ),
        (_, _, "BitmapData") => Some(
            "32-bit ARGB / 24-bit RGB pixel buffer supporting DisplayObject rasterization (draw), blitting (copyPixels), flood fill, Perlin noise, color transforms, and bitmap filters.",
        ),
        (true, _, "BitmapDataChannel") => Some(
            "Bitmask channel constants (RED, GREEN, BLUE, ALPHA) for BitmapData.copyChannel, noise, and DisplacementMapFilter.",
        ),
        (true, _, "PixelSnapping") => Some(
            "Constants (ALWAYS, AUTO, NEVER) controlling whether a Bitmap snaps to whole device pixels during rendering.",
        ),
        (true, _, "IBitmapDrawable") => Some(
            "Marker interface implemented by DisplayObject and BitmapData allowing either to serve as the source of BitmapData.draw().",
        ),
        (true, _, "BlendMode") => Some(
            "Compositing mode constants (NORMAL, MULTIPLY, SCREEN, ADD, OVERLAY, DARKEN, LIGHTEN, DIFFERENCE, ERASE, ALPHA, LAYER) for DisplayObject.blendMode and BitmapData.draw().",
        ),
        (true, _, "Scene") => Some(
            "Metadata descriptor for a named Scene on a MovieClip timeline, exposing its name, FrameLabel list, and numFrames count.",
        ),
        (true, _, "FrameLabel") => Some(
            "Read-only descriptor pairing an authored timeline label name with its 1-based frame number.",
        ),
        (true, _, "GradientType") => Some(
            "Constants (LINEAR and RADIAL) selecting the gradient geometry for Graphics.beginGradientFill and lineGradientStyle.",
        ),
        (true, _, "SpreadMethod") => Some(
            "Constants (PAD, REFLECT, REPEAT) controlling how gradients fill space outside their focal start-to-end span.",
        ),
        (true, _, "InterpolationMethod") => Some(
            "Constants (RGB, LINEAR_RGB) selecting gamma-encoded vs linear-light color interpolation in gradients.",
        ),
        (true, _, "LineScaleMode") => Some(
            "Constants (NORMAL, NONE, VERTICAL, HORIZONTAL) controlling how stroke thickness responds to DisplayObject scaling.",
        ),
        (true, _, "CapsStyle") => Some(
            "Constants (ROUND, NONE, SQUARE) specifying end-cap geometry on open vector strokes.",
        ),
        (true, _, "JointStyle") => Some(
            "Constants (ROUND, BEVEL, MITER) specifying corner join geometry where vector stroke segments meet.",
        ),
        // ── Text & Typography ──
        (false, _, "TextField") => Some(
            "Dynamic and input text display object supporting plain/HTML text, TextFormat runs, CSS StyleSheets, scrolling, word wrap, and input focus callbacks.",
        ),
        (true, _, "TextField") => Some(
            "Interactive display object for rendering and editing plain or HTML-formatted text with TextFormat styling, CSS StyleSheets, selection ranges, and scrolling.",
        ),
        (_, _, "TextFormat") => Some(
            "Character and paragraph formatting descriptor specifying font family, point size, color, bold/italic/underline, alignment, margins, leading, and kerning.",
        ),
        (true, _, "TextFormatAlign") => {
            Some("Alignment constants (LEFT, CENTER, RIGHT, JUSTIFY) for TextFormat.align.")
        }
        (true, _, "TextFieldAutoSize") => Some(
            "Constants (NONE, LEFT, CENTER, RIGHT) controlling how a TextField resizes its bounds to fit its text.",
        ),
        (true, _, "TextFieldType") => Some(
            "Constants (DYNAMIC and INPUT) selecting read-only/scripted vs user-editable TextField behavior.",
        ),
        (true, _, "TextLineMetrics") => Some(
            "Measured layout metrics (x, width, height, ascent, descent, leading) for a single rendered line inside a TextField.",
        ),
        (_, _, "StyleSheet") => Some(
            "Cascading Style Sheet parser and selector table for formatting HTML tags inside TextField.htmlText.",
        ),
        (true, _, "Font") => Some(
            "Embedded and device font registry providing Font.enumerateFonts(), Font.registerFont(), and glyph coverage queries.",
        ),
        (true, _, "StaticText") => Some(
            "Read-only display object representing author-time static text placed on the Timeline.",
        ),
        (_, _, "TextSnapshot") => Some(
            "Read-only text inspection and hit-testing helper exposing character runs inside static text on a MovieClip or DisplayObjectContainer.",
        ),
        (true, _, "StageText") => Some(
            "Native OS text input overlay providing platform virtual keyboards, auto-capitalization, and IME editing.",
        ),
        // ── Events ──
        (false, _, "EventDispatcher") => Some(
            "MX component event broadcaster mixin adding addEventListener, removeEventListener, and dispatchEvent to ActionScript 2.0 objects.",
        ),
        (true, _, "EventDispatcher") => Some(
            "Base class for all objects that dispatch W3C-style 3-phase events and manage typed listener registrations via addEventListener and removeEventListener.",
        ),
        (true, _, "IEventDispatcher") => Some(
            "Interface defining the five core event registration and dispatch methods (addEventListener, removeEventListener, dispatchEvent, hasEventListener, willTrigger).",
        ),
        (false, _, "AsBroadcaster") => Some(
            "ActionScript 2.0 observer mixin that equips an object with _listeners, addListener, removeListener, and broadcastMessage.",
        ),
        (true, _, "Event") => Some(
            "Base event object for display-list lifecycle (ENTER_FRAME, ADDED_TO_STAGE, RESIZE), media/network completion, and 3-phase propagation control.",
        ),
        (true, _, "EventPhase") => Some(
            "Constants (CAPTURING_PHASE = 1, AT_TARGET = 2, BUBBLING_PHASE = 3) identifying the current propagation stage of an Event.",
        ),
        (true, _, "MouseEvent") => Some(
            "Pointer interaction event carrying localX/localY, stageX/stageY, modifier key flags, wheel delta, and CLICK / MOUSE_DOWN / MOUSE_MOVE / ROLL_OVER constants.",
        ),
        (true, _, "KeyboardEvent") => Some(
            "Keyboard input event dispatched on KEY_DOWN and KEY_UP with keyCode, charCode, keyLocation, and Ctrl/Alt/Shift modifier state.",
        ),
        (true, _, "FocusEvent") => Some(
            "Focus transition event dispatched when keyboard or pointer focus moves between InteractiveObjects (FOCUS_IN, FOCUS_OUT, KEY_FOCUS_CHANGE, MOUSE_FOCUS_CHANGE).",
        ),
        (true, _, "TimerEvent") => Some(
            "Event dispatched by flash.utils.Timer on each interval tick (TIMER) and upon finishing repeatCount iterations (TIMER_COMPLETE).",
        ),
        (true, _, "ProgressEvent") => Some(
            "Byte-progress event reporting bytesLoaded and bytesTotal during asynchronous asset, sound, or socket transfers.",
        ),
        (true, _, "IOErrorEvent") => Some(
            "Failure event dispatched when a network load, file stream, or socket transfer encounters an input/output error.",
        ),
        (true, _, "SecurityErrorEvent") => Some(
            "Event dispatched when an asynchronous network or cross-domain operation is blocked by sandbox or CORS policy.",
        ),
        (true, _, "HTTPStatusEvent") => Some(
            "Network event reporting the numeric HTTP response status code and headers before completion or error.",
        ),
        (true, _, "FullScreenEvent") => {
            Some("Stage event dispatched when entering or leaving fullscreen display mode.")
        }
        (true, _, "TextEvent") => Some(
            "Event dispatched when a user enters text (TEXT_INPUT) or activates an event: hyperlink inside HTML text (LINK).",
        ),
        (true, _, "NetStatusEvent") => Some(
            "Status and lifecycle notification event carrying an info object (code, level) for NetConnection, NetStream, and SharedObject.",
        ),
        (true, _, "TouchEvent") => Some(
            "Multi-touch input event carrying touchPointID, stage/local coordinates, pressure, and contact size.",
        ),
        // ── Geometry & Color ──
        (_, _, "Point") => Some(
            "2D Cartesian point or vector (x, y) supporting vector addition, subtraction, normalization, polar conversion, distance, and linear interpolation.",
        ),
        (_, _, "Rectangle") => Some(
            "Axis-aligned 2D rectangle defined by (x, y, width, height) with edge accessors (left, right, top, bottom), hit containment, union, and intersection helpers.",
        ),
        (_, _, "Matrix") => Some(
            "2D 3x3 affine transformation matrix (a, b, c, d, tx, ty) for translation, scaling, rotation, skewing, point transformation, and gradient box setup.",
        ),
        (true, _, "Matrix3D") => Some(
            "4x4 homogeneous transformation matrix for 3D display-object positioning, rotation, scaling, projection, and Vector3D math.",
        ),
        (true, _, "Vector3D") => Some(
            "3D Cartesian vector / homogeneous coordinate (x, y, z, w) providing dot product, cross product, angle, distance, and normalization operations.",
        ),
        (true, _, "PerspectiveProjection") => Some(
            "3D camera projection settings (fieldOfView, focalLength, projectionCenter) projecting 3D DisplayObjects onto the 2D Stage.",
        ),
        (_, _, "ColorTransform") => Some(
            "Per-channel RGBA multiplier and offset transform applied to DisplayObject appearance or BitmapData pixel regions.",
        ),
        (_, _, "Transform") => Some(
            "DisplayObject transformation facade coupling 2D/3D affine matrices, ColorTransform, concatenated world transforms, and pixelBounds.",
        ),
        (false, _, "Color") => Some(
            "ActionScript 1.0/2.0 tint and RGBA channel controller targeting a MovieClip via setRGB, getRGB, setTransform, and getTransform.",
        ),
        // ── Filters ──
        (_, _, "BitmapFilter") => Some(
            "Abstract base class for all non-destructive per-display-object and BitmapData visual effect filters.",
        ),
        (true, _, "BitmapFilterQuality") => Some(
            "Pass-count quality constants (LOW = 1, MEDIUM = 2, HIGH = 3) for blur, glow, bevel, and drop-shadow filters.",
        ),
        (true, _, "BitmapFilterType") => Some(
            "Placement constants (INNER, OUTER, FULL) for bevel and gradient glow/bevel filters.",
        ),
        (_, _, "BlurFilter") => Some(
            "Gaussian-like box blur filter parameterized by horizontal blurX, vertical blurY, and pass quality.",
        ),
        (_, _, "GlowFilter") => Some(
            "Inner or outer colored halo filter with configurable color, alpha, blurX/blurY, strength, quality, and knockout.",
        ),
        (_, _, "DropShadowFilter") => Some(
            "Offset directional shadow filter with distance, angle, color, alpha, blurX/blurY, strength, inner shadow, knockout, and hideObject options.",
        ),
        (_, _, "BevelFilter") => Some(
            "Chiseled 3D highlight-and-shadow edge filter with configurable distance, angle, highlight/shadow colors, blur, and strength.",
        ),
        (_, _, "GradientGlowFilter") => Some(
            "Multi-stop color gradient glow filter controlled by arrays of colors, alphas, and 0..255 ratio stops.",
        ),
        (_, _, "GradientBevelFilter") => Some(
            "Multi-stop gradient bevel filter rendering sculpted lit edges from color, alpha, and ratio stop arrays.",
        ),
        (_, _, "ColorMatrixFilter") => Some(
            "4x5 RGBA color matrix filter transforming every pixel's red, green, blue, and alpha channels for saturation, hue, contrast, or sepia effects.",
        ),
        (_, _, "ConvolutionFilter") => Some(
            "2D spatial kernel convolution filter for custom sharpen, emboss, edge-detection, and box-blur effects.",
        ),
        (_, _, "DisplacementMapFilter") => Some(
            "Pixel-warping filter that offsets source pixels using channel values sampled from a secondary mapBitmap.",
        ),
        // ── Audio, Video & Media ──
        (false, _, "Sound") => Some(
            "ActionScript 2.0 audio controller for playing linked Library sounds (attachSound) or external MP3/audio streams (loadSound) with volume, pan, and stereo transforms.",
        ),
        (true, _, "Sound") => Some(
            "Audio asset and streaming loader that decodes embedded or URLRequest audio and spawns a SoundChannel on play().",
        ),
        (true, _, "SoundChannel") => Some(
            "Active playback handle for a playing Sound, exposing playhead position, stereo peak meters, live SoundTransform, stop(), and soundComplete events.",
        ),
        (true, _, "SoundTransform") => Some(
            "Volume, stereo pan (-1.0..1.0), and channel-routing matrix applied to a SoundChannel, Sprite, MovieClip, NetStream, or global SoundMixer.",
        ),
        (true, _, "SoundMixer") => Some(
            "Global audio master bus providing SoundMixer.stopAll(), master soundTransform, bufferTime, and real-time FFT/waveform computeSpectrum().",
        ),
        (true, _, "ID3Info") => Some(
            "Metadata container exposing MP3 ID3 tag fields (songName, artist, album, year, track, genre, comment) on a loaded Sound.",
        ),
        (_, _, "Video") => Some(
            "Display object viewport that renders live Camera capture or progressive/streaming video decoded by an attached NetStream.",
        ),
        (_, _, "Camera") => Some(
            "Video capture device interface providing webcam selection, resolution/fps configuration, motion activity detection, and live Video attachment.",
        ),
        (_, _, "Microphone") => Some(
            "Audio capture device interface providing microphone gain, silence threshold, activity level, and sample data capture.",
        ),
        // ── Networking, Loading & Persistence ──
        (true, _, "Loader") => Some(
            "DisplayObjectContainer that loads external SWF/NeoFlash bundles or image assets (PNG, JPEG, GIF, SVG) into the display list via load() or loadBytes().",
        ),
        (true, _, "LoaderInfo") => Some(
            "Progress, lifecycle event, dimension, frameRate, URL, and flashvars parameters descriptor shared between a Loader and its loaded content.",
        ),
        (true, _, "URLLoader") => Some(
            "Data loader that fetches text, URL-encoded variables, or raw binary ByteArray payloads from a URLRequest.",
        ),
        (true, _, "URLLoaderDataFormat") => Some(
            "Constants (TEXT, VARIABLES, BINARY) specifying how URLLoader decodes downloaded response bytes.",
        ),
        (true, _, "URLStream") => Some(
            "Low-level streaming HTTP client implementing IDataInput so bytes can be read incrementally as packets arrive.",
        ),
        (true, _, "URLRequest") => Some(
            "HTTP request descriptor encapsulating target url, method (GET/POST), request data payload, contentType, and custom URLRequestHeaders.",
        ),
        (true, _, "URLRequestHeader") => {
            Some("Single HTTP header name-value pair attached to URLRequest.requestHeaders.")
        }
        (true, _, "URLRequestMethod") => Some(
            "HTTP verb constants (GET, POST, PUT, DELETE, HEAD, OPTIONS) for URLRequest.method.",
        ),
        (true, _, "URLVariables") => Some(
            "Dynamic dictionary encoding and decoding application/x-www-form-urlencoded query strings for URLRequest and URLLoader.",
        ),
        (false, _, "LoadVars") => Some(
            "ActionScript 2.0 HTTP form-variable loader and sender supporting load(), send(), sendAndLoad(), onLoad, and onData.",
        ),
        (false, _, "MovieClipLoader") => Some(
            "ActionScript 2.0 asset loader with listener progress callbacks (onLoadStart, onLoadProgress, onLoadInit, onLoadComplete, onLoadError) for MovieClips.",
        ),
        (_, _, "NetConnection") => Some(
            "Transport connection manager for progressive HTTP media playback (connect(null)) and RPC/streaming endpoints.",
        ),
        (_, _, "NetStream") => Some(
            "Media streaming controller driving progressive or live video/audio playback, pause/resume, seek, buffering, and metadata callbacks over a NetConnection.",
        ),
        (_, _, "SharedObject") => Some(
            "Persistent local key-value storage (SharedObject.getLocal) saving structured ActionScript state across sessions within origin quota.",
        ),
        (_, _, "LocalConnection") => Some(
            "Inter-movie message channel allowing named ActionScript instances on the same client to invoke methods on one another.",
        ),
        (_, _, "FileReference") => Some(
            "User-initiated file picker and download/save dialog bridge exposing file name, size, type, and byte loading without arbitrary disk access.",
        ),
        (_, _, "FileReferenceList") => Some(
            "Multi-file selection dialog wrapper populating an array of FileReference objects chosen by the user.",
        ),
        (true, _, "Socket") => Some(
            "Asynchronous binary TCP socket implementing IDataInput and IDataOutput for custom framed protocols.",
        ),
        (_, _, "XMLSocket") => Some(
            "Asynchronous zero-byte-delimited UTF-8 text/XML socket client with connect, send, close, and data callbacks.",
        ),
        // ── Core Language, Collections, Math, XML & Binary ──
        (_, _, "Object") => Some(
            "Root dynamic class of all ActionScript objects, providing key-value properties, prototype lookup, hasOwnProperty(), toString(), and valueOf().",
        ),
        (false, _, "Function") => Some(
            "Callable function object supporting explicit receiver invocation (call, apply) and constructor prototype inheritance.",
        ),
        (true, _, "Function") => Some(
            "Callable closure or bound class method object supporting call(thisArg, ...), apply(thisArg, argArray), and parameter length inspection.",
        ),
        (false, _, "arguments") => Some(
            "Implicit local array-like call frame inside every AS1/AS2 function exposing runtime arguments, arguments.length, arguments.callee, and arguments.caller.",
        ),
        (true, _, "Class") => Some(
            "Runtime type object representing an ActionScript 3.0 class constructor, used with new, is, as, and getDefinitionByName().",
        ),
        (_, _, "Array") => Some(
            "Dynamic zero-indexed list supporting push/pop, shift/unshift, slice/splice, sort/sortOn, join, and functional iteration helpers.",
        ),
        (true, _, "Vector") => Some(
            "Dense, homogeneously typed generic sequence (Vector.<T>) with optional fixed length and fast bounds-checked element access.",
        ),
        (_, _, "String") => Some(
            "Immutable UTF-16 character sequence with substring slicing, case conversion, splitting, index lookup, and regular-expression matching/replacement.",
        ),
        (_, _, "Number") => Some(
            "IEEE-754 double-precision 64-bit floating-point numeric type with MIN_VALUE, MAX_VALUE, NaN, Infinity, and base/precision formatting.",
        ),
        (true, _, "int") => Some(
            "32-bit signed two's-complement integer primitive (-2,147,483,648..2,147,483,647) optimized for loop counters, frame indices, and pixel coordinates.",
        ),
        (true, _, "uint") => Some(
            "32-bit unsigned integer primitive (0..4,294,967,295) used for ARGB/RGB hexadecimal colors, bitmasks, and byte lengths.",
        ),
        (_, _, "Boolean") => Some("Logical truth value wrapper representing true or false."),
        (_, _, "Math") => Some(
            "Static IEEE-754 mathematical constants (PI, E, SQRT2) and functions for trigonometry, exponentiation, rounding, clamping, and pseudo-random numbers.",
        ),
        (_, _, "Date") => Some(
            "Calendar date and millisecond-epoch timestamp supporting local and UTC year, month, day, hour, minute, second, and timezone offset accessors.",
        ),
        (true, _, "RegExp") => Some(
            "Regular expression pattern matcher supporting exec(), test(), capture groups, and global/ignoreCase/multiline/dotall/extended flags.",
        ),
        (true, _, "JSON") => Some(
            "Standard JSON parser and serializer converting structured ActionScript values via JSON.parse() and JSON.stringify().",
        ),
        (false, _, "XML") => Some(
            "ActionScript 2.0 DOM-style XML document parser and loader extending XMLNode with parseXML, load, sendAndLoad, and onLoad.",
        ),
        (true, _, "XML") => Some(
            "ECMAScript for XML (E4X) element/node value supporting literal XML syntax, descendant (..), attribute (@), and predicate filtering.",
        ),
        (true, _, "XMLList") => Some(
            "Ordered E4X collection of XML nodes/attributes returned by child, attribute (@), and descendant (..) queries.",
        ),
        (_, _, "XMLNode") => Some(
            "Legacy DOM XML tree node exposing nodeName, nodeValue, attributes, childNodes, firstChild, nextSibling, appendChild, and removeNode.",
        ),
        (true, _, "XMLDocument") => Some(
            "Legacy DOM-style XML document class preserving ActionScript 2.0 XMLNode tree parsing in ActionScript 3.0.",
        ),
        (true, _, "Dictionary") => Some(
            "Hash map supporting object-identity keys (rather than stringified keys) with optional weak key references.",
        ),
        (true, _, "ByteArray") => Some(
            "Random-access binary buffer supporting big/little-endian primitive reads and writes, UTF strings, AMF serialization, and zlib/deflate compression.",
        ),
        (true, _, "Endian") => Some(
            "Byte-order constants (BIG_ENDIAN and LITTLE_ENDIAN) for ByteArray, Socket, URLStream, and IDataInput/IDataOutput.",
        ),
        (true, _, "IDataInput") => Some(
            "Binary stream reader interface implemented by ByteArray, Socket, FileStream, and URLStream.",
        ),
        (true, _, "IDataOutput") => {
            Some("Binary stream writer interface implemented by ByteArray, Socket, and FileStream.")
        }
        (true, _, "Timer") => Some(
            "Repeatable millisecond interval clock dispatching TimerEvent.TIMER and TimerEvent.TIMER_COMPLETE with start(), stop(), and reset().",
        ),
        (_, _, "Error") => Some(
            "Base runtime exception object carrying a human-readable message, error name, numeric errorID, and call stack.",
        ),
        (true, _, "ArgumentError") => Some(
            "Exception thrown when a function or method receives an invalid parameter value or mismatched argument count.",
        ),
        (true, _, "RangeError") => Some(
            "Exception thrown when a numeric argument or index falls outside its documented valid range.",
        ),
        (true, _, "ReferenceError") => Some(
            "Exception thrown when reading or writing an undeclared variable or missing sealed-class property.",
        ),
        (true, _, "SyntaxError") => Some(
            "Exception thrown when parsing malformed JSON, XML, or regular expression source text.",
        ),
        (true, _, "TypeError") => Some(
            "Exception thrown when operating on null/undefined or when an operand cannot be coerced to the expected runtime type.",
        ),
        (true, _, "SecurityError") => Some(
            "Exception thrown synchronously when a cross-origin, local-file, or sandbox boundary blocks an operation.",
        ),
        (true, _, "EOFError") => Some(
            "Exception thrown when reading past the end of available bytes in a ByteArray or IDataInput stream.",
        ),
        // ── Input, UI, System & Transitions ──
        (false, _, "Key") => Some(
            "Global ActionScript 2.0 keyboard polling and listener utility providing Key.isDown(code), Key.getCode(), Key.getAscii(), and key constants.",
        ),
        (true, _, "Keyboard") => Some(
            "Static key-code constants (SPACE, ENTER, ESCAPE, LEFT, RIGHT, UP, DOWN, A..Z, F1..F15) and keyboard capability properties for KeyboardEvent.",
        ),
        (false, _, "Mouse") => Some(
            "Global ActionScript 2.0 pointer utility controlling cursor visibility (Mouse.hide, Mouse.show) and mouse/wheel listener broadcasts.",
        ),
        (true, _, "Mouse") => Some(
            "Static pointer controller managing cursor visibility (Mouse.hide, Mouse.show) and system/custom cursor shapes (Mouse.cursor).",
        ),
        (true, _, "MouseCursor") => Some(
            "Cursor name constants (AUTO, ARROW, BUTTON, HAND, IBEAM) for flash.ui.Mouse.cursor.",
        ),
        (false, _, "Selection") => Some(
            "Global focus and text-caret manager for ActionScript 2.0 TextFields, Buttons, and MovieClips (getFocus, setFocus, setSelection).",
        ),
        (_, _, "ContextMenu") => Some(
            "Custom context-menu configuration managing customItems and built-in menu item visibility on InteractiveObjects.",
        ),
        (_, _, "ContextMenuItem") => Some(
            "Single custom entry in a ContextMenu with caption, separatorBefore, enabled, visible, and selection event handling.",
        ),
        (_, _, "Accessibility") => Some(
            "Assistive-technology status and update helper bridging Flash display accessibility metadata to screen readers.",
        ),
        (true, _, "AccessibilityProperties") => Some(
            "Per-display-object accessibility metadata (name, description, shortcut, silent, forceSimple, noAutoLabeling) exposed to assistive tools.",
        ),
        (_, _, "System") => Some(
            "Runtime environment facade exposing clipboard text (setClipboard), memory metrics, IME access, and player settings.",
        ),
        (_, _, "Capabilities" | "capabilities") => Some(
            "Read-only runtime environment and device capabilities descriptor reporting screen resolution, OS, player version, language, and audio/video support.",
        ),
        (_, _, "Security" | "security") => Some(
            "Cross-domain policy and sandbox descriptor exposing sandboxType, allowDomain, and policy-file loading.",
        ),
        (true, _, "ApplicationDomain") => Some(
            "Partition of ActionScript 3.0 class definitions enabling isolated or shared symbol lookup via getDefinition() and hasDefinition().",
        ),
        (true, _, "LoaderContext") => Some(
            "Options object configuring policy-file checks, target ApplicationDomain, and SecurityDomain for Loader.load().",
        ),
        (_, _, "ExternalInterface") => Some(
            "Bidirectional JavaScript host bridge registering callable callbacks (addCallback) and invoking host page functions (call) when available.",
        ),
        (_, _, "PrintJob") => Some(
            "Multi-page vector and bitmap print session manager with start(), addPage(), send(), and paper/page dimension properties.",
        ),
        (_, _, "Tween") => Some(
            "Programmatic property interpolator animating a target object's numeric property over frames or seconds using an easing function.",
        ),
        (_, _, "TransitionManager") => Some(
            "Component transition controller applying animated visual effects (Blinds, Fade, Fly, Iris, Photo, Wipe, Zoom) to a MovieClip.",
        ),
        (false, _, "Delegate") => Some(
            "ActionScript 2.0 callback helper (mx.utils.Delegate.create) binding a method closure to an explicit `this` scope.",
        ),
        _ => None,
    }
}

fn synthesize_class_description(
    as3: bool,
    key: &str,
    name: &str,
    package: &str,
    kind: &str,
    parents: &[String],
    implements: &[String],
) -> String {
    let dialect = if as3 {
        "ActionScript 3.0"
    } else {
        "ActionScript 2.0"
    };
    let pkg_note = if package.is_empty() || package == "Top level" || package == "Top Level" {
        format!("top-level {dialect}")
    } else {
        package.to_owned()
    };
    let mut out = if kind == "interface" {
        format!(
            "Interface `{name}` in `{pkg_note}` defining the {} contract",
            humanize_identifier(name.strip_prefix('I').unwrap_or(name))
        )
    } else if name.ends_with("Event") && name != "Event" {
        let topic = humanize_identifier(name.trim_end_matches("Event"));
        format!("Event class in `{pkg_note}` dispatched for {topic} notifications")
    } else if name.ends_with("Error") && name != "Error" {
        let topic = humanize_identifier(name.trim_end_matches("Error"));
        format!("Runtime error class in `{pkg_note}` signaling {topic} failures")
    } else if name.ends_with("Filter") {
        let topic = humanize_identifier(name.trim_end_matches("Filter"));
        format!(
            "Visual effect filter in `{pkg_note}` applying {topic} processing to display objects or BitmapData"
        )
    } else if package.ends_with(".easing") {
        let curve = humanize_identifier(name);
        format!(
            "Easing curve helper in `{pkg_note}` providing {curve} easeIn, easeOut, and easeInOut interpolation functions"
        )
    } else if package.ends_with("transitions") {
        let effect = humanize_identifier(name);
        format!(
            "Animated visual transition class in `{pkg_note}` performing a {effect} reveal or hide effect"
        )
    } else if matches!(
        name,
        n if n.ends_with("Mode")
            || n.ends_with("Style")
            || n.ends_with("Type")
            || n.ends_with("Status")
            || n.ends_with("Policy")
            || n.ends_with("Format")
            || n.ends_with("Method")
            || n.ends_with("Quality")
            || n.ends_with("Action")
            || n.ends_with("State")
            || n.ends_with("Orientation")
            || n.ends_with("Placement")
    ) {
        format!(
            "Class `{name}` in `{pkg_note}` providing typed {} constants and settings",
            humanize_identifier(name)
        )
    } else {
        let domain = match package {
            p if p.starts_with("flash.display3D") => "Stage3D GPU rendering",
            p if p.starts_with("flash.display") => "display list and rendering",
            p if p.starts_with("flash.text.engine") => "Flash Text Engine (FTE) typographic layout",
            p if p.starts_with("flash.text") => "text rendering and input",
            p if p.starts_with("flash.events") => "event dispatch and listener notification",
            p if p.starts_with("flash.geom") => "2D/3D geometry and color transformation",
            p if p.starts_with("flash.media") => "audio, video, and device capture",
            p if p.starts_with("flash.net.drm") => "digital rights management",
            p if p.starts_with("flash.net") => "networking, streaming, and data transfer",
            p if p.starts_with("flash.filesystem") => "file-system stream and path",
            p if p.starts_with("flash.data") => "local SQL database",
            p if p.starts_with("flash.desktop") => "desktop shell, clipboard, and drag-and-drop",
            p if p.starts_with("flash.sensors") => "device motion and location sensor",
            p if p.starts_with("flash.globalization") => "locale-aware formatting and collation",
            p if p.starts_with("flash.security") => "certificate and signature verification",
            p if p.starts_with("flash.system") => "runtime environment, domain, and security",
            p if p.starts_with("flash.ui") => "keyboard, mouse, touch, and menu input",
            p if p.starts_with("flash.utils") => "binary data, timer, and reflection utility",
            p if p.starts_with("flash.xml") => "legacy XML DOM node",
            p if p.starts_with("flash.printing") => "page printing",
            p if p.starts_with("flash.sampler") => "memory and stack sampling",
            p if p.starts_with("fl.motion") => "motion-XML keyframe and matrix",
            p if p.starts_with("mx.data") => "MX data-binding and dataset",
            p if p.starts_with("mx.services") => "MX SOAP/WebService RPC",
            p if p.starts_with("mx.video") => "MX FLV video playback",
            p if p.starts_with("mx.") => "MX component framework",
            _ => "runtime API",
        };
        format!(
            "Class `{name}` in `{pkg_note}` providing {domain} functionality for {}",
            humanize_identifier(name)
        )
    };
    if let Some(parent) = parents
        .iter()
        .map(|p| short_name(p))
        .find(|p| !p.is_empty() && *p != "Object")
    {
        let _ = write!(out, " (extends {parent})");
    }
    if !implements.is_empty() {
        let list: Vec<&str> = implements.iter().map(|i| short_name(i)).take(3).collect();
        let _ = write!(out, "; implements {}", list.join(", "));
    }
    let _ = key;
    out.push('.');
    out
}

/// Curated + signature-aware reference description for an ActionScript 2.0 or 3.0 member.
pub(crate) fn describe_as_member(
    language: ScriptLanguage,
    class_key: &str,
    class_name: &str,
    pseudo: bool,
    member: &Member,
) -> String {
    let as3 = language == ScriptLanguage::ActionScript3;
    let kind = member.kind.as_str();

    // 1. Language elements (operators, statements, directives, special types).
    if matches!(
        kind,
        "operator" | "statement" | "directive" | "keyword" | "special_type" | "compiler_directive"
    ) {
        return describe_language_element(as3, member);
    }

    // 2. Top-level & package-level functions, constants, properties, and handlers.
    if pseudo
        && let Some(global_desc) = curated_global_description(as3, class_key, &member.name, kind)
    {
        return global_desc.to_owned();
    }

    // 3. Curated core Flash Player class members.
    if !pseudo
        && let Some(curated) = curated_member_description(as3, class_name, &member.name, kind)
    {
        return curated.to_owned();
    }

    // 4. Deterministic signature/kind/owner/value/read-only synthesis.
    synthesize_member_description(as3, class_key, class_name, pseudo, member)
}

fn describe_language_element(as3: bool, member: &Member) -> String {
    let raw = member.name.trim();
    let symbol = raw.split(" (").next().unwrap_or(raw).trim();
    let paren_note = raw
        .split_once(" (")
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .map(str::trim)
        .or(member.operator.as_deref());

    match (member.kind.as_str(), symbol) {
        ("directive" | "compiler_directive", "#include") => {
            "Compile-time directive that splices external ActionScript source text into the current script.".into()
        }
        ("directive" | "compiler_directive", "#initclip") => {
            "ActionScript 1.0/2.0 component initialization block executed once before symbol instances are constructed.".into()
        }
        ("directive" | "compiler_directive", "#endinitclip") => {
            "Marks the end of an `#initclip` component registration block.".into()
        }
        ("special_type", "*") => {
            "Untyped ActionScript 3.0 type annotation allowing a variable, parameter, or return slot to hold any runtime value including `undefined`.".into()
        }
        ("special_type", "void") => {
            "ActionScript 3.0 return-only type annotation indicating that a function returns no value (`undefined`).".into()
        }
        ("special_type", "Null") => {
            "Special type of the `null` literal, assignable to any non-primitive object reference.".into()
        }
        (_, "... (rest) parameter" | "...") => {
            "Variadic parameter declaration (`...rest`) collecting all remaining call arguments into a dense `Array`.".into()
        }
        (_, "AS3") => {
            "Namespace qualifier selecting ActionScript 3.0 class methods on core ECMAScript classes (such as `Array` and `String`) over prototype methods.".into()
        }
        (_, "if") => "Conditional branch executing its statement block when the boolean condition evaluates to `true`.".into(),
        (_, "else") => "Fallback branch executed when the preceding `if` or `else if` condition evaluates to `false`.".into(),
        (_, "else if") => "Chained conditional branch tested only when earlier `if` conditions evaluate to `false`.".into(),
        (_, "switch") => "Multi-way branch comparing an expression against `case` labels using strict equality (`===`).".into(),
        (_, "case") => "Branch label inside a `switch` block matched against the switch expression.".into(),
        (_, "default") => "Fallback branch inside a `switch` block (or `default xml namespace` E4X directive).".into(),
        (_, "default xml namespace") => "Sets the default E4X `Namespace` applied to unqualified XML element constructions in the current scope.".into(),
        (_, "for") => "Counted loop statement evaluating an initializer, loop condition, and per-iteration step expression.".into(),
        (_, "for..in") => "Enumerates the dynamic enumerable property names (keys) of an object or array.".into(),
        (_, "for each..in") => "Iterates directly over the property values of an object, `Array`, `Dictionary`, `XML`, or `XMLList`.".into(),
        (_, "while") => "Pre-condition loop that repeats its body while the test expression remains truthy.".into(),
        (_, "do..while") => "Post-condition loop that executes its body once before testing the continuation condition.".into(),
        (_, "break") => "Exits the innermost enclosing loop (`for`, `while`, `do..while`) or `switch` statement immediately.".into(),
        (_, "continue") => "Skips the rest of the current loop iteration and advances to the next iteration test.".into(),
        (_, "return") => "Exits the current function immediately and returns the given expression (or `undefined` when omitted) to the caller.".into(),
        (_, "throw") => "Raises an `Error` or arbitrary value as an exception, unwinding the call stack to the nearest `catch` block.".into(),
        (_, "try..catch..finally") => "Structured exception-handling block catching thrown errors and running cleanup logic in `finally`.".into(),
        (_, "var") => "Declares a function-scoped local variable, class field, or timeline property with an optional `:Type` annotation.".into(),
        (_, "const") => "Declares a read-only constant binding that cannot be reassigned after initialization.".into(),
        (_, "function") => {
            if as3 {
                "Declares a typed function, bound class method, getter/setter accessor, or anonymous closure expression.".into()
            } else {
                "Declares a hoisted named function or anonymous function literal with optional AS2 type annotations.".into()
            }
        }
        (_, "class") => "Declares an object-oriented class definition with constructors, fields, methods, and optional `extends` / `implements` clauses.".into(),
        (_, "interface") => "Declares a type contract of method signatures (and getters/setters in AS3) without implementation bodies.".into(),
        (_, "extends") => "Specifies the superclass of a `class` or parent interfaces of an `interface`.".into(),
        (_, "implements") => "Declares one or more interfaces whose method contracts a `class` fulfills.".into(),
        (_, "package") => "Encloses ActionScript 3.0 class, interface, function, or namespace declarations inside a dotted package path.".into(),
        (_, "import") => "Brings a qualified class or package wildcard (`flash.display.*`) into lexical scope.".into(),
        (_, "include") => "Splices external ActionScript source text at the directive position during compilation.".into(),
        (_, "public") => "Access modifier making a class, method, property, or constant visible from any package.".into(),
        (_, "private") => "Access modifier restricting a class member to code inside the declaring class.".into(),
        (_, "protected") => "Access modifier making a class member visible to the declaring class and its subclasses.".into(),
        (_, "internal") => "Access modifier making a declaration visible to all code within the same package.".into(),
        (_, "static") => "Attaches a property, constant, or method to the class constructor itself rather than class instances.".into(),
        (_, "final") => "Prevents a class from being subclassed or prevents a method from being overridden in subclasses.".into(),
        (_, "dynamic") => "Allows arbitrary properties to be added, read, and deleted on instances of a class at runtime.".into(),
        (_, "override") => "Explicitly marks a subclass method or accessor as overriding an inherited superclass definition.".into(),
        (_, "intrinsic") => "ActionScript 2.0 compile-time declaration describing external host/player class signatures without emitting bytecode.".into(),
        (_, "native") => "Marks a declaration implemented natively by the Flash runtime.".into(),
        (_, "namespace") => "Declares a custom URI-backed ActionScript 3.0 member visibility or E4X XML namespace.".into(),
        (_, "use namespace") => "Opens a custom ActionScript 3.0 namespace inside the current block so its members can be referenced unqualified.".into(),
        (_, "flash_proxy") => "Namespace used with `flash.utils.Proxy` to intercept dynamic property reads, writes, calls, and enumeration.".into(),
        (_, "get") => "Defines a computed getter property accessor invoked when reading `obj.prop`.".into(),
        (_, "set") => "Defines a computed setter property accessor invoked when assigning `obj.prop = value`.".into(),
        (_, "set variable") => "Legacy Flash 4 dynamic variable assignment statement (`set(name, value)`).".into(),
        (_, "super") => "Invokes the superclass constructor (`super(...)`) or accesses an overridden superclass method (`super.method(...)`).".into(),
        (_, "this") => "References the current class instance, active timeline `MovieClip`, or explicit `Function.call` / `apply` receiver.".into(),
        (_, "with") => "Temporarily prepends a target object to the lexical scope chain for unqualified property lookups inside its block.".into(),
        (_, "label") => "Attaches an identifier (`labelName:`) to a loop or block so nested `break labelName` or `continue labelName` can target it.".into(),
        (_, "true") => "Boolean literal representing logical truth.".into(),
        (_, "false") => "Boolean literal representing logical falsehood.".into(),
        (_, "null") => "Literal representing the intentional absence of an object reference.".into(),
        ("operator", "is") => "ActionScript 3.0 runtime type-check operator returning `true` when the left operand implements or inherits from the right class/interface.".into(),
        ("operator", "as") => "ActionScript 3.0 safe type cast returning the left operand when compatible with the right type, or `null` instead of throwing `TypeError`.".into(),
        ("operator", "in") => "Property-existence operator returning `true` if the named property or index exists on the target object or its prototype chain.".into(),
        ("operator", "instanceof") => "Prototype-chain type check returning `true` if the right constructor's `prototype` appears in the left object's prototype chain.".into(),
        ("operator", "typeof") => "Unary type-inspection operator returning `\"number\"`, `\"string\"`, `\"boolean\"`, `\"object\"`, `\"movieclip\"`, `\"function\"`, `\"xml\"`, or `\"undefined\"`.".into(),
        ("operator", "new") => "Instantiates a class or constructor function and invokes its constructor on the newly allocated instance.".into(),
        ("operator", "delete") if raw.contains("XML") => "E4X node/attribute removal operator deleting matching child elements or `@attr` attributes from an `XML` / `XMLList`.".into(),
        ("operator", "delete") => "Removes a dynamic property from an object and returns `true` when deletion succeeds.".into(),
        ("operator", "void") => "Unary operator that evaluates its operand for side effects and always yields `undefined`.".into(),
        ("operator", "..") => "E4X descendant accessor returning an `XMLList` of all matching elements at any nesting depth.".into(),
        ("operator", "@") => "E4X attribute identifier reading or assigning XML element attributes (`node.@id`).".into(),
        ("operator", "::") => "Namespace name qualifier resolving a member or XML tag inside an explicit `Namespace` (`ns::member`).".into(),
        ("operator", "?:") => "Ternary conditional expression (`condition ? whenTrue : whenFalse`) evaluating and returning one of two branches.".into(),
        ("operator", "===") => "Strict equality operator comparing two values without implicit type coercion.".into(),
        ("operator", "!==") => "Strict inequality operator returning `true` when operands differ in type or value.".into(),
        ("operator", "==") => "Equality operator comparing two operands with ECMAScript/ActionScript type coercion.".into(),
        ("operator", "!=" | "<>") => "Inequality operator returning `true` when two operands are not equal after coercion.".into(),
        ("operator", "eq" | "ne" | "lt" | "gt" | "le" | "ge") => {
            "Legacy Flash 4 string comparison operator preserved for ActionScript 1.0/2.0 compatibility.".into()
        }
        ("operator", "add") => "Legacy Flash 4 string concatenation operator (`a add b`); prefer `+` in modern ActionScript.".into(),
        ("operator", "and" | "or" | "not") => {
            "Legacy Flash 4 logical operator preserved for ActionScript 1.0/2.0 compatibility; prefer `&&`, `||`, and `!`.".into()
        }
        ("operator", _) => match paren_note {
            Some(note) if !note.is_empty() => {
                format!(
                    "ActionScript `{symbol}` {note} operator used in expressions."
                )
            }
            _ => format!("ActionScript `{raw}` language operator."),
        },
        (_, _) => match paren_note {
            Some(note) if !note.is_empty() => {
                format!("ActionScript `{symbol}` ({note}) language construct.")
            }
            _ => format!("ActionScript `{raw}` language statement or keyword."),
        },
    }
}

fn curated_global_description(
    as3: bool,
    class_key: &str,
    name: &str,
    kind: &str,
) -> Option<&'static str> {
    let _ = class_key;
    match (as3, kind, name) {
        (_, _, "trace") => Some(
            "Formats and prints one or more values as a line in the NeoFlash Output panel (F2) and browser console during Test Movie.",
        ),
        (_, _, "play") => {
            Some("Resumes sequential frame playback on the current MovieClip timeline.")
        }
        (_, _, "stop") => Some(
            "Halts playhead advancement on the current MovieClip timeline while child MovieClips keep running on their own clocks.",
        ),
        (_, _, "gotoAndPlay") => Some(
            "Jumps the current timeline playhead to a 1-based frame number or frame label (and optional Scene) and continues playing.",
        ),
        (_, _, "gotoAndStop") => Some(
            "Moves the current timeline playhead to a 1-based frame number or frame label (and optional Scene) and stops immediately.",
        ),
        (_, _, "nextFrame") => {
            Some("Advances the current timeline playhead by one frame and stops playback.")
        }
        (_, _, "prevFrame") => {
            Some("Moves the current timeline playhead back by one frame and stops playback.")
        }
        (_, _, "nextScene") => {
            Some("Advances playback to Frame 1 of the next authored Scene in document order.")
        }
        (_, _, "prevScene") => {
            Some("Jumps to Frame 1 of the previous authored Scene in document order.")
        }
        (_, _, "stopAllSounds") => Some(
            "Immediately silences all currently playing Event, Start, and Stream sounds across the player.",
        ),
        (_, _, "getTimer") => Some(
            "Returns the elapsed integer milliseconds since the runtime initialized, ideal for frame-rate-independent delta timing.",
        ),
        (_, _, "setInterval") => Some(
            "Schedules a function (or object method) to run repeatedly every `delay` milliseconds and returns a numeric interval ID.",
        ),
        (_, _, "clearInterval") => {
            Some("Cancels a repeating timer previously scheduled with `setInterval(id)`.")
        }
        (_, _, "setTimeout") => Some(
            "Schedules a one-shot callback after `delay` milliseconds and returns a timeout ID.",
        ),
        (_, _, "clearTimeout") => {
            Some("Cancels a pending one-shot callback scheduled with `setTimeout(id)`.")
        }
        (false, _, "duplicateMovieClip") => Some(
            "Clones a stage MovieClip instance into a new instance name at the specified depth.",
        ),
        (false, _, "removeMovieClip") => Some(
            "Removes a dynamically created MovieClip instance (positive depth 0..1048575) from its parent timeline.",
        ),
        (false, _, "startDrag") => Some(
            "Makes the target MovieClip follow the pointer, optionally locking its registration center and constraining it to a bounding box.",
        ),
        (false, _, "stopDrag") => {
            Some("Ends any active interactive MovieClip drag started with `startDrag()`.")
        }
        (false, _, "loadMovie") => Some(
            "Loads an external SWF, published NeoFlash bundle, or image URL into a target MovieClip.",
        ),
        (false, _, "loadMovieNum") => {
            Some("Loads an external SWF, bundle, or image URL into `_levelN` of the player.")
        }
        (false, _, "unloadMovie") => {
            Some("Unloads external media previously loaded into a target MovieClip.")
        }
        (false, _, "unloadMovieNum") => Some("Unloads content previously loaded into `_levelN`."),
        (false, _, "loadVariables") => Some(
            "Fetches URL-encoded `name=value` pairs from a URL and assigns them as variables on the target MovieClip.",
        ),
        (false, _, "loadVariablesNum") => {
            Some("Fetches URL-encoded variables from a URL into `_levelN`.")
        }
        (false, _, "getURL") => Some(
            "Navigates a browser window/frame (`_blank`, `_self`, `_parent`, `_top`) to a URL, optionally forwarding GET or POST variables.",
        ),
        (true, _, "navigateToURL") => Some(
            "Opens a `URLRequest` in the specified browser window or tab target (`\"_blank\"`, `\"_self\"`).",
        ),
        (true, _, "sendToURL") => Some(
            "Dispatches an HTTP `URLRequest` (such as analytics or beacon data) while discarding any response body.",
        ),
        (false, _, "fscommand") => {
            Some("Sends a string command and argument pair to the host player or embedding page.")
        }
        (true, _, "fscommand") => {
            Some("Sends an external command and argument string to the host container.")
        }
        (false, _, "tellTarget") => Some(
            "Legacy Flash 4 block redirecting timeline commands (`play`, `stop`, `gotoAndPlay`) to another MovieClip target path.",
        ),
        (false, _, "ifFrameLoaded") => Some(
            "Legacy streaming check that runs its body when the specified frame has finished loading.",
        ),
        (false, _, "call") => Some(
            "Legacy Flash 4 action that executes the frame script attached to another frame without moving the playhead.",
        ),
        (false, _, "getProperty") => Some(
            "Reads a classic MovieClip display property (`_x`, `_y`, `_alpha`, `_currentframe`, etc.) by target path and property token.",
        ),
        (false, _, "setProperty") => Some(
            "Assigns a classic MovieClip display property (`_x`, `_y`, `_xscale`, `_alpha`, `_visible`, etc.) by target path.",
        ),
        (false, _, "targetPath") => Some(
            "Returns the absolute dot-syntax path string (such as `\"_level0.hero_mc\"`) of a MovieClip or Button instance.",
        ),
        (false, _, "eval") => Some(
            "Resolves a variable, property, or MovieClip reference by string name or target path in ActionScript 1.0/2.0 (does not parse arbitrary code).",
        ),
        (false, _, "updateAfterEvent") => Some(
            "Requests an immediate Stage redraw after a mouse, key, or `setInterval` handler without waiting for the next timeline frame.",
        ),
        (false, _, "getVersion") => Some(
            "Returns the runtime platform and Flash Player version string (for example `\"MAC 9,0,0,0\"`).",
        ),
        (false, _, "random") => Some(
            "Legacy Flash 4 helper returning a pseudo-random integer in `0..(value - 1)`; prefer `Math.random()` in new code.",
        ),
        (false, _, "asfunction") => Some(
            "Protocol helper used inside HTML `<a href=\"asfunction:fn,arg\">` links to invoke an ActionScript function on click.",
        ),
        (_, _, "MMExecute") => Some(
            "Authoring-extension bridge executing a JSFL command when running inside a Flash authoring panel.",
        ),
        (false, _, "chr" | "mbchr") => Some(
            "Legacy Flash 4 function converting a character code point into a 1-character string; prefer `String.fromCharCode()`.",
        ),
        (false, _, "ord" | "mbord") => Some(
            "Legacy Flash 4 function returning the numeric code point of a string's first character; prefer `String.charCodeAt(0)`.",
        ),
        (false, _, "length" | "mblength") => Some(
            "Legacy Flash 4 string length function returning the character count of a string; prefer `str.length`.",
        ),
        (false, _, "substring" | "mbsubstring") => Some(
            "Legacy Flash 4 1-based substring extraction function; prefer `String.substr()` or `String.substring()`.",
        ),
        (_, _, "parseInt") => Some(
            "Parses a leading integer from a string using an optional radix (2..36), returning `NaN` when no digits match.",
        ),
        (_, _, "parseFloat") => {
            Some("Parses a leading IEEE-754 floating-point number from a string, or returns `NaN`.")
        }
        (_, _, "isNaN") => {
            Some("Returns `true` when the value converts to IEEE-754 `NaN` (Not-a-Number).")
        }
        (_, _, "isFinite") => Some(
            "Returns `true` when the argument is a finite number (neither `NaN`, `Infinity`, nor `-Infinity`).",
        ),
        (_, _, "int") => {
            Some("Truncates a numeric or string value toward zero into a 32-bit signed integer.")
        }
        (true, _, "uint") => {
            Some("Converts a value into an unsigned 32-bit integer (`0..4294967295`) modulo 2^32.")
        }
        (_, _, "Number") => {
            Some("Converts an arbitrary value to an IEEE-754 floating-point `Number`.")
        }
        (_, _, "String") => Some("Converts any value into its string representation."),
        (_, _, "Boolean") => {
            Some("Converts an expression into its boolean truth value (`true` or `false`).")
        }
        (_, _, "Object") => Some(
            "Wraps a primitive or returns an object reference unchanged (or constructs a fresh empty object when given `null`/`undefined`).",
        ),
        (_, _, "Array") => {
            Some("Constructs a new `Array` from an element count or list of elements.")
        }
        (true, _, "Vector") => {
            Some("Converts an `Array` or sequence into a typed `Vector.<T>` instance.")
        }
        (true, _, "XML") => Some("Parses a string or converts an E4X value into an `XML` node."),
        (true, _, "XMLList") => {
            Some("Parses or converts an expression into an E4X `XMLList` node collection.")
        }
        (true, _, "isXMLName") => Some(
            "Returns `true` if `str` is a valid XML element or attribute identifier under E4X naming rules.",
        ),
        (_, _, "escape") => Some(
            "URL-encodes non-ASCII and reserved punctuation characters using legacy `%XX` / `%uXXXX` hexadecimal escapes.",
        ),
        (_, _, "unescape") => {
            Some("Decodes `%XX` and `%uXXXX` escape sequences produced by `escape()`.")
        }
        (true, _, "encodeURI") => Some(
            "Encodes a complete URI in UTF-8 percent-encoding while preserving URI structure characters (`:/?#[]@!$&'()*+,;=`).",
        ),
        (true, _, "decodeURI") => Some(
            "Decodes UTF-8 percent-encoded sequences inside a full URI encoded with `encodeURI()`.",
        ),
        (true, _, "encodeURIComponent") => Some(
            "UTF-8 percent-encodes an individual URI query parameter or path segment, escaping `&`, `=`, `/`, and `?`.",
        ),
        (true, _, "decodeURIComponent") => {
            Some("Decodes a UTF-8 percent-encoded URI component string.")
        }
        (true, _, "getQualifiedClassName") => Some(
            "Returns the fully qualified ActionScript 3.0 class name (such as `\"flash.display::MovieClip\"`) of any value or class.",
        ),
        (true, _, "getQualifiedSuperclassName") => Some(
            "Returns the fully qualified superclass name of a value or class, or `null` when none exists.",
        ),
        (true, _, "getDefinitionByName") => Some(
            "Looks up and returns the public class or function `Object` registered under a qualified name in the current `ApplicationDomain`.",
        ),
        (true, _, "describeType") => Some(
            "Generates an E4X `XML` reflection record describing a class or instance's inheritance chain, interfaces, fields, accessors, and methods.",
        ),
        (true, _, "registerClassAlias") => Some(
            "Registers a serialized AMF class alias mapping a string alias name to an ActionScript class constructor.",
        ),
        (true, _, "getClassByAlias") => Some(
            "Looks up the ActionScript class previously registered for an AMF alias with `registerClassAlias()`.",
        ),
        (true, _, "escapeMultiByte") => {
            Some("Percent-encodes a string using the runtime's system code page or UTF-8.")
        }
        (true, _, "unescapeMultiByte") => {
            Some("Decodes a percent-encoded string using the runtime's system code page or UTF-8.")
        }
        // Global properties & constants
        (false, _, "_root") => Some(
            "References the main root MovieClip timeline of the current level (`_level0` by default).",
        ),
        (false, _, "_parent") => Some(
            "References the MovieClip timeline that immediately contains the current MovieClip or Button.",
        ),
        (false, _, "_global") => Some(
            "Global object shared across all timelines and levels in ActionScript 2.0 for cross-timeline state and classes.",
        ),
        (false, _, "_level") => Some(
            "Addresses a numbered root player level (`_level0`, `_level1`, ...) loaded via `loadMovieNum`.",
        ),
        (false, _, "_quality") => Some(
            "Global Stage rendering anti-aliasing quality string (`\"LOW\"`, `\"MEDIUM\"`, `\"HIGH\"`, `\"BEST\"`).",
        ),
        (false, _, "_highquality") => Some(
            "Legacy Flash 4 anti-aliasing level (`0` = low, `1` = high, `2` = best); prefer `_quality` or `Stage.quality`.",
        ),
        (false, _, "_focusrect") => Some(
            "Controls whether InteractiveObjects display a yellow keyboard-focus highlight rectangle by default.",
        ),
        (false, _, "_soundbuftime") => Some(
            "Number of seconds of streaming audio pre-buffered before playback begins (default `5`).",
        ),
        (false, _, "_accProps") => Some(
            "Global accessibility metadata object exposing document-level screen-reader name and description.",
        ),
        (false, _, "scroll") => {
            Some("Legacy 1-based top visible line property of a variable-bound TextField.")
        }
        (false, _, "maxscroll") => {
            Some("Read-only maximum 1-based `scroll` line index of a variable-bound TextField.")
        }
        (_, "constant", "NaN") => Some(
            "IEEE-754 Not-a-Number value produced by invalid numeric operations; test with `isNaN(x)`.",
        ),
        (_, "constant", "Infinity") => Some(
            "IEEE-754 positive infinity constant (`Number.POSITIVE_INFINITY`), greater than `Number.MAX_VALUE`.",
        ),
        (_, "constant", "-Infinity") => {
            Some("IEEE-754 negative infinity constant (`Number.NEGATIVE_INFINITY`).")
        }
        (_, "constant", "undefined") => Some(
            "Default value of uninitialized untyped variables, missing dynamic properties, and `Void` returns.",
        ),
        (_, "constant", "null") => {
            Some("Null reference value indicating no object instance is assigned.")
        }
        (_, "constant", "true") => Some("Boolean `true` constant."),
        (_, "constant", "false") => Some("Boolean `false` constant."),
        (false, "constant", "newline") => Some(
            "Line-feed character constant (`\"\\n\"`) inserting a line break in strings and `trace()` output.",
        ),
        (false, "event", "on") => Some(
            "Button instance handler (`on(release)`, `on(press)`, `on(rollOver)`, `on(keyPress \"...\")`) attached directly to a stage Button.",
        ),
        (false, "event", "onClipEvent") => Some(
            "MovieClip instance handler (`onClipEvent(load)`, `onClipEvent(enterFrame)`, `onClipEvent(mouseDown)`, `onClipEvent(keyDown)`) attached to a stage clip.",
        ),
        _ => None,
    }
}

fn curated_member_description(
    as3: bool,
    owner: &str,
    name: &str,
    kind: &str,
) -> Option<&'static str> {
    if kind == "constructor" {
        return None;
    }
    // Owner-specific disambiguations first.
    match (owner, name) {
        ("MovieClip", "gotoAndPlay") => {
            return Some(
                "Moves the MovieClip playhead to the specified 1-based frame number or frame label (optionally in a named Scene) and resumes playback.",
            );
        }
        ("MovieClip", "gotoAndStop") => {
            return Some(
                "Moves the MovieClip playhead to the specified 1-based frame number or frame label (optionally in a named Scene) and stops immediately.",
            );
        }
        ("MovieClip", "play") => {
            return Some(
                "Starts or resumes sequential frame advancement on this MovieClip timeline.",
            );
        }
        ("MovieClip", "stop") => {
            return Some(
                "Stops playhead advancement on this MovieClip timeline at its current frame.",
            );
        }
        ("MovieClip", "nextFrame") => {
            return Some("Advances this MovieClip's playhead by one frame and stops.");
        }
        ("MovieClip", "prevFrame") => {
            return Some("Rewinds this MovieClip's playhead by one frame and stops.");
        }
        ("MovieClip", "nextScene") => {
            return Some("Advances the root MovieClip playhead to Frame 1 of the next Scene.");
        }
        ("MovieClip", "prevScene") => {
            return Some("Moves the root MovieClip playhead to Frame 1 of the previous Scene.");
        }
        ("MovieClip", "addFrameScript") => {
            return Some(
                "Registers zero-based frame callbacks (`addFrameScript(0, fn)`) invoked whenever the MovieClip enters those frames.",
            );
        }
        ("MovieClip", "currentFrame" | "_currentframe") => {
            return Some(
                "Read-only 1-based index of the frame where this MovieClip's playhead currently rests.",
            );
        }
        ("MovieClip", "totalFrames" | "_totalframes") => {
            return Some("Read-only total number of frames on this MovieClip's timeline.");
        }
        ("MovieClip", "framesLoaded" | "_framesloaded") => {
            return Some("Read-only count of timeline frames loaded and ready for playback.");
        }
        ("MovieClip", "currentLabel") => {
            return Some(
                "Read-only name of the most recent frame label at or before `currentFrame` in the active Scene, or `null` if none precedes it.",
            );
        }
        ("MovieClip", "currentFrameLabel") => {
            return Some(
                "Read-only frame label authored directly on `currentFrame`, or `null` when that exact frame has no label.",
            );
        }
        ("MovieClip", "currentLabels") => {
            return Some("Read-only array of `FrameLabel` objects authored on the current Scene.");
        }
        ("MovieClip", "currentScene") => {
            return Some(
                "Read-only `Scene` descriptor of the Scene currently containing the playhead.",
            );
        }
        ("MovieClip", "scenes") => {
            return Some("Read-only array of all `Scene` objects authored in this MovieClip.");
        }
        ("MovieClip", "isPlaying") => {
            return Some(
                "Read-only boolean indicating whether this MovieClip's timeline is currently playing.",
            );
        }
        ("MovieClip", "attachMovie") => {
            return Some(
                "Instantiates a Library symbol by Linkage Identifier as a child MovieClip at `depth`, optionally copying properties from `initObject`.",
            );
        }
        ("MovieClip", "createEmptyMovieClip") => {
            return Some(
                "Creates a new empty child MovieClip at `depth` ready for runtime vector drawing or media loading.",
            );
        }
        ("MovieClip", "createTextField") => {
            return Some(
                "Creates a new child `TextField` at `depth` with `(x, y, width, height)` bounds in parent coordinates.",
            );
        }
        ("MovieClip", "duplicateMovieClip") => {
            return Some(
                "Clones this MovieClip instance into a sibling clip at `depth` on the same parent timeline.",
            );
        }
        ("MovieClip", "removeMovieClip") => {
            return Some(
                "Removes this MovieClip from its parent if it occupies a positive dynamic depth (`0..1048575`).",
            );
        }
        ("MovieClip", "swapDepths") => {
            return Some(
                "Exchanges z-order depth with another sibling MovieClip or moves this clip to an explicit numeric depth.",
            );
        }
        ("MovieClip", "getDepth") => {
            return Some(
                "Returns the integer stacking depth of this instance inside its parent timeline.",
            );
        }
        ("MovieClip", "getNextHighestDepth") => {
            return Some(
                "Returns the smallest unused non-negative integer depth (`>= 0`) above all current children of this MovieClip.",
            );
        }
        ("MovieClip", "getInstanceAtDepth") => {
            return Some(
                "Returns the child MovieClip occupying `depth`, or `undefined` if that depth is empty.",
            );
        }
        ("MovieClip", "setMask") => {
            return Some(
                "Assigns another MovieClip as a runtime alpha/shape mask clipping this MovieClip (`null` clears the mask).",
            );
        }
        ("MovieClip", "attachBitmap") => {
            return Some(
                "Attaches a `BitmapData` pixel surface to this MovieClip at `depth` with optional pixel snapping and smoothing.",
            );
        }
        ("Sound", "play") => {
            return Some(
                "Starts playing the sound from `startTime` ms for `loops` repetitions with an optional `SoundTransform`, returning a `SoundChannel`.",
            );
        }
        ("Sound", "start") => {
            return Some(
                "Begins playing the attached or loaded ActionScript 2.0 sound with an optional second offset and loop count.",
            );
        }
        ("Sound", "stop") => {
            return Some(
                "Stops all currently playing sounds or only the specified Linkage Identifier sound.",
            );
        }
        ("Sound", "attachSound") => {
            return Some(
                "Binds a Library audio asset exported with Linkage Identifier `id` to this `Sound` object.",
            );
        }
        ("Sound", "loadSound") => {
            return Some(
                "Loads an external audio URL into this `Sound`, streaming immediately when `isStreaming` is `true`.",
            );
        }
        ("SoundChannel", "stop") => {
            return Some("Halts audio playback on this `SoundChannel` immediately.");
        }
        ("SoundChannel", "position") => {
            return Some(
                "Read-only current playback offset in milliseconds within the playing sound.",
            );
        }
        ("SoundMixer", "stopAll") => {
            return Some(
                "Stops every currently playing `SoundChannel` and timeline sound across the runtime.",
            );
        }
        ("SoundMixer", "computeSpectrum") => {
            return Some(
                "Writes 512 floating-point stereo waveform or FFT frequency samples (`256` left + `256` right) into `outputArray`.",
            );
        }
        ("Timer", "start") => {
            return Some("Starts the `Timer` if it is not already running.");
        }
        ("Timer", "stop") => {
            return Some("Pauses the `Timer` without resetting `currentCount`.");
        }
        ("Timer", "reset") => {
            return Some("Stops the `Timer` if running and resets `currentCount` back to `0`.");
        }
        ("Timer", "delay") => {
            return Some(
                "Interval duration in milliseconds between `TimerEvent.TIMER` dispatches.",
            );
        }
        ("Timer", "repeatCount") => {
            return Some(
                "Total number of ticks to run before stopping (`0` repeats indefinitely).",
            );
        }
        ("Timer", "currentCount") => {
            return Some(
                "Read-only number of times this `Timer` has fired since starting or last `reset()`.",
            );
        }
        ("NetStream", "play") => {
            return Some(
                "Begins progressive HTTP or streaming media playback for the given URL or stream name.",
            );
        }
        ("NetStream", "pause") => {
            return Some("Pauses media playback on this `NetStream` at the current playhead time.");
        }
        ("NetStream", "resume") => {
            return Some("Resumes media playback on a paused `NetStream`.");
        }
        ("NetStream", "togglePause") => {
            return Some("Pauses a playing `NetStream` or resumes a paused `NetStream`.");
        }
        ("NetStream", "seek") => {
            return Some("Seeks the stream playhead to the nearest keyframe at `offset` seconds.");
        }
        ("NetStream", "close") => {
            return Some(
                "Stops playback, releases buffered media data, and detaches network resources for this stream.",
            );
        }
        ("NetStream", "time") => {
            return Some("Read-only current playback position of the media stream in seconds.");
        }
        ("Video", "attachNetStream" | "attachVideo") => {
            return Some(
                "Connects a `NetStream` or `Camera` video feed to render inside this `Video` display object.",
            );
        }
        ("Video", "clear") => {
            return Some("Clears the currently displayed video frame from the `Video` viewport.");
        }
        ("Point", "length") => {
            return Some("Read-only Euclidean distance from the origin `(0, 0)` to `(x, y)`.");
        }
        ("Point", "distance") => {
            return Some("Returns the Euclidean distance between points `pt1` and `pt2`.");
        }
        ("Point", "interpolate") => {
            return Some(
                "Returns a `Point` interpolated between `pt2` (`f = 0`) and `pt1` (`f = 1`).",
            );
        }
        ("Point", "polar") => {
            return Some(
                "Converts polar coordinates `(len, angleRadians)` into a Cartesian `Point`.",
            );
        }
        ("Point", "normalize") => {
            return Some(
                "Scales the `(x, y)` vector in place so its Euclidean length equals `thickness`.",
            );
        }
        ("Matrix", "createBox") => {
            return Some(
                "Configures scaling, rotation (radians), and translation in a single call.",
            );
        }
        ("Matrix", "createGradientBox") => {
            return Some(
                "Configures this `Matrix` for linear or radial gradient fills across `(width, height, rotation, tx, ty)`.",
            );
        }
        ("Matrix", "identity") => {
            return Some(
                "Resets this matrix to the 2D identity matrix `(a=1, b=0, c=0, d=1, tx=0, ty=0)`.",
            );
        }
        ("Matrix", "invert") => {
            return Some("Replaces this matrix in place with its mathematical inverse.");
        }
        ("Matrix", "concat") => {
            return Some(
                "Post-multiplies this matrix with `m`, combining both 2D affine transformations.",
            );
        }
        ("Matrix", "transformPoint") => {
            return Some(
                "Applies the full affine matrix (including `tx`, `ty` translation) to `point` and returns the transformed `Point`.",
            );
        }
        ("Matrix", "deltaTransformPoint") => {
            return Some(
                "Applies scale, skew, and rotation to `point` while ignoring `tx` and `ty` translation.",
            );
        }
        ("BitmapData", "draw") => {
            return Some(
                "Rasterizes a `DisplayObject` or `BitmapData` onto this bitmap using an optional `Matrix`, `ColorTransform`, blend mode, and `clipRect`.",
            );
        }
        ("BitmapData", "copyPixels") => {
            return Some(
                "Blits a rectangular region from `sourceBitmapData` to `destPoint` with optional alpha-bitmap masking and alpha merging.",
            );
        }
        ("BitmapData", "fillRect") => {
            return Some(
                "Fills a rectangular pixel region with a 32-bit ARGB (transparent) or 24-bit RGB color.",
            );
        }
        ("BitmapData", "floodFill") => {
            return Some(
                "Performs a contiguous paint-bucket fill starting at pixel `(x, y)` with `color`.",
            );
        }
        ("BitmapData", "getPixel") => {
            return Some(
                "Returns the 24-bit `0xRRGGBB` color value at pixel `(x, y)` without alpha.",
            );
        }
        ("BitmapData", "getPixel32") => {
            return Some(
                "Returns the 32-bit `0xAARRGGBB` premultiplied/unmultiplied pixel color at `(x, y)`.",
            );
        }
        ("BitmapData", "setPixel") => {
            return Some(
                "Sets the 24-bit RGB color of pixel `(x, y)` while preserving its existing alpha channel.",
            );
        }
        ("BitmapData", "setPixel32") => {
            return Some("Sets the full 32-bit `0xAARRGGBB` color and alpha of pixel `(x, y)`.");
        }
        ("BitmapData", "perlinNoise") => {
            return Some(
                "Generates multi-octave procedural Perlin gradient noise across the bitmap.",
            );
        }
        ("BitmapData", "threshold") => {
            return Some(
                "Tests pixels against a masked threshold value and replaces or copies matching pixels.",
            );
        }
        ("BitmapData", "applyFilter") => {
            return Some(
                "Applies a `BitmapFilter` to `sourceRect` of `sourceBitmapData` and writes the filtered pixels to `destPoint`.",
            );
        }
        ("BitmapData", "dispose") => {
            return Some(
                "Immediately frees the underlying pixel buffer memory of this `BitmapData`.",
            );
        }
        ("SharedObject", "getLocal") => {
            return Some(
                "Returns a reference to a persistent local `SharedObject` identified by `name` and optional `localPath`.",
            );
        }
        ("SharedObject", "flush") => {
            return Some("Immediately commits `SharedObject.data` changes to persistent storage.");
        }
        ("SharedObject", "clear") => {
            return Some(
                "Deletes all properties from `SharedObject.data` and erases the persisted store.",
            );
        }
        ("Function", "call") => {
            return Some(
                "Invokes this function with `this` explicitly bound to `thisObject` and individual positional arguments.",
            );
        }
        ("Function", "apply") => {
            return Some(
                "Invokes this function with `this` bound to `thisObject` and positional arguments unpacked from `argArray`.",
            );
        }
        ("Object", "hasOwnProperty") => {
            return Some(
                "Returns `true` if `name` is defined directly on this object rather than inherited through its prototype chain.",
            );
        }
        ("Object", "registerClass") => {
            return Some(
                "Associates a Library symbol Linkage Identifier with an ActionScript 1.0/2.0 constructor function.",
            );
        }
        ("Key", "isDown") => {
            return Some("Returns `true` while the physical key matching `code` is held down.");
        }
        ("Key", "getCode") => {
            return Some("Returns the key code of the last key pressed or released.");
        }
        ("Mouse", "hide") => {
            return Some("Hides the native OS pointer cursor while over the Stage.");
        }
        ("Mouse", "show") => {
            return Some("Restores visibility of the native OS pointer cursor.");
        }
        _ => {}
    }

    // Shared high-frequency display, graphics, text, event, collection, math, and ByteArray members.
    match (as3, kind, name) {
        // Display properties
        (_, "property", "x" | "_x") => Some(
            "Horizontal position of the instance's registration point in parent coordinates (pixels).",
        ),
        (_, "property", "y" | "_y") => Some(
            "Vertical position of the instance's registration point in parent coordinates (pixels).",
        ),
        (true, "property", "z") => {
            Some("Depth position along the 3D z-axis in parent coordinates (pixels).")
        }
        (_, "property", "width" | "_width") => {
            Some("Measured or scaled horizontal width of the object in pixels.")
        }
        (_, "property", "height" | "_height") => {
            Some("Measured or scaled vertical height of the object in pixels.")
        }
        (false, "property", "_xscale") => Some(
            "Horizontal scale percentage relative to the authored symbol (`100` = 100% natural width).",
        ),
        (false, "property", "_yscale") => Some(
            "Vertical scale percentage relative to the authored symbol (`100` = 100% natural height).",
        ),
        (true, "property", "scaleX") => Some(
            "Horizontal scale multiplier relative to the registration point (`1.0` = 100% natural width).",
        ),
        (true, "property", "scaleY") => Some(
            "Vertical scale multiplier relative to the registration point (`1.0` = 100% natural height).",
        ),
        (_, "property", "rotation" | "_rotation") => {
            Some("Clockwise rotation angle in degrees (`-180..180`) around the registration point.")
        }
        (false, "property", "_alpha") => {
            Some("Opacity percentage of the display object (`0` = transparent, `100` = opaque).")
        }
        (true, "property", "alpha") => {
            Some("Opacity multiplier (`0.0` = transparent, `1.0` = opaque).")
        }
        (_, "property", "visible" | "_visible") => Some(
            "Controls whether the display object is rendered and participates in pointer hit testing.",
        ),
        (_, "property", "name" | "_name") => {
            Some("Instance name used for child lookup (`getChildByName` or dot-path addressing).")
        }
        (_, "property", "parent" | "_parent") => Some(
            "Read-only reference to the `DisplayObjectContainer` or `MovieClip` containing this object.",
        ),
        (true, "property", "root") => Some(
            "Read-only top-most display object of the loaded SWF/Timeline tree (`null` while off-list).",
        ),
        (true, "property", "stage") => Some(
            "Read-only reference to the `Stage` viewport while on the display list, or `null` when detached.",
        ),
        (_, "property", "mouseX" | "_xmouse") => Some(
            "Read-only horizontal pointer coordinate measured in this object's local coordinate space.",
        ),
        (_, "property", "mouseY" | "_ymouse") => Some(
            "Read-only vertical pointer coordinate measured in this object's local coordinate space.",
        ),
        (_, "property", "mask") => Some(
            "Display object whose silhouette clips this display object (`null` removes masking).",
        ),
        (_, "property", "filters") => Some(
            "Array of live `BitmapFilter` instances (`BlurFilter`, `GlowFilter`, `DropShadowFilter`, etc.) applied during compositing; reassign the array after mutating.",
        ),
        (_, "property", "blendMode") => Some(
            "Compositing blend mode (`\"normal\"`, `\"multiply\"`, `\"screen\"`, `\"add\"`, `\"overlay\"`, `\"erase\"`, etc.).",
        ),
        (_, "property", "cacheAsBitmap") => Some(
            "Caches an internal rasterized bitmap of the vector display object when its matrix only translates.",
        ),
        (_, "property", "scale9Grid") => Some(
            "Inner `Rectangle` defining 9-slice scaling so corners stay unstretched while edges and center scale.",
        ),
        (_, "property", "scrollRect") => {
            Some("Viewport clipping and scrolling `Rectangle` in local coordinates.")
        }
        (_, "property", "transform") => Some(
            "Accesses the object's `Transform` (2D/3D `Matrix`, `ColorTransform`, and concatenated bounds).",
        ),
        (true, "property", "graphics") => Some(
            "Read-only `Graphics` vector drawing surface owned by this `Shape`, `Sprite`, or `MovieClip`.",
        ),
        (true, "property", "buttonMode") => Some(
            "Enables button semantics and hand-cursor feedback on this `Sprite` or `MovieClip`.",
        ),
        (_, "property", "useHandCursor") => Some(
            "Controls whether hovering over an enabled button displays the pointing-hand cursor.",
        ),
        (_, "property", "mouseEnabled") => Some(
            "Controls whether this `InteractiveObject` receives pointer events (`MouseEvent`).",
        ),
        (true, "property", "mouseChildren") => Some(
            "Controls whether descendants inside this container receive pointer events independently.",
        ),
        (true, "property", "numChildren") => {
            Some("Read-only count of direct child display objects currently inside this container.")
        }
        // Display container & coordinate methods
        (true, "method", "addChild") => Some(
            "Appends `child` to the top of this container's display list (reparenting it if already elsewhere).",
        ),
        (true, "method", "addChildAt") => Some(
            "Inserts `child` into this container's display list at zero-based z-order `index`.",
        ),
        (true, "method", "removeChild") => {
            Some("Removes `child` from this container's display list and returns it.")
        }
        (true, "method", "removeChildAt") => {
            Some("Removes and returns the child display object at zero-based `index`.")
        }
        (true, "method", "removeChildren") => Some(
            "Removes all children between `beginIndex` and `endIndex` inclusive from this container.",
        ),
        (true, "method", "getChildAt") => {
            Some("Returns the direct child display object at zero-based z-order `index`.")
        }
        (true, "method", "getChildByName") => {
            Some("Returns the first direct child matching `name`, or `null` if none exists.")
        }
        (true, "method", "getChildIndex") => {
            Some("Returns the zero-based z-order index of `child` inside this container.")
        }
        (true, "method", "setChildIndex") => {
            Some("Moves an existing `child` to a new zero-based z-order `index`.")
        }
        (true, "method", "swapChildren") => {
            Some("Exchanges the z-order stacking positions of `child1` and `child2`.")
        }
        (true, "method", "swapChildrenAt") => {
            Some("Exchanges the z-order positions of the children at `index1` and `index2`.")
        }
        (_, "method", "localToGlobal") => Some(
            "Converts a `Point` from this object's local coordinate space into Stage coordinates.",
        ),
        (_, "method", "globalToLocal") => Some(
            "Converts a `Point` from Stage coordinates into this object's local coordinate space.",
        ),
        (_, "method", "getBounds") => Some(
            "Returns the bounding `Rectangle` of this object (including strokes) in `targetCoordinateSpace`.",
        ),
        (true, "method", "getRect") => Some(
            "Returns the fill-only bounding `Rectangle` (excluding stroke widths) in `targetCoordinateSpace`.",
        ),
        (true, "method", "hitTestObject") => {
            Some("Returns `true` if the bounding box of this display object overlaps `obj`.")
        }
        (true, "method", "hitTestPoint") => Some(
            "Tests whether Stage point `(x, y)` intersects this object's bounding box or exact vector/pixel shape (`shapeFlag = true`).",
        ),
        (false, "method", "hitTest") => Some(
            "Tests bounding-box collision with another MovieClip or tests Stage point `(x, y)` with optional `shapeFlag`.",
        ),
        (_, "method", "startDrag") => Some(
            "Begins dragging this clip with the pointer, optionally locking its center and clamping within `bounds`.",
        ),
        (_, "method", "stopDrag") => {
            Some("Ends an active pointer drag started with `startDrag()`.")
        }
        // Graphics & vector drawing
        (_, "method", "clear") => Some(
            "Clears all vector paths, fills, and line styles previously drawn on this graphics context.",
        ),
        (_, "method", "lineStyle") => Some(
            "Sets the stroke thickness, RGB color, alpha, pixel hinting, scale mode, caps, and joints for subsequent path segments.",
        ),
        (_, "method", "beginFill") => Some(
            "Begins a solid-color vector fill (`color`, `alpha`) closed automatically on `endFill()`.",
        ),
        (_, "method", "beginGradientFill") => {
            Some("Begins a linear or radial multi-stop gradient fill transformed by `matrix`.")
        }
        (_, "method", "beginBitmapFill") => {
            Some("Begins a repeating or clipped `BitmapData` pattern fill transformed by `matrix`.")
        }
        (_, "method", "endFill") => Some(
            "Closes and paints the fill started by the most recent `beginFill`, `beginGradientFill`, or `beginBitmapFill`.",
        ),
        (_, "method", "moveTo") => {
            Some("Moves the current vector drawing pen to `(x, y)` without drawing a stroke.")
        }
        (_, "method", "lineTo") => {
            Some("Draws a straight vector segment from the current pen position to `(x, y)`.")
        }
        (_, "method", "curveTo") => Some(
            "Draws a quadratic Bezier curve using control point `(controlX, controlY)` to anchor `(anchorX, anchorY)`.",
        ),
        (true, "method", "cubicCurveTo") => {
            Some("Draws a cubic Bezier curve through two control points to `(anchorX, anchorY)`.")
        }
        (true, "method", "drawRect") => {
            Some("Draws an axis-aligned rectangle at `(x, y)` of size `(width, height)`.")
        }
        (true, "method", "drawRoundRect") => Some(
            "Draws a rounded rectangle with corner ellipse dimensions `(ellipseWidth, ellipseHeight)`.",
        ),
        (true, "method", "drawCircle") => {
            Some("Draws a circle centered at `(x, y)` with the given `radius`.")
        }
        (true, "method", "drawEllipse") => {
            Some("Draws an ellipse inscribed inside `(x, y, width, height)`.")
        }
        // Events
        (_, "method", "addEventListener") => Some(
            "Registers an event listener function for `type`, with optional capture phase, priority, and weak reference settings in AS3.",
        ),
        (_, "method", "removeEventListener") => Some(
            "Unregisters a previously added listener function for `type` (matching `useCapture` in AS3).",
        ),
        (_, "method", "dispatchEvent") => Some(
            "Dispatches `event` into the event flow on this target and returns `false` if `preventDefault()` was called.",
        ),
        (true, "method", "hasEventListener") => {
            Some("Returns `true` if listeners are registered directly on this object for `type`.")
        }
        (true, "method", "willTrigger") => Some(
            "Returns `true` if listeners for `type` are registered on this object or any display-list ancestor.",
        ),
        (true, "method", "stopPropagation") => Some(
            "Prevents the event from advancing to subsequent nodes in the capture or bubbling phase.",
        ),
        (true, "method", "stopImmediatePropagation") => Some(
            "Prevents any remaining listeners on the current node and all subsequent nodes from receiving this event.",
        ),
        (true, "method", "preventDefault") => {
            Some("Cancels the event's default runtime action when `event.cancelable` is `true`.")
        }
        // TextField
        (_, "property", "text") => {
            Some("Plain unformatted string content currently displayed in the text field.")
        }
        (_, "property", "htmlText") => Some(
            "HTML-marked-up string content (`<b>`, `<i>`, `<u>`, `<font>`, `<p>`, `<a href>`) of the text field.",
        ),
        (true, "property", "defaultTextFormat") => {
            Some("Formatting applied automatically to newly inserted or assigned `text`.")
        }
        (_, "method", "setTextFormat") => Some(
            "Applies `TextFormat` styling to an existing character range (or the entire text field).",
        ),
        (false, "method", "setNewTextFormat") => Some(
            "Sets the default `TextFormat` applied to newly typed or assigned text in ActionScript 2.0.",
        ),
        (_, "method", "getTextFormat") => Some(
            "Returns a `TextFormat` containing the formatting shared across the specified character range.",
        ),
        (true, "method", "appendText") => Some(
            "Appends `newText` efficiently to the end of `TextField.text` without rebuilding existing runs.",
        ),
        // Array / Vector / String / Math
        (_, "method", "push") => Some(
            "Appends one or more elements to the end of the list and returns the updated `length`.",
        ),
        (_, "method", "pop") => {
            Some("Removes and returns the last element of the list (or `undefined` when empty).")
        }
        (_, "method", "shift") => Some(
            "Removes and returns the first element at index `0`, shifting remaining elements down.",
        ),
        (_, "method", "unshift") => Some(
            "Inserts one or more elements at the start of the list and returns the updated `length`.",
        ),
        (_, "method", "splice") => Some(
            "Removes `deleteCount` elements starting at `startIndex`, optionally inserting replacement items in place.",
        ),
        (_, "method", "slice") => Some(
            "Returns a shallow copy of the sub-range `[startIndex, endIndex)` without mutating the source.",
        ),
        (_, "method", "join") => {
            Some("Concatenates all elements into a single `String` separated by `sep`.")
        }
        (_, "method", "sort") => Some(
            "Sorts elements in place using a custom comparator function or `Array` sort option flags.",
        ),
        (_, "method", "sortOn") => Some(
            "Sorts an array of objects in place by one or more named field keys and option flags.",
        ),
        (_, "method", "indexOf") => Some(
            "Returns the first zero-based index equal to `searchElement` from `fromIndex`, or `-1` if absent.",
        ),
        (_, "method", "lastIndexOf") => Some(
            "Returns the last zero-based index equal to `searchElement` searching backward, or `-1` if absent.",
        ),
        (true, "method", "forEach") => {
            Some("Invokes `callback(item, index, list)` once for each element in ascending order.")
        }
        (true, "method", "map") => Some(
            "Builds and returns a new sequence containing `callback(item, index, list)` for every element.",
        ),
        (true, "method", "filter") => Some(
            "Returns a new sequence containing only elements for which `callback(item, index, list)` returns `true`.",
        ),
        (true, "method", "every") => {
            Some("Returns `true` if `callback` returns truthy for every element in the sequence.")
        }
        (true, "method", "some") => {
            Some("Returns `true` as soon as `callback` returns truthy for at least one element.")
        }
        (_, "method", "split") => Some(
            "Splits this string around `delimiter` (string or `RegExp`) into an `Array` of substrings.",
        ),
        (_, "method", "substring") => {
            Some("Returns the characters between indices `startIndex` and `endIndex`.")
        }
        (_, "method", "substr") => Some("Returns up to `len` characters starting at `startIndex`."),
        (_, "method", "charAt") => {
            Some("Returns the 1-character string at zero-based character `index`.")
        }
        (_, "method", "charCodeAt") => {
            Some("Returns the numeric UTF-16 code unit (`0..65535`) at zero-based `index`.")
        }
        (_, "method", "fromCharCode") => {
            Some("Constructs a `String` from one or more UTF-16 numeric character codes.")
        }
        (_, "method", "toLowerCase") => {
            Some("Returns a copy of this string with uppercase characters converted to lowercase.")
        }
        (_, "method", "toUpperCase") => {
            Some("Returns a copy of this string with lowercase characters converted to uppercase.")
        }
        (true, "method", "replace") => Some(
            "Replaces the first (or all `/g` global) matches of `pattern` with `repl` string or callback output.",
        ),
        (true, "method", "match") => Some(
            "Matches this string against `pattern` and returns an `Array` of matches or `null`.",
        ),
        (true, "method", "search") => {
            Some("Returns the zero-based character index of the first `pattern` match, or `-1`.")
        }
        (_, "method", "sin") => {
            Some("Returns the trigonometric sine of an angle given in radians.")
        }
        (_, "method", "cos") => {
            Some("Returns the trigonometric cosine of an angle given in radians.")
        }
        (_, "method", "tan") => {
            Some("Returns the trigonometric tangent of an angle given in radians.")
        }
        (_, "method", "atan2") => Some(
            "Returns the counter-clockwise angle in radians (`-PI..PI`) from the positive x-axis to `(x, y)`.",
        ),
        (_, "method", "sqrt") => Some("Returns the non-negative square root of `val`."),
        (_, "method", "pow") => Some("Returns `base` raised to the power `pow`."),
        (_, "method", "abs") => Some("Returns the non-negative absolute magnitude of `val`."),
        (_, "method", "floor") => {
            Some("Rounds `val` down to the greatest integer less than or equal to `val`.")
        }
        (_, "method", "ceil") => {
            Some("Rounds `val` up to the smallest integer greater than or equal to `val`.")
        }
        (_, "method", "round") => {
            Some("Rounds `val` to the nearest integer (`0.5` rounds toward `+Infinity`).")
        }
        (_, "method", "min") => {
            Some("Returns the smallest numeric argument (`NaN` if any argument is `NaN`).")
        }
        (_, "method", "max") => {
            Some("Returns the largest numeric argument (`NaN` if any argument is `NaN`).")
        }
        (_, "method", "random") => {
            Some("Returns a pseudo-random floating-point number `n` where `0.0 <= n < 1.0`.")
        }
        (_, "method", "clone") => Some("Creates and returns an independent copy of this instance."),
        (_, "method", "toString") => {
            Some("Returns a formatted string representation of this object or value.")
        }
        (_, "method", "valueOf") => Some("Returns the primitive underlying value of this object."),
        _ => None,
    }
}

fn param_name_list(params: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut depth = 0_i32;
    let mut current = String::new();
    for ch in params.chars() {
        match ch {
            '(' | '[' | '<' | '{' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' | '>' | '}' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth <= 0 => {
                if let Some(n) = clean_one_param_name(&current) {
                    names.push(n);
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if let Some(n) = clean_one_param_name(&current) {
        names.push(n);
    }
    names
}

fn clean_one_param_name(raw: &str) -> Option<String> {
    let mut text = raw.trim().trim_matches(|c| c == '[' || c == ']').trim();
    if text.is_empty() {
        return None;
    }
    let rest_prefix = text.starts_with("...");
    if let Some(stripped) = text.strip_prefix("...") {
        text = stripped.trim();
    }
    let before_eq = text.split('=').next().unwrap_or(text).trim();
    let name = before_eq.split(':').next().unwrap_or(before_eq).trim();
    if name.is_empty() {
        rest_prefix.then(|| "...rest".to_owned())
    } else if rest_prefix {
        Some(format!("...{name}"))
    } else {
        Some(name.to_owned())
    }
}

fn synthesize_member_description(
    as3: bool,
    class_key: &str,
    class_name: &str,
    pseudo: bool,
    member: &Member,
) -> String {
    let owner = if pseudo {
        if class_key.starts_with('<') {
            "global scope"
        } else {
            class_key
        }
    } else {
        class_name
    };
    let name = member.name.as_str();
    let human = humanize_identifier(name);
    let ty = member.ty.trim();

    match member.kind.as_str() {
        "constructor" => {
            let params = member
                .params
                .as_deref()
                .map(param_name_list)
                .unwrap_or_default();
            if params.is_empty() {
                format!("Creates a new `{class_name}` instance with default settings.")
            } else {
                format!(
                    "Creates a new `{class_name}` instance initialized with ({}).",
                    params.join(", ")
                )
            }
        }
        "constant" => {
            let val_note = member
                .value
                .as_deref()
                .filter(|v| !v.is_empty())
                .map_or_else(String::new, |v| format!(" (= {v})"));
            if class_name == "Keyboard" || class_name == "Key" {
                format!(
                    "Key constant `{class_name}.{name}`{val_note} representing the {human} key for keyboard input checks."
                )
            } else if class_name.ends_with("Event") {
                format!(
                    "Event type constant `{class_name}.{name}`{val_note} passed to `addEventListener()` and matched on `{class_name}.type`."
                )
            } else if pseudo {
                format!("Global constant `{name}`{val_note} representing {human}.")
            } else {
                let ty_note = if ty.is_empty() {
                    String::new()
                } else {
                    format!(" of type `{}`", short_name(ty))
                };
                format!(
                    "Constant `{class_name}.{name}`{val_note}{ty_note} specifying the {human} option or value."
                )
            }
        }
        "event" | "callback" => {
            if as3 {
                format!(
                    "Dispatched by `{owner}` when {human} occurs; register a listener with `addEventListener(\"{name}\", handler)`."
                )
            } else if let Some(rest) = name.strip_prefix("on").filter(|r| !r.is_empty()) {
                format!(
                    "Callback handler invoked on `{owner}` when the {} condition occurs.",
                    humanize_identifier(rest)
                )
            } else {
                format!(
                    "Component or broadcaster event `{name}` emitted by `{owner}` when {human} occurs."
                )
            }
        }
        "property" => {
            let ro = if member.read_only { "Read-only " } else { "" };
            let scope_prefix = if pseudo {
                "global "
            } else if member.is_static {
                "static "
            } else {
                ""
            };
            let cap_head = {
                let combined = format!("{ro}{scope_prefix}property");
                let mut chars = combined.chars();
                match chars.next() {
                    Some(c) => c.to_uppercase().chain(chars).collect::<String>(),
                    None => "Property".to_owned(),
                }
            };
            let ty_clause = if ty.is_empty() {
                String::new()
            } else {
                format!(" (`{}`)", short_name(ty))
            };
            if ty == "Boolean"
                || name.starts_with("is")
                || name.starts_with("has")
                || name.starts_with("can")
                || name.ends_with("Enabled")
            {
                format!("{cap_head}{ty_clause} on `{owner}` indicating whether {human}.")
            } else if member.read_only {
                format!("{cap_head}{ty_clause} returning the {human} of `{owner}`.")
            } else {
                format!("{cap_head}{ty_clause} getting or setting the {human} of `{owner}`.")
            }
        }
        _ => {
            // method or function
            let params = member
                .params
                .as_deref()
                .map(param_name_list)
                .unwrap_or_default();
            let args_clause = if params.is_empty() {
                String::new()
            } else {
                format!(" using ({})", params.join(", "))
            };
            let ret_clause = if ty.is_empty() || matches!(ty, "void" | "Void") {
                String::new()
            } else {
                format!(" and returns `{}`", short_name(ty))
            };
            let receiver = if pseudo {
                format!("in `{owner}`")
            } else if member.is_static {
                format!("on `{class_name}`")
            } else {
                format!("on this `{class_name}`")
            };
            if let Some(rest) = name
                .strip_prefix("get")
                .filter(|r| r.starts_with(char::is_uppercase))
            {
                format!(
                    "Returns the {} {receiver}{args_clause}{ret_clause}.",
                    humanize_identifier(rest)
                )
            } else if let Some(rest) = name
                .strip_prefix("set")
                .filter(|r| r.starts_with(char::is_uppercase))
            {
                format!(
                    "Updates the {} {receiver}{args_clause}{ret_clause}.",
                    humanize_identifier(rest)
                )
            } else if name.starts_with("is")
                || name.starts_with("has")
                || name.starts_with("can")
                || name.starts_with("supports")
            {
                format!("Checks whether {human} {receiver}{args_clause}{ret_clause}.")
            } else if let Some(rest) = name
                .strip_prefix("add")
                .filter(|r| r.starts_with(char::is_uppercase))
            {
                format!(
                    "Adds or registers {} {receiver}{args_clause}{ret_clause}.",
                    humanize_identifier(rest)
                )
            } else if let Some(rest) = name
                .strip_prefix("remove")
                .filter(|r| r.starts_with(char::is_uppercase))
            {
                format!(
                    "Removes {} {receiver}{args_clause}{ret_clause}.",
                    humanize_identifier(rest)
                )
            } else if let Some(rest) = name
                .strip_prefix("create")
                .filter(|r| r.starts_with(char::is_uppercase))
            {
                format!(
                    "Creates {} {receiver}{args_clause}{ret_clause}.",
                    humanize_identifier(rest)
                )
            } else if let Some(rest) = name
                .strip_prefix("read")
                .filter(|r| r.starts_with(char::is_uppercase))
            {
                format!(
                    "Reads {} from `{class_name}`{args_clause}{ret_clause}.",
                    humanize_identifier(rest)
                )
            } else if let Some(rest) = name
                .strip_prefix("write")
                .filter(|r| r.starts_with(char::is_uppercase))
            {
                format!(
                    "Writes {} into `{class_name}`{args_clause}{ret_clause}.",
                    humanize_identifier(rest)
                )
            } else if let Some(rest) = name
                .strip_prefix("load")
                .filter(|r| r.starts_with(char::is_uppercase))
            {
                format!(
                    "Loads {} {receiver}{args_clause}{ret_clause}.",
                    humanize_identifier(rest)
                )
            } else if let Some(rest) = name
                .strip_prefix("draw")
                .filter(|r| r.starts_with(char::is_uppercase))
            {
                format!(
                    "Draws {} {receiver}{args_clause}{ret_clause}.",
                    humanize_identifier(rest)
                )
            } else if let Some(rest) = name
                .strip_prefix("to")
                .filter(|r| r.starts_with(char::is_uppercase))
            {
                format!(
                    "Converts `{class_name}` to {} representation{args_clause}{ret_clause}.",
                    humanize_identifier(rest)
                )
            } else {
                format!("Executes `{name}()` ({human}) {receiver}{args_clause}{ret_clause}.")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_reference_token_handles_caret_and_selection() {
        let src = "function calcDamage(base:Number):Number {\n    return arguments.callee;\n}";
        assert_eq!(
            extract_reference_token(src, 0, 0).as_deref(),
            Some("function")
        );
        assert_eq!(
            extract_reference_token(src, 8, 8).as_deref(),
            Some("function")
        );
        assert_eq!(
            extract_reference_token(src, 12, 12).as_deref(),
            Some("calcDamage")
        );
        // Explicit selection across dotted expression
        let sel_start = src.find("arguments.callee").unwrap();
        let sel_end = sel_start + "arguments.callee".len();
        assert_eq!(
            extract_reference_token(src, sel_start, sel_end).as_deref(),
            Some("arguments.callee")
        );
    }

    #[test]
    fn keyword_reference_summary_distinguishes_dialects() {
        let as2 = keyword_reference_summary(ScriptLanguage::ActionScript2, "function").unwrap();
        let as3 = keyword_reference_summary(ScriptLanguage::ActionScript3, "function").unwrap();
        let ts = keyword_reference_summary(ScriptLanguage::TypeScript, "function").unwrap();
        assert!(as2.contains("AS1/AS2"));
        assert!(as3.contains("AS3"));
        assert!(ts.contains("JS/TS"));
        assert!(
            keyword_reference_summary(ScriptLanguage::ActionScript2, "callee")
                .unwrap()
                .contains("arguments.callee")
        );
    }

    #[test]
    fn curated_and_synthesized_descriptions_are_specific_and_non_empty() {
        let mc_desc = describe_as_class(
            ScriptLanguage::ActionScript3,
            "flash.display.MovieClip",
            "MovieClip",
            "flash.display",
            "class",
            &["flash.display.Sprite".into()],
            &["flash.display.IBitmapDrawable".into()],
        );
        assert!(mc_desc.contains("Sprite") && mc_desc.contains("playhead"));

        let click = Member {
            owner: "flash.events.MouseEvent".into(),
            name: "CLICK".into(),
            kind: "constant".into(),
            ty: "String".into(),
            is_static: true,
            value: Some("\"click\"".into()),
            ..Member::default()
        };
        let click_desc = describe_as_member(
            ScriptLanguage::ActionScript3,
            "flash.events.MouseEvent",
            "MouseEvent",
            false,
            &click,
        );
        assert!(click_desc.contains("\"click\""));
        assert!(click_desc.contains("addEventListener"));
    }
}
