//! Shared ActionScript 3 Area 8 runtime engines:
//! 1. Flash Text Engine (`flash.text.engine.*`) paragraph layout, atomization,
//!    justification, tab stops, word/atom boundary queries, and font metrics.
//! 2. Stage3D (`flash.display3D.*`) state machine, AGAL v1/v2 shader interpreter,
//!    buffer/texture arenas, and software triangle/wireframe rasterizer.
//! 3. Pixel Bender (`flash.display.Shader*`, `flash.filters.ShaderFilter`) `.pbj`
//!    and portable shader metadata decoder & per-pixel RGBA/float32 evaluator,
//!    plus 3D `DisplayObject` / `Transform` matrix and projection algebra.

#![allow(clippy::chunks_exact_to_as_chunks)]

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const FTE_MAX_LINE_WIDTH: f64 = 1_000_000.0;
pub const FTE_GRAPHIC_CHAR: char = '\u{FDEF}';
pub const MAX_STAGE3D_DIM: u32 = 4096;
pub const MAX_STAGE3D_BUFFERS: usize = 1024;
pub const MAX_STAGE3D_VERTICES: usize = 65_536;
pub const MAX_STAGE3D_INDICES: usize = 196_608;
pub const MAX_SHADER_PIXELS: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Fte3dError {
    #[error("ArgumentError: {0}")]
    Argument(String),
    #[error("RangeError: {0}")]
    Range(String),
    #[error("IllegalOperationError: {0}")]
    IllegalOperation(String),
    #[error("TypeError: {0}")]
    Type(String),
}

mod fte;
mod geom3d;
mod pixel_bender;
mod stage3d;
mod stage3d_agal;
#[cfg(test)]
mod tests;

pub use fte::*;
pub use geom3d::*;
pub use pixel_bender::*;
pub use stage3d::*;
pub use stage3d_agal::validate_stage3d_enum;

#[derive(Debug, Default)]
pub struct Fte3dRuntime {
    pub stage3d: Stage3dArena,
}

impl Fte3dRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.stage3d = Stage3dArena::default();
    }
}
