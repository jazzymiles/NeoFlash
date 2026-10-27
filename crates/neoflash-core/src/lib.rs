//! Document model and deterministic authoring/runtime evaluation shared by desktop and web.
mod clipboard;
mod demo;
mod evaluate;
mod geometry;
mod history;
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

pub use clipboard::{
    CLIPBOARD_PREFIX, ClipboardError, ClipboardPayload, MAX_CLIPBOARD_BYTES, PasteResult,
};
pub use evaluate::{DrawItem, affine_multiply, easing, evaluate, sample_layer, transform_matrix};
pub use geometry::{
    FlattenOptions, FlattenedContour, GeometryError, MAX_PATH_COMMANDS, PathBounds, flatten_path,
    hit_test_path, path_bounds, paths_have_same_topology, validate_path_commands,
};
pub use history::History;
pub use model::*;
pub use pixels::{
    BitmapError, MAX_BITMAP_BYTES, MAX_BITMAP_DIMENSION, PixelDescriptor, PixelRef, PixelSnapping,
};
pub use resource_image::DecodedImage;
pub use resources::{
    MAX_PROJECT_JSON_BYTES, MAX_RESOURCE_BYTES, MAX_RESOURCE_TOTAL_BYTES, MAX_RESOURCES,
    ResourceError, ResourceUnit,
};
pub use runtime::{
    AccessibleNode, DisplayNode, PlaybackCommand, Player, ScriptDispatch, ScriptEvent,
};
pub use runtime_graph::{
    ClipComponent, ColorTransform, DisplayError, DisplayEvent, DisplayMutation, DynamicKind,
    LoaderMutation, MAX_DYNAMIC_OBJECTS, MutationResult, ObjectRef, RuntimeAccessibleNode,
    RuntimeClip, RuntimeDrawItem, RuntimeDrawKind, RuntimeObject, RuntimeScriptDispatch,
    RuntimeSnapshot, ScrollRect,
};
pub use runtime_graphics::{
    GradientKind, GradientPaint, GradientSpec, GradientStop, GraphicsOp, MAX_GRAPHICS_COMMANDS,
    MAX_GRAPHICS_OPS,
};
pub use text::{
    MAX_TEXT_BYTES, TextAlign, TextBoxSize, TextError, TextFieldState, TextFormat, TextFormatPatch,
    TextLayout, TextLine, TextPatch, TextReplacement, layout_text,
};
pub use validation::{ProjectError, ValidationIssue, validate};
pub use xml::{
    MAX_XML_ATTRIBUTES, MAX_XML_BYTES, MAX_XML_DEPTH, MAX_XML_ITEMS, MAX_XML_NAME_BYTES,
    MAX_XML_STRING_BYTES, XmlAttribute, XmlDeclaration, XmlErrorKind, XmlName, XmlNode,
    XmlNodeKind, XmlParseError, XmlParseTree, parse_xml,
};
