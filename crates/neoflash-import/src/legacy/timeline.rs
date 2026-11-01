//! Private opt-in timeline preservation. Archive identity is not placement identity
//! across frames, and structural completion is not editable conversion.
use super::contents::{ContentsByteSpan, ContentsString};
use super::geometry::LocatedBinaryShape;
use serde::{Deserialize, Serialize};

/// Half-open offsets into the containing BinaryTimelineStream.original_bytes.
pub type TimelineByteSpan = ContentsByteSpan;
/// Encoded string spans refer to the containing timeline's original bytes.
pub type TimelineString = ContentsString;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineClass {
    Page,
    Layer,
    Frame,
    Shape,
    Bitmap,
    Text,
    Symbol,
    Sprite,
    Button,
    Morph,
    MorphSegment,
    MorphCurve,
    MorphHint,
    Rect,
    Oval,
    CompiledClip,
    VideoStream,
    Unknown,
}
impl TimelineClass {
    pub(super) fn read(name: &[u8]) -> Self {
        match name {
            b"CPicPage" => Self::Page,
            b"CPicLayer" => Self::Layer,
            b"CPicFrame" => Self::Frame,
            b"CPicShape" => Self::Shape,
            b"CPicBitmap" => Self::Bitmap,
            b"CPicText" => Self::Text,
            b"CPicSymbol" => Self::Symbol,
            b"CPicSprite" => Self::Sprite,
            b"CPicButton" => Self::Button,
            b"CPicMorphShape" => Self::Morph,
            b"CMorphSegment" => Self::MorphSegment,
            b"CMorphCurve" => Self::MorphCurve,
            b"CMorphHintItem" => Self::MorphHint,
            b"CPicRect" => Self::Rect,
            b"CPicOval" => Self::Oval,
            b"CPicSwf" => Self::CompiledClip,
            b"CPicVideoStream" => Self::VideoStream,
            _ => Self::Unknown,
        }
    }
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Page => "CPicPage",
            Self::Layer => "CPicLayer",
            Self::Frame => "CPicFrame",
            Self::Shape => "CPicShape",
            Self::Bitmap => "CPicBitmap",
            Self::Text => "CPicText",
            Self::Symbol => "CPicSymbol",
            Self::Sprite => "CPicSprite",
            Self::Button => "CPicButton",
            Self::Morph => "CPicMorphShape",
            Self::MorphSegment => "CMorphSegment",
            Self::MorphCurve => "CMorphCurve",
            Self::MorphHint => "CMorphHintItem",
            Self::Rect => "CPicRect",
            Self::Oval => "CPicOval",
            Self::CompiledClip => "CPicSwf",
            Self::VideoStream => "CPicVideoStream",
            Self::Unknown => "Unknown",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimelineReference {
    Null,
    Class {
        class: TimelineClass,
        schema: u16,
        encoded_name: TimelineByteSpan,
    },
    Object {
        object_index: usize,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineRelation {
    Root,
    Child,
    RelatedLayer,
    LegacyRelatedLayer,
    MorphShape,
    SoundSettings,
    /// Ordered CMorphSegment pointer of a CPicMorphShape. Null entries are values.
    MorphSegment,
    /// Ordered CMorphHintItem pointer of a CPicMorphShape.
    MorphHint,
    /// Ordered CMorphCurve pointer of a CMorphSegment.
    MorphCurve,
}

/// Every successfully resolved pointer, including nulls, in wire order. A new
/// object can remain incomplete. Shared child targets reuse the original identity;
/// a rejected cyclic child pointer is still retained.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelinePointer {
    pub source: Option<u32>,
    pub relation: TimelineRelation,
    pub target: Option<u32>,
    pub is_reference: bool,
    /// Pointer envelope only, excluding any newly serialized object's body.
    pub encoded: TimelineByteSpan,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineBase {
    pub revision: Option<u8>,
    pub flags: Option<u8>,
    pub children_complete: bool,
    pub registration_words: Option<[i32; 2]>,
    pub drawing_object_flags: Option<u8>,
    pub cache_as_bitmap: Option<u8>,
    #[serde(default)]
    pub identifier: Option<TimelineByteSpan>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelinePage {
    pub revision: Option<u8>,
    pub next_layer_id: Option<u16>,
    pub next_folder_id: Option<u16>,
    pub current_frame: Option<u32>,
    pub guide_count: Option<u32>,
    pub guides: Option<TimelineByteSpan>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineLayer {
    pub revision: Option<u8>,
    pub name: Option<TimelineString>,
    pub state_flags: Option<TimelineByteSpan>,
    pub height_literal: Option<u32>,
    pub outline_color_word: Option<u32>,
    pub outline: Option<u32>,
    pub height_multiplier: Option<u32>,
    pub layer_type: Option<u8>,
    pub legacy_flag: Option<u8>,
    pub mask_or_open: Option<u8>,
    pub auto_named: Option<u8>,
    #[serde(default)]
    pub animation_type: Option<u8>,
    #[serde(default)]
    pub animation_values: Vec<u32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Presence established by a complete source string or command count. A known
/// encoding mode alone does not imply nonempty code; unread data stays Unknown.
pub enum TimelineActionPresence {
    Unknown,
    Empty,
    Source,
    Compiled,
    Historical,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelineActions {
    pub encoded: TimelineByteSpan,
    pub complete: bool,
    pub presence: TimelineActionPresence,
    pub revision: Option<u32>,
    pub mode: Option<u32>,
    pub identifier: Option<u32>,
    pub breakpoint_count: Option<u32>,
    pub breakpoints: Option<TimelineByteSpan>,
    pub collapse_word_count: Option<u32>,
    pub collapse_words: Option<TimelineByteSpan>,
    pub source: Option<TimelineString>,
    pub compiled_revision: Option<u32>,
    /// Raw count word. Compiled lists encode it as signed; negative values stay
    /// unsupported with Unknown presence, rather than asserting nonempty code.
    pub command_count: Option<u32>,
    /// Typed historical (Flash 2–4) or compiled-mode (Flash 5/MX) authoring
    /// commands, retained even when a later command stops decoding.
    #[serde(default)]
    pub decoded: Option<TimelineDecodedActions>,
}
/// Container that held the typed commands. These are Actions-panel authoring
/// commands, not SWF bytecode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineActionForm {
    /// One inline historical command (frame revisions 4–9, button revision 2).
    HistoricalCommand,
    /// Counted historical commands (frame revisions 10–18, button revisions 3–4).
    HistoricalList,
    /// Compiled-mode command list inside an action-source container (mode 0).
    Compiled,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TimelineActionValue {
    Byte(u8),
    Word(u16),
    Signed(i32),
    Dword(u32),
    String(TimelineString),
    /// Compiled-mode parameter: text plus flags (0 = literal string, else expression).
    Parameter {
        text: TimelineString,
        flags: u32,
    },
    Bytes(TimelineByteSpan),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelineActionField {
    pub name: String,
    pub value: TimelineActionValue,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelineActionCommand {
    /// Exact command bytes; `end` is consumed progress when `complete` is false.
    pub encoded: TimelineByteSpan,
    pub complete: bool,
    /// Historical command kind byte or compiled command type word.
    pub kind: u32,
    pub revision: Option<u32>,
    /// Compiled lists with revision 0 omit the bodies of simple commands.
    pub body_present: bool,
    /// Fields in archive order.
    pub fields: Vec<TimelineActionField>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelineDecodedActions {
    pub form: TimelineActionForm,
    pub list_revision: Option<u32>,
    pub commands: Vec<TimelineActionCommand>,
    /// Importer-generated ActionScript 1 translation; empty while decoding is
    /// incomplete. Never executed by the importer.
    pub source: String,
    /// Commands kept only as marked `// NeoFlash: untranslated` comments in `source`.
    pub untranslated: Vec<usize>,
    /// True when the translation is incomplete: untranslated commands, block
    /// repairs or text that needs an explicit code page.
    pub needs_review: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineFrame {
    pub revision: Option<u8>,
    pub duration: Option<u16>,
    pub key_mode: Option<u16>,
    pub acceleration: Option<u16>,
    pub sound_media_id: Option<u16>,
    pub sound_envelope_count: Option<u16>,
    pub sound_envelope: Option<TimelineByteSpan>,
    pub sound_loop_count: Option<u16>,
    pub sound_sync: Option<u8>,
    pub sound_in_point: Option<u32>,
    pub sound_out_point: Option<u32>,
    pub sound_zoom: Option<u16>,
    pub label: Option<TimelineString>,
    pub actions: Option<TimelineActions>,
    pub rotation_mode: Option<u32>,
    pub rotation_count: Option<u32>,
    pub label_type: Option<u32>,
    /// Shape-tween mapping (XFL names): 0 distributive, 1 angular vertex,
    /// 2 random distribution, 3 linear walk.
    pub shape_tween_blend: Option<u32>,
    /// CPicMorphShape object ID of a shape-tween keyframe (revision >= 13);
    /// `None` for a null pointer or when absent.
    #[serde(default)]
    pub morph_shape: Option<u32>,
    pub extension: Option<TimelineString>,
    pub reserved: Option<u32>,
    pub sound_effect: Option<u32>,
    pub bookmark: Option<u32>,
    pub single_ease_curve: Option<u32>,
    pub custom_ease_present: Option<u32>,
    /// Six target curves: position, rotation, scale, color, filters, all.
    /// Each knot retains incoming handle, anchor, outgoing handle as x/y pairs.
    #[serde(default)]
    pub custom_ease: Vec<Vec<[f64; 6]>>,
    #[serde(default)]
    pub motion_xml: Option<TimelineString>,
    #[serde(default)]
    pub visible_animation_keyframes: Option<u32>,
    #[serde(default)]
    pub tween_instance_name: Option<TimelineString>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineColorTransform {
    pub revision: Option<u8>,
    /// Signed archive multipliers, in RGBA order; divisor is 256.
    pub multipliers: [i16; 4],
    pub offsets: [i16; 4],
    pub effect_type: Option<u16>,
    pub effect_percent: Option<i16>,
    pub effect_rgba: Option<u32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineAccessibility {
    pub revision: u32,
    pub flags: Option<u32>,
    pub name: Option<TimelineString>,
    pub description: Option<TimelineString>,
    pub shortcut: Option<TimelineString>,
    pub tab_index: Option<TimelineString>,
    pub reserved: Option<TimelineString>,
    pub force_simple: Option<u32>,
    pub auto_labeling: Option<u32>,
}

/// Component instance parameters, in archive order. Revision 0 lists carry a
/// legacy type word and a single value string per entry.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineComponentProperties {
    pub encoded: TimelineByteSpan,
    pub revision: u8,
    pub entries: Vec<TimelineComponentProperty>,
    pub parameters_are_new: Option<u32>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineComponentProperty {
    pub encoded: TimelineByteSpan,
    pub revision: Option<u8>,
    pub name: Option<TimelineString>,
    pub legacy_type: Option<u32>,
    pub legacy_value: Option<TimelineString>,
    pub value: Option<TimelinePropertyValue>,
    pub variable_name: Option<TimelineString>,
    /// Identifier revision byte plus 16 identifier bytes.
    pub identifier: Option<TimelineByteSpan>,
    pub category: Option<TimelineString>,
    pub verbosity: Option<u32>,
}
/// Typed parameter value: `value_type` is the archive type word (13 is a
/// table, whose exact span is retained in `table`). List-like values keep
/// every entry and the selected index.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelinePropertyValue {
    pub encoded: TimelineByteSpan,
    pub revision: u8,
    pub value_type: u16,
    pub selection_word: Option<u16>,
    pub selected_index: Option<u32>,
    pub named_entries: Option<u32>,
    pub entries: Vec<TimelinePropertyValueEntry>,
    pub table: Option<TimelineByteSpan>,
    pub data: Option<TimelineByteSpan>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelinePropertyValueEntry {
    pub name: Option<TimelineString>,
    pub value: TimelineString,
    pub value_type: Option<u16>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineInstance {
    pub revision: Option<u8>,
    pub matrix_words: Option<[i32; 6]>,
    pub first_frame: Option<u16>,
    pub loop_mode: Option<u16>,
    pub color_transform: Option<TimelineColorTransform>,
    pub legacy_name: Option<TimelineString>,
    pub library_id: Option<u32>,
    pub is_extensible: Option<u8>,
    /// Instance graphic filters in authored (application) order. Empty when the
    /// revision has no filter field or its presence byte is zero.
    #[serde(default)]
    pub filters: Vec<TimelineFilter>,
    /// Filter presence byte through the end of the last filter (revision >= 15).
    #[serde(default)]
    pub filters_encoded: Option<TimelineByteSpan>,
    /// Raw blend byte (revision >= 16); see [`TimelineBlendMode::from_archive`].
    pub blend_mode: Option<u8>,
    pub selected_effect_index: Option<u16>,
    pub matrix_3d: Option<[f32; 16]>,
    pub rotation_3d: Option<[f64; 3]>,
    pub center_point_3d: Option<[i32; 3]>,
    pub animation_frame_settings: Vec<[u16; 2]>,
    pub extension_revision: Option<u8>,
    pub instance_name: Option<TimelineString>,
    pub actions: Option<TimelineActions>,
    pub track_as_menu: Option<u8>,
    pub property_count: Option<u32>,
    #[serde(default)]
    pub properties: Option<TimelineComponentProperties>,
    pub property_inspector_state: Option<u32>,
    pub accessibility: Option<TimelineAccessibility>,
    pub component_xml: Option<TimelineString>,
    /// Exact known metadata blocks, retained independently of their semantics.
    pub metadata: Vec<TimelineByteSpan>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineTextStyle {
    pub revision: u8,
    pub font_size_twips: Option<u16>,
    pub font_family: Option<TimelineString>,
    pub font_face: Option<TimelineString>,
    pub font_flags: Option<u32>,
    pub color_rgba: Option<u32>,
    pub pitch_and_family: Option<u8>,
    pub char_set: Option<u8>,
    pub bold: Option<u8>,
    pub italic: Option<u8>,
    pub use_screen_spacing: Option<u8>,
    pub auto_kern: Option<u8>,
    pub character_position: Option<u8>,
    pub alignment: Option<u8>,
    pub line_spacing: Option<i16>,
    pub indent: Option<i16>,
    pub left_margin: Option<i16>,
    pub right_margin: Option<i16>,
    pub letter_spacing: Option<i16>,
    pub url: Option<TimelineString>,
    pub vertical: Option<u8>,
    pub right_to_left: Option<u8>,
    pub rotation: Option<u8>,
    pub bitmap_rendering: Option<u8>,
    pub target: Option<TimelineString>,
    pub anti_alias: Option<TimelineByteSpan>,
    pub url_override: Option<TimelineString>,
    pub legacy_font_name: Option<TimelineString>,
    pub legacy_font_value: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelineTextRun {
    pub characters: TimelineString,
    pub style: TimelineTextStyle,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineText {
    pub revision: Option<u8>,
    pub matrix_words: Option<[i32; 6]>,
    pub bounds_words: Option<[i32; 4]>,
    pub auto_expand: Option<u8>,
    pub reserved: Option<u8>,
    pub flags: Option<u32>,
    pub max_characters: Option<u16>,
    pub variable_name: Option<TimelineString>,
    pub font_library_name: Option<TimelineString>,
    pub embed_variant_glyphs: Option<u8>,
    pub default_style: Option<TimelineTextStyle>,
    pub runs: Vec<TimelineTextRun>,
    pub instance_name: Option<TimelineString>,
    pub accessibility: Option<TimelineAccessibility>,
    pub scrollable: Option<u32>,
    pub localization_id: Option<TimelineString>,
    pub embed_ranges: Option<TimelineString>,
    /// Classic-text graphic filters (text revision >= 13), in authored order.
    #[serde(default)]
    pub filters: Vec<TimelineFilter>,
    #[serde(default)]
    pub filters_encoded: Option<TimelineByteSpan>,
    pub selected_effect_index: Option<u16>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineBitmap {
    pub revision: Option<u8>,
    pub matrix_words: Option<[i32; 6]>,
    pub media_id: Option<u16>,
    pub extension_present: Option<u8>,
}

/// Instance blend modes. Archive byte 0 and 1 are both normal; values above 14
/// are unknown and remain raw in [`TimelineInstance::blend_mode`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineBlendMode {
    Normal,
    Layer,
    Multiply,
    Screen,
    Lighten,
    Darken,
    Difference,
    Add,
    Subtract,
    Invert,
    Alpha,
    Erase,
    Overlay,
    Hardlight,
}
impl TimelineBlendMode {
    pub fn from_archive(value: u8) -> Option<Self> {
        Some(match value {
            0 | 1 => Self::Normal,
            2 => Self::Layer,
            3 => Self::Multiply,
            4 => Self::Screen,
            5 => Self::Lighten,
            6 => Self::Darken,
            7 => Self::Difference,
            8 => Self::Add,
            9 => Self::Subtract,
            10 => Self::Invert,
            11 => Self::Alpha,
            12 => Self::Erase,
            13 => Self::Overlay,
            14 => Self::Hardlight,
            _ => return None,
        })
    }
}
impl TimelineInstance {
    /// Typed blend mode; `None` when absent from this revision or unknown.
    pub fn blend(&self) -> Option<TimelineBlendMode> {
        self.blend_mode.and_then(TimelineBlendMode::from_archive)
    }
}

/// Authoring filter types. The archive type byte is retained in
/// [`TimelineFilter::type_code`]; 5 (convolution) and unknown codes stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineFilterKind {
    DropShadow,
    Blur,
    Glow,
    Bevel,
    GradientGlow,
    AdjustColor,
    GradientBevel,
}

/// One gradient-glow/bevel ramp entry: ratio 0..=255 and packed color
/// (low byte R, then G, B, A).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineFilterGradientStop {
    pub ratio: u32,
    pub color_rgba: u32,
}

/// Archive filter values. Fields are present only when the filter's revision
/// stores them. Colors pack low byte R, then G, B, A. `blur_x`/`blur_y` and
/// `distance` are pixels; `angle_radians` is radians; `strength_percent` is
/// percent (100 = 1.0); `quality` is the pass count (1 low, 2 medium, 3 high).
/// Booleans are nonzero archive words. Bevel/gradient type: `full` nonzero is
/// full, otherwise `inner` nonzero is inner, otherwise outer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TimelineFilterParams {
    /// Revision of the nested shared shadow-settings block, when present.
    pub settings_revision: Option<u8>,
    /// Revision of the nested enabled-state block, when present.
    pub state_revision: Option<u8>,
    pub enabled_word: Option<u32>,
    /// Shadow/glow color; bevel shadow color.
    pub color_rgba: Option<u32>,
    pub highlight_color_rgba: Option<u32>,
    /// Unused color word of revision-1 blur records.
    pub legacy_color_rgba: Option<u32>,
    pub blur_x: Option<f32>,
    pub blur_y: Option<f32>,
    pub distance: Option<f32>,
    pub angle_radians: Option<f32>,
    pub strength_percent: Option<u32>,
    pub quality: Option<u32>,
    pub inner: Option<u32>,
    pub knockout: Option<u32>,
    pub hide_object: Option<u32>,
    pub full: Option<u32>,
    /// Revision-2 bevel compatibility fields with no known visual meaning.
    pub legacy_byte: Option<u8>,
    pub legacy_word: Option<u32>,
    /// Adjust-color values in authoring units: brightness/contrast/saturation
    /// -100..=100, hue -180..=180 degrees.
    pub brightness: Option<f32>,
    pub contrast: Option<f32>,
    pub saturation: Option<f32>,
    pub hue: Option<f32>,
    /// Revision of the gradient block (gradient glow/bevel).
    pub gradient_revision: Option<u8>,
    /// Raw gradient word preceding `full`; semantics unresolved.
    pub gradient_value: Option<u32>,
    #[serde(default)]
    pub gradient_stops: Vec<TimelineFilterGradientStop>,
}

/// One authored filter. A revision-0 record stores no settings at all.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimelineFilter {
    /// Type byte through the end of this filter.
    pub encoded: TimelineByteSpan,
    pub type_code: u8,
    pub kind: TimelineFilterKind,
    pub revision: u8,
    /// Decoded enabled state; `None` when this revision stores none (enabled).
    pub enabled: Option<bool>,
    pub params: TimelineFilterParams,
}

/// CPicMorphShape: shape-tween correspondence between a start shape (A) and
/// end shape (B). It is not a CPicObj; it is reached from a CPicFrame morph
/// pointer. Matrix words are a,b,c,d signed 16.16 and tx,ty twips.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineMorphShape {
    pub revision: Option<u8>,
    pub matrix_words_start: Option<[i32; 6]>,
    pub matrix_words_end: Option<[i32; 6]>,
    /// Revision >= 2 raw word with unresolved meaning.
    pub extension_value: Option<u32>,
    /// Revision >= 2 signed easing, nominally -100..=100.
    pub acceleration: Option<i32>,
    /// Ordered CMorphSegment object IDs; `None` retains a null pointer entry.
    pub segments: Vec<Option<u32>>,
    /// Ordered CMorphHintItem object IDs; `None` retains a null pointer entry.
    pub hints: Vec<Option<u32>>,
    /// Shared style tables referenced by segment indices (zero-based, -1 none).
    /// Encoded like revision-0 extended shape styles: no stroke extensions.
    pub fills: Vec<super::geometry::BinaryFill>,
    pub strokes: Vec<super::geometry::BinaryStroke>,
}
/// CMorphSegment: one contour, with style indices named as in XFL
/// `MorphSegment` (zero-based into the morph style tables; -1 means none).
/// Points are integer twips; A is the start shape, B the end shape.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineMorphSegment {
    pub stroke_index_1: Option<i32>,
    pub stroke_index_2: Option<i32>,
    pub fill_index_1: Option<i32>,
    pub fill_index_2: Option<i32>,
    pub start_point_a: Option<[i32; 2]>,
    pub start_point_b: Option<[i32; 2]>,
    /// Ordered CMorphCurve object IDs; `None` retains a null pointer entry.
    pub curves: Vec<Option<u32>>,
}
/// CMorphCurve: one quadratic step from the previous anchor, in integer twips.
/// A nonzero `line_flags` dword marks a straight segment.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineMorphCurve {
    pub control_point_a: Option<[i32; 2]>,
    pub anchor_point_a: Option<[i32; 2]>,
    pub control_point_b: Option<[i32; 2]>,
    pub anchor_point_b: Option<[i32; 2]>,
    pub line_flags: Option<u32>,
}
/// CMorphHintItem: shape hint correspondence in integer twips; `index` is the
/// hint key (0 = "a").
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineMorphHint {
    pub point_a: Option<[i32; 2]>,
    pub point_b: Option<[i32; 2]>,
    pub index: Option<u32>,
}

/// Rectangle/oval primitive fields following the inherited CPicShape body
/// (whose drawn edges are in the object's `shape_index`). Bit 0 of `flags` is
/// the rectangle lock / oval close-path flag; other bits are unresolved.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelinePrimitive {
    pub revision: Option<u8>,
    pub flags: Option<u8>,
    /// Rectangle corner radii in twips: top-left, top-right, bottom-left,
    /// bottom-right.
    pub corner_radii: Option<[i32; 4]>,
    /// Oval bounds in twips, `xmin,xmax,ymin,ymax` order.
    pub bounds_words: Option<[i32; 4]>,
    pub start_angle_degrees: Option<f32>,
    pub end_angle_degrees: Option<f32>,
    pub inner_radius_percent: Option<f32>,
}
/// CPicSwf: placement of a compiled-clip library item (`media_id` names its
/// Contents media descriptor). Actions use the frame source-action encoding.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineCompiledClip {
    pub revision: Option<u8>,
    pub matrix_words: Option<[i32; 6]>,
    pub media_present: Option<u32>,
    pub media_id: Option<u16>,
    pub actions: Option<TimelineActions>,
    pub instance_name: Option<TimelineString>,
    pub property_count: Option<u32>,
    pub properties: Option<TimelineComponentProperties>,
    pub accessibility: Option<TimelineAccessibility>,
    pub component_xml: Option<TimelineString>,
    pub reserved_byte: Option<u8>,
    pub metadata: Vec<TimelineByteSpan>,
}
/// CPicVideoStream: embedded/linked video placement. Bounds are twips in
/// `xmin,xmax,ymin,ymax` order. `media_id` is absent when `media_present` is 0.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineVideo {
    pub revision: Option<u8>,
    pub matrix_words: Option<[i32; 6]>,
    pub bounds_words: Option<[i32; 4]>,
    pub reserved_byte: Option<u8>,
    pub reserved_string: Option<TimelineString>,
    pub instance_name: Option<TimelineString>,
    pub media_present: Option<u32>,
    pub media_id: Option<u16>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "fields", rename_all = "snake_case")]
pub enum TimelinePayload {
    Page(TimelinePage),
    Layer(TimelineLayer),
    Frame(Box<TimelineFrame>),
    Bitmap(TimelineBitmap),
    Instance(Box<TimelineInstance>),
    Text(Box<TimelineText>),
    Shape,
    Morph(Box<TimelineMorphShape>),
    MorphSegment(TimelineMorphSegment),
    MorphCurve(TimelineMorphCurve),
    MorphHint(TimelineMorphHint),
    Primitive(Box<TimelinePrimitive>),
    CompiledClip(Box<TimelineCompiledClip>),
    Video(Box<TimelineVideo>),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelineObject {
    pub object_id: u32,
    pub class: TimelineClass,
    pub schema: u16,
    pub introduced_by_pointer: usize,
    /// Parent at first serialization, only when introduced as a CPicObj child.
    /// Ordered children and pointers also retain subsequent child references to
    /// objects first serialized through related-layer or other metadata links.
    pub owner: Option<u32>,
    pub children: Vec<u32>,
    /// Start includes the class/object envelope; end is consumed progress, not
    /// an inferred record ending when complete is false.
    pub encoded: TimelineByteSpan,
    pub body_offset: usize,
    pub complete: bool,
    pub base: Option<TimelineBase>,
    pub payload: Option<TimelinePayload>,
    /// Index into this stream's shapes, including inherited CPicFrame and
    /// CPicRect/CPicOval geometry.
    pub shape_index: Option<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BinaryTimelineStream {
    pub index: usize,
    pub stream_kind: Option<u8>,
    pub root: Option<u32>,
    pub references: Vec<TimelineReference>,
    pub pointers: Vec<TimelinePointer>,
    pub objects: Vec<TimelineObject>,
    pub shapes: Vec<LocatedBinaryShape>,
    pub original_bytes: Vec<u8>,
}
