//! Portable, immutable document bytes. Logical resource paths never access files.
use crate::Id;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const MAX_RESOURCE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_RESOURCE_TOTAL_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_RESOURCES: usize = 1024;
/// Includes base64 expansion of resources and the existing authoring document.
pub const MAX_PROJECT_JSON_BYTES: usize = 128 * 1024 * 1024;
const MAX_ENCODED_RESOURCE_BYTES: usize = MAX_RESOURCE_BYTES.div_ceil(3) * 4;
pub(crate) const MAX_ENCODED_TOTAL_BYTES: usize =
    MAX_RESOURCE_TOTAL_BYTES.div_ceil(3) * 4 + MAX_RESOURCES * 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundCompression {
    /// Follow the project-wide publish audio policy (`PublishOptions.audio`).
    #[default]
    Default,
    /// Publish source audio bytes without re-encoding.
    Original,
    /// Re-encode to MPEG Layer III CBR at publish time.
    Mp3,
    /// Re-encode to 16-bit PCM RIFF/WAVE at publish time.
    Wav,
}

impl SoundCompression {
    pub const ALL: [Self; 4] = [Self::Mp3, Self::Default, Self::Wav, Self::Original];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Original => "Original",
            Self::Mp3 => "MP3",
            Self::Wav => "WAV (PCM)",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundExportQuality {
    Fast,
    Medium,
    #[default]
    Best,
}

impl SoundExportQuality {
    pub const ALL: [Self; 3] = [Self::Fast, Self::Medium, Self::Best];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Fast => "Fast",
            Self::Medium => "Medium",
            Self::Best => "Best",
        }
    }
}

pub const VALID_AUDIO_BIT_RATES_KBPS: [u32; 7] = [64, 96, 128, 160, 192, 256, 320];
pub const VALID_JPEG_QUALITY_RANGE: std::ops::RangeInclusive<u8> = 0..=100;

const fn default_audio_bit_rate_kbps() -> u32 {
    128
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundExportSettings {
    #[serde(default)]
    pub compression: SoundCompression,
    #[serde(default = "default_audio_bit_rate_kbps")]
    pub bit_rate_kbps: u32,
    #[serde(default)]
    pub quality: SoundExportQuality,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub convert_to_mono: bool,
}

impl Default for SoundExportSettings {
    fn default() -> Self {
        Self {
            compression: SoundCompression::Default,
            bit_rate_kbps: 128,
            quality: SoundExportQuality::Best,
            convert_to_mono: false,
        }
    }
}

impl SoundExportSettings {
    pub const fn mp3_default() -> Self {
        Self {
            compression: SoundCompression::Mp3,
            bit_rate_kbps: 128,
            quality: SoundExportQuality::Best,
            convert_to_mono: false,
        }
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn is_valid_bit_rate(kbps: u32) -> bool {
        VALID_AUDIO_BIT_RATES_KBPS.contains(&kbps)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BitmapCompression {
    #[default]
    PhotoJpeg,
    LosslessPng,
}

impl BitmapCompression {
    pub const ALL: [Self; 2] = [Self::PhotoJpeg, Self::LosslessPng];

    pub const fn label(self) -> &'static str {
        match self {
            Self::PhotoJpeg => "Photo (JPEG)",
            Self::LosslessPng => "Lossless (PNG/GIF)",
        }
    }
}

const fn default_true() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

const fn default_jpeg_quality() -> u8 {
    80
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BitmapExportSettings {
    #[serde(default)]
    pub compression: BitmapCompression,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub use_imported_jpeg_data: bool,
    #[serde(default = "default_jpeg_quality")]
    pub jpeg_quality: u8,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_smoothing: bool,
}

impl Default for BitmapExportSettings {
    fn default() -> Self {
        Self {
            compression: BitmapCompression::PhotoJpeg,
            use_imported_jpeg_data: true,
            jpeg_quality: 80,
            allow_smoothing: false,
        }
    }
}

impl BitmapExportSettings {
    pub const fn lossless_default() -> Self {
        Self {
            compression: BitmapCompression::LosslessPng,
            use_imported_jpeg_data: true,
            jpeg_quality: 80,
            allow_smoothing: false,
        }
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceUnit {
    pub id: Id,
    pub path: String,
    pub media_type: String,
    /// Shared by document/history clones; serialized as an ordinary JSON string.
    pub data_base64: Arc<str>,
    /// Optional per-resource publish audio compression settings (document version 9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_export: Option<SoundExportSettings>,
    /// Optional per-resource publish bitmap compression settings (document version 9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitmap_export: Option<BitmapExportSettings>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("Invalid project resource: {0}")]
pub struct ResourceError(pub String);

impl ResourceUnit {
    pub fn from_bytes(
        id: impl Into<Id>,
        path: impl Into<String>,
        media_type: impl Into<String>,
        bytes: &[u8],
    ) -> Result<Self, ResourceError> {
        if bytes.len() > MAX_RESOURCE_BYTES {
            return Err(ResourceError("decoded resource exceeds 16 MiB".into()));
        }
        let mut result = Self {
            id: id.into(),
            path: path.into(),
            media_type: media_type.into(),
            data_base64: Arc::from(""),
            audio_export: None,
            bitmap_export: None,
        };
        result.validate_metadata()?;
        result.data_base64 = STANDARD.encode(bytes).into();
        Ok(result)
    }

    /// Audio resources (`audio/*`) can be attached to keyframes as timeline sounds.
    pub fn is_audio(&self) -> bool {
        self.media_type
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("audio/"))
    }

    /// Image resources (`image/*`) can be placed as bitmaps or used in bitmap fills.
    pub fn is_image(&self) -> bool {
        self.media_type
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("image/"))
    }

    /// Video resources (`video/*`) can be attached to authored timeline video spans.
    pub fn is_video(&self) -> bool {
        self.media_type
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("video/"))
    }

    /// O(1) decoded length for canonical data in a validated document. This is
    /// not a validator; use decoded_bytes() when handling untrusted resource data.
    pub fn byte_len(&self) -> usize {
        let padding = usize::from(self.data_base64.ends_with('='))
            + usize::from(self.data_base64.ends_with("=="));
        (self.data_base64.len() / 4 * 3).saturating_sub(padding)
    }

    /// Strict standard alphabet, required canonical padding and zero trailing bits.
    /// Preflight encoded size bounds the allocation even for untrusted JSON data.
    pub fn decoded_bytes(&self) -> Result<Vec<u8>, ResourceError> {
        if self.data_base64.len() > MAX_ENCODED_RESOURCE_BYTES {
            return Err(ResourceError(
                "encoded resource exceeds the 16 MiB decoded limit".into(),
            ));
        }
        let bytes = STANDARD
            .decode(self.data_base64.as_bytes())
            .map_err(|error| {
                ResourceError(format!(
                    "data_base64 must be canonical standard base64: {error}"
                ))
            })?;
        if bytes.len() > MAX_RESOURCE_BYTES {
            return Err(ResourceError("decoded resource exceeds 16 MiB".into()));
        }
        Ok(bytes)
    }

    pub(crate) fn validate_metadata(&self) -> Result<(), ResourceError> {
        if self.id.is_empty() || self.id.len() > 256 || self.id.contains('\0') {
            return Err(ResourceError(
                "identity must be nonempty, <=256 bytes, without NUL".into(),
            ));
        }
        if self.path.len() > 1024
            || self.path.contains(['\\', ':', '?', '#'])
            || self.path.chars().any(char::is_control)
            || self
                .path
                .split('/')
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
        {
            return Err(ResourceError("path must be a normalized relative path of at most 1024 UTF-8 bytes without query, fragment, control characters or traversal".into()));
        }
        let token = |part: &str| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'+-.^_`|~".contains(&byte))
        };
        let valid_type = self
            .media_type
            .split_once('/')
            .is_some_and(|(a, b)| token(a) && token(b));
        if self.media_type.len() > 255 || !valid_type {
            return Err(ResourceError("media_type must be an ASCII type/subtype token without parameters, at most 255 bytes".into()));
        }
        Ok(())
    }
}
