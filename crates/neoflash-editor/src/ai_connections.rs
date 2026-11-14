//! Provider-neutral, secure AI connection foundation (`T-219`).
//!
//! Architectural boundaries:
//! - **No autonomous edits**: AI responses are parsed strictly into untrusted value objects
//!   ([`AiUntrustedProposal`]). No method in this module mutates [`neoflash_core::Project`],
//!   executes JavaScript/JSFL, or invokes filesystem/shell tools on behalf of model output.
//! - **Zero secret persistence in documents or workspace prefs**: API keys and tokens live only in
//!   [`SecretStore`] implementations ([`KeychainSecretStore`], [`EnvSecretStore`],
//!   [`MemorySecretStore`]). [`AiProviderProfile`] uses `#[serde(deny_unknown_fields)]` and never
//!   contains secret fields.
//! - **Deterministic, bounded, redacted context**: [`AiContextPack`] captures only explicit
//!   user-selected objects, active frame index, a UTF-8 char-boundary-truncated script excerpt
//!   (`<= MAX_SCRIPT_BYTES`), and MIME-whitelisted asset metadata (`<= MAX_ASSET_BYTES`), with
//!   automatic token/key redaction ([`redact_sensitive_tokens`]).

use neoflash_core::{ElementKind, Project, ScriptLanguage};
use neoflash_export::ai_http::{
    AiEndpointSecurity, AiHttpError, ValidatedAiEndpoint, send_ai_get, send_ai_post_json,
    stream_ai_post_lines, validate_ai_base_url,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
#[cfg(test)]
use std::io::{BufRead, BufReader, Read};
#[cfg(test)]
use std::net::{IpAddr, SocketAddr};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
#[cfg(test)]
use url::Host;
use url::Url;

/// Maximum UTF-8 byte length of a script excerpt included in an [`AiContextPack`].
pub const MAX_SCRIPT_BYTES: usize = 16_384;

/// Maximum decoded byte size of an asset whose metadata can be marked eligible in an [`AiContextPack`].
pub const MAX_ASSET_BYTES: usize = 65_536;

/// Maximum accepted provider response body. This applies after transport decoding and prevents a
/// misbehaving local proxy from growing editor memory without bound.
pub const MAX_AI_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// Maximum diagnostic body retained from a failed provider response.
pub const MAX_AI_ERROR_BYTES: usize = 64 * 1024;

/// Maximum HTTP status/header/SSE line length accepted from a provider.
pub const MAX_AI_HTTP_LINE_BYTES: usize = 1024 * 1024;

/// Explicit MIME whitelist for asset inspection metadata in [`AiContextPack`].
pub const ALLOWED_ASSET_MIME_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/svg+xml",
    "audio/mpeg",
    "audio/wav",
    "text/plain",
];

/// macOS Keychain service name used by [`KeychainSecretStore`].
pub const KEYCHAIN_SERVICE_NAME: &str = "com.neoflash.ai-connections";

/// Supported AI provider families.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProviderKind {
    OpenAi,
    Anthropic,
    Gemini,
    LocalOpenAiCompatible,
}

impl AiProviderKind {
    pub const ALL: [Self; 4] = [
        Self::OpenAi,
        Self::Anthropic,
        Self::Gemini,
        Self::LocalOpenAiCompatible,
    ];

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Gemini => "Google Gemini",
            Self::LocalOpenAiCompatible => "Local (OpenAI-compatible)",
        }
    }

    pub const fn default_base_url(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com",
            Self::Anthropic => "https://api.anthropic.com",
            Self::Gemini => "https://generativelanguage.googleapis.com",
            Self::LocalOpenAiCompatible => "http://127.0.0.1:11434",
        }
    }

    pub const fn default_model_hint(self) -> &'static str {
        match self {
            Self::OpenAi => "gpt-4o",
            Self::Anthropic => "claude-sonnet-4-6",
            Self::Gemini => "gemini-2.5-flash",
            Self::LocalOpenAiCompatible => "llama3.1",
        }
    }

    pub const fn requires_secret(self) -> bool {
        match self {
            Self::OpenAi | Self::Anthropic | Self::Gemini => true,
            Self::LocalOpenAiCompatible => false,
        }
    }

    pub const fn endpoint_security(self) -> AiEndpointSecurity {
        match self {
            Self::OpenAi | Self::Anthropic | Self::Gemini => AiEndpointSecurity::HttpsRequired,
            Self::LocalOpenAiCompatible => AiEndpointSecurity::HttpsOrLoopbackHttp,
        }
    }
}

/// Declared capabilities of a configured provider profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiProviderCapabilities {
    pub streaming: bool,
    pub vision_input: bool,
    pub structured_json_proposal: bool,
}

impl Default for AiProviderCapabilities {
    fn default() -> Self {
        Self {
            streaming: true,
            vision_input: false,
            structured_json_proposal: true,
        }
    }
}

/// Non-secret provider connection profile safe for serialization in preferences.
///
/// `#[serde(deny_unknown_fields)]` guarantees that JSON containing stray `"api_key"`, `"secret"`,
/// or `"token"` fields is rejected at deserialization time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiProviderProfile {
    pub id: String,
    pub display_name: String,
    pub kind: AiProviderKind,
    pub base_url: String,
    pub default_model: String,
    pub timeout_ms: u64,
    pub max_retries: u32,
    pub capabilities: AiProviderCapabilities,
}

impl AiProviderProfile {
    pub fn new(
        id: impl Into<String>,
        display_name: impl Into<String>,
        kind: AiProviderKind,
        base_url: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            display_name: display_name.into(),
            kind,
            base_url: base_url.into(),
            default_model: default_model.into(),
            timeout_ms: 15_000,
            max_retries: 2,
            capabilities: AiProviderCapabilities::default(),
        }
    }
}

/// Errors produced by secret storage, request formatting, or transport.
///
/// Any embedded message is automatically scrubbed with [`redact_sensitive_tokens`] before display.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AiConnectionError {
    MissingSecret {
        profile_id: String,
    },
    InvalidEndpoint {
        reason: String,
    },
    Offline {
        reason: String,
    },
    Timeout {
        timeout_ms: u64,
    },
    RateLimited {
        retry_after_ms: Option<u64>,
        message: String,
    },
    HttpStatus {
        status: u16,
        message: String,
    },
    Protocol {
        message: String,
    },
    SecretStore {
        message: String,
    },
    Cancelled,
}

impl fmt::Debug for AiConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AiConnectionError({})",
            redact_sensitive_tokens(&self.to_string())
        )
    }
}

impl fmt::Display for AiConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let raw = match self {
            Self::MissingSecret { profile_id } => {
                format!("No API credential configured for profile '{profile_id}'.")
            }
            Self::InvalidEndpoint { reason } => format!("Invalid endpoint: {reason}"),
            Self::Offline { reason } => format!("Provider unreachable (offline): {reason}"),
            Self::Timeout { timeout_ms } => {
                format!("Connection timed out after {timeout_ms} ms.")
            }
            Self::RateLimited {
                retry_after_ms,
                message,
            } => match retry_after_ms {
                Some(ms) => format!("Rate limited (retry after {ms} ms): {message}"),
                None => format!("Rate limited: {message}"),
            },
            Self::HttpStatus { status, message } => {
                format!("HTTP {status}: {message}")
            }
            Self::Protocol { message } => format!("Protocol error: {message}"),
            Self::SecretStore { message } => format!("Secret store error: {message}"),
            Self::Cancelled => "Request cancelled.".to_owned(),
        };
        f.write_str(&redact_sensitive_tokens(&raw))
    }
}

impl std::error::Error for AiConnectionError {}

/// Credential store interface. Implementations keep secrets out of `Project` and `WorkspacePrefs`.
pub trait SecretStore: Send + Sync {
    fn get_secret(&self, profile_id: &str) -> Result<Option<String>, AiConnectionError>;
    fn set_secret(&self, profile_id: &str, secret: &str) -> Result<(), AiConnectionError>;
    fn delete_secret(&self, profile_id: &str) -> Result<(), AiConnectionError>;
}

/// Thread-safe in-memory secret store for tests and ephemeral sessions.
#[derive(Clone, Default)]
pub struct MemorySecretStore {
    secrets: Arc<Mutex<BTreeMap<String, String>>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl fmt::Debug for MemorySecretStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self.secrets.lock().map_or(0, |m| m.len());
        f.debug_struct("MemorySecretStore")
            .field("stored_profiles", &count)
            .field("secrets", &"[REDACTED]")
            .finish()
    }
}

impl SecretStore for MemorySecretStore {
    fn get_secret(&self, profile_id: &str) -> Result<Option<String>, AiConnectionError> {
        let guard = self
            .secrets
            .lock()
            .map_err(|_| AiConnectionError::SecretStore {
                message: "Secret store lock poisoned".into(),
            })?;
        Ok(guard.get(profile_id).cloned())
    }

    fn set_secret(&self, profile_id: &str, secret: &str) -> Result<(), AiConnectionError> {
        let trimmed = secret.trim();
        if trimmed.is_empty() {
            return self.delete_secret(profile_id);
        }
        let mut guard = self
            .secrets
            .lock()
            .map_err(|_| AiConnectionError::SecretStore {
                message: "Secret store lock poisoned".into(),
            })?;
        guard.insert(profile_id.to_owned(), trimmed.to_owned());
        Ok(())
    }

    fn delete_secret(&self, profile_id: &str) -> Result<(), AiConnectionError> {
        let mut guard = self
            .secrets
            .lock()
            .map_err(|_| AiConnectionError::SecretStore {
                message: "Secret store lock poisoned".into(),
            })?;
        guard.remove(profile_id);
        Ok(())
    }
}

/// Environment-variable secret store reading `NEOFLASH_AI_TOKEN_<PROFILE_ID>`.
#[derive(Clone, Debug, Default)]
pub struct EnvSecretStore {
    prefix: String,
}

impl EnvSecretStore {
    pub fn new() -> Self {
        Self {
            prefix: "NEOFLASH_AI_TOKEN_".into(),
        }
    }

    pub fn env_key_for(&self, profile_id: &str) -> String {
        let sanitized: String = profile_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect();
        format!("{}{sanitized}", self.prefix)
    }
}

impl SecretStore for EnvSecretStore {
    fn get_secret(&self, profile_id: &str) -> Result<Option<String>, AiConnectionError> {
        let key = self.env_key_for(profile_id);
        match std::env::var(&key) {
            Ok(val) if !val.trim().is_empty() => Ok(Some(val.trim().to_owned())),
            _ => Ok(None),
        }
    }

    fn set_secret(&self, _profile_id: &str, _secret: &str) -> Result<(), AiConnectionError> {
        Err(AiConnectionError::SecretStore {
            message: "Environment secret store is read-only; set the environment variable before launching or use KeychainSecretStore.".into(),
        })
    }

    fn delete_secret(&self, _profile_id: &str) -> Result<(), AiConnectionError> {
        Err(AiConnectionError::SecretStore {
            message: "Environment secret store is read-only.".into(),
        })
    }
}

/// macOS Keychain secret store backed by `/usr/bin/security` generic-password items
/// scoped to [`KEYCHAIN_SERVICE_NAME`].
#[derive(Clone, Debug)]
pub struct KeychainSecretStore {
    service: String,
}

impl Default for KeychainSecretStore {
    fn default() -> Self {
        Self {
            service: KEYCHAIN_SERVICE_NAME.to_owned(),
        }
    }
}

impl KeychainSecretStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
}

impl SecretStore for KeychainSecretStore {
    fn get_secret(&self, profile_id: &str) -> Result<Option<String>, AiConnectionError> {
        let output = std::process::Command::new("/usr/bin/security")
            .args([
                "find-generic-password",
                "-s",
                &self.service,
                "-a",
                profile_id,
                "-w",
            ])
            .output()
            .map_err(|e| AiConnectionError::SecretStore {
                message: format!("Failed to invoke /usr/bin/security: {e}"),
            })?;
        if output.status.success() {
            let secret = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            Ok((!secret.is_empty()).then_some(secret))
        } else if output.status.code() == Some(44) {
            // Exit code 44 in `/usr/bin/security` indicates item not found.
            Ok(None)
        } else {
            Err(AiConnectionError::SecretStore {
                message: format!(
                    "macOS Keychain lookup failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            })
        }
    }

    fn set_secret(&self, profile_id: &str, secret: &str) -> Result<(), AiConnectionError> {
        let trimmed = secret.trim();
        if trimmed.is_empty() {
            return self.delete_secret(profile_id);
        }
        let output = std::process::Command::new("/usr/bin/security")
            .args([
                "add-generic-password",
                "-U",
                "-s",
                &self.service,
                "-a",
                profile_id,
                "-w",
                trimmed,
            ])
            .output()
            .map_err(|e| AiConnectionError::SecretStore {
                message: format!("Failed to invoke /usr/bin/security: {e}"),
            })?;
        if output.status.success() {
            Ok(())
        } else {
            Err(AiConnectionError::SecretStore {
                message: "macOS Keychain rejected add-generic-password".into(),
            })
        }
    }

    fn delete_secret(&self, profile_id: &str) -> Result<(), AiConnectionError> {
        let output = std::process::Command::new("/usr/bin/security")
            .args([
                "delete-generic-password",
                "-s",
                &self.service,
                "-a",
                profile_id,
            ])
            .output()
            .map_err(|error| AiConnectionError::SecretStore {
                message: format!("Failed to invoke /usr/bin/security: {error}"),
            })?;
        if output.status.success() || output.status.code() == Some(44) {
            Ok(())
        } else {
            Err(AiConnectionError::SecretStore {
                message: format!(
                    "macOS Keychain delete failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            })
        }
    }
}

/// Platform default secret store: macOS Keychain (`com.neoflash.ai-connections`) with read-only
/// fallback to `NEOFLASH_AI_TOKEN_<PROFILE_ID>` environment variables; non-macOS uses session
/// memory plus environment fallback.
#[derive(Debug)]
pub struct SystemSecretStore {
    #[cfg(target_os = "macos")]
    primary: KeychainSecretStore,
    #[cfg(not(target_os = "macos"))]
    primary: MemorySecretStore,
    env_fallback: EnvSecretStore,
}

impl Default for SystemSecretStore {
    fn default() -> Self {
        Self {
            #[cfg(target_os = "macos")]
            primary: KeychainSecretStore::default(),
            #[cfg(not(target_os = "macos"))]
            primary: MemorySecretStore::new(),
            env_fallback: EnvSecretStore::new(),
        }
    }
}

impl SecretStore for SystemSecretStore {
    fn get_secret(&self, profile_id: &str) -> Result<Option<String>, AiConnectionError> {
        if let Some(secret) = self.primary.get_secret(profile_id)? {
            return Ok(Some(secret));
        }
        self.env_fallback.get_secret(profile_id)
    }

    fn set_secret(&self, profile_id: &str, secret: &str) -> Result<(), AiConnectionError> {
        self.primary.set_secret(profile_id, secret)
    }

    fn delete_secret(&self, profile_id: &str) -> Result<(), AiConnectionError> {
        self.primary.delete_secret(profile_id)
    }
}

/// Status badge state for an AI connection profile (`R-097`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiConnectionStatus {
    Ready,
    NotTested,
    NoKey,
    Failed,
}

impl AiConnectionStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::NotTested => "Not tested",
            Self::NoKey => "No key",
            Self::Failed => "Failed",
        }
    }
}

/// Ephemeral last-test outcome recorded by [`AiConnectionManager`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AiProfileTestState {
    #[default]
    Untested,
    Connected {
        model: String,
    },
    Failed {
        message: String,
    },
}

/// Non-secret AI preferences persisted inside `WorkspacePrefs`.
///
/// `#[serde(deny_unknown_fields)]` ensures stray credential keys in `preferences.json` are rejected.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AiPrefs {
    pub profiles: Vec<AiProviderProfile>,
    pub default_profile_id: Option<String>,
}

impl AiPrefs {
    pub fn sanitize(&mut self) {
        let mut seen_ids = std::collections::BTreeSet::new();
        self.profiles.retain(|p| {
            let clean = !p.id.trim().is_empty();
            clean && seen_ids.insert(p.id.clone())
        });
        for profile in &mut self.profiles {
            profile.timeout_ms = profile.timeout_ms.clamp(1_000, 300_000);
            profile.max_retries = profile.max_retries.min(5);
            if profile.display_name.trim().is_empty() {
                profile.display_name = profile.kind.display_name().to_owned();
            }
            if profile.base_url.trim().is_empty() {
                profile.base_url = profile.kind.default_base_url().to_owned();
            }
            if profile.default_model.trim().is_empty() {
                profile.default_model = profile.kind.default_model_hint().to_owned();
            }
        }
        let default_valid = self
            .default_profile_id
            .as_deref()
            .is_some_and(|id| self.profiles.iter().any(|p| p.id == id));
        if !default_valid {
            self.default_profile_id = self.profiles.first().map(|p| p.id.clone());
        }
    }

    pub fn profile(&self, id: &str) -> Option<&AiProviderProfile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn upsert_profile(&mut self, profile: AiProviderProfile) {
        if let Some(existing) = self.profiles.iter_mut().find(|p| p.id == profile.id) {
            *existing = profile;
        } else {
            if self.default_profile_id.is_none() {
                self.default_profile_id = Some(profile.id.clone());
            }
            self.profiles.push(profile);
        }
        self.sanitize();
    }

    pub fn remove_profile(&mut self, id: &str) -> bool {
        let before = self.profiles.len();
        self.profiles.retain(|p| p.id != id);
        let removed = self.profiles.len() != before;
        self.sanitize();
        removed
    }

    pub fn next_profile_id(&self, kind: AiProviderKind) -> String {
        let prefix = match kind {
            AiProviderKind::OpenAi => "openai",
            AiProviderKind::Anthropic => "anthropic",
            AiProviderKind::Gemini => "gemini",
            AiProviderKind::LocalOpenAiCompatible => "local",
        };
        for idx in 1..=9_999u32 {
            let candidate = format!("{prefix}-{idx}");
            if !self.profiles.iter().any(|p| p.id == candidate) {
                return candidate;
            }
        }
        format!("{prefix}-custom")
    }
}

/// Editor-wide AI connection coordinator owning the [`SecretStore`] and runtime connection test
/// status without ever exposing raw credentials to `WorkspacePrefs` or `.nflash` documents.
pub struct AiConnectionManager {
    secret_store: Arc<dyn SecretStore>,
    test_states: BTreeMap<String, AiProfileTestState>,
}

impl fmt::Debug for AiConnectionManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AiConnectionManager")
            .field("tracked_profiles", &self.test_states.len())
            .field("secret_store", &"[REDACTED]")
            .finish()
    }
}

impl AiConnectionManager {
    pub fn new(secret_store: Arc<dyn SecretStore>) -> Self {
        Self {
            secret_store,
            test_states: BTreeMap::new(),
        }
    }

    pub fn in_memory() -> Self {
        Self::new(Arc::new(MemorySecretStore::new()))
    }

    pub fn system_default() -> Self {
        Self::new(Arc::new(SystemSecretStore::default()))
    }

    pub fn secret_store(&self) -> Arc<dyn SecretStore> {
        Arc::clone(&self.secret_store)
    }

    pub fn has_secret(&self, profile_id: &str) -> bool {
        self.secret_store
            .get_secret(profile_id)
            .ok()
            .flatten()
            .is_some_and(|s| !s.trim().is_empty())
    }

    pub fn set_secret(&mut self, profile_id: &str, secret: &str) -> Result<(), AiConnectionError> {
        self.secret_store.set_secret(profile_id, secret)?;
        self.test_states.remove(profile_id);
        Ok(())
    }

    pub fn delete_secret(&mut self, profile_id: &str) -> Result<(), AiConnectionError> {
        let res = self.secret_store.delete_secret(profile_id);
        self.test_states.remove(profile_id);
        res
    }

    pub fn clear_test_state(&mut self, profile_id: &str) {
        self.test_states.remove(profile_id);
    }

    pub fn set_test_state(&mut self, profile_id: impl Into<String>, state: AiProfileTestState) {
        self.test_states.insert(profile_id.into(), state);
    }

    pub fn test_state(&self, profile_id: &str) -> AiProfileTestState {
        self.test_states
            .get(profile_id)
            .cloned()
            .unwrap_or(AiProfileTestState::Untested)
    }

    pub fn profile_status(&self, profile: &AiProviderProfile) -> AiConnectionStatus {
        if profile.kind.requires_secret() && !self.has_secret(&profile.id) {
            return AiConnectionStatus::NoKey;
        }
        match self.test_states.get(&profile.id) {
            Some(AiProfileTestState::Connected { .. }) => AiConnectionStatus::Ready,
            Some(AiProfileTestState::Failed { .. }) => AiConnectionStatus::Failed,
            None | Some(AiProfileTestState::Untested) => AiConnectionStatus::NotTested,
        }
    }

    pub fn test_profile(
        &mut self,
        profile: &AiProviderProfile,
    ) -> Result<String, AiConnectionError> {
        let res = test_provider_connection(profile, self.secret_store.as_ref());
        match &res {
            Ok(_) => {
                self.test_states.insert(
                    profile.id.clone(),
                    AiProfileTestState::Connected {
                        model: profile.default_model.clone(),
                    },
                );
            }
            Err(err) => {
                self.test_states.insert(
                    profile.id.clone(),
                    AiProfileTestState::Failed {
                        message: redact_sensitive_tokens(&err.to_string()),
                    },
                );
            }
        }
        res
    }

    pub fn has_ready_connection(&self, prefs: &AiPrefs) -> bool {
        prefs
            .profiles
            .iter()
            .any(|p| self.profile_status(p) == AiConnectionStatus::Ready)
    }

    pub fn ready_profiles<'a>(&self, prefs: &'a AiPrefs) -> Vec<&'a AiProviderProfile> {
        prefs
            .profiles
            .iter()
            .filter(|p| self.profile_status(p) == AiConnectionStatus::Ready)
            .collect()
    }

    pub fn default_ready_profile<'a>(&self, prefs: &'a AiPrefs) -> Option<&'a AiProviderProfile> {
        if let Some(default_id) = prefs.default_profile_id.as_deref()
            && let Some(profile) = prefs.profile(default_id)
            && self.profile_status(profile) == AiConnectionStatus::Ready
        {
            return Some(profile);
        }
        self.ready_profiles(prefs).into_iter().next()
    }

    pub fn spawn_request(
        &self,
        profile: AiProviderProfile,
        request: AiPromptRequest,
    ) -> AiConnectionWorker {
        AiConnectionWorker::spawn(profile, request, Arc::clone(&self.secret_store))
    }
}

/// Redact API keys, Bearer tokens, and key=value secret patterns from arbitrary text before it
/// can enter logs, UI diagnostics, or context packs.
pub fn redact_sensitive_tokens(input: &str) -> String {
    let mut out = input.to_owned();

    // 1. Redact `Bearer <token>` (case-insensitive header/inline prefix)
    out = redact_after_prefix(&out, "Bearer ");
    out = redact_after_prefix(&out, "bearer ");

    // 2. Redact `x-api-key:`, `x-goog-api-key:`, `api_key=`, `apikey=`, `access_token=`, `secret=`
    for prefix in [
        "x-api-key:",
        "X-Api-Key:",
        "x-goog-api-key:",
        "X-Goog-Api-Key:",
        "api_key=",
        "apikey=",
        "api-key=",
        "access_token=",
        "secret_key=",
    ] {
        out = redact_after_prefix(&out, prefix);
    }

    // 3. Redact recognizable token prefixes (`sk-...`, `AIza...`, `ant-api...`)
    out = redact_token_prefixes(&out, &["sk-", "AIza", "ant-api"]);
    out
}

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')
}

fn redact_after_prefix(input: &str, prefix: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(pos) = rest.find(prefix) {
        result.push_str(&rest[..pos + prefix.len()]);
        let after = &rest[pos + prefix.len()..];
        let ws_len: usize = after
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .map(char::len_utf8)
            .sum();
        result.push_str(&after[..ws_len]);
        let token_part = &after[ws_len..];
        let trimmed_quote = token_part.starts_with('"') || token_part.starts_with('\'');
        let quote_char = if trimmed_quote {
            token_part.chars().next()
        } else {
            None
        };
        let inner = if let Some(q) = quote_char {
            result.push(q);
            &token_part[q.len_utf8()..]
        } else {
            token_part
        };
        let tok_len: usize = inner
            .chars()
            .take_while(|c| is_token_char(*c))
            .map(char::len_utf8)
            .sum();
        if tok_len > 0 {
            result.push_str("[REDACTED_SECRET]");
            rest = &inner[tok_len..];
        } else {
            rest = inner;
        }
    }
    result.push_str(rest);
    result
}

fn redact_token_prefixes(input: &str, prefixes: &[&str]) -> String {
    let mut current = input.to_owned();
    for &prefix in prefixes {
        let mut out = String::with_capacity(current.len());
        let mut rest = current.as_str();
        while let Some(pos) = rest.find(prefix) {
            let valid_start = pos == 0
                || !rest[..pos]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_ascii_alphanumeric());
            if !valid_start {
                out.push_str(&rest[..pos + prefix.len()]);
                rest = &rest[pos + prefix.len()..];
                continue;
            }
            let candidate = &rest[pos..];
            let tok_len: usize = candidate
                .chars()
                .take_while(|c| is_token_char(*c))
                .map(char::len_utf8)
                .sum();
            if tok_len >= prefix.len() + 6 {
                out.push_str(&rest[..pos]);
                out.push_str("[REDACTED_SECRET]");
                rest = &candidate[tok_len..];
            } else {
                out.push_str(&rest[..pos + prefix.len()]);
                rest = &rest[pos + prefix.len()..];
            }
        }
        out.push_str(rest);
        current = out;
    }
    current
}

/// Summary of a user-selected stage element in an [`AiContextPack`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiSelectedElementSummary {
    pub id: String,
    pub name: String,
    pub kind_label: String,
    pub position: [f64; 2],
}

/// Bounded, UTF-8-safe, redacted script excerpt in an [`AiContextPack`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiScriptExcerpt {
    pub language: ScriptLanguage,
    pub source: String,
    pub truncated: bool,
    pub original_byte_len: usize,
}

/// Bounded asset metadata entry in an [`AiContextPack`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiAssetMetadata {
    pub resource_id: String,
    pub path: String,
    pub media_type: String,
    pub byte_len: usize,
    pub included: bool,
    pub exclusion_reason: Option<String>,
}

/// Deterministic, bounded, redacted context snapshot built strictly from user selection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiContextPack {
    pub project_name: String,
    pub symbol_name: String,
    pub active_frame: u32,
    pub selected_elements: Vec<AiSelectedElementSummary>,
    pub script_excerpt: Option<AiScriptExcerpt>,
    pub selected_assets: Vec<AiAssetMetadata>,
}

impl AiContextPack {
    /// Build a bounded, redacted context pack from the current editor selection.
    pub fn from_editor_selection(
        project: &Project,
        symbol_id: &str,
        active_frame: u32,
        selected_ids: &[String],
        script_input: Option<(&str, ScriptLanguage)>,
    ) -> Self {
        let symbol = project
            .symbol(symbol_id)
            .or_else(|| project.symbols.first());
        let symbol_name = symbol.map_or_else(
            || "Scene 1".to_owned(),
            |s| redact_sensitive_tokens(&s.name),
        );
        let mut selected_elements = Vec::new();
        let mut referenced_resource_ids = Vec::new();

        if let Some(sym) = symbol {
            for layer in &sym.timeline.layers {
                if let Some(kf) = layer.key_at(active_frame) {
                    for el in &kf.elements {
                        if !selected_ids.iter().any(|id| id == &el.id) {
                            continue;
                        }
                        let kind_label = match &el.kind {
                            ElementKind::Rectangle { .. } => "Rectangle",
                            ElementKind::Ellipse { .. } => "Ellipse",
                            ElementKind::Path { .. } => "Path",
                            ElementKind::PaintedPath { .. } => "PaintedPath",
                            ElementKind::Text { .. } => "Text",
                            ElementKind::TextBox { .. } => "TextBox",
                            ElementKind::Bitmap { resource_id, .. } => {
                                referenced_resource_ids.push(resource_id.clone());
                                "Bitmap"
                            }
                            ElementKind::Video { .. } => "Video",
                            ElementKind::Instance { .. } => "Instance",
                        }
                        .to_owned();
                        selected_elements.push(AiSelectedElementSummary {
                            id: el.id.clone(),
                            name: redact_sensitive_tokens(&el.name),
                            kind_label,
                            position: [el.transform.x, el.transform.y],
                        });
                    }
                }
            }
        }

        let script_excerpt = script_input.map(|(raw_source, language)| {
            let redacted = redact_sensitive_tokens(raw_source);
            let original_byte_len = redacted.len();
            let (bounded, truncated) = truncate_utf8_bytes(&redacted, MAX_SCRIPT_BYTES);
            AiScriptExcerpt {
                language,
                source: bounded.to_owned(),
                truncated,
                original_byte_len,
            }
        });

        let mut selected_assets = Vec::new();
        for resource in &project.resources {
            let is_referenced = referenced_resource_ids.iter().any(|id| id == &resource.id)
                || selected_ids.iter().any(|id| id == &resource.id);
            if !is_referenced {
                continue;
            }
            let decoded_len = resource
                .decoded_bytes()
                .map_or(resource.data_base64.len(), |b| b.len());
            let mime_allowed = ALLOWED_ASSET_MIME_TYPES
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(resource.media_type.trim()));
            let (included, exclusion_reason) = if !mime_allowed {
                (
                    false,
                    Some(format!(
                        "MIME type '{}' is not in the allowed inspection whitelist",
                        resource.media_type
                    )),
                )
            } else if decoded_len > MAX_ASSET_BYTES {
                (
                    false,
                    Some(format!(
                        "Asset size ({decoded_len} bytes) exceeds MAX_ASSET_BYTES ({MAX_ASSET_BYTES} bytes)"
                    )),
                )
            } else {
                (true, None)
            };
            selected_assets.push(AiAssetMetadata {
                resource_id: resource.id.clone(),
                path: redact_sensitive_tokens(&resource.path),
                media_type: resource.media_type.clone(),
                byte_len: decoded_len,
                included,
                exclusion_reason,
            });
        }

        Self {
            project_name: redact_sensitive_tokens(&project.name),
            symbol_name,
            active_frame,
            selected_elements,
            script_excerpt,
            selected_assets,
        }
    }

    pub fn included_asset_count(&self) -> usize {
        self.selected_assets.iter().filter(|a| a.included).count()
    }

    /// Return a copy of this context pack containing strictly the sections ticked in `selection`,
    /// with excluded (`!included`) assets stripped.
    pub fn filtered_for_consent(&self, selection: AiContextSelection) -> Self {
        Self {
            project_name: if selection.project_and_symbol {
                self.project_name.clone()
            } else {
                String::new()
            },
            symbol_name: if selection.project_and_symbol {
                self.symbol_name.clone()
            } else {
                String::new()
            },
            active_frame: if selection.project_and_symbol {
                self.active_frame
            } else {
                0
            },
            selected_elements: if selection.selected_elements {
                self.selected_elements.clone()
            } else {
                Vec::new()
            },
            script_excerpt: if selection.script_excerpt {
                self.script_excerpt.clone()
            } else {
                None
            },
            selected_assets: if selection.asset_list {
                self.selected_assets
                    .iter()
                    .filter(|a| a.included)
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            },
        }
    }

    /// Human-readable `"Not sent: ..."` summary for excluded assets and truncated scripts (`R-097`).
    pub fn omitted_items_note(&self) -> Option<String> {
        let mut items = Vec::new();
        if let Some(script) = &self.script_excerpt
            && script.truncated
        {
            items.push(format!(
                "script tail beyond {} KB",
                MAX_SCRIPT_BYTES / 1024
            ));
        }
        for asset in self.selected_assets.iter().filter(|a| !a.included) {
            let short_name = asset
                .path
                .rsplit(['/', '\\'])
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or(&asset.resource_id);
            let reason = if asset.byte_len > MAX_ASSET_BYTES {
                format!("larger than {} KB", MAX_ASSET_BYTES / 1024)
            } else {
                let category = asset
                    .media_type
                    .split('/')
                    .next()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("asset");
                format!("{category} is not an allowed type")
            };
            items.push(format!("{short_name} ({reason})"));
        }
        (!items.is_empty()).then(|| format!("Not sent: {}.", items.join(", ")))
    }
}

/// User consent toggles for the four `What will be sent` rows in `Dialog / Send to AI` (`R-097`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AiContextSelection {
    pub project_and_symbol: bool,
    pub selected_elements: bool,
    pub script_excerpt: bool,
    pub asset_list: bool,
}

impl Default for AiContextSelection {
    fn default() -> Self {
        Self {
            project_and_symbol: true,
            selected_elements: true,
            script_excerpt: true,
            asset_list: true,
        }
    }
}

impl AiContextSelection {
    pub fn from_pack(pack: &AiContextPack) -> Self {
        Self {
            project_and_symbol: true,
            selected_elements: !pack.selected_elements.is_empty(),
            script_excerpt: pack.script_excerpt.is_some(),
            asset_list: pack.included_asset_count() > 0,
        }
    }
}

fn truncate_utf8_bytes(s: &str, max_bytes: usize) -> (&str, bool) {
    if s.len() <= max_bytes {
        return (s, false);
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    (&s[..end], true)
}

fn deserialize_proposal_language<'de, D>(deserializer: D) -> Result<ScriptLanguage, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    let norm: String = raw
        .trim()
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    match norm.as_str() {
        "actionscript3" | "actionscript30" | "as3" => Ok(ScriptLanguage::ActionScript3),
        "actionscript2" | "actionscript20" | "as2" => Ok(ScriptLanguage::ActionScript2),
        "actionscript1" | "actionscript10" | "as1" => Ok(ScriptLanguage::ActionScript1),
        "javascript" | "js" | "ecmascript" => Ok(ScriptLanguage::JavaScript),
        "typescript" | "ts" => Ok(ScriptLanguage::TypeScript),
        _ => Err(serde::de::Error::custom(format!(
            "unsupported proposal script language: {raw}"
        ))),
    }
}

/// Strictly untrusted AI output proposal.
///
/// Proposals are inert value objects: they must be inspected and explicitly accepted by user action
/// in a higher-level UI workflow before any document transaction can occur.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "proposal_kind", rename_all = "snake_case")]
pub enum AiUntrustedProposal {
    ExplanationText {
        summary: String,
        body: String,
    },
    CodeSnippetProposal {
        #[serde(deserialize_with = "deserialize_proposal_language")]
        language: ScriptLanguage,
        target_hint: String,
        code: String,
        explanation: String,
    },
    KeyframeTimingProposal {
        layer_name: String,
        start_frame: u32,
        end_frame: u32,
        easing_hint: String,
        notes: String,
    },
}

/// Parse structured untrusted proposals from completed model text.
///
/// Recognizes fenced ` ```json ... ``` ` blocks (or top-level JSON objects) matching
/// [`AiUntrustedProposal`], fenced code blocks (` ```as3 `, ` ```ts `, etc.), or falls back to a
/// single [`AiUntrustedProposal::ExplanationText`] value object.
pub fn parse_untrusted_proposals(text: &str) -> Vec<AiUntrustedProposal> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    if let Ok(single) = serde_json::from_str::<AiUntrustedProposal>(trimmed) {
        return vec![single];
    }
    if let Ok(list) = serde_json::from_str::<Vec<AiUntrustedProposal>>(trimmed)
        && !list.is_empty()
    {
        return list;
    }

    let mut proposals = Vec::new();
    let mut scan = trimmed;
    while let Some(fence_start) = scan.find("```") {
        let after_ticks = &scan[fence_start + 3..];
        let Some(nl_pos) = after_ticks.find('\n') else {
            break;
        };
        let lang_tag = after_ticks[..nl_pos].trim().to_ascii_lowercase();
        let body_rest = &after_ticks[nl_pos + 1..];
        let Some(close_pos) = body_rest.find("```") else {
            break;
        };
        let block_body = body_rest[..close_pos].trim();
        if lang_tag == "json" {
            if let Ok(p) = serde_json::from_str::<AiUntrustedProposal>(block_body) {
                proposals.push(p);
            } else if let Ok(list) = serde_json::from_str::<Vec<AiUntrustedProposal>>(block_body) {
                proposals.extend(list);
            }
        } else if !block_body.is_empty() {
            let language = match lang_tag.as_str() {
                "as2" | "actionscript2" => ScriptLanguage::ActionScript2,
                "as1" | "actionscript1" => ScriptLanguage::ActionScript1,
                "js" | "javascript" => ScriptLanguage::JavaScript,
                "ts" | "typescript" => ScriptLanguage::TypeScript,
                _ => ScriptLanguage::ActionScript3,
            };
            proposals.push(AiUntrustedProposal::CodeSnippetProposal {
                language,
                target_hint: "active_script".into(),
                code: block_body.to_owned(),
                explanation: "Proposed code snippet (requires user confirmation)".into(),
            });
        }
        scan = &body_rest[close_pos + 3..];
    }

    if proposals.is_empty() {
        let first_line = trimmed.lines().next().unwrap_or("Explanation").trim();
        proposals.push(AiUntrustedProposal::ExplanationText {
            summary: first_line.chars().take(80).collect(),
            body: trimmed.to_owned(),
        });
    }

    proposals
}

/// Request payload for a provider-neutral AI prompt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiPromptRequest {
    pub request_id: String,
    pub profile_id: String,
    pub model: Option<String>,
    pub system_prompt: String,
    pub user_prompt: String,
    pub context_pack: AiContextPack,
    pub temperature: Option<f32>,
    pub max_output_tokens: u32,
    pub stream: bool,
}

impl AiPromptRequest {
    pub fn compose_user_message(&self) -> String {
        let user = redact_sensitive_tokens(&self.user_prompt);
        let mut ctx_map = serde_json::Map::new();
        if !self.context_pack.project_name.is_empty() || !self.context_pack.symbol_name.is_empty() {
            ctx_map.insert(
                "project_name".into(),
                serde_json::json!(self.context_pack.project_name),
            );
            ctx_map.insert(
                "symbol_name".into(),
                serde_json::json!(self.context_pack.symbol_name),
            );
            ctx_map.insert(
                "active_frame".into(),
                serde_json::json!(self.context_pack.active_frame),
            );
        }
        if !self.context_pack.selected_elements.is_empty() {
            ctx_map.insert(
                "selected_elements".into(),
                serde_json::to_value(&self.context_pack.selected_elements).unwrap_or_default(),
            );
        }
        if let Some(script) = &self.context_pack.script_excerpt {
            ctx_map.insert(
                "script_excerpt".into(),
                serde_json::to_value(script).unwrap_or_default(),
            );
        }
        let included_assets: Vec<&AiAssetMetadata> = self
            .context_pack
            .selected_assets
            .iter()
            .filter(|a| a.included)
            .collect();
        if !included_assets.is_empty() {
            ctx_map.insert(
                "selected_assets".into(),
                serde_json::to_value(&included_assets).unwrap_or_default(),
            );
        }
        if ctx_map.is_empty() {
            user
        } else {
            let ctx_json = serde_json::Value::Object(ctx_map).to_string();
            format!("{user}\n\n[NeoFlash Selection Context]\n{ctx_json}")
        }
    }
}

/// Streaming lifecycle events emitted by [`run_streaming_request`] and [`AiConnectionWorker`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AiStreamEvent {
    Started {
        request_id: String,
        provider: AiProviderKind,
        model: String,
    },
    TextDelta {
        request_id: String,
        delta: String,
    },
    ProposalReady {
        request_id: String,
        proposal: AiUntrustedProposal,
    },
    RateLimited {
        request_id: String,
        retry_after_ms: Option<u64>,
        message: String,
    },
    RetryScheduled {
        request_id: String,
        attempt: u32,
        delay_ms: u64,
        reason: String,
    },
    Cancelled {
        request_id: String,
    },
    Completed {
        request_id: String,
        full_text: String,
        proposals: Vec<AiUntrustedProposal>,
    },
    Failed {
        request_id: String,
        error: AiConnectionError,
    },
}

/// Thread-safe cancellation token for in-flight streaming requests.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Formatted HTTP request ready for transport.
#[derive(Clone)]
pub struct FormattedProviderRequest {
    pub method: &'static str,
    pub path_and_query: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl fmt::Debug for FormattedProviderRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let safe_headers: Vec<(String, String)> = self
            .headers
            .iter()
            .map(|(k, v)| {
                let lower = k.to_ascii_lowercase();
                if lower.contains("authorization") || lower.contains("api-key") {
                    (k.clone(), "[REDACTED_SECRET]".to_owned())
                } else {
                    (k.clone(), redact_sensitive_tokens(v))
                }
            })
            .collect();
        f.debug_struct("FormattedProviderRequest")
            .field("method", &self.method)
            .field("path_and_query", &self.path_and_query)
            .field("headers", &safe_headers)
            .field("body_len", &self.body.len())
            .finish()
    }
}

fn validate_gemini_model_id(model: &str) -> Result<(), AiConnectionError> {
    if model.is_empty()
        || model.len() > 128
        || !model
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(AiConnectionError::InvalidEndpoint {
            reason: "Gemini model name must be a 1..128 byte path-safe identifier.".into(),
        });
    }
    Ok(())
}

/// Build the provider-specific HTTP request for `OpenAi`, `Anthropic`, `Gemini`, or
/// `LocalOpenAiCompatible`.
pub fn format_provider_request(
    profile: &AiProviderProfile,
    request: &AiPromptRequest,
    secret: Option<&str>,
) -> Result<FormattedProviderRequest, AiConnectionError> {
    let token = secret.map(str::trim).filter(|s| !s.is_empty());
    if profile.kind.requires_secret() && token.is_none() {
        return Err(AiConnectionError::MissingSecret {
            profile_id: profile.id.clone(),
        });
    }
    if let Some(token) = token {
        validate_http_header_value("Provider credential", token)?;
    }

    let model = request
        .model
        .as_deref()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or(&profile.default_model);
    let system_text = redact_sensitive_tokens(&request.system_prompt);
    let user_text = request.compose_user_message();
    let base_path = extract_base_path(&profile.base_url)?;
    let accept_header = if request.stream {
        "text/event-stream"
    } else {
        "application/json"
    };

    match profile.kind {
        AiProviderKind::OpenAi | AiProviderKind::LocalOpenAiCompatible => {
            let path_and_query = join_endpoint_path(&base_path, "/v1/chat/completions");
            let mut headers = vec![
                ("Content-Type".to_owned(), "application/json".to_owned()),
                ("Accept".to_owned(), accept_header.to_owned()),
            ];
            if let Some(tok) = token {
                headers.push(("Authorization".to_owned(), format!("Bearer {tok}")));
            }
            let mut payload = serde_json::json!({
                "model": model,
                "stream": request.stream,
                "max_tokens": request.max_output_tokens,
                "messages": [
                    { "role": "system", "content": system_text },
                    { "role": "user", "content": user_text },
                ]
            });
            if let Some(temp) = request.temperature {
                payload["temperature"] = serde_json::json!(temp);
            }
            Ok(FormattedProviderRequest {
                method: "POST",
                path_and_query,
                headers,
                body: payload.to_string(),
            })
        }
        AiProviderKind::Anthropic => {
            let path_and_query = join_endpoint_path(&base_path, "/v1/messages");
            let tok = token.expect("checked above");
            let headers = vec![
                ("Content-Type".to_owned(), "application/json".to_owned()),
                ("Accept".to_owned(), accept_header.to_owned()),
                ("x-api-key".to_owned(), tok.to_owned()),
                ("anthropic-version".to_owned(), "2023-06-01".to_owned()),
            ];
            let mut payload = serde_json::json!({
                "model": model,
                "max_tokens": request.max_output_tokens,
                "stream": request.stream,
                "system": system_text,
                "messages": [
                    { "role": "user", "content": user_text }
                ]
            });
            if let Some(temp) = request.temperature {
                payload["temperature"] = serde_json::json!(temp);
            }
            Ok(FormattedProviderRequest {
                method: "POST",
                path_and_query,
                headers,
                body: payload.to_string(),
            })
        }
        AiProviderKind::Gemini => {
            validate_gemini_model_id(model)?;
            let action = if request.stream {
                "streamGenerateContent?alt=sse"
            } else {
                "generateContent"
            };
            let suffix = format!("/v1beta/models/{model}:{action}");
            let path_and_query = join_endpoint_path(&base_path, &suffix);
            let tok = token.expect("checked above");
            let headers = vec![
                ("Content-Type".to_owned(), "application/json".to_owned()),
                ("Accept".to_owned(), accept_header.to_owned()),
                ("x-goog-api-key".to_owned(), tok.to_owned()),
            ];
            let mut gen_cfg = serde_json::json!({
                "maxOutputTokens": request.max_output_tokens,
            });
            if let Some(temp) = request.temperature {
                gen_cfg["temperature"] = serde_json::json!(temp);
            }
            let payload = serde_json::json!({
                "systemInstruction": {
                    "parts": [{ "text": system_text }]
                },
                "contents": [
                    {
                        "role": "user",
                        "parts": [{ "text": user_text }]
                    }
                ],
                "generationConfig": gen_cfg,
            });
            Ok(FormattedProviderRequest {
                method: "POST",
                path_and_query,
                headers,
                body: payload.to_string(),
            })
        }
    }
}

/// Outcome of parsing a single SSE line from a provider stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SseParseOutcome {
    Ignore,
    Delta(String),
    Done,
}

/// Parse a single SSE line (`data: ...` or `event: ...`) for the given [`AiProviderKind`].
pub fn parse_sse_line(
    kind: AiProviderKind,
    line: &str,
) -> Result<SseParseOutcome, AiConnectionError> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with(':') {
        return Ok(SseParseOutcome::Ignore);
    }
    if let Some(event_name) = trimmed.strip_prefix("event:") {
        let ev = event_name.trim();
        if ev == "message_stop" {
            return Ok(SseParseOutcome::Done);
        }
        return Ok(SseParseOutcome::Ignore);
    }
    let Some(data_raw) = trimmed.strip_prefix("data:") else {
        return Ok(SseParseOutcome::Ignore);
    };
    let data = data_raw.trim();
    if data == "[DONE]" {
        return Ok(SseParseOutcome::Done);
    }
    let value: serde_json::Value =
        serde_json::from_str(data).map_err(|e| AiConnectionError::Protocol {
            message: format!("Malformed SSE JSON payload: {e}"),
        })?;
    if let Some(err_msg) = value
        .get("error")
        .and_then(|e| e.get("message").or(Some(e)))
        .and_then(|m| m.as_str())
    {
        return Err(AiConnectionError::Protocol {
            message: err_msg.to_owned(),
        });
    }

    match kind {
        AiProviderKind::OpenAi | AiProviderKind::LocalOpenAiCompatible => {
            if let Some(choice) = value
                .get("choices")
                .and_then(|c| c.as_array())
                .and_then(|a| a.first())
            {
                if let Some(content) = choice
                    .get("delta")
                    .and_then(|d| d.get("content"))
                    .and_then(|c| c.as_str())
                    .or_else(|| {
                        choice
                            .get("message")
                            .and_then(|m| m.get("content"))
                            .and_then(|c| c.as_str())
                    })
                    && !content.is_empty()
                {
                    return Ok(SseParseOutcome::Delta(content.to_owned()));
                }
                if choice
                    .get("finish_reason")
                    .and_then(|f| f.as_str())
                    .is_some_and(|r| r == "stop")
                {
                    return Ok(SseParseOutcome::Done);
                }
            }
            Ok(SseParseOutcome::Ignore)
        }
        AiProviderKind::Anthropic => {
            let msg_type = value.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if msg_type == "message_stop" {
                return Ok(SseParseOutcome::Done);
            }
            if msg_type == "content_block_delta"
                && let Some(text) = value
                    .get("delta")
                    .and_then(|d| d.get("text"))
                    .and_then(|t| t.as_str())
                && !text.is_empty()
            {
                return Ok(SseParseOutcome::Delta(text.to_owned()));
            }
            Ok(SseParseOutcome::Ignore)
        }
        AiProviderKind::Gemini => {
            if let Some(parts) = value
                .get("candidates")
                .and_then(|c| c.as_array())
                .and_then(|a| a.first())
                .and_then(|cand| cand.get("content"))
                .and_then(|content| content.get("parts"))
                .and_then(|p| p.as_array())
            {
                let mut combined = String::new();
                for part in parts {
                    if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                        combined.push_str(t);
                    }
                }
                if !combined.is_empty() {
                    return Ok(SseParseOutcome::Delta(combined));
                }
            }
            Ok(SseParseOutcome::Ignore)
        }
    }
}

/// Parse a complete non-streaming JSON completion body (`stream == false`).
pub fn parse_non_streaming_response(
    kind: AiProviderKind,
    body: &str,
) -> Result<String, AiConnectionError> {
    let value: serde_json::Value =
        serde_json::from_str(body.trim()).map_err(|e| AiConnectionError::Protocol {
            message: format!("Malformed JSON response payload: {e}"),
        })?;
    if let Some(err_msg) = value
        .get("error")
        .and_then(|e| e.get("message").or(Some(e)))
        .and_then(|m| m.as_str())
    {
        return Err(AiConnectionError::Protocol {
            message: redact_sensitive_tokens(err_msg),
        });
    }

    let raw_text = match kind {
        AiProviderKind::OpenAi | AiProviderKind::LocalOpenAiCompatible => {
            let content = value
                .get("choices")
                .and_then(|c| c.as_array())
                .and_then(|a| a.first())
                .and_then(|ch| ch.get("message"))
                .and_then(|m| m.get("content"));
            match content {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Array(items)) => items
                    .iter()
                    .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join(""),
                _ => String::new(),
            }
        }
        AiProviderKind::Anthropic => value
            .get("content")
            .and_then(|c| c.as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default(),
        AiProviderKind::Gemini => value
            .get("candidates")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|cand| cand.get("content"))
            .and_then(|content| content.get("parts"))
            .and_then(|p| p.as_array())
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default(),
    };
    let safe = redact_sensitive_tokens(&raw_text);
    if safe.len() > MAX_AI_RESPONSE_BYTES {
        return Err(AiConnectionError::Protocol {
            message: format!(
                "Provider response exceeds the {} MiB limit.",
                MAX_AI_RESPONSE_BYTES / (1024 * 1024)
            ),
        });
    }
    Ok(safe)
}

#[cfg(test)]
std::thread_local! {
    static TEST_LOOPBACK_HTTP_OVERRIDE: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// Allow deterministic local `http://127.0.0.1:<port>` mock fixtures for cloud providers strictly
/// within a test scope. Outside this scope (and unconditionally in non-test builds), cloud
/// providers reject all non-`https://` URLs.
#[cfg(test)]
pub(crate) fn with_test_loopback_http_override<R>(origin: &str, f: impl FnOnce() -> R) -> R {
    struct Guard(Option<String>);
    impl Drop for Guard {
        fn drop(&mut self) {
            TEST_LOOPBACK_HTTP_OVERRIDE.with(|slot| {
                *slot.borrow_mut() = self.0.take();
            });
        }
    }
    let prev = TEST_LOOPBACK_HTTP_OVERRIDE.with(|slot| slot.borrow_mut().replace(origin.to_owned()));
    let _guard = Guard(prev);
    f()
}

fn current_loopback_http_override() -> Option<String> {
    #[cfg(test)]
    {
        TEST_LOOPBACK_HTTP_OVERRIDE.with(|slot| slot.borrow().clone())
    }
    #[cfg(not(test))]
    {
        None
    }
}

/// Validate a profile's Base URL for UI display (destination host + scheme) and transport.
pub fn validate_profile_endpoint(
    profile: &AiProviderProfile,
) -> Result<ValidatedAiEndpoint, AiConnectionError> {
    let override_origin = current_loopback_http_override();
    validate_ai_base_url(
        &profile.base_url,
        profile.kind.endpoint_security(),
        override_origin.as_deref(),
    )
    .map_err(|err| map_ai_http_error(err, profile.timeout_ms))
}

fn map_ai_http_error(err: AiHttpError, timeout_ms: u64) -> AiConnectionError {
    match err {
        AiHttpError::InvalidEndpoint(reason) => AiConnectionError::InvalidEndpoint {
            reason: redact_sensitive_tokens(&reason),
        },
        AiHttpError::Offline(reason) => AiConnectionError::Offline {
            reason: redact_sensitive_tokens(&reason),
        },
        AiHttpError::Timeout => AiConnectionError::Timeout { timeout_ms },
        AiHttpError::RateLimited {
            retry_after_ms,
            body,
        } => AiConnectionError::RateLimited {
            retry_after_ms,
            message: if body.trim().is_empty() {
                "HTTP 429 Too Many Requests".into()
            } else {
                redact_sensitive_tokens(body.trim())
            },
        },
        AiHttpError::HttpStatus { status, body, .. } => AiConnectionError::HttpStatus {
            status,
            message: redact_sensitive_tokens(body.trim()),
        },
        AiHttpError::RedirectRejected { status, location } => AiConnectionError::InvalidEndpoint {
            reason: redact_sensitive_tokens(&format!(
                "Redirect HTTP {status} to '{location}' rejected; configure the direct HTTPS provider endpoint."
            )),
        },
        AiHttpError::Protocol(message) => AiConnectionError::Protocol {
            message: redact_sensitive_tokens(&message),
        },
        AiHttpError::Cancelled => AiConnectionError::Cancelled,
    }
}

#[cfg(test)]
struct ParsedUrl {
    host: String,
    #[allow(dead_code)]
    host_header: String,
    #[allow(dead_code)]
    port: u16,
}

fn parse_base_url(base_url: &str) -> Result<Url, AiConnectionError> {
    let trimmed = base_url.trim();
    if trimmed.is_empty()
        || trimmed.chars().any(char::is_control)
        || trimmed.chars().any(char::is_whitespace)
        || trimmed.contains('\\')
    {
        return Err(AiConnectionError::InvalidEndpoint {
            reason: "Endpoint contains empty, whitespace, control, or backslash characters.".into(),
        });
    }
    let parsed = Url::parse(trimmed).map_err(|error| AiConnectionError::InvalidEndpoint {
        reason: format!("Malformed endpoint URL: {error}"),
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AiConnectionError::InvalidEndpoint {
            reason: "Endpoint URL must use http:// or https://.".into(),
        });
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(AiConnectionError::InvalidEndpoint {
            reason: "Endpoint URL must not contain embedded credentials.".into(),
        });
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(AiConnectionError::InvalidEndpoint {
            reason: "Endpoint base URL must not contain a query or fragment.".into(),
        });
    }
    if parsed.host().is_none() {
        return Err(AiConnectionError::InvalidEndpoint {
            reason: "Endpoint URL is missing a host.".into(),
        });
    }
    Ok(parsed)
}

#[cfg(test)]
fn parse_http_url(base_url: &str) -> Result<ParsedUrl, AiConnectionError> {
    let parsed = parse_base_url(base_url)?;
    if parsed.scheme() == "https" {
        return Err(AiConnectionError::InvalidEndpoint {
            reason: "Plaintext socket helper accepts http:// only.".into(),
        });
    }
    let host = parsed.host_str().unwrap_or_default().to_owned();
    let host_header = match parsed.host() {
        Some(Host::Ipv6(address)) => format!("[{address}]"),
        _ => host.clone(),
    };
    let port = parsed.port_or_known_default().unwrap_or(80);
    Ok(ParsedUrl {
        host,
        host_header,
        port,
    })
}

fn extract_base_path(base_url: &str) -> Result<String, AiConnectionError> {
    let parsed = parse_base_url(base_url)?;
    let path = parsed.path().trim_matches('/');
    if path.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("/{path}"))
    }
}

fn join_endpoint_path(base_path: &str, suffix: &str) -> String {
    if base_path.is_empty() || base_path == "/" {
        return suffix.to_owned();
    }
    if suffix == base_path
        || suffix
            .strip_prefix(base_path)
            .is_some_and(|rest| rest.starts_with('/'))
    {
        return suffix.to_owned();
    }
    if let Some(rest) = suffix.strip_prefix("/v1/")
        && base_path.ends_with("/v1")
    {
        return format!("{base_path}/{rest}");
    }
    if suffix == "/v1" && base_path.ends_with("/v1") {
        return base_path.to_owned();
    }
    format!("{base_path}{suffix}")
}

#[cfg(test)]
fn read_bounded_line<R: BufRead + ?Sized>(
    reader: &mut R,
    max_bytes: usize,
) -> std::io::Result<Option<String>> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |idx| idx + 1);
        if bytes.len().saturating_add(take) > max_bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("provider HTTP line exceeds {max_bytes} bytes"),
            ));
        }
        bytes.extend_from_slice(&available[..take]);
        reader.consume(take);
        if bytes.last() == Some(&b'\n') {
            break;
        }
    }
    if bytes.is_empty() {
        return Ok(None);
    }
    String::from_utf8(bytes).map(Some).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "provider HTTP line is not valid UTF-8",
        )
    })
}

#[cfg(test)]
fn read_error_body_bounded<R: Read + ?Sized>(reader: &mut R) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_AI_ERROR_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let truncated = bytes.len() > MAX_AI_ERROR_BYTES;
    bytes.truncate(MAX_AI_ERROR_BYTES);
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        text.push_str("\n[diagnostic body truncated]");
    }
    Ok(text)
}

#[cfg(test)]
const MAX_AI_HTTP_CHUNK_BYTES: usize = 8 * 1024 * 1024;

#[cfg(test)]
struct ChunkedBodyReader<R> {
    inner: R,
    remaining: usize,
    needs_payload_crlf: bool,
    finished: bool,
}

#[cfg(test)]
impl<R: BufRead> ChunkedBodyReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            remaining: 0,
            needs_payload_crlf: false,
            finished: false,
        }
    }

    fn begin_next_chunk(&mut self) -> std::io::Result<()> {
        if self.needs_payload_crlf {
            let mut delimiter = [0u8; 2];
            self.inner.read_exact(&mut delimiter)?;
            if delimiter != *b"\r\n" {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "provider chunk payload is not followed by CRLF",
                ));
            }
            self.needs_payload_crlf = false;
        }
        let line = read_bounded_line(&mut self.inner, 8 * 1024)?.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "provider chunked body ended before the final chunk",
            )
        })?;
        if !line.ends_with("\r\n") {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "provider chunk-size line is not CRLF terminated",
            ));
        }
        let size_text = line
            .trim_end_matches("\r\n")
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "provider returned an invalid HTTP chunk size",
            )
        })?;
        if size > MAX_AI_HTTP_CHUNK_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "provider HTTP chunk exceeds the {} MiB limit",
                    MAX_AI_HTTP_CHUNK_BYTES / (1024 * 1024)
                ),
            ));
        }
        if size == 0 {
            let mut trailer_bytes = 0usize;
            let mut trailer_fields = 0usize;
            loop {
                let trailer = read_bounded_line(&mut self.inner, 64 * 1024)?.ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "provider chunked trailers ended unexpectedly",
                    )
                })?;
                if trailer == "\r\n" {
                    break;
                }
                trailer_bytes = trailer_bytes.saturating_add(trailer.len());
                trailer_fields = trailer_fields.saturating_add(1);
                if trailer_bytes > 64 * 1024 || trailer_fields > 256 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "provider HTTP trailers exceed the 64 KiB / 256-field limit",
                    ));
                }
            }
            self.finished = true;
        } else {
            self.remaining = size;
            self.needs_payload_crlf = true;
        }
        Ok(())
    }
}

#[cfg(test)]
impl<R: BufRead> Read for ChunkedBodyReader<R> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if output.is_empty() || self.finished {
            return Ok(0);
        }
        while self.remaining == 0 && !self.finished {
            self.begin_next_chunk()?;
        }
        if self.finished {
            return Ok(0);
        }
        let limit = output.len().min(self.remaining);
        let read = self.inner.read(&mut output[..limit])?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "provider chunk payload ended unexpectedly",
            ));
        }
        self.remaining -= read;
        Ok(read)
    }
}

#[cfg(test)]
fn response_uses_chunked_transfer(headers: &[(String, String)]) -> Result<bool, AiConnectionError> {
    let mut codings = Vec::new();
    for (_, value) in headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"))
    {
        codings.extend(
            value
                .split(',')
                .map(|coding| coding.trim().to_ascii_lowercase())
                .filter(|coding| !coding.is_empty()),
        );
    }
    if codings.is_empty() {
        return Ok(false);
    }
    if codings.last().is_some_and(|coding| coding == "chunked")
        && codings.iter().filter(|coding| *coding == "chunked").count() == 1
        && codings
            .iter()
            .all(|coding| matches!(coding.as_str(), "identity" | "chunked"))
    {
        Ok(true)
    } else {
        Err(AiConnectionError::Protocol {
            message: format!(
                "Unsupported provider Transfer-Encoding: {}",
                codings.join(", ")
            ),
        })
    }
}

fn validate_http_header_value(name: &str, value: &str) -> Result<(), AiConnectionError> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(AiConnectionError::InvalidEndpoint {
            reason: format!("{name} contains empty or control characters."),
        });
    }
    Ok(())
}

#[cfg(test)]
fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
fn validate_plaintext_transport(
    parsed: &ParsedUrl,
    resolved: SocketAddr,
) -> Result<(), AiConnectionError> {
    if is_loopback_host(&parsed.host) && resolved.ip().is_loopback() {
        return Ok(());
    }
    Err(AiConnectionError::InvalidEndpoint {
        reason: "Plaintext AI endpoints are limited to a trusted local proxy on localhost or a literal loopback address; the selected host and resolved address must both be loopback.".into(),
    })
}

/// Test a provider connection profile over TLS/HTTPS (or loopback HTTP for local providers)
/// without mutating project state.
pub fn test_provider_connection(
    profile: &AiProviderProfile,
    secret_store: &dyn SecretStore,
) -> Result<String, AiConnectionError> {
    let secret = secret_store
        .get_secret(&profile.id)?
        .filter(|s| !s.trim().is_empty());
    if profile.kind.requires_secret() && secret.is_none() {
        return Err(AiConnectionError::MissingSecret {
            profile_id: profile.id.clone(),
        });
    }
    if let Some(token) = secret.as_deref() {
        validate_http_header_value("Provider credential", token)?;
    }

    let endpoint = validate_profile_endpoint(profile)?;
    let suffix = match profile.kind {
        AiProviderKind::OpenAi | AiProviderKind::LocalOpenAiCompatible | AiProviderKind::Anthropic => {
            "/v1/models".to_owned()
        }
        AiProviderKind::Gemini => {
            let model = profile.default_model.trim();
            if model.is_empty() {
                "/v1beta/models".to_owned()
            } else {
                validate_gemini_model_id(model)?;
                format!("/v1beta/models/{model}")
            }
        }
    };
    let path_and_query = join_endpoint_path(&endpoint.base_path, &suffix);

    let mut headers = vec![("Accept".to_owned(), "application/json".to_owned())];
    if let Some(tok) = secret.as_deref().map(str::trim) {
        match profile.kind {
            AiProviderKind::OpenAi | AiProviderKind::LocalOpenAiCompatible => {
                headers.push(("Authorization".to_owned(), format!("Bearer {tok}")));
            }
            AiProviderKind::Anthropic => {
                headers.push(("x-api-key".to_owned(), tok.to_owned()));
                headers.push(("anthropic-version".to_owned(), "2023-06-01".to_owned()));
            }
            AiProviderKind::Gemini => {
                headers.push(("x-goog-api-key".to_owned(), tok.to_owned()));
            }
        }
    }

    let timeout = Duration::from_millis(profile.timeout_ms.max(50));
    let cancel = AtomicBool::new(false);
    send_ai_get(&endpoint, &path_and_query, &headers, timeout, &cancel)
        .map_err(|err| map_ai_http_error(err, profile.timeout_ms))?;

    let model_label = if profile.default_model.trim().is_empty() {
        profile.display_name.as_str()
    } else {
        profile.default_model.trim()
    };
    Ok(format!("Connected · {model_label}"))
}

/// Execute a streaming (or non-streaming) AI prompt request over `ureq` + `rustls` with
/// configurable timeout, [`CancellationToken`], and automatic HTTP 429 / 5xx retry handling.
pub fn run_streaming_request(
    profile: &AiProviderProfile,
    request: &AiPromptRequest,
    secret_store: &dyn SecretStore,
    cancel_token: &CancellationToken,
    mut on_event: impl FnMut(AiStreamEvent),
) -> Result<Vec<AiUntrustedProposal>, AiConnectionError> {
    if cancel_token.is_cancelled() {
        on_event(AiStreamEvent::Cancelled {
            request_id: request.request_id.clone(),
        });
        return Err(AiConnectionError::Cancelled);
    }

    let secret = secret_store.get_secret(&profile.id)?;
    let formatted = match format_provider_request(profile, request, secret.as_deref()) {
        Ok(f) => f,
        Err(err) => {
            on_event(AiStreamEvent::Failed {
                request_id: request.request_id.clone(),
                error: err.clone(),
            });
            return Err(err);
        }
    };
    let endpoint = match validate_profile_endpoint(profile) {
        Ok(ep) => ep,
        Err(err) => {
            on_event(AiStreamEvent::Failed {
                request_id: request.request_id.clone(),
                error: err.clone(),
            });
            return Err(err);
        }
    };

    let model_name = request
        .model
        .clone()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| profile.default_model.clone());
    let timeout = Duration::from_millis(profile.timeout_ms.max(25));
    let mut attempt = 0u32;

    loop {
        if cancel_token.is_cancelled() {
            on_event(AiStreamEvent::Cancelled {
                request_id: request.request_id.clone(),
            });
            return Err(AiConnectionError::Cancelled);
        }

        let mut full_text = String::new();
        let mut sse_error: Option<AiConnectionError> = None;

        let transport_result = if request.stream {
            let started_req_id = request.request_id.clone();
            let started_model = model_name.clone();
            let provider_kind = profile.kind;
            let delta_req_id = request.request_id.clone();
            let on_event_cell = std::cell::RefCell::new(&mut on_event);
            stream_ai_post_lines(
                &endpoint,
                &formatted.path_and_query,
                &formatted.headers,
                &formatted.body,
                timeout,
                &cancel_token.cancelled,
                || {
                    (on_event_cell.borrow_mut())(AiStreamEvent::Started {
                        request_id: started_req_id,
                        provider: provider_kind,
                        model: started_model,
                    });
                },
                |line| match parse_sse_line(provider_kind, line) {
                    Ok(SseParseOutcome::Ignore) => Ok(true),
                    Ok(SseParseOutcome::Delta(delta)) => {
                        let safe_delta = redact_sensitive_tokens(&delta);
                        if full_text.len().saturating_add(safe_delta.len()) > MAX_AI_RESPONSE_BYTES
                        {
                            sse_error = Some(AiConnectionError::Protocol {
                                message: format!(
                                    "Provider response exceeds the {} MiB limit.",
                                    MAX_AI_RESPONSE_BYTES / (1024 * 1024)
                                ),
                            });
                            return Err(AiHttpError::Protocol("response_limit".into()));
                        }
                        full_text.push_str(&safe_delta);
                        (on_event_cell.borrow_mut())(AiStreamEvent::TextDelta {
                            request_id: delta_req_id.clone(),
                            delta: safe_delta,
                        });
                        Ok(true)
                    }
                    Ok(SseParseOutcome::Done) => Ok(false),
                    Err(err) => {
                        sse_error = Some(err);
                        Err(AiHttpError::Protocol("sse_parse_error".into()))
                    }
                },
            )
        } else {
            send_ai_post_json(
                &endpoint,
                &formatted.path_and_query,
                &formatted.headers,
                &formatted.body,
                timeout,
                &cancel_token.cancelled,
            )
            .and_then(|json_body| {
                on_event(AiStreamEvent::Started {
                    request_id: request.request_id.clone(),
                    provider: profile.kind,
                    model: model_name.clone(),
                });
                match parse_non_streaming_response(profile.kind, &json_body) {
                    Ok(parsed_text) => {
                        if !parsed_text.is_empty() {
                            on_event(AiStreamEvent::TextDelta {
                                request_id: request.request_id.clone(),
                                delta: parsed_text.clone(),
                            });
                        }
                        full_text = parsed_text;
                        Ok(())
                    }
                    Err(err) => {
                        sse_error = Some(err);
                        Err(AiHttpError::Protocol("json_parse_error".into()))
                    }
                }
            })
        };

        if let Err(http_err) = transport_result {
            if let Some(err) = sse_error {
                on_event(AiStreamEvent::Failed {
                    request_id: request.request_id.clone(),
                    error: err.clone(),
                });
                return Err(err);
            }
            if matches!(http_err, AiHttpError::Cancelled) || cancel_token.is_cancelled() {
                on_event(AiStreamEvent::Cancelled {
                    request_id: request.request_id.clone(),
                });
                return Err(AiConnectionError::Cancelled);
            }

            let retryable = match &http_err {
                AiHttpError::RateLimited {
                    retry_after_ms,
                    body,
                } => Some((429u16, *retry_after_ms, body.as_str())),
                AiHttpError::HttpStatus {
                    status,
                    retry_after_ms,
                    body,
                } if (500..600).contains(status) => Some((*status, *retry_after_ms, body.as_str())),
                _ => None,
            };

            if let Some((status, retry_after_ms, raw_body)) = retryable {
                let message = if raw_body.trim().is_empty() {
                    format!("HTTP {status}")
                } else {
                    redact_sensitive_tokens(raw_body.trim())
                };
                if status == 429 {
                    on_event(AiStreamEvent::RateLimited {
                        request_id: request.request_id.clone(),
                        retry_after_ms,
                        message: message.clone(),
                    });
                }
                if attempt < profile.max_retries {
                    attempt += 1;
                    let delay_ms = retry_after_ms
                        .unwrap_or(50 * u64::from(attempt))
                        .min(30_000);
                    on_event(AiStreamEvent::RetryScheduled {
                        request_id: request.request_id.clone(),
                        attempt,
                        delay_ms,
                        reason: format!("HTTP {status}"),
                    });
                    let deadline = Instant::now() + Duration::from_millis(delay_ms);
                    while Instant::now() < deadline {
                        if cancel_token.is_cancelled() {
                            on_event(AiStreamEvent::Cancelled {
                                request_id: request.request_id.clone(),
                            });
                            return Err(AiConnectionError::Cancelled);
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    continue;
                }
                let err = if status == 429 {
                    AiConnectionError::RateLimited {
                        retry_after_ms,
                        message,
                    }
                } else {
                    AiConnectionError::HttpStatus { status, message }
                };
                on_event(AiStreamEvent::Failed {
                    request_id: request.request_id.clone(),
                    error: err.clone(),
                });
                return Err(err);
            }

            let err = map_ai_http_error(http_err, profile.timeout_ms);
            on_event(AiStreamEvent::Failed {
                request_id: request.request_id.clone(),
                error: err.clone(),
            });
            return Err(err);
        }

        let proposals = parse_untrusted_proposals(&full_text);
        for proposal in &proposals {
            on_event(AiStreamEvent::ProposalReady {
                request_id: request.request_id.clone(),
                proposal: proposal.clone(),
            });
        }
        on_event(AiStreamEvent::Completed {
            request_id: request.request_id.clone(),
            full_text,
            proposals: proposals.clone(),
        });
        return Ok(proposals);
    }
}

/// Background worker handle for cancellable streaming AI requests.
pub struct AiConnectionWorker {
    events: mpsc::Receiver<AiStreamEvent>,
    cancel_token: CancellationToken,
    handle: Option<JoinHandle<()>>,
}

impl AiConnectionWorker {
    pub fn spawn(
        profile: AiProviderProfile,
        request: AiPromptRequest,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel_token = CancellationToken::new();
        let worker_cancel = cancel_token.clone();
        #[cfg(test)]
        let inherited_override = current_loopback_http_override();
        let handle = std::thread::Builder::new()
            .name("neoflash-ai-connection".into())
            .spawn(move || {
                let run = || {
                    let _ = run_streaming_request(
                        &profile,
                        &request,
                        secret_store.as_ref(),
                        &worker_cancel,
                        |event| {
                            let _ = tx.send(event);
                        },
                    );
                };
                #[cfg(test)]
                if let Some(origin) = inherited_override.as_deref() {
                    with_test_loopback_http_override(origin, run);
                    return;
                }
                run();
            })
            .ok();
        Self {
            events: rx,
            cancel_token,
            handle,
        }
    }

    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancel_token.clone()
    }

    pub fn drain_events(&self) -> Vec<AiStreamEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.events.try_recv() {
            out.push(ev);
        }
        out
    }
}

impl Drop for AiConnectionWorker {
    fn drop(&mut self) {
        self.cancel_token.cancel();
        if let Some(handle) = self.handle.take() {
            // A socket read may remain blocked until its configured timeout. Dropping a UI-owned
            // worker must never freeze the editor while waiting for that timeout; finished workers
            // are joined, while an in-flight one is safely detached and observes cancellation at
            // the next bounded I/O boundary.
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{Element, ResourceUnit, Transform};
    use std::io::Write;
    use std::net::TcpListener;

    fn sample_context_pack() -> AiContextPack {
        let project = Project::blank();
        let root_id = project.root.clone();
        AiContextPack::from_editor_selection(
            &project,
            &root_id,
            0,
            &[],
            Some(("stop();", ScriptLanguage::ActionScript3)),
        )
    }

    fn drain_http_request_with_headers(socket: &std::net::TcpStream) -> (String, Vec<String>) {
        // Windows resets a connection that closes with unread request bytes. Test
        // servers must consume the complete HTTP request before sending a short response.
        let mut reader = BufReader::new(socket.try_clone().unwrap());
        let mut request_line = String::new();
        assert_ne!(reader.read_line(&mut request_line).unwrap(), 0);
        let mut headers = Vec::new();
        let mut content_length = 0usize;
        loop {
            let mut line = String::new();
            assert_ne!(reader.read_line(&mut line).unwrap(), 0);
            if line == "\r\n" || line == "\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                content_length = value.trim().parse().unwrap();
            }
            headers.push(line.trim().to_owned());
        }
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body).unwrap();
        (request_line, headers)
    }

    fn drain_http_request(socket: &std::net::TcpStream) {
        let _ = drain_http_request_with_headers(socket);
    }

    #[test]
    fn all_four_provider_adapters_stream_deltas_and_emit_untrusted_proposals_without_mutating_project()
     {
        let project_before = Project::demo();
        let project_snapshot = project_before.clone();

        for kind in [
            AiProviderKind::OpenAi,
            AiProviderKind::LocalOpenAiCompatible,
            AiProviderKind::Anthropic,
            AiProviderKind::Gemini,
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let expected_kind = kind;

            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                let (request_line, headers) = drain_http_request_with_headers(&socket);

                match expected_kind {
                    AiProviderKind::OpenAi => {
                        assert!(request_line.starts_with("POST /v1/chat/completions "));
                        assert!(headers.iter().any(|h| {
                            h.split_once(':').is_some_and(|(k, v)| {
                                k.eq_ignore_ascii_case("authorization")
                                    && v.trim() == "Bearer sk-test-openai-secret-12345"
                            })
                        }));
                    }
                    AiProviderKind::LocalOpenAiCompatible => {
                        assert!(request_line.starts_with("POST /v1/chat/completions "));
                    }
                    AiProviderKind::Anthropic => {
                        assert!(request_line.starts_with("POST /v1/messages "));
                        assert!(headers.iter().any(|h| {
                            h.split_once(':').is_some_and(|(k, v)| {
                                k.eq_ignore_ascii_case("x-api-key")
                                    && v.trim() == "sk-ant-secret-key-67890"
                            })
                        }));
                        assert!(headers.iter().any(|h| {
                            h.split_once(':').is_some_and(|(k, v)| {
                                k.eq_ignore_ascii_case("anthropic-version")
                                    && v.trim() == "2023-06-01"
                            })
                        }));
                    }
                    AiProviderKind::Gemini => {
                        assert!(
                            request_line.starts_with(
                                "POST /v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse "
                            ),
                            "unexpected Gemini request line: {request_line}"
                        );
                        assert!(
                            !request_line.contains("AIza"),
                            "Gemini API key must never appear in request URI: {request_line}"
                        );
                        assert!(headers.iter().any(|h| {
                            h.split_once(':').is_some_and(|(k, v)| {
                                k.eq_ignore_ascii_case("x-goog-api-key")
                                    && v.trim() == "AIzaSyTestGeminiKey123456789"
                            })
                        }));
                    }
                }

                let sse_payload = match expected_kind {
                    AiProviderKind::OpenAi | AiProviderKind::LocalOpenAiCompatible => concat!(
                        "data: {\"choices\":[{\"delta\":{\"content\":\"```as3\\n\"}}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{\"content\":\"gotoAndPlay(2);\\n```\"}}]}\n\n",
                        "data: [DONE]\n\n"
                    ),
                    AiProviderKind::Anthropic => concat!(
                        "event: content_block_delta\n",
                        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"```ts\\nthis.stop();\\n```\"}}\n\n",
                        "event: message_stop\n",
                        "data: {\"type\":\"message_stop\"}\n\n"
                    ),
                    AiProviderKind::Gemini => concat!(
                        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"```json\\n{\\\"proposal_kind\\\":\\\"keyframe_timing_proposal\\\",\\\"layer_name\\\":\\\"Layer 1\\\",\\\"start_frame\\\":1,\\\"end_frame\\\":12,\\\"easing_hint\\\":\\\"ease_out\\\",\\\"notes\\\":\\\"Smooth deceleration\\\"}\\n```\"}]}}]}\n\n",
                        "data: [DONE]\n\n"
                    ),
                };

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{sse_payload}"
                );
                socket.write_all(response.as_bytes()).unwrap();
                socket.flush().unwrap();
            });

            let store = MemorySecretStore::new();
            match kind {
                AiProviderKind::OpenAi => {
                    store
                        .set_secret("prof", "sk-test-openai-secret-12345")
                        .unwrap();
                }
                AiProviderKind::Anthropic => {
                    store.set_secret("prof", "sk-ant-secret-key-67890").unwrap();
                }
                AiProviderKind::Gemini => {
                    store
                        .set_secret("prof", "AIzaSyTestGeminiKey123456789")
                        .unwrap();
                }
                AiProviderKind::LocalOpenAiCompatible => {}
            }

            let model = if kind == AiProviderKind::Gemini {
                "gemini-2.5-flash"
            } else {
                "test-model"
            };
            let profile = AiProviderProfile::new(
                "prof",
                "Test Provider",
                kind,
                format!("http://{addr}"),
                model,
            );
            let request = AiPromptRequest {
                request_id: format!("req-{kind:?}"),
                profile_id: "prof".into(),
                model: None,
                system_prompt: "Return structured proposals only.".into(),
                user_prompt: "Suggest a snippet or timing.".into(),
                context_pack: sample_context_pack(),
                temperature: Some(0.2),
                max_output_tokens: 256,
                stream: true,
            };

            let mut events = Vec::new();
            let cancel = CancellationToken::new();
            let origin = format!("http://{addr}");
            let proposals = if kind == AiProviderKind::LocalOpenAiCompatible {
                run_streaming_request(&profile, &request, &store, &cancel, |ev| events.push(ev))
                    .unwrap()
            } else {
                // Without explicit test override, cloud providers reject loopback http://
                assert!(matches!(
                    run_streaming_request(&profile, &request, &store, &cancel, |_| {}),
                    Err(AiConnectionError::InvalidEndpoint { .. })
                ));
                with_test_loopback_http_override(&origin, || {
                    run_streaming_request(&profile, &request, &store, &cancel, |ev| events.push(ev))
                        .unwrap()
                })
            };

            server.join().unwrap();
            assert!(!proposals.is_empty());
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e, AiStreamEvent::Started { .. }))
            );
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e, AiStreamEvent::TextDelta { .. }))
            );
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e, AiStreamEvent::ProposalReady { .. }))
            );
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e, AiStreamEvent::Completed { .. }))
            );

            match kind {
                AiProviderKind::OpenAi | AiProviderKind::LocalOpenAiCompatible => {
                    assert_eq!(
                        proposals[0],
                        AiUntrustedProposal::CodeSnippetProposal {
                            language: ScriptLanguage::ActionScript3,
                            target_hint: "active_script".into(),
                            code: "gotoAndPlay(2);".into(),
                            explanation: "Proposed code snippet (requires user confirmation)"
                                .into(),
                        }
                    );
                }
                AiProviderKind::Anthropic => {
                    assert_eq!(
                        proposals[0],
                        AiUntrustedProposal::CodeSnippetProposal {
                            language: ScriptLanguage::TypeScript,
                            target_hint: "active_script".into(),
                            code: "this.stop();".into(),
                            explanation: "Proposed code snippet (requires user confirmation)"
                                .into(),
                        }
                    );
                }
                AiProviderKind::Gemini => {
                    assert_eq!(
                        proposals[0],
                        AiUntrustedProposal::KeyframeTimingProposal {
                            layer_name: "Layer 1".into(),
                            start_frame: 1,
                            end_frame: 12,
                            easing_hint: "ease_out".into(),
                            notes: "Smooth deceleration".into(),
                        }
                    );
                }
            }
        }

        // Verify that no autonomous mutation occurred on the project.
        assert_eq!(project_before, project_snapshot);
    }

    #[test]
    fn chunked_transport_decodes_split_sse_lines_error_bodies_and_rejects_bad_framing() {
        let payload = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"chunked answer\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let split = payload.find("content").unwrap() + 4;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            drain_http_request(&socket);
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            for chunk in [&payload.as_bytes()[..split], &payload.as_bytes()[split..]] {
                write!(socket, "{:X};test=yes\r\n", chunk.len()).unwrap();
                socket.write_all(chunk).unwrap();
                socket.write_all(b"\r\n").unwrap();
            }
            socket
                .write_all(b"0\r\nX-NeoFlash-Test: complete\r\n\r\n")
                .unwrap();
            socket.flush().unwrap();
        });

        let store = MemorySecretStore::new();
        let profile = AiProviderProfile::new(
            "chunked",
            "Chunked",
            AiProviderKind::LocalOpenAiCompatible,
            format!("http://{addr}"),
            "local-model",
        );
        let request = AiPromptRequest {
            request_id: "req-chunked".into(),
            profile_id: "chunked".into(),
            model: None,
            system_prompt: String::new(),
            user_prompt: "Ping".into(),
            context_pack: sample_context_pack(),
            temperature: None,
            max_output_tokens: 64,
            stream: true,
        };
        let mut events = Vec::new();
        run_streaming_request(
            &profile,
            &request,
            &store,
            &CancellationToken::new(),
            |event| events.push(event),
        )
        .unwrap();
        server.join().unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            AiStreamEvent::TextDelta { delta, .. } if delta == "chunked answer"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AiStreamEvent::Completed { full_text, .. } if full_text == "chunked answer"
        )));

        let error_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let error_addr = error_listener.local_addr().unwrap();
        let error_server = std::thread::spawn(move || {
            let (mut socket, _) = error_listener.accept().unwrap();
            drain_http_request(&socket);
            socket
                .write_all(
                    b"HTTP/1.1 503 Service Unavailable\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n9\r\ntemporari\r\nE\r\nly unavailable\r\n0\r\n\r\n",
                )
                .unwrap();
            socket.flush().unwrap();
        });
        let mut error_profile = profile.clone();
        error_profile.base_url = format!("http://{error_addr}");
        error_profile.max_retries = 0;
        let error = run_streaming_request(
            &error_profile,
            &request,
            &store,
            &CancellationToken::new(),
            |_| {},
        )
        .unwrap_err();
        error_server.join().unwrap();
        assert_eq!(
            error,
            AiConnectionError::HttpStatus {
                status: 503,
                message: "temporarily unavailable".into(),
            }
        );

        let mut valid = ChunkedBodyReader::new(BufReader::new(
            b"4\r\nWiki\r\n5;extension=ok\r\npedia\r\n0\r\nTrailer: value\r\n\r\n".as_slice(),
        ));
        let mut decoded = String::new();
        valid.read_to_string(&mut decoded).unwrap();
        assert_eq!(decoded, "Wikipedia");

        let mut bad_delimiter =
            ChunkedBodyReader::new(BufReader::new(b"1\r\naX\r\n0\r\n\r\n".as_slice()));
        let mut bytes = Vec::new();
        let delimiter_error = bad_delimiter.read_to_end(&mut bytes).unwrap_err();
        assert_eq!(delimiter_error.kind(), std::io::ErrorKind::InvalidData);

        let oversized = format!("{:X}\r\n", MAX_AI_HTTP_CHUNK_BYTES + 1);
        let mut oversized_reader = ChunkedBodyReader::new(BufReader::new(oversized.as_bytes()));
        let oversized_error = oversized_reader.read_to_end(&mut Vec::new()).unwrap_err();
        assert_eq!(oversized_error.kind(), std::io::ErrorKind::InvalidData);

        assert!(matches!(
            response_uses_chunked_transfer(&[("Transfer-Encoding".into(), "gzip, chunked".into())]),
            Err(AiConnectionError::Protocol { .. })
        ));
        assert!(matches!(
            response_uses_chunked_transfer(&[(
                "Transfer-Encoding".into(),
                "chunked, chunked".into()
            )]),
            Err(AiConnectionError::Protocol { .. })
        ));
    }

    #[test]
    fn streaming_response_limit_counts_ignored_sse_bytes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line == "\n" || line.is_empty() {
                    break;
                }
                if let Some(value) = line
                    .split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .map(|(_, value)| value.trim())
                {
                    content_length = value.parse().unwrap();
                }
            }
            let mut request_body = vec![0u8; content_length];
            reader.read_exact(&mut request_body).unwrap();
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            let ignored_line = format!(":{}\n", "x".repeat(64 * 1024));
            let line_count = MAX_AI_RESPONSE_BYTES / ignored_line.len() + 2;
            for _ in 0..line_count {
                if socket.write_all(ignored_line.as_bytes()).is_err() {
                    break;
                }
            }
        });

        let store = MemorySecretStore::new();
        let profile = AiProviderProfile::new(
            "bounded-ignored-sse",
            "Bounded ignored SSE",
            AiProviderKind::LocalOpenAiCompatible,
            format!("http://{addr}"),
            "local-model",
        );
        let request = AiPromptRequest {
            request_id: "req-bounded-ignored-sse".into(),
            profile_id: profile.id.clone(),
            model: None,
            system_prompt: String::new(),
            user_prompt: "Ping".into(),
            context_pack: sample_context_pack(),
            temperature: None,
            max_output_tokens: 64,
            stream: true,
        };
        let mut events = Vec::new();
        let error = run_streaming_request(
            &profile,
            &request,
            &store,
            &CancellationToken::new(),
            |event| events.push(event),
        )
        .unwrap_err();
        server.join().unwrap();
        assert!(
            matches!(
                &error,
                AiConnectionError::Protocol { message }
                    if message.contains("response exceeds the 4 MiB limit")
            ),
            "{error:?}"
        );
        assert!(events.iter().any(|event| matches!(
            event,
            AiStreamEvent::Failed { error: AiConnectionError::Protocol { message }, .. }
                if message.contains("response exceeds the 4 MiB limit")
        )));
    }

    #[test]
    fn mid_stream_cancellation_rate_limit_retry_timeout_and_offline_handling() {
        // 1. HTTP 429 rate-limit with Retry-After recovery on attempt 2
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            // First attempt -> 429 Too Many Requests
            {
                let (mut s1, _) = listener.accept().unwrap();
                drain_http_request(&s1);
                s1.write_all(
                    b"HTTP/1.1 429 Too Many Requests\r\nRetry-After-Ms: 10\r\nConnection: close\r\n\r\nRate limit exceeded",
                )
                .unwrap();
                s1.flush().unwrap();
            }

            // Second attempt -> 200 OK
            let (mut s2, _) = listener.accept().unwrap();
            drain_http_request(&s2);
            s2.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"Recovered after 429\"}}]}\n\ndata: [DONE]\n\n",
            )
            .unwrap();
            s2.flush().unwrap();
        });

        let store = MemorySecretStore::new();
        let mut profile = AiProviderProfile::new(
            "local",
            "Local",
            AiProviderKind::LocalOpenAiCompatible,
            format!("http://{addr}"),
            "qwen",
        );
        profile.max_retries = 2;
        let req = AiPromptRequest {
            request_id: "req-retry".into(),
            profile_id: "local".into(),
            model: None,
            system_prompt: String::new(),
            user_prompt: "Ping".into(),
            context_pack: sample_context_pack(),
            temperature: None,
            max_output_tokens: 64,
            stream: true,
        };
        let mut events = Vec::new();
        let proposals =
            run_streaming_request(&profile, &req, &store, &CancellationToken::new(), |ev| {
                events.push(ev)
            })
            .unwrap();
        server.join().unwrap();
        assert!(events.iter().any(|e| matches!(
            e,
            AiStreamEvent::RateLimited {
                retry_after_ms: Some(10),
                ..
            }
        )));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, AiStreamEvent::RetryScheduled { attempt: 1, .. }))
        );
        assert_eq!(proposals.len(), 1);

        // 2. Mid-stream cancellation via CancellationToken
        let cancel_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let cancel_addr = cancel_listener.local_addr().unwrap();
        let cancel_server = std::thread::spawn(move || {
            let (mut s, _) = cancel_listener.accept().unwrap();
            drain_http_request(&s);
            s.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"chunk-1\"}}]}\n\n",
            )
            .unwrap();
            s.flush().unwrap();
            std::thread::sleep(Duration::from_millis(80));
            let _ = s.write_all(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"chunk-2\"}}]}\n\ndata: [DONE]\n\n",
            );
            let _ = s.flush();
        });

        let cancel_profile = AiProviderProfile::new(
            "local-cancel",
            "Local",
            AiProviderKind::LocalOpenAiCompatible,
            format!("http://{cancel_addr}"),
            "qwen",
        );
        let token = CancellationToken::new();
        let token_clone = token.clone();
        let mut cancel_events = Vec::new();
        let res = run_streaming_request(&cancel_profile, &req, &store, &token, |ev| {
            if matches!(ev, AiStreamEvent::TextDelta { .. }) {
                token_clone.cancel();
            }
            cancel_events.push(ev);
        });
        cancel_server.join().unwrap();
        assert_eq!(res, Err(AiConnectionError::Cancelled));
        assert!(
            cancel_events
                .iter()
                .any(|e| matches!(e, AiStreamEvent::Cancelled { .. }))
        );

        // 3. Timeout handling
        let timeout_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let timeout_addr = timeout_listener.local_addr().unwrap();
        let timeout_server = std::thread::spawn(move || {
            let (_s, _) = timeout_listener.accept().unwrap();
            std::thread::sleep(Duration::from_millis(180));
        });
        let mut timeout_profile = AiProviderProfile::new(
            "local-timeout",
            "Local",
            AiProviderKind::LocalOpenAiCompatible,
            format!("http://{timeout_addr}"),
            "qwen",
        );
        timeout_profile.timeout_ms = 40;
        timeout_profile.max_retries = 0;
        let timeout_err = run_streaming_request(
            &timeout_profile,
            &req,
            &store,
            &CancellationToken::new(),
            |_| {},
        )
        .unwrap_err();
        timeout_server.join().unwrap();
        assert!(matches!(timeout_err, AiConnectionError::Timeout { .. }));

        // 4. Offline / connection refused handling
        let closed_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_addr = closed_listener.local_addr().unwrap();
        drop(closed_listener);
        let offline_profile = AiProviderProfile::new(
            "local-offline",
            "Local",
            AiProviderKind::LocalOpenAiCompatible,
            format!("http://{closed_addr}"),
            "qwen",
        );
        let offline_err = test_provider_connection(&offline_profile, &store).unwrap_err();
        assert!(matches!(offline_err, AiConnectionError::Offline { .. }));
    }

    #[test]
    fn zero_secret_leakage_and_bounded_context_pack_redaction() {
        let raw_secret = "sk-proj-SUPERSECRETKEY1234567890";
        let gemini_secret = "AIzaSySecretTokenValue987654321";

        let store = MemorySecretStore::new();
        store.set_secret("openai-main", raw_secret).unwrap();

        // 1. Debug formatting of MemorySecretStore and FormattedProviderRequest never leaks secrets
        let store_dbg = format!("{store:?}");
        assert!(!store_dbg.contains(raw_secret), "{store_dbg}");

        let profile = AiProviderProfile::new(
            "openai-main",
            "OpenAI Main",
            AiProviderKind::OpenAi,
            "http://127.0.0.1:8080",
            "gpt-4.1",
        );
        let profile_json = serde_json::to_string(&profile).unwrap();
        assert!(!profile_json.contains(raw_secret));

        // Stray secret fields in JSON are rejected by #[serde(deny_unknown_fields)]
        let malicious_json = r#"{
            "id": "openai-main",
            "display_name": "OpenAI",
            "kind": "open_ai",
            "base_url": "http://127.0.0.1:8080",
            "default_model": "gpt-4.1",
            "timeout_ms": 5000,
            "max_retries": 1,
            "capabilities": { "streaming": true, "vision_input": false, "structured_json_proposal": true },
            "api_key": "sk-proj-SUPERSECRETKEY1234567890"
        }"#;
        assert!(serde_json::from_str::<AiProviderProfile>(malicious_json).is_err());

        // 2. Context pack enforces MAX_SCRIPT_BYTES on UTF-8 char boundaries, MIME whitelist,
        // MAX_ASSET_BYTES, and redacts sensitive tokens.
        let mut project = Project::blank();
        let root_id = project.root.clone();
        let allowed_png =
            ResourceUnit::from_bytes("res-png", "sprite.png", "image/png", &[1, 2, 3, 4]).unwrap();
        let disallowed_bin = ResourceUnit::from_bytes(
            "res-exe",
            "payload.bin",
            "application/octet-stream",
            &[9, 9, 9],
        )
        .unwrap();
        let oversized_png = ResourceUnit::from_bytes(
            "res-huge",
            "huge.png",
            "image/png",
            &vec![0u8; MAX_ASSET_BYTES + 1024],
        )
        .unwrap();
        project.resources.push(allowed_png);
        project.resources.push(disallowed_bin);
        project.resources.push(oversized_png);

        project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "el-1".into(),
                name: format!("hero_{raw_secret}"),
                transform: Transform {
                    x: 42.0,
                    y: 84.0,
                    ..Default::default()
                },
                kind: ElementKind::Bitmap {
                    resource_id: "res-png".into(),
                    width: 16,
                    height: 16,
                    smoothing: false,
                },
                ..Default::default()
            });

        let mut long_script = format!(
            "// Token: {raw_secret} and {gemini_secret} and Bearer secret_bearer_token_999\n"
        );
        while long_script.len() <= MAX_SCRIPT_BYTES + 500 {
            long_script.push_str("trace(\"éüöß\");\n");
        }

        let pack = AiContextPack::from_editor_selection(
            &project,
            &root_id,
            0,
            &[
                "el-1".into(),
                "res-png".into(),
                "res-exe".into(),
                "res-huge".into(),
            ],
            Some((&long_script, ScriptLanguage::ActionScript3)),
        );

        let pack_json = serde_json::to_string(&pack).unwrap();
        assert!(!pack_json.contains(raw_secret), "{pack_json}");
        assert!(!pack_json.contains(gemini_secret), "{pack_json}");
        assert!(
            !pack_json.contains("secret_bearer_token_999"),
            "{pack_json}"
        );

        let excerpt = pack.script_excerpt.as_ref().unwrap();
        assert!(excerpt.truncated);
        assert!(excerpt.source.len() <= MAX_SCRIPT_BYTES);

        let png_meta = pack
            .selected_assets
            .iter()
            .find(|a| a.resource_id == "res-png")
            .unwrap();
        assert!(png_meta.included);
        assert!(png_meta.exclusion_reason.is_none());

        let bin_meta = pack
            .selected_assets
            .iter()
            .find(|a| a.resource_id == "res-exe")
            .unwrap();
        assert!(!bin_meta.included);
        assert!(
            bin_meta
                .exclusion_reason
                .as_deref()
                .unwrap()
                .contains("whitelist")
        );

        let huge_meta = pack
            .selected_assets
            .iter()
            .find(|a| a.resource_id == "res-huge")
            .unwrap();
        assert!(!huge_meta.included);
        assert!(
            huge_meta
                .exclusion_reason
                .as_deref()
                .unwrap()
                .contains("MAX_ASSET_BYTES")
        );

        let req = AiPromptRequest {
            request_id: "req-sec".into(),
            profile_id: "openai-main".into(),
            model: None,
            system_prompt: format!("Bearer {raw_secret}"),
            user_prompt: format!("api_key={gemini_secret}"),
            context_pack: pack,
            temperature: None,
            max_output_tokens: 128,
            stream: true,
        };
        let formatted = format_provider_request(&profile, &req, Some(raw_secret)).unwrap();
        let formatted_dbg = format!("{formatted:?}");
        assert!(!formatted_dbg.contains(raw_secret), "{formatted_dbg}");
        assert!(!formatted_dbg.contains(gemini_secret), "{formatted_dbg}");
        assert!(!formatted.body.contains(raw_secret));
        assert!(!formatted.body.contains(gemini_secret));
    }

    #[test]
    fn provider_endpoints_headers_and_response_memory_are_fail_closed() {
        let request = AiPromptRequest {
            request_id: "req-hardening".into(),
            profile_id: "provider".into(),
            model: None,
            system_prompt: "Keep edits reviewable.".into(),
            user_prompt: "Explain the selected frame.".into(),
            context_pack: sample_context_pack(),
            temperature: None,
            max_output_tokens: 64,
            stream: true,
        };

        let injected_endpoint = AiProviderProfile::new(
            "provider",
            "Injected",
            AiProviderKind::LocalOpenAiCompatible,
            "http://127.0.0.1:8080\r\nX-Injected: yes",
            "local-model",
        );
        assert!(matches!(
            format_provider_request(&injected_endpoint, &request, None),
            Err(AiConnectionError::InvalidEndpoint { .. })
        ));

        let local_profile = AiProviderProfile::new(
            "provider",
            "Local",
            AiProviderKind::LocalOpenAiCompatible,
            "http://127.0.0.1:8080",
            "local-model",
        );
        assert!(matches!(
            format_provider_request(&local_profile, &request, Some("token\r\nX-Leak: yes")),
            Err(AiConnectionError::InvalidEndpoint { .. })
        ));

        let cloud_profile = AiProviderProfile::new(
            "provider",
            "Cloud",
            AiProviderKind::OpenAi,
            "http://example.com",
            "gpt-4.1",
        );
        let store = MemorySecretStore::new();
        store.set_secret("provider", "sk-test-safe-value").unwrap();
        assert!(matches!(
            test_provider_connection(&cloud_profile, &store),
            Err(AiConnectionError::InvalidEndpoint { .. })
        ));

        let no_secret_remote = parse_http_url("http://192.0.2.1:8080").unwrap();
        assert!(matches!(
            validate_plaintext_transport(&no_secret_remote, "192.0.2.1:8080".parse().unwrap()),
            Err(AiConnectionError::InvalidEndpoint { .. })
        ));
        let localhost = parse_http_url("http://localhost:8080").unwrap();
        assert!(matches!(
            validate_plaintext_transport(&localhost, "192.0.2.1:8080".parse().unwrap()),
            Err(AiConnectionError::InvalidEndpoint { .. })
        ));
        assert!(
            validate_plaintext_transport(&localhost, "127.0.0.1:8080".parse().unwrap()).is_ok()
        );

        let https_profile = AiProviderProfile::new(
            "provider",
            "HTTPS",
            AiProviderKind::OpenAi,
            "https://api.example.com",
            "gpt-4.1",
        );
        assert!(validate_profile_endpoint(&https_profile).is_ok());

        for cloud_kind in [
            AiProviderKind::OpenAi,
            AiProviderKind::Anthropic,
            AiProviderKind::Gemini,
        ] {
            let loopback_cloud = AiProviderProfile::new(
                "provider",
                "Loopback Cloud",
                cloud_kind,
                "http://127.0.0.1:8080",
                "model-1",
            );
            assert!(matches!(
                test_provider_connection(&loopback_cloud, &store),
                Err(AiConnectionError::InvalidEndpoint { .. })
            ));
        }

        let mut gemini_request = request.clone();
        gemini_request.model = Some("gemini-safe\r\nInjected".into());
        let gemini_profile = AiProviderProfile::new(
            "provider",
            "Gemini",
            AiProviderKind::Gemini,
            "http://127.0.0.1:8080",
            "gemini-2.5-pro",
        );
        assert!(matches!(
            format_provider_request(&gemini_profile, &gemini_request, Some("safe-token")),
            Err(AiConnectionError::InvalidEndpoint { .. })
        ));

        let oversized_line = vec![b'a'; MAX_AI_HTTP_LINE_BYTES + 1];
        let mut line_reader = BufReader::new(oversized_line.as_slice());
        let line_error = read_bounded_line(&mut line_reader, MAX_AI_HTTP_LINE_BYTES).unwrap_err();
        assert_eq!(line_error.kind(), std::io::ErrorKind::InvalidData);

        let oversized_error = vec![b'e'; MAX_AI_ERROR_BYTES + 100];
        let mut error_reader = oversized_error.as_slice();
        let diagnostic = read_error_body_bounded(&mut error_reader).unwrap();
        assert!(diagnostic.len() < MAX_AI_ERROR_BYTES + 100);
        assert!(diagnostic.ends_with("[diagnostic body truncated]"));
    }
}
