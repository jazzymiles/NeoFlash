//! Public extension packaging, semantic-versioned API compatibility, and capability sandbox
//! contracts (`platform-extensions`: `extension-packaging`, `extension-versioning`,
//! `extension-sandbox`).
//!
//! Third-party and user extensions in NeoFlash are distributed as deterministic `.nfext` packages
//! with an explicit `ExtensionPackageManifest`, checked against the host's semantic API version,
//! and executed behind `ExtensionHostInvocationGate`. Extensions never mutate a `Project` in place
//! or access arbitrary host files, sockets, or processes; all document modifications are emitted
//! as auditable, undoable `ProposedDocumentTransaction`s.

use crate::{AuthoringCommandStep, AuthoringExtensionManifest, ExtensionPermission, SavedCommand};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

/// Current public extension API version supported by `neoflash-core`.
pub const CURRENT_EXTENSION_API_VERSION: SemVer = SemVer::new(1, 0, 0);

/// Current NeoFlash host product version used for `min_host_version` / `max_host_version` checks.
pub const CURRENT_HOST_VERSION: SemVer = SemVer::new(1, 0, 0);

/// File extension for serialized NeoFlash extension packages.
pub const NFEXT_PACKAGE_EXTENSION: &str = "nfext";

/// Magic header prefix for JSON-encoded `.nfext` package archives.
pub const NFEXT_FORMAT_MAGIC: &str = "neoflash.nfext.v1";

/// Maximum allowed size of a serialized `.nfext` manifest in bytes (512 KiB).
pub const MAX_MANIFEST_JSON_BYTES: usize = 512 * 1024;

/// Maximum allowed size of an entire `.nfext` package archive in bytes (8 MiB).
pub const MAX_PACKAGE_ARCHIVE_BYTES: usize = 8 * 1024 * 1024;

/// Maximum number of payload entries inside an `.nfext` archive.
pub const MAX_ARCHIVE_ENTRIES: usize = 128;

/// Default maximum execution steps allowed for a single sandboxed extension invocation.
pub const DEFAULT_MAX_SANDBOX_STEPS: u64 = 10_000;

/// Hard upper ceiling on sandbox execution steps.
pub const MAX_ALLOWED_SANDBOX_STEPS: u64 = 1_000_000;

/// Default maximum memory budget in bytes for a sandboxed extension invocation (4 MiB).
pub const DEFAULT_MAX_SANDBOX_MEMORY_BYTES: usize = 4 * 1024 * 1024;

/// Hard upper ceiling on sandbox memory budget in bytes (64 MiB).
pub const MAX_ALLOWED_SANDBOX_MEMORY_BYTES: usize = 64 * 1024 * 1024;

/// Default maximum output/artifact budget in bytes for a sandboxed extension invocation (2 MiB).
pub const DEFAULT_MAX_SANDBOX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

/// Hard upper ceiling on sandbox output budget in bytes (16 MiB).
pub const MAX_ALLOWED_SANDBOX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

/// Errors produced during extension manifest validation, package verification, compatibility
/// checking, or sandboxed host invocation.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ExtensionError {
    #[error(
        "Invalid semantic version '{0}': expected 'MAJOR.MINOR.PATCH' with non-negative integers."
    )]
    InvalidSemVer(String),
    #[error(
        "Invalid extension id '{0}': expected reverse-DNS identifier (e.g. 'org.example.tool')."
    )]
    InvalidExtensionId(String),
    #[error("Invalid manifest field '{field}': {reason}")]
    InvalidManifestField { field: &'static str, reason: String },
    #[error("Manifest JSON error: {0}")]
    ManifestJson(String),
    #[error("Package archive error: {0}")]
    ArchiveIntegrity(String),
    #[error("Extension requests forbidden unsafe capability '{0:?}', which is never permitted.")]
    ForbiddenCapabilityRequested(ForbiddenCapability),
    #[error("Unknown capability identifier '{0}'.")]
    UnknownCapability(String),
    #[error("Sandbox policy violation: {0}")]
    SandboxPolicyViolation(String),
    #[error("Capability denied: host call '{call}' requires capability '{required:?}'.")]
    CapabilityDenied {
        call: &'static str,
        required: ExtensionCapability,
    },
    #[error("Sandbox step budget exhausted: requested {requested} steps (used {used}/{limit}).")]
    StepBudgetExceeded {
        requested: u64,
        used: u64,
        limit: u64,
    },
    #[error("Sandbox memory budget exhausted: requested {requested} bytes (used {used}/{limit}).")]
    MemoryBudgetExceeded {
        requested: usize,
        used: usize,
        limit: usize,
    },
    #[error("Sandbox output budget exhausted: requested {requested} bytes (used {used}/{limit}).")]
    OutputBudgetExceeded {
        requested: usize,
        used: usize,
        limit: usize,
    },
    #[error("Invalid proposed document transaction: {0}")]
    InvalidTransaction(String),
}

/// Semantic version (`MAJOR.MINOR.PATCH`) used for extension package versions, host versions,
/// and the versioned public extension API contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemVer {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl SemVer {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parse a strict `"MAJOR.MINOR.PATCH"` semantic version string (optional leading `'v'` is
    /// rejected to keep manifest strings canonical).
    pub fn parse(raw: &str) -> Result<Self, ExtensionError> {
        let trimmed = raw.trim();
        let parts: Vec<&str> = trimmed.split('.').collect();
        if parts.len() != 3 {
            return Err(ExtensionError::InvalidSemVer(raw.to_string()));
        }
        let parse_part = |s: &str| -> Result<u32, ExtensionError> {
            if s.is_empty()
                || (s.len() > 1 && s.starts_with('0'))
                || !s.bytes().all(|b| b.is_ascii_digit())
            {
                return Err(ExtensionError::InvalidSemVer(raw.to_string()));
            }
            s.parse::<u32>()
                .map_err(|_| ExtensionError::InvalidSemVer(raw.to_string()))
        };
        Ok(Self {
            major: parse_part(parts[0])?,
            minor: parse_part(parts[1])?,
            patch: parse_part(parts[2])?,
        })
    }
}

impl fmt::Display for SemVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for SemVer {
    type Err = ExtensionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for SemVer {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SemVer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum SemVerRepr {
            Text(String),
            Parts { major: u32, minor: u32, patch: u32 },
        }
        match SemVerRepr::deserialize(deserializer)? {
            SemVerRepr::Text(s) => Self::parse(&s).map_err(serde::de::Error::custom),
            SemVerRepr::Parts {
                major,
                minor,
                patch,
            } => Ok(Self::new(major, minor, patch)),
        }
    }
}

/// Entry-point kind for a packaged NeoFlash extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionEntryKind {
    /// Declarative, replayable authoring command sequences (`AuthoringCommandStep`).
    DeclarativeCommandPack,
    /// Bounded, deterministic JSFL automation script executed inside `JsflSandbox`.
    SandboxedJsflScript,
    /// Editor workspace/theme preset metadata (without overriding locked NF2 layout rules).
    ThemePreset,
    /// Custom artifact exporter producing bounded bytes via `EmitExportArtifact`.
    CustomExporter,
}

/// Safe, explicit capabilities that an extension may request in its manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionCapability {
    /// Read metadata, symbol summaries, and timeline structure of the active document.
    ReadActiveDocument,
    /// Propose auditable, undoable `AuthoringCommandStep` transactions on the active document.
    ProposeDocumentTransaction,
    /// Read the currently selected element/layer/frame identifiers.
    ReadSelection,
    /// Emit named export artifacts into the host-controlled export buffer.
    ExportArtifact,
    /// Read and write key-value state inside the extension's isolated storage namespace.
    LocalStorageScoped,
}

/// Unsafe capabilities that are permanently forbidden by the NeoFlash extension sandbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForbiddenCapability {
    /// Direct host filesystem read/write outside the host-mediated artifact buffer.
    ArbitraryFilesystemAccess,
    /// Arbitrary outbound or inbound network sockets/HTTP requests.
    UnrestrictedNetworkAccess,
    /// Spawning native OS processes or loading native dynamic libraries.
    NativeProcessSpawn,
    /// Mutating document state directly without an auditable Undo transaction.
    SilentDocumentMutation,
}

/// Parse a capability name, distinguishing safe `ExtensionCapability` values from explicitly
/// forbidden unsafe capabilities (`ForbiddenCapability`).
pub fn parse_capability_token(token: &str) -> Result<ExtensionCapability, ExtensionError> {
    match token.trim() {
        "read_active_document" => Ok(ExtensionCapability::ReadActiveDocument),
        "propose_document_transaction" => Ok(ExtensionCapability::ProposeDocumentTransaction),
        "read_selection" => Ok(ExtensionCapability::ReadSelection),
        "export_artifact" => Ok(ExtensionCapability::ExportArtifact),
        "local_storage_scoped" => Ok(ExtensionCapability::LocalStorageScoped),
        "arbitrary_filesystem_access" => Err(ExtensionError::ForbiddenCapabilityRequested(
            ForbiddenCapability::ArbitraryFilesystemAccess,
        )),
        "unrestricted_network_access" => Err(ExtensionError::ForbiddenCapabilityRequested(
            ForbiddenCapability::UnrestrictedNetworkAccess,
        )),
        "native_process_spawn" => Err(ExtensionError::ForbiddenCapabilityRequested(
            ForbiddenCapability::NativeProcessSpawn,
        )),
        "silent_document_mutation" => Err(ExtensionError::ForbiddenCapabilityRequested(
            ForbiddenCapability::SilentDocumentMutation,
        )),
        other => Err(ExtensionError::UnknownCapability(other.to_string())),
    }
}

/// Command contributed by an extension to the `Commands` menu or `F1` Function Search.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContributedCommand {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<AuthoringCommandStep>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script_entry: Option<String>,
}

/// Menu placement contributed by an extension.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContributedMenuItem {
    pub command_id: String,
    pub menu_path: String,
    pub order: i32,
}

/// Keyboard shortcut binding suggestion contributed by an extension.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContributedShortcut {
    pub command_id: String,
    pub key_chord: String,
}

/// Import or export filter descriptor contributed by an extension.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContributedFilter {
    pub id: String,
    pub label: String,
    pub file_extensions: Vec<String>,
    pub is_exporter: bool,
}

/// Declarative UI and workflow contributions declared by an `ExtensionPackageManifest`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ExtensionContributions {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<ContributedCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub menu_items: Vec<ContributedMenuItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keyboard_shortcuts: Vec<ContributedShortcut>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub import_export_filters: Vec<ContributedFilter>,
}

/// Public `.nfext` extension package manifest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtensionPackageManifest {
    pub id: String,
    pub name: String,
    pub version: SemVer,
    pub api_version: SemVer,
    pub author: String,
    pub description: String,
    pub entry_kind: ExtensionEntryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_host_version: Option<SemVer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_host_version: Option<SemVer>,
    #[serde(default)]
    pub capabilities: Vec<ExtensionCapability>,
    #[serde(default)]
    pub contributes: ExtensionContributions,
    pub integrity_sha256: String,
}

/// Validate a reverse-DNS extension identifier (e.g. `org.example.tool` or `com.neoflash.align`).
///
/// Rules:
/// - Total length in `5..=128` bytes.
/// - At least 3 dot-separated segments (`domain.vendor.name`).
/// - Each segment is `1..=48` characters, starts with an ASCII lowercase letter (`a..=z`),
///   and contains only ASCII lowercase letters, digits, `'_'`, or `'-'`.
pub fn is_valid_reverse_dns_id(id: &str) -> bool {
    if !(5..=128).contains(&id.len()) {
        return false;
    }
    let segments: Vec<&str> = id.split('.').collect();
    if segments.len() < 3 {
        return false;
    }
    for (idx, seg) in segments.iter().enumerate() {
        if seg.is_empty() || seg.len() > 48 {
            return false;
        }
        let mut chars = seg.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        if !first.is_ascii_lowercase() {
            return false;
        }
        if idx == 0 && seg.len() < 2 {
            return false;
        }
        if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-')) {
            return false;
        }
    }
    true
}

fn is_valid_sha256_hex(hex: &str) -> bool {
    hex.len() == 64
        && hex
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn is_safe_archive_relative_path(path: &str) -> bool {
    if path.is_empty()
        || path.len() > 240
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path.contains(':')
        || path.chars().any(char::is_control)
    {
        return false;
    }
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return false;
        }
    }
    true
}

/// Validate an `ExtensionPackageManifest` for structural, capability, and contribution invariants.
pub fn validate_manifest(manifest: &ExtensionPackageManifest) -> Result<(), ExtensionError> {
    if !is_valid_reverse_dns_id(&manifest.id) {
        return Err(ExtensionError::InvalidExtensionId(manifest.id.clone()));
    }
    let name = manifest.name.trim();
    if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        return Err(ExtensionError::InvalidManifestField {
            field: "name",
            reason: "Extension name must be 1..=128 non-control characters.".into(),
        });
    }
    let author = manifest.author.trim();
    if author.is_empty() || author.len() > 128 || author.chars().any(char::is_control) {
        return Err(ExtensionError::InvalidManifestField {
            field: "author",
            reason: "Author must be 1..=128 non-control characters.".into(),
        });
    }
    if manifest.description.len() > 2048 || manifest.description.chars().any(char::is_control) {
        return Err(ExtensionError::InvalidManifestField {
            field: "description",
            reason: "Description must be <= 2048 non-control characters.".into(),
        });
    }
    if let (Some(min_v), Some(max_v)) = (manifest.min_host_version, manifest.max_host_version)
        && min_v > max_v
    {
        return Err(ExtensionError::InvalidManifestField {
            field: "min_host_version",
            reason: format!("min_host_version ({min_v}) exceeds max_host_version ({max_v})."),
        });
    }
    if !is_valid_sha256_hex(&manifest.integrity_sha256) {
        return Err(ExtensionError::InvalidManifestField {
            field: "integrity_sha256",
            reason: "integrity_sha256 must be 64 lowercase hexadecimal characters.".into(),
        });
    }

    let capability_set: BTreeSet<ExtensionCapability> =
        manifest.capabilities.iter().copied().collect();
    if capability_set.len() != manifest.capabilities.len() {
        return Err(ExtensionError::InvalidManifestField {
            field: "capabilities",
            reason: "Duplicate entries in manifest capabilities.".into(),
        });
    }

    let mut command_ids = BTreeSet::new();
    for cmd in &manifest.contributes.commands {
        if cmd.id.trim().is_empty() || cmd.id.len() > 128 {
            return Err(ExtensionError::InvalidManifestField {
                field: "contributes.commands.id",
                reason: "Command id must be 1..=128 characters.".into(),
            });
        }
        if !command_ids.insert(cmd.id.clone()) {
            return Err(ExtensionError::InvalidManifestField {
                field: "contributes.commands.id",
                reason: format!("Duplicate contributed command id '{}'.", cmd.id),
            });
        }
        let saved = SavedCommand {
            name: cmd.title.clone(),
            steps: cmd.steps.clone().unwrap_or_default(),
        };
        if let Some(steps) = &cmd.steps {
            if steps.is_empty() {
                return Err(ExtensionError::InvalidManifestField {
                    field: "contributes.commands.steps",
                    reason: format!("Command '{}' has an empty steps array.", cmd.id),
                });
            }
            saved
                .validate()
                .map_err(|reason| ExtensionError::InvalidManifestField {
                    field: "contributes.commands",
                    reason,
                })?;
            if !capability_set.contains(&ExtensionCapability::ProposeDocumentTransaction) {
                return Err(ExtensionError::InvalidManifestField {
                    field: "capabilities",
                    reason: format!(
                        "Command '{}' declares document steps but manifest omits 'propose_document_transaction'.",
                        cmd.id
                    ),
                });
            }
        } else if cmd.title.trim().is_empty() || cmd.title.len() > 128 {
            return Err(ExtensionError::InvalidManifestField {
                field: "contributes.commands.title",
                reason: "Command title must be 1..=128 characters.".into(),
            });
        }
        if let Some(entry) = &cmd.script_entry
            && !is_safe_archive_relative_path(entry)
        {
            return Err(ExtensionError::InvalidManifestField {
                field: "contributes.commands.script_entry",
                reason: format!("Unsafe script_entry path '{entry}'."),
            });
        }
    }

    for item in &manifest.contributes.menu_items {
        if !command_ids.contains(&item.command_id) {
            return Err(ExtensionError::InvalidManifestField {
                field: "contributes.menu_items",
                reason: format!(
                    "Menu item references unknown contributed command_id '{}'.",
                    item.command_id
                ),
            });
        }
        if item.menu_path.trim().is_empty() || item.menu_path.len() > 128 {
            return Err(ExtensionError::InvalidManifestField {
                field: "contributes.menu_items.menu_path",
                reason: "Menu path must be 1..=128 characters.".into(),
            });
        }
    }

    for sc in &manifest.contributes.keyboard_shortcuts {
        if !command_ids.contains(&sc.command_id) {
            return Err(ExtensionError::InvalidManifestField {
                field: "contributes.keyboard_shortcuts",
                reason: format!(
                    "Keyboard shortcut references unknown contributed command_id '{}'.",
                    sc.command_id
                ),
            });
        }
        if sc.key_chord.trim().is_empty() || sc.key_chord.len() > 64 {
            return Err(ExtensionError::InvalidManifestField {
                field: "contributes.keyboard_shortcuts.key_chord",
                reason: "Key chord must be 1..=64 characters.".into(),
            });
        }
    }

    for filter in &manifest.contributes.import_export_filters {
        if filter.id.trim().is_empty()
            || filter.label.trim().is_empty()
            || filter.file_extensions.is_empty()
        {
            return Err(ExtensionError::InvalidManifestField {
                field: "contributes.import_export_filters",
                reason: "Filter id, label, and file_extensions must be non-empty.".into(),
            });
        }
        if filter.is_exporter && !capability_set.contains(&ExtensionCapability::ExportArtifact) {
            return Err(ExtensionError::InvalidManifestField {
                field: "capabilities",
                reason: format!(
                    "Exporter filter '{}' requires 'export_artifact' capability.",
                    filter.id
                ),
            });
        }
    }

    match manifest.entry_kind {
        ExtensionEntryKind::DeclarativeCommandPack => {
            if manifest.contributes.commands.is_empty() {
                return Err(ExtensionError::InvalidManifestField {
                    field: "contributes.commands",
                    reason: "DeclarativeCommandPack must contribute at least one command.".into(),
                });
            }
        }
        ExtensionEntryKind::CustomExporter => {
            if !capability_set.contains(&ExtensionCapability::ExportArtifact) {
                return Err(ExtensionError::InvalidManifestField {
                    field: "capabilities",
                    reason: "CustomExporter entry_kind requires 'export_artifact' capability."
                        .into(),
                });
            }
        }
        ExtensionEntryKind::SandboxedJsflScript | ExtensionEntryKind::ThemePreset => {}
    }

    Ok(())
}

impl ExtensionPackageManifest {
    pub fn validate(&self) -> Result<(), ExtensionError> {
        validate_manifest(self)
    }

    pub fn to_manifest_json(&self) -> Result<String, ExtensionError> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|e| ExtensionError::ManifestJson(e.to_string()))
    }

    /// Parse a manifest from JSON, explicitly rejecting forbidden capabilities before general
    /// deserialization so security diagnostics remain unambiguous.
    pub fn from_manifest_json(raw: &str) -> Result<Self, ExtensionError> {
        if raw.len() > MAX_MANIFEST_JSON_BYTES {
            return Err(ExtensionError::ManifestJson(format!(
                "Manifest JSON exceeds {MAX_MANIFEST_JSON_BYTES} bytes."
            )));
        }
        let value: serde_json::Value =
            serde_json::from_str(raw).map_err(|e| ExtensionError::ManifestJson(e.to_string()))?;
        if let Some(caps) = value.get("capabilities").and_then(|v| v.as_array()) {
            for cap_val in caps {
                if let Some(s) = cap_val.as_str() {
                    parse_capability_token(s)?;
                }
            }
        }
        let manifest: Self = serde_json::from_value(value)
            .map_err(|e| ExtensionError::ManifestJson(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Convert a legacy internal `AuthoringExtensionManifest` into a public `ExtensionPackageManifest`.
    pub fn from_authoring_manifest(
        legacy: &AuthoringExtensionManifest,
        reverse_dns_prefix: &str,
    ) -> Result<Self, ExtensionError> {
        legacy
            .validate()
            .map_err(|reason| ExtensionError::InvalidManifestField {
                field: "legacy_manifest",
                reason,
            })?;
        let sanitized_leaf: String = legacy
            .id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect();
        let full_id = if is_valid_reverse_dns_id(&legacy.id) {
            legacy.id.clone()
        } else {
            format!(
                "{}.{}",
                reverse_dns_prefix.trim_end_matches('.'),
                sanitized_leaf
            )
        };
        let version = SemVer::parse(&legacy.version).unwrap_or(SemVer::new(1, 0, 0));
        let commands: Vec<ContributedCommand> = legacy
            .commands
            .iter()
            .enumerate()
            .map(|(idx, cmd)| ContributedCommand {
                id: format!("cmd_{idx}"),
                title: cmd.name.clone(),
                steps: Some(cmd.steps.clone()),
                script_entry: None,
            })
            .collect();
        let manifest = Self {
            id: full_id,
            name: legacy.name.clone(),
            version,
            api_version: CURRENT_EXTENSION_API_VERSION,
            author: if legacy.author.trim().is_empty() {
                "Unknown".into()
            } else {
                legacy.author.clone()
            },
            description: String::new(),
            entry_kind: ExtensionEntryKind::DeclarativeCommandPack,
            min_host_version: Some(CURRENT_HOST_VERSION),
            max_host_version: None,
            capabilities: vec![
                ExtensionCapability::ReadActiveDocument,
                ExtensionCapability::ReadSelection,
                ExtensionCapability::ProposeDocumentTransaction,
            ],
            contributes: ExtensionContributions {
                commands,
                menu_items: Vec::new(),
                keyboard_shortcuts: Vec::new(),
                import_export_filters: Vec::new(),
            },
            integrity_sha256: sha256_hex(&[]),
        };
        manifest.validate()?;
        Ok(manifest)
    }

    /// Extract a legacy `AuthoringExtensionManifest` for declarative command packs.
    pub fn to_authoring_manifest(&self) -> Result<AuthoringExtensionManifest, ExtensionError> {
        self.validate()?;
        let mut permissions = BTreeSet::new();
        let mut commands = Vec::new();
        for cmd in &self.contributes.commands {
            if let Some(steps) = &cmd.steps {
                for step in steps {
                    let perm: ExtensionPermission = step.required_permission();
                    permissions.insert(match perm {
                        ExtensionPermission::StageTransform => 0u8,
                        ExtensionPermission::StyleMutation => 1u8,
                        ExtensionPermission::SymbolMutation => 2u8,
                        ExtensionPermission::StructureMutation => 3u8,
                    });
                }
                commands.push(SavedCommand {
                    name: cmd.title.clone(),
                    steps: steps.clone(),
                });
            }
        }
        let mapped_permissions = permissions
            .into_iter()
            .map(|code| match code {
                0 => ExtensionPermission::StageTransform,
                1 => ExtensionPermission::StyleMutation,
                2 => ExtensionPermission::SymbolMutation,
                _ => ExtensionPermission::StructureMutation,
            })
            .collect();
        let legacy = AuthoringExtensionManifest {
            schema_version: AuthoringExtensionManifest::CURRENT_SCHEMA_VERSION,
            id: self.id.clone(),
            name: self.name.clone(),
            version: self.version.to_string(),
            author: self.author.clone(),
            permissions: mapped_permissions,
            commands,
        };
        legacy
            .validate()
            .map_err(|reason| ExtensionError::InvalidManifestField {
                field: "commands",
                reason,
            })?;
        Ok(legacy)
    }
}

/// A single asset or script payload entry inside an `ExtensionPackageArchive`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionArchiveEntry {
    pub path: String,
    pub sha256: String,
    pub contents_utf8: String,
}

impl ExtensionArchiveEntry {
    pub fn from_utf8(path: impl Into<String>, contents_utf8: impl Into<String>) -> Self {
        let contents_utf8 = contents_utf8.into();
        let sha256 = sha256_hex(contents_utf8.as_bytes());
        Self {
            path: path.into(),
            sha256,
            contents_utf8,
        }
    }
}

/// Deterministic `.nfext` package archive bundling an `ExtensionPackageManifest` and verified
/// script/asset entries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtensionPackageArchive {
    pub format_magic: String,
    pub manifest: ExtensionPackageManifest,
    pub entries: Vec<ExtensionArchiveEntry>,
}

/// Compute the deterministic canonical SHA-256 digest over a sorted slice of archive entries.
pub fn compute_archive_entries_sha256(entries: &[ExtensionArchiveEntry]) -> String {
    let mut sorted: Vec<&ExtensionArchiveEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut canonical = Vec::new();
    for entry in sorted {
        canonical.extend_from_slice(entry.path.as_bytes());
        canonical.push(0);
        canonical.extend_from_slice(entry.contents_utf8.as_bytes());
        canonical.push(0xff);
    }
    sha256_hex(&canonical)
}

impl ExtensionPackageArchive {
    /// Seal a manifest and payload entries into a verified `.nfext` package archive, computing
    /// `manifest.integrity_sha256` deterministically from the sorted payload entries.
    pub fn seal(
        mut manifest: ExtensionPackageManifest,
        mut entries: Vec<ExtensionArchiveEntry>,
    ) -> Result<Self, ExtensionError> {
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        for entry in &mut entries {
            entry.sha256 = sha256_hex(entry.contents_utf8.as_bytes());
        }
        manifest.integrity_sha256 = compute_archive_entries_sha256(&entries);
        let archive = Self {
            format_magic: NFEXT_FORMAT_MAGIC.to_string(),
            manifest,
            entries,
        };
        archive.verify()?;
        Ok(archive)
    }

    /// Verify archive magic, manifest invariants, entry paths, entry SHA-256 digests, and the
    /// overall `manifest.integrity_sha256` digest.
    pub fn verify(&self) -> Result<(), ExtensionError> {
        if self.format_magic != NFEXT_FORMAT_MAGIC {
            return Err(ExtensionError::ArchiveIntegrity(format!(
                "Invalid .nfext format_magic '{}' (expected '{NFEXT_FORMAT_MAGIC}').",
                self.format_magic
            )));
        }
        self.manifest.validate()?;
        if self.entries.len() > MAX_ARCHIVE_ENTRIES {
            return Err(ExtensionError::ArchiveIntegrity(format!(
                "Archive contains {} entries, exceeding limit of {MAX_ARCHIVE_ENTRIES}.",
                self.entries.len()
            )));
        }
        let mut seen_paths = BTreeSet::new();
        let mut total_bytes = 0usize;
        for entry in &self.entries {
            if !is_safe_archive_relative_path(&entry.path) {
                return Err(ExtensionError::ArchiveIntegrity(format!(
                    "Unsafe archive entry path '{}'.",
                    entry.path
                )));
            }
            if !seen_paths.insert(entry.path.clone()) {
                return Err(ExtensionError::ArchiveIntegrity(format!(
                    "Duplicate archive entry path '{}'.",
                    entry.path
                )));
            }
            total_bytes = total_bytes.saturating_add(entry.contents_utf8.len());
            if total_bytes > MAX_PACKAGE_ARCHIVE_BYTES {
                return Err(ExtensionError::ArchiveIntegrity(format!(
                    "Archive payload exceeds maximum size of {MAX_PACKAGE_ARCHIVE_BYTES} bytes."
                )));
            }
            let actual_entry_sha = sha256_hex(entry.contents_utf8.as_bytes());
            if entry.sha256 != actual_entry_sha {
                return Err(ExtensionError::ArchiveIntegrity(format!(
                    "SHA-256 mismatch for archive entry '{}': expected {}, computed {}.",
                    entry.path, entry.sha256, actual_entry_sha
                )));
            }
        }
        for cmd in &self.manifest.contributes.commands {
            if let Some(script_entry) = &cmd.script_entry
                && !seen_paths.contains(script_entry)
            {
                return Err(ExtensionError::ArchiveIntegrity(format!(
                    "Command '{}' references missing archive entry '{}'.",
                    cmd.id, script_entry
                )));
            }
        }
        let computed_package_sha = compute_archive_entries_sha256(&self.entries);
        if self.manifest.integrity_sha256 != computed_package_sha {
            return Err(ExtensionError::ArchiveIntegrity(format!(
                "Package integrity_sha256 mismatch: manifest declares {}, computed {}.",
                self.manifest.integrity_sha256, computed_package_sha
            )));
        }
        Ok(())
    }

    pub fn to_nfext_json(&self) -> Result<String, ExtensionError> {
        self.verify()?;
        serde_json::to_string_pretty(self).map_err(|e| ExtensionError::ManifestJson(e.to_string()))
    }

    pub fn from_nfext_json(raw: &str) -> Result<Self, ExtensionError> {
        if raw.len() > MAX_PACKAGE_ARCHIVE_BYTES {
            return Err(ExtensionError::ArchiveIntegrity(format!(
                "Serialized .nfext archive exceeds {MAX_PACKAGE_ARCHIVE_BYTES} bytes."
            )));
        }
        let archive: Self =
            serde_json::from_str(raw).map_err(|e| ExtensionError::ManifestJson(e.to_string()))?;
        archive.verify()?;
        Ok(archive)
    }
}

/// Compatibility classification returned by `check_api_compatibility`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityStatus {
    /// Extension API and host version bounds match cleanly.
    Compatible,
    /// Extension is compatible with the host API but targets an older minor version with
    /// non-breaking deprecation notes.
    CompatibleWithWarnings,
    /// Major API version mismatch (`required_api.major != host_api.major`).
    IncompatibleApiMajor,
    /// Extension requires a newer minor or patch API than the host provides.
    IncompatibleApiMinorTooNew,
    /// Current host version is below `min_host_version`.
    HostVersionTooOld,
    /// Current host version is above `max_host_version`.
    HostVersionTooNew,
}

/// Structured compatibility report returned when evaluating an extension against the host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatibilityReport {
    pub status: CompatibilityStatus,
    pub is_compatible: bool,
    pub host_api: SemVer,
    pub required_api: SemVer,
    pub host_version: SemVer,
    pub deprecation_warnings: Vec<String>,
    pub rejection_reason: Option<String>,
}

/// Check semantic API compatibility between `host_api` and an extension's `required_api`, plus
/// optional `min_host_version` / `max_host_version` constraints against `CURRENT_HOST_VERSION`.
pub fn check_api_compatibility(
    host_api: SemVer,
    required_api: SemVer,
    min_host_version: Option<SemVer>,
    max_host_version: Option<SemVer>,
) -> CompatibilityReport {
    check_api_compatibility_with_host(
        CURRENT_HOST_VERSION,
        host_api,
        required_api,
        min_host_version,
        max_host_version,
    )
}

/// Check semantic API compatibility using an explicit `host_version`.
pub fn check_api_compatibility_with_host(
    host_version: SemVer,
    host_api: SemVer,
    required_api: SemVer,
    min_host_version: Option<SemVer>,
    max_host_version: Option<SemVer>,
) -> CompatibilityReport {
    if let Some(min_v) = min_host_version
        && host_version < min_v
    {
        return CompatibilityReport {
            status: CompatibilityStatus::HostVersionTooOld,
            is_compatible: false,
            host_api,
            required_api,
            host_version,
            deprecation_warnings: Vec::new(),
            rejection_reason: Some(format!(
                "Host version {host_version} is older than required min_host_version {min_v}."
            )),
        };
    }
    if let Some(max_v) = max_host_version
        && host_version > max_v
    {
        return CompatibilityReport {
            status: CompatibilityStatus::HostVersionTooNew,
            is_compatible: false,
            host_api,
            required_api,
            host_version,
            deprecation_warnings: Vec::new(),
            rejection_reason: Some(format!(
                "Host version {host_version} exceeds supported max_host_version {max_v}."
            )),
        };
    }
    if required_api.major != host_api.major {
        return CompatibilityReport {
            status: CompatibilityStatus::IncompatibleApiMajor,
            is_compatible: false,
            host_api,
            required_api,
            host_version,
            deprecation_warnings: Vec::new(),
            rejection_reason: Some(format!(
                "Extension requires API major version {}, but host provides {}.",
                required_api.major, host_api.major
            )),
        };
    }
    if (required_api.minor, required_api.patch) > (host_api.minor, host_api.patch) {
        return CompatibilityReport {
            status: CompatibilityStatus::IncompatibleApiMinorTooNew,
            is_compatible: false,
            host_api,
            required_api,
            host_version,
            deprecation_warnings: Vec::new(),
            rejection_reason: Some(format!(
                "Extension requires API {required_api}, which is newer than host API {host_api}."
            )),
        };
    }

    let mut warnings = Vec::new();
    if required_api.minor < host_api.minor {
        warnings.push(format!(
            "Extension targets older minor API {required_api} on host API {host_api}; backward-compatible shims are active."
        ));
    }
    let status = if warnings.is_empty() {
        CompatibilityStatus::Compatible
    } else {
        CompatibilityStatus::CompatibleWithWarnings
    };
    CompatibilityReport {
        status,
        is_compatible: true,
        host_api,
        required_api,
        host_version,
        deprecation_warnings: warnings,
        rejection_reason: None,
    }
}

/// Execution and resource policy for a sandboxed extension invocation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionSandboxPolicy {
    pub extension_id: String,
    pub granted_capabilities: BTreeSet<ExtensionCapability>,
    pub max_steps: u64,
    pub max_memory_bytes: usize,
    pub max_output_bytes: usize,
}

impl ExtensionSandboxPolicy {
    /// Construct a validated sandbox policy from a verified `ExtensionPackageManifest` using
    /// default CPU/memory/output budgets.
    pub fn from_manifest(manifest: &ExtensionPackageManifest) -> Result<Self, ExtensionError> {
        manifest.validate()?;
        let policy = Self {
            extension_id: manifest.id.clone(),
            granted_capabilities: manifest.capabilities.iter().copied().collect(),
            max_steps: DEFAULT_MAX_SANDBOX_STEPS,
            max_memory_bytes: DEFAULT_MAX_SANDBOX_MEMORY_BYTES,
            max_output_bytes: DEFAULT_MAX_SANDBOX_OUTPUT_BYTES,
        };
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), ExtensionError> {
        if !is_valid_reverse_dns_id(&self.extension_id) {
            return Err(ExtensionError::InvalidExtensionId(
                self.extension_id.clone(),
            ));
        }
        if self.max_steps == 0 || self.max_steps > MAX_ALLOWED_SANDBOX_STEPS {
            return Err(ExtensionError::SandboxPolicyViolation(format!(
                "max_steps ({}) must be in 1..={MAX_ALLOWED_SANDBOX_STEPS}.",
                self.max_steps
            )));
        }
        if self.max_memory_bytes == 0 || self.max_memory_bytes > MAX_ALLOWED_SANDBOX_MEMORY_BYTES {
            return Err(ExtensionError::SandboxPolicyViolation(format!(
                "max_memory_bytes ({}) must be in 1..={MAX_ALLOWED_SANDBOX_MEMORY_BYTES}.",
                self.max_memory_bytes
            )));
        }
        if self.max_output_bytes == 0 || self.max_output_bytes > MAX_ALLOWED_SANDBOX_OUTPUT_BYTES {
            return Err(ExtensionError::SandboxPolicyViolation(format!(
                "max_output_bytes ({}) must be in 1..={MAX_ALLOWED_SANDBOX_OUTPUT_BYTES}.",
                self.max_output_bytes
            )));
        }
        Ok(())
    }

    pub fn has_capability(&self, cap: ExtensionCapability) -> bool {
        self.granted_capabilities.contains(&cap)
    }
}

/// Read-only snapshot of active document and selection context exposed to a sandboxed extension.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionHostContextSnapshot {
    pub document_name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub symbol_count: usize,
    pub active_symbol_id: String,
    pub active_frame_index: u32,
    pub selected_element_ids: Vec<String>,
}

/// Host API call submitted by a running extension to `ExtensionHostInvocationGate`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "call", rename_all = "snake_case")]
pub enum HostApiCall {
    ReadDocumentMetadata,
    ReadSelectedElementIds,
    ProposeCommandTransaction {
        label: String,
        steps: Vec<AuthoringCommandStep>,
    },
    ReadScopedStorage {
        key: String,
    },
    WriteScopedStorage {
        key: String,
        value: Vec<u8>,
    },
    EmitExportArtifact {
        relative_name: String,
        mime_type: String,
        bytes: Vec<u8>,
    },
    AttemptUnsafeOperation {
        forbidden: ForbiddenCapability,
        detail: String,
    },
}

/// Structured response returned by `ExtensionHostInvocationGate::invoke`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostApiResponse {
    DocumentMetadata {
        name: String,
        width: u32,
        height: u32,
        fps: u32,
        symbol_count: usize,
        active_symbol_id: String,
        active_frame_index: u32,
    },
    SelectedElementIds {
        element_ids: Vec<String>,
    },
    TransactionQueued {
        transaction_index: usize,
        step_count: usize,
    },
    ScopedStorageValue {
        key: String,
        value: Option<Vec<u8>>,
    },
    ScopedStorageWritten {
        key: String,
        bytes_written: usize,
    },
    ArtifactEmitted {
        artifact_index: usize,
        relative_name: String,
        byte_len: usize,
    },
}

/// Auditable, undoable document edit transaction proposed by a sandboxed extension.
///
/// Extensions never mutate `Project` directly; the host inspects and commits queued
/// `ProposedDocumentTransaction`s through `Editor::commit_history`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProposedDocumentTransaction {
    pub extension_id: String,
    pub label: String,
    pub steps: Vec<AuthoringCommandStep>,
}

/// Export artifact emitted into memory by a sandboxed extension.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedExtensionArtifact {
    pub extension_id: String,
    pub relative_name: String,
    pub mime_type: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

/// Audit log entry recorded by `ExtensionHostInvocationGate`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxAuditEntry {
    pub call_name: String,
    pub allowed: bool,
    pub steps_charged: u64,
    pub detail: String,
}

/// Host invocation gate that mediates every extension call against its `ExtensionSandboxPolicy`,
/// enforces step/memory/output budgets, blocks all `ForbiddenCapability` requests, and queues
/// document modifications as auditable `ProposedDocumentTransaction`s.
#[derive(Clone, Debug)]
pub struct ExtensionHostInvocationGate {
    policy: ExtensionSandboxPolicy,
    steps_used: u64,
    memory_used_bytes: usize,
    output_used_bytes: usize,
    scoped_storage: BTreeMap<String, Vec<u8>>,
    pending_transactions: Vec<ProposedDocumentTransaction>,
    emitted_artifacts: Vec<ExportedExtensionArtifact>,
    audit_log: Vec<SandboxAuditEntry>,
}

impl ExtensionHostInvocationGate {
    pub fn new(policy: ExtensionSandboxPolicy) -> Result<Self, ExtensionError> {
        policy.validate()?;
        Ok(Self {
            policy,
            steps_used: 0,
            memory_used_bytes: 0,
            output_used_bytes: 0,
            scoped_storage: BTreeMap::new(),
            pending_transactions: Vec::new(),
            emitted_artifacts: Vec::new(),
            audit_log: Vec::new(),
        })
    }

    pub fn policy(&self) -> &ExtensionSandboxPolicy {
        &self.policy
    }

    pub fn steps_used(&self) -> u64 {
        self.steps_used
    }

    pub fn memory_used_bytes(&self) -> usize {
        self.memory_used_bytes
    }

    pub fn output_used_bytes(&self) -> usize {
        self.output_used_bytes
    }

    pub fn pending_transactions(&self) -> &[ProposedDocumentTransaction] {
        &self.pending_transactions
    }

    pub fn take_pending_transactions(&mut self) -> Vec<ProposedDocumentTransaction> {
        std::mem::take(&mut self.pending_transactions)
    }

    pub fn emitted_artifacts(&self) -> &[ExportedExtensionArtifact] {
        &self.emitted_artifacts
    }

    pub fn audit_log(&self) -> &[SandboxAuditEntry] {
        &self.audit_log
    }

    /// Charge execution steps against the sandbox policy budget.
    pub fn charge_steps(&mut self, steps: u64) -> Result<(), ExtensionError> {
        let next = self.steps_used.saturating_add(steps);
        if next > self.policy.max_steps {
            self.audit_log.push(SandboxAuditEntry {
                call_name: "charge_steps".into(),
                allowed: false,
                steps_charged: 0,
                detail: format!(
                    "Step budget exceeded: requested {steps}, used {}/{}",
                    self.steps_used, self.policy.max_steps
                ),
            });
            return Err(ExtensionError::StepBudgetExceeded {
                requested: steps,
                used: self.steps_used,
                limit: self.policy.max_steps,
            });
        }
        self.steps_used = next;
        Ok(())
    }

    /// Charge working memory bytes against the sandbox policy budget.
    pub fn allocate_memory(&mut self, bytes: usize) -> Result<(), ExtensionError> {
        let next = self.memory_used_bytes.saturating_add(bytes);
        if next > self.policy.max_memory_bytes {
            self.audit_log.push(SandboxAuditEntry {
                call_name: "allocate_memory".into(),
                allowed: false,
                steps_charged: 0,
                detail: format!(
                    "Memory budget exceeded: requested {bytes}, used {}/{}",
                    self.memory_used_bytes, self.policy.max_memory_bytes
                ),
            });
            return Err(ExtensionError::MemoryBudgetExceeded {
                requested: bytes,
                used: self.memory_used_bytes,
                limit: self.policy.max_memory_bytes,
            });
        }
        self.memory_used_bytes = next;
        Ok(())
    }

    fn require_capability(
        &mut self,
        call_name: &'static str,
        required: ExtensionCapability,
    ) -> Result<(), ExtensionError> {
        if !self.policy.has_capability(required) {
            self.audit_log.push(SandboxAuditEntry {
                call_name: call_name.into(),
                allowed: false,
                steps_charged: 0,
                detail: format!("Missing required capability {required:?}"),
            });
            return Err(ExtensionError::CapabilityDenied {
                call: call_name,
                required,
            });
        }
        Ok(())
    }

    /// Validate and execute a single `HostApiCall` against the active snapshot and sandbox policy.
    pub fn invoke(
        &mut self,
        snapshot: &ExtensionHostContextSnapshot,
        call: HostApiCall,
    ) -> Result<HostApiResponse, ExtensionError> {
        match call {
            HostApiCall::AttemptUnsafeOperation { forbidden, detail } => {
                self.audit_log.push(SandboxAuditEntry {
                    call_name: "attempt_unsafe_operation".into(),
                    allowed: false,
                    steps_charged: 0,
                    detail: format!("Blocked forbidden capability {forbidden:?}: {detail}"),
                });
                Err(ExtensionError::ForbiddenCapabilityRequested(forbidden))
            }
            HostApiCall::ReadDocumentMetadata => {
                self.require_capability(
                    "read_document_metadata",
                    ExtensionCapability::ReadActiveDocument,
                )?;
                self.charge_steps(1)?;
                self.audit_log.push(SandboxAuditEntry {
                    call_name: "read_document_metadata".into(),
                    allowed: true,
                    steps_charged: 1,
                    detail: format!("Read metadata for '{}'", snapshot.document_name),
                });
                Ok(HostApiResponse::DocumentMetadata {
                    name: snapshot.document_name.clone(),
                    width: snapshot.width,
                    height: snapshot.height,
                    fps: snapshot.fps,
                    symbol_count: snapshot.symbol_count,
                    active_symbol_id: snapshot.active_symbol_id.clone(),
                    active_frame_index: snapshot.active_frame_index,
                })
            }
            HostApiCall::ReadSelectedElementIds => {
                self.require_capability(
                    "read_selected_element_ids",
                    ExtensionCapability::ReadSelection,
                )?;
                self.charge_steps(1)?;
                self.audit_log.push(SandboxAuditEntry {
                    call_name: "read_selected_element_ids".into(),
                    allowed: true,
                    steps_charged: 1,
                    detail: format!("Read {} selected IDs", snapshot.selected_element_ids.len()),
                });
                Ok(HostApiResponse::SelectedElementIds {
                    element_ids: snapshot.selected_element_ids.clone(),
                })
            }
            HostApiCall::ProposeCommandTransaction { label, steps } => {
                self.require_capability(
                    "propose_command_transaction",
                    ExtensionCapability::ProposeDocumentTransaction,
                )?;
                let step_cost = (steps.len() as u64).max(1);
                self.charge_steps(step_cost)?;
                let saved = SavedCommand {
                    name: label.clone(),
                    steps: steps.clone(),
                };
                saved
                    .validate()
                    .map_err(ExtensionError::InvalidTransaction)?;
                let estimated_bytes = label.len() + steps.len() * 64;
                self.allocate_memory(estimated_bytes)?;
                let tx_index = self.pending_transactions.len();
                let step_count = steps.len();
                self.pending_transactions.push(ProposedDocumentTransaction {
                    extension_id: self.policy.extension_id.clone(),
                    label: label.clone(),
                    steps,
                });
                self.audit_log.push(SandboxAuditEntry {
                    call_name: "propose_command_transaction".into(),
                    allowed: true,
                    steps_charged: step_cost,
                    detail: format!("Queued transaction '{label}' ({step_count} steps)"),
                });
                Ok(HostApiResponse::TransactionQueued {
                    transaction_index: tx_index,
                    step_count,
                })
            }
            HostApiCall::ReadScopedStorage { key } => {
                self.require_capability(
                    "read_scoped_storage",
                    ExtensionCapability::LocalStorageScoped,
                )?;
                self.charge_steps(1)?;
                if key.trim().is_empty() || key.len() > 128 {
                    return Err(ExtensionError::SandboxPolicyViolation(
                        "Scoped storage key must be 1..=128 characters.".into(),
                    ));
                }
                let value = self.scoped_storage.get(&key).cloned();
                self.audit_log.push(SandboxAuditEntry {
                    call_name: "read_scoped_storage".into(),
                    allowed: true,
                    steps_charged: 1,
                    detail: format!("Read key '{key}'"),
                });
                Ok(HostApiResponse::ScopedStorageValue { key, value })
            }
            HostApiCall::WriteScopedStorage { key, value } => {
                self.require_capability(
                    "write_scoped_storage",
                    ExtensionCapability::LocalStorageScoped,
                )?;
                self.charge_steps(1)?;
                if key.trim().is_empty() || key.len() > 128 {
                    return Err(ExtensionError::SandboxPolicyViolation(
                        "Scoped storage key must be 1..=128 characters.".into(),
                    ));
                }
                let previous_len = self.scoped_storage.get(&key).map_or(0, Vec::len);
                let additional_bytes = value.len().saturating_sub(previous_len);
                if additional_bytes > 0 {
                    self.allocate_memory(additional_bytes)?;
                }
                let bytes_written = value.len();
                self.scoped_storage.insert(key.clone(), value);
                self.audit_log.push(SandboxAuditEntry {
                    call_name: "write_scoped_storage".into(),
                    allowed: true,
                    steps_charged: 1,
                    detail: format!("Wrote key '{key}' ({bytes_written} bytes)"),
                });
                Ok(HostApiResponse::ScopedStorageWritten { key, bytes_written })
            }
            HostApiCall::EmitExportArtifact {
                relative_name,
                mime_type,
                bytes,
            } => {
                self.require_capability(
                    "emit_export_artifact",
                    ExtensionCapability::ExportArtifact,
                )?;
                self.charge_steps(2)?;
                if !is_safe_archive_relative_path(&relative_name) {
                    return Err(ExtensionError::SandboxPolicyViolation(format!(
                        "Unsafe export artifact relative_name '{relative_name}'."
                    )));
                }
                if mime_type.trim().is_empty() || mime_type.len() > 128 {
                    return Err(ExtensionError::SandboxPolicyViolation(
                        "Artifact mime_type must be 1..=128 characters.".into(),
                    ));
                }
                let next_output = self.output_used_bytes.saturating_add(bytes.len());
                if next_output > self.policy.max_output_bytes {
                    self.audit_log.push(SandboxAuditEntry {
                        call_name: "emit_export_artifact".into(),
                        allowed: false,
                        steps_charged: 2,
                        detail: format!(
                            "Output budget exceeded: requested {}, used {}/{}",
                            bytes.len(),
                            self.output_used_bytes,
                            self.policy.max_output_bytes
                        ),
                    });
                    return Err(ExtensionError::OutputBudgetExceeded {
                        requested: bytes.len(),
                        used: self.output_used_bytes,
                        limit: self.policy.max_output_bytes,
                    });
                }
                self.output_used_bytes = next_output;
                let byte_len = bytes.len();
                let sha256 = sha256_hex(&bytes);
                let artifact_index = self.emitted_artifacts.len();
                self.emitted_artifacts.push(ExportedExtensionArtifact {
                    extension_id: self.policy.extension_id.clone(),
                    relative_name: relative_name.clone(),
                    mime_type,
                    sha256,
                    bytes,
                });
                self.audit_log.push(SandboxAuditEntry {
                    call_name: "emit_export_artifact".into(),
                    allowed: true,
                    steps_charged: 2,
                    detail: format!("Emitted artifact '{relative_name}' ({byte_len} bytes)"),
                });
                Ok(HostApiResponse::ArtifactEmitted {
                    artifact_index,
                    relative_name,
                    byte_len,
                })
            }
        }
    }
}

/// Compute a lowercase hexadecimal FIPS 180-4 SHA-256 digest of `data`.
pub fn sha256_hex(data: &[u8]) -> String {
    let digest = sha256_digest(data);
    let mut out = String::with_capacity(64);
    for byte in digest {
        use fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

/// Compute a 32-byte FIPS 180-4 SHA-256 digest of `data`.
pub fn sha256_digest(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity((data.len() + 9).div_ceil(64) * 64);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() + 8) % 64 != 0 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word_bytes) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word_bytes);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }

    let mut out = [0u8; 32];
    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> ExtensionPackageManifest {
        ExtensionPackageManifest {
            id: "org.neoflash.pixel_snap".into(),
            name: "Pixel Snap Command Pack".into(),
            version: SemVer::new(1, 2, 0),
            api_version: SemVer::new(1, 0, 0),
            author: "NeoFlash Team".into(),
            description: "Snaps selected elements by whole pixels.".into(),
            entry_kind: ExtensionEntryKind::DeclarativeCommandPack,
            min_host_version: Some(SemVer::new(1, 0, 0)),
            max_host_version: None,
            capabilities: vec![
                ExtensionCapability::ReadActiveDocument,
                ExtensionCapability::ReadSelection,
                ExtensionCapability::ProposeDocumentTransaction,
            ],
            contributes: ExtensionContributions {
                commands: vec![ContributedCommand {
                    id: "snap_nudge".into(),
                    title: "Nudge Right 10px".into(),
                    steps: Some(vec![AuthoringCommandStep::Translate { dx: 10.0, dy: 0.0 }]),
                    script_entry: Some("scripts/nudge.jsfl".into()),
                }],
                menu_items: vec![ContributedMenuItem {
                    command_id: "snap_nudge".into(),
                    menu_path: "Commands/Pixel Snap".into(),
                    order: 10,
                }],
                keyboard_shortcuts: vec![ContributedShortcut {
                    command_id: "snap_nudge".into(),
                    key_chord: "Cmd+Shift+P".into(),
                }],
                import_export_filters: Vec::new(),
            },
            integrity_sha256: sha256_hex(&[]),
        }
    }

    #[test]
    fn sha256_matches_nist_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn reverse_dns_validation_accepts_canonical_ids_and_rejects_malformed() {
        assert!(is_valid_reverse_dns_id("org.example.tool"));
        assert!(is_valid_reverse_dns_id("com.neoflash.stage_helper_2"));
        assert!(!is_valid_reverse_dns_id("invalid"));
        assert!(!is_valid_reverse_dns_id("org.example"));
        assert!(!is_valid_reverse_dns_id("Org.Example.Tool"));
        assert!(!is_valid_reverse_dns_id("org..tool"));
        assert!(!is_valid_reverse_dns_id("org.1example.tool"));
    }

    #[test]
    fn manifest_round_trip_and_archive_seal_verification() {
        let manifest = sample_manifest();
        let entry = ExtensionArchiveEntry::from_utf8(
            "scripts/nudge.jsfl",
            "fl.getDocumentDOM().moveSelectionBy({x: 10, y: 0});",
        );
        let archive =
            ExtensionPackageArchive::seal(manifest, vec![entry]).expect("seal should succeed");
        let serialized = archive.to_nfext_json().expect("serialize nfext");
        let restored =
            ExtensionPackageArchive::from_nfext_json(&serialized).expect("deserialize nfext");
        assert_eq!(archive, restored);

        // Tamper with payload and verify SHA-256 rejection
        let mut tampered = restored.clone();
        tampered.entries[0].contents_utf8.push_str("// tampered");
        let err = tampered.verify().expect_err("tampered payload must fail");
        assert!(matches!(err, ExtensionError::ArchiveIntegrity(_)));
    }

    #[test]
    fn semver_parsing_ordering_and_compatibility_matrix() {
        let v1_0_0 = SemVer::parse("1.0.0").unwrap();
        let v1_2_0 = SemVer::parse("1.2.0").unwrap();
        let v1_3_1 = SemVer::parse("1.3.1").unwrap();
        let v2_0_0 = SemVer::parse("2.0.0").unwrap();
        assert!(v1_0_0 < v1_2_0 && v1_2_0 < v1_3_1 && v1_3_1 < v2_0_0);
        assert!(SemVer::parse("01.0.0").is_err());
        assert!(SemVer::parse("1.0").is_err());

        // Exact match -> Compatible
        let exact = check_api_compatibility_with_host(v1_2_0, v1_2_0, v1_2_0, Some(v1_0_0), None);
        assert_eq!(exact.status, CompatibilityStatus::Compatible);
        assert!(exact.is_compatible);
        assert!(exact.deprecation_warnings.is_empty());

        // Older minor required on newer minor host -> CompatibleWithWarnings
        let older_minor =
            check_api_compatibility_with_host(v1_2_0, v1_3_1, v1_0_0, Some(v1_0_0), None);
        assert_eq!(
            older_minor.status,
            CompatibilityStatus::CompatibleWithWarnings
        );
        assert!(older_minor.is_compatible);
        assert_eq!(older_minor.deprecation_warnings.len(), 1);

        // Newer minor required than host supports -> IncompatibleApiMinorTooNew
        let newer_minor = check_api_compatibility_with_host(v1_2_0, v1_2_0, v1_3_1, None, None);
        assert_eq!(
            newer_minor.status,
            CompatibilityStatus::IncompatibleApiMinorTooNew
        );
        assert!(!newer_minor.is_compatible);

        // Major mismatch -> IncompatibleApiMajor
        let major_mismatch = check_api_compatibility_with_host(v1_2_0, v1_2_0, v2_0_0, None, None);
        assert_eq!(
            major_mismatch.status,
            CompatibilityStatus::IncompatibleApiMajor
        );
        assert!(!major_mismatch.is_compatible);

        // Host version bounds
        let host_too_old =
            check_api_compatibility_with_host(v1_0_0, v1_2_0, v1_0_0, Some(v1_2_0), None);
        assert_eq!(host_too_old.status, CompatibilityStatus::HostVersionTooOld);
        let host_too_new =
            check_api_compatibility_with_host(v2_0_0, v1_2_0, v1_0_0, None, Some(v1_3_1));
        assert_eq!(host_too_new.status, CompatibilityStatus::HostVersionTooNew);
    }

    #[test]
    fn sandbox_gate_enforces_capabilities_budgets_and_denies_unsafe_calls() {
        // Requesting a forbidden capability in JSON must fail explicitly
        let raw_bad = r#"{
            "id": "org.example.bad",
            "name": "Bad Extension",
            "version": "1.0.0",
            "api_version": "1.0.0",
            "author": "Test",
            "description": "",
            "entry_kind": "declarative_command_pack",
            "capabilities": ["arbitrary_filesystem_access"],
            "contributes": {},
            "integrity_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        }"#;
        let err = ExtensionPackageManifest::from_manifest_json(raw_bad)
            .expect_err("must reject arbitrary_filesystem_access");
        assert_eq!(
            err,
            ExtensionError::ForbiddenCapabilityRequested(
                ForbiddenCapability::ArbitraryFilesystemAccess
            )
        );

        let mut policy = ExtensionSandboxPolicy::from_manifest(&sample_manifest()).unwrap();
        policy.max_steps = 5;
        policy.max_output_bytes = 64;
        let mut gate = ExtensionHostInvocationGate::new(policy).unwrap();
        let snapshot = ExtensionHostContextSnapshot {
            document_name: "Demo.nflash".into(),
            width: 800,
            height: 600,
            fps: 24,
            symbol_count: 3,
            active_symbol_id: "sym_root".into(),
            active_frame_index: 0,
            selected_element_ids: vec!["el_1".into()],
        };

        // Allowed read
        let meta = gate
            .invoke(&snapshot, HostApiCall::ReadDocumentMetadata)
            .expect("ReadDocumentMetadata allowed");
        assert!(matches!(meta, HostApiResponse::DocumentMetadata { .. }));

        // Propose transaction queues auditable transaction without mutating host directly
        let tx = gate
            .invoke(
                &snapshot,
                HostApiCall::ProposeCommandTransaction {
                    label: "Move 10px".into(),
                    steps: vec![AuthoringCommandStep::Translate { dx: 10.0, dy: 0.0 }],
                },
            )
            .expect("ProposeCommandTransaction allowed");
        assert_eq!(
            tx,
            HostApiResponse::TransactionQueued {
                transaction_index: 0,
                step_count: 1
            }
        );
        assert_eq!(gate.pending_transactions().len(), 1);

        // Capability not declared in manifest (ExportArtifact) is denied
        let export_denied = gate
            .invoke(
                &snapshot,
                HostApiCall::EmitExportArtifact {
                    relative_name: "out.json".into(),
                    mime_type: "application/json".into(),
                    bytes: b"{}".to_vec(),
                },
            )
            .expect_err("ExportArtifact not granted");
        assert_eq!(
            export_denied,
            ExtensionError::CapabilityDenied {
                call: "emit_export_artifact",
                required: ExtensionCapability::ExportArtifact,
            }
        );

        // Unsafe operation is unconditionally blocked
        let unsafe_err = gate
            .invoke(
                &snapshot,
                HostApiCall::AttemptUnsafeOperation {
                    forbidden: ForbiddenCapability::NativeProcessSpawn,
                    detail: "/bin/sh".into(),
                },
            )
            .expect_err("unsafe operation must fail");
        assert_eq!(
            unsafe_err,
            ExtensionError::ForbiddenCapabilityRequested(ForbiddenCapability::NativeProcessSpawn)
        );

        // Step budget exhaustion
        let budget_err = gate.charge_steps(10).expect_err("must exceed step budget");
        assert!(matches!(
            budget_err,
            ExtensionError::StepBudgetExceeded { .. }
        ));
    }
}
