//! Transport only: all AS2 depth/path/duplication behavior lives in core Rust.
use super::*;
#[wasm_bindgen]
impl WebPlayer {
    pub fn as2_render_command(
        &mut self,
        reference: &str,
        request: &str,
    ) -> Result<String, JsValue> {
        if request.len() > 4096 {
            return Err(JsValue::from_str("AS2 render request exceeds 4096 bytes"));
        }
        let request = serde_json::from_str(request).map_err(display_error)?;
        self.player
            .as2_render_command(&self.project, &object_ref(reference)?, request)
            .map(|value| value.to_string())
            .map_err(display_error)
    }

    pub fn as2_input_command(&mut self, reference: &str, request: &str) -> Result<String, JsValue> {
        if request.len() > 1024 * 1024 {
            return Err(JsValue::from_str("AS2 input request exceeds 1 MiB"));
        }
        let request = serde_json::from_str(request).map_err(display_error)?;
        self.player
            .as2_input_command(&self.project, &object_ref(reference)?, request)
            .map_err(display_error)
    }

    pub fn as2_settings_command(&mut self, request: &str) -> Result<String, JsValue> {
        if request.len() > 4096 {
            return Err(JsValue::from_str("AS2 settings request exceeds 4096 bytes"));
        }
        let request = serde_json::from_str(request).map_err(display_error)?;
        self.player
            .as2_settings_command(request)
            .map_err(display_error)
    }

    pub fn as2_root(&self, target: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .as2_root(&object_ref(target)?)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }

    pub fn as2_get_depth(&self, reference: &str) -> Result<i32, JsValue> {
        self.player
            .as2_get_depth(&object_ref(reference)?)
            .map_err(display_error)
    }
    pub fn as2_next_depth(&self, parent: &str) -> Result<i32, JsValue> {
        self.player
            .as2_next_depth(&object_ref(parent)?)
            .map_err(display_error)
    }
    pub fn as2_child_at_depth(&self, parent: &str, depth: i32) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .as2_child_at_depth(&object_ref(parent)?, depth)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn as2_place_depth(
        &mut self,
        parent: &str,
        child: &str,
        depth: i32,
    ) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .as2_place_depth(
                    &self.project,
                    &object_ref(parent)?,
                    &object_ref(child)?,
                    depth,
                )
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn as2_swap_depth(&mut self, reference: &str, depth: i32) -> Result<(), JsValue> {
        self.player
            .as2_swap_depth(&object_ref(reference)?, depth)
            .map_err(display_error)
    }
    pub fn as2_duplicate(
        &mut self,
        source: &str,
        name: &str,
        depth: i32,
    ) -> Result<String, JsValue> {
        let (object_ref, result) = self
            .player
            .as2_duplicate(&self.project, &object_ref(source)?, name, depth)
            .map_err(display_error)?;
        // Preserve ObjectRef's canonical id/generation field order. Passing it
        // through serde_json::Value sorts keys and breaks the existing host's
        // reference-key identity convention.
        #[derive(serde::Serialize)]
        struct DuplicatePacket {
            object_ref: ObjectRef,
            events: Vec<neoflash_core::DisplayEvent>,
        }
        serde_json::to_string(&DuplicatePacket {
            object_ref,
            events: result.events,
        })
        .map_err(display_error)
    }
    pub fn as2_target_path(&self, target: &str, slash: bool) -> Result<String, JsValue> {
        self.player
            .as2_target_path(&self.project, &object_ref(target)?, slash)
            .map_err(display_error)
    }
    pub fn as2_resolve_target(&self, relative: &str, path: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &self
                .player
                .as2_resolve_target(&self.project, &object_ref(relative)?, path)
                .map_err(display_error)?,
        )
        .map_err(display_error)
    }
    pub fn as2_call_frame_scripts(&self, relative: &str, spec: &str) -> Result<String, JsValue> {
        if spec.len() > 4096 {
            return Err(JsValue::from_str("AS2 call() specifier exceeds 4096 bytes"));
        }
        let parsed: neoflash_core::GotoFrame = serde_json::from_str(spec).map_err(display_error)?;
        #[derive(serde::Serialize)]
        struct CallFramePacket {
            target_ref: ObjectRef,
            scripts: Vec<neoflash_core::Script>,
        }
        let result = self
            .player
            .as2_call_frame_scripts(&self.project, &object_ref(relative)?, &parsed)
            .map_err(display_error)?
            .map(|(target_ref, scripts)| CallFramePacket {
                target_ref,
                scripts,
            });
        serde_json::to_string(&result).map_err(display_error)
    }
    pub fn as2_is_frame_loaded(&self, relative: &str, request: &str) -> Result<bool, JsValue> {
        if request.len() > 4096 {
            return Err(JsValue::from_str(
                "AS2 ifFrameLoaded request exceeds 4096 bytes",
            ));
        }
        let parsed: neoflash_core::GotoRequest =
            serde_json::from_str(request).map_err(display_error)?;
        self.player
            .as2_is_frame_loaded(&self.project, &object_ref(relative)?, &parsed)
            .map_err(display_error)
    }
    pub fn as2_accessibility_update(&mut self, entries: &str) -> Result<(), JsValue> {
        if entries.len() > 1024 * 1024 {
            return Err(JsValue::from_str("AS2 accessibility update exceeds 1 MiB"));
        }
        let parsed = serde_json::from_str(entries).map_err(display_error)?;
        self.player
            .as2_accessibility_update(parsed)
            .map_err(display_error)
    }
}
