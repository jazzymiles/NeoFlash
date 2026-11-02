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
mod as3_collections;
mod as3_geom;
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

pub use as2_strings::{as2_string_json, legacy_case_name_json};

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
#[wasm_bindgen]
pub fn amf_decode(format: u8, bytes: &[u8]) -> Result<String, JsValue> {
    serde_json::to_string(&neoflash_core::amf_decode(format, bytes).map_err(display_error)?)
        .map_err(display_error)
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
) -> Result<Vec<f64>, TextError> {
    let strings = serde_json::to_string(strings).map_err(|e| TextError(e.to_string()))?;
    let format = serde_json::to_string(format).map_err(|e| TextError(e.to_string()))?;
    let result = measure
        .call2(
            &JsValue::NULL,
            &JsValue::from_str(&strings),
            &JsValue::from_str(&format),
        )
        .map_err(|e| {
            TextError(format!(
                "font measurement failed: {}",
                e.as_string()
                    .unwrap_or_else(|| "host callback error".into())
            ))
        })?;
    let json = result
        .as_string()
        .ok_or_else(|| TextError("font measurement must return JSON widths".into()))?;
    if json.len() > 4 * 1024 * 1024 {
        return Err(TextError("font measurement result exceeds 4 MiB".into()));
    }
    serde_json::from_str(&json).map_err(|e| TextError(format!("invalid font measurement: {e}")))
}

#[wasm_bindgen]
pub fn text_layout_json(field_json: &str, measure: &js_sys::Function) -> Result<String, JsValue> {
    if field_json.len() > 8 * 1024 * 1024 {
        return Err(JsValue::from_str("Text layout input exceeds 8 MiB"));
    }
    let field: TextFieldState = serde_json::from_str(field_json).map_err(display_error)?;
    let layout = layout_text(&field, |strings, format| {
        invoke_text_measure(measure, strings, format)
    })
    .map_err(display_error)?;
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
    let result = query_text(&field, &query, |strings, format| {
        invoke_text_measure(measure, strings, format)
    })
    .map_err(display_error)?;
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
        let authored =
            Project::from_json(json).map_err(|error| JsValue::from_str(&error.to_string()))?;
        let (project, mut ranges) = neoflash_core::flatten_scenes_with_ranges(&authored);
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
        Ok(Self { project, player })
    }

    pub fn amf_encode(&self, format: u8, graph_json: &str) -> Result<Vec<u8>, JsValue> {
        amf_encode(format, graph_json)
    }
    pub fn amf_decode(&self, format: u8, bytes: &[u8]) -> Result<String, JsValue> {
        amf_decode(format, bytes)
    }
    pub fn sol_encode(&self, format: u8, name: &str, graph_json: &str) -> Result<Vec<u8>, JsValue> {
        sol_encode(format, name, graph_json)
    }
    pub fn sol_decode(&self, bytes: &[u8]) -> Result<String, JsValue> {
        sol_decode(bytes)
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
    }
    pub fn script_overflowed(&self) -> bool {
        self.player.script_overflowed()
    }
    /// Timeline sound commands (`SoundCommand`, tagged by `op`) since the last call.
    pub fn sound_commands_json(&mut self) -> Result<String, JsValue> {
        serde_json::to_string(&self.player.drain_sound_commands()).map_err(display_error)
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
        portable_measure_text_json(strings_json, format_json)
    }
    pub fn parse_html_text_json(&self, field_json: &str, html: &str) -> Result<String, JsValue> {
        parse_html_text_json(field_json, html)
    }
    pub fn serialize_html_text_json(&self, field_json: &str) -> Result<String, JsValue> {
        serialize_html_text_json(field_json)
    }
    pub fn text_to_outlines_json(&self, field_json: &str) -> Result<String, JsValue> {
        text_to_outlines_json(field_json)
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
    pub fn bitmap_dispose(&mut self, target: &str) -> Result<(), JsValue> {
        self.player
            .bitmap_dispose(&pixel_ref(target)?)
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
}

#[wasm_bindgen]
pub struct AirKernel {
    inner: neoflash_core::as3_air::AirRuntime,
}

fn air_js_err(err: neoflash_core::as3_air::AirError) -> JsValue {
    JsValue::from_str(&err.to_string())
}

fn air_sql_js_err(err: neoflash_core::as3_air::AirSqlError) -> JsValue {
    JsValue::from_str(&err.to_json())
}

#[wasm_bindgen]
impl AirKernel {
    #[wasm_bindgen(constructor)]
    pub fn new(
        project_name: &str,
        width: u32,
        height: u32,
        resources_json: &str,
        now_ms: f64,
    ) -> Result<AirKernel, JsValue> {
        let resources: Vec<neoflash_core::as3_air::AirResourceInit> =
            if resources_json.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(resources_json)
                    .map_err(|e| JsValue::from_str(&format!("Invalid AIR resources JSON: {e}")))?
            };
        let inner = neoflash_core::as3_air::AirRuntime::new(
            project_name,
            width,
            height,
            &resources,
            now_ms,
        )
        .map_err(air_js_err)?;
        Ok(Self { inner })
    }

    pub fn app_id(&self) -> String {
        self.inner.app_id.clone()
    }

    pub fn descriptor_xml(&self) -> String {
        self.inner.descriptor_xml.clone()
    }

    pub fn normalize_path(&self, input: &str) -> Result<String, JsValue> {
        neoflash_core::as3_air::normalize_air_path(input).map_err(air_js_err)
    }

    pub fn resolve_path(&self, base_path: &str, relative: &str) -> Result<String, JsValue> {
        neoflash_core::as3_air::resolve_air_path(base_path, relative).map_err(air_js_err)
    }

    pub fn relative_path(
        &self,
        from_path: &str,
        to_path: &str,
        use_dot_dot: bool,
    ) -> Result<Option<String>, JsValue> {
        neoflash_core::as3_air::relative_air_path(from_path, to_path, use_dot_dot)
            .map_err(air_js_err)
    }

    pub fn canonicalize_path(&self, path: &str) -> Result<String, JsValue> {
        self.inner.fs.canonicalize(path).map_err(air_js_err)
    }

    pub fn path_info_json(&self, path: &str) -> Result<String, JsValue> {
        let info = self.inner.fs.info(path).map_err(air_js_err)?;
        serde_json::to_string(&info).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    pub fn fs_create_directory(&mut self, path: &str, now_ms: f64) -> Result<(), JsValue> {
        self.inner
            .fs
            .create_directory(path, now_ms)
            .map_err(air_js_err)
    }

    pub fn fs_create_temp(&mut self, is_directory: bool, now_ms: f64) -> Result<String, JsValue> {
        self.inner
            .fs
            .create_temp(is_directory, now_ms)
            .map_err(air_js_err)
    }

    pub fn fs_list_directory_json(&self, path: &str) -> Result<String, JsValue> {
        let items = self.inner.fs.list_directory(path).map_err(air_js_err)?;
        serde_json::to_string(&items).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    pub fn fs_delete_file(&mut self, path: &str) -> Result<(), JsValue> {
        self.inner.fs.delete_file(path).map_err(air_js_err)
    }

    pub fn fs_delete_directory(
        &mut self,
        path: &str,
        delete_contents: bool,
    ) -> Result<(), JsValue> {
        self.inner
            .fs
            .delete_directory(path, delete_contents)
            .map_err(air_js_err)
    }

    pub fn fs_copy(
        &mut self,
        src: &str,
        dst: &str,
        overwrite: bool,
        now_ms: f64,
    ) -> Result<(), JsValue> {
        self.inner
            .fs
            .copy_to(src, dst, overwrite, now_ms)
            .map_err(air_js_err)
    }

    pub fn fs_move(
        &mut self,
        src: &str,
        dst: &str,
        overwrite: bool,
        now_ms: f64,
    ) -> Result<(), JsValue> {
        self.inner
            .fs
            .move_to(src, dst, overwrite, now_ms)
            .map_err(air_js_err)
    }

    pub fn fs_open_stream(
        &mut self,
        path: &str,
        mode: &str,
        now_ms: f64,
    ) -> Result<Vec<u8>, JsValue> {
        self.inner
            .fs
            .open_stream(path, mode, now_ms)
            .map_err(air_js_err)
    }

    pub fn fs_write_bytes(&mut self, path: &str, data: &[u8], now_ms: f64) -> Result<(), JsValue> {
        self.inner
            .fs
            .write_bytes(path, data, now_ms)
            .map_err(air_js_err)
    }

    pub fn els_set(&mut self, name: &str, bytes: &[u8]) -> Result<(), JsValue> {
        self.inner.els.set_item(name, bytes).map_err(air_js_err)
    }

    pub fn els_get(&self, name: &str) -> Result<Option<Vec<u8>>, JsValue> {
        self.inner.els.get_item(name).map_err(air_js_err)
    }

    pub fn els_remove(&mut self, name: &str) -> Result<(), JsValue> {
        self.inner.els.remove_item(name).map_err(air_js_err)
    }

    pub fn els_reset(&mut self) {
        self.inner.els.reset();
    }

    pub fn sql_open(
        &mut self,
        reference_path: Option<String>,
        open_mode: &str,
        auto_compact: bool,
        page_size: u32,
        now_ms: f64,
    ) -> Result<u32, JsValue> {
        self.inner
            .sql_open(
                reference_path.as_deref(),
                open_mode,
                auto_compact,
                page_size,
                now_ms,
            )
            .map_err(air_sql_js_err)
    }

    pub fn sql_close(&mut self, conn_id: u32) -> Result<(), JsValue> {
        self.inner.sql_close(conn_id).map_err(air_sql_js_err)
    }

    pub fn sql_attach(
        &mut self,
        conn_id: u32,
        name: &str,
        reference_path: Option<String>,
        now_ms: f64,
    ) -> Result<(), JsValue> {
        self.inner
            .sql_attach(conn_id, name, reference_path.as_deref(), now_ms)
            .map_err(air_sql_js_err)
    }

    pub fn sql_detach(&mut self, conn_id: u32, name: &str) -> Result<(), JsValue> {
        self.inner.sql_detach(conn_id, name).map_err(air_sql_js_err)
    }

    pub fn sql_begin(&mut self, conn_id: u32, option: Option<String>) -> Result<(), JsValue> {
        self.inner
            .sql_begin(conn_id, option.as_deref())
            .map_err(air_sql_js_err)
    }

    pub fn sql_commit(&mut self, conn_id: u32, now_ms: f64) -> Result<(), JsValue> {
        self.inner
            .sql_commit(conn_id, now_ms)
            .map_err(air_sql_js_err)
    }

    pub fn sql_rollback(&mut self, conn_id: u32) -> Result<(), JsValue> {
        self.inner.sql_rollback(conn_id).map_err(air_sql_js_err)
    }

    pub fn sql_set_savepoint(
        &mut self,
        conn_id: u32,
        name: Option<String>,
    ) -> Result<String, JsValue> {
        self.inner
            .sql_set_savepoint(conn_id, name.as_deref())
            .map_err(air_sql_js_err)
    }

    pub fn sql_release_savepoint(
        &mut self,
        conn_id: u32,
        name: Option<String>,
        now_ms: f64,
    ) -> Result<(), JsValue> {
        self.inner
            .sql_release_savepoint(conn_id, name.as_deref(), now_ms)
            .map_err(air_sql_js_err)
    }

    pub fn sql_rollback_to_savepoint(
        &mut self,
        conn_id: u32,
        name: Option<String>,
    ) -> Result<(), JsValue> {
        self.inner
            .sql_rollback_to_savepoint(conn_id, name.as_deref())
            .map_err(air_sql_js_err)
    }

    pub fn sql_compact(&mut self, conn_id: u32, now_ms: f64) -> Result<(), JsValue> {
        self.inner
            .sql_compact(conn_id, now_ms)
            .map_err(air_sql_js_err)
    }

    pub fn sql_analyze(&self, conn_id: u32, resource_name: Option<String>) -> Result<(), JsValue> {
        self.inner
            .sql_analyze(conn_id, resource_name.as_deref())
            .map_err(air_sql_js_err)
    }

    pub fn sql_deanalyze(&self, conn_id: u32) -> Result<(), JsValue> {
        self.inner.sql_deanalyze(conn_id).map_err(air_sql_js_err)
    }

    pub fn sql_state_json(&self, conn_id: u32) -> Result<String, JsValue> {
        self.inner.sql_state_json(conn_id).map_err(air_sql_js_err)
    }

    pub fn sql_set_config(
        &mut self,
        conn_id: u32,
        cache_size: Option<u32>,
        column_name_style: Option<String>,
    ) -> Result<(), JsValue> {
        self.inner
            .sql_set_config(conn_id, cache_size, column_name_style.as_deref())
            .map_err(air_sql_js_err)
    }

    pub fn sql_load_schema_json(
        &self,
        conn_id: u32,
        schema_type: Option<String>,
        name: Option<String>,
        database: &str,
        include_columns: bool,
    ) -> Result<String, JsValue> {
        self.inner
            .sql_load_schema_json(
                conn_id,
                schema_type.as_deref(),
                name.as_deref(),
                database,
                include_columns,
            )
            .map_err(air_sql_js_err)
    }

    pub fn sql_execute_json(
        &mut self,
        conn_id: u32,
        sql_text: &str,
        params_json: &str,
        now_ms: f64,
    ) -> Result<String, JsValue> {
        self.inner
            .sql_execute_json(conn_id, sql_text, params_json, now_ms)
            .map_err(air_sql_js_err)
    }

    pub fn sql_open_encrypted(
        &mut self,
        reference_path: Option<String>,
        open_mode: &str,
        auto_compact: bool,
        page_size: u32,
        encryption_key: &[u8],
        now_ms: f64,
    ) -> Result<u32, JsValue> {
        let key_opt = if encryption_key.is_empty() {
            None
        } else {
            Some(encryption_key)
        };
        self.inner
            .sql_open_with_key(
                reference_path.as_deref(),
                open_mode,
                auto_compact,
                page_size,
                key_opt,
                now_ms,
            )
            .map_err(air_sql_js_err)
    }

    pub fn sql_attach_encrypted(
        &mut self,
        conn_id: u32,
        name: &str,
        reference_path: Option<String>,
        encryption_key: &[u8],
        now_ms: f64,
    ) -> Result<(), JsValue> {
        let key_opt = if encryption_key.is_empty() {
            None
        } else {
            Some(encryption_key)
        };
        self.inner
            .sql_attach_with_key(conn_id, name, reference_path.as_deref(), key_opt, now_ms)
            .map_err(air_sql_js_err)
    }

    pub fn sql_reencrypt(
        &mut self,
        conn_id: u32,
        new_encryption_key: &[u8],
        now_ms: f64,
    ) -> Result<(), JsValue> {
        self.inner
            .sql_reencrypt(conn_id, new_encryption_key, now_ms)
            .map_err(air_sql_js_err)
    }

    pub fn html_create(&mut self, width: f64, height: f64) -> u32 {
        self.inner.html.create_session(width, height)
    }

    pub fn html_dispose(&mut self, id: u32) {
        self.inner.html.dispose_session(id);
    }

    pub fn html_load_string(
        &mut self,
        id: u32,
        html: &str,
        base_url: &str,
    ) -> Result<String, JsValue> {
        self.inner
            .html
            .load_string(id, html, base_url)
            .map_err(air_js_err)
    }

    pub fn html_load_url(
        &mut self,
        id: u32,
        url: &str,
        is_post: bool,
        fallback_html: Option<String>,
    ) -> Result<String, JsValue> {
        self.inner
            .html_load_url(id, url, is_post, fallback_html.as_deref())
            .map_err(air_js_err)
    }

    pub fn html_history_go(&mut self, id: u32, steps: i32) -> Result<String, JsValue> {
        self.inner.html.history_go(id, steps).map_err(air_js_err)
    }

    pub fn html_reload(&mut self, id: u32) -> Result<String, JsValue> {
        self.inner.html.reload(id).map_err(air_js_err)
    }

    pub fn html_cancel(&mut self, id: u32) -> Result<String, JsValue> {
        self.inner.html.cancel(id).map_err(air_js_err)
    }

    pub fn html_set_viewport(
        &mut self,
        id: u32,
        width: f64,
        height: f64,
        scroll_h: f64,
        scroll_v: f64,
    ) -> Result<String, JsValue> {
        self.inner
            .html
            .set_viewport(id, width, height, scroll_h, scroll_v)
            .map_err(air_js_err)
    }

    pub fn html_state_json(&self, id: u32) -> Result<String, JsValue> {
        self.inner.html.state_json(id).map_err(air_js_err)
    }

    pub fn net_interfaces_json(&self) -> String {
        self.inner.net.interfaces_json()
    }

    pub fn net_set_login_credentials(
        &mut self,
        hostname: &str,
        user: &str,
        password: &str,
    ) -> String {
        self.inner
            .net
            .set_login_credentials(hostname, user, password)
    }

    pub fn net_get_login_auth_header(&self, hostname: &str) -> Option<String> {
        self.inner.net.get_login_auth_header(hostname)
    }

    pub fn udp_create(&mut self) -> u32 {
        self.inner.net.udp_create()
    }

    pub fn udp_bind(&mut self, sock_id: u32, port: u32, address: &str) -> Result<String, JsValue> {
        self.inner
            .net
            .udp_bind(sock_id, port, address)
            .map_err(air_js_err)
    }

    pub fn udp_connect(
        &mut self,
        sock_id: u32,
        remote_address: &str,
        remote_port: u32,
    ) -> Result<String, JsValue> {
        self.inner
            .net
            .udp_connect(sock_id, remote_address, remote_port)
            .map_err(air_js_err)
    }

    pub fn udp_receive(&mut self, sock_id: u32) -> Result<(), JsValue> {
        self.inner.net.udp_receive(sock_id).map_err(air_js_err)
    }

    pub fn udp_send(
        &mut self,
        sock_id: u32,
        target_address: Option<String>,
        target_port: u32,
    ) -> Result<String, JsValue> {
        self.inner
            .net
            .udp_send(sock_id, target_address.as_deref(), target_port)
            .map_err(air_js_err)
    }

    pub fn udp_close(&mut self, sock_id: u32) {
        self.inner.net.udp_close(sock_id);
    }

    pub fn tcp_server_create(&mut self) -> u32 {
        self.inner.net.tcp_server_create()
    }

    pub fn tcp_server_bind(
        &mut self,
        server_id: u32,
        port: u32,
        address: &str,
    ) -> Result<String, JsValue> {
        self.inner
            .net
            .tcp_server_bind(server_id, port, address)
            .map_err(air_js_err)
    }

    pub fn tcp_server_listen(&mut self, server_id: u32, backlog: u32) -> Result<(), JsValue> {
        self.inner
            .net
            .tcp_server_listen(server_id, backlog)
            .map_err(air_js_err)
    }

    pub fn tcp_server_close(&mut self, server_id: u32) {
        self.inner.net.tcp_server_close(server_id);
    }

    pub fn tcp_connect_loopback(&mut self, host: &str, port: u32) -> String {
        self.inner.net.tcp_connect_loopback(host, port)
    }

    pub fn tcp_write(&mut self, conn_id: u32, data: &[u8]) -> Result<u32, JsValue> {
        self.inner.net.tcp_write(conn_id, data).map_err(air_js_err)
    }

    pub fn tcp_read(&mut self, conn_id: u32) -> Vec<u8> {
        self.inner.net.tcp_read(conn_id)
    }

    pub fn tcp_endpoint_json(&self, conn_id: u32) -> String {
        self.inner.net.tcp_endpoint_json(conn_id)
    }

    pub fn tcp_close(&mut self, conn_id: u32) -> u32 {
        self.inner.net.tcp_close(conn_id)
    }

    pub fn xml_sig_add_cert(&mut self, cert_bytes: &[u8], trusted: bool) -> Result<(), JsValue> {
        self.inner
            .xml_sig
            .add_certificate(cert_bytes, trusted)
            .map_err(air_js_err)
    }

    pub fn xml_sig_verify_json(
        &self,
        signature_xml: &str,
        deref_map_json: &str,
        references_setting: &str,
        revocation_setting: &str,
        use_system_trust_store: bool,
    ) -> String {
        self.inner.xml_sig.verify_json(
            signature_xml,
            deref_map_json,
            references_setting,
            revocation_setting,
            use_system_trust_store,
        )
    }

    pub fn sha256(&self, data: &[u8]) -> Vec<u8> {
        neoflash_core::as3_air::sha256_bytes(data).to_vec()
    }

    pub fn sha1(&self, data: &[u8]) -> Vec<u8> {
        neoflash_core::as3_air::sha1_bytes(data).to_vec()
    }

    pub fn hmac_sha256(&self, key: &[u8], data: &[u8]) -> Vec<u8> {
        neoflash_core::as3_air::hmac_sha256(key, data).to_vec()
    }

    pub fn xml_c14n(&self, xml_fragment: &str) -> String {
        neoflash_core::as3_air::canonicalize_xml_fragment(xml_fragment)
    }

    pub fn device_rotation_quaternion(
        &self,
        alpha_deg: f64,
        beta_deg: f64,
        gamma_deg: f64,
    ) -> Vec<f64> {
        neoflash_core::as3_air::device_rotation_quaternion(alpha_deg, beta_deg, gamma_deg)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rasterize_text_rgba(
        &self,
        width: u32,
        height: u32,
        text: &str,
        fg_rgb: u32,
        bg_argb: u32,
        font_size: u32,
        flags_align: &str,
    ) -> Vec<u8> {
        neoflash_core::as3_air::rasterize_text_rgba(
            width,
            height,
            text,
            fg_rgb,
            bg_argb,
            font_size,
            flags_align,
        )
    }
}

#[wasm_bindgen]
pub struct As3NetSysKernel {
    inner: neoflash_core::as3_net_sys::As3NetSysKernel,
}

impl Default for As3NetSysKernel {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl As3NetSysKernel {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: neoflash_core::as3_net_sys::As3NetSysKernel::new(),
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
    }

    pub fn execute_json(&mut self, request_json: &str) -> Result<String, JsValue> {
        self.inner
            .execute_json(request_json)
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn random_bytes(&mut self, count: u32, entropy: &[u8]) -> Result<Vec<u8>, JsValue> {
        self.inner
            .random_bytes(count, entropy)
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn flv_append(&mut self, stream_id: u32, bytes: &[u8]) -> String {
        self.inner.flv_append(stream_id, bytes)
    }

    pub fn xml_socket_feed(&mut self, socket_id: u32, bytes: &[u8]) -> String {
        self.inner.xml_socket_feed(socket_id, bytes)
    }

    pub fn parse_x509(&self, bytes: &[u8]) -> Result<String, JsValue> {
        self.inner
            .parse_x509(bytes)
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn loopback_x509_der(&self, host: &str) -> Vec<u8> {
        self.inner.loopback_x509_der(host)
    }
}

#[wasm_bindgen]
pub struct As3Fte3dKernel {
    inner: neoflash_core::as3_fte_3d::Fte3dRuntime,
}

fn fte3d_js_err(err: neoflash_core::as3_fte_3d::Fte3dError) -> JsValue {
    JsValue::from_str(&err.to_string())
}

#[wasm_bindgen]
impl As3Fte3dKernel {
    #[wasm_bindgen(constructor)]
    pub fn new() -> As3Fte3dKernel {
        Self {
            inner: neoflash_core::as3_fte_3d::Fte3dRuntime::new(),
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
    }

    // ---- Flash Text Engine ----
    pub fn validate_fte_enum(&self, kind: &str, value: &str) -> Result<String, JsValue> {
        neoflash_core::as3_fte_3d::validate_fte_enum(kind, value)
            .map(str::to_owned)
            .map_err(fte3d_js_err)
    }

    pub fn compute_font_metrics_json(
        &self,
        font_name: &str,
        font_size: f64,
        font_weight: &str,
        font_posture: &str,
    ) -> Result<String, JsValue> {
        let m = neoflash_core::as3_fte_3d::compute_font_metrics(
            font_name,
            font_weight,
            font_posture,
            font_size,
        )
        .map_err(fte3d_js_err)?;
        serde_json::to_string(&m).map_err(display_error)
    }

    pub fn is_font_compatible(
        &self,
        font_name: &str,
        font_weight: &str,
        font_posture: &str,
    ) -> bool {
        neoflash_core::as3_fte_3d::is_font_compatible(font_name, font_weight, font_posture, true)
            .unwrap_or(false)
    }

    pub fn justifier_kind_for_locale(&self, locale: &str) -> Result<String, JsValue> {
        neoflash_core::as3_fte_3d::justifier_kind_for_locale(locale)
            .map(str::to_owned)
            .map_err(fte3d_js_err)
    }

    pub fn layout_text_line_json(&self, req_json: &str) -> Result<String, JsValue> {
        let req: neoflash_core::as3_fte_3d::FteLayoutRequest = serde_json::from_str(req_json)
            .map_err(|e| {
                JsValue::from_str(&format!("ArgumentError: Invalid FTE layout request: {e}"))
            })?;
        let resp = neoflash_core::as3_fte_3d::layout_text_line(&req).map_err(fte3d_js_err)?;
        serde_json::to_string(&resp).map_err(display_error)
    }

    pub fn find_next_atom_boundary(
        &self,
        raw_text: &str,
        after_char_index: i32,
    ) -> Result<i32, JsValue> {
        neoflash_core::as3_fte_3d::find_next_atom_boundary(raw_text, after_char_index)
            .map_err(fte3d_js_err)
    }

    pub fn find_previous_atom_boundary(
        &self,
        raw_text: &str,
        before_char_index: i32,
    ) -> Result<i32, JsValue> {
        neoflash_core::as3_fte_3d::find_previous_atom_boundary(raw_text, before_char_index)
            .map_err(fte3d_js_err)
    }

    pub fn find_next_word_boundary(
        &self,
        raw_text: &str,
        after_char_index: i32,
    ) -> Result<i32, JsValue> {
        neoflash_core::as3_fte_3d::find_next_word_boundary(raw_text, after_char_index)
            .map_err(fte3d_js_err)
    }

    pub fn find_previous_word_boundary(
        &self,
        raw_text: &str,
        before_char_index: i32,
    ) -> Result<i32, JsValue> {
        neoflash_core::as3_fte_3d::find_previous_word_boundary(raw_text, before_char_index)
            .map_err(fte3d_js_err)
    }

    // ---- Stage3D ----
    pub fn validate_stage3d_enum(&self, kind: &str, value: &str) -> Result<String, JsValue> {
        neoflash_core::as3_fte_3d::validate_stage3d_enum(kind, value)
            .map(str::to_owned)
            .map_err(fte3d_js_err)
    }

    pub fn create_context(&mut self, render_mode: &str, profile: &str) -> Result<u32, JsValue> {
        self.inner
            .stage3d
            .create_context(render_mode, profile)
            .map_err(fte3d_js_err)
    }

    pub fn dispose_context(&mut self, ctx_id: u32) {
        self.inner.stage3d.dispose_context(ctx_id);
    }

    pub fn total_gpu_memory(&self, ctx_id: u32) -> f64 {
        self.inner.stage3d.total_gpu_memory(ctx_id)
    }

    pub fn configure_back_buffer(
        &mut self,
        ctx_id: u32,
        width: i32,
        height: i32,
        anti_alias: i32,
        enable_depth_and_stencil: bool,
    ) -> Result<(), JsValue> {
        self.inner
            .stage3d
            .configure_back_buffer(ctx_id, width, height, anti_alias, enable_depth_and_stencil)
            .map_err(fte3d_js_err)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn clear(
        &mut self,
        ctx_id: u32,
        red: f64,
        green: f64,
        blue: f64,
        alpha: f64,
        depth: f64,
        stencil: u32,
        mask: u32,
    ) -> Result<(), JsValue> {
        self.inner
            .stage3d
            .clear(ctx_id, red, green, blue, alpha, depth, stencil, mask)
            .map_err(fte3d_js_err)
    }

    pub fn create_vertex_buffer(
        &mut self,
        ctx_id: u32,
        num_vertices: i32,
        data32_per_vertex: i32,
        usage: &str,
    ) -> Result<u32, JsValue> {
        self.inner
            .stage3d
            .create_vertex_buffer(ctx_id, num_vertices, data32_per_vertex, usage)
            .map_err(fte3d_js_err)
    }

    pub fn upload_vertex_buffer(
        &mut self,
        vb_id: u32,
        floats: &[f32],
        start_vertex: i32,
        num_vertices: i32,
    ) -> Result<(), JsValue> {
        self.inner
            .stage3d
            .upload_vertex_buffer(vb_id, floats, start_vertex, num_vertices)
            .map_err(fte3d_js_err)
    }

    pub fn dispose_vertex_buffer(&mut self, vb_id: u32) {
        self.inner.stage3d.dispose_vertex_buffer(vb_id);
    }

    pub fn create_index_buffer(
        &mut self,
        ctx_id: u32,
        num_indices: i32,
        usage: &str,
    ) -> Result<u32, JsValue> {
        self.inner
            .stage3d
            .create_index_buffer(ctx_id, num_indices, usage)
            .map_err(fte3d_js_err)
    }

    pub fn upload_index_buffer(
        &mut self,
        ib_id: u32,
        indices: &[u16],
        start_offset: i32,
        count: i32,
    ) -> Result<(), JsValue> {
        self.inner
            .stage3d
            .upload_index_buffer(ib_id, indices, start_offset, count)
            .map_err(fte3d_js_err)
    }

    pub fn dispose_index_buffer(&mut self, ib_id: u32) {
        self.inner.stage3d.dispose_index_buffer(ib_id);
    }

    pub fn create_program(&mut self, ctx_id: u32) -> Result<u32, JsValue> {
        self.inner
            .stage3d
            .create_program(ctx_id)
            .map_err(fte3d_js_err)
    }

    pub fn upload_program(
        &mut self,
        prog_id: u32,
        vertex_bytes: &[u8],
        fragment_bytes: &[u8],
    ) -> Result<(), JsValue> {
        self.inner
            .stage3d
            .upload_program(prog_id, vertex_bytes, fragment_bytes)
            .map_err(fte3d_js_err)
    }

    pub fn dispose_program(&mut self, prog_id: u32) {
        self.inner.stage3d.dispose_program(prog_id);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_texture(
        &mut self,
        ctx_id: u32,
        kind: &str,
        width: i32,
        height: i32,
        format: &str,
        optimize_for_rtt: bool,
        streaming_levels: i32,
    ) -> Result<u32, JsValue> {
        let tex_kind = match kind {
            "2d" => neoflash_core::as3_fte_3d::TextureKind::TwoD,
            "cube" => neoflash_core::as3_fte_3d::TextureKind::Cube,
            "rectangle" => neoflash_core::as3_fte_3d::TextureKind::Rectangle,
            "video" => neoflash_core::as3_fte_3d::TextureKind::Video,
            _ => {
                return Err(JsValue::from_str(&format!(
                    "ArgumentError: Unknown Stage3D texture kind '{kind}'."
                )));
            }
        };
        if streaming_levels < 0 {
            return Err(JsValue::from_str(
                "ArgumentError: streamingLevels must be non-negative.",
            ));
        }
        self.inner
            .stage3d
            .create_texture(ctx_id, tex_kind, width, height, format, optimize_for_rtt)
            .map_err(fte3d_js_err)
    }

    pub fn upload_texture_rgba(
        &mut self,
        tex_id: u32,
        side: u32,
        miplevel: u32,
        rgba: &[u8],
    ) -> Result<(), JsValue> {
        self.inner
            .stage3d
            .upload_texture_mip_rgba(tex_id, side as usize, miplevel, rgba)
            .map_err(fte3d_js_err)
    }

    pub fn upload_compressed_texture(
        &mut self,
        tex_id: u32,
        bytes: &[u8],
        byte_offset: usize,
    ) -> Result<(), JsValue> {
        if byte_offset > bytes.len() {
            return Err(JsValue::from_str(
                "RangeError: byteArrayOffset is out of bounds.",
            ));
        }
        self.inner
            .stage3d
            .upload_compressed_texture(tex_id, &bytes[byte_offset..])
            .map_err(fte3d_js_err)
    }

    pub fn dispose_texture(&mut self, tex_id: u32) {
        self.inner.stage3d.dispose_texture(tex_id);
    }

    pub fn set_state_json(
        &mut self,
        ctx_id: u32,
        op: &str,
        args_json: &str,
    ) -> Result<(), JsValue> {
        let args: serde_json::Value = serde_json::from_str(args_json).map_err(|e| {
            JsValue::from_str(&format!("ArgumentError: Invalid Stage3D state JSON: {e}"))
        })?;
        self.inner
            .stage3d
            .set_state_cmd(ctx_id, op, &args)
            .map_err(fte3d_js_err)
    }

    pub fn draw_triangles(
        &mut self,
        ctx_id: u32,
        ib_id: u32,
        first_index: i32,
        num_triangles: i32,
    ) -> Result<(), JsValue> {
        self.inner
            .stage3d
            .draw_triangles(ctx_id, ib_id, first_index, num_triangles)
            .map_err(fte3d_js_err)
    }

    pub fn present(&mut self, ctx_id: u32) -> Result<(), JsValue> {
        self.inner.stage3d.present(ctx_id).map_err(fte3d_js_err)
    }

    pub fn presented_width(&self, ctx_id: u32) -> Result<u32, JsValue> {
        let (w, _, _) = self
            .inner
            .stage3d
            .presented_rgba(ctx_id)
            .map_err(fte3d_js_err)?;
        Ok(w)
    }

    pub fn presented_height(&self, ctx_id: u32) -> Result<u32, JsValue> {
        let (_, h, _) = self
            .inner
            .stage3d
            .presented_rgba(ctx_id)
            .map_err(fte3d_js_err)?;
        Ok(h)
    }

    pub fn presented_rgba(&self, ctx_id: u32) -> Result<Vec<u8>, JsValue> {
        let (_, _, rgba) = self
            .inner
            .stage3d
            .presented_rgba(ctx_id)
            .map_err(fte3d_js_err)?;
        Ok(rgba)
    }

    pub fn inspect_swf_bytes_json(&self, bytes: &[u8]) -> Result<String, JsValue> {
        let info = neoflash_core::as3_fte_3d::inspect_swf_bytes(bytes).map_err(fte3d_js_err)?;
        serde_json::to_string(&info).map_err(display_error)
    }

    pub fn backbuffer_width(&self, ctx_id: u32) -> Result<u32, JsValue> {
        let (w, _, _) = self
            .inner
            .stage3d
            .backbuffer_rgba(ctx_id)
            .map_err(fte3d_js_err)?;
        Ok(w)
    }

    pub fn backbuffer_height(&self, ctx_id: u32) -> Result<u32, JsValue> {
        let (_, h, _) = self
            .inner
            .stage3d
            .backbuffer_rgba(ctx_id)
            .map_err(fte3d_js_err)?;
        Ok(h)
    }

    pub fn backbuffer_rgba(&self, ctx_id: u32) -> Result<Vec<u8>, JsValue> {
        let (_, _, rgba) = self
            .inner
            .stage3d
            .backbuffer_rgba(ctx_id)
            .map_err(fte3d_js_err)?;
        Ok(rgba)
    }

    // ---- Pixel Bender Shaders ----
    pub fn validate_shader_precision(&self, value: &str) -> Result<String, JsValue> {
        neoflash_core::as3_fte_3d::validate_shader_precision(value)
            .map(str::to_owned)
            .map_err(fte3d_js_err)
    }

    pub fn parse_shader_bytecode_json(&self, bytes: &[u8]) -> Result<String, JsValue> {
        let meta = neoflash_core::as3_fte_3d::parse_shader_bytecode(bytes).map_err(fte3d_js_err)?;
        serde_json::to_string(&meta).map_err(display_error)
    }

    pub fn evaluate_shader_json(&self, req_json: &str) -> Result<String, JsValue> {
        let req: neoflash_core::as3_fte_3d::ShaderEvalRequest = serde_json::from_str(req_json)
            .map_err(|e| {
                JsValue::from_str(&format!("ArgumentError: Invalid ShaderJob request: {e}"))
            })?;
        let res = neoflash_core::as3_fte_3d::evaluate_shader(&req).map_err(fte3d_js_err)?;
        serde_json::to_string(&res).map_err(display_error)
    }

    // ---- 3D DisplayObject & Transform ----
    pub fn compose_display_matrix3d_json(&self, pose_json: &str) -> Result<String, JsValue> {
        let pose: neoflash_core::as3_fte_3d::Display3dPose = serde_json::from_str(pose_json)
            .map_err(|e| JsValue::from_str(&format!("ArgumentError: Invalid 3D pose: {e}")))?;
        let raw = neoflash_core::as3_fte_3d::compose_display_matrix3d(&pose);
        serde_json::to_string(&raw).map_err(display_error)
    }

    pub fn decompose_display_matrix3d_json(&self, raw_json: &str) -> Result<String, JsValue> {
        let raw: [f64; 16] = serde_json::from_str(raw_json).map_err(|e| {
            JsValue::from_str(&format!("ArgumentError: Invalid Matrix3D array: {e}"))
        })?;
        let pose = neoflash_core::as3_fte_3d::decompose_display_matrix3d(&raw);
        serde_json::to_string(&pose).map_err(display_error)
    }

    pub fn relative_matrix_3d_json(
        &self,
        target_chain_json: &str,
        relative_chain_json: &str,
    ) -> Result<String, JsValue> {
        let target: Vec<[f64; 16]> = serde_json::from_str(target_chain_json).map_err(|e| {
            JsValue::from_str(&format!("ArgumentError: Invalid target 3D chain: {e}"))
        })?;
        let rel: Vec<[f64; 16]> = serde_json::from_str(relative_chain_json).map_err(|e| {
            JsValue::from_str(&format!("ArgumentError: Invalid relativeTo 3D chain: {e}"))
        })?;
        let out = neoflash_core::as3_fte_3d::relative_matrix_3d(&target, &rel);
        serde_json::to_string(&out).map_err(display_error)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn local3d_to_global_json(
        &self,
        chain_json: &str,
        x: f64,
        y: f64,
        z: f64,
        focal_length: f64,
        cx: f64,
        cy: f64,
    ) -> Result<String, JsValue> {
        let chain: Vec<[f64; 16]> = serde_json::from_str(chain_json)
            .map_err(|e| JsValue::from_str(&format!("ArgumentError: Invalid 3D chain: {e}")))?;
        let pt =
            neoflash_core::as3_fte_3d::local3d_to_global(&chain, [x, y, z], focal_length, [cx, cy]);
        serde_json::to_string(&pt).map_err(display_error)
    }

    pub fn global_to_local3d_json(
        &self,
        chain_json: &str,
        sx: f64,
        sy: f64,
        focal_length: f64,
        cx: f64,
        cy: f64,
    ) -> Result<String, JsValue> {
        let chain: Vec<[f64; 16]> = serde_json::from_str(chain_json)
            .map_err(|e| JsValue::from_str(&format!("ArgumentError: Invalid 3D chain: {e}")))?;
        let pt =
            neoflash_core::as3_fte_3d::global_to_local3d(&chain, [sx, sy], focal_length, [cx, cy]);
        serde_json::to_string(&pt).map_err(display_error)
    }
}

impl Default for As3Fte3dKernel {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
pub struct As3MediaEventsKernel {
    inner: neoflash_core::as3_media_events::As3MediaEventsRuntime,
}

fn media_events_js_err(err: neoflash_core::as3_media_events::MediaEventsError) -> JsValue {
    JsValue::from_str(&err.to_wire_string())
}

fn media_events_json<T: serde::Serialize>(val: &T) -> Result<String, JsValue> {
    serde_json::to_string(val).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
impl As3MediaEventsKernel {
    #[wasm_bindgen(constructor)]
    pub fn new(max_touch_points: i32) -> As3MediaEventsKernel {
        Self {
            inner: neoflash_core::as3_media_events::As3MediaEventsRuntime::new(max_touch_points),
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
    }

    pub fn camera_names_json(&self) -> Result<String, JsValue> {
        media_events_json(&self.inner.camera_names())
    }

    pub fn resolve_camera(&self, name: Option<String>) -> i32 {
        self.inner
            .resolve_camera(name.as_deref())
            .map_or(-1, |idx| idx as i32)
    }

    pub fn camera_state_json(&self, index: u32) -> Result<String, JsValue> {
        let state = self
            .inner
            .camera_get(index as usize)
            .map_err(media_events_js_err)?;
        media_events_json(state)
    }

    pub fn camera_set_mode_json(
        &mut self,
        index: u32,
        width: i32,
        height: i32,
        fps: f64,
        favor_area: bool,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .camera_set_mode(index as usize, width, height, fps, favor_area)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn camera_set_quality_json(
        &mut self,
        index: u32,
        bandwidth: i32,
        quality: i32,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .camera_set_quality(index as usize, bandwidth, quality)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn camera_set_key_frame_interval_json(
        &mut self,
        index: u32,
        interval: i32,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .camera_set_key_frame_interval(index as usize, interval)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn camera_set_loopback_json(
        &mut self,
        index: u32,
        compress: bool,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .camera_set_loopback(index as usize, compress)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn camera_set_motion_level_json(
        &mut self,
        index: u32,
        motion_level: i32,
        timeout: i32,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .camera_set_motion_level(index as usize, motion_level, timeout)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn camera_set_cursor_json(&mut self, index: u32, cursor: bool) -> Result<String, JsValue> {
        let state = self
            .inner
            .camera_set_cursor(index as usize, cursor)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn camera_frame_argb_u32(
        &self,
        index: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Result<Vec<u32>, JsValue> {
        self.inner
            .camera_frame_argb_u32(index as usize, x, y, w, h)
            .map_err(media_events_js_err)
    }

    pub fn camera_frame_argb_bytes(
        &self,
        index: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Result<Vec<u8>, JsValue> {
        self.inner
            .camera_frame_argb_bytes(index as usize, x, y, w, h)
            .map_err(media_events_js_err)
    }

    pub fn microphone_names_json(&self) -> Result<String, JsValue> {
        media_events_json(&self.inner.microphone_names())
    }

    pub fn resolve_microphone(&self, index: i32) -> i32 {
        self.inner
            .resolve_microphone(index)
            .map_or(-1, |idx| idx as i32)
    }

    pub fn mic_state_json(&self, index: u32) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_get(index as usize)
            .map_err(media_events_js_err)?;
        media_events_json(state)
    }

    pub fn mic_enable_enhanced_json(&mut self, index: u32) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_enable_enhanced(index as usize)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_codec_json(&mut self, index: u32, codec: &str) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_codec(index as usize, codec)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_rate_json(&mut self, index: u32, rate: i32) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_rate(index as usize, rate)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_gain_json(&mut self, index: u32, gain: f64) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_gain(index as usize, gain)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_encode_quality_json(
        &mut self,
        index: u32,
        quality: i32,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_encode_quality(index as usize, quality)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_frames_per_packet_json(
        &mut self,
        index: u32,
        frames: i32,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_frames_per_packet(index as usize, frames)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_noise_suppression_level_json(
        &mut self,
        index: u32,
        level: i32,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_noise_suppression_level(index as usize, level)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_enable_vad_json(&mut self, index: u32, enable: bool) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_enable_vad(index as usize, enable)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_loop_back_json(&mut self, index: u32, state: bool) -> Result<String, JsValue> {
        let st = self
            .inner
            .mic_set_loop_back(index as usize, state)
            .map_err(media_events_js_err)?;
        media_events_json(&st)
    }

    pub fn mic_set_silence_level_json(
        &mut self,
        index: u32,
        silence_level: f64,
        timeout: i32,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_silence_level(index as usize, silence_level, timeout)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_use_echo_suppression_json(
        &mut self,
        index: u32,
        use_echo: bool,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_use_echo_suppression(index as usize, use_echo)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_enhanced_options_json(
        &mut self,
        index: u32,
        options_json: &str,
    ) -> Result<String, JsValue> {
        let opts: Option<neoflash_core::as3_media_events::MicrophoneEnhancedOptionsState> =
            if options_json.trim().is_empty() || options_json.trim() == "null" {
                None
            } else {
                Some(
                    serde_json::from_str(options_json).map_err(|e| {
                        JsValue::from_str(&format!("Invalid enhanced options: {e}"))
                    })?,
                )
            };
        let state = self
            .inner
            .mic_set_enhanced_options(index as usize, opts)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_sound_transform_json(
        &mut self,
        index: u32,
        volume: f64,
        pan: f64,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_sound_transform(index as usize, volume, pan)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn validate_enhanced_mode(&self, mode: &str) -> Result<String, JsValue> {
        neoflash_core::as3_media_events::validate_enhanced_mode(mode)
            .map(String::from)
            .map_err(media_events_js_err)
    }

    pub fn validate_echo_path(&self, echo_path: i32) -> Result<i32, JsValue> {
        neoflash_core::as3_media_events::validate_echo_path(echo_path).map_err(media_events_js_err)
    }

    pub fn create_video_stream_settings_json(&mut self, kind: &str) -> Result<String, JsValue> {
        let (id, state) = self
            .inner
            .create_video_stream_settings(kind)
            .map_err(media_events_js_err)?;
        media_events_json(&serde_json::json!({ "id": id, "state": state }))
    }

    pub fn video_settings_set_mode_json(
        &mut self,
        id: u32,
        width: i32,
        height: i32,
        fps: f64,
    ) -> Result<String, JsValue> {
        let vs = self
            .inner
            .video_settings_mut(id as usize)
            .map_err(media_events_js_err)?;
        vs.set_mode(width, height, fps)
            .map_err(media_events_js_err)?;
        let cloned = vs.clone();
        media_events_json(&cloned)
    }

    pub fn video_settings_set_quality_json(
        &mut self,
        id: u32,
        bandwidth: i32,
        quality: i32,
    ) -> Result<String, JsValue> {
        let vs = self
            .inner
            .video_settings_mut(id as usize)
            .map_err(media_events_js_err)?;
        vs.set_quality(bandwidth, quality);
        let cloned = vs.clone();
        media_events_json(&cloned)
    }

    pub fn video_settings_set_key_frame_interval_json(
        &mut self,
        id: u32,
        interval: i32,
    ) -> Result<String, JsValue> {
        let vs = self
            .inner
            .video_settings_mut(id as usize)
            .map_err(media_events_js_err)?;
        vs.set_key_frame_interval(interval);
        let cloned = vs.clone();
        media_events_json(&cloned)
    }

    pub fn video_settings_set_profile_level_json(
        &mut self,
        id: u32,
        profile: &str,
        level: &str,
    ) -> Result<String, JsValue> {
        let vs = self
            .inner
            .video_settings_mut(id as usize)
            .map_err(media_events_js_err)?;
        vs.set_profile_level(profile, level)
            .map_err(media_events_js_err)?;
        let cloned = vs.clone();
        media_events_json(&cloned)
    }

    pub fn stage_video_slot_count(&self) -> u32 {
        self.inner.stage_videos.len() as u32
    }

    pub fn stage_video_state_json(&self, slot: u32) -> Result<String, JsValue> {
        let sv = self
            .inner
            .stage_video_get(slot as usize)
            .map_err(media_events_js_err)?;
        media_events_json(sv)
    }

    pub fn stage_video_set_viewport_json(
        &mut self,
        slot: u32,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    ) -> Result<String, JsValue> {
        let sv = self
            .inner
            .stage_video_set_viewport(slot as usize, x, y, w, h)
            .map_err(media_events_js_err)?;
        media_events_json(&sv)
    }

    pub fn stage_video_set_pan_json(
        &mut self,
        slot: u32,
        x: f64,
        y: f64,
    ) -> Result<String, JsValue> {
        let sv = self
            .inner
            .stage_video_set_pan(slot as usize, x, y)
            .map_err(media_events_js_err)?;
        media_events_json(&sv)
    }

    pub fn stage_video_set_zoom_json(
        &mut self,
        slot: u32,
        x: f64,
        y: f64,
    ) -> Result<String, JsValue> {
        let sv = self
            .inner
            .stage_video_set_zoom(slot as usize, x, y)
            .map_err(media_events_js_err)?;
        media_events_json(&sv)
    }

    pub fn stage_video_set_depth_json(&mut self, slot: u32, depth: i32) -> Result<String, JsValue> {
        let sv = self
            .inner
            .stage_video_set_depth(slot as usize, depth)
            .map_err(media_events_js_err)?;
        media_events_json(&sv)
    }

    pub fn stage_video_set_dimensions_json(
        &mut self,
        slot: u32,
        width: i32,
        height: i32,
    ) -> Result<String, JsValue> {
        let sv = self
            .inner
            .stage_video_set_dimensions(slot as usize, width, height)
            .map_err(media_events_js_err)?;
        media_events_json(&sv)
    }

    pub fn audio_device_names_json(&self) -> Result<String, JsValue> {
        media_events_json(&self.inner.audio_device_names)
    }

    pub fn selected_audio_device(&self) -> i32 {
        self.inner.selected_audio_device
    }

    pub fn select_audio_device(&mut self, index: i32) -> Result<bool, JsValue> {
        self.inner
            .select_audio_device(index)
            .map_err(media_events_js_err)
    }

    pub fn multitouch_input_mode(&self) -> String {
        self.inner.multitouch_input_mode.clone()
    }

    pub fn set_multitouch_input_mode(&mut self, mode: &str) -> Result<String, JsValue> {
        self.inner
            .set_multitouch_input_mode(mode)
            .map_err(media_events_js_err)
    }

    pub fn max_touch_points(&self) -> i32 {
        self.inner.max_touch_points
    }

    pub fn map_touch_to_mouse(&self) -> bool {
        self.inner.map_touch_to_mouse
    }

    pub fn set_map_touch_to_mouse(&mut self, value: bool) {
        self.inner.map_touch_to_mouse = value;
    }

    pub fn supported_gestures_json(&self) -> Result<String, JsValue> {
        media_events_json(&self.inner.supported_gestures)
    }

    pub fn game_device_count(&self) -> u32 {
        self.inner.game_devices.len() as u32
    }

    pub fn game_device_state_json(&self, index: i32) -> Result<String, JsValue> {
        let dev = self
            .inner
            .game_device_get(index)
            .map_err(media_events_js_err)?;
        media_events_json(dev)
    }

    pub fn game_device_set_enabled(&mut self, index: i32, enabled: bool) -> Result<(), JsValue> {
        self.inner
            .game_device_set_enabled(index, enabled)
            .map_err(media_events_js_err)
    }

    pub fn game_device_set_sample_interval(
        &mut self,
        index: i32,
        interval: i32,
    ) -> Result<(), JsValue> {
        self.inner
            .game_device_set_sample_interval(index, interval)
            .map_err(media_events_js_err)
    }

    pub fn game_device_start_caching(
        &mut self,
        index: i32,
        num_samples: i32,
        control_ids_json: &str,
    ) -> Result<(), JsValue> {
        let control_ids: Vec<String> = serde_json::from_str(control_ids_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid control IDs JSON: {e}")))?;
        self.inner
            .game_device_start_caching(index, num_samples, control_ids)
            .map_err(media_events_js_err)
    }

    pub fn game_device_stop_caching(&mut self, index: i32) -> Result<(), JsValue> {
        self.inner
            .game_device_stop_caching(index)
            .map_err(media_events_js_err)
    }

    pub fn game_device_drain_samples_json(&mut self, index: i32) -> Result<String, JsValue> {
        let (rows, bytes) = self
            .inner
            .game_device_drain_samples(index)
            .map_err(media_events_js_err)?;
        media_events_json(&serde_json::json!({ "rows": rows, "bytes": bytes }))
    }

    pub fn mic_sample_pcm_json(
        &mut self,
        index: u32,
        sample_count: u32,
    ) -> Result<String, JsValue> {
        let (position, bytes) = self
            .inner
            .mic_sample_pcm_bytes(index as usize, sample_count as usize)
            .map_err(media_events_js_err)?;
        let state = self
            .inner
            .mic_get(index as usize)
            .map_err(media_events_js_err)?;
        media_events_json(&serde_json::json!({
            "position": position,
            "bytes": bytes,
            "activity_level": state.activity_level,
            "sample_position": state.sample_position,
        }))
    }

    pub fn sync_hardware_devices_json(
        &mut self,
        cameras_json: &str,
        microphones_json: &str,
        audio_outputs_json: &str,
    ) -> Result<String, JsValue> {
        let cameras: Vec<String> = serde_json::from_str(cameras_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid cameras JSON: {e}")))?;
        let microphones: Vec<String> = serde_json::from_str(microphones_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid microphones JSON: {e}")))?;
        let audio_outputs: Vec<String> = serde_json::from_str(audio_outputs_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid audio_outputs JSON: {e}")))?;
        self.inner
            .sync_hardware_devices(cameras, microphones, audio_outputs);
        media_events_json(&serde_json::json!({
            "cameras": self.inner.camera_names(),
            "microphones": self.inner.microphone_names(),
            "audio_outputs": self.inner.audio_device_names,
        }))
    }

    pub fn game_device_sync_json(
        &mut self,
        index: u32,
        id: &str,
        name: &str,
        axes_json: &str,
        buttons_json: &str,
    ) -> Result<String, JsValue> {
        let axes: Vec<f64> = serde_json::from_str(axes_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid axes JSON: {e}")))?;
        let buttons: Vec<f64> = serde_json::from_str(buttons_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid buttons JSON: {e}")))?;
        let (is_new, changed_controls) = self
            .inner
            .game_device_sync(index as usize, id, name, &axes, &buttons)
            .map_err(media_events_js_err)?;
        let device = self
            .inner
            .game_device_get(index as i32)
            .map_err(media_events_js_err)?;
        media_events_json(&serde_json::json!({
            "is_new": is_new,
            "changed_controls": changed_controls,
            "device": device,
        }))
    }

    pub fn game_device_remove_json(&mut self, index: u32) -> Result<String, JsValue> {
        let removed = self
            .inner
            .game_device_remove(index as usize)
            .map_err(media_events_js_err)?;
        media_events_json(&removed)
    }

    pub fn validate_acc_string(&self, value: &str, field: &str) -> Result<String, JsValue> {
        neoflash_core::as3_media_events::validate_acc_string(value, field)
            .map_err(media_events_js_err)
    }

    pub fn record_accessibility_update(&mut self) -> u32 {
        self.inner.accessibility_updates = self.inner.accessibility_updates.saturating_add(1);
        self.inner.accessibility_updates
    }
}
