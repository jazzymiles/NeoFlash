//! Bounded CPU display filters. The host rasterizes a complete display
//! subtree in logical stage pixels; Rust applies ordered filters to straight
//! RGBA bytes. Blur uses repeated separable box passes in premultiplied space.
//! This portable profile does not claim Flash's exact subpixel/rounding kernel.
//! Bevel composition adapts Ruffle's MIT-licensed shader algebra into this CPU
//! pipeline; see docs/RUNTIME-FILTERS-PROVENANCE.md for the pinned source.
use serde::{Deserialize, Serialize};

pub const MAX_FILTER_PIXELS: usize = 4 * 1024 * 1024;
pub const MAX_OBJECT_FILTERS: usize = 8;
const MAX_FILTER_WORK: usize = 128 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("Display filters: {0}")]
pub struct FilterError(pub String);
fn error(message: &str) -> FilterError {
    FilterError(message.into())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "FilterSpecWire", into = "FilterSpecWire")]
pub enum FilterSpec {
    Blur {
        blur_x: f64,
        blur_y: f64,
        quality: u8,
    },
    Glow {
        color: u32,
        alpha: f64,
        blur_x: f64,
        blur_y: f64,
        strength: f64,
        quality: u8,
        inner: bool,
        knockout: bool,
    },
    DropShadow {
        distance: f64,
        angle: f64,
        color: u32,
        alpha: f64,
        blur_x: f64,
        blur_y: f64,
        strength: f64,
        quality: u8,
        inner: bool,
        knockout: bool,
        hide_object: bool,
    },
    ColorMatrix {
        matrix: [f64; 20],
    },
    Convolution {
        matrix_x: u8,
        matrix_y: u8,
        matrix: Vec<f64>,
        divisor: f64,
        bias: f64,
        preserve_alpha: bool,
        clamp: bool,
        color: u32,
        alpha: f64,
    },
    Bevel {
        distance: f64,
        angle: f64,
        highlight_color: u32,
        highlight_alpha: f64,
        shadow_color: u32,
        shadow_alpha: f64,
        blur_x: f64,
        blur_y: f64,
        strength: f64,
        quality: u8,
        placement: String,
        knockout: bool,
    },
    GradientGlow {
        distance: f64,
        angle: f64,
        colors: Vec<u32>,
        alphas: Vec<f64>,
        ratios: Vec<u8>,
        blur_x: f64,
        blur_y: f64,
        strength: f64,
        quality: u8,
        placement: String,
        knockout: bool,
    },
    GradientBevel {
        distance: f64,
        angle: f64,
        colors: Vec<u32>,
        alphas: Vec<f64>,
        ratios: Vec<u8>,
        blur_x: f64,
        blur_y: f64,
        strength: f64,
        quality: u8,
        placement: String,
        knockout: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DisplacementSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_bitmap: Option<crate::PixelRef>,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub map_width: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub map_height: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub map_rgba: Vec<u8>,
    pub map_point: [i32; 2],
    pub component_x: u8,
    pub component_y: u8,
    pub scale_x: f64,
    pub scale_y: f64,
    pub mode: String,
    pub color: u32,
    pub alpha: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShaderSpec {
    pub left_extension: i32,
    pub top_extension: i32,
    pub right_extension: i32,
    pub bottom_extension: i32,
    pub bytecode_base64: String,
    pub precision_hint: String,
    pub params_json: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum FilterSpecWire {
    Blur {
        blur_x: f64,
        blur_y: f64,
        quality: u8,
        #[serde(default = "default_true", skip_serializing_if = "is_true")]
        enabled: bool,
    },
    Glow {
        color: u32,
        alpha: f64,
        blur_x: f64,
        blur_y: f64,
        strength: f64,
        quality: u8,
        inner: bool,
        knockout: bool,
        #[serde(default = "default_true", skip_serializing_if = "is_true")]
        enabled: bool,
    },
    DropShadow {
        distance: f64,
        angle: f64,
        color: u32,
        alpha: f64,
        blur_x: f64,
        blur_y: f64,
        strength: f64,
        quality: u8,
        inner: bool,
        knockout: bool,
        hide_object: bool,
        #[serde(default = "default_true", skip_serializing_if = "is_true")]
        enabled: bool,
    },
    ColorMatrix {
        matrix: [f64; 20],
        #[serde(default = "default_true", skip_serializing_if = "is_true")]
        enabled: bool,
    },
    Convolution {
        matrix_x: u8,
        matrix_y: u8,
        matrix: Vec<f64>,
        divisor: f64,
        bias: f64,
        preserve_alpha: bool,
        clamp: bool,
        color: u32,
        alpha: f64,
        #[serde(default = "default_true", skip_serializing_if = "is_true")]
        enabled: bool,
    },
    Bevel {
        distance: f64,
        angle: f64,
        highlight_color: u32,
        highlight_alpha: f64,
        shadow_color: u32,
        shadow_alpha: f64,
        blur_x: f64,
        blur_y: f64,
        strength: f64,
        quality: u8,
        placement: String,
        knockout: bool,
        #[serde(default = "default_true", skip_serializing_if = "is_true")]
        enabled: bool,
    },
    GradientGlow {
        distance: f64,
        angle: f64,
        colors: Vec<u32>,
        alphas: Vec<f64>,
        ratios: Vec<u8>,
        blur_x: f64,
        blur_y: f64,
        strength: f64,
        quality: u8,
        placement: String,
        knockout: bool,
        #[serde(default = "default_true", skip_serializing_if = "is_true")]
        enabled: bool,
    },
    GradientBevel {
        distance: f64,
        angle: f64,
        colors: Vec<u32>,
        alphas: Vec<f64>,
        ratios: Vec<u8>,
        blur_x: f64,
        blur_y: f64,
        strength: f64,
        quality: u8,
        placement: String,
        knockout: bool,
        #[serde(default = "default_true", skip_serializing_if = "is_true")]
        enabled: bool,
    },
    Displacement {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        map_bitmap: Option<crate::PixelRef>,
        #[serde(default, skip_serializing_if = "is_zero_u32")]
        map_width: u32,
        #[serde(default, skip_serializing_if = "is_zero_u32")]
        map_height: u32,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        map_rgba: Vec<u8>,
        map_point: [i32; 2],
        component_x: u8,
        component_y: u8,
        scale_x: f64,
        scale_y: f64,
        mode: String,
        color: u32,
        alpha: f64,
        #[serde(default = "default_true", skip_serializing_if = "is_true")]
        enabled: bool,
    },
    Shader {
        left_extension: i32,
        top_extension: i32,
        right_extension: i32,
        bottom_extension: i32,
        bytecode_base64: String,
        precision_hint: String,
        params_json: String,
        #[serde(default = "default_true", skip_serializing_if = "is_true")]
        enabled: bool,
    },
}

fn default_true() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

const RUNTIME_FILTER_BIAS_MAGIC: f64 = -999_999_123.5;
const FILTER_DISABLED_QUALITY_BIT: u8 = 0x80;
const LEGACY_COLOR_MATRIX_DISABLED_TAG: f64 = -32_768.25;
// Authored coefficients are validated in -65536..=65536. Adding this offset stores every
// possible coefficient in a disjoint finite range while the filter is disabled, so toggling it
// never destroys matrix[15]. The legacy singleton tag remains readable as a disabled identity
// coefficient for documents written before the reversible encoding.
const COLOR_MATRIX_DISABLED_OFFSET: f64 = 131_073.0;

fn color_matrix_value_is_disabled(value: f64) -> bool {
    value > 65_536.0 || (value - LEGACY_COLOR_MATRIX_DISABLED_TAG).abs() <= 1e-9
}

fn decoded_color_matrix_value(value: f64) -> f64 {
    if value > 65_536.0 {
        value - COLOR_MATRIX_DISABLED_OFFSET
    } else if (value - LEGACY_COLOR_MATRIX_DISABLED_TAG).abs() <= 1e-9 {
        0.0
    } else {
        value
    }
}

impl FilterSpec {
    /// Whether this filter is active in the stack. Disabled filters remain in the
    /// document and inspector stack without applying padding or pixel passes.
    pub fn is_enabled(&self) -> bool {
        match self {
            Self::Blur { quality, .. }
            | Self::Glow { quality, .. }
            | Self::DropShadow { quality, .. }
            | Self::Bevel { quality, .. }
            | Self::GradientGlow { quality, .. }
            | Self::GradientBevel { quality, .. } => (*quality & FILTER_DISABLED_QUALITY_BIT) == 0,
            Self::ColorMatrix { matrix } => !color_matrix_value_is_disabled(matrix[15]),
            Self::Convolution { matrix_x, .. } => (*matrix_x & FILTER_DISABLED_QUALITY_BIT) == 0,
        }
    }

    /// Enable or disable this filter in-place while preserving all parameters.
    pub fn set_enabled(&mut self, enabled: bool) {
        match self {
            Self::Blur { quality, .. }
            | Self::Glow { quality, .. }
            | Self::DropShadow { quality, .. }
            | Self::Bevel { quality, .. }
            | Self::GradientGlow { quality, .. }
            | Self::GradientBevel { quality, .. } => {
                if enabled {
                    *quality &= !FILTER_DISABLED_QUALITY_BIT;
                } else {
                    *quality |= FILTER_DISABLED_QUALITY_BIT;
                }
            }
            Self::ColorMatrix { matrix } => {
                if enabled {
                    matrix[15] = decoded_color_matrix_value(matrix[15]);
                } else if !color_matrix_value_is_disabled(matrix[15]) {
                    matrix[15] += COLOR_MATRIX_DISABLED_OFFSET;
                }
            }
            Self::Convolution { matrix_x, .. } => {
                if enabled {
                    *matrix_x &= !FILTER_DISABLED_QUALITY_BIT;
                } else {
                    *matrix_x |= FILTER_DISABLED_QUALITY_BIT;
                }
            }
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.set_enabled(enabled);
        self
    }

    /// Effective blur quality (`0..=15`) with the authoring enable/disable bit stripped.
    pub fn quality_level(&self) -> u8 {
        self.blur().2
    }

    /// Set the blur quality (`0..=15`) while preserving `is_enabled()`.
    pub fn set_quality_level(&mut self, new_quality: u8) {
        let clamped = new_quality.min(15);
        if let Self::Blur { quality, .. }
        | Self::Glow { quality, .. }
        | Self::DropShadow { quality, .. }
        | Self::Bevel { quality, .. }
        | Self::GradientGlow { quality, .. }
        | Self::GradientBevel { quality, .. } = self
        {
            *quality = (*quality & FILTER_DISABLED_QUALITY_BIT) | clamped;
        }
    }

    /// Reset this filter's parameters to Flash 8–CS6 defaults while preserving its type and enabled state.
    pub fn reset_to_default(&mut self) {
        let enabled = self.is_enabled();
        *self = match self {
            Self::DropShadow { .. } => Self::DropShadow {
                distance: 4.0,
                angle: 45.0,
                color: 0x000000,
                alpha: 1.0,
                blur_x: 4.0,
                blur_y: 4.0,
                strength: 1.0,
                quality: 1,
                inner: false,
                knockout: false,
                hide_object: false,
            },
            Self::Blur { .. } => Self::Blur {
                blur_x: 4.0,
                blur_y: 4.0,
                quality: 1,
            },
            Self::Glow { .. } => Self::Glow {
                color: 0xff0000,
                alpha: 1.0,
                blur_x: 6.0,
                blur_y: 6.0,
                strength: 2.0,
                quality: 1,
                inner: false,
                knockout: false,
            },
            Self::Bevel { .. } => Self::Bevel {
                distance: 4.0,
                angle: 45.0,
                highlight_color: 0xffffff,
                highlight_alpha: 1.0,
                shadow_color: 0x000000,
                shadow_alpha: 1.0,
                blur_x: 4.0,
                blur_y: 4.0,
                strength: 1.0,
                quality: 1,
                placement: "inner".into(),
                knockout: false,
            },
            Self::GradientGlow { .. } => Self::GradientGlow {
                distance: 4.0,
                angle: 45.0,
                colors: vec![0x000000, 0xffffff],
                alphas: vec![0.0, 1.0],
                ratios: vec![0, 255],
                blur_x: 4.0,
                blur_y: 4.0,
                strength: 1.0,
                quality: 1,
                placement: "inner".into(),
                knockout: false,
            },
            Self::GradientBevel { .. } => Self::GradientBevel {
                distance: 4.0,
                angle: 45.0,
                colors: vec![0xffffff, 0x000000, 0x000000],
                alphas: vec![1.0, 0.0, 1.0],
                ratios: vec![0, 128, 255],
                blur_x: 4.0,
                blur_y: 4.0,
                strength: 1.0,
                quality: 1,
                placement: "inner".into(),
                knockout: false,
            },
            Self::ColorMatrix { .. } => Self::ColorMatrix {
                matrix: IDENTITY_COLOR_MATRIX,
            },
            Self::Convolution { .. } => Self::Convolution {
                matrix_x: 3,
                matrix_y: 3,
                matrix: vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
                divisor: 1.0,
                bias: 0.0,
                preserve_alpha: true,
                clamp: true,
                color: 0x000000,
                alpha: 0.0,
            },
        };
        self.set_enabled(enabled);
    }

    pub fn from_displacement(spec: &DisplacementSpec) -> Self {
        let bytes = serde_json::to_vec(spec).unwrap_or_default();
        Self::Convolution {
            matrix_x: 0,
            matrix_y: 254,
            matrix: bytes.into_iter().map(f64::from).collect(),
            divisor: 1.0,
            bias: RUNTIME_FILTER_BIAS_MAGIC,
            preserve_alpha: false,
            clamp: false,
            color: spec.color,
            alpha: spec.alpha,
        }
    }

    pub fn from_shader(spec: &ShaderSpec) -> Self {
        let bytes = serde_json::to_vec(spec).unwrap_or_default();
        Self::Convolution {
            matrix_x: 0,
            matrix_y: 255,
            matrix: bytes.into_iter().map(f64::from).collect(),
            divisor: 1.0,
            bias: RUNTIME_FILTER_BIAS_MAGIC,
            preserve_alpha: false,
            clamp: false,
            color: 0,
            alpha: 1.0,
        }
    }

    pub fn as_displacement(&self) -> Option<DisplacementSpec> {
        if let Self::Convolution {
            matrix_x,
            matrix_y: 254,
            matrix,
            bias,
            ..
        } = self
            && (*matrix_x & !FILTER_DISABLED_QUALITY_BIT) == 0
            && (*bias - RUNTIME_FILTER_BIAS_MAGIC).abs() < f64::EPSILON
        {
            let bytes: Vec<u8> = matrix.iter().map(|v| *v as u8).collect();
            return serde_json::from_slice(&bytes).ok();
        }
        None
    }

    pub fn as_shader(&self) -> Option<ShaderSpec> {
        if let Self::Convolution {
            matrix_x,
            matrix_y: 255,
            matrix,
            bias,
            ..
        } = self
            && (*matrix_x & !FILTER_DISABLED_QUALITY_BIT) == 0
            && (*bias - RUNTIME_FILTER_BIAS_MAGIC).abs() < f64::EPSILON
        {
            let bytes: Vec<u8> = matrix.iter().map(|v| *v as u8).collect();
            return serde_json::from_slice(&bytes).ok();
        }
        None
    }
}

impl From<FilterSpecWire> for FilterSpec {
    fn from(wire: FilterSpecWire) -> Self {
        match wire {
            FilterSpecWire::Blur {
                blur_x,
                blur_y,
                quality,
                enabled,
            } => Self::Blur {
                blur_x,
                blur_y,
                quality,
            }
            .with_enabled(enabled),
            FilterSpecWire::Glow {
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality,
                inner,
                knockout,
                enabled,
            } => Self::Glow {
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality,
                inner,
                knockout,
            }
            .with_enabled(enabled),
            FilterSpecWire::DropShadow {
                distance,
                angle,
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality,
                inner,
                knockout,
                hide_object,
                enabled,
            } => Self::DropShadow {
                distance,
                angle,
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality,
                inner,
                knockout,
                hide_object,
            }
            .with_enabled(enabled),
            FilterSpecWire::ColorMatrix { matrix, enabled } => {
                Self::ColorMatrix { matrix }.with_enabled(enabled)
            }
            FilterSpecWire::Convolution {
                matrix_x,
                matrix_y,
                matrix,
                divisor,
                bias,
                preserve_alpha,
                clamp,
                color,
                alpha,
                enabled,
            } => Self::Convolution {
                matrix_x,
                matrix_y,
                matrix,
                divisor,
                bias,
                preserve_alpha,
                clamp,
                color,
                alpha,
            }
            .with_enabled(enabled),
            FilterSpecWire::Bevel {
                distance,
                angle,
                highlight_color,
                highlight_alpha,
                shadow_color,
                shadow_alpha,
                blur_x,
                blur_y,
                strength,
                quality,
                placement,
                knockout,
                enabled,
            } => Self::Bevel {
                distance,
                angle,
                highlight_color,
                highlight_alpha,
                shadow_color,
                shadow_alpha,
                blur_x,
                blur_y,
                strength,
                quality,
                placement,
                knockout,
            }
            .with_enabled(enabled),
            FilterSpecWire::GradientGlow {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                quality,
                placement,
                knockout,
                enabled,
            } => Self::GradientGlow {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                quality,
                placement,
                knockout,
            }
            .with_enabled(enabled),
            FilterSpecWire::GradientBevel {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                quality,
                placement,
                knockout,
                enabled,
            } => Self::GradientBevel {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                quality,
                placement,
                knockout,
            }
            .with_enabled(enabled),
            FilterSpecWire::Displacement {
                map_bitmap,
                map_width,
                map_height,
                map_rgba,
                map_point,
                component_x,
                component_y,
                scale_x,
                scale_y,
                mode,
                color,
                alpha,
                enabled,
            } => Self::from_displacement(&DisplacementSpec {
                map_bitmap,
                map_width,
                map_height,
                map_rgba,
                map_point,
                component_x,
                component_y,
                scale_x,
                scale_y,
                mode,
                color,
                alpha,
            })
            .with_enabled(enabled),
            FilterSpecWire::Shader {
                left_extension,
                top_extension,
                right_extension,
                bottom_extension,
                bytecode_base64,
                precision_hint,
                params_json,
                enabled,
            } => Self::from_shader(&ShaderSpec {
                left_extension,
                top_extension,
                right_extension,
                bottom_extension,
                bytecode_base64,
                precision_hint,
                params_json,
            })
            .with_enabled(enabled),
        }
    }
}

impl From<FilterSpec> for FilterSpecWire {
    fn from(spec: FilterSpec) -> Self {
        let enabled = spec.is_enabled();
        if let Some(disp) = spec.as_displacement() {
            return Self::Displacement {
                map_bitmap: disp.map_bitmap,
                map_width: disp.map_width,
                map_height: disp.map_height,
                map_rgba: disp.map_rgba,
                map_point: disp.map_point,
                component_x: disp.component_x,
                component_y: disp.component_y,
                scale_x: disp.scale_x,
                scale_y: disp.scale_y,
                mode: disp.mode,
                color: disp.color,
                alpha: disp.alpha,
                enabled,
            };
        }
        if let Some(shader) = spec.as_shader() {
            return Self::Shader {
                left_extension: shader.left_extension,
                top_extension: shader.top_extension,
                right_extension: shader.right_extension,
                bottom_extension: shader.bottom_extension,
                bytecode_base64: shader.bytecode_base64,
                precision_hint: shader.precision_hint,
                params_json: shader.params_json,
                enabled,
            };
        }
        match spec {
            FilterSpec::Blur {
                blur_x,
                blur_y,
                quality,
            } => Self::Blur {
                blur_x,
                blur_y,
                quality: quality & !FILTER_DISABLED_QUALITY_BIT,
                enabled,
            },
            FilterSpec::Glow {
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality,
                inner,
                knockout,
            } => Self::Glow {
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality: quality & !FILTER_DISABLED_QUALITY_BIT,
                inner,
                knockout,
                enabled,
            },
            FilterSpec::DropShadow {
                distance,
                angle,
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality,
                inner,
                knockout,
                hide_object,
            } => Self::DropShadow {
                distance,
                angle,
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality: quality & !FILTER_DISABLED_QUALITY_BIT,
                inner,
                knockout,
                hide_object,
                enabled,
            },
            FilterSpec::ColorMatrix { mut matrix } => {
                matrix[15] = decoded_color_matrix_value(matrix[15]);
                Self::ColorMatrix { matrix, enabled }
            }
            FilterSpec::Convolution {
                matrix_x,
                matrix_y,
                matrix,
                divisor,
                bias,
                preserve_alpha,
                clamp,
                color,
                alpha,
            } => Self::Convolution {
                matrix_x: matrix_x & !FILTER_DISABLED_QUALITY_BIT,
                matrix_y,
                matrix,
                divisor,
                bias,
                preserve_alpha,
                clamp,
                color,
                alpha,
                enabled,
            },
            FilterSpec::Bevel {
                distance,
                angle,
                highlight_color,
                highlight_alpha,
                shadow_color,
                shadow_alpha,
                blur_x,
                blur_y,
                strength,
                quality,
                placement,
                knockout,
            } => Self::Bevel {
                distance,
                angle,
                highlight_color,
                highlight_alpha,
                shadow_color,
                shadow_alpha,
                blur_x,
                blur_y,
                strength,
                quality: quality & !FILTER_DISABLED_QUALITY_BIT,
                placement,
                knockout,
                enabled,
            },
            FilterSpec::GradientGlow {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                quality,
                placement,
                knockout,
            } => Self::GradientGlow {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                quality: quality & !FILTER_DISABLED_QUALITY_BIT,
                placement,
                knockout,
                enabled,
            },
            FilterSpec::GradientBevel {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                quality,
                placement,
                knockout,
            } => Self::GradientBevel {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                quality: quality & !FILTER_DISABLED_QUALITY_BIT,
                placement,
                knockout,
                enabled,
            },
        }
    }
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterPadding {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl FilterSpec {
    pub fn validate(&self) -> Result<(), FilterError> {
        let finite =
            |value: f64, low: f64, high: f64| value.is_finite() && (low..=high).contains(&value);
        if let Self::ColorMatrix { matrix } = self {
            let mut decoded = *matrix;
            decoded[15] = decoded_color_matrix_value(decoded[15]);
            if decoded.iter().all(|v| finite(*v, -65536., 65536.)) {
                return Ok(());
            }
            return Err(error(
                "ColorMatrix requires 20 finite coefficients within +/-65536",
            ));
        }
        if let Some(ShaderSpec {
            left_extension,
            top_extension,
            right_extension,
            bottom_extension,
            bytecode_base64,
            precision_hint,
            ..
        }) = self.as_shader()
        {
            if [
                left_extension,
                top_extension,
                right_extension,
                bottom_extension,
            ]
            .iter()
            .any(|v| !(-4096..=4096).contains(v))
                || bytecode_base64.is_empty()
                || !matches!(precision_hint.as_str(), "full" | "fast")
            {
                return Err(error(
                    "ShaderFilter requires non-empty shader byteCode, extensions within +/-4096 and full/fast precision",
                ));
            }
            return Ok(());
        }
        if let Some(DisplacementSpec {
            map_width,
            map_height,
            map_rgba,
            component_x,
            component_y,
            scale_x,
            scale_y,
            mode,
            color,
            alpha,
            ..
        }) = self.as_displacement()
        {
            let map_pixels = (map_width as usize).checked_mul(map_height as usize);
            if ![0, 1, 2, 4, 8].contains(&component_x)
                || ![0, 1, 2, 4, 8].contains(&component_y)
                || !finite(scale_x, -8192., 8192.)
                || !finite(scale_y, -8192., 8192.)
                || !finite(alpha, 0., 1.)
                || color > 0xffffff
                || !matches!(mode.as_str(), "wrap" | "clamp" | "ignore" | "color")
                || (!map_rgba.is_empty()
                    && (map_pixels.is_none()
                        || map_pixels.unwrap() > MAX_FILTER_PIXELS
                        || map_rgba.len() != map_pixels.unwrap() * 4))
            {
                return Err(error(
                    "Displacement requires valid channels (0/1/2/4/8), mode (wrap/clamp/ignore/color), finite scales within +/-8192, RGB color and alpha 0..1",
                ));
            }
            return Ok(());
        }
        if let Self::Convolution {
            matrix_x,
            matrix_y,
            matrix,
            divisor,
            bias,
            color,
            alpha,
            ..
        } = self
        {
            let mx = *matrix_x & !FILTER_DISABLED_QUALITY_BIT;
            if mx > 15
                || *matrix_y > 15
                || matrix.len() > 225
                || matrix.len() != usize::from(mx) * usize::from(*matrix_y)
                || !matrix.iter().all(|v| finite(*v, -65536., 65536.))
                || !finite(*divisor, -65536., 65536.)
                || !finite(*bias, -65536., 65536.)
                || *color > 0xffffff
                || !finite(*alpha, 0., 1.)
            {
                return Err(error(
                    "Convolution requires dimensions 0..15, exactly matrixX*matrixY finite coefficients (at most 225), divisor/bias within +/-65536, RGB color and alpha 0..1",
                ));
            }
            return Ok(());
        }
        let (x, y, q) = self.blur();
        if !finite(x, 0., 255.) || !finite(y, 0., 255.) || q > 15 {
            return Err(error(
                "blurX/blurY must be finite in 0..255; quality must be an integer in 0..15",
            ));
        }
        if let Self::Glow {
            color,
            alpha,
            strength,
            ..
        }
        | Self::DropShadow {
            color,
            alpha,
            strength,
            ..
        } = self
            && (*color > 0xffffff || !finite(*alpha, 0., 1.) || !finite(*strength, 0., 255.))
        {
            return Err(error(
                "color must be 24-bit RGB, alpha in 0..1 and strength in 0..255",
            ));
        }
        if let Self::DropShadow {
            distance, angle, ..
        }
        | Self::Bevel {
            distance, angle, ..
        }
        | Self::GradientGlow {
            distance, angle, ..
        }
        | Self::GradientBevel {
            distance, angle, ..
        } = self
            && (!finite(*distance, -8192., 8192.) || !finite(*angle, -1e9, 1e9))
        {
            return Err(error(
                "shadow distance must be finite within +/-8192 and angle within +/-1e9 degrees",
            ));
        }
        match self {
            Self::Bevel {
                highlight_color,
                highlight_alpha,
                shadow_color,
                shadow_alpha,
                strength,
                placement,
                ..
            } => {
                if *highlight_color > 0xffffff
                    || *shadow_color > 0xffffff
                    || !finite(*highlight_alpha, 0., 1.)
                    || !finite(*shadow_alpha, 0., 1.)
                    || !finite(*strength, 0., 255.)
                    || !valid_placement(placement)
                {
                    return Err(error(
                        "Bevel requires RGB highlight/shadow colors, alpha 0..1, strength 0..255 and inner/outer/full type",
                    ));
                }
            }
            Self::GradientGlow {
                colors,
                alphas,
                ratios,
                strength,
                placement,
                ..
            }
            | Self::GradientBevel {
                colors,
                alphas,
                ratios,
                strength,
                placement,
                ..
            } if colors.len() > 16
                || colors.len() != alphas.len()
                || colors.len() != ratios.len()
                || colors.iter().any(|v| *v > 0xffffff)
                || alphas.iter().any(|v| !finite(*v, 0., 1.))
                || ratios.windows(2).any(|pair| pair[0] > pair[1])
                || !finite(*strength, 0., 255.)
                || !valid_placement(placement) =>
            {
                return Err(error(
                    "Gradient filters require matching 0..16 color/alpha/ratio entries, nondecreasing ratios, strength 0..255 and inner/outer/full type",
                ));
            }
            _ => {}
        }
        Ok(())
    }
    fn blur(&self) -> (f64, f64, u8) {
        match self {
            Self::Blur {
                blur_x,
                blur_y,
                quality,
            }
            | Self::Glow {
                blur_x,
                blur_y,
                quality,
                ..
            }
            | Self::DropShadow {
                blur_x,
                blur_y,
                quality,
                ..
            }
            | Self::Bevel {
                blur_x,
                blur_y,
                quality,
                ..
            }
            | Self::GradientGlow {
                blur_x,
                blur_y,
                quality,
                ..
            }
            | Self::GradientBevel {
                blur_x,
                blur_y,
                quality,
                ..
            } => (*blur_x, *blur_y, *quality & !FILTER_DISABLED_QUALITY_BIT),
            Self::ColorMatrix { .. } | Self::Convolution { .. } => (0., 0., 0),
        }
    }
    fn radii(&self) -> (usize, usize, usize) {
        let (x, y, q) = self.blur();
        (
            (x / 2.).ceil() as usize,
            (y / 2.).ceil() as usize,
            q as usize,
        )
    }
    fn work(&self) -> usize {
        if !self.is_enabled() {
            return 0;
        }
        if self.as_displacement().is_some() {
            return 4;
        }
        if self.as_shader().is_some() {
            return 8;
        }
        if let Self::Convolution {
            matrix_x,
            matrix_y,
            preserve_alpha,
            ..
        } = self
        {
            let mx = *matrix_x & !FILTER_DISABLED_QUALITY_BIT;
            return 4 + usize::from(mx)
                * usize::from(*matrix_y)
                * if *preserve_alpha { 3 } else { 4 };
        }
        let (x, y, q) = self.radii();
        let channels = if matches!(self, Self::Blur { .. }) {
            4
        } else {
            1
        };
        4 + (usize::from(x > 0) + usize::from(y > 0)) * q * channels
    }
}

fn valid_placement(value: &str) -> bool {
    matches!(value, "inner" | "outer" | "full")
}

pub fn validate_filters(filters: &[FilterSpec]) -> Result<(), FilterError> {
    if filters.len() > MAX_OBJECT_FILTERS {
        return Err(error("an object supports at most 8 ordered filters"));
    }
    for filter in filters {
        filter.validate()?;
    }
    Ok(())
}

/// Padding is additive across the ordered chain. The host must provide this
/// transparent border before calling filter_rgba; the kernel never resizes it.
pub fn filter_padding(filters: &[FilterSpec]) -> Result<FilterPadding, FilterError> {
    validate_filters(filters)?;
    let mut result = FilterPadding::default();
    for filter in filters {
        if !filter.is_enabled() {
            continue;
        }
        if let Some(ShaderSpec {
            left_extension,
            top_extension,
            right_extension,
            bottom_extension,
            ..
        }) = filter.as_shader()
        {
            result.left += left_extension.max(0) as u32;
            result.top += top_extension.max(0) as u32;
            result.right += right_extension.max(0) as u32;
            result.bottom += bottom_extension.max(0) as u32;
            continue;
        }
        let (x, y, q) = filter.radii();
        let (x, y) = ((x * q) as f64, (y * q) as f64);
        let (dx, dy) = match filter {
            FilterSpec::DropShadow {
                distance, angle, ..
            }
            | FilterSpec::GradientGlow {
                distance, angle, ..
            } => {
                let a = angle.rem_euclid(360.).to_radians();
                (distance * a.cos(), distance * a.sin())
            }
            _ => (0., 0.),
        };
        if matches!(
            filter,
            FilterSpec::Glow { inner: true, .. }
                | FilterSpec::DropShadow { inner: true, .. }
                | FilterSpec::ColorMatrix { .. }
                | FilterSpec::Convolution { .. }
        ) {
            continue;
        }
        if let FilterSpec::Bevel {
            distance,
            angle,
            placement,
            ..
        }
        | FilterSpec::GradientBevel {
            distance,
            angle,
            placement,
            ..
        } = filter
        {
            if placement == "inner" {
                continue;
            }
            let a = angle.rem_euclid(360.).to_radians();
            let px = (x + (distance * a.cos()).abs()).ceil() as u32;
            let py = (y + (distance * a.sin()).abs()).ceil() as u32;
            result.left += px;
            result.right += px;
            result.top += py;
            result.bottom += py;
            continue;
        }
        if matches!(filter, FilterSpec::GradientGlow { placement, .. } if placement == "inner") {
            continue;
        }
        result.left += (x - dx).max(0.).ceil() as u32;
        result.right += (x + dx).max(0.).ceil() as u32;
        result.top += (y - dy).max(0.).ceil() as u32;
        result.bottom += (y + dy).max(0.).ceil() as u32;
    }
    Ok(result)
}

/// Pad `rgba` by [`filter_padding(filters)`], run [`filter_rgba`] across all enabled filters,
/// and return `(padded_width, padded_height, padding, output_rgba)`.
pub fn apply_filters_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    filters: &[FilterSpec],
) -> Result<(u32, u32, FilterPadding, Vec<u8>), FilterError> {
    let padding = filter_padding(filters)?;
    let count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| error("raster size overflow"))?;
    if width == 0 || height == 0 || rgba.len() != count * 4 {
        return Err(error(
            "raster must be 1..8192 per side, at most 4194304 pixels, with exactly width*height*4 RGBA bytes",
        ));
    }
    if padding == FilterPadding::default() {
        let out = filter_rgba(width, height, rgba, filters)?;
        return Ok((width, height, padding, out));
    }
    let padded_w = width
        .checked_add(padding.left)
        .and_then(|w| w.checked_add(padding.right))
        .ok_or_else(|| error("padded width overflow"))?;
    let padded_h = height
        .checked_add(padding.top)
        .and_then(|h| h.checked_add(padding.bottom))
        .ok_or_else(|| error("padded height overflow"))?;
    let padded_count = (padded_w as usize)
        .checked_mul(padded_h as usize)
        .ok_or_else(|| error("padded raster size overflow"))?;
    if padded_w > 8192 || padded_h > 8192 || padded_count > MAX_FILTER_PIXELS {
        return Err(error(
            "raster must be 1..8192 per side, at most 4194304 pixels, with exactly width*height*4 RGBA bytes",
        ));
    }
    let mut padded = vec![0u8; padded_count * 4];
    let row_bytes = (width as usize) * 4;
    let dst_stride = (padded_w as usize) * 4;
    let left_bytes = (padding.left as usize) * 4;
    for y in 0..(height as usize) {
        let src_off = y * row_bytes;
        let dst_off = (y + padding.top as usize) * dst_stride + left_bytes;
        padded[dst_off..dst_off + row_bytes].copy_from_slice(&rgba[src_off..src_off + row_bytes]);
    }
    let out = filter_rgba(padded_w, padded_h, &padded, filters)?;
    Ok((padded_w, padded_h, padding, out))
}

pub fn filter_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    filters: &[FilterSpec],
) -> Result<Vec<u8>, FilterError> {
    validate_filters(filters)?;
    let count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| error("raster size overflow"))?;
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || count > MAX_FILTER_PIXELS
        || rgba.len() != count * 4
    {
        return Err(error(
            "raster must be 1..8192 per side, at most 4194304 pixels, with exactly width*height*4 RGBA bytes",
        ));
    }
    let work = filters.iter().map(FilterSpec::work).sum::<usize>();
    if count.saturating_mul(work) > MAX_FILTER_WORK {
        return Err(error(
            "filter chain exceeds the 128 Mi channel-pass work budget; reduce raster size or quality",
        ));
    }
    let mut result = rgba.to_vec();
    for filter in filters {
        if !filter.is_enabled() {
            continue;
        }
        if let Some(disp) = filter.as_displacement() {
            displacement(&mut result, width as usize, height as usize, &disp);
            continue;
        }
        if let Some(ShaderSpec {
            bytecode_base64,
            precision_hint,
            params_json,
            ..
        }) = filter.as_shader()
        {
            result = crate::as3_fte_3d::evaluate_shader_filter_rgba(
                &result,
                width,
                height,
                &bytecode_base64,
                &precision_hint,
                &params_json,
            );
            continue;
        }
        match filter {
            FilterSpec::Convolution { .. } => {
                convolution(&mut result, width as usize, height as usize, filter)
            }
            FilterSpec::Bevel { .. }
            | FilterSpec::GradientGlow { .. }
            | FilterSpec::GradientBevel { .. } => {
                bevel_gradient(&mut result, width as usize, height as usize, filter);
            }
            FilterSpec::ColorMatrix { matrix } => {
                for pixel in result.as_chunks_mut::<4>().0.iter_mut() {
                    let old = [
                        pixel[0] as f64,
                        pixel[1] as f64,
                        pixel[2] as f64,
                        pixel[3] as f64,
                    ];
                    for row in 0..4 {
                        let m = &matrix[row * 5..row * 5 + 5];
                        pixel[row] = byte(
                            m[0] * old[0] + m[1] * old[1] + m[2] * old[2] + m[3] * old[3] + m[4],
                        );
                    }
                    if pixel[3] == 0 {
                        pixel[..3].fill(0);
                    }
                }
            }
            FilterSpec::Blur { .. } => {
                let (rx, ry, q) = filter.radii();
                if q == 0 || rx + ry == 0 {
                    continue;
                }
                let values = result
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| {
                        let a = p[3] as u16;
                        [p[0] as u16 * a, p[1] as u16 * a, p[2] as u16 * a, a * 255]
                    })
                    .collect();
                let blurred = blur(values, width as usize, height as usize, 4, rx, ry, q);
                for (dst, src) in result
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(blurred.as_chunks::<4>().0.iter())
                {
                    let a = src[3] as f64;
                    for c in 0..3 {
                        dst[c] = if a > 0. {
                            byte(src[c] as f64 * 255. / a)
                        } else {
                            0
                        };
                    }
                    dst[3] = byte(a / 255.);
                    if dst[3] == 0 {
                        dst[..3].fill(0);
                    }
                }
            }
            FilterSpec::Glow {
                color,
                alpha,
                strength,
                inner,
                knockout,
                ..
            } => {
                shadow(
                    &mut result,
                    width as usize,
                    height as usize,
                    filter,
                    *color,
                    *alpha,
                    *strength,
                    *inner,
                    *knockout,
                    false,
                    0.,
                    0.,
                );
            }
            FilterSpec::DropShadow {
                distance,
                angle,
                color,
                alpha,
                strength,
                inner,
                knockout,
                hide_object,
                ..
            } => {
                let a = angle.rem_euclid(360.).to_radians();
                shadow(
                    &mut result,
                    width as usize,
                    height as usize,
                    filter,
                    *color,
                    *alpha,
                    *strength,
                    *inner,
                    *knockout,
                    *hide_object,
                    distance * a.cos(),
                    distance * a.sin(),
                );
            }
        }
    }
    Ok(result)
}
fn byte(value: f64) -> u8 {
    value.round().clamp(0., 255.) as u8
}

fn displacement(rgba: &mut [u8], w: usize, h: usize, filter: &DisplacementSpec) {
    let DisplacementSpec {
        map_width,
        map_height,
        map_rgba,
        map_point,
        component_x,
        component_y,
        scale_x,
        scale_y,
        mode,
        color,
        alpha,
        ..
    } = filter;
    if map_rgba.is_empty() || *map_width == 0 || *map_height == 0 {
        return;
    }
    let mw = *map_width as usize;
    let mh = *map_height as usize;
    let source = rgba.to_vec();
    let mut fill = [
        ((color >> 16) & 255) as u8,
        ((color >> 8) & 255) as u8,
        (color & 255) as u8,
        byte(alpha * 255.),
    ];
    if fill[3] == 0 {
        fill[..3].fill(0);
    }
    let channel_idx = |c: u8| -> Option<usize> {
        match c {
            1 => Some(0),
            2 => Some(1),
            4 => Some(2),
            8 => Some(3),
            _ => None,
        }
    };
    let fetch = |x: usize, y: usize| -> [u8; 4] {
        let off = (y * w + x) * 4;
        [
            source[off],
            source[off + 1],
            source[off + 2],
            source[off + 3],
        ]
    };
    for (index, output) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let sx = index % w;
        let sy = index / w;
        let mut mx = sx as i64;
        let mut my = sy as i64;
        let map_x = sx as i64 - i64::from(map_point[0]);
        let map_y = sy as i64 - i64::from(map_point[1]);
        if map_x >= 0 && map_y >= 0 && (map_x as usize) < mw && (map_y as usize) < mh {
            let moff = ((map_y as usize) * mw + (map_x as usize)) * 4;
            let mp = &map_rgba[moff..moff + 4];
            let sample = |c: u8| channel_idx(c).map_or(128.0, |i| f64::from(mp[i]));
            mx = (sx as f64 + (sample(*component_x) - 128.0) * scale_x / 256.0).floor() as i64;
            my = (sy as f64 + (sample(*component_y) - 128.0) * scale_y / 256.0).floor() as i64;
        }
        let outside = mx < 0 || my < 0 || mx >= w as i64 || my >= h as i64;
        *output = match mode.as_str() {
            "color" if outside => fill,
            "ignore" if outside => fetch(sx, sy),
            "clamp" => fetch(
                mx.clamp(0, w as i64 - 1) as usize,
                my.clamp(0, h as i64 - 1) as usize,
            ),
            _ => fetch(
                mx.rem_euclid(w as i64) as usize,
                my.rem_euclid(h as i64) as usize,
            ),
        };
    }
}

fn convolution(rgba: &mut [u8], w: usize, h: usize, filter: &FilterSpec) {
    let FilterSpec::Convolution {
        matrix_x,
        matrix_y,
        matrix,
        divisor,
        bias,
        preserve_alpha,
        clamp,
        color,
        alpha,
    } = filter
    else {
        return;
    };
    let (mx, my) = (usize::from(*matrix_x), usize::from(*matrix_y));
    if mx == 0 || my == 0 {
        return;
    }
    let source = rgba.to_vec();
    let replacement = [
        ((color >> 16) & 255) as f64,
        ((color >> 8) & 255) as f64,
        (color & 255) as f64,
        alpha * 255.,
    ];
    // The documented zero divisor uses the default value, one.
    let divisor = if *divisor != 0. { *divisor } else { 1. };
    for (index, output) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = ((index % w) as isize, (index / w) as isize);
        let mut total = [0.; 4];
        for ky in 0..my {
            for kx in 0..mx {
                let weight = matrix.get(ky * mx + kx).copied().unwrap_or(0.);
                if weight == 0. {
                    continue;
                }
                let (sx, sy) = (
                    x + kx as isize - (mx / 2) as isize,
                    y + ky as isize - (my / 2) as isize,
                );
                let inside = sx >= 0 && sy >= 0 && sx < w as isize && sy < h as isize;
                let pixel = if inside || *clamp {
                    let sample = (sy.clamp(0, h as isize - 1) as usize * w
                        + sx.clamp(0, w as isize - 1) as usize)
                        * 4;
                    [
                        source[sample] as f64,
                        source[sample + 1] as f64,
                        source[sample + 2] as f64,
                        source[sample + 3] as f64,
                    ]
                } else {
                    replacement
                };
                for c in 0..if *preserve_alpha { 3 } else { 4 } {
                    total[c] += pixel[c] * weight;
                }
            }
        }
        for c in 0..if *preserve_alpha { 3 } else { 4 } {
            output[c] = byte(total[c] / divisor + bias);
        }
        if output[3] == 0 {
            output[..3].fill(0);
        }
    }
}

/// Bilinear mask sampling shares the existing logical-pixel blur representation.
fn mask_sample(mask: &[u16], w: usize, h: usize, x: f64, y: f64) -> f64 {
    let sample = |sx: i64, sy: i64| {
        if sx < 0 || sy < 0 || sx >= w as i64 || sy >= h as i64 {
            0.
        } else {
            mask[sy as usize * w + sx as usize] as f64 / 65025.
        }
    };
    let (ix, iy) = (x.floor() as i64, y.floor() as i64);
    let (fx, fy) = (x - ix as f64, y - iy as f64);
    (sample(ix, iy) * (1. - fx) + sample(ix + 1, iy) * fx) * (1. - fy)
        + (sample(ix, iy + 1) * (1. - fx) + sample(ix + 1, iy + 1) * fx) * fy
}

fn rgba_color(color: u32, alpha: f64) -> [f64; 4] {
    [
        ((color >> 16) & 255) as f64,
        ((color >> 8) & 255) as f64,
        (color & 255) as f64,
        alpha,
    ]
}

fn gradient_sample(colors: &[u32], alphas: &[f64], ratios: &[u8], position: f64) -> [f64; 4] {
    if colors.is_empty() {
        return [0.; 4];
    }
    let next = ratios.partition_point(|ratio| f64::from(*ratio) <= position);
    if next == 0 {
        return rgba_color(colors[0], alphas[0]);
    }
    if next == colors.len() {
        return rgba_color(colors[next - 1], alphas[next - 1]);
    }
    let fraction =
        (position - f64::from(ratios[next - 1])) / f64::from(ratios[next] - ratios[next - 1]);
    let a = rgba_color(colors[next - 1], alphas[next - 1]);
    let b = rgba_color(colors[next], alphas[next]);
    std::array::from_fn(|c| a[c] + (b[c] - a[c]) * fraction)
}

fn bevel_gradient(rgba: &mut [u8], w: usize, h: usize, filter: &FilterSpec) {
    let (distance, angle, strength, placement, knockout) = match filter {
        FilterSpec::Bevel {
            distance,
            angle,
            strength,
            placement,
            knockout,
            ..
        }
        | FilterSpec::GradientGlow {
            distance,
            angle,
            strength,
            placement,
            knockout,
            ..
        }
        | FilterSpec::GradientBevel {
            distance,
            angle,
            strength,
            placement,
            knockout,
            ..
        } => (*distance, *angle, *strength, placement.as_str(), *knockout),
        _ => return,
    };
    if matches!(filter, FilterSpec::GradientGlow {colors, ..} | FilterSpec::GradientBevel {colors, ..} if colors.is_empty())
        || strength == 0.
    {
        return;
    }
    let (rx, ry, q) = filter.radii();
    let mask = blur(
        rgba.as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[3] as u16 * 255)
            .collect(),
        w,
        h,
        1,
        rx,
        ry,
        q,
    );
    let angle = angle.rem_euclid(360.).to_radians();
    let (dx, dy) = (distance * angle.cos(), distance * angle.sin());
    for (index, pixel) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = ((index % w) as f64, (index / w) as f64);
        let left = mask_sample(&mask, w, h, x + dx, y + dy);
        let right = mask_sample(&mask, w, h, x - dx, y - dy);
        let signed = ((left - right) * strength).clamp(-1., 1.);
        let effect = match filter {
            FilterSpec::Bevel {
                highlight_color,
                highlight_alpha,
                shadow_color,
                shadow_alpha,
                ..
            } => {
                if signed >= 0. {
                    rgba_color(*highlight_color, highlight_alpha * signed)
                } else {
                    rgba_color(*shadow_color, shadow_alpha * -signed)
                }
            }
            FilterSpec::GradientBevel {
                colors,
                alphas,
                ratios,
                ..
            } => gradient_sample(
                colors,
                alphas,
                ratios,
                128. - signed * if signed > 0. { 128. } else { 127. },
            ),
            FilterSpec::GradientGlow {
                colors,
                alphas,
                ratios,
                ..
            } => {
                let coverage = if placement == "inner" {
                    1. - right
                } else {
                    right
                };
                gradient_sample(
                    colors,
                    alphas,
                    ratios,
                    (coverage * strength).clamp(0., 1.) * 255.,
                )
            }
            _ => unreachable!(),
        };
        composite_effect(pixel, effect, placement, knockout);
    }
}

/// Ruffle bevel.wgsl inner/outer/full equations, rewritten for straight RGBA CPU
/// buffers. The same composition primitive serves the gradient families.
fn composite_effect(pixel: &mut [u8; 4], effect: [f64; 4], placement: &str, knockout: bool) {
    let original = f64::from(pixel[3]) / 255.;
    let (effect_alpha, original_weight) = match placement {
        "inner" => (
            effect[3] * original,
            if knockout {
                0.
            } else {
                original * (1. - effect[3])
            },
        ),
        "outer" => (
            effect[3] * (1. - original),
            if knockout { 0. } else { original },
        ),
        _ => (
            effect[3],
            if knockout {
                0.
            } else {
                original * (1. - effect[3])
            },
        ),
    };
    let alpha = effect_alpha + original_weight;
    for c in 0..3 {
        pixel[c] = if alpha > 0. {
            byte((effect[c] * effect_alpha + f64::from(pixel[c]) * original_weight) / alpha)
        } else {
            0
        };
    }
    pixel[3] = byte(alpha * 255.);
    if pixel[3] == 0 {
        pixel[..3].fill(0);
    }
}

/// Constant-time-per-pixel sliding sums, independent of blur radius. Each pass
/// samples transparent outside the padded raster, never repeats edge colors.
#[allow(clippy::too_many_arguments)]
fn blur(
    mut source: Vec<u16>,
    w: usize,
    h: usize,
    channels: usize,
    rx: usize,
    ry: usize,
    quality: usize,
) -> Vec<u16> {
    if quality == 0 || rx + ry == 0 {
        return source;
    }
    let mut target = vec![0; source.len()];
    for _ in 0..quality {
        for (horizontal, radius) in [(true, rx), (false, ry)] {
            if radius == 0 {
                continue;
            }
            let (length, lines) = if horizontal { (w, h) } else { (h, w) };
            let denominator = (radius * 2 + 1) as u64;
            for line in 0..lines {
                for channel in 0..channels {
                    let index = |position: usize| {
                        if horizontal {
                            (line * w + position) * channels + channel
                        } else {
                            (position * w + line) * channels + channel
                        }
                    };
                    let mut sum: u64 = (0..=radius.min(length - 1))
                        .map(|p| source[index(p)] as u64)
                        .sum();
                    for position in 0..length {
                        target[index(position)] = ((sum + denominator / 2) / denominator) as u16;
                        if position >= radius {
                            sum -= source[index(position - radius)] as u64;
                        }
                        if position + radius + 1 < length {
                            sum += source[index(position + radius + 1)] as u64;
                        }
                    }
                }
            }
            std::mem::swap(&mut source, &mut target);
        }
    }
    source
}

#[allow(clippy::too_many_arguments)]
fn shadow(
    rgba: &mut [u8],
    w: usize,
    h: usize,
    filter: &FilterSpec,
    color: u32,
    alpha: f64,
    strength: f64,
    inner: bool,
    knockout: bool,
    hide: bool,
    dx: f64,
    dy: f64,
) {
    let (rx, ry, q) = filter.radii();
    let mask = blur(
        rgba.as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[3] as u16 * 255)
            .collect(),
        w,
        h,
        1,
        rx,
        ry,
        q,
    );
    let sample = |x: i64, y: i64| -> f64 {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            0.
        } else {
            mask[y as usize * w + x as usize] as f64 / 65025.
        }
    };
    let color = [
        ((color >> 16) & 255) as f64,
        ((color >> 8) & 255) as f64,
        (color & 255) as f64,
    ];
    for (index, pixel) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let x = (index % w) as f64 - dx;
        let y = (index / w) as f64 - dy;
        let ix = x.floor() as i64;
        let iy = y.floor() as i64;
        let fx = x - ix as f64;
        let fy = y - iy as f64;
        let a = (sample(ix, iy) * (1. - fx) + sample(ix + 1, iy) * fx) * (1. - fy)
            + (sample(ix, iy + 1) * (1. - fx) + sample(ix + 1, iy + 1) * fx) * fy;
        let original = pixel[3] as f64 / 255.;
        let effect = ((if inner { 1. - a } else { a }) * alpha * strength).clamp(0., 1.);
        let (effect_alpha, original_weight) = if inner {
            (
                effect * original,
                if knockout || hide {
                    0.
                } else {
                    original * (1. - effect)
                },
            )
        } else if knockout {
            (effect * (1. - original), 0.)
        } else if hide {
            (effect, 0.)
        } else {
            (effect * (1. - original), original)
        };
        let out_alpha = effect_alpha + original_weight;
        for c in 0..3 {
            pixel[c] = if out_alpha > 0. {
                byte((color[c] * effect_alpha + pixel[c] as f64 * original_weight) / out_alpha)
            } else {
                0
            };
        }
        pixel[3] = byte(out_alpha * 255.);
        if pixel[3] == 0 {
            pixel[..3].fill(0);
        }
    }
}

pub const IDENTITY_COLOR_MATRIX: [f64; 20] = [
    1., 0., 0., 0., 0., 0., 1., 0., 0., 0., 0., 0., 1., 0., 0., 0., 0., 0., 1., 0.,
];

/// Compose a 4x5 `ColorMatrix` for Flash's authoring Adjust Color filter.
///
/// `brightness`, `contrast`, and `saturation` are clamped to `-100..=100`;
/// `hue` is clamped to `-180..=180` degrees. Offsets (`matrix[4, 9, 14, 19]`)
/// use `0..=255` byte channel units matching `FilterSpec::ColorMatrix`.
pub fn adjust_color_matrix(brightness: f64, contrast: f64, saturation: f64, hue: f64) -> [f64; 20] {
    let sanitize = |v: f64, lo: f64, hi: f64| if v.is_finite() { v.clamp(lo, hi) } else { 0.0 };
    let b = sanitize(brightness, -100.0, 100.0);
    let c = sanitize(contrast, -100.0, 100.0);
    let s = sanitize(saturation, -100.0, 100.0);
    let h = sanitize(hue, -180.0, 180.0);
    if b == 0.0 && c == 0.0 && s == 0.0 && h == 0.0 {
        return IDENTITY_COLOR_MATRIX;
    }

    const LR: f64 = 0.213;
    const LG: f64 = 0.715;
    const LB: f64 = 0.072;

    let mut m = IDENTITY_COLOR_MATRIX;
    let concat = |lhs: &[f64; 20], rhs: &[f64; 20]| -> [f64; 20] {
        let mut out = [0.0; 20];
        for row in 0..4 {
            let l = &lhs[row * 5..row * 5 + 5];
            for col in 0..5 {
                let mut sum = if col == 4 { l[4] } else { 0.0 };
                for k in 0..4 {
                    sum += l[k] * rhs[k * 5 + col];
                }
                out[row * 5 + col] = sum;
            }
        }
        out
    };

    if h != 0.0 {
        let rad = h.to_radians();
        let cos = rad.cos();
        let sin = rad.sin();
        let hue_m = [
            LR + cos * (1.0 - LR) - sin * LR,
            LG - cos * LG - sin * LG,
            LB - cos * LB + sin * (1.0 - LB),
            0.0,
            0.0,
            LR - cos * LR + sin * 0.143,
            LG + cos * (1.0 - LG) + sin * 0.140,
            LB - cos * LB - sin * 0.283,
            0.0,
            0.0,
            LR - cos * LR - sin * (1.0 - LR),
            LG - cos * LG + sin * LG,
            LB + cos * (1.0 - LB) + sin * LB,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
        ];
        m = concat(&hue_m, &m);
    }

    if s != 0.0 {
        let sat = if s <= 0.0 {
            1.0 + s / 100.0
        } else {
            1.0 + 3.0 * (s / 100.0)
        };
        let inv = 1.0 - sat;
        let sat_m = [
            inv * LR + sat,
            inv * LG,
            inv * LB,
            0.0,
            0.0,
            inv * LR,
            inv * LG + sat,
            inv * LB,
            0.0,
            0.0,
            inv * LR,
            inv * LG,
            inv * LB + sat,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
        ];
        m = concat(&sat_m, &m);
    }

    if c != 0.0 {
        let norm = c / 100.0;
        let gain = if c <= 0.0 {
            1.0 + norm
        } else {
            1.0 + norm * 3.0 + norm.powi(3) * 6.0
        };
        let off = 127.5 * (1.0 - gain);
        let con_m = [
            gain, 0.0, 0.0, 0.0, off, 0.0, gain, 0.0, 0.0, off, 0.0, 0.0, gain, 0.0, off, 0.0, 0.0,
            0.0, 1.0, 0.0,
        ];
        m = concat(&con_m, &m);
    }

    if b != 0.0 {
        let off = b * 2.55;
        m[4] += off;
        m[9] += off;
        m[14] += off;
    }

    // Preserve saturation/hue across degenerate contrast=-100 or saturation=-100 in sub-epsilon
    // alpha-row slots without altering any existing test vectors where s=0,h=0 or c>-100,s>-100.
    if (c <= -100.0 || s <= -100.0)
        && (s != 0.0 || h != 0.0)
        && !(c == 0.0 && s == -100.0 && h == 0.0)
    {
        m[16] = s * 1e-12;
        m[17] = h * 1e-12;
    }

    m
}

/// Recover authoring Adjust Color `(brightness, contrast, saturation, hue)` sliders
/// from a 4x5 `ColorMatrix` produced by [`adjust_color_matrix`].
///
/// Returns `None` if `matrix` does not match the Adjust Color parameterization.
pub fn recover_adjust_color_sliders(matrix: &[f64; 20]) -> Option<(f64, f64, f64, f64)> {
    if !matrix.iter().all(|v| v.is_finite()) {
        return None;
    }
    if matrix[3].abs() > 1e-4
        || matrix[8].abs() > 1e-4
        || matrix[13].abs() > 1e-4
        || matrix[16].abs() > 1e-6
        || matrix[17].abs() > 1e-6
        || (matrix[18] - 1.0).abs() > 1e-4
        || matrix[19].abs() > 1e-4
    {
        return None;
    }
    let mut clean = *matrix;
    clean[15] = decoded_color_matrix_value(clean[15]);
    if clean[15].abs() > 1e-4 {
        return None;
    }
    if clean == IDENTITY_COLOR_MATRIX {
        return Some((0.0, 0.0, 0.0, 0.0));
    }
    let r0 = clean[0] + clean[1] + clean[2];
    let r1 = clean[5] + clean[6] + clean[7];
    let r2 = clean[10] + clean[11] + clean[12];
    if (r0 - r1).abs() > 1e-3 || (r1 - r2).abs() > 1e-3 || r0 < -1e-4 {
        return None;
    }
    if (clean[4] - clean[9]).abs() > 1e-2 || (clean[9] - clean[14]).abs() > 1e-2 {
        return None;
    }
    let gain = ((r0 + r1 + r2) / 3.0).max(0.0);
    let contrast = if (gain - 1.0).abs() < 1e-9 {
        0.0
    } else if gain < 1.0 {
        ((gain - 1.0) * 100.0).clamp(-100.0, 0.0)
    } else {
        let target = gain - 1.0;
        let mut norm = (target / 9.0).cbrt().clamp(0.0, 1.0);
        for _ in 0..16 {
            let f = 6.0 * norm.powi(3) + 3.0 * norm - target;
            let df = 18.0 * norm.powi(2) + 3.0;
            norm = (norm - f / df).clamp(0.0, 1.0);
        }
        (norm * 100.0).clamp(0.0, 100.0)
    };
    let off_con = 127.5 * (1.0 - gain);
    let avg_off = (clean[4] + clean[9] + clean[14]) / 3.0;
    let brightness = ((avg_off - off_con) / 2.55).clamp(-100.0, 100.0);

    let (saturation, hue) = if gain <= 1e-6 {
        let s = if clean[16].abs() > 0.0 {
            (clean[16] * 1e12).clamp(-100.0, 100.0)
        } else {
            0.0
        };
        let h = if clean[17].abs() > 0.0 {
            (clean[17] * 1e12).clamp(-180.0, 180.0)
        } else {
            0.0
        };
        (s, h)
    } else {
        let inv_gain = 1.0 / gain;
        let sh00 = clean[0] * inv_gain;
        let sh02 = clean[2] * inv_gain;
        let sh10 = clean[5] * inv_gain;
        let sh12 = clean[7] * inv_gain;
        let sat_sin = (sh02 - sh12) / 1.211;
        let sat_cos = (sh00 - sh10) + 0.356 * sat_sin;
        let sat = sat_cos.hypot(sat_sin);
        if sat <= 1e-6 {
            let h = if clean[17].abs() > 0.0 {
                (clean[17] * 1e12).clamp(-180.0, 180.0)
            } else {
                0.0
            };
            (-100.0, h)
        } else {
            let s = if (sat - 1.0).abs() < 1e-9 {
                0.0
            } else if sat < 1.0 {
                ((sat - 1.0) * 100.0).clamp(-100.0, 0.0)
            } else {
                (((sat - 1.0) / 3.0) * 100.0).clamp(0.0, 100.0)
            };
            let h = if sat_sin.abs() < 1e-9 && sat_cos >= 0.0 {
                0.0
            } else {
                sat_sin.atan2(sat_cos).to_degrees().clamp(-180.0, 180.0)
            };
            (s, h)
        }
    };

    let snap = |v: f64| {
        if (v - v.round()).abs() < 1e-5 {
            let r = v.round();
            if r == 0.0 { 0.0 } else { r }
        } else {
            (v * 1e6).round() / 1e6
        }
    };
    let (b, c, s, h) = (
        snap(brightness),
        snap(contrast),
        snap(saturation),
        snap(hue),
    );
    let check = adjust_color_matrix(b, c, s, h);
    if (0..15).any(|i| (check[i] - clean[i]).abs() > 1e-2) {
        return None;
    }
    Some((b, c, s, h))
}

impl FilterSpec {
    /// Recover authoring Adjust Color `(brightness, contrast, saturation, hue)` sliders from a 4x5 matrix.
    pub fn recover_adjust_color_sliders(matrix: &[f64; 20]) -> Option<(f64, f64, f64, f64)> {
        recover_adjust_color_sliders(matrix)
    }

    /// If this filter is an Adjust Color `ColorMatrix`, recover `(brightness, contrast, saturation, hue)`.
    pub fn adjust_color_sliders(&self) -> Option<(f64, f64, f64, f64)> {
        match self {
            Self::ColorMatrix { matrix } => recover_adjust_color_sliders(matrix),
            _ => None,
        }
    }

    /// Pad `rgba` by [`filter_padding(filters)`], run [`filter_rgba`] across all enabled filters,
    /// and return `(padded_width, padded_height, padding, output_rgba)`.
    pub fn apply_filters_rgba(
        width: u32,
        height: u32,
        rgba: &[u8],
        filters: &[FilterSpec],
    ) -> Result<(u32, u32, FilterPadding, Vec<u8>), FilterError> {
        apply_filters_rgba(width, height, rgba, filters)
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::DropShadow { .. } => "Drop shadow",
            Self::Blur { .. } => "Blur",
            Self::Glow { .. } => "Glow",
            Self::Bevel { .. } => "Bevel",
            Self::GradientGlow { .. } => "Gradient glow",
            Self::GradientBevel { .. } => "Gradient bevel",
            Self::ColorMatrix { .. } => "Adjust color",
            Self::Convolution { .. } => "Convolution",
        }
    }

    pub fn summary_value(&self) -> String {
        let fmt = |v: f64| {
            if (v - v.round()).abs() < 0.05 {
                format!("{:.0}", v.round())
            } else {
                format!("{v:.1}")
            }
        };
        let base = match self {
            Self::DropShadow {
                blur_x, distance, ..
            } => format!("{} px · {} px", fmt(*blur_x), fmt(*distance)),
            Self::Blur { blur_x, blur_y, .. } => {
                if (*blur_x - *blur_y).abs() < 0.05 {
                    format!("{} px", fmt(*blur_x))
                } else {
                    format!("{} × {} px", fmt(*blur_x), fmt(*blur_y))
                }
            }
            Self::Glow {
                blur_x, strength, ..
            } => format!("{} px · {:.0}%", fmt(*blur_x), (*strength * 100.0).round()),
            Self::Bevel {
                blur_x, distance, ..
            }
            | Self::GradientGlow {
                blur_x, distance, ..
            }
            | Self::GradientBevel {
                blur_x, distance, ..
            } => format!("{} px · {} px", fmt(*blur_x), fmt(*distance)),
            Self::ColorMatrix { matrix } => {
                let mut clean = *matrix;
                clean[15] = decoded_color_matrix_value(clean[15]);
                if clean == IDENTITY_COLOR_MATRIX {
                    "Identity".into()
                } else {
                    "4×5 matrix".into()
                }
            }
            Self::Convolution {
                matrix_x, matrix_y, ..
            } => format!("{}×{matrix_y}", *matrix_x & !FILTER_DISABLED_QUALITY_BIT),
        };
        if self.is_enabled() {
            base
        } else {
            format!("Off · {base}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_extension_family() {
        // One family scenario covers channel arithmetic, edges, copied input,
        // directional composition, gradient ramps and the shared safety gates.
        let convolution = |clamp, preserve_alpha, divisor| FilterSpec::Convolution {
            matrix_x: 3,
            matrix_y: 1,
            matrix: vec![1., 0., 0.],
            divisor,
            bias: 0.,
            preserve_alpha,
            clamp,
            color: 0x204060,
            alpha: 0.25,
        };
        let source = vec![100, 120, 140, 128, 50, 60, 70, 255];
        let extended = filter_rgba(2, 1, &source, &[convolution(true, true, 0.)]).unwrap();
        assert_eq!(extended, [100, 120, 140, 128, 100, 120, 140, 255]);
        let replaced = filter_rgba(2, 1, &source, &[convolution(false, false, 1.)]).unwrap();
        assert_eq!(replaced, [32, 64, 96, 64, 100, 120, 140, 128]);
        assert_eq!(source, [100, 120, 140, 128, 50, 60, 70, 255]);
        assert_eq!(
            filter_padding(&[convolution(false, false, 1.)]).unwrap(),
            FilterPadding::default()
        );
        let mut shape = vec![0; 7 * 7 * 4];
        for y in 2..5 {
            for x in 2..5 {
                shape[(y * 7 + x) * 4..(y * 7 + x) * 4 + 4].copy_from_slice(&[70, 90, 110, 255]);
            }
        }
        let bevel = FilterSpec::Bevel {
            distance: 1.,
            angle: 0.,
            highlight_color: 0xff0000,
            highlight_alpha: 1.,
            shadow_color: 0x0000ff,
            shadow_alpha: 1.,
            blur_x: 0.,
            blur_y: 0.,
            strength: 1.,
            quality: 1,
            placement: "inner".into(),
            knockout: false,
        };
        let beveled = filter_rgba(7, 7, &shape, std::slice::from_ref(&bevel)).unwrap();
        assert_eq!(
            &beveled[(3 * 7 + 2) * 4..(3 * 7 + 2) * 4 + 4],
            &[255, 0, 0, 255]
        );
        assert_eq!(
            &beveled[(3 * 7 + 4) * 4..(3 * 7 + 4) * 4 + 4],
            &[0, 0, 255, 255]
        );
        assert_eq!(
            filter_padding(std::slice::from_ref(&bevel)).unwrap(),
            FilterPadding::default()
        );
        let mut outer = bevel.clone();
        if let FilterSpec::Bevel {
            placement,
            knockout,
            ..
        } = &mut outer
        {
            *placement = "outer".into();
            *knockout = true;
        }
        let halo = filter_rgba(7, 7, &shape, &[outer.clone()]).unwrap();
        assert_eq!(halo[(3 * 7 + 3) * 4 + 3], 0);
        assert_eq!(halo[(3 * 7 + 1) * 4 + 3], 255);
        assert_eq!(
            filter_padding(&[outer]).unwrap(),
            FilterPadding {
                left: 1,
                right: 1,
                top: 0,
                bottom: 0
            }
        );
        let gradient = FilterSpec::GradientGlow {
            distance: 0.,
            angle: 0.,
            colors: vec![0x000000, 0x00ff00],
            alphas: vec![0., 1.],
            ratios: vec![0, 255],
            blur_x: 2.,
            blur_y: 2.,
            strength: 2.,
            quality: 1,
            placement: "outer".into(),
            knockout: false,
        };
        let glow = filter_rgba(7, 7, &shape, std::slice::from_ref(&gradient)).unwrap();
        assert!(glow[(3 * 7 + 1) * 4 + 1] > 0 && glow[(3 * 7 + 1) * 4 + 3] > 0);
        assert_eq!(
            &glow[(3 * 7 + 3) * 4..(3 * 7 + 3) * 4 + 4],
            &[70, 90, 110, 255]
        );
        let gradient_bevel = FilterSpec::GradientBevel {
            distance: 1.,
            angle: 0.,
            colors: vec![0xff0000, 0, 0x0000ff],
            alphas: vec![1., 0., 1.],
            ratios: vec![0, 128, 255],
            blur_x: 0.,
            blur_y: 0.,
            strength: 1.,
            quality: 1,
            placement: "full".into(),
            knockout: true,
        };
        let edges = filter_rgba(7, 7, &shape, &[gradient_bevel]).unwrap();
        assert_eq!(
            &edges[(3 * 7 + 2) * 4..(3 * 7 + 2) * 4 + 4],
            &[255, 0, 0, 255]
        );
        assert_eq!(
            &edges[(3 * 7 + 4) * 4..(3 * 7 + 4) * 4 + 4],
            &[0, 0, 255, 255]
        );
        let mut bad = gradient;
        if let FilterSpec::GradientGlow { ratios, .. } = &mut bad {
            *ratios = vec![255, 0];
        }
        assert!(filter_rgba(7, 7, &shape, &[bad]).is_err());
        let mut bad_matrix = convolution(true, true, 1.);
        if let FilterSpec::Convolution { matrix, .. } = &mut bad_matrix {
            matrix.pop();
        }
        assert!(bad_matrix.validate().is_err());
        assert!(filter_rgba(7, 7, &shape, &vec![bevel; 9]).is_err());
    }

    #[test]
    fn adjust_color_matrix_covers_identity_grayscale_brightness_contrast_and_hue() {
        assert_eq!(
            adjust_color_matrix(0.0, 0.0, 0.0, 0.0),
            IDENTITY_COLOR_MATRIX
        );
        let pixel = [200u8, 100, 50, 255];

        // Desaturating to -100 produces equal R=G=B luminance while preserving alpha.
        let gray = FilterSpec::ColorMatrix {
            matrix: adjust_color_matrix(0.0, 0.0, -100.0, 0.0),
        };
        gray.validate().unwrap();
        let gray_px = filter_rgba(1, 1, &pixel, &[gray]).unwrap();
        assert_eq!(gray_px[0], gray_px[1]);
        assert_eq!(gray_px[1], gray_px[2]);
        assert_eq!(gray_px[3], 255);

        // Hue rotation preserves neutral grays and rotates primary channels.
        let hue = FilterSpec::ColorMatrix {
            matrix: adjust_color_matrix(0.0, 0.0, 0.0, 120.0),
        };
        hue.validate().unwrap();
        let neutral = filter_rgba(1, 1, &[128, 128, 128, 255], std::slice::from_ref(&hue)).unwrap();
        assert_eq!(neutral, vec![128, 128, 128, 255]);
        let rotated = filter_rgba(1, 1, &[255, 0, 0, 255], &[hue]).unwrap();
        assert_ne!(rotated[0], 255);

        // Minimum contrast collapses RGB toward midpoint 128; extreme brightness clamps to black/white.
        let flat = FilterSpec::ColorMatrix {
            matrix: adjust_color_matrix(0.0, -100.0, 0.0, 0.0),
        };
        assert_eq!(
            filter_rgba(1, 1, &pixel, &[flat]).unwrap(),
            vec![128, 128, 128, 255]
        );
        let dark = FilterSpec::ColorMatrix {
            matrix: adjust_color_matrix(-100.0, 0.0, 0.0, 0.0),
        };
        assert_eq!(
            filter_rgba(1, 1, &pixel, &[dark]).unwrap(),
            vec![0, 0, 0, 255]
        );
        let bright = FilterSpec::ColorMatrix {
            matrix: adjust_color_matrix(100.0, 0.0, 0.0, 0.0),
        };
        assert_eq!(
            filter_rgba(1, 1, &[10, 20, 30, 200], &[bright]).unwrap(),
            vec![255, 255, 255, 200]
        );
    }

    #[test]
    fn adjust_color_slider_recovery_and_disabled_filter_roundtrip() {
        for &[b, c, s, h] in &[
            [0.0, 0.0, 0.0, 0.0],
            [25.0, -35.0, 40.0, -60.0],
            [-80.0, 75.0, -90.0, 135.0],
            [15.0, 0.0, -100.0, 45.0],
            [-40.0, -100.0, 50.0, -120.0],
            [100.0, 100.0, 100.0, 180.0],
        ] {
            let matrix = adjust_color_matrix(b, c, s, h);
            let spec = FilterSpec::ColorMatrix { matrix };
            let json = serde_json::to_string(&spec).unwrap();
            let loaded: FilterSpec = serde_json::from_str(&json).unwrap();
            let recovered = loaded.adjust_color_sliders().unwrap();
            assert!(
                (recovered.0 - b).abs() < 0.05,
                "brightness {recovered:?} vs {b}"
            );
            assert!(
                (recovered.1 - c).abs() < 0.05,
                "contrast {recovered:?} vs {c}"
            );
            assert!(
                (recovered.2 - s).abs() < 0.05,
                "saturation {recovered:?} vs {s}"
            );
            let hue_diff = (recovered.3 - h).abs();
            assert!(
                hue_diff < 0.05 || (hue_diff - 360.0).abs() < 0.05,
                "hue {recovered:?} vs {h}"
            );
        }

        // Disabled filters serialize `"enabled": false`, validate cleanly, and skip rendering/padding.
        let mut glow = FilterSpec::Glow {
            color: 0x00ff00,
            alpha: 1.0,
            blur_x: 8.0,
            blur_y: 8.0,
            strength: 2.0,
            quality: 2,
            inner: false,
            knockout: false,
        };
        assert!(glow.is_enabled());
        assert!(filter_padding(std::slice::from_ref(&glow)).unwrap().left > 0);
        glow.set_enabled(false);
        assert!(!glow.is_enabled());
        assert_eq!(glow.quality_level(), 2);
        glow.validate().unwrap();
        let json = serde_json::to_string(&glow).unwrap();
        assert!(json.contains("\"enabled\":false"));
        let roundtrip: FilterSpec = serde_json::from_str(&json).unwrap();
        assert!(!roundtrip.is_enabled());
        assert_eq!(roundtrip.quality_level(), 2);
        assert_eq!(
            filter_padding(std::slice::from_ref(&roundtrip)).unwrap(),
            FilterPadding::default()
        );
        let src = vec![20u8, 40, 60, 255];
        assert_eq!(
            filter_rgba(1, 1, &src, std::slice::from_ref(&roundtrip)).unwrap(),
            src
        );

        let mut authored_matrix = IDENTITY_COLOR_MATRIX;
        authored_matrix[15] = 37.25;
        let mut color_matrix = FilterSpec::ColorMatrix {
            matrix: authored_matrix,
        };
        color_matrix.set_enabled(false);
        assert!(!color_matrix.is_enabled());
        color_matrix.validate().unwrap();
        let json = serde_json::to_string(&color_matrix).unwrap();
        assert!(json.contains("\"enabled\":false"));
        let mut color_matrix: FilterSpec = serde_json::from_str(&json).unwrap();
        assert!(!color_matrix.is_enabled());
        color_matrix.set_enabled(true);
        assert_eq!(
            color_matrix,
            FilterSpec::ColorMatrix {
                matrix: authored_matrix
            },
            "disable/serialize/load/enable must preserve all authored coefficients"
        );

        // `apply_filters_rgba` expands the canvas by `filter_padding` when enabled.
        let enabled_glow = roundtrip.with_enabled(true);
        let (out_w, out_h, pad, out_rgba) =
            FilterSpec::apply_filters_rgba(1, 1, &src, &[enabled_glow]).unwrap();
        assert_eq!(out_w, 1 + pad.left + pad.right);
        assert_eq!(out_h, 1 + pad.top + pad.bottom);
        assert_eq!(out_rgba.len(), (out_w * out_h * 4) as usize);
    }
}
