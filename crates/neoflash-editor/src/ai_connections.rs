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
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Maximum UTF-8 byte length of a script excerpt included in an [`AiContextPack`].
pub const MAX_SCRIPT_BYTES: usize = 16_384;

/// Maximum decoded byte size of an asset whose metadata can be marked eligible in an [`AiContextPack`].
pub const MAX_ASSET_BYTES: usize = 65_536;

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
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Gemini => "Google Gemini",
            Self::LocalOpenAiCompatible => "Local (OpenAI-compatible)",
        }
    }

    pub const fn requires_secret(self) -> bool {
        match self {
            Self::OpenAi | Self::Anthropic | Self::Gemini => true,
            Self::LocalOpenAiCompatible => false,
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
        } else {
            // Exit code 44 in `/usr/bin/security` indicates item not found.
            Ok(None)
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
        let _ = std::process::Command::new("/usr/bin/security")
            .args([
                "delete-generic-password",
                "-s",
                &self.service,
                "-a",
                profile_id,
            ])
            .output();
        Ok(())
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
        let ctx_json = serde_json::to_string(&self.context_pack).unwrap_or_default();
        format!(
            "{}\n\n[NeoFlash Selection Context]\n{}",
            redact_sensitive_tokens(&self.user_prompt),
            ctx_json
        )
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

    let model = request
        .model
        .as_deref()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or(&profile.default_model);
    let system_text = redact_sensitive_tokens(&request.system_prompt);
    let user_text = request.compose_user_message();
    let base_path = extract_base_path(&profile.base_url)?;

    match profile.kind {
        AiProviderKind::OpenAi | AiProviderKind::LocalOpenAiCompatible => {
            let path_and_query = join_endpoint_path(&base_path, "/v1/chat/completions");
            let mut headers = vec![
                ("Content-Type".to_owned(), "application/json".to_owned()),
                ("Accept".to_owned(), "text/event-stream".to_owned()),
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
                ("Accept".to_owned(), "text/event-stream".to_owned()),
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
                ("Accept".to_owned(), "text/event-stream".to_owned()),
                ("x-goog-api-key".to_owned(), tok.to_owned()),
            ];
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
                "generationConfig": {
                    "maxOutputTokens": request.max_output_tokens,
                }
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

struct ParsedUrl {
    host: String,
    port: u16,
}

fn parse_http_url(base_url: &str) -> Result<ParsedUrl, AiConnectionError> {
    let trimmed = base_url.trim();
    let rest = if let Some(r) = trimmed.strip_prefix("http://") {
        r
    } else if trimmed.starts_with("https://") {
        return Err(AiConnectionError::InvalidEndpoint {
            reason: "Direct TcpStream transport requires an http:// endpoint or local proxy."
                .into(),
        });
    } else {
        return Err(AiConnectionError::InvalidEndpoint {
            reason: format!("URL must start with http:// (got '{trimmed}')"),
        });
    };
    let authority = rest.split('/').next().unwrap_or("");
    if authority.is_empty() {
        return Err(AiConnectionError::InvalidEndpoint {
            reason: "Missing host in base_url".into(),
        });
    }
    let (host, port) = if let Some((h, p)) = authority.rsplit_once(':') {
        let port = p
            .parse::<u16>()
            .map_err(|_| AiConnectionError::InvalidEndpoint {
                reason: format!("Invalid port in '{authority}'"),
            })?;
        (h.to_owned(), port)
    } else {
        (authority.to_owned(), 80)
    };
    Ok(ParsedUrl { host, port })
}

fn extract_base_path(base_url: &str) -> Result<String, AiConnectionError> {
    let trimmed = base_url.trim().trim_end_matches('/');
    let rest = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .ok_or_else(|| AiConnectionError::InvalidEndpoint {
            reason: format!("URL must start with http:// or https:// (got '{trimmed}')"),
        })?;
    if let Some((_, path)) = rest.split_once('/') {
        Ok(format!("/{}", path.trim_matches('/')))
    } else {
        Ok(String::new())
    }
}

fn join_endpoint_path(base_path: &str, suffix: &str) -> String {
    if base_path.is_empty() || base_path == "/" || suffix.starts_with(base_path) {
        suffix.to_owned()
    } else {
        format!("{base_path}{suffix}")
    }
}

fn resolve_socket_addr(host: &str, port: u16) -> Result<SocketAddr, AiConnectionError> {
    (host, port)
        .to_socket_addrs()
        .map_err(|e| AiConnectionError::Offline {
            reason: format!("Could not resolve {host}:{port}: {e}"),
        })?
        .next()
        .ok_or_else(|| AiConnectionError::Offline {
            reason: format!("No socket address found for {host}:{port}"),
        })
}

fn is_timeout_io_error(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    )
}

fn parse_retry_after_ms(headers: &[(String, String)]) -> Option<u64> {
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("retry-after-ms")
            && let Ok(ms) = v.trim().parse::<u64>()
        {
            return Some(ms);
        }
        if k.eq_ignore_ascii_case("retry-after")
            && let Ok(secs) = v.trim().parse::<u64>()
        {
            return Some(secs.saturating_mul(1_000));
        }
    }
    None
}

/// Test a provider connection profile without mutating project state.
pub fn test_provider_connection(
    profile: &AiProviderProfile,
    secret_store: &dyn SecretStore,
) -> Result<String, AiConnectionError> {
    let secret = secret_store.get_secret(&profile.id)?;
    if profile.kind.requires_secret() && secret.is_none() {
        return Err(AiConnectionError::MissingSecret {
            profile_id: profile.id.clone(),
        });
    }
    let parsed = parse_http_url(&profile.base_url)?;
    let addr = resolve_socket_addr(&parsed.host, parsed.port)?;
    let timeout = Duration::from_millis(profile.timeout_ms.max(50));
    let mut stream = TcpStream::connect_timeout(&addr, timeout).map_err(|e| {
        if is_timeout_io_error(&e) {
            AiConnectionError::Timeout {
                timeout_ms: profile.timeout_ms,
            }
        } else {
            AiConnectionError::Offline {
                reason: e.to_string(),
            }
        }
    })?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let base_path = extract_base_path(&profile.base_url)?;
    let path = join_endpoint_path(&base_path, "/v1/models");
    let mut req = format!(
        "GET {path} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n",
        parsed.host, parsed.port
    );
    if let Some(tok) = secret.as_deref() {
        match profile.kind {
            AiProviderKind::OpenAi | AiProviderKind::LocalOpenAiCompatible => {
                req.push_str(&format!("Authorization: Bearer {tok}\r\n"));
            }
            AiProviderKind::Anthropic => {
                req.push_str(&format!(
                    "x-api-key: {tok}\r\nanthropic-version: 2023-06-01\r\n"
                ));
            }
            AiProviderKind::Gemini => {
                req.push_str(&format!("x-goog-api-key: {tok}\r\n"));
            }
        }
    }
    req.push_str("\r\n");
    stream
        .write_all(req.as_bytes())
        .map_err(|e| AiConnectionError::Offline {
            reason: e.to_string(),
        })?;

    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line).map_err(|e| {
        if is_timeout_io_error(&e) {
            AiConnectionError::Timeout {
                timeout_ms: profile.timeout_ms,
            }
        } else {
            AiConnectionError::Offline {
                reason: e.to_string(),
            }
        }
    })?;
    let status = parse_http_status(&status_line)?;
    if (200..300).contains(&status) {
        Ok(format!(
            "{} ({}) reachable",
            profile.display_name,
            profile.kind.display_name()
        ))
    } else if status == 429 {
        Err(AiConnectionError::RateLimited {
            retry_after_ms: None,
            message: "HTTP 429 Too Many Requests".into(),
        })
    } else {
        Err(AiConnectionError::HttpStatus {
            status,
            message: status_line.trim().to_owned(),
        })
    }
}

fn parse_http_status(status_line: &str) -> Result<u16, AiConnectionError> {
    let mut parts = status_line.split_whitespace();
    let _http = parts.next().ok_or_else(|| AiConnectionError::Protocol {
        message: "Empty HTTP status line".into(),
    })?;
    let code_str = parts.next().ok_or_else(|| AiConnectionError::Protocol {
        message: format!("Malformed HTTP status line: '{}'", status_line.trim()),
    })?;
    code_str
        .parse::<u16>()
        .map_err(|_| AiConnectionError::Protocol {
            message: format!("Invalid HTTP status code: '{code_str}'"),
        })
}

/// Execute a streaming (or non-streaming) AI prompt request with configurable timeout,
/// [`CancellationToken`], and automatic HTTP 429 / 5xx retry handling.
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
    let parsed_url = match parse_http_url(&profile.base_url) {
        Ok(u) => u,
        Err(err) => {
            on_event(AiStreamEvent::Failed {
                request_id: request.request_id.clone(),
                error: err.clone(),
            });
            return Err(err);
        }
    };
    let addr = match resolve_socket_addr(&parsed_url.host, parsed_url.port) {
        Ok(a) => a,
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

        let stream_res = TcpStream::connect_timeout(&addr, timeout);
        let mut stream = match stream_res {
            Ok(s) => s,
            Err(e) => {
                let err = if is_timeout_io_error(&e) {
                    AiConnectionError::Timeout {
                        timeout_ms: profile.timeout_ms,
                    }
                } else {
                    AiConnectionError::Offline {
                        reason: e.to_string(),
                    }
                };
                on_event(AiStreamEvent::Failed {
                    request_id: request.request_id.clone(),
                    error: err.clone(),
                });
                return Err(err);
            }
        };
        let _ = stream.set_read_timeout(Some(timeout));
        let _ = stream.set_write_timeout(Some(timeout));

        let mut raw_http = format!(
            "{} {} HTTP/1.1\r\nHost: {}:{}\r\nContent-Length: {}\r\nConnection: close\r\n",
            formatted.method,
            formatted.path_and_query,
            parsed_url.host,
            parsed_url.port,
            formatted.body.len()
        );
        for (k, v) in &formatted.headers {
            raw_http.push_str(&format!("{k}: {v}\r\n"));
        }
        raw_http.push_str("\r\n");
        raw_http.push_str(&formatted.body);

        if let Err(e) = stream.write_all(raw_http.as_bytes()) {
            let err = if is_timeout_io_error(&e) {
                AiConnectionError::Timeout {
                    timeout_ms: profile.timeout_ms,
                }
            } else {
                AiConnectionError::Offline {
                    reason: e.to_string(),
                }
            };
            on_event(AiStreamEvent::Failed {
                request_id: request.request_id.clone(),
                error: err.clone(),
            });
            return Err(err);
        }

        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        if let Err(e) = reader.read_line(&mut status_line) {
            let err = if is_timeout_io_error(&e) {
                AiConnectionError::Timeout {
                    timeout_ms: profile.timeout_ms,
                }
            } else {
                AiConnectionError::Offline {
                    reason: e.to_string(),
                }
            };
            on_event(AiStreamEvent::Failed {
                request_id: request.request_id.clone(),
                error: err.clone(),
            });
            return Err(err);
        }

        let status = match parse_http_status(&status_line) {
            Ok(s) => s,
            Err(err) => {
                on_event(AiStreamEvent::Failed {
                    request_id: request.request_id.clone(),
                    error: err.clone(),
                });
                return Err(err);
            }
        };

        let mut resp_headers = Vec::new();
        loop {
            let mut hline = String::new();
            match reader.read_line(&mut hline) {
                Ok(0) => break,
                Ok(_) => {
                    if hline == "\r\n" || hline == "\n" {
                        break;
                    }
                    if let Some((k, v)) = hline.split_once(':') {
                        resp_headers.push((k.trim().to_owned(), v.trim().to_owned()));
                    }
                }
                Err(e) => {
                    let err = if is_timeout_io_error(&e) {
                        AiConnectionError::Timeout {
                            timeout_ms: profile.timeout_ms,
                        }
                    } else {
                        AiConnectionError::Offline {
                            reason: e.to_string(),
                        }
                    };
                    on_event(AiStreamEvent::Failed {
                        request_id: request.request_id.clone(),
                        error: err.clone(),
                    });
                    return Err(err);
                }
            }
        }

        if status == 429 || (500..600).contains(&status) {
            let mut err_body = String::new();
            let _ = reader.read_to_string(&mut err_body);
            let retry_after_ms = parse_retry_after_ms(&resp_headers);
            let message = if err_body.trim().is_empty() {
                status_line.trim().to_owned()
            } else {
                redact_sensitive_tokens(err_body.trim())
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
                let delay_ms = retry_after_ms.unwrap_or(20 * u64::from(attempt)).min(250);
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

        if !(200..300).contains(&status) {
            let mut err_body = String::new();
            let _ = reader.read_to_string(&mut err_body);
            let err = AiConnectionError::HttpStatus {
                status,
                message: redact_sensitive_tokens(err_body.trim()),
            };
            on_event(AiStreamEvent::Failed {
                request_id: request.request_id.clone(),
                error: err.clone(),
            });
            return Err(err);
        }

        on_event(AiStreamEvent::Started {
            request_id: request.request_id.clone(),
            provider: profile.kind,
            model: model_name,
        });

        let mut full_text = String::new();
        loop {
            if cancel_token.is_cancelled() {
                on_event(AiStreamEvent::Cancelled {
                    request_id: request.request_id.clone(),
                });
                return Err(AiConnectionError::Cancelled);
            }
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => match parse_sse_line(profile.kind, &line) {
                    Ok(SseParseOutcome::Ignore) => {}
                    Ok(SseParseOutcome::Delta(delta)) => {
                        let safe_delta = redact_sensitive_tokens(&delta);
                        full_text.push_str(&safe_delta);
                        on_event(AiStreamEvent::TextDelta {
                            request_id: request.request_id.clone(),
                            delta: safe_delta,
                        });
                    }
                    Ok(SseParseOutcome::Done) => break,
                    Err(err) => {
                        on_event(AiStreamEvent::Failed {
                            request_id: request.request_id.clone(),
                            error: err.clone(),
                        });
                        return Err(err);
                    }
                },
                Err(e) => {
                    let err = if is_timeout_io_error(&e) {
                        AiConnectionError::Timeout {
                            timeout_ms: profile.timeout_ms,
                        }
                    } else {
                        AiConnectionError::Offline {
                            reason: e.to_string(),
                        }
                    };
                    on_event(AiStreamEvent::Failed {
                        request_id: request.request_id.clone(),
                        error: err.clone(),
                    });
                    return Err(err);
                }
            }
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
        let handle = std::thread::Builder::new()
            .name("neoflash-ai-connection".into())
            .spawn(move || {
                let _ = run_streaming_request(
                    &profile,
                    &request,
                    secret_store.as_ref(),
                    &worker_cancel,
                    |event| {
                        let _ = tx.send(event);
                    },
                );
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
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{Element, ResourceUnit, Transform};
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
                let mut reader = BufReader::new(socket.try_clone().unwrap());
                let mut request_line = String::new();
                reader.read_line(&mut request_line).unwrap();
                let mut headers = Vec::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line == "\n" || line.is_empty() {
                        break;
                    }
                    headers.push(line.trim().to_owned());
                }

                match expected_kind {
                    AiProviderKind::OpenAi => {
                        assert!(request_line.starts_with("POST /v1/chat/completions "));
                        assert!(
                            headers
                                .iter()
                                .any(|h| h == "Authorization: Bearer sk-test-openai-secret-12345")
                        );
                    }
                    AiProviderKind::LocalOpenAiCompatible => {
                        assert!(request_line.starts_with("POST /v1/chat/completions "));
                    }
                    AiProviderKind::Anthropic => {
                        assert!(request_line.starts_with("POST /v1/messages "));
                        assert!(
                            headers
                                .iter()
                                .any(|h| h == "x-api-key: sk-ant-secret-key-67890")
                        );
                        assert!(headers.iter().any(|h| h == "anthropic-version: 2023-06-01"));
                    }
                    AiProviderKind::Gemini => {
                        assert!(
                            request_line.starts_with(
                                "POST /v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse "
                            ),
                            "unexpected Gemini request line: {request_line}"
                        );
                        assert!(
                            headers
                                .iter()
                                .any(|h| h == "x-goog-api-key: AIzaSyTestGeminiKey123456789")
                        );
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
            let proposals =
                run_streaming_request(&profile, &request, &store, &cancel, |ev| events.push(ev))
                    .unwrap();

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
    fn mid_stream_cancellation_rate_limit_retry_timeout_and_offline_handling() {
        // 1. HTTP 429 rate-limit with Retry-After recovery on attempt 2
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            // First attempt -> 429 Too Many Requests
            {
                let (mut s1, _) = listener.accept().unwrap();
                let mut r1 = BufReader::new(s1.try_clone().unwrap());
                let mut line = String::new();
                let _ = r1.read_line(&mut line);
                s1.write_all(
                    b"HTTP/1.1 429 Too Many Requests\r\nRetry-After-Ms: 10\r\nConnection: close\r\n\r\nRate limit exceeded",
                )
                .unwrap();
                let _ = s1.shutdown(std::net::Shutdown::Both);
            }

            // Second attempt -> 200 OK
            let (mut s2, _) = listener.accept().unwrap();
            let mut r2 = BufReader::new(s2.try_clone().unwrap());
            let mut line = String::new();
            let _ = r2.read_line(&mut line);
            s2.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"Recovered after 429\"}}]}\n\ndata: [DONE]\n\n",
            )
            .unwrap();
            let _ = s2.shutdown(std::net::Shutdown::Both);
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
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut line = String::new();
            let _ = r.read_line(&mut line);
            s.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"chunk-1\"}}]}\n\n",
            )
            .unwrap();
            s.flush().unwrap();
            std::thread::sleep(Duration::from_millis(80));
            let _ = s.write_all(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"chunk-2\"}}]}\n\ndata: [DONE]\n\n",
            );
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
}
