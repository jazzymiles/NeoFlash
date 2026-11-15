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
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push('…');
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
            }
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
        self.head = 0;
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

    /// Coalesce a sequence of structured Output events against the trailing events of the active
    /// document's pipeline.
    pub(crate) fn push_output_batch_coalesced(&mut self, batch: Vec<OutputEvent>) {
        self.sync_output_document_id();
        self.output.push_coalesced_batch(batch);
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
        self.push_output_coalesced(
            OutputEvent::new(severity, source, text).with_optional_location(location),
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
                ScriptLocation::Frame { symbol, frame } => {
                    let sym_name = self.project.symbol(&symbol).map(|s| s.name.clone());
                    let layer_idx = self.project.symbol(&symbol).and_then(|sym| {
                        sym.timeline
                            .layers
                            .iter()
                            .position(|l| l.name.eq_ignore_ascii_case("actions"))
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
            ScriptLocation::Frame { symbol, frame } => {
                let sym_name = self.project.symbol(symbol).map(|s| s.name.clone());
                let layer_idx = self
                    .project
                    .symbol(symbol)
                    .and_then(|sym| sym.timeline.action_layer_for_frame(*frame));
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
