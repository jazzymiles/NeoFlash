//! Thin JavaScript bindings around the same Rust timeline runtime used by the editor.
mod as2_binding;
mod as2_boxing;
mod as2_capture;
mod as2_connections;
mod as2_data;
mod as2_depth_manager;
mod as2_display;
mod as2_files;
mod as2_locale;
mod as2_media;
mod as2_object;
mod as2_platform;
mod as2_services;
mod as2_sound;
mod as2_stage;
mod as2_strings;
mod as2_text;
mod as2_transitions;
mod as2_variables;
mod as2d_geom;
mod as3_air;
mod as3_collections;
mod as3_fte_3d;
mod as3_geom;
mod as3_media_events;
mod as3_net_sys;
mod as3_object;
mod as3_strings;
mod as_binary;
mod as_broadcaster;
mod as_date;
mod as_easing;
mod as_events;
mod as_math;
mod as_number_format;
mod as_timers;
pub mod swf_loader;

pub use as2_strings::{as2_string_json, legacy_case_name_json};
pub use as3_air::AirKernel;
pub use as3_fte_3d::As3Fte3dKernel;
pub use as3_media_events::As3MediaEventsKernel;
pub use as3_net_sys::As3NetSysKernel;
pub use swf_loader::decode_swf_bundle_json;

use neoflash_core::{
    ColorTransform, DisplayMutation, DynamicKind, GotoRequest, GraphicsOp, ObjectRef, PixelRef,
    PixelSnapping, PlaybackCommand, Player, Project, RuntimeScriptDispatch, SceneRange,
    ScriptDispatch, ScrollRect, StageMutation,
};
use neoflash_core::{
    TextError, TextFieldState, TextInputEdit, TextPatch, TextQuery, layout_text, query_text,
};
use wasm_bindgen::prelude::*;

/// Pure, bounded XML fragment parsing. No Player borrow or host callback occurs.
#[wasm_bindgen]
pub fn parse_xml_json(source: &str) -> String {
    match neoflash_core::parse_xml(source) {
        Ok(tree) => serde_json::json!({ "ok": tree }).to_string(),
        Err(error) => serde_json::json!({ "error": error }).to_string(),
    }
}

#[wasm_bindgen]
pub fn parse_as2_xml_json(source: &str) -> String {
    match neoflash_core::as2_xml::parse_as2_xml(source) {
        Ok(tree) => serde_json::json!({"ok": tree}).to_string(),
        Err(error) => serde_json::json!({"error": error.to_string()}).to_string(),
    }
}
#[wasm_bindgen]
pub fn as2_xml_namespace_json(
    ancestors_json: &str,
    value: &str,
    reverse: bool,
) -> Result<String, JsValue> {
    neoflash_core::as2_xml::namespace_lookup_json(ancestors_json, value, reverse)
        .map_err(display_error)
}
#[wasm_bindgen]
pub fn as2_property_name(index: u32) -> Result<String, JsValue> {
    neoflash_core::as2_strings::property_name(index)
        .map(str::to_owned)
        .ok_or_else(|| JsValue::from_str("AS2 property index must be between 0 and 21."))
}

#[wasm_bindgen]
pub fn frame_label_json(name: &str, frame: i32) -> Result<String, JsValue> {
    serde_json::to_string(&neoflash_core::FrameLabel::new(name.to_owned(), frame))
        .map_err(display_error)
}

/// Pure AMF codecs; graph JSON is a transport IR, never the persisted encoding.
#[wasm_bindgen]
pub fn amf_encode(format: u8, graph_json: &str) -> Result<Vec<u8>, JsValue> {
    if graph_json.len() > neoflash_core::MAX_AMF_JSON_BYTES {
        return Err(JsValue::from_str("AMF graph JSON exceeds 64 MiB"));
    }
    let graph: neoflash_core::AmfGraph = serde_json::from_str(graph_json).map_err(display_error)?;
    neoflash_core::amf_encode(format, &graph).map_err(display_error)
}
fn call_js_ext_reader(
    func: &js_sys::Function,
    class_name: &str,
    rem: &[u8],
) -> Result<(usize, Option<usize>), String> {
    let slice = js_sys::Uint8Array::from(rem);
    let out = func
        .call2(&JsValue::NULL, &JsValue::from_str(class_name), &slice)
        .map_err(|err| {
            err.as_string()
                .unwrap_or_else(|| "externalizable readExternal failed".into())
        })?;
    let raw = out
        .as_string()
        .ok_or_else(|| "externalizable reader must return JSON string".to_owned())?;
    let v: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("invalid externalizable response: {e}"))?;
    let consumed = v["consumed"]
        .as_u64()
        .ok_or_else(|| "externalizable response missing consumed count".to_owned())?
        as usize;
    let token = v["token"].as_u64().map(|t| t as usize);
    Ok((consumed, token))
}

#[wasm_bindgen]
pub fn amf_decode(format: u8, bytes: &[u8]) -> Result<String, JsValue> {
    serde_json::to_string(&neoflash_core::amf_decode(format, bytes).map_err(display_error)?)
        .map_err(display_error)
}
#[wasm_bindgen]
pub fn amf_decode_ext(
    format: u8,
    bytes: &[u8],
    ext_reader: Option<js_sys::Function>,
) -> Result<String, JsValue> {
    let decoded = if let Some(func) = ext_reader {
        let mut cb = |cls: &str, rem: &[u8]| call_js_ext_reader(&func, cls, rem);
        neoflash_core::AmfDecoded::decode_with_ext(format, bytes, Some(&mut cb))
            .map_err(display_error)?
    } else {
        neoflash_core::amf_decode(format, bytes).map_err(display_error)?
    };
    serde_json::to_string(&decoded).map_err(display_error)
}
#[wasm_bindgen]
pub fn sol_encode(format: u8, name: &str, graph_json: &str) -> Result<Vec<u8>, JsValue> {
    if graph_json.len() > neoflash_core::MAX_AMF_JSON_BYTES {
        return Err(JsValue::from_str("AMF graph JSON exceeds 64 MiB"));
    }
    let graph: neoflash_core::AmfGraph = serde_json::from_str(graph_json).map_err(display_error)?;
    graph.encode_sol(name, format).map_err(display_error)
}
#[wasm_bindgen]
pub fn sol_decode(bytes: &[u8]) -> Result<String, JsValue> {
    let (name, format, decoded) =
        neoflash_core::AmfDecoded::decode_sol(bytes).map_err(display_error)?;
    serde_json::to_string(&serde_json::json!({
        "name": name,
        "format": format,
        "decoded": decoded,
    }))
    .map_err(display_error)
}
#[wasm_bindgen]
pub fn sol_decode_ext(
    bytes: &[u8],
    ext_reader: Option<js_sys::Function>,
) -> Result<String, JsValue> {
    let (name, format, decoded) = if let Some(func) = ext_reader {
        let mut cb = |cls: &str, rem: &[u8]| call_js_ext_reader(&func, cls, rem);
        neoflash_core::AmfDecoded::decode_sol_with_ext(bytes, Some(&mut cb))
            .map_err(display_error)?
    } else {
        neoflash_core::AmfDecoded::decode_sol(bytes).map_err(display_error)?
    };
    serde_json::to_string(&serde_json::json!({
        "name": name,
        "format": format,
        "decoded": decoded,
    }))
    .map_err(display_error)
}
#[wasm_bindgen]
pub fn portable_measure_text_json(
    strings_json: &str,
    format_json: &str,
) -> Result<String, JsValue> {
    if strings_json.len() > 8 * 1024 * 1024 || format_json.len() > 64 * 1024 {
        return Err(JsValue::from_str(
            "Portable text measurement input exceeds size budget",
        ));
    }
    let strings: Vec<String> = serde_json::from_str(strings_json).map_err(display_error)?;
    let format: neoflash_core::TextFormat =
        serde_json::from_str(format_json).map_err(display_error)?;
    serde_json::to_string(&neoflash_core::TextFormat::portable_measure_strings(
        &strings, &format, None,
    ))
    .map_err(display_error)
}
#[wasm_bindgen]
pub fn parse_html_text_json(field_json: &str, html: &str) -> Result<String, JsValue> {
    if field_json.len() > 8 * 1024 * 1024 || html.len() > 8 * 1024 * 1024 {
        return Err(JsValue::from_str("HTML text input exceeds 8 MiB"));
    }
    let mut field: TextFieldState = serde_json::from_str(field_json).map_err(display_error)?;
    let condense = field.condense_white;
    field
        .set_html_text(html, None, condense)
        .map_err(display_error)?;
    serde_json::to_string(&field).map_err(display_error)
}
#[wasm_bindgen]
pub fn serialize_html_text_json(field_json: &str) -> Result<String, JsValue> {
    if field_json.len() > 8 * 1024 * 1024 {
        return Err(JsValue::from_str("Text field state exceeds 8 MiB"));
    }
    let field: TextFieldState = serde_json::from_str(field_json).map_err(display_error)?;
    Ok(field.to_html_text())
}
#[wasm_bindgen]
pub fn text_to_outlines_json(field_json: &str) -> Result<String, JsValue> {
    if field_json.len() > 8 * 1024 * 1024 {
        return Err(JsValue::from_str("Text field state exceeds 8 MiB"));
    }
    let field: TextFieldState = serde_json::from_str(field_json).map_err(display_error)?;
    let commands = field.to_outline_commands(None);
    serde_json::to_string(&commands).map_err(display_error)
}

#[wasm_bindgen]
pub struct WebPlayer {
    project: Project,
    player: Player,
    avi_streams: std::collections::HashMap<u32, (Vec<u8>, neoflash_core::AviIndexedContainer)>,
    next_avi_stream: u32,
}

fn path(value: &str) -> Result<Vec<String>, JsValue> {
    serde_json::from_str(value)
        .map_err(|error| JsValue::from_str(&format!("Invalid display path: {error}")))
}

fn object_ref(value: &str) -> Result<ObjectRef, JsValue> {
    if value.len() > 256 {
        return Err(JsValue::from_str(
            "Display reference exceeds its input limit",
        ));
    }
    serde_json::from_str(value)
        .map_err(|error| JsValue::from_str(&format!("Invalid display reference: {error}")))
}
fn pixel_ref(value: &str) -> Result<PixelRef, JsValue> {
    if value.len() > 256 {
        return Err(JsValue::from_str("Pixel reference exceeds its input limit"));
    }
    serde_json::from_str(value).map_err(display_error)
}
fn display_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}
fn color_transform(value: &str) -> Result<ColorTransform, JsValue> {
    if value.len() > 4096 {
        return Err(JsValue::from_str("ColorTransform input exceeds 4096 bytes"));
    }
    serde_json::from_str(value).map_err(display_error)
}
fn filter_specs(value: &str) -> Result<Vec<neoflash_core::FilterSpec>, JsValue> {
    if value.len() > 65536 {
        return Err(JsValue::from_str("Filter specifications exceed 64 KiB"));
    }
    serde_json::from_str(value).map_err(display_error)
}
fn single_filter(value: &str) -> Result<neoflash_core::FilterSpec, JsValue> {
    if value.len() > 65536 {
        return Err(JsValue::from_str("Filter specification exceeds 64 KiB"));
    }
    serde_json::from_str(value).map_err(display_error)
}
fn goto_request(value: &str) -> Result<GotoRequest, JsValue> {
    if value.len() > 65536 {
        return Err(JsValue::from_str("Goto arguments exceed 64 KiB"));
    }
    serde_json::from_str(value)
        .map_err(|_| JsValue::from_str("Frame must be a frame number or a frame label."))
}
fn goto_result(result: Result<Option<u32>, String>) -> Result<i32, JsValue> {
    result
        .map(|frame| frame.map_or(-1, |frame| frame as i32))
        .map_err(|error| JsValue::from_str(&error))
}
fn command(name: &str, frame: u32) -> Result<PlaybackCommand, JsValue> {
    match name {
        "play" => Ok(PlaybackCommand::Play),
        "stop" => Ok(PlaybackCommand::Stop),
        "goto_and_play" => Ok(PlaybackCommand::GotoAndPlay(frame)),
        "goto_and_stop" => Ok(PlaybackCommand::GotoAndStop(frame)),
        _ => Err(JsValue::from_str("Unknown timeline control command")),
    }
}

fn invoke_text_measure(
    measure: &js_sys::Function,
    strings: &[String],
    format: &neoflash_core::TextFormat,
    callback_error: &mut Option<JsValue>,
) -> Result<Vec<f64>, TextError> {
    let strings = serde_json::to_string(strings).map_err(|e| TextError(e.to_string()))?;
    let format = serde_json::to_string(format).map_err(|e| TextError(e.to_string()))?;
    let result = measure
        .call2(
            &JsValue::NULL,
            &JsValue::from_str(&strings),
            &JsValue::from_str(&format),
        )
        .map_err(|error| {
            // TextError unwinds the shared core; the bridge rethrows the original JavaScript
            // value afterwards, retaining identity, primitive values and authored stacks.
            *callback_error = Some(error);
            TextError("font measurement callback failed".into())
        })?;
    let json = result
        .as_string()
        .ok_or_else(|| TextError("font measurement must return JSON widths".into()))?;
    if json.len() > 4 * 1024 * 1024 {
        return Err(TextError("font measurement result exceeds 4 MiB".into()));
    }
    serde_json::from_str(&json).map_err(|e| TextError(format!("invalid font measurement: {e}")))
}

fn text_measure_result<T, E: std::fmt::Display>(
    result: Result<T, E>,
    callback_error: Option<JsValue>,
) -> Result<T, JsValue> {
    // Per-invocation storage also keeps nested measurement calls independent. Some(undefined)
    // and Some(null) are failures just as an Error object is; never test their truthiness.
    match callback_error {
        Some(error) => Err(error),
        None => result.map_err(display_error),
    }
}

#[wasm_bindgen]
pub fn text_layout_json(field_json: &str, measure: &js_sys::Function) -> Result<String, JsValue> {
    if field_json.len() > 8 * 1024 * 1024 {
        return Err(JsValue::from_str("Text layout input exceeds 8 MiB"));
    }
    let field: TextFieldState = serde_json::from_str(field_json).map_err(display_error)?;
    let mut callback_error = None;
    let layout = layout_text(&field, |strings, format| {
        invoke_text_measure(measure, strings, format, &mut callback_error)
    });
    let layout = text_measure_result(layout, callback_error)?;
    serde_json::to_string(&layout).map_err(display_error)
}

#[wasm_bindgen]
pub fn text_query_json(
    field_json: &str,
    query_json: &str,
    measure: &js_sys::Function,
) -> Result<String, JsValue> {
    if field_json.len() > 8 * 1024 * 1024 || query_json.len() > 64 * 1024 {
        return Err(JsValue::from_str("Text query input exceeds size budget"));
    }
    let field: TextFieldState = serde_json::from_str(field_json).map_err(display_error)?;
    let query: TextQuery = serde_json::from_str(query_json).map_err(display_error)?;
    let mut callback_error = None;
    let result = query_text(&field, &query, |strings, format| {
        invoke_text_measure(measure, strings, format, &mut callback_error)
    });
    let result = text_measure_result(result, callback_error)?;
    serde_json::to_string(&result).map_err(display_error)
}

#[wasm_bindgen]
impl WebPlayer {
    #[wasm_bindgen]
    pub fn as2_bitmap_load(&mut self, id: &str) -> Result<String, JsValue> {
        let result = self
            .player
            .as2_bitmap_load(&self.project, id)
            .map_err(display_error)?;
        serde_json::to_string(&result).map_err(display_error)
    }
    #[wasm_bindgen]
    pub fn as2_bitmap_command(&mut self, input: &str) -> Result<String, JsValue> {
        if input.len() > 65536 {
            return Err(JsValue::from_str("AS2 bitmap command exceeds 64 KiB"));
        }
        let command: neoflash_core::As2BitmapCommand =
            serde_json::from_str(input).map_err(display_error)?;
        serde_json::to_string(
            &self
                .player
                .as2_bitmap_command(command)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }

    /// Multi-scene documents are flattened into one main timeline here, so
    /// editor previews and published bundles share one runtime path. A
    /// published bundle that is already flattened passes its scene ranges.
    #[wasm_bindgen(constructor)]
    pub fn new(json: &str, scenes: Option<String>) -> Result<WebPlayer, JsValue> {
        Self::construct(Project::from_json(json), scenes)
    }

    /// Construct a `WebPlayer` from a published HTML5 bundle whose Stage bitmaps may be packed
    /// into an external `sprite_sheet.png` + `sprite_sheet.json` hydrated via [`Self::hydrate_sprite_sheet`].
    #[wasm_bindgen]
    pub fn new_published(json: &str, scenes: Option<String>) -> Result<WebPlayer, JsValue> {
        Self::construct(Project::from_published_bundle_json(json), scenes)
    }

    fn construct(
        authored: Result<Project, neoflash_core::ProjectError>,
        scenes: Option<String>,
    ) -> Result<WebPlayer, JsValue> {
        let authored = authored.map_err(|error| JsValue::from_str(&error.to_string()))?;
        let (flat, mut ranges) = neoflash_core::flatten_scenes_cow(&authored);
        let project = match flat {
            std::borrow::Cow::Borrowed(_) => authored,
            std::borrow::Cow::Owned(project) => project,
        };
        if ranges.is_empty()
            && let Some(scenes) = scenes
        {
            if scenes.len() > 1024 * 1024 {
                return Err(JsValue::from_str("Scene metadata exceeds 1 MiB"));
            }
            ranges = serde_json::from_str::<Vec<SceneRange>>(&scenes).map_err(display_error)?;
        }
        let mut player = Player::new(&project);
        if let Some(error) = player.bitmap_error() {
            return Err(display_error(error));
        }
        player
            .set_scenes(&project, ranges)
            .map_err(|error| JsValue::from_str(&error))?;
        Ok(Self {
            project,
            player,
            avi_streams: std::collections::HashMap::new(),
            next_avi_stream: 1,
        })
    }

    #[wasm_bindgen]
    pub fn hydrate_sprite_sheet(
        &mut self,
        sheet_png_bytes: &[u8],
        manifest_json: &str,
    ) -> Result<u32, JsValue> {
        self.player
            .hydrate_sprite_sheet(sheet_png_bytes, manifest_json)
            .map(|count| count as u32)
            .map_err(display_error)
    }

    pub fn amf_encode(&self, format: u8, graph_json: &str) -> Result<Vec<u8>, JsValue> {
        amf_encode(format, graph_json)
    }
    pub fn amf_decode(&self, format: u8, bytes: &[u8]) -> Result<String, JsValue> {
        amf_decode(format, bytes)
    }
    pub fn amf_decode_ext(
        &self,
        format: u8,
        bytes: &[u8],
        ext_reader: Option<js_sys::Function>,
    ) -> Result<String, JsValue> {
        amf_decode_ext(format, bytes, ext_reader)
    }
    pub fn sol_encode(&self, format: u8, name: &str, graph_json: &str) -> Result<Vec<u8>, JsValue> {
        sol_encode(format, name, graph_json)
    }
    pub fn sol_decode(&self, bytes: &[u8]) -> Result<String, JsValue> {
        sol_decode(bytes)
    }
    pub fn sol_decode_ext(
        &self,
        bytes: &[u8],
        ext_reader: Option<js_sys::Function>,
    ) -> Result<String, JsValue> {
        sol_decode_ext(bytes, ext_reader)
    }
    pub fn scene_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.player.runtime_snapshot(&self.project).draws)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    pub fn display_nodes_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.player.display_nodes(&self.project))
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    pub fn set_name(&mut self, target: &str, name: &str) -> Result<bool, JsValue> {
        Ok(self.player.set_name(&path(target)?, name))
    }

    pub fn set_text(&mut self, target: &str, text: &str) -> Result<bool, JsValue> {
        Ok(self.player.set_text(&path(target)?, text))
    }

    pub fn set_text_color(&mut self, target: &str, color: u32) -> Result<bool, JsValue> {
        Ok(self.player.set_text_color(&path(target)?, color))
    }

    pub fn accessibility_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.player.accessibility_nodes(&self.project))
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    pub fn activate_accessible(
        &mut self,
        target: &str,
        lifetime_id: &str,
    ) -> Result<bool, JsValue> {
        Ok(self
            .player
            .activate_accessible(&self.project, &path(target)?, lifetime_id))
    }

    pub fn scripts_json(&mut self) -> Result<String, JsValue> {
        serde_json::to_string(&self.player.drain_runtime_scripts())
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    pub fn is_dispatch_current(&self, json: &str) -> Result<bool, JsValue> {
        let value: serde_json::Value = serde_json::from_str(json).map_err(display_error)?;
        if value.get("target_ref").is_some() || value.get("namespace_ref").is_some() {
            let dispatch: RuntimeScriptDispatch =
                serde_json::from_value(value).map_err(display_error)?;
            return Ok(self
                .player
                .is_runtime_dispatch_current(&self.project, &dispatch));
        }
        let dispatch: ScriptDispatch =
            serde_json::from_str(json).map_err(|error| JsValue::from_str(&error.to_string()))?;
        Ok(self.player.is_dispatch_current(&self.project, &dispatch))
    }

    pub fn lifetime_id(&self, target: &str) -> Result<String, JsValue> {
        Ok(self
            .player
            .lifetime_id(&path(target)?)
            .map_or_else(String::new, |id| id.to_string()))
    }

    pub fn tick(&mut self) {
        self.player.tick(&self.project);
    }
    pub fn reset(&mut self) -> Result<(), JsValue> {
        self.project.symbols.retain(|s| !s.id.starts_with("__pub"));
        self.project
            .resources
            .retain(|r| !r.id.starts_with("__pub"));
        self.avi_streams.clear();
        self.player.reset(&self.project);
        if let Some(error) = self.player.bitmap_error() {
            return Err(display_error(error));
        }
        Ok(())
    }
    pub fn current_frame(&self) -> u32 {
        self.player.frame()
    }
    pub fn total_frames(&self) -> u32 {
        self.project
            .symbol(&self.project.root)
            .map_or(0, |symbol| symbol.timeline.frame_count)
    }
    pub fn is_playing(&self) -> bool {
        self.player.is_playing()
    }
    pub fn set_playing(&mut self, value: bool) {
        self.player.set_playing(value);
        if value {
            self.player.sync_timeline_videos(&self.project);
        }
    }
    pub fn script_overflowed(&self) -> bool {
        self.player.script_overflowed()
    }
    /// Timeline sound commands (`SoundCommand`, tagged by `op`) since the last call.
    pub fn sound_commands_json(&mut self) -> Result<String, JsValue> {
        serde_json::to_string(&self.player.drain_sound_commands()).map_err(display_error)
    }
    /// Authored timeline-video commands (`VideoCommand`, tagged by `op`) since the last call.
    pub fn video_commands_json(&mut self) -> Result<String, JsValue> {
        serde_json::to_string(&self.player.drain_video_commands()).map_err(display_error)
    }
    /// Resolved deployment sources (`[(resource_id, VideoSourceSpec)]`) for all timeline videos.
    pub fn timeline_video_sources_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&neoflash_core::timeline_video_sources(&self.project))
            .map_err(display_error)
    }
    /// Resolved deployment source (`Option<VideoSourceSpec>`) for a video resource by ID.
    pub fn video_source_spec_json(&self, resource_id: &str) -> Result<String, JsValue> {
        serde_json::to_string(&neoflash_core::resolve_video_resource_source(
            &self.project,
            resource_id,
        ))
        .map_err(display_error)
    }
    /// Package manifest entries (`[VideoManifestEntry]`) for all video resources in the project.
    pub fn video_manifest_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&neoflash_core::video_manifest_entries(&self.project))
            .map_err(display_error)
    }
    /// Audio resource IDs used by timeline sounds (decoded ahead of their keyframes).
    pub fn timeline_sound_resources_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&neoflash_core::timeline_sound_resources(&self.project))
            .map_err(display_error)
    }
    /// AS2 `stopAllSounds()` / AS3 `SoundMixer.stopAll()`; drain `sound_commands_json` next.
    pub fn stop_all_sounds(&mut self) {
        self.player.stop_all_sounds();
    }

    pub fn control(&mut self, target: &str, command: &str, frame: u32) -> Result<bool, JsValue> {
        let target = path(target)?;
        let command = match command {
            "play" => PlaybackCommand::Play,
            "stop" => PlaybackCommand::Stop,
            "goto_and_play" => PlaybackCommand::GotoAndPlay(frame),
            "goto_and_stop" => PlaybackCommand::GotoAndStop(frame),
            _ => return Err(JsValue::from_str("Unknown timeline control command")),
        };
        Ok(self.player.control(&self.project, &target, command))
    }

    pub fn click(&mut self, target: &str) -> Result<(), JsValue> {
        self.player.dispatch_click(&self.project, &path(target)?);
        Ok(())
    }

    pub fn property(&self, target: &str, name: &str) -> Result<f64, JsValue> {
        Ok(self
            .player
            .property(&self.project, &path(target)?, name)
            .unwrap_or(f64::NAN))
    }

    pub fn visible(&self, target: &str) -> Result<bool, JsValue> {
        self.player
            .visible(&self.project, &path(target)?)
            .ok_or_else(|| JsValue::from_str("Display object is no longer active."))
    }

    pub fn set_visible(&mut self, target: &str, visible: bool) -> Result<bool, JsValue> {
        Ok(self.player.set_visible(&path(target)?, visible))
    }

    pub fn set_property(&mut self, target: &str, name: &str, value: f64) -> Result<bool, JsValue> {
        Ok(self.player.set_property(&path(target)?, name, value))
    }

    pub fn clip_frame(&self, target: &str) -> Result<u32, JsValue> {
        Ok(self.player.clip_frame(&path(target)?).unwrap_or(0))
    }

    pub fn clip_frame_count(&self, target: &str) -> Result<u32, JsValue> {
        Ok(self
            .player
            .clip_frame_count(&self.project, &path(target)?)
            .unwrap_or(0))
    }

    pub fn clip_metadata(&self, target: &str) -> Result<String, JsValue> {
        let metadata = self
            .player
            .clip_metadata(&self.project, &path(target)?)
            .ok_or_else(|| {
                JsValue::from_str("Timeline metadata requires a live local MovieClip.")
            })?;
        serde_json::to_string(&metadata).map_err(display_error)
    }

    pub fn resolve_frame(&self, target: &str, label: &str) -> Result<i32, JsValue> {
        Ok(self
            .player
            .resolve_frame(&self.project, &path(target)?, label)
            .map_or(-1, |frame| frame as i32))
    }

    pub fn resolve_frame_as2(&self, target: &str, label: &str) -> Result<i32, JsValue> {
        Ok(self
            .player
            .resolve_frame_as2(&self.project, &path(target)?, label)
            .map_or(-1, |frame| frame as i32))
    }

    /// Scenes of a live timeline; only a multi-scene main timeline has more than one.
    pub fn scene_count(&self, target: &str) -> Result<u32, JsValue> {
        Ok(self
            .player
            .clip_scene_count(&self.project, &path(target)?)
            .unwrap_or(0))
    }

    /// `{"frame": number|label, "scene": name|null, "as2": bool}` to a
    /// zero-based frame; -1 is an AS2 label that does not exist (no jump).
    pub fn resolve_goto(&self, target: &str, request: &str) -> Result<i32, JsValue> {
        goto_result(self.player.resolve_goto(
            &self.project,
            &path(target)?,
            &goto_request(request)?,
        ))
    }

    /// First frame of the adjacent scene, or -1 past the first/last scene.
    pub fn scene_step(&self, target: &str, step: i32) -> Result<i32, JsValue> {
        Ok(self
            .player
            .resolve_scene_step(&self.project, &path(target)?, step)
            .map_or(-1, |frame| frame as i32))
    }

    pub fn child_path(&self, parent: &str, name: &str) -> Result<String, JsValue> {
        serde_json::to_string(&self.player.child_path(&self.project, &path(parent)?, name))
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    pub fn runtime_snapshot_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.player.runtime_snapshot(&self.project)).map_err(display_error)
    }
    pub fn object_ref(&self, target: &str) -> Result<String, JsValue> {
        serde_json::to_string(&self.player.object_ref(&path(target)?)).map_err(display_error)
    }
    pub fn object_alive(&self, target: &str) -> Result<bool, JsValue> {
        Ok(self.player.object_alive(&object_ref(target)?))
    }
    pub fn object_input_info(&self, target: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .object_input_info(&self.project, &object_ref(target)?)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn object_input_update(&mut self, target: &str, state: &str) -> Result<(), JsValue> {
        if state.len() > 4096 {
            return Err(JsValue::from_str("Display input state exceeds 4096 bytes"));
        }
        let state: neoflash_core::DisplayInputState =
            serde_json::from_str(state).map_err(display_error)?;
        self.player
            .object_input_update(&self.project, &object_ref(target)?, state)
            .map_err(display_error)
    }
    pub fn object_movieclip_pointer(&mut self, event: &str, target: &str) -> Result<bool, JsValue> {
        let target = if target == "null" {
            None
        } else {
            Some(object_ref(target)?)
        };
        self.player
            .object_movieclip_pointer(&self.project, event, target.as_ref())
            .map_err(display_error)
    }
    pub fn object_remove_children(
        &mut self,
        target: &str,
        begin: i32,
        end: i32,
    ) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .object_remove_children(&self.project, &object_ref(target)?, begin, end)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn object_stop_all_movie_clips(&mut self, target: &str) -> Result<(), JsValue> {
        self.player
            .object_stop_all_movie_clips(&self.project, &object_ref(target)?)
            .map_err(display_error)
    }
    pub fn create_linked(&mut self, linkage_id: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .create_linked(&self.project, linkage_id)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn object_click(&mut self, target: &str) -> Result<bool, JsValue> {
        Ok(self
            .player
            .object_click(&self.project, &object_ref(target)?))
    }
    pub fn object_activate_accessible(&mut self, target: &str) -> Result<bool, JsValue> {
        Ok(self
            .player
            .object_activate_accessible(&self.project, &object_ref(target)?))
    }

    pub fn object_text_info(&self, target: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .object_text_info(&self.project, &object_ref(target)?)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn object_text_update(&mut self, target: &str, patch_json: &str) -> Result<bool, JsValue> {
        if patch_json.len() > 8 * 1024 * 1024 {
            return Err(JsValue::from_str("Text update exceeds 8 MiB"));
        }
        let patch: TextPatch = serde_json::from_str(patch_json).map_err(display_error)?;
        self.player
            .object_text_update(&self.project, &object_ref(target)?, &patch)
            .map_err(display_error)
    }
    pub fn object_text_input(&mut self, target: &str, edit_json: &str) -> Result<String, JsValue> {
        if edit_json.len() > 8 * 1024 * 1024 {
            return Err(JsValue::from_str("Text input transaction exceeds 8 MiB"));
        }
        let edit: TextInputEdit = serde_json::from_str(edit_json).map_err(display_error)?;
        serde_json::to_string(
            &self
                .player
                .object_text_input(&self.project, &object_ref(target)?, &edit)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn text_layout_json(
        &self,
        field_json: &str,
        measure: &js_sys::Function,
    ) -> Result<String, JsValue> {
        text_layout_json(field_json, measure)
    }
    pub fn text_query_json(
        &self,
        field_json: &str,
        query_json: &str,
        measure: &js_sys::Function,
    ) -> Result<String, JsValue> {
        text_query_json(field_json, query_json, measure)
    }
    pub fn portable_measure_text_json(
        &self,
        strings_json: &str,
        format_json: &str,
    ) -> Result<String, JsValue> {
        if strings_json.len() > 8 * 1024 * 1024 || format_json.len() > 64 * 1024 {
            return Err(JsValue::from_str(
                "Portable text measurement input exceeds size budget",
            ));
        }
        let strings: Vec<String> = serde_json::from_str(strings_json).map_err(display_error)?;
        let format: neoflash_core::TextFormat =
            serde_json::from_str(format_json).map_err(display_error)?;
        let font_bytes = neoflash_core::resolve_project_font_bytes(
            &self.project,
            &format.font,
            format.bold,
            format.italic,
        );
        serde_json::to_string(&neoflash_core::TextFormat::portable_measure_strings(
            &strings,
            &format,
            font_bytes.as_deref(),
        ))
        .map_err(display_error)
    }
    pub fn parse_html_text_json(&self, field_json: &str, html: &str) -> Result<String, JsValue> {
        parse_html_text_json(field_json, html)
    }
    pub fn serialize_html_text_json(&self, field_json: &str) -> Result<String, JsValue> {
        serialize_html_text_json(field_json)
    }
    pub fn text_to_outlines_json(&self, field_json: &str) -> Result<String, JsValue> {
        if field_json.len() > 8 * 1024 * 1024 {
            return Err(JsValue::from_str("Text field state exceeds 8 MiB"));
        }
        let field: TextFieldState = serde_json::from_str(field_json).map_err(display_error)?;
        let font_bytes = neoflash_core::resolve_project_font_bytes(
            &self.project,
            &field.format.font,
            field.format.bold,
            field.format.italic,
        );
        let commands = field.to_outline_commands(font_bytes.as_deref());
        serde_json::to_string(&commands).map_err(display_error)
    }
    pub fn object_text_fit_portable(&mut self, target: &str) -> Result<String, JsValue> {
        let layout = self
            .player
            .object_text_fit_portable(&self.project, &object_ref(target)?)
            .map_err(display_error)?;
        serde_json::to_string(&layout).map_err(display_error)
    }
    pub fn pixel_decode(&mut self, encoded: &[u8]) -> Result<String, JsValue> {
        serde_json::to_string(&self.player.pixel_decode(encoded).map_err(display_error)?)
            .map_err(display_error)
    }
    pub fn pixel_release(&mut self, pixels: &str) -> Result<(), JsValue> {
        self.player
            .pixel_release(&pixel_ref(pixels)?)
            .map_err(display_error)
    }
    pub fn loader_content(&self, target: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .loader_content(&object_ref(target)?)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn loader_replace(
        &mut self,
        target: &str,
        pixels: &str,
        as2_replace: bool,
    ) -> Result<String, JsValue> {
        if pixels.len() > 4096 {
            return Err(JsValue::from_str(
                "Loader pixel reference exceeds input limit",
            ));
        }
        let pixels: Option<PixelRef> = serde_json::from_str(pixels).map_err(display_error)?;
        serde_json::to_string(
            &self
                .player
                .loader_replace(&self.project, &object_ref(target)?, pixels, as2_replace)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn loader_mount_project(
        &mut self,
        target: &str,
        child_project_json: &str,
        as2_replace: bool,
    ) -> Result<String, JsValue> {
        if child_project_json.len() > neoflash_core::MAX_PROJECT_JSON_BYTES {
            return Err(JsValue::from_str(
                "Published bundle JSON exceeds 128 MiB limit",
            ));
        }
        let child_project = Project::from_published_bundle_json(child_project_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid published project JSON: {e}")))?;
        let mounted = self
            .player
            .loader_mount_project(
                &mut self.project,
                &object_ref(target)?,
                child_project,
                as2_replace,
            )
            .map_err(display_error)?;
        serde_json::to_string(&mounted).map_err(display_error)
    }
    pub fn hydrate_mounted_resource_rgba(
        &mut self,
        resource_id: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<(), JsValue> {
        self.player
            .hydrate_mounted_resource_rgba(resource_id, width, height, rgba.to_vec())
            .map_err(display_error)
    }
    pub fn drain_retired_published_prefixes(&mut self) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .drain_retired_published_prefixes(&mut self.project),
        )
        .map_err(display_error)
    }
    pub fn loader_unload_published(
        &mut self,
        target: &str,
        as2_replace: bool,
        stop_detached: bool,
    ) -> Result<String, JsValue> {
        let mutation = self
            .player
            .loader_unload_published(
                &mut self.project,
                &object_ref(target)?,
                as2_replace,
                stop_detached,
            )
            .map_err(display_error)?;
        serde_json::to_string(&mutation).map_err(display_error)
    }
    pub fn avi_open_stream(&mut self, bytes: &[u8]) -> Result<String, JsValue> {
        if self.avi_streams.len() >= 32 {
            return Err(JsValue::from_str(
                "At most 32 indexed AVI streams may be open concurrently.",
            ));
        }
        let indexed =
            neoflash_core::index_avi_container(bytes).map_err(|e| JsValue::from_str(&e))?;
        let info = indexed.info.clone();
        let frame_count = indexed.frames.len();
        let audio_mime = match info.audio_format_tag {
            Some(1) => Some("audio/wav"),
            Some(0x0055) => Some("audio/mpeg"),
            _ => None,
        };
        let has_audio = audio_mime.is_some();
        let handle = self.next_avi_stream;
        self.next_avi_stream = self.next_avi_stream.wrapping_add(1).max(1);
        self.avi_streams.insert(handle, (bytes.to_vec(), indexed));
        Ok(serde_json::json!({
            "handle": handle,
            "width": info.width,
            "height": info.height,
            "fps": info.fps,
            "duration_ms": info.duration_ms(),
            "frame_count": frame_count,
            "has_audio": has_audio,
            "audio_mime": audio_mime,
        })
        .to_string())
    }
    pub fn avi_stream_frame_rgba(&self, handle: u32, time_ms: u32) -> Result<Vec<u8>, JsValue> {
        let (bytes, indexed) = self
            .avi_streams
            .get(&handle)
            .ok_or_else(|| JsValue::from_str("AVI stream handle is closed or invalid."))?;
        let idx = indexed
            .frame_index_at_ms(time_ms)
            .ok_or_else(|| JsValue::from_str("AVI contains no decodable video frames."))?;
        let img = indexed
            .decode_frame(bytes, idx)
            .map_err(|e| JsValue::from_str(&e))?;
        Ok(img.rgba.to_vec())
    }
    pub fn avi_stream_audio_wav(&self, handle: u32) -> Result<Vec<u8>, JsValue> {
        let (bytes, indexed) = self
            .avi_streams
            .get(&handle)
            .ok_or_else(|| JsValue::from_str("AVI stream handle is closed or invalid."))?;
        let (audio, _) = indexed
            .extract_audio(bytes)
            .map_err(|e| JsValue::from_str(&e))?;
        Ok(audio.unwrap_or_default())
    }
    pub fn avi_close_stream(&mut self, handle: u32) {
        self.avi_streams.remove(&handle);
    }
    pub fn avi_clear_streams(&mut self) {
        self.avi_streams.clear();
    }
    pub fn avi_inspect_json(&self, bytes: &[u8]) -> Result<String, JsValue> {
        let indexed =
            neoflash_core::index_avi_container(bytes).map_err(|e| JsValue::from_str(&e))?;
        Ok(serde_json::json!({
            "width": indexed.info.width,
            "height": indexed.info.height,
            "fps": indexed.info.fps,
            "duration_ms": indexed.info.duration_ms(),
            "frame_count": indexed.frames.len(),
            "has_audio": indexed.info.audio_format_tag.is_some(),
        })
        .to_string())
    }
    pub fn avi_frame_rgba(&self, bytes: &[u8], time_ms: u32) -> Result<Vec<u8>, JsValue> {
        let indexed =
            neoflash_core::index_avi_container(bytes).map_err(|e| JsValue::from_str(&e))?;
        let idx = indexed
            .frame_index_at_ms(time_ms)
            .ok_or_else(|| JsValue::from_str("AVI contains no decodable video frames."))?;
        let img = indexed
            .decode_frame(bytes, idx)
            .map_err(|e| JsValue::from_str(&e))?;
        Ok(img.rgba.to_vec())
    }
    pub fn bitmap_create(
        &mut self,
        width: u32,
        height: u32,
        transparent: bool,
        fill_argb: u32,
    ) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .bitmap_create(width, height, transparent, fill_argb)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn bitmap_info(&self, target: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .bitmap_info(&pixel_ref(target)?)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn bitmap_pixels(&self, target: &str) -> Result<Vec<u8>, JsValue> {
        self.player
            .bitmap_pixels(&pixel_ref(target)?)
            .map_err(display_error)
    }
    pub fn bitmap_pixels_transformed(&self, target: &str, color: &str) -> Result<Vec<u8>, JsValue> {
        self.player
            .bitmap_pixels_transformed(&pixel_ref(target)?, color_transform(color)?)
            .map_err(display_error)
    }
    pub fn bitmap_render_snapshot(
        &self,
        source: &str,
        matrix: &str,
        color: &str,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, JsValue> {
        if source.is_empty() {
            return self
                .player
                .render_snapshot_rgba(&self.project, width, height)
                .map_err(display_error);
        }
        if matrix.len() > 4096 {
            return Err(JsValue::from_str("Matrix input exceeds 4096 bytes"));
        }
        let matrix: [f64; 6] = serde_json::from_str(matrix).map_err(display_error)?;
        let snapshot = self
            .player
            .bitmap_draw_snapshot(
                &self.project,
                &object_ref(source)?,
                matrix,
                color_transform(color)?,
            )
            .map_err(display_error)?;
        self.player
            .render_runtime_snapshot_rgba(&self.project, &snapshot, width, height)
            .map_err(display_error)
    }
    pub fn bitmap_render_runtime_snapshot(
        &self,
        snapshot_json: &str,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, JsValue> {
        let snapshot: neoflash_core::RuntimeSnapshot =
            serde_json::from_str(snapshot_json).map_err(display_error)?;
        self.player
            .render_runtime_snapshot_rgba(&self.project, &snapshot, width, height)
            .map_err(display_error)
    }
    pub fn bitmap_get_pixels(
        &self,
        target: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        little: bool,
    ) -> Result<Vec<u8>, JsValue> {
        self.player
            .bitmap_get_pixels(
                &pixel_ref(target)?,
                neoflash_core::BitmapRect {
                    x,
                    y,
                    width,
                    height,
                },
                little,
            )
            .map_err(display_error)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bitmap_set_pixels(
        &mut self,
        target: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        bytes: &[u8],
        little: bool,
    ) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .bitmap_set_pixels(
                    &pixel_ref(target)?,
                    neoflash_core::BitmapRect {
                        x,
                        y,
                        width,
                        height,
                    },
                    bytes,
                    little,
                )
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn bitmap_color_transform(
        &mut self,
        target: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: &str,
    ) -> Result<(), JsValue> {
        self.player
            .bitmap_color_transform(
                &pixel_ref(target)?,
                neoflash_core::BitmapRect {
                    x,
                    y,
                    width,
                    height,
                },
                color_transform(color)?,
            )
            .map_err(display_error)
    }
    pub fn bitmap_generate_filter_rect(
        &self,
        target: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        filter: &str,
    ) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .bitmap_generate_filter_rect(
                    &pixel_ref(target)?,
                    neoflash_core::BitmapRect {
                        x,
                        y,
                        width,
                        height,
                    },
                    &single_filter(filter)?,
                )
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bitmap_apply_filter(
        &mut self,
        target: &str,
        source: &str,
        sx: i32,
        sy: i32,
        width: i32,
        height: i32,
        dx: i32,
        dy: i32,
        filter: &str,
    ) -> Result<(), JsValue> {
        self.player
            .bitmap_apply_filter(
                &pixel_ref(target)?,
                &pixel_ref(source)?,
                neoflash_core::BitmapRect {
                    x: sx,
                    y: sy,
                    width,
                    height,
                },
                dx,
                dy,
                &single_filter(filter)?,
            )
            .map_err(display_error)
    }
    pub fn bitmap_encode_png(
        &self,
        target: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        fast: bool,
    ) -> Result<Vec<u8>, JsValue> {
        self.player
            .bitmap_encode_png(
                &pixel_ref(target)?,
                neoflash_core::BitmapRect {
                    x,
                    y,
                    width,
                    height,
                },
                fast,
            )
            .map_err(display_error)
    }
    pub fn bitmap_encode_jpeg(
        &self,
        target: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        quality: u8,
    ) -> Result<Vec<u8>, JsValue> {
        self.player
            .bitmap_encode_jpeg(
                &pixel_ref(target)?,
                neoflash_core::BitmapRect {
                    x,
                    y,
                    width,
                    height,
                },
                quality,
            )
            .map_err(display_error)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bitmap_encode_jpegxr(
        &self,
        target: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        quantization: u8,
        color_space: &str,
        trim_flex_bits: u8,
    ) -> Result<Vec<u8>, JsValue> {
        let pref = pixel_ref(target)?;
        let info = self.player.bitmap_info(&pref).map_err(display_error)?;
        let rect = neoflash_core::BitmapRect {
            x,
            y,
            width,
            height,
        };
        let (x0, y0, x1, y1) = rect.clipped_dims(info.width, info.height);
        if x0 >= x1 || y0 >= y1 {
            return Err(JsValue::from_str(
                "BitmapData: JPEG-XR encoding requires a nonempty clipped rectangle",
            ));
        }
        let argb = self
            .player
            .bitmap_get_pixels(&pref, rect, false)
            .map_err(display_error)?;
        neoflash_core::BitmapRect::encode_jpegxr_argb(
            (x1 - x0) as u32,
            (y1 - y0) as u32,
            info.transparent,
            &argb,
            quantization,
            color_space,
            trim_flex_bits,
        )
        .map_err(display_error)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bitmap_draw_rgba(
        &mut self,
        target: &str,
        width: u32,
        height: u32,
        bytes: &[u8],
        clip_rect: &str,
        expected_revision: &str,
    ) -> Result<(), JsValue> {
        if clip_rect.len() > 4096 || expected_revision.len() > 32 {
            return Err(JsValue::from_str(
                "BitmapData draw arguments exceed their input bounds",
            ));
        }
        let rect: Option<neoflash_core::BitmapRect> =
            serde_json::from_str(clip_rect).map_err(display_error)?;
        self.player
            .bitmap_draw_rgba(
                &pixel_ref(target)?,
                width,
                height,
                bytes,
                rect,
                expected_revision,
            )
            .map_err(display_error)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bitmap_draw_rgba_blended(
        &mut self,
        target: &str,
        width: u32,
        height: u32,
        bytes: &[u8],
        clip_rect: &str,
        expected_revision: &str,
        blend_mode: &str,
    ) -> Result<(), JsValue> {
        if clip_rect.len() > 4096 || expected_revision.len() > 32 || blend_mode.len() > 32 {
            return Err(JsValue::from_str(
                "BitmapData draw arguments exceed their input bounds",
            ));
        }
        let rect: Option<neoflash_core::BitmapRect> =
            serde_json::from_str(clip_rect).map_err(display_error)?;
        self.player
            .bitmap_draw_rgba_blended(
                &pixel_ref(target)?,
                width,
                height,
                bytes,
                rect,
                expected_revision,
                blend_mode,
            )
            .map_err(display_error)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bitmap_draw_rgba_shader(
        &mut self,
        target: &str,
        width: u32,
        height: u32,
        fg_rgba: &[u8],
        clip_rect: &str,
        expected_revision: &str,
        bytecode_base64: &str,
        precision_hint: &str,
        params_json: &str,
    ) -> Result<(), JsValue> {
        if clip_rect.len() > 4096 || expected_revision.len() > 32 || precision_hint.len() > 32 {
            return Err(JsValue::from_str(
                "BitmapData draw arguments exceed their input bounds",
            ));
        }
        let pref = pixel_ref(target)?;
        let info = self.player.bitmap_info(&pref).map_err(display_error)?;
        if info.width != width
            || info.height != height
            || fg_rgba.len() != width as usize * height as usize * 4
        {
            return Err(JsValue::from_str(
                "BitmapData: draw raster must match destination dimensions and RGBA byte length",
            ));
        }
        if info.revision != expected_revision {
            return Err(JsValue::from_str(
                "BitmapData: draw destination changed during raster capture",
            ));
        }
        let rect: Option<neoflash_core::BitmapRect> =
            serde_json::from_str(clip_rect).map_err(display_error)?;
        let effective = rect.unwrap_or(neoflash_core::BitmapRect {
            x: 0,
            y: 0,
            width: width as i32,
            height: height as i32,
        });
        let (x0, y0, x1, y1) = effective.clipped_dims(width, height);
        if x0 >= x1 || y0 >= y1 {
            return Ok(());
        }
        let sub_w = (x1 - x0) as u32;
        let sub_h = (y1 - y0) as u32;
        let sub_rect = neoflash_core::BitmapRect {
            x: x0 as i32,
            y: y0 as i32,
            width: sub_w as i32,
            height: sub_h as i32,
        };
        let bg_argb = self
            .player
            .bitmap_get_pixels(&pref, sub_rect, false)
            .map_err(display_error)?;
        let px_count = (sub_w * sub_h) as usize;
        let mut bg_sub = Vec::with_capacity(px_count * 4);
        let mut fg_sub = Vec::with_capacity(px_count * 4);
        for sy in 0..sub_h as usize {
            for sx in 0..sub_w as usize {
                let bi = (sy * sub_w as usize + sx) * 4;
                bg_sub.extend_from_slice(&[
                    bg_argb[bi + 1],
                    bg_argb[bi + 2],
                    bg_argb[bi + 3],
                    bg_argb[bi],
                ]);
                let fi = ((y0 + sy) * width as usize + (x0 + sx)) * 4;
                fg_sub.extend_from_slice(&fg_rgba[fi..fi + 4]);
            }
        }
        let blended = neoflash_core::as3_fte_3d::evaluate_blend_shader_rgba(
            &bg_sub,
            &fg_sub,
            sub_w,
            sub_h,
            bytecode_base64,
            precision_hint,
            params_json,
        )
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let mut out_argb = Vec::with_capacity(px_count * 4);
        for p in blended.as_chunks::<4>().0 {
            let a = if info.transparent { p[3] } else { 255 };
            out_argb.extend_from_slice(&[a, p[0], p[1], p[2]]);
        }
        self.player
            .bitmap_set_pixels(&pref, sub_rect, &out_argb, false)
            .map_err(display_error)?;
        Ok(())
    }
    pub fn bitmap_draw_snapshot(
        &self,
        source: &str,
        matrix: &str,
        color: &str,
    ) -> Result<String, JsValue> {
        if matrix.len() > 4096 {
            return Err(JsValue::from_str("Matrix input exceeds 4096 bytes"));
        }
        let matrix: [f64; 6] = serde_json::from_str(matrix).map_err(display_error)?;
        serde_json::to_string(
            &self
                .player
                .bitmap_draw_snapshot(
                    &self.project,
                    &object_ref(source)?,
                    matrix,
                    color_transform(color)?,
                )
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn bitmap_get_pixel(
        &self,
        target: &str,
        x: i32,
        y: i32,
        with_alpha: bool,
    ) -> Result<u32, JsValue> {
        self.player
            .bitmap_get_pixel(&pixel_ref(target)?, x, y, with_alpha)
            .map_err(display_error)
    }
    pub fn bitmap_set_pixel(
        &mut self,
        target: &str,
        x: i32,
        y: i32,
        color: u32,
        with_alpha: bool,
    ) -> Result<(), JsValue> {
        self.player
            .bitmap_set_pixel(&pixel_ref(target)?, x, y, color, with_alpha)
            .map_err(display_error)
    }
    pub fn bitmap_fill_rect(
        &mut self,
        target: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: u32,
    ) -> Result<(), JsValue> {
        self.player
            .bitmap_fill_rect(&pixel_ref(target)?, x, y, width, height, color)
            .map_err(display_error)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bitmap_copy_pixels(
        &mut self,
        dst: &str,
        src: &str,
        sx: i32,
        sy: i32,
        width: i32,
        height: i32,
        dx: i32,
        dy: i32,
        merge_alpha: bool,
    ) -> Result<(), JsValue> {
        self.player
            .bitmap_copy_pixels(
                &pixel_ref(dst)?,
                &pixel_ref(src)?,
                sx,
                sy,
                width,
                height,
                dx,
                dy,
                merge_alpha,
            )
            .map_err(display_error)
    }
    pub fn bitmap_histogram_json(&self, target: &str, rect: &str) -> Result<String, JsValue> {
        if rect.len() > 4096 {
            return Err(JsValue::from_str("Histogram rect exceeds 4096 bytes"));
        }
        let rect: Option<neoflash_core::BitmapRect> =
            serde_json::from_str(rect).map_err(display_error)?;
        serde_json::to_string(
            &self
                .player
                .bitmap_histogram(&pixel_ref(target)?, rect)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn bitmap_clone(&mut self, target: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .bitmap_clone(&pixel_ref(target)?)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn bitmap_scroll(&mut self, target: &str, dx: i32, dy: i32) -> Result<(), JsValue> {
        self.player
            .bitmap_scroll(&pixel_ref(target)?, dx, dy)
            .map_err(display_error)
    }
    pub fn bitmap_get_pixel_u32(
        &self,
        id: u32,
        generation: u32,
        x: i32,
        y: i32,
        with_alpha: bool,
    ) -> Result<u32, JsValue> {
        let reference = PixelRef {
            id: id.to_string(),
            generation: generation.to_string(),
        };
        self.player
            .bitmap_get_pixel(&reference, x, y, with_alpha)
            .map_err(display_error)
    }
    pub fn bitmap_set_pixel_u32(
        &mut self,
        id: u32,
        generation: u32,
        x: i32,
        y: i32,
        color: u32,
        with_alpha: bool,
    ) -> Result<(), JsValue> {
        let reference = PixelRef {
            id: id.to_string(),
            generation: generation.to_string(),
        };
        self.player
            .bitmap_set_pixel(&reference, x, y, color, with_alpha)
            .map_err(display_error)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bitmap_fill_rect_u32(
        &mut self,
        id: u32,
        generation: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: u32,
    ) -> Result<(), JsValue> {
        let reference = PixelRef {
            id: id.to_string(),
            generation: generation.to_string(),
        };
        self.player
            .bitmap_fill_rect(&reference, x, y, width, height, color)
            .map_err(display_error)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bitmap_copy_pixels_u32(
        &mut self,
        dst_id: u32,
        dst_generation: u32,
        src_id: u32,
        src_generation: u32,
        sx: i32,
        sy: i32,
        width: i32,
        height: i32,
        dx: i32,
        dy: i32,
        merge_alpha: bool,
    ) -> Result<(), JsValue> {
        let dst = PixelRef {
            id: dst_id.to_string(),
            generation: dst_generation.to_string(),
        };
        let src = PixelRef {
            id: src_id.to_string(),
            generation: src_generation.to_string(),
        };
        self.player
            .bitmap_copy_pixels(&dst, &src, sx, sy, width, height, dx, dy, merge_alpha)
            .map_err(display_error)
    }
    pub fn bitmap_scroll_u32(
        &mut self,
        id: u32,
        generation: u32,
        dx: i32,
        dy: i32,
    ) -> Result<(), JsValue> {
        let reference = PixelRef {
            id: id.to_string(),
            generation: generation.to_string(),
        };
        self.player
            .bitmap_scroll(&reference, dx, dy)
            .map_err(display_error)
    }
    pub fn bitmap_dispose(&mut self, target: &str) -> Result<(), JsValue> {
        self.player
            .bitmap_dispose(&pixel_ref(target)?)
            .map_err(display_error)
    }
    pub fn bitmap_lock_u32(&mut self, id: u32, generation: u32) -> Result<String, JsValue> {
        let reference = PixelRef {
            id: id.to_string(),
            generation: generation.to_string(),
        };
        self.player.bitmap_lock(&reference).map_err(display_error)
    }
    pub fn bitmap_unlock_u32(&mut self, id: u32, generation: u32) -> Result<String, JsValue> {
        let reference = PixelRef {
            id: id.to_string(),
            generation: generation.to_string(),
        };
        self.player.bitmap_unlock(&reference).map_err(display_error)
    }
    pub fn bitmap_locked_pixels(&self, target: &str) -> Result<Vec<u8>, JsValue> {
        self.player
            .bitmap_locked_pixels(&pixel_ref(target)?)
            .map_err(display_error)
    }
    pub fn object_video_info(&self, target: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .object_video_info(&self.project, &object_ref(target)?)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn object_video_update(&mut self, target: &str, state: &str) -> Result<(), JsValue> {
        if state.len() > 4096 {
            return Err(JsValue::from_str("Video state exceeds 4096 bytes"));
        }
        let state: neoflash_core::VideoState =
            serde_json::from_str(state).map_err(display_error)?;
        self.player
            .object_video_update(&self.project, &object_ref(target)?, state)
            .map_err(display_error)
    }
    pub fn object_bitmap(&self, target: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .object_bitmap(&self.project, &object_ref(target)?)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn object_set_bitmap(&mut self, target: &str, value: &str) -> Result<bool, JsValue> {
        let value = if value == "null" {
            None
        } else {
            Some(pixel_ref(value)?)
        };
        self.player
            .object_set_bitmap(&self.project, &object_ref(target)?, value)
            .map_err(display_error)
    }
    pub fn object_smoothing(&self, target: &str) -> Result<bool, JsValue> {
        self.player
            .object_smoothing(&self.project, &object_ref(target)?)
            .map_err(display_error)
    }
    pub fn object_set_smoothing(&mut self, target: &str, value: bool) -> Result<bool, JsValue> {
        self.player
            .object_set_smoothing(&self.project, &object_ref(target)?, value)
            .map_err(display_error)
    }
    pub fn object_pixel_snapping(&self, target: &str) -> Result<String, JsValue> {
        Ok(match self
            .player
            .object_pixel_snapping(&self.project, &object_ref(target)?)
            .map_err(display_error)?
        {
            PixelSnapping::Auto => "auto",
            PixelSnapping::Always => "always",
            PixelSnapping::Never => "never",
        }
        .into())
    }
    pub fn object_set_pixel_snapping(
        &mut self,
        target: &str,
        value: &str,
    ) -> Result<bool, JsValue> {
        let mode = match value {
            "auto" => PixelSnapping::Auto,
            "always" => PixelSnapping::Always,
            "never" => PixelSnapping::Never,
            _ => {
                return Err(JsValue::from_str(
                    "PixelSnapping must be auto, always or never",
                ));
            }
        };
        self.player
            .object_set_pixel_snapping(&self.project, &object_ref(target)?, mode)
            .map_err(display_error)
    }
    pub fn create_display(&mut self, kind: &str) -> Result<String, JsValue> {
        let kind = match kind {
            "sprite" => DynamicKind::Sprite,
            "shape" => DynamicKind::Shape,
            "bitmap" => DynamicKind::Bitmap,
            "text_field" => DynamicKind::TextField,
            "video" => DynamicKind::Video,
            "movie_clip" => DynamicKind::MovieClip,
            "loader" => DynamicKind::Loader,
            "button" => DynamicKind::Button,
            "text_line" => DynamicKind::TextLine,
            "morph_shape" => DynamicKind::MorphShape,
            "avm1_movie" => DynamicKind::Avm1Movie,
            _ => {
                return Err(JsValue::from_str(
                    "Only empty Sprite, Shape and MovieClip creation is supported; linked dynamic timelines are pending",
                ));
            }
        };
        serde_json::to_string(&self.player.create_display(kind).map_err(display_error)?)
            .map_err(display_error)
    }
    pub fn set_text_line_draws(&mut self, target: &str, runs_json: &str) -> Result<(), JsValue> {
        let runs: Vec<neoflash_core::as3_fte_3d::FteDrawRunOut> =
            serde_json::from_str(runs_json).map_err(display_error)?;
        self.player
            .set_text_line_draws(&object_ref(target)?, runs)
            .map_err(display_error)
    }
    pub fn stage_children_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.player.stage_children()).map_err(display_error)
    }
    pub fn stage_mutate(&mut self, json: &str) -> Result<String, JsValue> {
        if json.len() > 4096 {
            return Err(JsValue::from_str("Stage mutation input exceeds its limit"));
        }
        let mutation: StageMutation = serde_json::from_str(json).map_err(display_error)?;
        serde_json::to_string(
            &self
                .player
                .stage_mutate(&self.project, mutation)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn display_mutate(&mut self, json: &str) -> Result<String, JsValue> {
        if json.len() > 4096 {
            return Err(JsValue::from_str(
                "Display mutation input exceeds its limit",
            ));
        }
        let mutation: DisplayMutation = serde_json::from_str(json).map_err(display_error)?;
        serde_json::to_string(
            &self
                .player
                .display_mutate(&self.project, mutation)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn display_events_json(&mut self) -> Result<String, JsValue> {
        serde_json::to_string(&self.player.drain_display_events().map_err(display_error)?)
            .map_err(display_error)
    }
    pub fn graphics_apply(&mut self, target: &str, json: &str) -> Result<(), JsValue> {
        if json.len() > 1_048_576 {
            return Err(JsValue::from_str("Graphics batch input exceeds 1 MiB"));
        }
        let operations: Vec<GraphicsOp> = serde_json::from_str(json).map_err(display_error)?;
        self.player
            .graphics_apply(&object_ref(target)?, &operations)
            .map_err(display_error)
    }
    pub fn graphics_copy_from(&mut self, target: &str, source: &str) -> Result<(), JsValue> {
        self.player
            .graphics_copy_from(&object_ref(target)?, &object_ref(source)?)
            .map_err(display_error)
    }
    pub fn object_button_state(&mut self, target: &str, state: u32) -> Result<bool, JsValue> {
        Ok(self
            .player
            .object_button_state(&self.project, &object_ref(target)?, state))
    }
    pub fn object_button_slot_json(&mut self, target: &str, slot: u8) -> Result<String, JsValue> {
        let value = self
            .player
            .object_button_slot(&self.project, &object_ref(target)?, slot)
            .map_err(display_error)?;
        serde_json::to_string(&value).map_err(display_error)
    }
    pub fn object_set_button_slot(
        &mut self,
        target: &str,
        slot: u8,
        value_json: &str,
    ) -> Result<bool, JsValue> {
        let value: Option<ObjectRef> = serde_json::from_str(value_json).map_err(display_error)?;
        self.player
            .object_set_button_slot(&self.project, &object_ref(target)?, slot, value)
            .map_err(display_error)
    }
    pub fn object_button_sound_transform_json(&self, target: &str) -> Result<String, JsValue> {
        let value = self
            .player
            .object_button_sound_transform(&self.project, &object_ref(target)?)
            .map_err(display_error)?;
        serde_json::to_string(&value).map_err(display_error)
    }
    pub fn object_set_button_sound_transform(
        &mut self,
        target: &str,
        volume: f64,
        pan: f64,
    ) -> Result<bool, JsValue> {
        self.player
            .object_set_button_sound_transform(&self.project, &object_ref(target)?, volume, pan)
            .map_err(display_error)
    }
    pub fn object_set_property(
        &mut self,
        target: &str,
        name: &str,
        value: f64,
    ) -> Result<bool, JsValue> {
        Ok(self
            .player
            .object_set_property(&object_ref(target)?, name, value))
    }
    pub fn object_mask(&self, target: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .object_mask(&object_ref(target)?)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn object_set_mask(&mut self, target: &str, source: &str) -> Result<(), JsValue> {
        if source.len() > 4096 {
            return Err(JsValue::from_str("Mask reference exceeds 4096 bytes"));
        }
        let source: Option<ObjectRef> = serde_json::from_str(source).map_err(display_error)?;
        self.player
            .object_set_mask(&self.project, &object_ref(target)?, source)
            .map_err(display_error)
    }
    pub fn object_scroll_rect(&self, target: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .object_scroll_rect(&object_ref(target)?)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn object_filters(&self, target: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .object_filters(&object_ref(target)?)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn object_set_filters(&mut self, target: &str, filters: &str) -> Result<(), JsValue> {
        self.player
            .object_set_filters(&object_ref(target)?, &filter_specs(filters)?)
            .map_err(display_error)
    }
    pub fn filter_padding(&self, filters: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &neoflash_core::filter_padding(&filter_specs(filters)?).map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn filter_rgba(
        &self,
        width: u32,
        height: u32,
        rgba: &[u8],
        filters: &str,
    ) -> Result<Vec<u8>, JsValue> {
        self.player
            .filter_rgba(width, height, rgba, &filter_specs(filters)?)
            .map_err(display_error)
    }
    pub fn object_set_scroll_rect(&mut self, target: &str, rect: &str) -> Result<(), JsValue> {
        if rect.len() > 4096 {
            return Err(JsValue::from_str("scrollRect exceeds 4096 bytes"));
        }
        let rect: Option<ScrollRect> = serde_json::from_str(rect).map_err(display_error)?;
        self.player
            .object_set_scroll_rect(&self.project, &object_ref(target)?, rect)
            .map_err(display_error)
    }
    pub fn object_matrix(&self, target: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .object_matrix(&self.project, &object_ref(target)?)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn object_set_matrix(&mut self, target: &str, matrix: &str) -> Result<bool, JsValue> {
        if matrix.len() > 4096 {
            return Err(JsValue::from_str("Matrix input exceeds 4096 bytes"));
        }
        let matrix: [f64; 6] = serde_json::from_str(matrix).map_err(display_error)?;
        self.player
            .object_set_matrix(&object_ref(target)?, matrix)
            .map_err(display_error)
    }
    pub fn object_color_transform(&self, target: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .object_color_transform(&self.project, &object_ref(target)?)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn object_set_color_transform(
        &mut self,
        target: &str,
        color: &str,
    ) -> Result<bool, JsValue> {
        self.player
            .object_set_color_transform(&object_ref(target)?, color_transform(color)?)
            .map_err(display_error)
    }
    pub fn object_property(&self, target: &str, name: &str) -> Result<f64, JsValue> {
        Ok(self
            .player
            .object_property(&self.project, &object_ref(target)?, name)
            .unwrap_or(f64::NAN))
    }
    pub fn object_set_visible(&mut self, target: &str, value: bool) -> Result<bool, JsValue> {
        Ok(self.player.object_set_visible(&object_ref(target)?, value))
    }
    pub fn object_visible(&self, target: &str) -> Result<bool, JsValue> {
        self.player
            .object_visible(&self.project, &object_ref(target)?)
            .ok_or_else(|| JsValue::from_str("Display object is no longer alive"))
    }
    pub fn object_set_name(&mut self, target: &str, name: &str) -> Result<bool, JsValue> {
        Ok(self.player.object_set_name(&object_ref(target)?, name))
    }
    pub fn object_set_text(&mut self, target: &str, text: &str) -> Result<bool, JsValue> {
        Ok(self
            .player
            .object_set_text(&self.project, &object_ref(target)?, text))
    }
    pub fn object_set_text_color(&mut self, target: &str, color: u32) -> Result<bool, JsValue> {
        Ok(self
            .player
            .object_set_text_color(&self.project, &object_ref(target)?, color))
    }
    pub fn object_control(
        &mut self,
        target: &str,
        name: &str,
        frame: u32,
    ) -> Result<bool, JsValue> {
        Ok(self
            .player
            .object_control(&self.project, &object_ref(target)?, command(name, frame)?))
    }
    pub fn object_frame(&self, target: &str) -> Result<u32, JsValue> {
        Ok(self.player.object_frame(&object_ref(target)?).unwrap_or(0))
    }
    pub fn object_frame_count(&self, target: &str) -> Result<u32, JsValue> {
        Ok(self
            .player
            .object_frame_count(&self.project, &object_ref(target)?)
            .unwrap_or(0))
    }
    pub fn object_metadata(&self, target: &str) -> Result<String, JsValue> {
        let metadata = self
            .player
            .object_metadata(&self.project, &object_ref(target)?)
            .ok_or_else(|| {
                JsValue::from_str("Timeline metadata requires a live local MovieClip.")
            })?;
        serde_json::to_string(&metadata).map_err(display_error)
    }
    pub fn object_resolve_frame(&self, target: &str, label: &str) -> Result<i32, JsValue> {
        Ok(self
            .player
            .object_resolve_frame(&self.project, &object_ref(target)?, label, false)
            .map_or(-1, |frame| frame as i32))
    }
    pub fn object_resolve_frame_as2(&self, target: &str, label: &str) -> Result<i32, JsValue> {
        Ok(self
            .player
            .object_resolve_frame(&self.project, &object_ref(target)?, label, true)
            .map_or(-1, |frame| frame as i32))
    }
    pub fn object_scene_count(&self, target: &str) -> Result<u32, JsValue> {
        Ok(self
            .player
            .object_scene_count(&self.project, &object_ref(target)?)
            .unwrap_or(0))
    }
    pub fn object_resolve_goto(&self, target: &str, request: &str) -> Result<i32, JsValue> {
        goto_result(self.player.object_resolve_goto(
            &self.project,
            &object_ref(target)?,
            &goto_request(request)?,
        ))
    }
    pub fn object_scene_step(&self, target: &str, step: i32) -> Result<i32, JsValue> {
        Ok(self
            .player
            .object_scene_step(&self.project, &object_ref(target)?, step)
            .map_or(-1, |frame| frame as i32))
    }
    pub fn set_morph_shape_ops(&mut self, target: &str, json: &str) -> Result<(), JsValue> {
        let ops: Vec<GraphicsOp> = serde_json::from_str(json).map_err(display_error)?;
        self.player
            .set_morph_shape_ops(&object_ref(target)?, &ops)
            .map_err(display_error)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn loader_replace_swf(
        &mut self,
        target: &str,
        as_version: u32,
        width: u32,
        height: u32,
        has_bg_color: bool,
        bg_color: u32,
        as2_replace: bool,
    ) -> Result<String, JsValue> {
        let result = self
            .player
            .loader_replace_swf(
                &self.project,
                &object_ref(target)?,
                as_version,
                width,
                height,
                has_bg_color.then_some(bg_color),
                as2_replace,
            )
            .map_err(display_error)?;
        serde_json::to_string(&result).map_err(display_error)
    }

    pub fn set_authored_display_events(&mut self, observe: bool) {
        self.player.set_authored_display_events(observe);
    }

    pub fn enable_lifecycle(&mut self) {
        self.player.enable_lifecycle();
    }

    pub fn reset_generation(&self) -> f64 {
        self.player.reset_generation() as f64
    }

    pub fn lifecycle_depth(&self) -> u32 {
        self.player.lifecycle_depth() as u32
    }

    pub fn begin_inner_goto(
        &mut self,
        target: &str,
        name: &str,
        frame: u32,
    ) -> Result<u32, JsValue> {
        let depth = self
            .player
            .begin_inner_goto(&self.project, &path(target)?, command(name, frame)?)
            .map_err(display_error)?;
        Ok(depth as u32)
    }

    pub fn object_begin_inner_goto(
        &mut self,
        target: &str,
        name: &str,
        frame: u32,
    ) -> Result<u32, JsValue> {
        let depth = self
            .player
            .object_begin_inner_goto(&self.project, &object_ref(target)?, command(name, frame)?)
            .map_err(display_error)?;
        Ok(depth as u32)
    }

    pub fn lifecycle_next_step_json(&mut self) -> Result<String, JsValue> {
        let step = self.player.lifecycle_next_step().map_err(display_error)?;
        serde_json::to_string(&step).map_err(display_error)
    }
}
