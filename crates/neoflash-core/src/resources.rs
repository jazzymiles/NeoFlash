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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceUnit {
    pub id: Id,
    pub path: String,
    pub media_type: String,
    /// Shared by document/history clones; serialized as an ordinary JSON string.
    pub data_base64: Arc<str>,
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
        };
        result.validate_metadata()?;
        result.data_base64 = STANDARD.encode(bytes).into();
        Ok(result)
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
