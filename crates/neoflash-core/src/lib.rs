//! Document model and deterministic authoring/runtime evaluation shared by desktop and web.
mod amf;
pub mod as2_binding;
pub mod as2_boxing;
pub mod as2_capture;
pub mod as2_connections;
pub mod as2_data;
pub mod as2_files;
pub mod as2_locale;
pub mod as2_media;
pub mod as2_object;
pub mod as2_platform;
pub mod as2_services;
pub mod as2_sound;
pub mod as2_stage;
pub mod as2_strings;
pub mod as2_text;
pub mod as2_transitions;
pub mod as2_variables;
pub mod as2_xml;
pub mod as2d_geom;
pub mod as3_collections;
pub mod as3_geom;
pub mod as3_object;
pub mod as3_strings;
pub mod as_binary;
pub mod as_broadcaster;
pub mod as_date;
pub mod as_easing;
pub mod as_events;
pub mod as_math;
pub mod as_number_format;
pub mod as_timers;
mod authored_layers;
mod clipboard;
mod demo;
mod evaluate;
mod filters;
mod geometry;
mod history;
mod library_folders;
pub mod model;
mod pixels;
mod resource_image;
mod resources;
mod runtime;
mod runtime_graph;
mod runtime_graphics;
mod text;
mod validation;
mod xml;

pub use authored_layers::{
    AuthoredDraw, AuthoredScene, evaluate_scene, layer_effective_outline_color,
    layer_effectively_locked, layer_effectively_visible, layer_outline_color,
};
pub use clipboard::{
    CLIPBOARD_PREFIX, ClipboardError, ClipboardPayload, MAX_CLIPBOARD_BYTES, PasteResult,
};
pub use evaluate::{DrawItem, affine_multiply, easing, evaluate, sample_layer, transform_matrix};
pub use filters::{
    FilterError, FilterPadding, FilterSpec, MAX_FILTER_PIXELS, MAX_OBJECT_FILTERS, filter_padding,
    filter_rgba, validate_filters,
};
pub use geometry::{
    FlattenOptions, FlattenedContour, GeometryError, MAX_PATH_COMMANDS, PathBounds, flatten_path,
    hit_test_path, path_bounds, paths_have_same_topology, validate_path_commands,
};
pub use history::History;
pub use library_folders::valid_library_folder;
pub use model::*;
pub use pixels::{
    As2BitmapCommand, BitmapError, BitmapRect, MAX_BITMAP_BYTES, MAX_BITMAP_DIMENSION,
    PixelDescriptor, PixelRef, PixelSnapping, PixelWrite,
};
pub use resource_image::DecodedImage;
pub use resources::{
    MAX_PROJECT_JSON_BYTES, MAX_RESOURCE_BYTES, MAX_RESOURCE_TOTAL_BYTES, MAX_RESOURCES,
    ResourceError, ResourceUnit,
};
pub use runtime::{
    AccessibleNode, DisplayNode, FrameLabel, PlaybackCommand, Player, ScriptDispatch, ScriptEvent,
    TimelineMetadata,
};
pub use runtime_graph::{
    ClipComponent, ColorTransform, DisplayError, DisplayEvent, DisplayInputState, DisplayMutation,
    DynamicKind, LoaderMutation, MAX_DYNAMIC_OBJECTS, MutationResult, ObjectRef,
    RuntimeAccessibleNode, RuntimeClip, RuntimeDrawItem, RuntimeDrawKind, RuntimeFilterGroup,
    RuntimeObject, RuntimeScriptDispatch, RuntimeSnapshot, ScrollRect, VideoState,
};
pub use runtime_graphics::{
    GradientKind, GradientPaint, GradientSpec, GradientStop, GraphicsOp, MAX_GRAPHICS_COMMANDS,
    MAX_GRAPHICS_OPS,
};
pub use text::{
    MAX_TEXT_BYTES, TextAlign, TextBoxSize, TextError, TextFieldState, TextFieldType, TextFormat,
    TextFormatPatch, TextInputEdit, TextLayout, TextLine, TextPatch, TextReplacement,
    TextRestrictPatch, TextSelection, layout_text,
};
pub use validation::{ProjectError, ValidationIssue, validate};
pub use xml::{
    MAX_XML_ATTRIBUTES, MAX_XML_BYTES, MAX_XML_DEPTH, MAX_XML_ITEMS, MAX_XML_NAME_BYTES,
    MAX_XML_STRING_BYTES, XmlAttribute, XmlDeclaration, XmlErrorKind, XmlName, XmlNode,
    XmlNodeKind, XmlParseError, XmlParseTree, parse_xml,
};

pub use amf::{
    AmfDecoded, AmfError, AmfGraph, AmfNode, AmfValue, MAX_AMF_BYTES, MAX_AMF_JSON_BYTES,
    amf_decode, amf_encode,
};
