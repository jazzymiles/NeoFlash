//! Document model and deterministic authoring/runtime evaluation shared by desktop and web.
mod mask_composition;
mod mask_coverage;
pub use mask_coverage::MaskCoverage;
pub mod accessibility;
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
pub mod as3_air;
pub mod as3_collections;
pub mod as3_fte_3d;
pub mod as3_geom;
pub mod as3_media_events;
pub mod as3_net_sys;
pub mod as3_object;
pub mod as3_sound_media;
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
mod authored_playback;
mod authoring_geometry;
pub mod bitmap_trace;
mod clipboard;
mod demo;
mod evaluate;
mod filters;
mod geometry;
mod history;
mod library_folders;
pub mod merge_drawing;
pub mod model;
pub mod motion_tools;
mod pixels;
mod resource_image;
mod resources;
mod runtime;
mod runtime_graph;
mod runtime_graphics;
mod scenes;
mod shape_tween;
mod text;
mod timeline_edit;
mod timeline_sound;
pub mod timeline_video;
mod validation;
pub mod vector_effects;
mod xml;

pub use accessibility::{
    AccessibilityDiagnostic, AccessibilityLivePoliteness, AccessibilityTreeNode,
    ContentAccessibleRole, DocumentAccessibility, ExtendedAccessibility,
    resolve_frame_accessibility_tree, validate_accessibility_tree,
};
pub use authored_layers::{
    AuthoredDraw, AuthoredMaskGroup, AuthoredScene, evaluate_scene, evaluate_scene_with_buttons,
    layer_effective_blend_mode, layer_effective_outline_color, layer_effectively_locked,
    layer_effectively_synced, layer_effectively_visible, layer_outline_color,
};
pub use authored_playback::AuthoredPlayback;
pub use authoring_geometry::{
    GapCloseResult, StrokeEraseResult, close_path_gaps, erase_path_stroke,
};
pub use clipboard::{
    CLIPBOARD_PREFIX, ClipboardError, ClipboardPayload, MAX_CLIPBOARD_BYTES, PasteResult,
};
pub use evaluate::{
    DrawItem, GuidePath, affine_multiply, camera_layer_matrix, easing, effective_property_ease,
    evaluate, guide_polyline, motion_path_polyline, motion_path_segment, motion_path_waypoints,
    motion_span, placement_start, property_value, sample_layer, sample_layer_at_fps,
    sample_layer_in, sample_layer_in_at_fps, sample_timeline_camera, transform_matrix,
};
pub use filters::{
    FilterError, FilterPadding, FilterSpec, IDENTITY_COLOR_MATRIX, MAX_FILTER_PIXELS,
    MAX_OBJECT_FILTERS, adjust_color_matrix, filter_padding, filter_rgba, validate_filters,
};
pub use geometry::{
    FlattenOptions, FlattenedContour, GeometryError, MAX_PATH_COMMANDS, PathBounds,
    PathFingerprint, apply_asset_warp_to_element, apply_vector_modifier, deform_point_with_pins,
    evaluate_element_appearance, expand_pattern_brush_stroke, expand_variable_width_stroke,
    flatten_path, hit_test_path, invert_affine_matrix, nearest_path_segment, oval_path_commands,
    path_bounds, path_commands_fingerprint, paths_have_same_topology, polystar_path_commands,
    rectangle_path_commands, solve_ik_armature, subdivide_path_at, validate_path_commands,
};
pub use history::{
    AuthoringCommandStep, History, MAX_COMMAND_STEPS, SavedCommand, apply_command_steps_to_elements,
};
pub use library_folders::valid_library_folder;
pub use merge_drawing::{
    BooleanOp, BrushPaintMode, apply_brush_stroke_with_mode, apply_merge_shape_to_keyframe,
    boolean_paths, boolean_rings, combine_elements_boolean, decompose_element_to_merge_parts,
    element_world_commands, erase_merge_geometry_with_sweep, flatten_closed_rings,
    flatten_stroke_polylines, is_merge_fill, is_merge_stroke, lasso_split_merge_fills,
    merge_connected_stroke_ids, merge_fill_adjacent_stroke_ids, paint_bucket_planar_face,
    point_in_rings, remerge_moved_elements_in_keyframe, slice_fill_rings_with_polylines,
    slice_fill_with_stroke, split_bezier_path_with_segments, split_merge_geometry_by_polygon,
    split_polyline_with_segments, sub_arc_bezier_command, trim_polyline_inside_fill,
};
pub use model::*;
pub use motion_tools::{
    MotionSketchSample, PreviewWorkArea, PropertyExpression, StageSnapshotAB, TimelineMarker,
    WigglerConfig, WigglerDimension, WigglerNoiseKind, apply_keyframe_property_expressions,
    apply_wiggler_to_keyframes, capture_motion_sketch_to_keyframes, deterministic_smooth_noise,
    evaluate_property_expression, smooth_motion_keyframes,
};
pub use pixels::{
    As2BitmapCommand, BitmapError, BitmapRect, MAX_BITMAP_BYTES, MAX_BITMAP_DIMENSION,
    PixelDescriptor, PixelRef, PixelSnapping, PixelWrite,
};
pub use resource_image::DecodedImage;
pub use resources::{
    BitmapCompression, BitmapExportSettings, MAX_PROJECT_JSON_BYTES, MAX_RESOURCE_BYTES,
    MAX_RESOURCE_TOTAL_BYTES, MAX_RESOURCES, ResourceError, ResourceUnit, SoundCompression,
    SoundExportQuality, SoundExportSettings, VALID_AUDIO_BIT_RATES_KBPS, VALID_JPEG_QUALITY_RANGE,
};
pub use runtime::{
    AccessibleNode, DisplayNode, FrameLabel, PlaybackCommand, Player, ScriptDispatch, ScriptEvent,
    TimelineMetadata,
};
pub use runtime_graph::{
    AlphaClipComponent, As2AccProps, As2AccUpdateEntry, ClipComponent, ColorTransform,
    DisplayError, DisplayEvent, DisplayInputState, DisplayMutation, DynamicKind, LoaderMutation,
    MAX_DYNAMIC_OBJECTS, MaskResourceBitmap, MutationResult, ObjectRef, RuntimeAccessibleNode,
    RuntimeAlphaMask, RuntimeClip, RuntimeDrawItem, RuntimeDrawKind, RuntimeFilterGroup,
    RuntimeObject, RuntimeScriptDispatch, RuntimeSnapshot, ScrollRect, StageMutation, VideoState,
};
pub use runtime_graphics::{
    BitmapFillSpec, BitmapPaint, GradientKind, GradientPaint, GradientSpec, GradientStop,
    GraphicsOp, MAX_GRAPHICS_COMMANDS, MAX_GRAPHICS_OPS,
};
pub use scenes::{
    GotoFrame, GotoRequest, SceneMetadata, SceneRange, flatten_scenes, flatten_scenes_with_ranges,
    resolve_goto, scene_ranges, scene_step,
};
pub use shape_tween::{
    MAX_SHAPE_TWEEN_SEGMENTS, shape_tween_elements, shape_tween_elements_with_hints,
};
pub use text::{
    AuthoredTextFieldOptions, MAX_TEXT_BYTES, TextAlign, TextBoxSize, TextCharBox, TextError,
    TextFieldState, TextFieldType, TextFormat, TextFormatPatch, TextFormatRangePatch,
    TextFormatRun, TextInputEdit, TextLayout, TextLine, TextPatch, TextQuery, TextReplacement,
    TextRestrictPatch, TextSelection, layout_char_boxes, layout_text, query_text,
};
pub use timeline_edit::{
    FrameClipboard, FrameLayerClip, FrameRange, TimelineEdit, copy_frames, edit_frames,
    sync_layer_symbols,
};
pub use timeline_sound::{
    MAX_WAVEFORM_BUCKETS, ScrubSoundSlice, SoundCommand, WaveformPeaks, compute_lip_sync_keyframes,
    envelope_gains, sample_waveform_peaks, scrub_sound_slices, sound_span_end,
    timeline_sound_resources, wav_waveform_peaks,
};
pub use timeline_video::{
    TimelineVideoSpan, VideoCodecDiagnostic, VideoCommand, VideoCuePoint, VideoCuePointKind,
    VideoSyncMode, collect_crossed_cue_points, compute_frame_video_time_ms, inspect_video_resource,
};
pub use validation::{ProjectError, ValidationIssue, validate};
pub use vector_effects::{
    ElementVectorEffects, EnvelopeWarp, EnvelopeWarpPreset, FreeformGradient, FreeformGradientLine,
    FreeformGradientPoint, GradientMesh, GradientMeshPatchVertex, PerspectiveGridSpec,
    PerspectivePlane, ProceduralVectorEffect, VectorBlendSpacing, VectorBlendSpec,
    apply_envelope_warp_to_path, apply_envelope_warp_to_point, apply_procedural_effect_to_path,
    expand_vector_effects, generate_vector_blend_steps, sample_freeform_gradient,
};
pub use xml::{
    MAX_XML_ATTRIBUTES, MAX_XML_BYTES, MAX_XML_DEPTH, MAX_XML_ITEMS, MAX_XML_NAME_BYTES,
    MAX_XML_STRING_BYTES, XmlAttribute, XmlDeclaration, XmlErrorKind, XmlName, XmlNode,
    XmlNodeKind, XmlParseError, XmlParseTree, parse_xml,
};

pub use amf::{
    AmfDecoded, AmfError, AmfGraph, AmfNode, AmfValue, MAX_AMF_BYTES, MAX_AMF_JSON_BYTES,
    amf_decode, amf_encode,
};
