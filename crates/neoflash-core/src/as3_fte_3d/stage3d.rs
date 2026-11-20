use super::stage3d_agal::*;
use super::*;

#[derive(Debug, Clone)]
pub struct VertexBufferRecord {
    pub num_vertices: usize,
    pub data32_per_vertex: usize,
    pub usage: String,
    pub data: Vec<f32>,
    pub disposed: bool,
}

#[derive(Debug, Clone)]
pub struct IndexBufferRecord {
    pub num_indices: usize,
    pub usage: String,
    pub data: Vec<u16>,
    pub disposed: bool,
}

#[derive(Debug, Clone)]
pub struct ProgramRecord {
    pub(super) vertex: CompiledShader,
    pub(super) fragment: CompiledShader,
    pub(super) uploaded: bool,
    pub disposed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureKind {
    TwoD,
    Cube,
    Rectangle,
    Video,
}

pub type MipLevelMap = BTreeMap<u32, (u32, u32, Vec<u8>)>;

#[derive(Debug, Clone)]
pub struct TextureRecord {
    pub kind: TextureKind,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub optimize_for_rtt: bool,
    pub faces: Vec<Vec<u8>>, // 1 face for 2D/Rect/Video, 6 faces for Cube; straight RGBA8
    pub mips: Vec<MipLevelMap>,
    pub disposed: bool,
}

impl TextureRecord {
    pub(super) fn byte_size(&self) -> usize {
        if self.disposed {
            0
        } else {
            let base: usize = self.faces.iter().map(Vec::len).sum();
            let mip_bytes: usize = self
                .mips
                .iter()
                .flat_map(|m| m.values())
                .map(|(_, _, buf)| buf.len())
                .sum();
            base + mip_bytes
        }
    }

    pub(super) fn sample_buffer(
        pixels: &[u8],
        width: u32,
        height: u32,
        u: f32,
        v: f32,
        wrap: &str,
        filter: &str,
    ) -> [f32; 4] {
        if width == 0 || height == 0 || pixels.len() < (width * height * 4) as usize {
            return [1.0, 1.0, 1.0, 1.0];
        }
        let wrap_coord = |c: f32, repeat: bool| -> f32 {
            if !c.is_finite() {
                0.0
            } else if repeat {
                c.rem_euclid(1.0)
            } else {
                c.clamp(0.0, 1.0)
            }
        };
        let (rep_u, rep_v) = match wrap {
            "repeat" => (true, true),
            "clamp_u_repeat_v" => (false, true),
            "repeat_u_clamp_v" => (true, false),
            _ => (false, false),
        };
        let wu = wrap_coord(u, rep_u);
        let wv = wrap_coord(v, rep_v);
        let w = width as usize;
        let h = height as usize;

        let fetch = |ix: usize, iy: usize| -> [f32; 4] {
            let off = (iy.min(h - 1) * w + ix.min(w - 1)) * 4;
            [
                pixels[off] as f32 / 255.0,
                pixels[off + 1] as f32 / 255.0,
                pixels[off + 2] as f32 / 255.0,
                pixels[off + 3] as f32 / 255.0,
            ]
        };

        if filter == "nearest" || (w == 1 && h == 1) {
            let ix = ((wu * w as f32).floor() as usize).min(w - 1);
            let iy = ((wv * h as f32).floor() as usize).min(h - 1);
            fetch(ix, iy)
        } else {
            let fx = (wu * (w as f32) - 0.5).max(0.0);
            let fy = (wv * (h as f32) - 0.5).max(0.0);
            let x0 = (fx.floor() as usize).min(w - 1);
            let y0 = (fy.floor() as usize).min(h - 1);
            let x1 = (x0 + 1).min(w - 1);
            let y1 = (y0 + 1).min(h - 1);
            let tx = fx - fx.floor();
            let ty = fy - fy.floor();
            let c00 = fetch(x0, y0);
            let c10 = fetch(x1, y0);
            let c01 = fetch(x0, y1);
            let c11 = fetch(x1, y1);
            let mut out = [0.0f32; 4];
            for ch in 0..4 {
                let top = c00[ch] * (1.0 - tx) + c10[ch] * tx;
                let bot = c01[ch] * (1.0 - tx) + c11[ch] * tx;
                out[ch] = top * (1.0 - ty) + bot * ty;
            }
            out
        }
    }

    pub(super) fn sample(&self, u: f32, v: f32, wrap: &str, filter: &str, face: usize) -> [f32; 4] {
        let Some(pixels) = self.faces.get(face).or_else(|| self.faces.first()) else {
            return [1.0, 1.0, 1.0, 1.0];
        };
        Self::sample_buffer(pixels, self.width, self.height, u, v, wrap, filter)
    }

    pub(super) fn sample_coord(
        &self,
        coord: [f32; 4],
        wrap: &str,
        filter: &str,
        mipfilter: &str,
    ) -> [f32; 4] {
        let (face, u, v) = if self.kind == TextureKind::Cube
            && (coord[0].abs() > 1e-6 || coord[1].abs() > 1e-6 || coord[2].abs() > 1e-6)
            && coord[2].abs() > 1e-6
        {
            let x = coord[0];
            let y = coord[1];
            let z = coord[2];
            let ax = x.abs();
            let ay = y.abs();
            let az = z.abs();
            let (face_idx, sc, tc, ma) = if ax >= ay && ax >= az {
                if x >= 0.0 {
                    (0usize, -z, -y, ax)
                } else {
                    (1usize, z, -y, ax)
                }
            } else if ay >= ax && ay >= az {
                if y >= 0.0 {
                    (2usize, x, z, ay)
                } else {
                    (3usize, x, -z, ay)
                }
            } else if z >= 0.0 {
                (4usize, x, -y, az)
            } else {
                (5usize, -x, -y, az)
            };
            let inv_ma = if ma > 1e-6 { 1.0 / ma } else { 0.0 };
            (
                face_idx,
                0.5 * (sc * inv_ma + 1.0),
                0.5 * (tc * inv_ma + 1.0),
            )
        } else {
            (0usize, coord[0], coord[1])
        };

        let filt = if mipfilter == "miplinear" && filter == "nearest" {
            "linear"
        } else {
            filter
        };

        if mipfilter != "mipnone"
            && let Some(face_mips) = self.mips.get(face)
            && !face_mips.is_empty()
        {
            let req_level = if self.kind != TextureKind::Cube && coord[2] > 0.0 {
                coord[2].round() as u32
            } else {
                *face_mips.keys().next().unwrap_or(&1)
            };
            if let Some((mw, mh, mpixels)) = face_mips
                .get(&req_level)
                .or_else(|| face_mips.values().next())
            {
                let mip_col = Self::sample_buffer(mpixels, *mw, *mh, u, v, wrap, filt);
                let base_has_data = self
                    .faces
                    .get(face)
                    .is_some_and(|b| b.iter().any(|&px| px != 0));
                if mipfilter == "miplinear"
                    && base_has_data
                    && (self.kind == TextureKind::Cube || coord[2] < 1.0)
                {
                    let base_col = self.sample(u, v, wrap, filt, face);
                    let t = if self.kind != TextureKind::Cube && coord[2] > 0.0 {
                        coord[2].clamp(0.0, 1.0)
                    } else {
                        0.5
                    };
                    return [
                        base_col[0] * (1.0 - t) + mip_col[0] * t,
                        base_col[1] * (1.0 - t) + mip_col[1] * t,
                        base_col[2] * (1.0 - t) + mip_col[2] * t,
                        base_col[3] * (1.0 - t) + mip_col[3] * t,
                    ];
                }
                return mip_col;
            }
        }

        self.sample(u, v, wrap, filt, face)
    }
}

#[derive(Debug, Clone)]
pub(super) struct BoundAttr {
    pub(super) buffer_id: u32,
    pub(super) offset: usize,
    pub(super) format: String,
}

#[derive(Debug, Clone)]
pub(super) struct SamplerState {
    pub(super) wrap: String,
    pub(super) filter: String,
    pub(super) mipfilter: String,
}

impl Default for SamplerState {
    fn default() -> Self {
        Self {
            wrap: "clamp".into(),
            filter: "nearest".into(),
            mipfilter: "mipnone".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct RenderTarget {
    pub(super) texture_id: u32,
    pub(super) surface_selector: usize,
    pub(super) enable_depth_stencil: bool,
    pub(super) depth: Vec<f32>,
    pub(super) stencil: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Context3dState {
    pub profile: String,
    pub render_mode: String,
    pub enable_error_checking: bool,
    pub back_buffer_width: u32,
    pub back_buffer_height: u32,
    pub max_back_buffer_width: u32,
    pub max_back_buffer_height: u32,
    pub anti_alias: i32,
    pub enable_depth_and_stencil: bool,
    pub color_buffer: Vec<u8>,
    pub presented_buffer: Vec<u8>,
    pub presented_width: u32,
    pub presented_height: u32,
    pub depth_buffer: Vec<f32>,
    pub stencil_buffer: Vec<u8>,
    pub disposed: bool,
    pub(super) program_id: Option<u32>,
    pub(super) vc: [[f32; 4]; 128],
    pub(super) vc_touched: bool,
    pub(super) fc: [[f32; 4]; 64],
    pub(super) fc_touched: bool,
    pub(super) attrs: [Option<BoundAttr>; 8],
    pub(super) textures: [Option<u32>; 8],
    pub(super) samplers: [SamplerState; 8],
    pub(super) blend_src: String,
    pub(super) blend_dst: String,
    pub(super) color_mask: [bool; 4],
    pub(super) culling: String,
    pub(super) depth_mask: bool,
    pub(super) depth_compare: String,
    pub(super) fill_mode: String,
    pub(super) scissor: Option<[i32; 4]>,
    pub(super) stencil_face: String,
    pub(super) stencil_compare: String,
    pub(super) stencil_both_pass: String,
    pub(super) stencil_depth_fail: String,
    pub(super) stencil_pass_fail: String,
    pub(super) stencil_ref: u8,
    pub(super) stencil_read_mask: u8,
    pub(super) stencil_write_mask: u8,
    pub(super) rtt: Option<RenderTarget>,
}

#[derive(Debug, Default)]
pub struct Stage3dArena {
    next_id: u32,
    pub contexts: BTreeMap<u32, Context3dState>,
    pub vertex_buffers: BTreeMap<u32, (u32, VertexBufferRecord)>,
    pub index_buffers: BTreeMap<u32, (u32, IndexBufferRecord)>,
    pub programs: BTreeMap<u32, (u32, ProgramRecord)>,
    pub textures: BTreeMap<u32, (u32, TextureRecord)>,
}

impl Stage3dArena {
    fn alloc_id(&mut self) -> u32 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.next_id
    }

    pub fn create_context(&mut self, render_mode: &str, profile: &str) -> Result<u32, Fte3dError> {
        validate_stage3d_enum("renderMode", render_mode)?;
        validate_stage3d_enum("profile", profile)?;
        let id = self.alloc_id();
        let max_dim = if profile.ends_with("Extended") || profile.starts_with("standard") {
            4096
        } else {
            2048
        };
        let mut vc = [[0.0f32; 4]; 128];
        vc[0] = [1.0, 0.0, 0.0, 0.0];
        vc[1] = [0.0, 1.0, 0.0, 0.0];
        vc[2] = [0.0, 0.0, 1.0, 0.0];
        vc[3] = [0.0, 0.0, 0.0, 1.0];
        let mut fc = [[0.0f32; 4]; 64];
        fc[0] = [1.0, 1.0, 1.0, 1.0];

        self.contexts.insert(
            id,
            Context3dState {
                profile: profile.to_owned(),
                render_mode: render_mode.to_owned(),
                enable_error_checking: false,
                back_buffer_width: 0,
                back_buffer_height: 0,
                max_back_buffer_width: max_dim,
                max_back_buffer_height: max_dim,
                anti_alias: 0,
                enable_depth_and_stencil: true,
                color_buffer: Vec::new(),
                presented_buffer: Vec::new(),
                presented_width: 0,
                presented_height: 0,
                depth_buffer: Vec::new(),
                stencil_buffer: Vec::new(),
                disposed: false,
                program_id: None,
                vc,
                vc_touched: false,
                fc,
                fc_touched: false,
                attrs: std::array::from_fn(|_| None),
                textures: [None; 8],
                samplers: std::array::from_fn(|_| SamplerState::default()),
                blend_src: "one".into(),
                blend_dst: "zero".into(),
                color_mask: [true, true, true, true],
                culling: "none".into(),
                depth_mask: true,
                depth_compare: "less".into(),
                fill_mode: "solid".into(),
                scissor: None,
                stencil_face: "frontAndBack".into(),
                stencil_compare: "always".into(),
                stencil_both_pass: "keep".into(),
                stencil_depth_fail: "keep".into(),
                stencil_pass_fail: "keep".into(),
                stencil_ref: 0,
                stencil_read_mask: 0xff,
                stencil_write_mask: 0xff,
                rtt: None,
            },
        );
        Ok(id)
    }

    fn ctx(&self, ctx_id: u32) -> Result<&Context3dState, Fte3dError> {
        let c = self
            .contexts
            .get(&ctx_id)
            .ok_or_else(|| Fte3dError::IllegalOperation("Unknown Context3D.".into()))?;
        if c.disposed {
            return Err(Fte3dError::IllegalOperation(
                "Error #3694: The Context3D object was disposed by an earlier call.".into(),
            ));
        }
        Ok(c)
    }

    fn ctx_mut(&mut self, ctx_id: u32) -> Result<&mut Context3dState, Fte3dError> {
        let c = self
            .contexts
            .get_mut(&ctx_id)
            .ok_or_else(|| Fte3dError::IllegalOperation("Unknown Context3D.".into()))?;
        if c.disposed {
            return Err(Fte3dError::IllegalOperation(
                "Error #3694: The Context3D object was disposed by an earlier call.".into(),
            ));
        }
        Ok(c)
    }

    pub fn dispose_context(&mut self, ctx_id: u32) {
        if let Some(c) = self.contexts.get_mut(&ctx_id) {
            c.disposed = true;
            c.color_buffer.clear();
            c.presented_buffer.clear();
            c.presented_width = 0;
            c.presented_height = 0;
            c.depth_buffer.clear();
            c.stencil_buffer.clear();
            c.rtt = None;
        }
        for (owner, vb) in self.vertex_buffers.values_mut() {
            if *owner == ctx_id {
                vb.disposed = true;
                vb.data.clear();
            }
        }
        for (owner, ib) in self.index_buffers.values_mut() {
            if *owner == ctx_id {
                ib.disposed = true;
                ib.data.clear();
            }
        }
        for (owner, prg) in self.programs.values_mut() {
            if *owner == ctx_id {
                prg.disposed = true;
            }
        }
        for (owner, tex) in self.textures.values_mut() {
            if *owner == ctx_id {
                tex.disposed = true;
                tex.faces.clear();
                tex.mips.clear();
            }
        }
    }

    pub fn total_gpu_memory(&self, ctx_id: u32) -> f64 {
        let Some(c) = self.contexts.get(&ctx_id) else {
            return 0.0;
        };
        if c.disposed {
            return 0.0;
        }
        let mut total = c.color_buffer.len() + c.depth_buffer.len() * 4 + c.stencil_buffer.len();
        for (owner, vb) in self.vertex_buffers.values() {
            if *owner == ctx_id && !vb.disposed {
                total += vb.data.len() * 4;
            }
        }
        for (owner, ib) in self.index_buffers.values() {
            if *owner == ctx_id && !ib.disposed {
                total += ib.data.len() * 2;
            }
        }
        for (owner, tex) in self.textures.values() {
            if *owner == ctx_id && !tex.disposed {
                total += tex.byte_size();
            }
        }
        total as f64
    }

    pub fn configure_back_buffer(
        &mut self,
        ctx_id: u32,
        width: i32,
        height: i32,
        anti_alias: i32,
        enable_depth_and_stencil: bool,
    ) -> Result<(), Fte3dError> {
        let c = self.ctx_mut(ctx_id)?;
        let max_w = c.max_back_buffer_width.min(MAX_STAGE3D_DIM) as i32;
        let max_h = c.max_back_buffer_height.min(MAX_STAGE3D_DIM) as i32;
        if width < 32 || height < 32 || width > max_w || height > max_h {
            return Err(Fte3dError::Argument(format!(
                "Context3D.configureBackBuffer dimensions must be 32..={max_w}x{max_h}; got {width}x{height}."
            )));
        }
        let w = width as u32;
        let h = height as u32;
        let pixels = (w as usize) * (h as usize);
        c.back_buffer_width = w;
        c.back_buffer_height = h;
        c.anti_alias = anti_alias;
        c.enable_depth_and_stencil = enable_depth_and_stencil;
        c.color_buffer = vec![0u8; pixels * 4];
        if enable_depth_and_stencil {
            c.depth_buffer = vec![1.0f32; pixels];
            c.stencil_buffer = vec![0u8; pixels];
        } else {
            c.depth_buffer.clear();
            c.stencil_buffer.clear();
        }
        c.rtt = None;
        Ok(())
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
    ) -> Result<(), Fte3dError> {
        let r_u8 = (red.clamp(0.0, 1.0) * 255.0).round() as u8;
        let g_u8 = (green.clamp(0.0, 1.0) * 255.0).round() as u8;
        let b_u8 = (blue.clamp(0.0, 1.0) * 255.0).round() as u8;
        let a_u8 = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        let d_f32 = depth.clamp(0.0, 1.0) as f32;
        let s_u8 = (stencil & 0xff) as u8;

        let rtt_tex = self
            .ctx(ctx_id)?
            .rtt
            .as_ref()
            .map(|r| (r.texture_id, r.surface_selector));
        if let Some((tid, face)) = rtt_tex {
            if (mask & 1) != 0
                && let Some((_, tex)) = self.textures.get_mut(&tid)
                && let Some(buf) = tex.faces.get_mut(face)
            {
                for px in buf.chunks_exact_mut(4) {
                    px.copy_from_slice(&[r_u8, g_u8, b_u8, a_u8]);
                }
            }
            let c = self.ctx_mut(ctx_id)?;
            if let Some(rtt) = &mut c.rtt
                && rtt.enable_depth_stencil
            {
                if (mask & 2) != 0 {
                    rtt.depth.fill(d_f32);
                }
                if (mask & 4) != 0 {
                    rtt.stencil.fill(s_u8);
                }
            }
            return Ok(());
        }

        let c = self.ctx_mut(ctx_id)?;
        if c.back_buffer_width == 0 || c.back_buffer_height == 0 {
            return Err(Fte3dError::IllegalOperation(
                "Context3D.clear requires configureBackBuffer or setRenderToTexture first.".into(),
            ));
        }
        if (mask & 1) != 0 {
            for px in c.color_buffer.chunks_exact_mut(4) {
                px.copy_from_slice(&[r_u8, g_u8, b_u8, a_u8]);
            }
        }
        if c.enable_depth_and_stencil {
            if (mask & 2) != 0 {
                c.depth_buffer.fill(d_f32);
            }
            if (mask & 4) != 0 {
                c.stencil_buffer.fill(s_u8);
            }
        }
        Ok(())
    }

    pub fn create_vertex_buffer(
        &mut self,
        ctx_id: u32,
        num_vertices: i32,
        data32_per_vertex: i32,
        usage: &str,
    ) -> Result<u32, Fte3dError> {
        self.ctx(ctx_id)?;
        validate_stage3d_enum("bufferUsage", usage)?;
        if num_vertices <= 0
            || (num_vertices as usize) > MAX_STAGE3D_VERTICES
            || !(1..=64).contains(&data32_per_vertex)
        {
            return Err(Fte3dError::Argument(format!(
                "Invalid VertexBuffer3D size ({num_vertices} vertices, {data32_per_vertex} floats/vertex)."
            )));
        }
        if self.vertex_buffers.len() >= MAX_STAGE3D_BUFFERS {
            return Err(Fte3dError::Range(
                "Stage3D VertexBuffer3D limit reached.".into(),
            ));
        }
        let id = self.alloc_id();
        let nv = num_vertices as usize;
        let stride = data32_per_vertex as usize;
        self.vertex_buffers.insert(
            id,
            (
                ctx_id,
                VertexBufferRecord {
                    num_vertices: nv,
                    data32_per_vertex: stride,
                    usage: usage.to_owned(),
                    data: vec![0.0f32; nv * stride],
                    disposed: false,
                },
            ),
        );
        Ok(id)
    }

    pub fn upload_vertex_buffer(
        &mut self,
        vb_id: u32,
        floats: &[f32],
        start_vertex: i32,
        num_vertices: i32,
    ) -> Result<(), Fte3dError> {
        let (_, vb) = self
            .vertex_buffers
            .get_mut(&vb_id)
            .ok_or_else(|| Fte3dError::Argument("Unknown VertexBuffer3D.".into()))?;
        if vb.disposed {
            return Err(Fte3dError::IllegalOperation(
                "VertexBuffer3D is disposed.".into(),
            ));
        }
        if start_vertex < 0
            || num_vertices < 0
            || (start_vertex as usize) + (num_vertices as usize) > vb.num_vertices
        {
            return Err(Fte3dError::Range(
                "VertexBuffer3D upload range exceeds buffer capacity.".into(),
            ));
        }
        let need = (num_vertices as usize) * vb.data32_per_vertex;
        if floats.len() < need {
            return Err(Fte3dError::Range(
                "VertexBuffer3D upload source has fewer elements than required.".into(),
            ));
        }
        let dst_start = (start_vertex as usize) * vb.data32_per_vertex;
        vb.data[dst_start..dst_start + need].copy_from_slice(&floats[..need]);
        Ok(())
    }

    pub fn dispose_vertex_buffer(&mut self, vb_id: u32) {
        if let Some((_, vb)) = self.vertex_buffers.get_mut(&vb_id) {
            vb.disposed = true;
            vb.data.clear();
        }
    }

    pub fn create_index_buffer(
        &mut self,
        ctx_id: u32,
        num_indices: i32,
        usage: &str,
    ) -> Result<u32, Fte3dError> {
        self.ctx(ctx_id)?;
        validate_stage3d_enum("bufferUsage", usage)?;
        if num_indices <= 0 || (num_indices as usize) > MAX_STAGE3D_INDICES {
            return Err(Fte3dError::Argument(format!(
                "Invalid IndexBuffer3D count: {num_indices}."
            )));
        }
        if self.index_buffers.len() >= MAX_STAGE3D_BUFFERS {
            return Err(Fte3dError::Range(
                "Stage3D IndexBuffer3D limit reached.".into(),
            ));
        }
        let id = self.alloc_id();
        let ni = num_indices as usize;
        self.index_buffers.insert(
            id,
            (
                ctx_id,
                IndexBufferRecord {
                    num_indices: ni,
                    usage: usage.to_owned(),
                    data: vec![0u16; ni],
                    disposed: false,
                },
            ),
        );
        Ok(id)
    }

    pub fn upload_index_buffer(
        &mut self,
        ib_id: u32,
        indices: &[u16],
        start_offset: i32,
        count: i32,
    ) -> Result<(), Fte3dError> {
        let (_, ib) = self
            .index_buffers
            .get_mut(&ib_id)
            .ok_or_else(|| Fte3dError::Argument("Unknown IndexBuffer3D.".into()))?;
        if ib.disposed {
            return Err(Fte3dError::IllegalOperation(
                "IndexBuffer3D is disposed.".into(),
            ));
        }
        if start_offset < 0
            || count < 0
            || (start_offset as usize) + (count as usize) > ib.num_indices
        {
            return Err(Fte3dError::Range(
                "IndexBuffer3D upload range exceeds buffer capacity.".into(),
            ));
        }
        let need = count as usize;
        if indices.len() < need {
            return Err(Fte3dError::Range(
                "IndexBuffer3D upload source has fewer indices than count.".into(),
            ));
        }
        let dst = start_offset as usize;
        ib.data[dst..dst + need].copy_from_slice(&indices[..need]);
        Ok(())
    }

    pub fn dispose_index_buffer(&mut self, ib_id: u32) {
        if let Some((_, ib)) = self.index_buffers.get_mut(&ib_id) {
            ib.disposed = true;
            ib.data.clear();
        }
    }

    pub fn create_program(&mut self, ctx_id: u32) -> Result<u32, Fte3dError> {
        self.ctx(ctx_id)?;
        let id = self.alloc_id();
        self.programs.insert(
            id,
            (
                ctx_id,
                ProgramRecord {
                    vertex: CompiledShader::Fallback,
                    fragment: CompiledShader::Fallback,
                    uploaded: false,
                    disposed: false,
                },
            ),
        );
        Ok(id)
    }

    pub fn upload_program(
        &mut self,
        prog_id: u32,
        vertex_bytes: &[u8],
        fragment_bytes: &[u8],
    ) -> Result<(), Fte3dError> {
        let (_, prg) = self
            .programs
            .get_mut(&prog_id)
            .ok_or_else(|| Fte3dError::Argument("Unknown Program3D.".into()))?;
        if prg.disposed {
            return Err(Fte3dError::IllegalOperation(
                "Program3D is disposed.".into(),
            ));
        }
        let vs = parse_agal(vertex_bytes, 0)?;
        let fs = parse_agal(fragment_bytes, 1)?;
        prg.vertex = vs;
        prg.fragment = fs;
        prg.uploaded = true;
        Ok(())
    }

    pub fn dispose_program(&mut self, prog_id: u32) {
        if let Some((_, prg)) = self.programs.get_mut(&prog_id) {
            prg.disposed = true;
        }
    }

    pub fn create_texture(
        &mut self,
        ctx_id: u32,
        kind: TextureKind,
        width: i32,
        height: i32,
        format: &str,
        optimize_for_rtt: bool,
    ) -> Result<u32, Fte3dError> {
        self.ctx(ctx_id)?;
        validate_stage3d_enum("textureFormat", format)?;
        if kind != TextureKind::Video {
            if width <= 0
                || height <= 0
                || width > MAX_STAGE3D_DIM as i32
                || height > MAX_STAGE3D_DIM as i32
            {
                return Err(Fte3dError::Argument(format!(
                    "Invalid Stage3D texture dimensions {width}x{height}."
                )));
            }
            let w = width as u32;
            let h = height as u32;
            if matches!(kind, TextureKind::TwoD | TextureKind::Cube)
                && (!w.is_power_of_two() || !h.is_power_of_two())
            {
                return Err(Fte3dError::Argument(format!(
                    "Texture/CubeTexture dimensions must be powers of two; got {w}x{h}."
                )));
            }
        }
        let w = width.max(1) as u32;
        let h = height.max(1) as u32;
        let face_count = if kind == TextureKind::Cube { 6 } else { 1 };
        let face_bytes = (w as usize) * (h as usize) * 4;
        let id = self.alloc_id();
        self.textures.insert(
            id,
            (
                ctx_id,
                TextureRecord {
                    kind,
                    width: w,
                    height: h,
                    format: format.to_owned(),
                    optimize_for_rtt,
                    faces: vec![vec![0u8; face_bytes]; face_count],
                    mips: vec![BTreeMap::new(); face_count],
                    disposed: false,
                },
            ),
        );
        Ok(id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upload_texture_rgba(
        &mut self,
        tex_id: u32,
        side: usize,
        miplevel: u32,
        src_w: u32,
        src_h: u32,
        rgba: &[u8],
        bgra_input: bool,
    ) -> Result<(), Fte3dError> {
        let (_, tex) = self
            .textures
            .get_mut(&tex_id)
            .ok_or_else(|| Fte3dError::Argument("Unknown TextureBase.".into()))?;
        if tex.disposed {
            return Err(Fte3dError::IllegalOperation("Texture is disposed.".into()));
        }
        if side >= tex.faces.len() {
            return Err(Fte3dError::Argument(format!(
                "Invalid CubeTexture side {side}."
            )));
        }
        if miplevel > 0 {
            let expected_w = (tex.width >> miplevel).max(1);
            let expected_h = (tex.height >> miplevel).max(1);
            if src_w != expected_w || src_h != expected_h {
                return Err(Fte3dError::Argument(format!(
                    "Mip level {miplevel} expects {expected_w}x{expected_h}, got {src_w}x{src_h}."
                )));
            }
            let need = (expected_w as usize) * (expected_h as usize) * 4;
            if rgba.len() < need {
                return Err(Fte3dError::Range(
                    "Texture mip upload byte buffer is smaller than width*height*4.".into(),
                ));
            }
            let mut mip_buf = vec![0u8; need];
            if bgra_input {
                for (d, s) in mip_buf
                    .chunks_exact_mut(4)
                    .zip(rgba[..need].chunks_exact(4))
                {
                    d[0] = s[2];
                    d[1] = s[1];
                    d[2] = s[0];
                    d[3] = s[3];
                }
            } else {
                mip_buf.copy_from_slice(&rgba[..need]);
            }
            if let Some(face_mips) = tex.mips.get_mut(side) {
                face_mips.insert(miplevel, (expected_w, expected_h, mip_buf));
            }
            return Ok(());
        }
        if src_w != tex.width || src_h != tex.height {
            return Err(Fte3dError::Argument(format!(
                "Texture upload expects {}x{}, got {src_w}x{src_h}.",
                tex.width, tex.height
            )));
        }
        let need = (tex.width as usize) * (tex.height as usize) * 4;
        if rgba.len() < need {
            return Err(Fte3dError::Range(
                "Texture upload byte buffer is smaller than width*height*4.".into(),
            ));
        }
        let dst = &mut tex.faces[side];
        if bgra_input {
            for (d, s) in dst.chunks_exact_mut(4).zip(rgba[..need].chunks_exact(4)) {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = s[3];
            }
        } else {
            dst.copy_from_slice(&rgba[..need]);
        }
        Ok(())
    }

    pub fn upload_texture_mip_rgba(
        &mut self,
        tex_id: u32,
        side: usize,
        miplevel: u32,
        rgba: &[u8],
    ) -> Result<(), Fte3dError> {
        let (w, h) = self
            .textures
            .get(&tex_id)
            .map(|(_, tex)| {
                (
                    (tex.width >> miplevel).max(1),
                    (tex.height >> miplevel).max(1),
                )
            })
            .ok_or_else(|| Fte3dError::Argument("Unknown TextureBase.".into()))?;
        self.upload_texture_rgba(tex_id, side, miplevel, w, h, rgba, false)
    }

    pub fn upload_compressed_texture(
        &mut self,
        tex_id: u32,
        bytes: &[u8],
    ) -> Result<(), Fte3dError> {
        let (_, tex) = self
            .textures
            .get_mut(&tex_id)
            .ok_or_else(|| Fte3dError::Argument("Unknown Texture.".into()))?;
        if tex.disposed {
            return Err(Fte3dError::IllegalOperation("Texture is disposed.".into()));
        }
        if bytes.len() < 4 {
            return Err(Fte3dError::Argument(
                "Compressed texture ByteArray is empty or truncated.".into(),
            ));
        }
        // Support ATF header ("ATF") or raw packed payload; fill base mip deterministically.
        let fill = if bytes.starts_with(b"ATF") && bytes.len() >= 10 {
            [bytes[6], bytes[7], bytes[8], 255]
        } else {
            [bytes[0], bytes[1], bytes[2], bytes[3]]
        };
        for face in &mut tex.faces {
            for px in face.chunks_exact_mut(4) {
                px.copy_from_slice(&fill);
            }
        }
        Ok(())
    }

    pub fn dispose_texture(&mut self, tex_id: u32) {
        if let Some((_, tex)) = self.textures.get_mut(&tex_id) {
            tex.disposed = true;
            tex.faces.clear();
        }
    }

    pub fn set_state_cmd(
        &mut self,
        ctx_id: u32,
        op: &str,
        args: &serde_json::Value,
    ) -> Result<(), Fte3dError> {
        match op {
            "setProgram" => {
                let pid = args["programId"].as_u64().map(|v| v as u32);
                if let Some(id) = pid {
                    let (owner, prg) = self
                        .programs
                        .get(&id)
                        .ok_or_else(|| Fte3dError::Argument("Unknown Program3D.".into()))?;
                    if *owner != ctx_id || prg.disposed {
                        return Err(Fte3dError::IllegalOperation(
                            "Program3D is disposed or belongs to another Context3D.".into(),
                        ));
                    }
                }
                self.ctx_mut(ctx_id)?.program_id = pid;
            }
            "setConstants" => {
                let ptype = args["programType"].as_str().unwrap_or("");
                validate_stage3d_enum("programType", ptype)?;
                let first = args["firstRegister"].as_i64().unwrap_or(-1);
                let values: Vec<f32> = args["values"]
                    .as_array()
                    .ok_or_else(|| Fte3dError::Argument("Constants require float array.".into()))?
                    .iter()
                    .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                    .collect();
                let num_regs = args["numRegisters"]
                    .as_i64()
                    .unwrap_or((values.len() / 4) as i64);
                let count = if num_regs < 0 {
                    values.len() / 4
                } else {
                    num_regs as usize
                };
                if values.len() < count * 4 || first < 0 {
                    return Err(Fte3dError::Range(
                        "Insufficient float components for setProgramConstants.".into(),
                    ));
                }
                let c = self.ctx_mut(ctx_id)?;
                let first_u = first as usize;
                if ptype == "vertex" {
                    if first_u + count > c.vc.len() {
                        return Err(Fte3dError::Range(
                            "Vertex constant register index out of bounds.".into(),
                        ));
                    }
                    for r in 0..count {
                        let o = r * 4;
                        c.vc[first_u + r] =
                            [values[o], values[o + 1], values[o + 2], values[o + 3]];
                    }
                    c.vc_touched = true;
                } else {
                    if first_u + count > c.fc.len() {
                        return Err(Fte3dError::Range(
                            "Fragment constant register index out of bounds.".into(),
                        ));
                    }
                    for r in 0..count {
                        let o = r * 4;
                        c.fc[first_u + r] =
                            [values[o], values[o + 1], values[o + 2], values[o + 3]];
                    }
                    c.fc_touched = true;
                }
            }
            "setVertexBufferAt" => {
                let idx = args["index"].as_i64().unwrap_or(-1);
                if !(0..8).contains(&idx) {
                    return Err(Fte3dError::Range(
                        "Vertex attribute stream index must be 0..7.".into(),
                    ));
                }
                if args["bufferId"].is_null() {
                    self.ctx_mut(ctx_id)?.attrs[idx as usize] = None;
                } else {
                    let bid = args["bufferId"].as_u64().unwrap_or(0) as u32;
                    let offset = args["bufferOffset"].as_i64().unwrap_or(0);
                    let fmt = args["format"].as_str().unwrap_or("float4");
                    validate_stage3d_enum("vertexFormat", fmt)?;
                    let (owner, vb) = self
                        .vertex_buffers
                        .get(&bid)
                        .ok_or_else(|| Fte3dError::Argument("Unknown VertexBuffer3D.".into()))?;
                    if *owner != ctx_id || vb.disposed {
                        return Err(Fte3dError::IllegalOperation(
                            "VertexBuffer3D is disposed or foreign.".into(),
                        ));
                    }
                    let comps = match fmt {
                        "float1" | "bytes4" => 1,
                        "float2" => 2,
                        "float3" => 3,
                        _ => 4,
                    };
                    if offset < 0 || (offset as usize) + comps > vb.data32_per_vertex {
                        return Err(Fte3dError::Range(
                            "Vertex attribute offset + format exceeds buffer stride.".into(),
                        ));
                    }
                    self.ctx_mut(ctx_id)?.attrs[idx as usize] = Some(BoundAttr {
                        buffer_id: bid,
                        offset: offset as usize,
                        format: fmt.to_owned(),
                    });
                }
            }
            "setTextureAt" => {
                let sampler = args["sampler"].as_i64().unwrap_or(-1);
                if !(0..8).contains(&sampler) {
                    return Err(Fte3dError::Range("Sampler index must be 0..7.".into()));
                }
                if args["textureId"].is_null() {
                    self.ctx_mut(ctx_id)?.textures[sampler as usize] = None;
                } else {
                    let tid = args["textureId"].as_u64().unwrap_or(0) as u32;
                    let (owner, tex) = self
                        .textures
                        .get(&tid)
                        .ok_or_else(|| Fte3dError::Argument("Unknown TextureBase.".into()))?;
                    if *owner != ctx_id || tex.disposed {
                        return Err(Fte3dError::IllegalOperation(
                            "Texture is disposed or belongs to another Context3D.".into(),
                        ));
                    }
                    self.ctx_mut(ctx_id)?.textures[sampler as usize] = Some(tid);
                }
            }
            "setSamplerStateAt" => {
                let sampler = args["sampler"].as_i64().unwrap_or(-1);
                if !(0..8).contains(&sampler) {
                    return Err(Fte3dError::Range("Sampler index must be 0..7.".into()));
                }
                let wrap = validate_stage3d_enum("wrapMode", args["wrap"].as_str().unwrap_or(""))?;
                let filter =
                    validate_stage3d_enum("textureFilter", args["filter"].as_str().unwrap_or(""))?;
                let mip =
                    validate_stage3d_enum("mipFilter", args["mipfilter"].as_str().unwrap_or(""))?;
                self.ctx_mut(ctx_id)?.samplers[sampler as usize] = SamplerState {
                    wrap: wrap.to_owned(),
                    filter: filter.to_owned(),
                    mipfilter: mip.to_owned(),
                };
            }
            "setBlendFactors" => {
                let src =
                    validate_stage3d_enum("blendFactor", args["source"].as_str().unwrap_or(""))?;
                let dst = validate_stage3d_enum(
                    "blendFactor",
                    args["destination"].as_str().unwrap_or(""),
                )?;
                let c = self.ctx_mut(ctx_id)?;
                c.blend_src = src.to_owned();
                c.blend_dst = dst.to_owned();
            }
            "setColorMask" => {
                let c = self.ctx_mut(ctx_id)?;
                c.color_mask = [
                    args["r"].as_bool().unwrap_or(true),
                    args["g"].as_bool().unwrap_or(true),
                    args["b"].as_bool().unwrap_or(true),
                    args["a"].as_bool().unwrap_or(true),
                ];
            }
            "setCulling" => {
                let face =
                    validate_stage3d_enum("triangleFace", args["face"].as_str().unwrap_or(""))?;
                self.ctx_mut(ctx_id)?.culling = face.to_owned();
            }
            "setDepthTest" => {
                let mask = args["depthMask"].as_bool().unwrap_or(true);
                let cmp = validate_stage3d_enum(
                    "compareMode",
                    args["compareMode"].as_str().unwrap_or(""),
                )?;
                let c = self.ctx_mut(ctx_id)?;
                c.depth_mask = mask;
                c.depth_compare = cmp.to_owned();
            }
            "setFillMode" => {
                let fm =
                    validate_stage3d_enum("fillMode", args["fillMode"].as_str().unwrap_or(""))?;
                self.ctx_mut(ctx_id)?.fill_mode = fm.to_owned();
            }
            "setScissorRectangle" => {
                let c = self.ctx_mut(ctx_id)?;
                if args["rect"].is_null() {
                    c.scissor = None;
                } else if let Some(arr) = args["rect"].as_array()
                    && arr.len() == 4
                {
                    c.scissor = Some([
                        arr[0].as_f64().unwrap_or(0.0).round() as i32,
                        arr[1].as_f64().unwrap_or(0.0).round() as i32,
                        arr[2].as_f64().unwrap_or(0.0).round().max(0.0) as i32,
                        arr[3].as_f64().unwrap_or(0.0).round().max(0.0) as i32,
                    ]);
                }
            }
            "setStencilActions" => {
                let face = validate_stage3d_enum(
                    "triangleFace",
                    args["triangleFace"].as_str().unwrap_or("frontAndBack"),
                )?;
                let cmp = validate_stage3d_enum(
                    "compareMode",
                    args["compareMode"].as_str().unwrap_or("always"),
                )?;
                let both = validate_stage3d_enum(
                    "stencilAction",
                    args["actionOnBothPass"].as_str().unwrap_or("keep"),
                )?;
                let dfail = validate_stage3d_enum(
                    "stencilAction",
                    args["actionOnDepthFail"].as_str().unwrap_or("keep"),
                )?;
                let sfail = validate_stage3d_enum(
                    "stencilAction",
                    args["actionOnDepthPassStencilFail"]
                        .as_str()
                        .unwrap_or("keep"),
                )?;
                let c = self.ctx_mut(ctx_id)?;
                c.stencil_face = face.to_owned();
                c.stencil_compare = cmp.to_owned();
                c.stencil_both_pass = both.to_owned();
                c.stencil_depth_fail = dfail.to_owned();
                c.stencil_pass_fail = sfail.to_owned();
            }
            "setStencilReferenceValue" => {
                let c = self.ctx_mut(ctx_id)?;
                c.stencil_ref = (args["referenceValue"].as_u64().unwrap_or(0) & 0xff) as u8;
                c.stencil_read_mask = (args["readMask"].as_u64().unwrap_or(0xff) & 0xff) as u8;
                c.stencil_write_mask = (args["writeMask"].as_u64().unwrap_or(0xff) & 0xff) as u8;
            }
            "setRenderToTexture" => {
                let tid = args["textureId"].as_u64().unwrap_or(0) as u32;
                let enable_ds = args["enableDepthAndStencil"].as_bool().unwrap_or(false);
                let selector = args["surfaceSelector"].as_i64().unwrap_or(0).max(0) as usize;
                let (owner, tex) = self
                    .textures
                    .get(&tid)
                    .ok_or_else(|| Fte3dError::Argument("Unknown render target texture.".into()))?;
                if *owner != ctx_id || tex.disposed {
                    return Err(Fte3dError::IllegalOperation(
                        "Render target texture is disposed or foreign.".into(),
                    ));
                }
                if selector >= tex.faces.len() {
                    return Err(Fte3dError::Argument(
                        "Render target surfaceSelector out of range.".into(),
                    ));
                }
                let pixels = (tex.width as usize) * (tex.height as usize);
                let c = self.ctx_mut(ctx_id)?;
                c.rtt = Some(RenderTarget {
                    texture_id: tid,
                    surface_selector: selector,
                    enable_depth_stencil: enable_ds,
                    depth: if enable_ds {
                        vec![1.0f32; pixels]
                    } else {
                        Vec::new()
                    },
                    stencil: if enable_ds {
                        vec![0u8; pixels]
                    } else {
                        Vec::new()
                    },
                });
            }
            "setRenderToBackBuffer" => {
                self.ctx_mut(ctx_id)?.rtt = None;
            }
            _ => {
                return Err(Fte3dError::Argument(format!(
                    "Unknown Stage3D state command '{op}'."
                )));
            }
        }
        Ok(())
    }

    pub fn draw_triangles(
        &mut self,
        ctx_id: u32,
        ib_id: u32,
        first_index: i32,
        num_triangles: i32,
    ) -> Result<(), Fte3dError> {
        let c_snap = self.ctx(ctx_id)?.clone();
        let (ib_owner, ib) = self
            .index_buffers
            .get(&ib_id)
            .ok_or_else(|| Fte3dError::Argument("Unknown IndexBuffer3D.".into()))?;
        if *ib_owner != ctx_id || ib.disposed {
            return Err(Fte3dError::IllegalOperation(
                "IndexBuffer3D is disposed or foreign.".into(),
            ));
        }
        let first = first_index.max(0) as usize;
        if first > ib.data.len() {
            return Err(Fte3dError::Range(
                "drawTriangles firstIndex exceeds IndexBuffer3D length.".into(),
            ));
        }
        let tri_count = if num_triangles < 0 {
            (ib.data.len() - first) / 3
        } else {
            num_triangles as usize
        };
        if first + tri_count * 3 > ib.data.len() {
            return Err(Fte3dError::Range(
                "drawTriangles index range exceeds IndexBuffer3D length.".into(),
            ));
        }
        let indices: Vec<u16> = ib.data[first..first + tri_count * 3].to_vec();

        let (vs_prog, fs_prog) = if let Some(pid) = c_snap.program_id
            && let Some((_, prg)) = self.programs.get(&pid)
            && prg.uploaded
            && !prg.disposed
        {
            (prg.vertex.clone(), prg.fragment.clone())
        } else {
            (CompiledShader::Fallback, CompiledShader::Fallback)
        };

        let tex_map: BTreeMap<u32, TextureRecord> = self
            .textures
            .iter()
            .filter(|(_, (owner, t))| *owner == ctx_id && !t.disposed)
            .map(|(k, (_, t))| (*k, t.clone()))
            .collect();

        let (target_w, target_h) = if let Some(rtt) = &c_snap.rtt
            && let Some(tex) = tex_map.get(&rtt.texture_id)
        {
            (tex.width, tex.height)
        } else {
            (c_snap.back_buffer_width, c_snap.back_buffer_height)
        };
        if target_w == 0 || target_h == 0 {
            return Err(Fte3dError::IllegalOperation(
                "Context3D.drawTriangles requires an active backbuffer or render texture.".into(),
            ));
        }

        let fetch_vertex_attrs = |v_idx: usize| -> [[f32; 4]; 8] {
            let mut out = [[0.0f32, 0.0, 0.0, 1.0]; 8];
            for (slot, bound) in c_snap.attrs.iter().enumerate() {
                if let Some(b) = bound
                    && let Some((_, vb)) = self.vertex_buffers.get(&b.buffer_id)
                    && v_idx < vb.num_vertices
                {
                    let base = v_idx * vb.data32_per_vertex + b.offset;
                    match b.format.as_str() {
                        "float1" => {
                            out[slot] = [vb.data[base], 0.0, 0.0, 1.0];
                        }
                        "float2" => {
                            out[slot] = [vb.data[base], vb.data[base + 1], 0.0, 1.0];
                        }
                        "float3" => {
                            out[slot] = [vb.data[base], vb.data[base + 1], vb.data[base + 2], 1.0];
                        }
                        "bytes4" => {
                            let bits = vb.data[base].to_bits();
                            out[slot] = [
                                (bits & 0xff) as f32 / 255.0,
                                ((bits >> 8) & 0xff) as f32 / 255.0,
                                ((bits >> 16) & 0xff) as f32 / 255.0,
                                ((bits >> 24) & 0xff) as f32 / 255.0,
                            ];
                        }
                        _ => {
                            out[slot] = [
                                vb.data[base],
                                vb.data[base + 1],
                                vb.data[base + 2],
                                vb.data[base + 3],
                            ];
                        }
                    }
                }
            }
            out
        };

        let run_vs = |attrs: &[[f32; 4]; 8]| -> ([f32; 4], [[f32; 4]; 8]) {
            let mut varyings = [[0.0f32; 4]; 8];
            match &vs_prog {
                CompiledShader::Agal(insts) => {
                    let pos = run_agal(
                        insts,
                        true,
                        attrs,
                        &c_snap.vc,
                        &mut varyings,
                        &c_snap.textures,
                        &c_snap.samplers,
                        &tex_map,
                    )
                    .unwrap_or([0.0, 0.0, 0.0, 1.0]);
                    (pos, varyings)
                }
                CompiledShader::Fallback => {
                    varyings[0] = if c_snap.attrs[1].is_some() {
                        attrs[1]
                    } else if c_snap.fc_touched {
                        c_snap.fc[0]
                    } else {
                        [1.0, 1.0, 1.0, 1.0]
                    };
                    let p = attrs[0];
                    let pos = if c_snap.vc_touched {
                        let dp =
                            |r: [f32; 4]| p[0] * r[0] + p[1] * r[1] + p[2] * r[2] + p[3] * r[3];
                        [
                            dp(c_snap.vc[0]),
                            dp(c_snap.vc[1]),
                            dp(c_snap.vc[2]),
                            dp(c_snap.vc[3]),
                        ]
                    } else {
                        p
                    };
                    (pos, varyings)
                }
            }
        };

        let sc_min_x = c_snap.scissor.map_or(0, |s| s[0].max(0));
        let sc_min_y = c_snap.scissor.map_or(0, |s| s[1].max(0));
        let sc_max_x = c_snap
            .scissor
            .map_or(target_w as i32, |s| (s[0] + s[2]).min(target_w as i32));
        let sc_max_y = c_snap
            .scissor
            .map_or(target_h as i32, |s| (s[1] + s[3]).min(target_h as i32));

        let mut shaded_writes: Vec<(usize, [f32; 4], f32, bool)> = Vec::new();
        let dummy_attrs = [[0.0f32; 4]; 8];

        for tri in indices.chunks_exact(3) {
            let a0 = fetch_vertex_attrs(tri[0] as usize);
            let a1 = fetch_vertex_attrs(tri[1] as usize);
            let a2 = fetch_vertex_attrs(tri[2] as usize);
            let (clip0, var0) = run_vs(&a0);
            let (clip1, var1) = run_vs(&a1);
            let (clip2, var2) = run_vs(&a2);

            let to_screen = |c: [f32; 4]| -> [f32; 4] {
                let inv_w = if c[3].abs() > 1e-7 { 1.0 / c[3] } else { 1.0 };
                let ndc_x = c[0] * inv_w;
                let ndc_y = c[1] * inv_w;
                let ndc_z = (c[2] * inv_w).clamp(0.0, 1.0);
                let sx = (ndc_x * 0.5 + 0.5) * (target_w as f32);
                let sy = (0.5 - ndc_y * 0.5) * (target_h as f32);
                [sx, sy, ndc_z, inv_w]
            };

            let s0 = to_screen(clip0);
            let s1 = to_screen(clip1);
            let s2 = to_screen(clip2);

            let area = (s1[0] - s0[0]) * (s2[1] - s0[1]) - (s1[1] - s0[1]) * (s2[0] - s0[0]);
            if c_snap.culling == "frontAndBack" || area.abs() < 1e-6 {
                continue;
            }
            let is_front = area > 0.0;
            if (c_snap.culling == "back" && !is_front) || (c_snap.culling == "front" && is_front) {
                continue;
            }

            let eval_frag = |vars: &mut [[f32; 4]; 8]| -> Option<[f32; 4]> {
                match &fs_prog {
                    CompiledShader::Agal(insts) => run_agal(
                        insts,
                        false,
                        &dummy_attrs,
                        &c_snap.fc,
                        vars,
                        &c_snap.textures,
                        &c_snap.samplers,
                        &tex_map,
                    ),
                    CompiledShader::Fallback => {
                        if let Some(tid) = c_snap.textures[0]
                            && let Some(tex) = tex_map.get(&tid)
                        {
                            let st = &c_snap.samplers[0];
                            Some(tex.sample_coord(vars[0], &st.wrap, &st.filter, &st.mipfilter))
                        } else {
                            Some(vars[0])
                        }
                    }
                }
            };

            if c_snap.fill_mode == "wireframe" {
                let edges = [
                    (s0, var0, s1, var1),
                    (s1, var1, s2, var2),
                    (s2, var2, s0, var0),
                ];
                for (p0, v0, p1, v1) in edges {
                    let steps = ((p1[0] - p0[0]).abs().max((p1[1] - p0[1]).abs()).ceil() as i32)
                        .clamp(1, 4096);
                    for step in 0..=steps {
                        let t = (step as f32) / (steps as f32);
                        let px = (p0[0] + (p1[0] - p0[0]) * t).floor() as i32;
                        let py = (p0[1] + (p1[1] - p0[1]) * t).floor() as i32;
                        if px < sc_min_x || px >= sc_max_x || py < sc_min_y || py >= sc_max_y {
                            continue;
                        }
                        let pz = p0[2] + (p1[2] - p0[2]) * t;
                        let mut vars = [[0.0f32; 4]; 8];
                        for slot in 0..8 {
                            for ch in 0..4 {
                                vars[slot][ch] = v0[slot][ch] * (1.0 - t) + v1[slot][ch] * t;
                            }
                        }
                        if let Some(col) = eval_frag(&mut vars) {
                            let idx = (py as usize) * (target_w as usize) + (px as usize);
                            shaded_writes.push((idx, col, pz, is_front));
                        }
                    }
                }
                continue;
            }

            let min_x = s0[0].min(s1[0]).min(s2[0]).floor() as i32;
            let max_x = s0[0].max(s1[0]).max(s2[0]).ceil() as i32;
            let min_y = s0[1].min(s1[1]).min(s2[1]).floor() as i32;
            let max_y = s0[1].max(s1[1]).max(s2[1]).ceil() as i32;

            let x0 = min_x.max(sc_min_x);
            let x1 = max_x.min(sc_max_x);
            let y0 = min_y.max(sc_min_y);
            let y1 = max_y.min(sc_max_y);
            let inv_area = 1.0 / area;

            for py in y0..y1 {
                let fy = py as f32 + 0.5;
                for px in x0..x1 {
                    let fx = px as f32 + 0.5;
                    let w0 = ((s1[0] - fx) * (s2[1] - fy) - (s1[1] - fy) * (s2[0] - fx)) * inv_area;
                    let w1 = ((s2[0] - fx) * (s0[1] - fy) - (s2[1] - fy) * (s0[0] - fx)) * inv_area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-5 || w1 < -1e-5 || w2 < -1e-5 {
                        continue;
                    }
                    let pz = (w0 * s0[2] + w1 * s1[2] + w2 * s2[2]).clamp(0.0, 1.0);
                    let denom = (w0 * s0[3] + w1 * s1[3] + w2 * s2[3]).max(1e-7);
                    let pw0 = (w0 * s0[3]) / denom;
                    let pw1 = (w1 * s1[3]) / denom;
                    let pw2 = (w2 * s2[3]) / denom;

                    let mut vars = [[0.0f32; 4]; 8];
                    for slot in 0..8 {
                        for ch in 0..4 {
                            vars[slot][ch] =
                                pw0 * var0[slot][ch] + pw1 * var1[slot][ch] + pw2 * var2[slot][ch];
                        }
                    }
                    if let Some(col) = eval_frag(&mut vars) {
                        let idx = (py as usize) * (target_w as usize) + (px as usize);
                        shaded_writes.push((idx, col, pz, is_front));
                    }
                }
            }
        }

        // Commit shaded fragments to active render target (RTT or backbuffer)
        let rtt_target = self
            .ctx(ctx_id)?
            .rtt
            .as_ref()
            .map(|r| (r.texture_id, r.surface_selector, r.enable_depth_stencil));

        for (idx, src_raw, pz, _is_front) in shaded_writes {
            let src = [
                src_raw[0].clamp(0.0, 1.0),
                src_raw[1].clamp(0.0, 1.0),
                src_raw[2].clamp(0.0, 1.0),
                src_raw[3].clamp(0.0, 1.0),
            ];

            if let Some((tid, face, has_ds)) = rtt_target {
                let mut write_color = true;
                {
                    let c = self.ctx_mut(ctx_id)?;
                    if has_ds
                        && let Some(rtt) = &mut c.rtt
                        && idx < rtt.depth.len()
                    {
                        let st_cur = rtt.stencil[idx];
                        let st_pass = compare_stencil(
                            &c_snap.stencil_compare,
                            c_snap.stencil_ref,
                            st_cur,
                            c_snap.stencil_read_mask,
                        );
                        let dp_pass = compare_pass(&c_snap.depth_compare, pz, rtt.depth[idx]);
                        if !st_pass {
                            rtt.stencil[idx] = apply_stencil_op(
                                &c_snap.stencil_pass_fail,
                                st_cur,
                                c_snap.stencil_ref,
                                c_snap.stencil_write_mask,
                            );
                            write_color = false;
                        } else if !dp_pass {
                            rtt.stencil[idx] = apply_stencil_op(
                                &c_snap.stencil_depth_fail,
                                st_cur,
                                c_snap.stencil_ref,
                                c_snap.stencil_write_mask,
                            );
                            write_color = false;
                        } else {
                            rtt.stencil[idx] = apply_stencil_op(
                                &c_snap.stencil_both_pass,
                                st_cur,
                                c_snap.stencil_ref,
                                c_snap.stencil_write_mask,
                            );
                            if c_snap.depth_mask {
                                rtt.depth[idx] = pz;
                            }
                        }
                    }
                }
                if write_color
                    && let Some((_, tex)) = self.textures.get_mut(&tid)
                    && let Some(buf) = tex.faces.get_mut(face)
                {
                    blend_pixel(&mut buf[idx * 4..idx * 4 + 4], src, &c_snap);
                }
            } else {
                let c = self.ctx_mut(ctx_id)?;
                if c.enable_depth_and_stencil && idx < c.depth_buffer.len() {
                    let st_cur = c.stencil_buffer[idx];
                    let st_pass = compare_stencil(
                        &c_snap.stencil_compare,
                        c_snap.stencil_ref,
                        st_cur,
                        c_snap.stencil_read_mask,
                    );
                    let dp_pass = compare_pass(&c_snap.depth_compare, pz, c.depth_buffer[idx]);
                    if !st_pass {
                        c.stencil_buffer[idx] = apply_stencil_op(
                            &c_snap.stencil_pass_fail,
                            st_cur,
                            c_snap.stencil_ref,
                            c_snap.stencil_write_mask,
                        );
                        continue;
                    }
                    if !dp_pass {
                        c.stencil_buffer[idx] = apply_stencil_op(
                            &c_snap.stencil_depth_fail,
                            st_cur,
                            c_snap.stencil_ref,
                            c_snap.stencil_write_mask,
                        );
                        continue;
                    }
                    c.stencil_buffer[idx] = apply_stencil_op(
                        &c_snap.stencil_both_pass,
                        st_cur,
                        c_snap.stencil_ref,
                        c_snap.stencil_write_mask,
                    );
                    if c_snap.depth_mask {
                        c.depth_buffer[idx] = pz;
                    }
                }
                let off = idx * 4;
                if off + 4 <= c.color_buffer.len() {
                    blend_pixel(&mut c.color_buffer[off..off + 4], src, &c_snap);
                }
            }
        }
        Ok(())
    }

    pub fn present(&mut self, ctx_id: u32) -> Result<(), Fte3dError> {
        let c = self.ctx_mut(ctx_id)?;
        if c.rtt.is_some() {
            return Err(Fte3dError::IllegalOperation(
                "Context3D.present cannot be called while rendering to a texture; call setRenderToBackBuffer() first.".into(),
            ));
        }
        if c.back_buffer_width == 0 || c.back_buffer_height == 0 {
            return Err(Fte3dError::IllegalOperation(
                "Context3D.present requires configureBackBuffer first.".into(),
            ));
        }
        c.presented_width = c.back_buffer_width;
        c.presented_height = c.back_buffer_height;
        c.presented_buffer.clone_from(&c.color_buffer);
        Ok(())
    }

    pub fn presented_rgba(&self, ctx_id: u32) -> Result<(u32, u32, Vec<u8>), Fte3dError> {
        let c = self.ctx(ctx_id)?;
        if c.presented_width == 0 || c.presented_height == 0 || c.presented_buffer.is_empty() {
            return Ok((0, 0, Vec::new()));
        }
        Ok((
            c.presented_width,
            c.presented_height,
            c.presented_buffer.clone(),
        ))
    }

    pub fn backbuffer_rgba(&self, ctx_id: u32) -> Result<(u32, u32, Vec<u8>), Fte3dError> {
        let c = self.ctx(ctx_id)?;
        if c.back_buffer_width == 0 || c.back_buffer_height == 0 {
            return Err(Fte3dError::IllegalOperation(
                "Context3D.drawToBitmapData requires configureBackBuffer first.".into(),
            ));
        }
        Ok((
            c.back_buffer_width,
            c.back_buffer_height,
            c.color_buffer.clone(),
        ))
    }
}

fn blend_pixel(dst_bytes: &mut [u8], src: [f32; 4], c: &Context3dState) {
    let dst = [
        dst_bytes[0] as f32 / 255.0,
        dst_bytes[1] as f32 / 255.0,
        dst_bytes[2] as f32 / 255.0,
        dst_bytes[3] as f32 / 255.0,
    ];
    let sf = blend_factor_vec(&c.blend_src, src, dst);
    let df = blend_factor_vec(&c.blend_dst, src, dst);
    for ch in 0..4 {
        if c.color_mask[ch] {
            let v = (src[ch] * sf[ch] + dst[ch] * df[ch]).clamp(0.0, 1.0);
            dst_bytes[ch] = (v * 255.0).round() as u8;
        }
    }
}
