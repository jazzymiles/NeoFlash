//! Self-contained binary `.nflash` bundle (`NFLASHB1`) for large-document persistence (`T-471`).
//!
//! Ordinary authoring documents that satisfy inline resource limits (`16 MiB` per resource,
//! `64 MiB` total resources, `1024` resources) and fit within `MAX_PROJECT_JSON_BYTES` (`128 MiB`)
//! serialize through [`Project::to_nflash_bytes`] as plain UTF-8 JSON (100% identical to
//! [`Project::to_json`]).
//!
//! Documents that opt into `large_bundle` (up to `MAX_BUNDLE_RESOURCE_BYTES` = `128 MiB` per
//! resource, `MAX_BUNDLE_RESOURCE_TOTAL_BYTES` = `512 MiB` total resources, `MAX_BUNDLE_RESOURCES`
//! = `4096`, `MAX_NFLASH_BUNDLE_BYTES` = `640 MiB`) or exceed the `128 MiB` inline base64 JSON
//! ceiling serialize as a zero-dependency `NFLASHB1` binary container:
//! - `0..8`: magic `b"NFLASHB1"`
//! - `8..12`: little-endian `u32` container version (`1`)
//! - `12..20`: little-endian `u64` UTF-8 `project.json` byte length (externalized resource payloads)
//! - `20..28`: little-endian `u64` UTF-8 JSON manifest byte length
//! - `project.json` bytes
//! - manifest JSON bytes (`{ version: 1, resources: [{ id, path, media_type, offset, len, sha256 }] }`)
//! - raw concatenated binary resource payloads verified by bounds and SHA-256 on load.

use crate::sha256::Sha256;
use crate::{
    Id, MAX_BUNDLE_RESOURCE_BYTES, MAX_BUNDLE_RESOURCE_TOTAL_BYTES, MAX_BUNDLE_RESOURCES,
    MAX_NFLASH_BUNDLE_BYTES, MAX_PROJECT_JSON_BYTES, Project, ProjectError, ResourceUnit,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// 8-byte magic header identifying a binary `.nflash` bundle (`b"NFLASHB1"`).
pub const NFLASH_BUNDLE_MAGIC: &[u8; 8] = b"NFLASHB1";
/// Supported `NFLASHB1` binary container format version.
pub const NFLASH_BUNDLE_VERSION: u32 = 1;
/// Fixed byte size of the `NFLASHB1` binary header (`8 + 4 + 8 + 8 = 28` bytes).
pub const NFLASH_BUNDLE_HEADER_BYTES: usize = 28;
const MAX_BUNDLE_MANIFEST_BYTES: usize = 16 * 1024 * 1024;

/// Per-resource manifest record inside an `NFLASHB1` bundle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NflashBundleEntry {
    pub id: Id,
    pub path: String,
    pub media_type: String,
    pub offset: u64,
    pub len: u64,
    pub sha256: String,
}

/// Top-level JSON manifest stored inside an `NFLASHB1` bundle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NflashBundleManifest {
    pub version: u32,
    pub resources: Vec<NflashBundleEntry>,
}

/// True when `bytes` begins with the 8-byte `b"NFLASHB1"` container magic.
#[must_use]
pub fn is_nflash_bundle_bytes(bytes: &[u8]) -> bool {
    bytes.starts_with(NFLASH_BUNDLE_MAGIC)
}

/// True when `bytes` looks like a complete, versioned, or truncated `NFLASH` / `NFLASHB*` binary
/// header (`>= 6` bytes) rather than a UTF-8 JSON document.
#[must_use]
pub fn has_nflash_bundle_prefix(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"NFLASHB") {
        return true;
    }
    bytes.len() >= 6
        && bytes.len() < NFLASH_BUNDLE_MAGIC.len()
        && NFLASH_BUNDLE_MAGIC.starts_with(bytes)
}

impl Project {
    /// Serialize this project to `.nflash` file bytes.
    ///
    /// Emits ordinary UTF-8 JSON bytes (100% identical to [`Project::to_json`]) whenever the
    /// project does not opt into `large_bundle`, passes standard inline [`Project::validate`], and
    /// fits within `MAX_PROJECT_JSON_BYTES` (`128 MiB`). Otherwise validates via
    /// [`Project::validate`] (honoring `is_large_bundle()`) and emits a self-contained `NFLASHB1`
    /// binary bundle.
    pub fn to_nflash_bytes(&self) -> Result<Vec<u8>, ProjectError> {
        if !self.is_large_bundle() && crate::validation::validate_inline_only(self).is_empty() {
            match self.to_json() {
                Ok(json) => return Ok(json.into_bytes()),
                Err(ProjectError::Invalid(msg)) if msg.contains("128 MiB encoded JSON limit") => {}
                Err(err) => return Err(err),
            }
        }
        self.encode_nflash_bundle()
    }

    /// Parse and validate `.nflash` file bytes from either plain UTF-8 JSON or an `NFLASHB1`
    /// binary container.
    pub fn from_nflash_bytes(bytes: &[u8]) -> Result<Self, ProjectError> {
        if is_nflash_bundle_bytes(bytes) {
            return decode_nflash_bundle(bytes);
        }
        if has_nflash_bundle_prefix(bytes) {
            return Err(ProjectError::Invalid(
                "truncated or unsupported NFLASHB1 bundle header".into(),
            ));
        }
        if bytes.len() > MAX_PROJECT_JSON_BYTES {
            return Err(ProjectError::Invalid(
                "document exceeds the 128 MiB encoded JSON limit".into(),
            ));
        }
        let raw = std::str::from_utf8(bytes)
            .map_err(|e| ProjectError::Invalid(format!("invalid UTF-8 in project JSON: {e}")))?;
        Self::from_json(raw)
    }

    fn encode_nflash_bundle(&self) -> Result<Vec<u8>, ProjectError> {
        self.validate()?;

        let total_blob_bytes = self
            .resources
            .iter()
            .try_fold(0usize, |acc, res| acc.checked_add(res.byte_len()))
            .ok_or_else(|| {
                ProjectError::Invalid("bundle resource byte length overflowed".into())
            })?;
        if total_blob_bytes > MAX_BUNDLE_RESOURCE_TOTAL_BYTES {
            return Err(ProjectError::Invalid(
                "decoded resources exceed the 512 MiB total budget".into(),
            ));
        }

        let stripped_resources: Vec<ResourceUnit> = self
            .resources
            .iter()
            .map(|res| ResourceUnit {
                id: res.id.clone(),
                path: res.path.clone(),
                media_type: res.media_type.clone(),
                data_base64: Arc::from(""),
                audio_export: res.audio_export,
                bitmap_export: res.bitmap_export,
            })
            .collect();

        let skeleton = Project {
            format_version: self.format_version,
            name: self.name.clone(),
            width: self.width,
            height: self.height,
            fps: self.fps,
            background: self.background,
            root: self.root.clone(),
            symbols: self.symbols.clone(),
            library_folders: self.library_folders.clone(),
            sources: self.sources.clone(),
            resources: stripped_resources,
            document_class: self.document_class.clone(),
            scenes: self.scenes.clone(),
            script_language: self.script_language,
            also_compile_compatible_scripts: self.also_compile_compatible_scripts,
        };

        let skeleton_json = serde_json::to_vec(&skeleton)?;
        if skeleton_json.len() > MAX_PROJECT_JSON_BYTES {
            return Err(ProjectError::Invalid(
                "bundle project skeleton exceeds the 128 MiB JSON limit".into(),
            ));
        }

        let mut raw_blobs = Vec::with_capacity(total_blob_bytes);
        let mut entries = Vec::with_capacity(self.resources.len());
        for res in &self.resources {
            let start = raw_blobs.len();
            append_decoded_base64(&res.data_base64, &mut raw_blobs)?;
            let end = raw_blobs.len();
            let len = end - start;
            if len > MAX_BUNDLE_RESOURCE_BYTES {
                return Err(ProjectError::Invalid(format!(
                    "bundle resource '{}' exceeds 128 MiB",
                    res.id
                )));
            }
            let sha256 = Sha256::digest_hex(&raw_blobs[start..end]);
            entries.push(NflashBundleEntry {
                id: res.id.clone(),
                path: res.path.clone(),
                media_type: res.media_type.clone(),
                offset: start as u64,
                len: len as u64,
                sha256,
            });
        }

        let manifest = NflashBundleManifest {
            version: NFLASH_BUNDLE_VERSION,
            resources: entries,
        };
        let manifest_bytes = serde_json::to_vec(&manifest)?;
        if manifest_bytes.len() > MAX_BUNDLE_MANIFEST_BYTES {
            return Err(ProjectError::Invalid(
                "bundle manifest exceeds the 16 MiB limit".into(),
            ));
        }

        let total_len = NFLASH_BUNDLE_HEADER_BYTES
            .checked_add(skeleton_json.len())
            .and_then(|n| n.checked_add(manifest_bytes.len()))
            .and_then(|n| n.checked_add(raw_blobs.len()))
            .ok_or_else(|| ProjectError::Invalid("bundle container size overflowed".into()))?;
        if total_len > MAX_NFLASH_BUNDLE_BYTES {
            return Err(ProjectError::Invalid(
                "bundle exceeds the 640 MiB container limit".into(),
            ));
        }

        let mut out = Vec::with_capacity(total_len);
        out.extend_from_slice(NFLASH_BUNDLE_MAGIC);
        out.extend_from_slice(&NFLASH_BUNDLE_VERSION.to_le_bytes());
        out.extend_from_slice(&(skeleton_json.len() as u64).to_le_bytes());
        out.extend_from_slice(&(manifest_bytes.len() as u64).to_le_bytes());
        out.extend_from_slice(&skeleton_json);
        out.extend_from_slice(&manifest_bytes);
        out.extend_from_slice(&raw_blobs);
        Ok(out)
    }
}

fn append_decoded_base64(encoded: &str, out: &mut Vec<u8>) -> Result<(), ProjectError> {
    let raw = encoded.as_bytes();
    if raw.is_empty() {
        return Ok(());
    }
    const CHUNK_ENC: usize = 65_536;
    let mut scratch = [0u8; CHUNK_ENC / 4 * 3];
    let mut offset = 0usize;
    while offset + CHUNK_ENC < raw.len() {
        let chunk = &raw[offset..offset + CHUNK_ENC];
        if chunk.contains(&b'=') {
            return Err(ProjectError::Invalid(
                "invalid resource base64: padding before final quantum".into(),
            ));
        }
        let n = STANDARD
            .decode_slice(chunk, &mut scratch)
            .map_err(|e| ProjectError::Invalid(format!("invalid resource base64: {e}")))?;
        out.extend_from_slice(&scratch[..n]);
        offset += CHUNK_ENC;
    }
    let n = STANDARD
        .decode_slice(&raw[offset..], &mut scratch)
        .map_err(|e| ProjectError::Invalid(format!("invalid resource base64: {e}")))?;
    out.extend_from_slice(&scratch[..n]);
    Ok(())
}

fn decode_nflash_bundle(bytes: &[u8]) -> Result<Project, ProjectError> {
    if bytes.len() > MAX_NFLASH_BUNDLE_BYTES {
        return Err(ProjectError::Invalid(
            "bundle exceeds the 640 MiB container limit".into(),
        ));
    }
    if bytes.len() < NFLASH_BUNDLE_HEADER_BYTES {
        return Err(ProjectError::Invalid(
            "truncated NFLASHB1 bundle header".into(),
        ));
    }
    if &bytes[..8] != NFLASH_BUNDLE_MAGIC {
        return Err(ProjectError::Invalid("invalid NFLASHB1 magic".into()));
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into().expect("4-byte version"));
    if version != NFLASH_BUNDLE_VERSION {
        return Err(ProjectError::Invalid(format!(
            "unsupported NFLASHB1 version {version}; expected {NFLASH_BUNDLE_VERSION}"
        )));
    }

    let json_len_u64 = u64::from_le_bytes(bytes[12..20].try_into().expect("8-byte json_len"));
    let manifest_len_u64 =
        u64::from_le_bytes(bytes[20..28].try_into().expect("8-byte manifest_len"));
    let json_len = usize::try_from(json_len_u64)
        .map_err(|_| ProjectError::Invalid("NFLASHB1 json_len overflow".into()))?;
    let manifest_len = usize::try_from(manifest_len_u64)
        .map_err(|_| ProjectError::Invalid("NFLASHB1 manifest_len overflow".into()))?;

    if json_len > MAX_PROJECT_JSON_BYTES {
        return Err(ProjectError::Invalid(
            "NFLASHB1 project JSON exceeds the 128 MiB limit".into(),
        ));
    }
    if manifest_len > MAX_BUNDLE_MANIFEST_BYTES {
        return Err(ProjectError::Invalid(
            "NFLASHB1 manifest exceeds the 16 MiB limit".into(),
        ));
    }

    let json_start = NFLASH_BUNDLE_HEADER_BYTES;
    let json_end = json_start
        .checked_add(json_len)
        .ok_or_else(|| ProjectError::Invalid("NFLASHB1 json section overflow".into()))?;
    let manifest_end = json_end
        .checked_add(manifest_len)
        .ok_or_else(|| ProjectError::Invalid("NFLASHB1 manifest section overflow".into()))?;
    if manifest_end > bytes.len() {
        return Err(ProjectError::Invalid(
            "truncated NFLASHB1 metadata section".into(),
        ));
    }

    let manifest: NflashBundleManifest = serde_json::from_slice(&bytes[json_end..manifest_end])?;
    if manifest.version != NFLASH_BUNDLE_VERSION {
        return Err(ProjectError::Invalid(format!(
            "unsupported NFLASHB1 manifest version {}",
            manifest.version
        )));
    }
    if manifest.resources.len() > MAX_BUNDLE_RESOURCES {
        return Err(ProjectError::Invalid(format!(
            "NFLASHB1 resource count exceeds {MAX_BUNDLE_RESOURCES}"
        )));
    }

    let blob_section = &bytes[manifest_end..];
    let mut expected_offset = 0u64;
    let mut total_blob_bytes = 0usize;
    for entry in &manifest.resources {
        if entry.offset != expected_offset {
            return Err(ProjectError::Invalid(format!(
                "non-contiguous NFLASHB1 offset {} for resource '{}' (expected {expected_offset})",
                entry.offset, entry.id
            )));
        }
        let len = usize::try_from(entry.len)
            .map_err(|_| ProjectError::Invalid("NFLASHB1 resource len overflow".into()))?;
        if len > MAX_BUNDLE_RESOURCE_BYTES {
            return Err(ProjectError::Invalid(format!(
                "NFLASHB1 resource '{}' exceeds 128 MiB",
                entry.id
            )));
        }
        total_blob_bytes = total_blob_bytes
            .checked_add(len)
            .ok_or_else(|| ProjectError::Invalid("NFLASHB1 total resource overflow".into()))?;
        if total_blob_bytes > MAX_BUNDLE_RESOURCE_TOTAL_BYTES {
            return Err(ProjectError::Invalid(
                "NFLASHB1 resources exceed the 512 MiB total budget".into(),
            ));
        }
        let start = usize::try_from(entry.offset)
            .map_err(|_| ProjectError::Invalid("NFLASHB1 resource offset overflow".into()))?;
        let end = start
            .checked_add(len)
            .ok_or_else(|| ProjectError::Invalid("NFLASHB1 resource span overflow".into()))?;
        if end > blob_section.len() {
            return Err(ProjectError::Invalid(format!(
                "truncated NFLASHB1 payload for resource '{}'",
                entry.id
            )));
        }
        if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ProjectError::Invalid(format!(
                "invalid SHA-256 digest format for resource '{}'",
                entry.id
            )));
        }
        let actual_sha = Sha256::digest_hex(&blob_section[start..end]);
        if !actual_sha.eq_ignore_ascii_case(&entry.sha256) {
            return Err(ProjectError::Invalid(format!(
                "NFLASHB1 SHA-256 mismatch for resource '{}' (expected {}, got {actual_sha})",
                entry.id, entry.sha256
            )));
        }
        expected_offset = expected_offset
            .checked_add(entry.len)
            .ok_or_else(|| ProjectError::Invalid("NFLASHB1 offset cursor overflow".into()))?;
    }
    if expected_offset as usize != blob_section.len() {
        return Err(ProjectError::Invalid(
            "NFLASHB1 bundle contains trailing or unindexed bytes".into(),
        ));
    }

    let project_json_str = std::str::from_utf8(&bytes[json_start..json_end]).map_err(|e| {
        ProjectError::Invalid(format!("invalid UTF-8 in NFLASHB1 project JSON: {e}"))
    })?;
    let mut project: Project = serde_json::from_str(project_json_str)?;
    crate::validation::apply_legacy_format_upgrades(&mut project);

    if project.resources.len() != manifest.resources.len() {
        return Err(ProjectError::Invalid(format!(
            "NFLASHB1 resource count mismatch (project={}, manifest={})",
            project.resources.len(),
            manifest.resources.len()
        )));
    }

    for (res, entry) in project.resources.iter_mut().zip(&manifest.resources) {
        if res.id != entry.id || res.path != entry.path || res.media_type != entry.media_type {
            return Err(ProjectError::Invalid(format!(
                "NFLASHB1 resource metadata mismatch for '{}'",
                entry.id
            )));
        }
        if !res.data_base64.is_empty() {
            return Err(ProjectError::Invalid(format!(
                "NFLASHB1 skeleton resource '{}' must not duplicate inline base64",
                entry.id
            )));
        }
        let start = entry.offset as usize;
        let end = start + entry.len as usize;
        let slice = &blob_section[start..end];
        res.data_base64 = if slice.is_empty() {
            Arc::from("")
        } else {
            STANDARD.encode(slice).into()
        };
    }

    project.validate()?;
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_project_emits_plain_utf8_json_and_large_bundle_round_trips_with_sha256() {
        let demo = Project::demo();
        let plain_bytes = demo.to_nflash_bytes().expect("demo to_nflash_bytes");
        assert!(!is_nflash_bundle_bytes(&plain_bytes));
        assert_eq!(plain_bytes, demo.to_json().unwrap().into_bytes());
        assert_eq!(Project::from_nflash_bytes(&plain_bytes).unwrap(), demo);

        assert!(!has_nflash_bundle_prefix(b""));
        assert!(!has_nflash_bundle_prefix(b"N"));
        assert!(!has_nflash_bundle_prefix(b"NaN"));
        assert!(!has_nflash_bundle_prefix(b"NFLAS"));
        assert!(has_nflash_bundle_prefix(b"NFLASH"));
        assert!(has_nflash_bundle_prefix(b"NFLASHB"));
        assert!(has_nflash_bundle_prefix(b"NFLASHB1"));

        let mut large = Project::blank();
        large.set_large_bundle(true);
        let payload = vec![0x5Au8; 64 * 1024];
        large.resources.push(
            ResourceUnit::new_large(
                "blob-1",
                "media/blob1.bin",
                "application/octet-stream",
                &payload,
            )
            .unwrap(),
        );
        large.validate().expect("large_bundle validates");

        let bundled = large.to_nflash_bytes().expect("encode NFLASHB1");
        assert!(is_nflash_bundle_bytes(&bundled));
        let restored = Project::from_nflash_bytes(&bundled).expect("decode NFLASHB1");
        assert_eq!(restored, large);
        assert!(restored.is_large_bundle());
        restored.validate().expect("decoded project must validate");
        let restored_blob = restored
            .resources
            .iter()
            .find(|r| r.id == "blob-1")
            .unwrap()
            .decoded_bundle_bytes()
            .unwrap();
        assert_eq!(restored_blob, payload);

        // Corrupt 1 payload byte -> SHA-256 mismatch rejected cleanly.
        let mut corrupted = bundled.clone();
        let last = corrupted.len() - 1;
        corrupted[last] ^= 0x01;
        let err = Project::from_nflash_bytes(&corrupted)
            .unwrap_err()
            .to_string();
        assert!(err.contains("SHA-256 mismatch"), "got: {err}");

        // Unopted project with >16 MiB resource fails closed on `to_nflash_bytes()`
        // and tampered `NFLASHB1` omitting `large_bundle` on >16 MiB resource fails `from_nflash_bytes()`.
        let big_payload = vec![0x11u8; crate::MAX_RESOURCE_BYTES + 1024];
        let mut unopted = Project::blank();
        unopted.resources.push(
            ResourceUnit::new_large(
                "too-big",
                "media/big.bin",
                "application/octet-stream",
                &big_payload,
            )
            .unwrap(),
        );
        assert!(unopted.validate().is_err());
        assert!(unopted.to_nflash_bytes().is_err());

        let mut opted_big = unopted;
        opted_big.set_large_bundle(true);
        let valid_big_bundle = opted_big.to_nflash_bytes().unwrap();
        // Strip `"publish/settings.json"` (`large_bundle: true`) from the skeleton & manifest while keeping valid SHA-256 on `too-big`!
        let mut skeleton = Project::blank();
        skeleton.resources.push(ResourceUnit {
            id: "too-big".into(),
            path: "media/big.bin".into(),
            media_type: "application/octet-stream".into(),
            data_base64: Arc::from(""),
            audio_export: None,
            bitmap_export: None,
        });
        let skel_bytes = serde_json::to_vec(&skeleton).unwrap();
        let tampered_manifest = serde_json::to_vec(&NflashBundleManifest {
            version: NFLASH_BUNDLE_VERSION,
            resources: vec![NflashBundleEntry {
                id: "too-big".into(),
                path: "media/big.bin".into(),
                media_type: "application/octet-stream".into(),
                offset: 0,
                len: big_payload.len() as u64,
                sha256: Sha256::digest_hex(&big_payload),
            }],
        })
        .unwrap();
        let mut forged = Vec::new();
        forged.extend_from_slice(NFLASH_BUNDLE_MAGIC);
        forged.extend_from_slice(&NFLASH_BUNDLE_VERSION.to_le_bytes());
        forged.extend_from_slice(&(skel_bytes.len() as u64).to_le_bytes());
        forged.extend_from_slice(&(tampered_manifest.len() as u64).to_le_bytes());
        forged.extend_from_slice(&skel_bytes);
        forged.extend_from_slice(&tampered_manifest);
        forged.extend_from_slice(&big_payload);
        assert!(Project::from_nflash_bytes(&valid_big_bundle).is_ok());
        assert!(
            Project::from_nflash_bytes(&forged).is_err(),
            "forged NFLASHB1 with >16 MiB resource and large_bundle == false must fail from_nflash_bytes()"
        );

        // Truncated header & truncated payload rejected cleanly.
        for cut in [6usize, 7, 8, 16, 27, bundled.len() - 1] {
            assert!(
                Project::from_nflash_bytes(&bundled[..cut]).is_err(),
                "expected truncation failure at cut={cut}"
            );
        }
    }
}
