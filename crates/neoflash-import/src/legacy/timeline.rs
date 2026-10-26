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
}

/// Every successfully resolved pointer, including nulls, in wire order. A new
/// object can remain incomplete. A rejected shared child pointer is still retained.
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
    pub shape_tween_blend: Option<u32>,
    pub extension: Option<TimelineString>,
    pub reserved: Option<u32>,
    pub sound_effect: Option<u32>,
    pub bookmark: Option<u32>,
    pub single_ease_curve: Option<u32>,
    pub custom_ease_present: Option<u32>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimelineBitmap {
    pub revision: Option<u8>,
    pub matrix_words: Option<[i32; 6]>,
    pub media_id: Option<u16>,
    pub extension_present: Option<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "fields", rename_all = "snake_case")]
pub enum TimelinePayload {
    Page(TimelinePage),
    Layer(TimelineLayer),
    Frame(Box<TimelineFrame>),
    Bitmap(TimelineBitmap),
    Shape,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelineObject {
    pub object_id: u32,
    pub class: TimelineClass,
    pub schema: u16,
    pub introduced_by_pointer: usize,
    /// Only a newly introduced CPicObj child has an owning parent. Referenced
    /// related layers/sound/morph objects must not be treated as owned children.
    pub owner: Option<u32>,
    pub children: Vec<u32>,
    /// Start includes the class/object envelope; end is consumed progress, not
    /// an inferred record ending when complete is false.
    pub encoded: TimelineByteSpan,
    pub body_offset: usize,
    pub complete: bool,
    pub base: Option<TimelineBase>,
    pub payload: Option<TimelinePayload>,
    /// Index into this stream's shapes, including inherited CPicFrame geometry.
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
