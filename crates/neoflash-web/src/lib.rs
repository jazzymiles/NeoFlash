//! Thin JavaScript bindings around the same Rust timeline runtime used by the editor.
use neoflash_core::{
    DisplayMutation, DynamicKind, GraphicsOp, ObjectRef, PixelRef, PixelSnapping, PlaybackCommand,
    Player, Project, RuntimeScriptDispatch, ScriptDispatch,
};
use neoflash_core::{TextError, TextFieldState, TextPatch, layout_text};
use wasm_bindgen::prelude::*;

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
fn command(name: &str, frame: u32) -> Result<PlaybackCommand, JsValue> {
    match name {
        "play" => Ok(PlaybackCommand::Play),
        "stop" => Ok(PlaybackCommand::Stop),
        "goto_and_play" => Ok(PlaybackCommand::GotoAndPlay(frame)),
        "goto_and_stop" => Ok(PlaybackCommand::GotoAndStop(frame)),
        _ => Err(JsValue::from_str("Unknown timeline control command")),
    }
}

#[wasm_bindgen]
pub fn text_layout_json(field_json: &str, measure: &js_sys::Function) -> Result<String, JsValue> {
    if field_json.len() > 8 * 1024 * 1024 {
        return Err(JsValue::from_str("Text layout input exceeds 8 MiB"));
    }
    let field: TextFieldState = serde_json::from_str(field_json).map_err(display_error)?;
    let layout = layout_text(&field, |strings, format| {
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
    })
    .map_err(display_error)?;
    serde_json::to_string(&layout).map_err(display_error)
}

#[wasm_bindgen]
impl WebPlayer {
    #[wasm_bindgen(constructor)]
    pub fn new(json: &str) -> Result<WebPlayer, JsValue> {
        let project =
            Project::from_json(json).map_err(|error| JsValue::from_str(&error.to_string()))?;
        let player = Player::new(&project);
        if let Some(error) = player.bitmap_error() {
            return Err(display_error(error));
        }
        Ok(Self { project, player })
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
    pub fn text_layout_json(
        &self,
        field_json: &str,
        measure: &js_sys::Function,
    ) -> Result<String, JsValue> {
        text_layout_json(field_json, measure)
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
            "movie_clip" => DynamicKind::MovieClip,
            _ => {
                return Err(JsValue::from_str(
                    "Only empty Sprite, Shape and MovieClip creation is supported; linked dynamic timelines are pending",
                ));
            }
        };
        serde_json::to_string(&self.player.create_display(kind).map_err(display_error)?)
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
}
