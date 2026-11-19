//! WASM bindings for Flash Text Engine, Stage3D, Pixel Bender shaders, and 3D transforms (`As3Fte3dKernel`).

use super::display_error;
use wasm_bindgen::prelude::*;

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
