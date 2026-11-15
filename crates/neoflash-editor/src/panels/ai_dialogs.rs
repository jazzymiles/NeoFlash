//! AI workflow dialogs (`R-097` & `R-099`):
//! - `Dialog / AI connection` (`480 pt`): add or edit an OpenAI, Anthropic, Google Gemini or
//!   local OpenAI-compatible connection profile and test it without persisting secrets in
//!   `WorkspacePrefs` or `.nflash` documents.
//! - `Dialog / Send to AI` (`560 pt`): mandatory consent review dialog listing the exact
//!   `AiContextPack` sections, omissions, redaction notice and destination host before anything
//!   leaves the computer.
//! - `Dialog / AI answer` (`480 pt` waiting/streaming/failed, `600 pt` with proposals): streaming
//!   progress, `Stop` cancellation, untrusted proposal cards (`Insert into Actions` at caret and
//!   `Apply to timeline` with layer/range validation), and single-step Undo.

use super::dialog_helpers::{DialogKeyboardOutcome, DialogKeyboardState};
use super::planned::{FooterClick, footer_clicked_with_enabled};
use crate::ai_connections::{
    AiConnectionError, AiConnectionManager, AiConnectionWorker, AiContextPack, AiContextSelection,
    AiPrefs, AiProfileTestState, AiPromptRequest, AiProviderCapabilities, AiProviderKind,
    AiProviderProfile, AiStreamEvent, AiUntrustedProposal, SecretStore, redact_sensitive_tokens,
    test_provider_connection, validate_profile_endpoint,
};
use crate::app::{Editor, ScriptTarget};
use crate::theme::ScrollTheme as _;
use eframe::egui::{self, Stroke, Ui};
use neoflash_core::{
    Easing, ElementKind, FrameScript, Keyframe, LayerRole, ObjectScript, Script, ScriptLanguage,
    Timeline, sample_layer_in_at_fps,
};
use neoflash_ui::components as c;
use neoflash_ui::icons::{self, i16};
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;
use std::sync::Arc;

const CONNECTION_DIALOG_WIDTH: f32 = 480.0;
const SEND_DIALOG_WIDTH: f32 = 560.0;
const ANSWER_COMPACT_WIDTH: f32 = 480.0;
const ANSWER_PROPOSALS_WIDTH: f32 = 600.0;

const MASKED_KEY_HINT: &str = "••••••••••••••••••••";

const DEFAULT_AI_SYSTEM_PROMPT: &str = "\
You are an assistant inside NeoFlash, a timeline, vector and scripting authoring environment \
targeting HTML5. Never assume your suggestions run automatically; the author reviews every \
proposal before applying it. Answer concisely in plain text and, when suggesting concrete script \
edits or timeline easing spans, include a fenced ```json block containing a JSON array of \
proposal objects using one of these schemas:\n\
- {\"proposal_kind\":\"code_snippet_proposal\",\"language\":\"ActionScript3\"|\"ActionScript2\"|\"JavaScript\"|\"TypeScript\",\"target_hint\":\"Frame script\",\"code\":\"...\",\"explanation\":\"...\"}\n\
- {\"proposal_kind\":\"keyframe_timing_proposal\",\"layer_name\":\"...\",\"start_frame\":1,\"end_frame\":30,\"easing_hint\":\"ease_out\",\"notes\":\"...\"}";

/// Where `Save` in `Dialog / AI connection` writes the edited profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AiConnectionModalTarget {
    /// Opened from `Preferences ▸ AI` (`Add connection…` or `Edit…`); writes to `PreferencesDraft`.
    Preferences,
    /// Opened from `Dialog / AI answer` (`Edit connection…`); writes directly to `WorkspacePrefs`.
    Workspace,
}

/// State of `Dialog / AI connection` (`R-097` item 2, `480 pt`).
#[derive(Clone)]
pub(crate) struct AiConnectionModal {
    pub(crate) target: AiConnectionModalTarget,
    pub(crate) is_new: bool,
    pub(crate) profile_id: String,
    pub(crate) kind: AiProviderKind,
    pub(crate) display_name: String,
    pub(crate) base_url: String,
    pub(crate) default_model: String,
    /// Typed replacement secret. Never populated from stored secrets; cleared when dropped.
    pub(crate) api_key_input: String,
    pub(crate) has_stored_key: bool,
    pub(crate) timeout_secs: f64,
    pub(crate) max_retries: f64,
    pub(crate) streaming: bool,
    pub(crate) vision_input: bool,
    pub(crate) test_outcome: Option<Result<String, String>>,
}

impl std::fmt::Debug for AiConnectionModal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiConnectionModal")
            .field("target", &self.target)
            .field("is_new", &self.is_new)
            .field("profile_id", &self.profile_id)
            .field("kind", &self.kind)
            .field("display_name", &self.display_name)
            .field("base_url", &self.base_url)
            .field("default_model", &self.default_model)
            .field("api_key_input", &"[REDACTED]")
            .field("has_stored_key", &self.has_stored_key)
            .finish()
    }
}

impl AiConnectionModal {
    pub(crate) fn new_add(prefs: &AiPrefs, target: AiConnectionModalTarget) -> Self {
        let kind = AiProviderKind::OpenAi;
        let profile_id = prefs.next_profile_id(kind);
        Self {
            target,
            is_new: true,
            profile_id,
            kind,
            display_name: kind.display_name().to_owned(),
            base_url: kind.default_base_url().to_owned(),
            default_model: kind.default_model_hint().to_owned(),
            api_key_input: String::new(),
            has_stored_key: false,
            timeout_secs: 15.0,
            max_retries: 2.0,
            streaming: true,
            vision_input: false,
            test_outcome: None,
        }
    }

    pub(crate) fn new_edit(
        profile: &AiProviderProfile,
        has_stored_key: bool,
        target: AiConnectionModalTarget,
    ) -> Self {
        Self {
            target,
            is_new: false,
            profile_id: profile.id.clone(),
            kind: profile.kind,
            display_name: profile.display_name.clone(),
            base_url: profile.base_url.clone(),
            default_model: profile.default_model.clone(),
            api_key_input: String::new(),
            has_stored_key,
            timeout_secs: (profile.timeout_ms as f64 / 1000.0).clamp(1.0, 300.0),
            max_retries: f64::from(profile.max_retries.min(5)),
            streaming: profile.capabilities.streaming,
            vision_input: profile.capabilities.vision_input,
            test_outcome: None,
        }
    }

    pub(crate) fn set_provider(&mut self, next: AiProviderKind, prefs: &AiPrefs) {
        if self.kind == next {
            return;
        }
        let prev = self.kind;
        if self.base_url.trim().is_empty() || self.base_url.trim() == prev.default_base_url() {
            self.base_url = next.default_base_url().to_owned();
        }
        if self.default_model.trim().is_empty()
            || self.default_model.trim() == prev.default_model_hint()
        {
            self.default_model = next.default_model_hint().to_owned();
        }
        if self.is_new {
            if self.display_name.trim().is_empty()
                || self.display_name.trim() == prev.display_name()
            {
                self.display_name = next.display_name().to_owned();
            }
            self.profile_id = prefs.next_profile_id(next);
        }
        if !next.requires_secret() {
            self.api_key_input.clear();
        }
        self.kind = next;
        self.test_outcome = None;
    }

    pub(crate) fn build_profile(&self) -> AiProviderProfile {
        let name = if self.display_name.trim().is_empty() {
            self.kind.display_name().to_owned()
        } else {
            self.display_name.trim().to_owned()
        };
        let base_url = if self.base_url.trim().is_empty() {
            self.kind.default_base_url().to_owned()
        } else {
            self.base_url.trim().to_owned()
        };
        let model = if self.default_model.trim().is_empty() {
            self.kind.default_model_hint().to_owned()
        } else {
            self.default_model.trim().to_owned()
        };
        let timeout_ms = if self.timeout_secs.is_finite() {
            (self.timeout_secs.round().clamp(1.0, 300.0) as u64) * 1000
        } else {
            15_000
        };
        let max_retries = if self.max_retries.is_finite() {
            self.max_retries.round().clamp(0.0, 5.0) as u32
        } else {
            2
        };
        AiProviderProfile {
            id: self.profile_id.clone(),
            display_name: name,
            kind: self.kind,
            base_url,
            default_model: model,
            timeout_ms,
            max_retries,
            capabilities: AiProviderCapabilities {
                streaming: self.streaming,
                vision_input: self.vision_input,
                structured_json_proposal: true,
            },
        }
    }

    pub(crate) fn run_test(&mut self, ai_manager: &AiConnectionManager) {
        let profile = self.build_profile();
        let draft_secret = self.api_key_input.trim().to_owned();
        if !draft_secret.is_empty() {
            crate::ai_connections::register_redacted_secret(&draft_secret);
        }
        let store = DraftOverlaySecretStore {
            profile_id: profile.id.clone(),
            draft_secret: (!draft_secret.is_empty()).then_some(draft_secret),
            inner: ai_manager.secret_store(),
        };
        self.test_outcome = Some(
            test_provider_connection(&profile, &store)
                .map_err(|err| redact_sensitive_tokens(&err.to_string())),
        );
    }
}

struct DraftOverlaySecretStore {
    profile_id: String,
    draft_secret: Option<String>,
    inner: Arc<dyn SecretStore>,
}

impl std::fmt::Debug for DraftOverlaySecretStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DraftOverlaySecretStore")
            .field("profile_id", &self.profile_id)
            .field("draft_secret", &"[REDACTED]")
            .finish()
    }
}

impl SecretStore for DraftOverlaySecretStore {
    fn get_secret(&self, profile_id: &str) -> Result<Option<String>, AiConnectionError> {
        if profile_id == self.profile_id
            && let Some(secret) = &self.draft_secret
        {
            return Ok(Some(secret.clone()));
        }
        self.inner.get_secret(profile_id)
    }

    fn set_secret(&self, _profile_id: &str, _secret: &str) -> Result<(), AiConnectionError> {
        Ok(())
    }

    fn delete_secret(&self, _profile_id: &str) -> Result<(), AiConnectionError> {
        Ok(())
    }
}

/// Consent review dialog (`Dialog / Send to AI`, `R-097` item 3, `560 pt`).
#[derive(Clone)]
pub(crate) struct SendToAiDialog {
    pub(crate) profile_id: String,
    pub(crate) request_text: String,
    pub(crate) context_pack: AiContextPack,
    pub(crate) selection: AiContextSelection,
}

impl std::fmt::Debug for SendToAiDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SendToAiDialog")
            .field("profile_id", &self.profile_id)
            .field("request_text", &redact_sensitive_tokens(&self.request_text))
            .field("context_pack", &self.context_pack)
            .field("selection", &self.selection)
            .finish()
    }
}

impl SendToAiDialog {
    pub(crate) fn new(profile_id: String, context_pack: AiContextPack) -> Self {
        let selection = AiContextSelection::from_pack(&context_pack);
        Self {
            profile_id,
            request_text: String::new(),
            context_pack,
            selection,
        }
    }

    pub(crate) fn build_prompt_request(
        &self,
        profile: &AiProviderProfile,
        request_seq: u64,
    ) -> AiPromptRequest {
        AiPromptRequest {
            request_id: format!("ai-req-{request_seq}"),
            profile_id: profile.id.clone(),
            model: Some(profile.default_model.clone()),
            system_prompt: DEFAULT_AI_SYSTEM_PROMPT.to_owned(),
            user_prompt: redact_sensitive_tokens(self.request_text.trim()),
            context_pack: self.context_pack.filtered_for_consent(self.selection),
            temperature: None,
            max_output_tokens: 1_536,
            stream: profile.capabilities.streaming,
        }
    }
}

/// State of an individual proposal card in `Dialog / AI answer` (`R-099`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProposalCardState {
    Pending,
    Applied {
        /// Message displayed beside `Undo` (`"Inserted into Actions at the caret."` or
        /// `"Applied to layer …, frames … – …."`).
        message: String,
        /// `History::revision()` right after applying this proposal, so card-level `Undo` only
        /// fires when this proposal is still at the top of the Undo stack.
        post_revision: u64,
    },
    Dismissed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AiProposalCard {
    pub(crate) proposal: AiUntrustedProposal,
    pub(crate) state: ProposalCardState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AiAnswerPhase {
    Waiting { retry_notice: Option<String> },
    Streaming,
    Cancelled,
    Completed,
    Failed { error_text: String },
}

/// Streaming answer and proposal review dialog (`Dialog / AI answer`, `R-099`).
#[derive(Clone, Debug)]
pub(crate) struct AiAnswerDialog {
    pub(crate) profile_id: String,
    pub(crate) connection_label: String,
    pub(crate) model_label: String,
    pub(crate) max_retries: u32,
    pub(crate) origin_send: SendToAiDialog,
    pub(crate) sent_request: AiPromptRequest,
    pub(crate) phase: AiAnswerPhase,
    pub(crate) streamed_text: String,
    pub(crate) explanation_summary: String,
    pub(crate) explanation_body: String,
    pub(crate) cards: Vec<AiProposalCard>,
}

impl AiAnswerDialog {
    fn from_sent(
        profile: &AiProviderProfile,
        origin_send: SendToAiDialog,
        sent_request: AiPromptRequest,
    ) -> Self {
        Self {
            profile_id: profile.id.clone(),
            connection_label: profile.display_name.clone(),
            model_label: profile.default_model.clone(),
            max_retries: profile.max_retries,
            origin_send,
            sent_request,
            phase: AiAnswerPhase::Waiting { retry_notice: None },
            streamed_text: String::new(),
            explanation_summary: String::new(),
            explanation_body: String::new(),
            cards: Vec::new(),
        }
    }

    pub(crate) fn apply_event(&mut self, event: AiStreamEvent) {
        match event {
            AiStreamEvent::Started { model, .. } => {
                if !model.trim().is_empty() {
                    self.model_label = model;
                }
            }
            AiStreamEvent::TextDelta { delta, .. } => {
                self.streamed_text.push_str(&delta);
                if matches!(self.phase, AiAnswerPhase::Waiting { .. }) {
                    self.phase = AiAnswerPhase::Streaming;
                }
            }
            AiStreamEvent::RateLimited { retry_after_ms, .. } => {
                let wait_secs = retry_after_ms
                    .map(|ms| ms.div_ceil(1000).max(1))
                    .unwrap_or(1);
                self.phase = AiAnswerPhase::Waiting {
                    retry_notice: Some(format!(
                        "The provider limits requests. Trying again in {wait_secs} s."
                    )),
                };
            }
            AiStreamEvent::RetryScheduled {
                attempt,
                delay_ms,
                reason,
                ..
            } => {
                let wait_secs = delay_ms.div_ceil(1000).max(1);
                let total = self.max_retries.saturating_add(1);
                let current = attempt.saturating_add(1).min(total);
                let prefix = if reason.contains("429") {
                    "The provider limits requests."
                } else {
                    "Temporary provider error."
                };
                self.phase = AiAnswerPhase::Waiting {
                    retry_notice: Some(format!(
                        "{prefix} Trying again in {wait_secs} s (attempt {current} of {total})."
                    )),
                };
            }
            AiStreamEvent::ProposalReady { .. } => {}
            AiStreamEvent::Cancelled { .. } => {
                self.phase = AiAnswerPhase::Cancelled;
            }
            AiStreamEvent::Completed {
                full_text,
                proposals,
                ..
            } => {
                if self.streamed_text.is_empty() {
                    self.streamed_text = full_text.clone();
                }
                let (summary, body, cards) = split_explanation_and_cards(&full_text, proposals);
                self.explanation_summary = summary;
                self.explanation_body = body;
                self.cards = cards;
                self.phase = AiAnswerPhase::Completed;
            }
            AiStreamEvent::Failed { error, .. } => {
                if matches!(error, AiConnectionError::Cancelled) {
                    self.phase = AiAnswerPhase::Cancelled;
                } else {
                    self.phase = AiAnswerPhase::Failed {
                        error_text: redact_sensitive_tokens(&error.to_string()),
                    };
                }
            }
        }
    }
}

fn split_explanation_and_cards(
    full_text: &str,
    proposals: Vec<AiUntrustedProposal>,
) -> (String, String, Vec<AiProposalCard>) {
    let mut explanations = Vec::new();
    let mut cards = Vec::new();
    for proposal in proposals {
        match proposal {
            AiUntrustedProposal::ExplanationText { summary, body } => {
                explanations.push((summary, body));
            }
            AiUntrustedProposal::CodeSnippetProposal { .. }
            | AiUntrustedProposal::KeyframeTimingProposal { .. } => {
                cards.push(AiProposalCard {
                    proposal,
                    state: ProposalCardState::Pending,
                });
            }
        }
    }

    if let Some((first_summary, first_body)) = explanations.first() {
        let extra: Vec<&str> = explanations
            .iter()
            .skip(1)
            .map(|(_, b)| b.as_str())
            .filter(|s| !s.is_empty())
            .collect();
        let mut combined_body = if first_body.trim() == first_summary.trim() {
            String::new()
        } else if let Some(rest) = first_body.trim().strip_prefix(first_summary.trim()) {
            rest.trim().to_owned()
        } else {
            first_body.trim().to_owned()
        };
        for item in extra {
            if !combined_body.is_empty() {
                combined_body.push_str("\n\n");
            }
            combined_body.push_str(item);
        }
        return (first_summary.trim().to_owned(), combined_body, cards);
    }

    let prose = strip_fenced_blocks(full_text);
    let mut lines = prose.lines().map(str::trim).filter(|l| !l.is_empty());
    let summary = lines
        .next()
        .unwrap_or("AI proposal ready for review.")
        .to_owned();
    let body: String = lines.collect::<Vec<_>>().join("\n");
    (summary, body, cards)
}

fn strip_fenced_blocks(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 3..];
        if let Some(end) = after_open.find("```") {
            rest = &after_open[end + 3..];
        } else {
            return out.trim().to_owned();
        }
    }
    out.push_str(rest);
    out.trim().to_owned()
}

/// Per-document AI session state (`DocumentSessions` isolation): switching tabs swaps the active
/// `Send to AI` / `AI answer` dialog and worker with the document, and closing a tab drops and
/// cancels any in-flight worker immediately.
#[derive(Default)]
pub(crate) struct AiDocumentSession {
    pub(crate) send_dialog: Option<SendToAiDialog>,
    pub(crate) answer_dialog: Option<AiAnswerDialog>,
    pub(crate) worker: Option<AiConnectionWorker>,
    pub(crate) next_request_seq: u64,
}

impl std::fmt::Debug for AiDocumentSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiDocumentSession")
            .field("has_send_dialog", &self.send_dialog.is_some())
            .field("has_answer_dialog", &self.answer_dialog.is_some())
            .field("has_active_worker", &self.worker.is_some())
            .finish()
    }
}

impl AiDocumentSession {
    pub(crate) fn has_open_modal(&self) -> bool {
        self.send_dialog.is_some() || self.answer_dialog.is_some()
    }

    pub(crate) fn cancel_and_clear(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.cancel();
        }
        self.send_dialog = None;
        self.answer_dialog = None;
    }

    pub(crate) fn stop_active_request(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.cancel();
        }
        if let Some(answer) = &mut self.answer_dialog
            && matches!(
                answer.phase,
                AiAnswerPhase::Waiting { .. } | AiAnswerPhase::Streaming
            )
        {
            answer.phase = AiAnswerPhase::Cancelled;
        }
    }
}

fn success_inline_message(ui: &mut Ui, text: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        icons::show(ui, i16::CHECK, p.success_default);
        ui.add(egui::Label::new(rich(text, TextStyle::Small).color(p.success_default)).wrap());
    });
}

fn script_language_badge_label(language: ScriptLanguage) -> &'static str {
    match language {
        ScriptLanguage::ActionScript1 => "ActionScript 1.0",
        ScriptLanguage::ActionScript2 => "ActionScript 2.0",
        ScriptLanguage::ActionScript3 => "ActionScript 3.0",
        ScriptLanguage::JavaScript => "JavaScript",
        ScriptLanguage::TypeScript => "TypeScript",
    }
}

fn format_byte_size_short(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    }
}

pub(crate) fn parse_easing_hint(hint: &str) -> Easing {
    let norm: String = hint
        .trim()
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    match norm.as_str() {
        "linear" | "none" => Easing::Linear,
        "easein" | "in" | "quadin" | "cubicin" => Easing::EaseIn,
        "easeout" | "out" | "quadout" | "cubicout" => Easing::EaseOut,
        "bounce" | "easebounce" => Easing::Bounce,
        "elastic" | "easeelastic" => Easing::Elastic,
        "easeinout" | "inout" | "standard" => Easing::EaseInOut,
        _ => Easing::EaseInOut,
    }
}

fn human_easing_label(hint: &str) -> &'static str {
    match parse_easing_hint(hint) {
        Easing::Linear => "Linear",
        Easing::EaseIn => "Ease in",
        Easing::EaseOut => "Ease out",
        Easing::Bounce => "Bounce",
        Easing::Elastic => "Elastic",
        Easing::EaseInOut | Easing::CubicBezier { .. } | Easing::Flash { .. } => "Ease in-out",
    }
}

fn find_timeline_layer_index(timeline: &Timeline, layer_name: &str) -> Option<usize> {
    let trimmed = layer_name.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(idx) = timeline.layers.iter().position(|l| l.name == trimmed) {
        return Some(idx);
    }
    if let Some(idx) = timeline
        .layers
        .iter()
        .position(|l| l.name.eq_ignore_ascii_case(trimmed))
    {
        return Some(idx);
    }
    if let Some(stripped) = trimmed
        .strip_prefix("Layer ")
        .or_else(|| trimmed.strip_prefix("layer "))
        .map(str::trim)
        && !stripped.is_empty()
        && let Some(idx) = timeline
            .layers
            .iter()
            .position(|l| l.name.eq_ignore_ascii_case(stripped))
    {
        return Some(idx);
    }
    None
}

impl Editor {
    /// True when at least one configured AI connection profile is in the `Ready` state (`R-097`
    /// rule 4: AI commands stay hidden until one connection is `Ready`).
    pub fn has_ready_ai_connection(&self) -> bool {
        self.ai_manager.has_ready_connection(&self.workspace.ai)
    }

    /// Build the bounded, redacted [`AiContextPack`] for the active document selection.
    pub fn build_ai_context_pack(&self) -> AiContextPack {
        let mut selected_ids: Vec<String> = self
            .selection
            .targets
            .iter()
            .map(|t| t.element.clone())
            .collect();
        if let Some(lib_id) = &self.library_selected
            && !selected_ids.contains(lib_id)
        {
            selected_ids.push(lib_id.clone());
        }
        // Also surface video/sound resources referenced by the selected elements or active frame.
        let sym_id = self.symbol_id().to_owned();
        if let Some(sym) = self.project.symbol(&sym_id) {
            for layer in &sym.timeline.layers {
                if let Some(kf) = layer.key_at(self.frame) {
                    if let Some(sound) = &kf.sound
                        && !selected_ids.contains(&sound.resource)
                    {
                        selected_ids.push(sound.resource.clone());
                    }
                    if let Some(video) = &kf.video
                        && !video.resource.is_empty()
                        && !selected_ids.contains(&video.resource)
                    {
                        selected_ids.push(video.resource.clone());
                    }
                    for el in &kf.elements {
                        if self.selection.targets.iter().any(|t| t.element == el.id)
                            && let ElementKind::Bitmap { resource_id, .. } = &el.kind
                            && !selected_ids.contains(resource_id)
                        {
                            selected_ids.push(resource_id.clone());
                        }
                    }
                }
            }
        }

        let script_source_and_lang: Option<(String, ScriptLanguage)> = match self.script_target {
            ScriptTarget::Source => self
                .selected_source
                .as_deref()
                .and_then(|id| self.project.sources.iter().find(|s| s.id == id))
                .filter(|u| !u.source.trim().is_empty())
                .map(|u| (u.source.clone(), u.language)),
            ScriptTarget::Object => self
                .selected_element()
                .and_then(|el| {
                    el.scripts
                        .iter()
                        .find(|s| s.event == self.script_event)
                        .or_else(|| el.scripts.first())
                })
                .filter(|s| !s.script.source.trim().is_empty())
                .map(|s| (s.script.source.clone(), s.script.language)),
            ScriptTarget::Frame => self
                .symbol()
                .timeline
                .scripts
                .iter()
                .find(|s| s.frame == self.frame)
                .filter(|s| !s.script.source.trim().is_empty())
                .map(|s| (s.script.source.clone(), s.script.language)),
        };

        AiContextPack::from_editor_selection(
            &self.project,
            &sym_id,
            self.frame,
            &selected_ids,
            script_source_and_lang
                .as_ref()
                .map(|(src, lang)| (src.as_str(), *lang)),
        )
    }

    /// Open `Dialog / Send to AI` (`R-097` item 3) if a document is open and at least one
    /// connection is `Ready`. Never transmits anything on open.
    pub fn open_send_to_ai(&mut self) -> bool {
        if !self.has_document() {
            return false;
        }
        let Some(default_ready) = self
            .ai_manager
            .default_ready_profile(&self.workspace.ai)
            .cloned()
        else {
            return false;
        };
        let pack = self.build_ai_context_pack();
        self.ai_session.cancel_and_clear();
        self.ai_session.send_dialog = Some(SendToAiDialog::new(default_ready.id, pack));
        true
    }

    /// Confirm `Send` in `Dialog / Send to AI`, transitioning in-place to `Dialog / AI answer` and
    /// spawning the cancellable background request (`R-097` & `R-099`).
    pub fn submit_send_to_ai(&mut self) -> bool {
        let Some(send_dialog) = self.ai_session.send_dialog.clone() else {
            return false;
        };
        if send_dialog.request_text.trim().is_empty() {
            return false;
        }
        let Some(profile) = self.workspace.ai.profile(&send_dialog.profile_id).cloned() else {
            return false;
        };
        if let Some(old_worker) = self.ai_session.worker.take() {
            old_worker.cancel();
        }
        self.ai_session.next_request_seq = self.ai_session.next_request_seq.saturating_add(1);
        let req = send_dialog.build_prompt_request(&profile, self.ai_session.next_request_seq);
        let answer = AiAnswerDialog::from_sent(&profile, send_dialog, req.clone());
        let worker = self.ai_manager.spawn_request(profile, req);
        self.ai_session.send_dialog = None;
        self.ai_session.answer_dialog = Some(answer);
        self.ai_session.worker = Some(worker);
        true
    }

    /// Retry a failed request from `Dialog / AI answer` (`Try again`).
    pub fn retry_ai_request(&mut self) -> bool {
        let Some(answer) = self.ai_session.answer_dialog.clone() else {
            return false;
        };
        let Some(profile) = self.workspace.ai.profile(&answer.profile_id).cloned() else {
            return false;
        };
        if let Some(old_worker) = self.ai_session.worker.take() {
            old_worker.cancel();
        }
        self.ai_session.next_request_seq = self.ai_session.next_request_seq.saturating_add(1);
        let mut req = answer.sent_request;
        req.request_id = format!("ai-req-{}", self.ai_session.next_request_seq);
        req.profile_id = profile.id.clone();
        req.model = Some(profile.default_model.clone());
        req.stream = profile.capabilities.streaming;
        let next_answer = AiAnswerDialog::from_sent(&profile, answer.origin_send, req.clone());
        let worker = self.ai_manager.spawn_request(profile, req);
        self.ai_session.answer_dialog = Some(next_answer);
        self.ai_session.worker = Some(worker);
        true
    }

    /// Drain streaming events for the active document's AI worker.
    pub fn poll_ai_session(&mut self, ctx: &egui::Context) {
        let events = match &self.ai_session.worker {
            Some(worker) => worker.drain_events(),
            None => return,
        };
        let mut finished = false;
        if let Some(answer) = &mut self.ai_session.answer_dialog {
            for ev in events {
                if matches!(
                    ev,
                    AiStreamEvent::Completed { .. }
                        | AiStreamEvent::Failed { .. }
                        | AiStreamEvent::Cancelled { .. }
                ) {
                    finished = true;
                }
                answer.apply_event(ev);
            }
        } else {
            finished = true;
        }
        if finished {
            self.ai_session.worker = None;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    /// Validate whether a `KeyframeTimingProposal` can be applied to the current symbol's timeline
    /// (`R-099` item 4: disabled with an explanatory `Small` line if the layer is missing or the
    /// range does not fit).
    pub fn validate_ai_timing_proposal(
        &self,
        layer_name: &str,
        start_frame: u32,
        end_frame: u32,
    ) -> Result<usize, String> {
        let timeline = &self.symbol().timeline;
        let Some(layer_idx) = find_timeline_layer_index(timeline, layer_name) else {
            return Err(format!(
                "Layer “{}” is not in the current timeline.",
                layer_name.trim()
            ));
        };
        let layer = &timeline.layers[layer_idx];
        if layer.properties.role == LayerRole::Folder {
            return Err(format!(
                "Layer “{}” is a folder and cannot hold a tween.",
                layer.name
            ));
        }
        if neoflash_core::layer_effectively_locked(timeline, layer) {
            return Err(format!("Layer “{}” is locked.", layer.name));
        }
        if start_frame < 1 || end_frame < start_frame {
            return Err(format!(
                "Frames {start_frame} – {end_frame} are not a valid range."
            ));
        }
        let total_frames = timeline.frame_count.max(1);
        if end_frame > total_frames {
            return Err(format!(
                "Frames {start_frame} – {end_frame} do not fit in the current timeline ({total_frames} frames)."
            ));
        }
        Ok(layer_idx)
    }

    /// Insert a code proposal at the caret of the script currently open in `Actions` as a single
    /// Undo step (`R-099` items 4 & 6).
    pub fn apply_ai_code_proposal(
        &mut self,
        ctx: &egui::Context,
        language: ScriptLanguage,
        code: &str,
    ) -> bool {
        if code.is_empty() || !self.has_document() {
            return false;
        }
        self.show_panel(crate::workspace::PanelId::Actions);
        let editor_id = self.actions_editor_id();
        let caret_chars = egui::text_edit::TextEditState::load(ctx, editor_id)
            .and_then(|s| s.cursor.char_range().map(|r| r.primary.index));

        let insert_at_caret = |source: &mut String| -> usize {
            let total_chars = source.chars().count();
            let char_pos = caret_chars.unwrap_or(total_chars).min(total_chars);
            let byte_pos = source
                .char_indices()
                .nth(char_pos)
                .map_or(source.len(), |(idx, _)| idx);
            source.insert_str(byte_pos, code);
            char_pos + code.chars().count()
        };

        let rev_before = self.history.revision();
        let mut new_caret_chars = None;

        match self.script_target {
            ScriptTarget::Source
                if self
                    .selected_source
                    .as_deref()
                    .is_some_and(|id| self.project.sources.iter().any(|s| s.id == id)) =>
            {
                let selected_id = self.selected_source.clone().unwrap_or_default();
                self.transact_named("Insert AI code proposal", |ed| {
                    if let Some(unit) = ed.project.sources.iter_mut().find(|s| s.id == selected_id)
                    {
                        new_caret_chars = Some(insert_at_caret(&mut unit.source));
                    }
                });
            }
            ScriptTarget::Object if self.selected_editable() => {
                let event = self.script_event;
                self.transact_named("Insert AI code proposal", |ed| {
                    let proj_lang = ed.project.effective_script_language();
                    let resolved_lang = if ed
                        .project
                        .script_disposition_for(
                            proj_lang,
                            ed.project.also_compile_compatible_scripts,
                            language,
                        )
                        .is_compiled()
                    {
                        language
                    } else {
                        proj_lang
                    };
                    if let Some(element) = ed.selected_element_mut() {
                        if let Some(obj_script) =
                            element.scripts.iter_mut().find(|s| s.event == event)
                        {
                            new_caret_chars = Some(insert_at_caret(&mut obj_script.script.source));
                        } else {
                            let mut src = String::new();
                            new_caret_chars = Some(insert_at_caret(&mut src));
                            element.scripts.push(ObjectScript {
                                event,
                                script: Script {
                                    language: resolved_lang,
                                    source: src,
                                },
                            });
                        }
                    }
                });
            }
            _ => {
                let frame = self.frame;
                self.transact_named("Insert AI code proposal", |ed| {
                    let proj_lang = ed.project.effective_script_language();
                    let resolved_lang = if ed
                        .project
                        .script_disposition_for(
                            proj_lang,
                            ed.project.also_compile_compatible_scripts,
                            language,
                        )
                        .is_compiled()
                    {
                        language
                    } else {
                        proj_lang
                    };
                    if frame >= ed.symbol().timeline.frame_count {
                        ed.ensure_key(false);
                    }
                    let scripts = &mut ed.symbol_mut().timeline.scripts;
                    if let Some(existing) = scripts.iter_mut().find(|s| s.frame == frame) {
                        new_caret_chars = Some(insert_at_caret(&mut existing.script.source));
                    } else {
                        let mut src = String::new();
                        new_caret_chars = Some(insert_at_caret(&mut src));
                        scripts.push(FrameScript {
                            frame,
                            script: Script {
                                language: resolved_lang,
                                source: src,
                            },
                        });
                    }
                });
            }
        }

        if let Some(after_char) = new_caret_chars {
            let mut state =
                egui::text_edit::TextEditState::load(ctx, editor_id).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(after_char),
                )));
            state.store(ctx, editor_id);
        }

        self.history.revision() != rev_before
    }

    /// Apply a `KeyframeTimingProposal` to the current symbol's timeline as a single Undo step
    /// (`R-099` items 4 & 6).
    pub fn apply_ai_timing_proposal(
        &mut self,
        layer_name: &str,
        start_frame: u32,
        end_frame: u32,
        easing_hint: &str,
    ) -> Result<String, String> {
        let layer_idx = self.validate_ai_timing_proposal(layer_name, start_frame, end_frame)?;
        let resolved_name = self.symbol().timeline.layers[layer_idx].name.clone();
        let easing = parse_easing_hint(easing_hint);
        let start_idx = start_frame - 1;
        let end_idx = end_frame - 1;
        let fps = f64::from(self.project.fps.max(1));

        self.transact_named("Apply AI timing proposal", |ed| {
            let ensure_ordinary_key = |timeline: &mut Timeline, idx: usize, frame_idx: u32| {
                let already = timeline.layers[idx]
                    .keyframes
                    .iter()
                    .position(|k| k.frame == frame_idx);
                if let Some(pos) = already {
                    timeline.layers[idx].keyframes[pos].keyed_properties = None;
                    timeline.layers[idx].keyframes[pos].motion_keys = None;
                } else {
                    let sampled =
                        sample_layer_in_at_fps(timeline, &timeline.layers[idx], frame_idx, fps);
                    let inherited_tween_kind = timeline.layers[idx]
                        .key_at(frame_idx)
                        .map_or(neoflash_core::TweenKind::Classic, |k| k.tween_kind);
                    timeline.layers[idx].keyframes.push(Keyframe {
                        frame: frame_idx,
                        elements: sampled,
                        tween_kind: inherited_tween_kind,
                        ..Keyframe::default()
                    });
                    timeline.layers[idx].keyframes.sort_by_key(|k| k.frame);
                }
            };

            let timeline = &mut ed.symbol_mut().timeline;
            ensure_ordinary_key(timeline, layer_idx, start_idx);
            if end_idx > start_idx {
                ensure_ordinary_key(timeline, layer_idx, end_idx);
            }
            for kf in &mut timeline.layers[layer_idx].keyframes {
                let in_span = if end_idx > start_idx {
                    kf.frame >= start_idx && kf.frame < end_idx
                } else {
                    kf.frame == start_idx
                };
                if in_span && kf.keyed_properties.is_none() {
                    kf.tween = Some(easing);
                }
            }
            timeline.layers[layer_idx].normalize_motion_keys();
        });

        Ok(format!(
            "Applied to layer {resolved_name}, frames {start_frame} – {end_frame}."
        ))
    }

    pub(crate) fn save_ai_connection_modal(&mut self, mut modal: AiConnectionModal) {
        let profile = modal.build_profile();
        if profile.kind.requires_secret()
            && !modal.api_key_input.trim().is_empty()
            && let Err(err) = self
                .ai_manager
                .set_secret(&profile.id, modal.api_key_input.trim())
        {
            let redacted = redact_sensitive_tokens(&err.to_string());
            modal.test_outcome = Some(Err(redacted.clone()));
            self.ai_manager.set_test_state(
                profile.id,
                AiProfileTestState::Failed {
                    message: redacted.clone(),
                },
            );
            self.notify(c::ToastType::Error, &redacted);
            self.message(redacted);
            self.ai_connection_modal = Some(modal);
            return;
        }
        match modal.test_outcome {
            Some(Ok(_)) => {
                self.ai_manager.set_test_state(
                    profile.id.clone(),
                    AiProfileTestState::Connected {
                        model: profile.default_model.clone(),
                    },
                );
            }
            Some(Err(err)) => {
                self.ai_manager.set_test_state(
                    profile.id.clone(),
                    AiProfileTestState::Failed { message: err },
                );
            }
            None => {
                self.ai_manager.clear_test_state(&profile.id);
            }
        }
        match modal.target {
            AiConnectionModalTarget::Preferences => {
                if let Some(draft) = &mut self.preferences {
                    draft.ai.upsert_profile(profile.clone());
                    draft.selected_ai_profile = Some(profile.id);
                    draft.has_ready_ai = self.ai_manager.has_ready_connection(&draft.ai);
                }
            }
            AiConnectionModalTarget::Workspace => {
                self.workspace.ai.upsert_profile(profile.clone());
                self.request_workspace_persist();
                if let Some(answer) = &mut self.ai_session.answer_dialog
                    && answer.profile_id == profile.id
                {
                    answer.connection_label = profile.display_name;
                    answer.model_label = profile.default_model;
                    answer.max_retries = profile.max_retries;
                }
            }
        }
    }

    pub(crate) fn ai_dialogs(&mut self, ctx: &egui::Context) {
        self.poll_ai_session(ctx);
        self.send_to_ai_dialog(ctx);
        self.ai_answer_dialog(ctx);
        self.ai_connection_dialog(ctx);
    }

    fn ai_connection_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut modal_state) = self.ai_connection_modal.clone() else {
            return;
        };
        let prefs_snapshot = match modal_state.target {
            AiConnectionModalTarget::Preferences => self
                .preferences
                .as_ref()
                .map_or_else(|| self.workspace.ai.clone(), |d| d.ai.clone()),
            AiConnectionModalTarget::Workspace => self.workspace.ai.clone(),
        };
        let kb = DialogKeyboardState::capture(ctx);
        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut close_save = false;
        let mut trigger_test = false;

        let modal_id = egui::Id::new("nf2-ai-connection-dialog");
        let modal = egui::Modal::new(modal_id)
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(CONNECTION_DIALOG_WIDTH);
                let top = ui.cursor().min;
                close_cancel |= c::DialogHeader {
                    title: "AI connection",
                }
                .show(ui);
                ui.painter().hline(
                    top.x..=top.x + CONNECTION_DIALOG_WIDTH,
                    top.y + neoflash_ui::metrics::dialog_header() - 0.5,
                    Stroke::new(1.0_f32, p.border_subtle),
                );
                let inner_w = CONNECTION_DIALOG_WIDTH - 2.0 * space::S5;
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        ui.set_width(inner_w);
                        ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S3);

                        let provider_label =
                            format!("Provider: {}", modal_state.kind.display_name());
                        c::Select::new("ai-conn-provider", &provider_label)
                            .width(inner_w)
                            .show(ui, |ui| {
                                for candidate in AiProviderKind::ALL {
                                    if ui
                                        .add(
                                            c::MenuItem::new(candidate.display_name())
                                                .checked(modal_state.kind == candidate),
                                        )
                                        .clicked()
                                    {
                                        modal_state.set_provider(candidate, &prefs_snapshot);
                                        ui.close();
                                    }
                                }
                            });

                        if ui
                            .add(
                                c::TextField::new(&mut modal_state.display_name, "Name")
                                    .prefix("Name")
                                    .width(inner_w),
                            )
                            .changed()
                        {
                            modal_state.test_outcome = None;
                        }
                        if ui
                            .add(
                                c::TextField::new(&mut modal_state.base_url, "Base URL")
                                    .prefix("Base URL")
                                    .width(inner_w),
                            )
                            .changed()
                        {
                            modal_state.test_outcome = None;
                        }
                        if ui
                            .add(
                                c::TextField::new(&mut modal_state.default_model, "Model")
                                    .prefix("Model")
                                    .width(inner_w),
                            )
                            .changed()
                        {
                            modal_state.test_outcome = None;
                        }

                        if modal_state.kind.requires_secret() {
                            let hint = if modal_state.has_stored_key {
                                MASKED_KEY_HINT
                            } else {
                                ""
                            };
                            if ui
                                .add(
                                    c::TextField::new(&mut modal_state.api_key_input, "API key")
                                        .prefix("API key")
                                        .hint(hint)
                                        .width(inner_w),
                                )
                                .changed()
                            {
                                modal_state.test_outcome = None;
                            }
                            ui.add(
                                egui::Label::new(
                                    rich(
                                        "Stored in the system keychain. The key is never shown again and never saved with the project.",
                                        TextStyle::Small,
                                    )
                                    .color(p.text_secondary),
                                )
                                .wrap(),
                            );
                        } else {
                            let mut not_required = "Not required".to_owned();
                            ui.scope(|ui| {
                                ui.disable();
                                ui.add(
                                    c::TextField::new(&mut not_required, "API key")
                                        .prefix("API key")
                                        .width(inner_w),
                                );
                            });
                            ui.add(
                                egui::Label::new(
                                    rich(
                                        "A local provider may use http:// on this computer. Cloud providers must use https://.",
                                        TextStyle::Small,
                                    )
                                    .color(p.text_secondary),
                                )
                                .wrap(),
                            );
                        }

                        c::SectionHeader::new("Advanced")
                            .id_salt("ai-conn-advanced")
                            .default_open(false)
                            .show(ui, |ui| {
                                let half_w = (inner_w - space::S2) * 0.5;
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = space::S2;
                                    ui.add(
                                        c::Field::new(&mut modal_state.timeout_secs, "Timeout")
                                            .prefix("Timeout")
                                            .unit("s")
                                            .range(1.0..=300.0)
                                            .decimals(0)
                                            .width(half_w),
                                    );
                                    ui.add(
                                        c::Field::new(&mut modal_state.max_retries, "Retries")
                                            .prefix("Retries")
                                            .range(0.0..=5.0)
                                            .decimals(0)
                                            .width(half_w),
                                    );
                                });
                                ui.add(c::Checkbox::new(
                                    &mut modal_state.streaming,
                                    "Streaming responses",
                                ));
                                ui.add(c::Checkbox::new(
                                    &mut modal_state.vision_input,
                                    "Image input",
                                ));
                            });

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::S3;
                            if ui.add(c::Button::new("Test connection")).clicked() {
                                trigger_test = true;
                            }
                            match &modal_state.test_outcome {
                                Some(Ok(msg)) => success_inline_message(ui, msg),
                                Some(Err(err)) => {
                                    ui.add(c::InlineMessage::error(err));
                                }
                                None => {}
                            }
                        });

                        match footer_clicked_with_enabled(
                            ui,
                            "",
                            None,
                            &[("Cancel", true), ("Save", true)],
                        ) {
                            Some(FooterClick::Button(0)) => close_cancel = true,
                            Some(FooterClick::Button(_)) => close_save = true,
                            _ => {}
                        }
                    });
            });

        if trigger_test {
            modal_state.run_test(&self.ai_manager);
        }

        if !close_cancel && !close_save {
            match kb.resolve(ctx, &modal, true) {
                DialogKeyboardOutcome::Cancel => close_cancel = true,
                DialogKeyboardOutcome::Confirm => close_save = true,
                DialogKeyboardOutcome::None => {}
            }
        }

        if close_cancel {
            self.ai_connection_modal = None;
        } else if close_save {
            self.ai_connection_modal = None;
            self.save_ai_connection_modal(modal_state);
        } else {
            self.ai_connection_modal = Some(modal_state);
        }
    }

    fn send_to_ai_dialog(&mut self, ctx: &egui::Context) {
        if self.ai_connection_modal.is_some() {
            return;
        }
        let Some(mut dialog) = self.ai_session.send_dialog.clone() else {
            return;
        };
        let ready: Vec<AiProviderProfile> = self
            .ai_manager
            .ready_profiles(&self.workspace.ai)
            .into_iter()
            .cloned()
            .collect();
        if ready.is_empty() {
            self.ai_session.send_dialog = None;
            return;
        }
        if !ready.iter().any(|p| p.id == dialog.profile_id)
            && let Some(first) = ready.first()
        {
            dialog.profile_id = first.id.clone();
        }
        let active_profile = ready
            .iter()
            .find(|p| p.id == dialog.profile_id)
            .or_else(|| ready.first())
            .cloned();

        let kb = DialogKeyboardState::capture(ctx);
        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut confirm_send = false;
        let can_send = active_profile.is_some() && !dialog.request_text.trim().is_empty();

        let modal_id = egui::Id::new("nf2-send-to-ai-dialog");
        let modal = egui::Modal::new(modal_id)
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(SEND_DIALOG_WIDTH);
                let top = ui.cursor().min;
                close_cancel |= c::DialogHeader {
                    title: "Send to AI",
                }
                .show(ui);
                ui.painter().hline(
                    top.x..=top.x + SEND_DIALOG_WIDTH,
                    top.y + neoflash_ui::metrics::dialog_header() - 0.5,
                    Stroke::new(1.0_f32, p.border_subtle),
                );
                let inner_w = SEND_DIALOG_WIDTH - 2.0 * space::S5;
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        ui.set_width(inner_w);
                        ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S3);

                        let conn_label = active_profile.as_ref().map_or_else(
                            || "Connection: —".to_owned(),
                            |prof| {
                                format!(
                                    "Connection: {} · {}",
                                    prof.display_name, prof.default_model
                                )
                            },
                        );
                        c::Select::new("send-ai-connection", &conn_label)
                            .width(inner_w)
                            .show(ui, |ui| {
                                for prof in &ready {
                                    let row_label =
                                        format!("{} · {}", prof.display_name, prof.default_model);
                                    if ui
                                        .add(
                                            c::MenuItem::new(&row_label)
                                                .checked(dialog.profile_id == prof.id),
                                        )
                                        .clicked()
                                    {
                                        dialog.profile_id = prof.id.clone();
                                        ui.close();
                                    }
                                }
                            });

                        ui.label(rich("Request", TextStyle::BodyStrong).color(p.text_primary));
                        ui.add(
                            c::TextField::new(&mut dialog.request_text, "Ask")
                                .prefix("Ask")
                                .hint("Describe what to inspect or explain…")
                                .width(inner_w),
                        );

                        ui.label(
                            rich("What will be sent", TextStyle::BodyStrong).color(p.text_primary),
                        );

                        egui::Frame::new()
                            .fill(p.bg_panel)
                            .stroke(Stroke::new(1.0_f32, p.border_subtle))
                            .corner_radius(egui::CornerRadius::same(radius::MD as u8))
                            .show(ui, |ui| {
                                ui.set_width(inner_w);
                                ui.spacing_mut().item_spacing.y = 0.0;

                                let proj_summary = format!(
                                    "{} · {} · frame {}",
                                    dialog.context_pack.project_name,
                                    dialog.context_pack.symbol_name,
                                    dialog.context_pack.active_frame + 1
                                );
                                context_checkbox_row(
                                    ui,
                                    &mut dialog.selection.project_and_symbol,
                                    true,
                                    "Project and symbol names",
                                    &proj_summary,
                                    true,
                                );

                                let el_count = dialog.context_pack.selected_elements.len();
                                let el_label = format!("Selected elements ({el_count})");
                                let el_summary = if el_count > 0 {
                                    "name, type and position"
                                } else {
                                    "none selected"
                                };
                                context_checkbox_row(
                                    ui,
                                    &mut dialog.selection.selected_elements,
                                    el_count > 0,
                                    &el_label,
                                    el_summary,
                                    true,
                                );

                                let (has_script, script_summary) =
                                    match &dialog.context_pack.script_excerpt {
                                        Some(s) => (
                                            true,
                                            format!(
                                                "{} · {}",
                                                script_language_badge_label(s.language),
                                                format_byte_size_short(s.source.len())
                                            ),
                                        ),
                                        None => (false, "none".to_owned()),
                                    };
                                context_checkbox_row(
                                    ui,
                                    &mut dialog.selection.script_excerpt,
                                    has_script,
                                    "Script excerpt",
                                    &script_summary,
                                    true,
                                );

                                let asset_count = dialog.context_pack.included_asset_count();
                                let asset_label = format!("Asset list ({asset_count})");
                                let asset_summary = if asset_count > 0 {
                                    "name, type and size only"
                                } else {
                                    "none"
                                };
                                context_checkbox_row(
                                    ui,
                                    &mut dialog.selection.asset_list,
                                    asset_count > 0,
                                    &asset_label,
                                    asset_summary,
                                    false,
                                );
                            });

                        if let Some(omitted_note) = dialog.context_pack.omitted_items_note() {
                            ui.add(
                                egui::Label::new(
                                    rich(&omitted_note, TextStyle::Small).color(p.text_tertiary),
                                )
                                .wrap(),
                            );
                        }

                        ui.add(
                            c::InlineMessage::info(
                                "Keys, tokens and passwords found in the text are replaced with [REDACTED_SECRET] before sending.",
                            )
                            .width(inner_w),
                        );

                        let dest_line = match active_profile
                            .as_ref()
                            .and_then(|prof| validate_profile_endpoint(prof).ok())
                        {
                            Some(ep) => {
                                let proto = if ep.is_https { "HTTPS" } else { "HTTP" };
                                format!(
                                    "Destination: {} over {proto}. The answer is a proposal: nothing in the document changes until you apply it.",
                                    ep.host_display
                                )
                            }
                            None => "Destination: invalid endpoint URL. Check the connection in Preferences ▸ AI.".to_owned(),
                        };
                        ui.add(
                            egui::Label::new(
                                rich(&dest_line, TextStyle::Small).color(p.text_secondary),
                            )
                            .wrap(),
                        );

                        match footer_clicked_with_enabled(
                            ui,
                            "",
                            None,
                            &[("Cancel", true), ("Send", can_send)],
                        ) {
                            Some(FooterClick::Button(0)) => close_cancel = true,
                            Some(FooterClick::Button(_)) if can_send => confirm_send = true,
                            _ => {}
                        }
                    });
            });

        if !close_cancel && !confirm_send {
            match kb.resolve(ctx, &modal, can_send) {
                DialogKeyboardOutcome::Cancel => close_cancel = true,
                DialogKeyboardOutcome::Confirm if can_send => confirm_send = true,
                _ => {}
            }
        }

        if close_cancel {
            self.ai_session.send_dialog = None;
        } else if confirm_send {
            self.ai_session.send_dialog = Some(dialog);
            let _ = self.submit_send_to_ai();
        } else {
            self.ai_session.send_dialog = Some(dialog);
        }
    }

    fn ai_answer_dialog(&mut self, ctx: &egui::Context) {
        if self.ai_connection_modal.is_some() {
            return;
        }
        let Some(mut answer) = self.ai_session.answer_dialog.clone() else {
            return;
        };

        let dialog_w = if matches!(answer.phase, AiAnswerPhase::Completed) {
            ANSWER_PROPOSALS_WIDTH
        } else {
            ANSWER_COMPACT_WIDTH
        };
        let inner_w = dialog_w - 2.0 * space::S5;

        let kb = DialogKeyboardState::capture(ctx);
        let p = neoflash_ui::palette(ctx);
        let mut close_dialog = false;
        let mut stop_clicked = false;
        let mut reopen_new_request = false;
        let mut edit_connection = false;
        let mut retry_clicked = false;
        let mut apply_code_idx: Option<usize> = None;
        let mut apply_timing_idx: Option<usize> = None;
        let mut undo_card_idx: Option<usize> = None;

        let current_revision = self.history.revision();
        let can_undo_now = self.history.can_undo();

        let modal_id = egui::Id::new("nf2-ai-answer-dialog");
        let modal = egui::Modal::new(modal_id)
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(dialog_w);
                let top = ui.cursor().min;
                close_dialog |= c::DialogHeader { title: "AI answer" }.show(ui);
                ui.painter().hline(
                    top.x..=top.x + dialog_w,
                    top.y + neoflash_ui::metrics::dialog_header() - 0.5,
                    Stroke::new(1.0_f32, p.border_subtle),
                );
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        ui.set_width(inner_w);
                        ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S3);

                        let subtitle =
                            format!("{} · {}", answer.connection_label, answer.model_label);
                        ui.label(rich(&subtitle, TextStyle::Small).color(p.text_secondary));

                        match &answer.phase {
                            AiAnswerPhase::Waiting { retry_notice } => {
                                c::progress(ui, "Waiting for the provider…", None, inner_w);
                                if let Some(notice) = retry_notice {
                                    ui.add(c::InlineMessage::info(notice).width(inner_w));
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.add(c::Button::new("Stop")).clicked() {
                                            stop_clicked = true;
                                        }
                                    },
                                );
                            }
                            AiAnswerPhase::Streaming => {
                                c::progress(ui, "Receiving the answer…", None, inner_w);
                                readonly_answer_box(
                                    ui,
                                    inner_w,
                                    None,
                                    &answer.streamed_text,
                                    180.0,
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.add(c::Button::new("Stop")).clicked() {
                                            stop_clicked = true;
                                        }
                                    },
                                );
                            }
                            AiAnswerPhase::Cancelled => {
                                if !answer.streamed_text.trim().is_empty() {
                                    readonly_answer_box(
                                        ui,
                                        inner_w,
                                        None,
                                        &answer.streamed_text,
                                        180.0,
                                    );
                                }
                                ui.add(
                                    c::InlineMessage::info("Request cancelled.").width(inner_w),
                                );
                                match footer_clicked_with_enabled(
                                    ui,
                                    "",
                                    Some(("New request…", true)),
                                    &[("Done", true)],
                                ) {
                                    Some(FooterClick::Ghost) => reopen_new_request = true,
                                    Some(FooterClick::Button(_)) => close_dialog = true,
                                    None => {}
                                }
                            }
                            AiAnswerPhase::Completed => {
                                egui::ScrollArea::vertical()
                                    .id_salt("ai-answer-completed-scroll")
                                    .max_height(420.0)
                                    .scroll_bar_visibility(
                                        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                                    )
                                    .auto_shrink([false, true])
                                    .themed()
                                    .show(ui, |ui| {
                                        ui.set_width(inner_w);
                                        ui.spacing_mut().item_spacing.y = space::S3;
                                        readonly_answer_box(
                                            ui,
                                            inner_w,
                                            Some(&answer.explanation_summary),
                                            &answer.explanation_body,
                                            160.0,
                                        );

                                        if !answer.cards.is_empty() {
                                            ui.label(
                                                rich("Proposals", TextStyle::Overline)
                                                    .color(p.text_secondary),
                                            );
                                            for (idx, card) in answer.cards.iter_mut().enumerate() {
                                                render_proposal_card(
                                                    ui,
                                                    self,
                                                    inner_w,
                                                    idx,
                                                    card,
                                                    current_revision,
                                                    can_undo_now,
                                                    &mut apply_code_idx,
                                                    &mut apply_timing_idx,
                                                    &mut undo_card_idx,
                                                );
                                            }
                                        }
                                    });

                                ui.add(
                                    c::InlineMessage::warning(
                                        "NeoFlash does not check AI output. Read a proposal before applying it; each one is a single Undo step.",
                                    )
                                    .width(inner_w),
                                );

                                match footer_clicked_with_enabled(
                                    ui,
                                    "",
                                    Some(("New request…", true)),
                                    &[("Done", true)],
                                ) {
                                    Some(FooterClick::Ghost) => reopen_new_request = true,
                                    Some(FooterClick::Button(_)) => close_dialog = true,
                                    None => {}
                                }
                            }
                            AiAnswerPhase::Failed { error_text } => {
                                ui.add(c::InlineMessage::error(error_text).width(inner_w));
                                ui.label(
                                    rich(
                                        "Nothing was changed in the document.",
                                        TextStyle::Small,
                                    )
                                    .color(p.text_secondary),
                                );
                                match footer_clicked_with_enabled(
                                    ui,
                                    "",
                                    Some(("Edit connection…", true)),
                                    &[("Close", true), ("Try again", true)],
                                ) {
                                    Some(FooterClick::Ghost) => edit_connection = true,
                                    Some(FooterClick::Button(0)) => close_dialog = true,
                                    Some(FooterClick::Button(_)) => retry_clicked = true,
                                    None => {}
                                }
                            }
                        }
                    });
            });

        if !close_dialog
            && !stop_clicked
            && !reopen_new_request
            && !edit_connection
            && !retry_clicked
            && kb.resolve(ctx, &modal, false) == DialogKeyboardOutcome::Cancel
        {
            close_dialog = true;
        }

        if let Some(idx) = apply_code_idx
            && let Some(card) = answer.cards.get_mut(idx)
            && let AiUntrustedProposal::CodeSnippetProposal { language, code, .. } =
                card.proposal.clone()
            && self.apply_ai_code_proposal(ctx, language, &code)
        {
            card.state = ProposalCardState::Applied {
                message: "Inserted into Actions at the caret.".to_owned(),
                post_revision: self.history.revision(),
            };
        }

        if let Some(idx) = apply_timing_idx
            && let Some(card) = answer.cards.get_mut(idx)
            && let AiUntrustedProposal::KeyframeTimingProposal {
                layer_name,
                start_frame,
                end_frame,
                easing_hint,
                ..
            } = card.proposal.clone()
            && let Ok(msg) =
                self.apply_ai_timing_proposal(&layer_name, start_frame, end_frame, &easing_hint)
        {
            card.state = ProposalCardState::Applied {
                message: msg,
                post_revision: self.history.revision(),
            };
        }

        if let Some(idx) = undo_card_idx
            && let Some(card) = answer.cards.get_mut(idx)
            && let ProposalCardState::Applied { post_revision, .. } = card.state
            && self.history.can_undo()
            && self.history.revision() == post_revision
        {
            self.undo();
            card.state = ProposalCardState::Pending;
        }

        if close_dialog {
            self.ai_session.cancel_and_clear();
        } else if stop_clicked {
            self.ai_session.answer_dialog = Some(answer);
            self.ai_session.stop_active_request();
        } else if reopen_new_request {
            let mut origin = answer.origin_send;
            origin.context_pack = self.build_ai_context_pack();
            self.ai_session.cancel_and_clear();
            self.ai_session.send_dialog = Some(origin);
        } else if retry_clicked {
            self.ai_session.answer_dialog = Some(answer);
            let _ = self.retry_ai_request();
        } else {
            if edit_connection
                && let Some(prof) = self.workspace.ai.profile(&answer.profile_id).cloned()
            {
                let has_key = self.ai_manager.has_secret(&prof.id);
                self.ai_connection_modal = Some(AiConnectionModal::new_edit(
                    &prof,
                    has_key,
                    AiConnectionModalTarget::Workspace,
                ));
            }
            self.ai_session.answer_dialog = Some(answer);
        }
    }
}

fn context_checkbox_row(
    ui: &mut Ui,
    checked: &mut bool,
    enabled: bool,
    label: &str,
    summary: &str,
    bottom_border: bool,
) {
    let p = neoflash_ui::palette(ui.ctx());
    let row_h = neoflash_ui::metrics::control();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_h),
        egui::Sense::hover(),
    );
    if bottom_border {
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0_f32, p.border_subtle),
        );
    }
    let inner = rect.shrink2(egui::vec2(space::S3, 0.0));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.scope(|ui| {
                if !enabled {
                    ui.disable();
                }
                ui.add(c::Checkbox::new(checked, label));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(rich(summary, TextStyle::Small).color(p.text_secondary));
            });
        },
    );
}

fn readonly_answer_box(
    ui: &mut Ui,
    width: f32,
    summary: Option<&str>,
    body: &str,
    max_height: f32,
) {
    let p = neoflash_ui::palette(ui.ctx());
    egui::Frame::new()
        .fill(p.bg_field)
        .stroke(Stroke::new(1.0_f32, p.border_subtle))
        .corner_radius(egui::CornerRadius::same(radius::MD as u8))
        .inner_margin(egui::Margin::same(space::S3 as i8))
        .show(ui, |ui| {
            let content_w = (width - 2.0 * space::S3).max(48.0);
            ui.set_width(content_w);
            egui::ScrollArea::vertical()
                .id_salt(("ai-readonly-answer", summary.unwrap_or("stream")))
                .max_height(max_height)
                .auto_shrink([false, true])
                .themed()
                .show(ui, |ui| {
                    ui.set_width(content_w);
                    ui.spacing_mut().item_spacing.y = space::S2;
                    if let Some(s) = summary
                        && !s.trim().is_empty()
                    {
                        ui.add(
                            egui::Label::new(rich(s, TextStyle::BodyStrong).color(p.text_primary))
                                .wrap(),
                        );
                    }
                    if !body.trim().is_empty() {
                        ui.add(
                            egui::Label::new(rich(body, TextStyle::Body).color(p.text_primary))
                                .wrap(),
                        );
                    }
                });
        });
}

#[allow(clippy::too_many_arguments)]
fn render_proposal_card(
    ui: &mut Ui,
    editor: &Editor,
    width: f32,
    idx: usize,
    card: &mut AiProposalCard,
    current_revision: u64,
    can_undo_now: bool,
    apply_code_idx: &mut Option<usize>,
    apply_timing_idx: &mut Option<usize>,
    undo_card_idx: &mut Option<usize>,
) {
    let p = neoflash_ui::palette(ui.ctx());
    egui::Frame::new()
        .fill(p.bg_panel)
        .stroke(Stroke::new(1.0_f32, p.border_subtle))
        .corner_radius(egui::CornerRadius::same(radius::MD as u8))
        .inner_margin(egui::Margin::same(space::S3 as i8))
        .show(ui, |ui| {
            let card_w = (width - 2.0 * space::S3).max(64.0);
            ui.set_width(card_w);
            ui.spacing_mut().item_spacing.y = space::S2;

            match &card.proposal {
                AiUntrustedProposal::CodeSnippetProposal {
                    language,
                    target_hint,
                    code,
                    explanation,
                } => {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S2;
                        ui.add(c::Badge::new(
                            script_language_badge_label(*language),
                            c::BadgeType::Neutral,
                        ));
                        ui.label(rich(target_hint, TextStyle::BodyStrong).color(p.text_primary));
                        if matches!(card.state, ProposalCardState::Dismissed) {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        rich("Dismissed", TextStyle::Small).color(p.text_tertiary),
                                    );
                                },
                            );
                        }
                    });
                    if matches!(card.state, ProposalCardState::Dismissed) {
                        return;
                    }

                    egui::Frame::new()
                        .fill(p.bg_app)
                        .stroke(Stroke::new(1.0_f32, p.border_subtle))
                        .corner_radius(egui::CornerRadius::same(radius::SM as u8))
                        .inner_margin(egui::Margin::same(space::S2 as i8))
                        .show(ui, |ui| {
                            let code_w = (card_w - 2.0 * space::S2).max(48.0);
                            ui.set_width(code_w);
                            // R-099: at most eight lines before scrolling.
                            let eight_lines_h = 8.0 * TextStyle::Mono.spec().line_height;
                            egui::ScrollArea::vertical()
                                .id_salt(("ai-code-proposal", idx))
                                .max_height(eight_lines_h)
                                .auto_shrink([false, true])
                                .themed()
                                .show(ui, |ui| {
                                    ui.set_width(code_w);
                                    ui.add(
                                        egui::Label::new(
                                            rich(code, TextStyle::Mono).color(p.text_primary),
                                        )
                                        .wrap(),
                                    );
                                });
                        });

                    if !explanation.trim().is_empty() {
                        ui.add(
                            egui::Label::new(
                                rich(explanation, TextStyle::Small).color(p.text_secondary),
                            )
                            .wrap(),
                        );
                    }

                    match &card.state {
                        ProposalCardState::Pending => {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = space::S2;
                                if ui.add(c::Button::new("Insert into Actions")).clicked() {
                                    *apply_code_idx = Some(idx);
                                }
                                if ui.add(c::Button::ghost("Copy")).clicked() {
                                    ui.ctx().copy_text(code.clone());
                                }
                            });
                        }
                        ProposalCardState::Applied {
                            message,
                            post_revision,
                        } => {
                            let can_undo_this = can_undo_now && current_revision == *post_revision;
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = space::S3;
                                success_inline_message(ui, message);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add_enabled(can_undo_this, c::Button::ghost("Undo"))
                                            .clicked()
                                        {
                                            *undo_card_idx = Some(idx);
                                        }
                                    },
                                );
                            });
                        }
                        ProposalCardState::Dismissed => {}
                    }
                }
                AiUntrustedProposal::KeyframeTimingProposal {
                    layer_name,
                    start_frame,
                    end_frame,
                    easing_hint,
                    notes,
                } => {
                    let header_title = format!("Layer {layer_name}");
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S2;
                        ui.add(c::Badge::new("Timing", c::BadgeType::Neutral));
                        ui.label(rich(&header_title, TextStyle::BodyStrong).color(p.text_primary));
                        if matches!(card.state, ProposalCardState::Dismissed) {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        rich("Dismissed", TextStyle::Small).color(p.text_tertiary),
                                    );
                                },
                            );
                        }
                    });
                    if matches!(card.state, ProposalCardState::Dismissed) {
                        return;
                    }

                    let frames_str = format!("{start_frame} – {end_frame}");
                    let ease_str = human_easing_label(easing_hint);
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.add(c::TableRow {
                            cells: &["Frames", &frames_str],
                            kind: c::TableRowKind::Row,
                            columns: &[0.0, 0.35],
                        });
                        ui.add(c::TableRow {
                            cells: &["Easing", ease_str],
                            kind: c::TableRowKind::Row,
                            columns: &[0.0, 0.35],
                        });
                    });

                    if !notes.trim().is_empty() {
                        ui.add(
                            egui::Label::new(rich(notes, TextStyle::Small).color(p.text_secondary))
                                .wrap(),
                        );
                    }

                    match &card.state {
                        ProposalCardState::Pending => {
                            let validation = editor.validate_ai_timing_proposal(
                                layer_name,
                                *start_frame,
                                *end_frame,
                            );
                            if let Err(reason) = &validation {
                                ui.add(
                                    egui::Label::new(
                                        rich(reason, TextStyle::Small).color(p.warning_default),
                                    )
                                    .wrap(),
                                );
                            }
                            let mut dismiss_now = false;
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = space::S2;
                                if ui
                                    .add_enabled(
                                        validation.is_ok(),
                                        c::Button::new("Apply to timeline"),
                                    )
                                    .clicked()
                                {
                                    *apply_timing_idx = Some(idx);
                                }
                                if ui.add(c::Button::ghost("Dismiss")).clicked() {
                                    dismiss_now = true;
                                }
                            });
                            if dismiss_now {
                                card.state = ProposalCardState::Dismissed;
                            }
                        }
                        ProposalCardState::Applied {
                            message,
                            post_revision,
                        } => {
                            let can_undo_this = can_undo_now && current_revision == *post_revision;
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = space::S3;
                                success_inline_message(ui, message);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add_enabled(can_undo_this, c::Button::ghost("Undo"))
                                            .clicked()
                                        {
                                            *undo_card_idx = Some(idx);
                                        }
                                    },
                                );
                            });
                        }
                        ProposalCardState::Dismissed => {}
                    }
                }
                AiUntrustedProposal::ExplanationText { .. } => {}
            }
        });
}
