//! Structured, bounded Output event pipeline (`T-310`).
//!
//! Replaces raw `Vec<String>` Output storage with a per-document ring-bounded structured log
//! carrying severity, subsystem source, document-session identity, monotonic/wall-clock timestamp,
//! consecutive-duplicate coalescing (`repeat_count`), optional jump-to-source `OutputLocation`,
//! and a strictly monotonic `revision` counter for O(1) UI cache invalidation.

use crate::ai_connections::{AiConnectionError, redact_sensitive_tokens};
use crate::app::Editor;
use crate::panels::actions_sidebar::ScriptLocation;
use eframe::egui;
use neoflash_core::{Id, ObjectEvent};
use neoflash_script::{Diagnostic, DiagnosticSeverity};
use neoflash_ui::components::{self as nf, code::Severity};
use std::fmt;
use std::ops::{Deref, Index, IndexMut, Range, RangeFrom, RangeFull, RangeTo};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Default per-document ring capacity for structured Output events.
pub const MAX_OUTPUT_EVENTS: usize = 8_192;

/// Maximum UTF-8 byte budget retained per single Output event message.
pub const MAX_OUTPUT_EVENT_BYTES: usize = 64 * 1024;

static NEXT_PIPELINE_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

fn next_pipeline_instance_id() -> u64 {
    NEXT_PIPELINE_INSTANCE_ID.fetch_add(1, Ordering::Relaxed)
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn clamp_utf8_bytes(mut text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    const ELLIPSIS: &str = "…";
    let suffix = if max_bytes >= ELLIPSIS.len() {
        ELLIPSIS
    } else {
        ""
    };
    let mut end = max_bytes.saturating_sub(suffix.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str(suffix);
    text
}

/// Structured Output message severity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OutputSeverity {
    #[default]
    Info,
    Warning,
    Error,
    Trace,
}

impl OutputSeverity {
    /// Map to the NF2 `LogLine` component severity without changing NF2 visuals.
    pub const fn ui_severity(self) -> Severity {
        match self {
            Self::Info | Self::Trace => Severity::Info,
            Self::Warning => Severity::Warning,
            Self::Error => Severity::Error,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Warning => "Warning",
            Self::Error => "Error",
            Self::Trace => "Trace",
        }
    }

    pub const fn slug(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Trace => "trace",
        }
    }

    pub const fn from_toast(kind: nf::ToastType) -> Self {
        match kind {
            nf::ToastType::Info | nf::ToastType::Success => Self::Info,
            nf::ToastType::Warning => Self::Warning,
            nf::ToastType::Error => Self::Error,
        }
    }

    pub const fn from_diagnostic(severity: DiagnosticSeverity) -> Self {
        match severity {
            DiagnosticSeverity::Error => Self::Error,
            DiagnosticSeverity::Warning => Self::Warning,
            DiagnosticSeverity::Info => Self::Info,
        }
    }

    pub fn parse_token(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "info" => Some(Self::Info),
            "warn" | "warning" | "warnings" => Some(Self::Warning),
            "err" | "error" | "errors" => Some(Self::Error),
            "trace" | "traces" => Some(Self::Trace),
            _ => None,
        }
    }
}

/// Subsystem producer of a structured Output event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OutputSource {
    #[default]
    System,
    Trace,
    Compiler,
    Runtime,
    Publish,
    Import,
    Debugger,
    Updater,
    Ai,
}

impl OutputSource {
    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Trace => "Trace",
            Self::Compiler => "Compiler",
            Self::Runtime => "Runtime",
            Self::Publish => "Publish",
            Self::Import => "Import",
            Self::Debugger => "Debugger",
            Self::Updater => "Updater",
            Self::Ai => "AI",
        }
    }

    pub const fn slug(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Trace => "trace",
            Self::Compiler => "compiler",
            Self::Runtime => "runtime",
            Self::Publish => "publish",
            Self::Import => "import",
            Self::Debugger => "debugger",
            Self::Updater => "updater",
            Self::Ai => "ai",
        }
    }

    pub fn parse_token(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "system" | "sys" => Some(Self::System),
            "trace" => Some(Self::Trace),
            "compiler" | "compile" | "actions" | "script" => Some(Self::Compiler),
            "runtime" | "preview" | "swf" => Some(Self::Runtime),
            "publish" | "export" => Some(Self::Publish),
            "import" => Some(Self::Import),
            "debugger" | "debug" => Some(Self::Debugger),
            "updater" | "update" => Some(Self::Updater),
            "ai" => Some(Self::Ai),
            _ => None,
        }
    }
}

/// Optional structured provenance for navigating from an Output row to an authored script,
/// timeline frame, object handler, or project source file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputLocation {
    pub document_id: Option<String>,
    pub file_path: Option<PathBuf>,
    pub source_id: Option<Id>,
    pub symbol_id: Option<Id>,
    pub symbol_name: Option<String>,
    pub layer_index: Option<usize>,
    pub frame_index: Option<u32>,
    pub element_id: Option<Id>,
    pub object_event: Option<ObjectEvent>,
    /// 1-based start line.
    pub line: Option<usize>,
    /// 1-based start column.
    pub column: Option<usize>,
    /// Optional 1-based end line for span selection.
    pub end_line: Option<usize>,
    /// Optional 1-based end column for span selection.
    pub end_column: Option<usize>,
}

impl OutputLocation {
    pub fn for_file(path: impl Into<PathBuf>) -> Self {
        Self {
            file_path: Some(path.into()),
            ..Self::default()
        }
    }

    pub fn for_source(source_id: impl Into<Id>, path: impl Into<PathBuf>) -> Self {
        Self {
            source_id: Some(source_id.into()),
            file_path: Some(path.into()),
            ..Self::default()
        }
    }

    pub fn for_frame(
        symbol_id: impl Into<Id>,
        symbol_name: Option<String>,
        layer_index: Option<usize>,
        frame_index: u32,
    ) -> Self {
        Self {
            symbol_id: Some(symbol_id.into()),
            symbol_name,
            layer_index,
            frame_index: Some(frame_index),
            ..Self::default()
        }
    }

    pub fn for_object(
        symbol_id: impl Into<Id>,
        symbol_name: Option<String>,
        layer_index: Option<usize>,
        frame_index: u32,
        element_id: impl Into<Id>,
        object_event: ObjectEvent,
    ) -> Self {
        Self {
            symbol_id: Some(symbol_id.into()),
            symbol_name,
            layer_index,
            frame_index: Some(frame_index),
            element_id: Some(element_id.into()),
            object_event: Some(object_event),
            ..Self::default()
        }
    }

    pub fn with_document(mut self, document_id: impl Into<String>) -> Self {
        self.document_id = Some(document_id.into());
        self
    }

    pub fn with_line_col(mut self, line: usize, column: usize) -> Self {
        self.line = Some(line.max(1));
        self.column = Some(column.max(1));
        self
    }

    pub fn with_span(
        mut self,
        line: usize,
        column: usize,
        end_line: usize,
        end_column: usize,
    ) -> Self {
        let start_line = line.max(1);
        let start_col = column.max(1);
        self.line = Some(start_line);
        self.column = Some(start_col);
        self.end_line = Some(end_line.max(start_line));
        self.end_column = Some(end_column.max(1));
        self
    }

    /// True when clicking this location can navigate to a source unit, timeline frame, or script line.
    pub fn is_navigable(&self) -> bool {
        self.source_id.is_some()
            || self.symbol_id.is_some()
            || self.symbol_name.is_some()
            || self.frame_index.is_some()
            || self.line.is_some()
            || self.file_path.is_some()
    }

    /// Lowercase text summary used so location metadata participates in Output search.
    pub fn search_summary_lower(&self) -> String {
        let mut parts = Vec::new();
        if let Some(path) = &self.file_path {
            parts.push(path.to_string_lossy().to_lowercase());
        }
        if let Some(sym) = &self.symbol_name {
            parts.push(sym.to_lowercase());
        }
        if let Some(frame) = self.frame_index {
            parts.push(format!("frame {}", frame + 1));
        }
        if let Some(line) = self.line {
            parts.push(format!("line {line}"));
        }
        parts.join(" ")
    }
}

/// One structured Output event stored in [`OutputPipeline`].
#[derive(Clone, Debug)]
pub struct OutputEvent {
    pub id: u64,
    pub severity: OutputSeverity,
    pub source: OutputSource,
    pub document: String,
    pub timestamp_ms: u64,
    pub message: String,
    pub location: Option<OutputLocation>,
    pub repeat_count: u32,
}

impl OutputEvent {
    pub fn new(severity: OutputSeverity, source: OutputSource, message: impl Into<String>) -> Self {
        Self {
            id: 0,
            severity,
            source,
            document: String::new(),
            timestamp_ms: 0,
            message: clamp_utf8_bytes(message.into(), MAX_OUTPUT_EVENT_BYTES),
            location: None,
            repeat_count: 1,
        }
    }

    pub fn info(source: OutputSource, message: impl Into<String>) -> Self {
        Self::new(OutputSeverity::Info, source, message)
    }

    pub fn warning(source: OutputSource, message: impl Into<String>) -> Self {
        Self::new(OutputSeverity::Warning, source, message)
    }

    pub fn error(source: OutputSource, message: impl Into<String>) -> Self {
        Self::new(OutputSeverity::Error, source, message)
    }

    pub fn trace(message: impl Into<String>) -> Self {
        Self::new(OutputSeverity::Trace, OutputSource::Trace, message)
    }

    pub fn ai_info(message: impl Into<String>) -> Self {
        let raw = message.into();
        Self::new(
            OutputSeverity::Info,
            OutputSource::Ai,
            redact_sensitive_tokens(&raw),
        )
    }

    pub fn from_ai_error(error: &AiConnectionError) -> Self {
        let severity = match error {
            AiConnectionError::RateLimited { .. } | AiConnectionError::Cancelled => {
                OutputSeverity::Warning
            }
            _ => OutputSeverity::Error,
        };
        Self::new(severity, OutputSource::Ai, format!("AI: {error}"))
    }

    pub fn with_location(mut self, location: OutputLocation) -> Self {
        self.location = Some(location);
        self
    }

    pub fn with_optional_location(mut self, location: Option<OutputLocation>) -> Self {
        self.location = location;
        self
    }

    pub fn with_document(mut self, document: impl Into<String>) -> Self {
        self.document = document.into();
        self
    }

    pub fn with_timestamp_ms(mut self, timestamp_ms: u64) -> Self {
        self.timestamp_ms = timestamp_ms;
        self
    }

    pub fn as_str(&self) -> &str {
        &self.message
    }

    /// UTC `HH:MM:SS.mmm` formatted timestamp for structured inspection/export.
    pub fn formatted_timestamp(&self) -> String {
        let day_ms = self.timestamp_ms % 86_400_000;
        let hours = day_ms / 3_600_000;
        let minutes = (day_ms % 3_600_000) / 60_000;
        let seconds = (day_ms % 60_000) / 1_000;
        let millis = day_ms % 1_000;
        format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
    }

    pub fn same_coalesce_key(&self, other: &Self) -> bool {
        self.severity == other.severity
            && self.source == other.source
            && self.message == other.message
            && self.location == other.location
    }
}

impl PartialEq for OutputEvent {
    fn eq(&self, other: &Self) -> bool {
        self.severity == other.severity
            && self.source == other.source
            && self.document == other.document
            && self.message == other.message
            && self.location == other.location
            && self.repeat_count == other.repeat_count
    }
}

impl Eq for OutputEvent {}

impl Deref for OutputEvent {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.message
    }
}

impl AsRef<str> for OutputEvent {
    fn as_ref(&self) -> &str {
        &self.message
    }
}

impl PartialEq<str> for OutputEvent {
    fn eq(&self, other: &str) -> bool {
        self.message == other
    }
}

impl PartialEq<&str> for OutputEvent {
    fn eq(&self, other: &&str) -> bool {
        self.message == *other
    }
}

impl PartialEq<String> for OutputEvent {
    fn eq(&self, other: &String) -> bool {
        &self.message == other
    }
}

impl fmt::Display for OutputEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.repeat_count > 1 {
            write!(f, "{} (×{})", self.message, self.repeat_count)
        } else {
            f.write_str(&self.message)
        }
    }
}

/// Programmatic structured query over an [`OutputPipeline`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputFilter {
    pub severities: Vec<OutputSeverity>,
    pub sources: Vec<OutputSource>,
    pub document: Option<String>,
    pub search: String,
}

impl OutputFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_severity(mut self, severity: OutputSeverity) -> Self {
        if !self.severities.contains(&severity) {
            self.severities.push(severity);
        }
        self
    }

    pub fn with_severities(mut self, severities: impl IntoIterator<Item = OutputSeverity>) -> Self {
        for sev in severities {
            if !self.severities.contains(&sev) {
                self.severities.push(sev);
            }
        }
        self
    }

    pub fn with_source(mut self, source: OutputSource) -> Self {
        if !self.sources.contains(&source) {
            self.sources.push(source);
        }
        self
    }

    pub fn with_document(mut self, document: impl Into<String>) -> Self {
        self.document = Some(document.into());
        self
    }

    pub fn with_search(mut self, search: impl Into<String>) -> Self {
        self.search = search.into();
        self
    }
}

/// Parsed representation of the `"Filter output"` search box.
///
/// Supports structured prefix tokens (`error:`, `warn:`, `info:`, `trace:`,
/// `severity:error,warn`, `source:compiler,debugger`, `compiler:`, `debugger:`,
/// `publish:`, `import:`, `runtime:`, `updater:`, `ai:`) alongside plain substring search,
/// without adding unauthorized widgets to the NF2 Output dock.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedOutputSearch {
    pub raw_trimmed: String,
    pub severities: Vec<OutputSeverity>,
    pub sources: Vec<OutputSource>,
    pub text_needle_lower: String,
}

impl ParsedOutputSearch {
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Self::default();
        }
        let mut severities = Vec::new();
        let mut sources = Vec::new();
        let mut remaining_tokens = Vec::new();
        let mut any_qualifier = false;

        for token in trimmed.split_whitespace() {
            if let Some((prefix, rest)) = token.split_once(':') {
                let key = prefix.to_ascii_lowercase();
                match key.as_str() {
                    "severity" | "sev" | "level" => {
                        let parsed: Vec<OutputSeverity> = rest
                            .split(',')
                            .filter(|s| !s.is_empty())
                            .filter_map(OutputSeverity::parse_token)
                            .collect();
                        if !parsed.is_empty() {
                            any_qualifier = true;
                            for sev in parsed {
                                if !severities.contains(&sev) {
                                    severities.push(sev);
                                }
                            }
                            continue;
                        }
                    }
                    "source" | "src" => {
                        let parsed: Vec<OutputSource> = rest
                            .split(',')
                            .filter(|s| !s.is_empty())
                            .filter_map(OutputSource::parse_token)
                            .collect();
                        if !parsed.is_empty() {
                            any_qualifier = true;
                            for src in parsed {
                                if !sources.contains(&src) {
                                    sources.push(src);
                                }
                            }
                            continue;
                        }
                    }
                    _ => {
                        if let Some(sev) = OutputSeverity::parse_token(&key) {
                            any_qualifier = true;
                            if !severities.contains(&sev) {
                                severities.push(sev);
                            }
                            if !rest.is_empty() {
                                remaining_tokens.push(rest);
                            }
                            continue;
                        }
                        if let Some(src) = OutputSource::parse_token(&key) {
                            any_qualifier = true;
                            if !sources.contains(&src) {
                                sources.push(src);
                            }
                            if !rest.is_empty() {
                                remaining_tokens.push(rest);
                            }
                            continue;
                        }
                    }
                }
            }
            remaining_tokens.push(token);
        }

        let text_needle_lower = if any_qualifier {
            remaining_tokens.join(" ").to_lowercase()
        } else {
            trimmed.to_lowercase()
        };

        Self {
            raw_trimmed: trimmed.to_owned(),
            severities,
            sources,
            text_needle_lower,
        }
    }

    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        !self.raw_trimmed.is_empty()
    }

    pub fn matches_event_metadata(&self, event: &OutputEvent) -> bool {
        if !self.severities.is_empty() && !self.severities.contains(&event.severity) {
            return false;
        }
        if !self.sources.is_empty() && !self.sources.contains(&event.source) {
            return false;
        }
        true
    }

    pub fn matches_line(&self, line_text: &str, location_summary_lower: &str) -> bool {
        if self.text_needle_lower.is_empty() {
            return true;
        }
        line_text.to_lowercase().contains(&self.text_needle_lower)
            || (!location_summary_lower.is_empty()
                && location_summary_lower.contains(&self.text_needle_lower))
    }
}

/// Iterator over plain message strings inside [`OutputPipeline`], preserving legacy `.iter()` callers.
pub struct OutputMessageIter<'a> {
    inner: std::slice::Iter<'a, OutputEvent>,
}

impl<'a> Iterator for OutputMessageIter<'a> {
    type Item = &'a String;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|event| &event.message)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<'a> DoubleEndedIterator for OutputMessageIter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.inner.next_back().map(|event| &event.message)
    }
}

impl ExactSizeIterator for OutputMessageIter<'_> {}

/// Bounded, revision-tracked per-document Output event buffer.
///
/// Uses a head-indexed compacting ring over `Vec<OutputEvent>` so:
/// - Evicting the oldest event at capacity is amortized `O(1)` (immediate slot drop + periodic drain),
/// - Live events are always contiguous (`&self.events[self.head..]`) for `O(1)` slice indexing,
/// - `revision` increments strictly monotonically on every mutation (`push`, `coalesce`, `index_mut`,
///   `replace`, ring eviction, `clear`).
pub struct OutputPipeline {
    instance_id: u64,
    document: String,
    revision: u64,
    next_event_id: u64,
    last_timestamp_ms: u64,
    capacity: usize,
    evicted_count: u64,
    events: Vec<OutputEvent>,
    head: usize,
}

#[allow(dead_code)]
pub type OutputBuffer = OutputPipeline;
#[allow(dead_code)]
pub type OutputLog = OutputPipeline;

impl Default for OutputPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for OutputPipeline {
    fn clone(&self) -> Self {
        Self {
            instance_id: next_pipeline_instance_id(),
            document: self.document.clone(),
            revision: self.revision,
            next_event_id: self.next_event_id,
            last_timestamp_ms: self.last_timestamp_ms,
            capacity: self.capacity,
            evicted_count: self.evicted_count,
            events: self.events().to_vec(),
            head: 0,
        }
    }
}

impl fmt::Debug for OutputPipeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutputPipeline")
            .field("instance_id", &self.instance_id)
            .field("document", &self.document)
            .field("revision", &self.revision)
            .field("len", &self.len())
            .field("capacity", &self.capacity)
            .field("evicted_count", &self.evicted_count)
            .field("events", &self.events())
            .finish()
    }
}

impl OutputPipeline {
    pub fn new() -> Self {
        Self::with_capacity(MAX_OUTPUT_EVENTS)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let instance_id = next_pipeline_instance_id();
        Self {
            instance_id,
            document: format!("output-session-{instance_id}"),
            revision: 0,
            next_event_id: 1,
            last_timestamp_ms: 0,
            capacity: capacity.max(1),
            evicted_count: 0,
            events: Vec::new(),
            head: 0,
        }
    }

    pub fn with_document_id(document: impl Into<String>) -> Self {
        let mut pipeline = Self::new();
        pipeline.document = document.into();
        pipeline
    }

    /// Unique runtime identity of this pipeline instance (safe for O(1) cache keying across sessions).
    pub const fn instance_id(&self) -> u64 {
        self.instance_id
    }

    /// Strictly monotonic mutation counter bumped on every append, coalesce, edit, eviction, or clear.
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Owning document-session identifier stamped onto events recorded in this pipeline.
    pub fn document_id(&self) -> &str {
        &self.document
    }

    /// Synchronize the owning document-session identifier without mutating stored events if unchanged.
    pub fn set_document_id(&mut self, document: impl Into<String>) {
        let next = document.into();
        if self.document != next {
            self.document = next;
            for event in &mut self.events[self.head..] {
                if event.document.is_empty() || event.document.starts_with("output-session-") {
                    event.document = self.document.clone();
                }
                if let Some(location) = &mut event.location
                    && location.document_id.as_deref().is_none_or(|document_id| {
                        document_id.is_empty() || document_id.starts_with("output-session-")
                    })
                {
                    location.document_id = Some(event.document.clone());
                }
            }
            self.bump_revision();
        }
    }

    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn set_capacity(&mut self, capacity: usize) {
        let cap = capacity.max(1);
        self.capacity = cap;
        let mut evicted_any = false;
        while self.len() > cap {
            self.evict_oldest_one();
            evicted_any = true;
        }
        if evicted_any {
            self.bump_revision();
        }
    }

    pub const fn evicted_count(&self) -> u64 {
        self.evicted_count
    }

    pub fn len(&self) -> usize {
        self.events.len().saturating_sub(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn events(&self) -> &[OutputEvent] {
        &self.events[self.head..]
    }

    pub fn iter_events(&self) -> std::slice::Iter<'_, OutputEvent> {
        self.events().iter()
    }

    pub fn iter(&self) -> OutputMessageIter<'_> {
        OutputMessageIter {
            inner: self.events().iter(),
        }
    }

    pub fn first(&self) -> Option<&String> {
        self.events().first().map(|e| &e.message)
    }

    pub fn last(&self) -> Option<&String> {
        self.events().last().map(|e| &e.message)
    }

    pub fn first_event(&self) -> Option<&OutputEvent> {
        self.events().first()
    }

    pub fn last_event(&self) -> Option<&OutputEvent> {
        self.events().last()
    }

    pub fn contains(&self, message: &str) -> bool {
        self.events().iter().any(|e| e.message == message)
    }

    pub fn contains_event(
        &self,
        source: OutputSource,
        severity: OutputSeverity,
        message: &str,
    ) -> bool {
        self.events()
            .iter()
            .any(|e| e.source == source && e.severity == severity && e.message == message)
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn next_timestamp_ms(&mut self) -> u64 {
        let now = unix_now_ms().max(self.last_timestamp_ms);
        self.last_timestamp_ms = now;
        now
    }

    fn evict_oldest_one(&mut self) {
        if self.head < self.events.len() {
            // Release heap allocations on the evicted slot immediately; compact amortizing O(1).
            self.events[self.head] = OutputEvent {
                id: 0,
                severity: OutputSeverity::Info,
                source: OutputSource::System,
                document: String::new(),
                timestamp_ms: 0,
                message: String::new(),
                location: None,
                repeat_count: 0,
            };
            self.head += 1;
            self.evicted_count = self.evicted_count.saturating_add(1);
            if self.head >= self.capacity {
                self.events.drain(..self.head);
                self.head = 0;
            }
        }
    }

    fn stamp_event(&mut self, mut event: OutputEvent) -> OutputEvent {
        let id = self.next_event_id;
        self.next_event_id = self.next_event_id.saturating_add(1);
        event.id = id;
        if event.document.is_empty() {
            event.document = self.document.clone();
        }
        if event.timestamp_ms == 0 {
            event.timestamp_ms = self.next_timestamp_ms();
        } else {
            self.last_timestamp_ms = self.last_timestamp_ms.max(event.timestamp_ms);
        }
        event.message = clamp_utf8_bytes(event.message, MAX_OUTPUT_EVENT_BYTES);
        event.repeat_count = event.repeat_count.max(1);
        if let Some(loc) = &mut event.location
            && loc.document_id.is_none()
        {
            loc.document_id = Some(event.document.clone());
        }
        event
    }

    /// Push a plain-text `Info` / `System` message (backward-compatible with `Vec<String>::push`).
    pub fn push(&mut self, message: impl Into<String>) -> u64 {
        self.push_event(OutputEvent::new(
            OutputSeverity::Info,
            OutputSource::System,
            message,
        ))
    }

    /// Append a structured [`OutputEvent`], evicting the oldest entry in amortized `O(1)` if full.
    pub fn push_event(&mut self, event: OutputEvent) -> u64 {
        if self.len() >= self.capacity {
            self.evict_oldest_one();
        }
        let stamped = self.stamp_event(event);
        let id = stamped.id;
        self.events.push(stamped);
        self.bump_revision();
        id
    }

    /// Coalesce consecutive identical `(severity, source, message, location)` events by bumping
    /// `repeat_count` on the tail entry instead of appending a duplicate row. Always bumps `revision`.
    pub fn push_coalesced(&mut self, mut event: OutputEvent) -> u64 {
        event.message = clamp_utf8_bytes(event.message, MAX_OUTPUT_EVENT_BYTES);
        if event.document.is_empty() {
            event.document = self.document.clone();
        }
        if let Some(loc) = &mut event.location
            && loc.document_id.is_none()
        {
            loc.document_id = Some(event.document.clone());
        }
        let ts = if event.timestamp_ms == 0 {
            self.next_timestamp_ms()
        } else {
            self.last_timestamp_ms = self.last_timestamp_ms.max(event.timestamp_ms);
            event.timestamp_ms
        };
        if self.head < self.events.len()
            && let Some(tail) = self.events.last_mut()
            && tail.same_coalesce_key(&event)
        {
            tail.repeat_count = tail.repeat_count.saturating_add(event.repeat_count.max(1));
            tail.timestamp_ms = ts;
            let id = tail.id;
            self.bump_revision();
            return id;
        }
        event.timestamp_ms = ts;
        self.push_event(event)
    }

    /// Coalesce a multi-event diagnostic batch (such as a multi-error `Check syntax` pass) against
    /// the trailing `batch.len()` events in this document pipeline so repeated checks never append
    /// duplicate diagnostic sequences.
    pub fn push_coalesced_batch(&mut self, batch: Vec<OutputEvent>) {
        if batch.is_empty() {
            return;
        }
        let normalized: Vec<OutputEvent> = batch
            .into_iter()
            .map(|mut ev| {
                ev.message = clamp_utf8_bytes(ev.message, MAX_OUTPUT_EVENT_BYTES);
                if ev.document.is_empty() && !self.document.is_empty() {
                    ev.document = self.document.clone();
                }
                if let Some(loc) = &mut ev.location
                    && !ev.document.is_empty()
                    && loc.document_id.is_none()
                {
                    loc.document_id = Some(ev.document.clone());
                }
                ev
            })
            .collect();
        let k = normalized.len();
        let live_len = self.len();
        if live_len >= k {
            let tail_start = self.head + live_len - k;
            let matches_tail = self.events[tail_start..]
                .iter()
                .zip(normalized.iter())
                .all(|(existing, incoming)| existing.same_coalesce_key(incoming));
            if matches_tail {
                let ts = self.next_timestamp_ms();
                for (existing, incoming) in
                    self.events[tail_start..].iter_mut().zip(normalized.iter())
                {
                    existing.repeat_count = existing
                        .repeat_count
                        .saturating_add(incoming.repeat_count.max(1));
                    existing.timestamp_ms = ts;
                }
                self.bump_revision();
                return;
            }
        }
        for ev in normalized {
            self.push_coalesced(ev);
        }
    }

    /// Append `event` only if no live entry in this document pipeline already carries the same
    /// message text. Returns `Some(id)` when inserted, or `None` when suppressed as a duplicate.
    pub fn push_unique(&mut self, mut event: OutputEvent) -> Option<u64> {
        let clamped = clamp_utf8_bytes(event.message, MAX_OUTPUT_EVENT_BYTES);
        if self.contains(&clamped) {
            return None;
        }
        event.message = clamped;
        Some(self.push_event(event))
    }

    /// Replace an existing event at `index` and bump `revision`.
    pub fn replace(&mut self, index: usize, event: OutputEvent) {
        let len = self.len();
        assert!(
            index < len,
            "OutputPipeline index {index} out of bounds (len {len})"
        );
        let stamped = self.stamp_event(event);
        self.events[self.head + index] = stamped;
        self.bump_revision();
    }

    /// Clear all retained events in this document pipeline and bump `revision`.
    pub fn clear(&mut self) {
        self.events.clear();
        self.events.shrink_to(64);
        self.head = 0;
        self.evicted_count = 0;
        self.bump_revision();
    }

    pub fn events_by_severity(
        &self,
        severity: OutputSeverity,
    ) -> impl Iterator<Item = &OutputEvent> {
        self.events().iter().filter(move |e| e.severity == severity)
    }

    pub fn events_by_source(&self, source: OutputSource) -> impl Iterator<Item = &OutputEvent> {
        self.events().iter().filter(move |e| e.source == source)
    }

    /// Programmatically filter structured events by severity, source, document, and search query.
    pub fn filter_events(&self, filter: &OutputFilter) -> Vec<&OutputEvent> {
        let parsed = ParsedOutputSearch::parse(&filter.search);
        self.events()
            .iter()
            .filter(|event| {
                if !filter.severities.is_empty() && !filter.severities.contains(&event.severity) {
                    return false;
                }
                if !filter.sources.is_empty() && !filter.sources.contains(&event.source) {
                    return false;
                }
                if let Some(doc) = &filter.document
                    && &event.document != doc
                {
                    return false;
                }
                if !parsed.matches_event_metadata(event) {
                    return false;
                }
                if parsed.text_needle_lower.is_empty() {
                    return true;
                }
                let loc_summary = event
                    .location
                    .as_ref()
                    .map(OutputLocation::search_summary_lower)
                    .unwrap_or_default();
                parsed.matches_line(&event.message, &loc_summary)
            })
            .collect()
    }
}

impl FromIterator<String> for OutputPipeline {
    fn from_iter<T: IntoIterator<Item = String>>(iter: T) -> Self {
        let mut pipeline = Self::new();
        for msg in iter {
            pipeline.push(msg);
        }
        pipeline
    }
}

impl FromIterator<OutputEvent> for OutputPipeline {
    fn from_iter<T: IntoIterator<Item = OutputEvent>>(iter: T) -> Self {
        let mut pipeline = Self::new();
        for event in iter {
            pipeline.push_event(event);
        }
        pipeline
    }
}

impl PartialEq for OutputPipeline {
    fn eq(&self, other: &Self) -> bool {
        self.events() == other.events()
    }
}

impl Eq for OutputPipeline {}

impl PartialEq<Vec<String>> for OutputPipeline {
    fn eq(&self, other: &Vec<String>) -> bool {
        self.as_slice_eq(other.as_slice())
    }
}

impl PartialEq<[String]> for OutputPipeline {
    fn eq(&self, other: &[String]) -> bool {
        self.as_slice_eq(other)
    }
}

impl PartialEq<&[String]> for OutputPipeline {
    fn eq(&self, other: &&[String]) -> bool {
        self.as_slice_eq(other)
    }
}

impl OutputPipeline {
    fn as_slice_eq(&self, other: &[String]) -> bool {
        let events = self.events();
        events.len() == other.len()
            && events
                .iter()
                .zip(other.iter())
                .all(|(ev, text)| &ev.message == text)
    }
}

impl Index<usize> for OutputPipeline {
    type Output = String;

    fn index(&self, index: usize) -> &Self::Output {
        &self.events()[index].message
    }
}

impl IndexMut<usize> for OutputPipeline {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        let len = self.len();
        assert!(
            index < len,
            "OutputPipeline index {index} out of bounds (len {len})"
        );
        self.bump_revision();
        let slot = &mut self.events[self.head + index];
        slot.repeat_count = 1;
        &mut slot.message
    }
}

impl Index<RangeFrom<usize>> for OutputPipeline {
    type Output = [OutputEvent];

    fn index(&self, index: RangeFrom<usize>) -> &Self::Output {
        &self.events()[index]
    }
}

impl Index<Range<usize>> for OutputPipeline {
    type Output = [OutputEvent];

    fn index(&self, index: Range<usize>) -> &Self::Output {
        &self.events()[index]
    }
}

impl Index<RangeTo<usize>> for OutputPipeline {
    type Output = [OutputEvent];

    fn index(&self, index: RangeTo<usize>) -> &Self::Output {
        &self.events()[index]
    }
}

impl Index<RangeFull> for OutputPipeline {
    type Output = [OutputEvent];

    fn index(&self, _index: RangeFull) -> &Self::Output {
        self.events()
    }
}

impl<'a> IntoIterator for &'a OutputPipeline {
    type Item = &'a OutputEvent;
    type IntoIter = std::slice::Iter<'a, OutputEvent>;

    fn into_iter(self) -> Self::IntoIter {
        self.events().iter()
    }
}

/// Move the Actions caret (or select `[start..end]` with primary caret at `start`) and schedule
/// the Actions `ScrollArea` to scroll the target line into view on its next layout pass.
pub(crate) fn jump_to_span(
    ctx: &egui::Context,
    id: egui::Id,
    source: &str,
    line: usize,
    column: usize,
    end_line: Option<usize>,
    end_column: Option<usize>,
) {
    let start_line = line.max(1);
    let start_col = column.max(1);
    let start = nf::code::char_index(source, start_line, start_col);
    let end = match (end_line, end_column) {
        (Some(el), Some(ec)) => nf::code::char_index(source, el.max(start_line), ec.max(1)),
        (Some(el), None) if el > start_line => nf::code::char_index(source, el, 1),
        _ => start,
    };
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    let range = if end > start {
        egui::text::CCursorRange::two(
            egui::text::CCursor::new(end),
            egui::text::CCursor::new(start),
        )
    } else {
        egui::text::CCursorRange::one(egui::text::CCursor::new(start))
    };
    state.cursor.set_char_range(Some(range));
    state.store(ctx, id);
    ctx.memory_mut(|memory| memory.request_focus(id));
    ctx.data_mut(|data| data.insert_temp(id.with("pending-scroll-to-caret"), true));
}

impl Editor {
    /// Synchronize the active document pipeline's session identity with `self.clipboard.session`.
    pub(crate) fn sync_output_document_id(&mut self) {
        if !self.clipboard.session.is_empty() {
            self.output.set_document_id(self.clipboard.session.clone());
        }
    }

    /// Plain-text Output append (`Info` / `System`), preserving backward compatibility.
    #[allow(dead_code)]
    pub fn push_output(&mut self, msg: impl Into<String>) -> u64 {
        self.sync_output_document_id();
        self.output.push(msg)
    }

    /// Append a structured [`OutputEvent`] to the active document's Output pipeline.
    pub fn push_output_event(&mut self, event: OutputEvent) -> u64 {
        self.sync_output_document_id();
        self.output.push_event(event)
    }

    /// Coalesce consecutive identical structured Output events in the active document's pipeline.
    pub fn push_output_coalesced(&mut self, event: OutputEvent) -> u64 {
        self.sync_output_document_id();
        self.output.push_coalesced(event)
    }

    /// Permanently purge all stored Output events for the active document, drain any pending
    /// loopback `PreviewHttpServer` items or live-preview traces, and clear runtime error overlays.
    pub fn clear_output(&mut self, ctx: Option<&egui::Context>) {
        self.output.clear();
        crate::preview::purge_shared_preview_output_on_clear(&self.clipboard.session);
        if let Some(preview) = &mut self.authored_preview {
            let _ = preview.playback.drain_traces();
            let _ = preview.playback.drain_diagnostics();
        }
        self.clear_runtime_diagnostics_in_actions();
        if let Some(ctx) = ctx {
            crate::panels::timeline::output::invalidate_output_panel_cache(
                ctx,
                &self.clipboard.session,
            );
        }
    }

    /// Coalesce a sequence of structured Output events against the trailing events of the active
    /// document's pipeline.
    pub(crate) fn push_output_batch_coalesced(&mut self, batch: Vec<OutputEvent>) {
        self.sync_output_document_id();
        let mut first_compiler_err_loc: Option<OutputLocation> = None;
        let formatted_batch: Vec<OutputEvent> = batch
            .into_iter()
            .map(|mut ev| {
                if ev.source == OutputSource::Compiler && ev.location.is_some() {
                    if ev.severity == OutputSeverity::Error && first_compiler_err_loc.is_none() {
                        first_compiler_err_loc = ev.location.clone();
                    }
                    ev.message = format_structured_script_diagnostic_message(
                        &self.project,
                        ev.severity,
                        ev.location.as_ref(),
                        None,
                        ev.location.as_ref().and_then(|l| l.line),
                        &ev.message,
                        None,
                    );
                }
                ev
            })
            .collect();
        self.output.push_coalesced_batch(formatted_batch);
        if first_compiler_err_loc.is_some() {
            self.highlight_compiler_error_in_actions(first_compiler_err_loc.as_ref());
        }
    }

    /// Append a structured [`OutputEvent`] only if its message is not already present in the
    /// active document's Output pipeline.
    pub fn push_output_unique(&mut self, event: OutputEvent) -> Option<u64> {
        self.sync_output_document_id();
        self.output.push_unique(event)
    }

    /// Set `self.status` and record a coalesced structured [`OutputEvent`] in Output.
    pub(crate) fn message_event(
        &mut self,
        severity: OutputSeverity,
        source: OutputSource,
        msg: impl Into<String>,
        location: Option<OutputLocation>,
    ) -> u64 {
        let text = msg.into();
        self.status = text.clone();
        let output_text = if source == OutputSource::Publish
            && severity == OutputSeverity::Error
            && text.starts_with("Export failed: script compilation failed:")
        {
            "[Error] Export aborted due to script compile error(s) — see exact line(s) below:"
                .to_owned()
        } else {
            text
        };
        self.push_output_coalesced(
            OutputEvent::new(severity, source, output_text).with_optional_location(location),
        )
    }

    /// Minimal producer hook for redacted AI status/diagnostic messages.
    #[allow(dead_code)]
    pub fn push_ai_output(&mut self, severity: OutputSeverity, msg: impl Into<String>) -> u64 {
        let redacted = redact_sensitive_tokens(&msg.into());
        self.push_output_coalesced(OutputEvent::new(severity, OutputSource::Ai, redacted))
    }

    /// Minimal producer hook for [`AiConnectionError`] reporting into Output.
    #[allow(dead_code)]
    pub fn report_ai_error(&mut self, error: &AiConnectionError) -> u64 {
        let event = OutputEvent::from_ai_error(error);
        self.status = event.message.clone();
        self.push_output_coalesced(event)
    }

    /// Build an [`OutputLocation`] for the currently open Actions script and optional compiler [`Diagnostic`].
    pub(crate) fn output_location_for_diagnostic(
        &self,
        diagnostic: Option<&Diagnostic>,
    ) -> Option<OutputLocation> {
        let mut loc = if let Some(path) = diagnostic.and_then(|d| d.path.as_deref()) {
            if let Some(unit) = self.project.sources.iter().find(|u| u.path == path) {
                OutputLocation::for_source(unit.id.clone(), PathBuf::from(&unit.path))
            } else {
                OutputLocation::for_file(PathBuf::from(path))
            }
        } else {
            match self.current_script_location()? {
                ScriptLocation::Source(source_id) => {
                    let path = self
                        .project
                        .sources
                        .iter()
                        .find(|u| u.id == source_id)
                        .map_or_else(|| PathBuf::from(&source_id), |u| PathBuf::from(&u.path));
                    OutputLocation::for_source(source_id, path)
                }
                ScriptLocation::Frame {
                    symbol,
                    layer,
                    frame,
                } => {
                    let sym_name = self.project.symbol(&symbol).map(|s| s.name.clone());
                    let layer_idx = self.project.symbol(&symbol).and_then(|sym| {
                        layer
                            .as_deref()
                            .and_then(|id| sym.timeline.layers.iter().position(|l| l.id == id))
                            .or_else(|| {
                                sym.timeline
                                    .layers
                                    .iter()
                                    .position(|l| l.name.eq_ignore_ascii_case("actions"))
                            })
                            .or_else(|| {
                                (self.layer < sym.timeline.layers.len()).then_some(self.layer)
                            })
                    });
                    OutputLocation::for_frame(symbol, sym_name, layer_idx, frame)
                }
                ScriptLocation::Object {
                    symbol,
                    frame,
                    element,
                    event,
                } => {
                    let sym_name = self.project.symbol(&symbol).map(|s| s.name.clone());
                    let layer_idx = self.project.symbol(&symbol).and_then(|sym| {
                        sym.timeline.layers.iter().position(|layer| {
                            layer
                                .key_at(frame)
                                .is_some_and(|k| k.elements.iter().any(|el| el.id == element))
                        })
                    });
                    OutputLocation::for_object(symbol, sym_name, layer_idx, frame, element, event)
                }
            }
        };
        loc.document_id = Some(self.clipboard.session.clone());
        if let Some(d) = diagnostic {
            loc = loc.with_span(d.line, d.column, d.end_line, d.end_column);
        }
        Some(loc)
    }

    /// Convert a resolved [`ScriptLocation`] + 1-based line/column into an [`OutputLocation`].
    pub(crate) fn output_location_from_script(
        &self,
        script: &ScriptLocation,
        line: Option<usize>,
        column: Option<usize>,
    ) -> OutputLocation {
        let mut loc = match script {
            ScriptLocation::Source(id) => {
                let path = self
                    .project
                    .sources
                    .iter()
                    .find(|u| &u.id == id)
                    .map_or_else(|| PathBuf::from(id), |u| PathBuf::from(&u.path));
                OutputLocation::for_source(id.clone(), path)
            }
            ScriptLocation::Frame {
                symbol,
                layer,
                frame,
            } => {
                let sym_name = self.project.symbol(symbol).map(|s| s.name.clone());
                let layer_idx = self.project.symbol(symbol).and_then(|sym| {
                    layer
                        .as_deref()
                        .and_then(|id| sym.timeline.layers.iter().position(|item| item.id == id))
                        .or_else(|| sym.timeline.action_layer_for_frame(*frame))
                });
                OutputLocation::for_frame(symbol.clone(), sym_name, layer_idx, *frame)
            }
            ScriptLocation::Object {
                symbol,
                frame,
                element,
                event,
            } => {
                let sym_name = self.project.symbol(symbol).map(|s| s.name.clone());
                let layer_idx = self.project.symbol(symbol).and_then(|sym| {
                    sym.timeline.layers.iter().position(|layer| {
                        layer
                            .key_at(*frame)
                            .is_some_and(|k| k.elements.iter().any(|el| &el.id == element))
                    })
                });
                OutputLocation::for_object(
                    symbol.clone(),
                    sym_name,
                    layer_idx,
                    *frame,
                    element.clone(),
                    *event,
                )
            }
        };
        loc.document_id = Some(self.clipboard.session.clone());
        if let Some(l) = line {
            loc = loc.with_line_col(l, column.unwrap_or(1));
        }
        loc
    }

    /// Navigate the editor to an [`OutputLocation`] when a user clicks a located `LogLine` in Output.
    ///
    /// Switches to the owning document tab if open, enters the target symbol/scene/layer/frame or
    /// project `.as`/`.js`/`.ts` `SourceUnit`, opens `Actions`, selects the exact line/span, and
    /// schedules the code editor scroll area to center the target line.
    pub fn navigate_to_output_location(
        &mut self,
        ctx: &egui::Context,
        loc: &OutputLocation,
    ) -> bool {
        if let Some(doc_id) = loc.document_id.as_deref()
            && !doc_id.is_empty()
            && self.clipboard.session != doc_id
            && let Some(tab_idx) = self
                .sessions
                .iter()
                .position(|slot| slot.as_ref().is_some_and(|s| s.clipboard.session == doc_id))
        {
            self.switch_session_tab(ctx, tab_idx);
        } else if let Some(path) = loc.file_path.as_deref()
            && let Some(tab_idx) = self.session_for_open_path(path)
            && tab_idx != self.active_session
        {
            self.switch_session_tab(ctx, tab_idx);
        }

        // 1. Project SourceUnit match (`source_id` or `file_path`).
        let matched_unit = loc
            .source_id
            .as_deref()
            .and_then(|id| self.project.sources.iter().find(|u| u.id == id))
            .or_else(|| {
                let path = loc.file_path.as_deref()?;
                let path_str = path.to_string_lossy();
                self.project.sources.iter().find(|u| {
                    u.path == path_str.as_ref()
                        || Path::new(&u.path) == path
                        || path
                            .file_name()
                            .and_then(|f| f.to_str())
                            .is_some_and(|name| u.path == name)
                })
            })
            .cloned();

        if let Some(unit) = matched_unit {
            let script_loc = ScriptLocation::Source(unit.id.clone());
            self.open_script_location(script_loc);
            self.open_actions();
            self.sync_actions_diagnostics_after_navigation();
            let editor_id = self.actions_editor_id();
            jump_to_span(
                ctx,
                editor_id,
                &unit.source,
                loc.line.unwrap_or(1),
                loc.column.unwrap_or(1),
                loc.end_line,
                loc.end_column,
            );
            return true;
        }

        // Resolve symbol ID if given by ID or name.
        let resolved_symbol_id = loc
            .symbol_id
            .as_deref()
            .filter(|id| self.project.symbol(id).is_some())
            .map(str::to_owned)
            .or_else(|| {
                let name = loc.symbol_name.as_deref()?;
                self.project
                    .symbols
                    .iter()
                    .find(|s| s.name == name)
                    .map(|s| s.id.clone())
            });

        // 2. Object event handler script match.
        if let (Some(element_id), Some(event)) = (loc.element_id.clone(), loc.object_event) {
            let symbol = resolved_symbol_id.unwrap_or_else(|| self.symbol_id().to_owned());
            if self.project.symbol(&symbol).is_none() {
                return false;
            }
            let frame = loc.frame_index.unwrap_or(self.frame);
            let script_loc = ScriptLocation::Object {
                symbol: symbol.clone(),
                frame,
                element: element_id,
                event,
            };
            let source = self.script_source_for(&script_loc).unwrap_or_default();
            self.open_script_location(script_loc);
            if let Some(layer_idx) = loc.layer_index
                && layer_idx < self.symbol().timeline.layers.len()
            {
                self.layer = layer_idx;
            }
            self.open_actions();
            self.sync_actions_diagnostics_after_navigation();
            let editor_id = self.actions_editor_id();
            jump_to_span(
                ctx,
                editor_id,
                &source,
                loc.line.unwrap_or(1),
                loc.column.unwrap_or(1),
                loc.end_line,
                loc.end_column,
            );
            return true;
        }

        // 3. Symbol / timeline frame / frame-script match.
        if resolved_symbol_id.is_some() || loc.frame_index.is_some() {
            let symbol = resolved_symbol_id.unwrap_or_else(|| self.symbol_id().to_owned());
            if self.project.symbol(&symbol).is_none() {
                return false;
            }
            let max_frame = self
                .project
                .symbol(&symbol)
                .map_or(0, |s| s.timeline.frame_count.saturating_sub(1));
            let frame = loc.frame_index.unwrap_or(self.frame).min(max_frame);
            let script_loc = ScriptLocation::Frame {
                symbol: symbol.clone(),
                layer: loc.layer_index.and_then(|index| {
                    self.project
                        .symbol(&symbol)
                        .and_then(|sym| sym.timeline.layers.get(index))
                        .map(|layer| layer.id.clone())
                }),
                frame,
            };
            let frame_source = self.script_source_for(&script_loc);
            self.open_script_location(script_loc);
            if let Some(layer_idx) = loc.layer_index
                && layer_idx < self.symbol().timeline.layers.len()
            {
                self.layer = layer_idx;
            }
            if let Some(el_id) = loc.element_id.as_ref() {
                if let Some(layer_id) = self
                    .symbol()
                    .timeline
                    .layers
                    .iter()
                    .find(|layer| {
                        layer
                            .key_at(self.frame)
                            .is_some_and(|k| k.elements.iter().any(|e| &e.id == el_id))
                    })
                    .map(|l| l.id.clone())
                    && !self.is_layer_id_effectively_soloed(&layer_id)
                {
                    self.clear_all_solo_layers();
                }
                self.select_single_id(el_id.clone());
                self.normalize_selection();
            }
            if loc.line.is_some() || frame_source.is_some() {
                let source = frame_source.unwrap_or_default();
                self.open_actions();
                self.sync_actions_diagnostics_after_navigation();
                let editor_id = self.actions_editor_id();
                jump_to_span(
                    ctx,
                    editor_id,
                    &source,
                    loc.line.unwrap_or(1),
                    loc.column.unwrap_or(1),
                    loc.end_line,
                    loc.end_column,
                );
            }
            return true;
        }

        // 4. Line-only location against the currently selected Actions target.
        if let Some(line) = loc.line
            && let Some(cur_loc) = self.current_script_location()
        {
            let source = self.script_source_for(&cur_loc).unwrap_or_default();
            self.open_actions();
            self.sync_actions_diagnostics_after_navigation();
            let editor_id = self.actions_editor_id();
            jump_to_span(
                ctx,
                editor_id,
                &source,
                line,
                loc.column.unwrap_or(1),
                loc.end_line,
                loc.end_column,
            );
            return true;
        }

        false
    }
}

fn select_matching_frame_script<'a>(
    symbol: &'a neoflash_core::Symbol,
    frame_idx: u32,
    hint_layer_name: Option<&str>,
    line_1based: Option<usize>,
    source_line_hint: Option<&str>,
) -> Option<&'a neoflash_core::FrameScript> {
    let candidates: Vec<&neoflash_core::FrameScript> = symbol
        .timeline
        .scripts
        .iter()
        .filter(|fs| fs.frame == frame_idx)
        .collect();
    if candidates.is_empty() {
        return None;
    }
    if let Some(layer_name) = hint_layer_name.map(str::trim).filter(|s| !s.is_empty())
        && let Some(layer) = symbol
            .timeline
            .layers
            .iter()
            .find(|l| l.name == layer_name || l.name.eq_ignore_ascii_case(layer_name))
        && let Some(&matched) = candidates
            .iter()
            .find(|fs| fs.layer.as_deref() == Some(layer.id.as_str()))
    {
        return Some(matched);
    }
    if let Some(snippet) = source_line_hint.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(line_no) = line_1based.filter(|&l| l >= 1)
            && let Some(&exact) = candidates.iter().find(|fs| {
                fs.script
                    .source
                    .lines()
                    .nth(line_no - 1)
                    .is_some_and(|l| l.trim() == snippet)
            })
        {
            return Some(exact);
        }
        if let Some(&anywhere) = candidates
            .iter()
            .find(|fs| fs.script.source.lines().any(|l| l.trim() == snippet))
        {
            return Some(anywhere);
        }
    }
    if let Some(line_no) = line_1based.filter(|&l| l > 1)
        && let Some(&in_bounds) = candidates
            .iter()
            .find(|fs| fs.script.source.lines().count() >= line_no)
    {
        return Some(in_bounds);
    }
    candidates.into_iter().next()
}

/// Resolve an exported script origin string (`"Scene 1 / frame 1"`,
/// `"Scene 1 · figures · Frame 1"`, `"Symbol / Layer / frame 1 / btn / Click"`,
/// or `"src/Main.as"`) back to an authored [`ScriptLocation`] in `project`.
#[allow(dead_code)]
pub(crate) fn resolve_script_location_in_project(
    project: &neoflash_core::Project,
    source: &str,
) -> Option<ScriptLocation> {
    resolve_script_location_in_project_with_hint(project, source, None, None)
}

pub(crate) fn resolve_script_location_in_project_with_hint(
    project: &neoflash_core::Project,
    source: &str,
    line: Option<usize>,
    source_line: Option<&str>,
) -> Option<ScriptLocation> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(unit) = project.sources.iter().find(|unit| {
        unit.path == trimmed
            || Path::new(&unit.path) == Path::new(trimmed)
            || Path::new(trimmed)
                .file_name()
                .and_then(|f| f.to_str())
                .is_some_and(|name| unit.path == name)
    }) {
        return Some(ScriptLocation::Source(unit.id.clone()));
    }
    for symbol in &project.symbols {
        for sep in [" / frame ", " · Frame ", " · frame "] {
            if let Some((prefix, frame_str)) = trimmed.rsplit_once(sep)
                && let Ok(frame_no) = frame_str.trim().parse::<u32>()
                && frame_no >= 1
            {
                let frame_idx = frame_no - 1;
                let prefix_trimmed = prefix.trim();
                let matched_layer_hint = if prefix_trimmed == symbol.name {
                    Some(None)
                } else {
                    prefix_trimmed
                        .strip_prefix(&format!("{} · ", symbol.name))
                        .or_else(|| prefix_trimmed.strip_prefix(&format!("{} / ", symbol.name)))
                        .map(|layer_part| Some(layer_part.trim()))
                };
                if let Some(layer_hint) = matched_layer_hint
                    && let Some(fs) = select_matching_frame_script(
                        symbol,
                        frame_idx,
                        layer_hint,
                        line,
                        source_line,
                    )
                {
                    return Some(ScriptLocation::Frame {
                        symbol: symbol.id.clone(),
                        layer: fs.layer.clone(),
                        frame: fs.frame,
                    });
                }
            }
        }
        for layer in &symbol.timeline.layers {
            for keyframe in &layer.keyframes {
                for element in &keyframe.elements {
                    for handler in &element.scripts {
                        let by_name = format!(
                            "{} / {} / frame {} / {} / {:?}",
                            symbol.name,
                            layer.name,
                            keyframe.frame + 1,
                            element.name,
                            handler.event
                        );
                        let by_id = format!(
                            "{} / {} / frame {} / {} / {:?}",
                            symbol.name,
                            layer.name,
                            keyframe.frame + 1,
                            element.id,
                            handler.event
                        );
                        if by_name == trimmed || by_id == trimmed {
                            return Some(ScriptLocation::Object {
                                symbol: symbol.id.clone(),
                                frame: keyframe.frame,
                                element: element.id.clone(),
                                event: handler.event,
                            });
                        }
                    }
                }
            }
        }
    }
    None
}

/// Convert a [`ScriptLocation`] in `project` into a navigable [`OutputLocation`] stamped with
/// `document_id` and optional 1-based `(line, column)`.
pub(crate) fn output_location_for_project_script(
    project: &neoflash_core::Project,
    document_id: &str,
    script: &ScriptLocation,
    line: Option<usize>,
    column: Option<usize>,
) -> OutputLocation {
    let mut loc = match script {
        ScriptLocation::Source(id) => {
            let path = project
                .sources
                .iter()
                .find(|u| &u.id == id)
                .map_or_else(|| PathBuf::from(id), |u| PathBuf::from(&u.path));
            OutputLocation::for_source(id.clone(), path)
        }
        ScriptLocation::Frame {
            symbol,
            layer,
            frame,
        } => {
            let sym_name = project.symbol(symbol).map(|s| s.name.clone());
            let layer_idx = project.symbol(symbol).and_then(|sym| {
                layer
                    .as_deref()
                    .and_then(|id| sym.timeline.layers.iter().position(|item| item.id == id))
                    .or_else(|| sym.timeline.action_layer_for_frame(*frame))
            });
            OutputLocation::for_frame(symbol.clone(), sym_name, layer_idx, *frame)
        }
        ScriptLocation::Object {
            symbol,
            frame,
            element,
            event,
        } => {
            let sym_name = project.symbol(symbol).map(|s| s.name.clone());
            let layer_idx = project.symbol(symbol).and_then(|sym| {
                sym.timeline.layers.iter().position(|layer| {
                    layer
                        .key_at(*frame)
                        .is_some_and(|k| k.elements.iter().any(|el| &el.id == element))
                })
            });
            OutputLocation::for_object(
                symbol.clone(),
                sym_name,
                layer_idx,
                *frame,
                element.clone(),
                *event,
            )
        }
    };
    if !document_id.is_empty() {
        loc.document_id = Some(document_id.to_owned());
    }
    if let Some(l) = line {
        loc = loc.with_line_col(l.max(1), column.unwrap_or(1).max(1));
    }
    loc
}

/// Resolve a runtime `trace()` or exception payload (`source_path`, `line`, `column`, `message`)
/// from `Test Movie` / `Test Scene` back to a navigable [`OutputLocation`] in `project`.
#[allow(dead_code)]
pub(crate) fn resolve_runtime_output_location(
    project: &neoflash_core::Project,
    document_id: &str,
    source_path: Option<&str>,
    line: Option<usize>,
    column: Option<usize>,
    message: &str,
) -> Option<OutputLocation> {
    resolve_runtime_output_location_with_snippet(
        project,
        document_id,
        source_path,
        line,
        column,
        message,
        None,
    )
}

pub(crate) fn resolve_runtime_output_location_with_snippet(
    project: &neoflash_core::Project,
    document_id: &str,
    source_path: Option<&str>,
    line: Option<usize>,
    column: Option<usize>,
    message: &str,
    source_line: Option<&str>,
) -> Option<OutputLocation> {
    let inferred_prefix = source_path
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            message
                .split_once(": ")
                .map(|(head, _)| head.trim())
                .filter(|head| {
                    resolve_script_location_in_project_with_hint(project, head, line, source_line)
                        .is_some()
                })
        });
    let raw = inferred_prefix?;
    if let Some(script_loc) =
        resolve_script_location_in_project_with_hint(project, raw, line, source_line)
    {
        return Some(output_location_for_project_script(
            project,
            document_id,
            &script_loc,
            line,
            column,
        ));
    }
    for sep in [" / frame ", " · Frame ", " · frame "] {
        if let Some((sym_name, frame_str)) = raw.rsplit_once(sep)
            && let Ok(frame_1based) = frame_str.trim().parse::<u32>()
            && frame_1based >= 1
            && let Some(sym) = project.symbols.iter().find(|s| s.name == sym_name.trim())
        {
            let max_frame = sym.timeline.frame_count.saturating_sub(1);
            let frame_idx = (frame_1based - 1).min(max_frame);
            let layer_idx = sym.timeline.action_layer_for_frame(frame_idx);
            let mut loc = OutputLocation::for_frame(
                sym.id.clone(),
                Some(sym.name.clone()),
                layer_idx,
                frame_idx,
            );
            if !document_id.is_empty() {
                loc.document_id = Some(document_id.to_owned());
            }
            if let Some(l) = line {
                loc = loc.with_line_col(l.max(1), column.unwrap_or(1).max(1));
            }
            return Some(loc);
        }
    }
    if raw.starts_with("Shared script ") {
        for sym in &project.symbols {
            if let Some(fs) = sym
                .timeline
                .scripts
                .iter()
                .find(|s| !s.script.source.trim().is_empty())
            {
                let script_loc = ScriptLocation::Frame {
                    symbol: sym.id.clone(),
                    layer: fs.layer.clone(),
                    frame: fs.frame,
                };
                return Some(output_location_for_project_script(
                    project,
                    document_id,
                    &script_loc,
                    line,
                    column,
                ));
            }
        }
    }
    None
}

/// Format an authored script [`OutputLocation`] into a human-readable header such as
/// `"Scene 1 · figures · Frame 1, Line 19"` or `"src/Game.as, Line 3"`.
pub(crate) fn format_authored_output_location(
    project: &neoflash_core::Project,
    loc: Option<&OutputLocation>,
    fallback_source: Option<&str>,
    fallback_line: Option<usize>,
) -> Option<String> {
    let line_num = loc.and_then(|l| l.line).or(fallback_line);
    let base = if let Some(loc) = loc {
        if let Some(src_id) = loc.source_id.as_deref()
            && let Some(unit) = project.sources.iter().find(|u| u.id == src_id)
        {
            Some(unit.path.clone())
        } else if let Some(path) = loc.file_path.as_deref() {
            Some(path.to_string_lossy().into_owned())
        } else if let Some(sym) = loc
            .symbol_id
            .as_deref()
            .and_then(|id| project.symbol(id))
            .or_else(|| {
                let name = loc.symbol_name.as_deref()?;
                project.symbols.iter().find(|s| s.name == name)
            })
        {
            let frame_1 = loc.frame_index.unwrap_or(0) + 1;
            let layer_name = loc
                .layer_index
                .and_then(|idx| sym.timeline.layers.get(idx))
                .or_else(|| {
                    sym.timeline
                        .action_layer_for_frame(loc.frame_index.unwrap_or(0))
                        .and_then(|idx| sym.timeline.layers.get(idx))
                })
                .map(|l| l.name.trim())
                .filter(|n| !n.is_empty());
            if let (Some(el_id), Some(ev)) = (loc.element_id.as_deref(), loc.object_event) {
                let target_name = sym
                    .timeline
                    .layers
                    .iter()
                    .filter_map(|layer| layer.key_at(loc.frame_index.unwrap_or(0)))
                    .flat_map(|kf| &kf.elements)
                    .find(|el| el.id == el_id)
                    .map(|el| {
                        if el.name.is_empty() {
                            "object"
                        } else {
                            el.name.as_str()
                        }
                    })
                    .unwrap_or("object");
                match layer_name {
                    Some(lname) => Some(format!(
                        "{} · {lname} · Frame {frame_1} ({target_name} · {ev:?})",
                        sym.name
                    )),
                    None => Some(format!(
                        "{} · Frame {frame_1} ({target_name} · {ev:?})",
                        sym.name
                    )),
                }
            } else {
                match layer_name {
                    Some(lname) => Some(format!("{} · {lname} · Frame {frame_1}", sym.name)),
                    None => Some(format!("{} · Frame {frame_1}", sym.name)),
                }
            }
        } else {
            fallback_source
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        }
    } else {
        fallback_source
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    }?;

    Some(match line_num {
        Some(line) if line >= 1 => format!("{base}, Line {line}"),
        _ => base,
    })
}

/// Look up the trimmed offending line of authored source code for `loc` in `project`.
pub(crate) fn source_line_snippet_for_output_location(
    project: &neoflash_core::Project,
    loc: &OutputLocation,
) -> Option<String> {
    let line_1based = loc.line.filter(|&l| l >= 1)?;
    let source: &str = if let Some(src_id) = loc.source_id.as_deref()
        && let Some(unit) = project.sources.iter().find(|u| u.id == src_id)
    {
        &unit.source
    } else {
        let sym = loc
            .symbol_id
            .as_deref()
            .and_then(|id| project.symbol(id))
            .or_else(|| {
                let name = loc.symbol_name.as_deref()?;
                project.symbols.iter().find(|s| s.name == name)
            })?;
        let frame = loc.frame_index.unwrap_or(0);
        if let (Some(el_id), Some(ev)) = (loc.element_id.as_deref(), loc.object_event) {
            sym.timeline
                .layers
                .iter()
                .filter_map(|layer| layer.key_at(frame))
                .flat_map(|kf| &kf.elements)
                .find(|el| el.id == el_id)
                .and_then(|el| el.scripts.iter().find(|os| os.event == ev))
                .map(|os| os.script.source.as_str())?
        } else {
            let layer_id = loc
                .layer_index
                .and_then(|idx| sym.timeline.layers.get(idx))
                .map(|l| l.id.as_str());
            sym.timeline
                .scripts
                .iter()
                .find(|fs| {
                    fs.frame == frame && layer_id.is_some() && fs.layer.as_deref() == layer_id
                })
                .or_else(|| {
                    sym.timeline
                        .scripts
                        .iter()
                        .find(|fs| fs.frame == frame && fs.layer.is_none())
                })
                .or_else(|| sym.timeline.scripts.iter().find(|fs| fs.frame == frame))
                .map(|fs| fs.script.source.as_str())?
        }
    };

    let raw_line = source.lines().nth(line_1based - 1)?.trim();
    if raw_line.is_empty() {
        return None;
    }
    Some(clamp_utf8_bytes(raw_line.to_owned(), 160))
}

fn snippet_has_length_call(s: &str) -> bool {
    let mut rest = s;
    while let Some(idx) = rest.find(".length") {
        let after = &rest[idx + ".length".len()..];
        if after.trim_start().starts_with('(') {
            return true;
        }
        rest = after;
    }
    false
}

fn extract_called_member_from_snippet(snippet: &str) -> Option<&str> {
    let bytes = snippet.as_bytes();
    let mut last_member: Option<&str> = None;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'.' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'$')
            {
                end += 1;
            }
            if end > start {
                let first = bytes[start];
                if !first.is_ascii_digit() {
                    let mut p = end;
                    while p < bytes.len() && bytes[p].is_ascii_whitespace() {
                        p += 1;
                    }
                    if p < bytes.len() && bytes[p] == b'(' {
                        last_member = snippet.get(start..end);
                    }
                }
            }
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    last_member
}

fn replace_remaining_internal_nf_identifiers(input: &str) -> String {
    if !input.contains("__nf") {
        return input.to_owned();
    }
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"__nf")
            && (i == 0
                || !(bytes[i - 1].is_ascii_alphanumeric()
                    || bytes[i - 1] == b'_'
                    || bytes[i - 1] == b'$'))
        {
            let mut end = i + 4;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'$')
            {
                end += 1;
            }
            if end > i + 4 {
                out.push_str("call");
                i = end;
                continue;
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Clean internal `__nf...` compiler identifiers and expand cryptic runtime errors (such as calling
/// numeric `.length` as `.length()` or Safari/V8 `Reflect.apply` / `Function.prototype.apply` leaks)
/// into clear, actionable explanations.
#[allow(dead_code)]
pub(crate) fn humanize_script_error_reason(raw: &str) -> String {
    humanize_script_error_reason_with_snippet(raw, None)
}

pub(crate) fn humanize_script_error_reason_with_snippet(
    raw: &str,
    snippet: Option<&str>,
) -> String {
    let mut text = raw.trim().to_owned();
    if text.is_empty() {
        return "Script error".to_owned();
    }
    for wrapper_prefix in [
        "Input callback: ",
        "Timer callback: ",
        "EnterFrame callback: ",
        "FrameConstructed callback: ",
        "ExitFrame callback: ",
        "FrameLabel callback: ",
    ] {
        if let Some(rest) = text.strip_prefix(wrapper_prefix) {
            text = rest.trim().to_owned();
        }
    }

    // Strip Safari/JSC internal `(In '...__nf...')` / `(near '...__nf...')` compiler call-site noise.
    for marker in [" (In '", " (near '"] {
        while let Some(idx) = text.find(marker) {
            let after = &text[idx + 1..];
            if let Some(end_rel) = after.find(')') {
                let parenthetical = &after[..=end_rel];
                if parenthetical.contains("__nf") {
                    text.replace_range(idx..idx + 1 + end_rel + 1, "");
                    continue;
                }
            }
            break;
        }
    }

    for (internal, clean) in [
        ("__nfContainerMethod", "method call"),
        ("__nfContainerRef", "property access"),
        ("__nfAs2Method", "timeline method"),
        ("__nfAs2Value", "function value"),
        ("__nfArrayMethod", "Array member"),
        ("__nfStringMethod", "String member"),
        ("__nfDateMethod", "Date member"),
        ("__nfFunctionMethod", "Function member"),
        ("__nfStringApply", "function call"),
        ("__nfArrayApply", "function call"),
        ("__nfVectorApply", "function call"),
        ("__nfApply", "function call"),
        ("__nfSourceFunction", "function"),
    ] {
        if text.contains(internal) {
            text = text.replace(internal, clean);
        }
    }
    text = replace_remaining_internal_nf_identifiers(&text);

    let snippet_str = snippet.unwrap_or("");
    let has_length_call = snippet_has_length_call(snippet_str) || snippet_has_length_call(&text);
    let is_apply_non_function = text
        .contains("Reflect.apply requires the first argument be a function")
        || text.contains("Reflect.apply: target is not callable")
        || (text.contains("Function.prototype.apply was called on ")
            && text.contains("and not a function"));
    if is_apply_non_function {
        if has_length_call {
            return "TypeError: 'length' is a property — use '.length' without '()'".to_owned();
        }
        if let Some(member) = extract_called_member_from_snippet(snippet_str) {
            return format!(
                "TypeError: '{member}' is not a function (attempted to call a non-function value)"
            );
        }
        return "TypeError: Attempted to call a non-function value".to_owned();
    }

    if let Some(idx) = text.find("Array/custom member '") {
        let rest = &text[idx + "Array/custom member '".len()..];
        if let Some((member, after_quote)) = rest.split_once('\'')
            && after_quote.starts_with(" is not callable")
        {
            return if member == "length" {
                "TypeError: 'length' is a numeric property, not a method — use '.length' without '()' (Array/custom member 'length' is not callable)."
                    .to_owned()
            } else {
                format!(
                    "TypeError: Array member '{member}' is not a function (Array/custom member '{member}' is not callable)."
                )
            };
        }
    }
    if text.contains(".length is not a function") && !text.contains("without '()'") {
        let prefixed = if text.starts_with("TypeError:") {
            text
        } else {
            format!("TypeError: {text}")
        };
        return format!("{prefixed} ('length' is a property — use '.length' without '()')");
    }
    text
}

fn strip_redundant_diagnostic_prefixes<'a>(
    mut raw: &'a str,
    fallback_source: Option<&str>,
) -> &'a str {
    raw = raw.trim();
    for sev_prefix in [
        "[Error] ",
        "[Warning] ",
        "[Info] ",
        "Warning: ",
        "Info: ",
        "Runtime error: ",
    ] {
        if let Some(rest) = raw.strip_prefix(sev_prefix) {
            raw = rest.trim();
        }
    }
    if let Some(rest) = raw.strip_prefix("Error: ")
        && (rest.starts_with("Line ") || rest.contains(" · Line "))
    {
        raw = rest.trim();
    }
    if let Some(src) = fallback_source.map(str::trim).filter(|s| !s.is_empty())
        && let Some(rest) = raw.strip_prefix(&format!("{src}: "))
    {
        raw = rest.trim();
    }
    // Strip compiler `location_label: ` prefix (`Scene 1 · layer "figures" · frame 1 · Line 19: `)
    // or `Line N, column M: ` prefix when we already render the authored location header.
    if let Some((head, tail)) = raw.split_once(": ")
        && (head.contains(" · Line ")
            || (head.starts_with("Line ")
                && head[5..].chars().next().is_some_and(|c| c.is_ascii_digit())))
    {
        raw = tail.trim();
        if let Some((head2, tail2)) = raw.split_once(": ")
            && head2.starts_with("Line ")
            && head2[5..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        {
            raw = tail2.trim();
        }
    }
    // Also strip `<Symbol> / frame <N>: ` prefix added by `executeDispatchedScript` in `runtime.js`!
    if let Some((head, tail)) = raw.split_once(": ")
        && (head.contains(" / frame ") || head.starts_with("Shared script "))
    {
        raw = tail.trim();
    }
    raw
}

/// Format a located compile or runtime diagnostic into `[Error] Scene · Layer · Frame, Line N: <reason>`
/// (with `\n  > <offending source line>` appended for errors when available).
pub(crate) fn format_structured_script_diagnostic_message(
    project: &neoflash_core::Project,
    severity: OutputSeverity,
    loc: Option<&OutputLocation>,
    fallback_source: Option<&str>,
    fallback_line: Option<usize>,
    raw_message: &str,
    explicit_snippet: Option<&str>,
) -> String {
    let badge = match severity {
        OutputSeverity::Error => "[Error]",
        OutputSeverity::Warning => "[Warning]",
        OutputSeverity::Info => "[Info]",
        OutputSeverity::Trace => "[trace]",
    };
    let snippet = explicit_snippet
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| clamp_utf8_bytes(s.to_owned(), 160))
        .or_else(|| loc.and_then(|l| source_line_snippet_for_output_location(project, l)));
    let stripped = strip_redundant_diagnostic_prefixes(raw_message, fallback_source);
    let reason = humanize_script_error_reason_with_snippet(stripped, snippet.as_deref());
    let header = format_authored_output_location(project, loc, fallback_source, fallback_line);
    let first_line = match header {
        Some(hdr) => format!("{badge} {hdr}: {reason}"),
        None => format!("{badge} {reason}"),
    };
    if severity == OutputSeverity::Error
        && let Some(code_line) = snippet
    {
        return format!("{first_line}\n  > {code_line}");
    }
    first_line
}
