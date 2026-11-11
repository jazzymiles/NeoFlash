//! Ephemeral transport state. The serialized document stays authored and pure.
use crate::app::Editor;
use neoflash_core::{
    AuthoredDraw, AuthoredPlayback, AuthoredScene, DrawItem, ElementKind, Id, Project,
    ResolvedVideoPlacement, SoundCommand, VideoCommand, VideoSyncMode, clamp_or_loop_video_time_ms,
    evaluate_scene, evaluate_scene_with_buttons, path_commands_fingerprint,
    resolve_draw_item_video_placement, timeline_video::VideoSourceSpec,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const VIDEO_HISTORY: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActivePreviewVideo {
    pub(crate) channel: String,
    pub(crate) target_path: Vec<Id>,
    pub(crate) layer_id: Id,
    pub(crate) resource: Id,
    pub(crate) source: Option<VideoSourceSpec>,
    pub(crate) sync: VideoSyncMode,
    pub(crate) time_ms: u32,
    pub(crate) in_point_ms: u32,
    pub(crate) out_point_ms: Option<u32>,
    pub(crate) media_duration_ms: Option<u32>,
    pub(crate) media_unavailable: bool,
    pub(crate) playing: bool,
    pub(crate) muted: bool,
    pub(crate) loop_video: bool,
    pub(crate) stopped: bool,
    pub(crate) last_cue_point: Option<String>,
}

impl ActivePreviewVideo {
    pub(crate) fn to_resolved_placement(&self) -> ResolvedVideoPlacement {
        ResolvedVideoPlacement {
            channel: self.channel.clone(),
            target_path: self.target_path.clone(),
            layer_id: self.layer_id.clone(),
            resource: self.resource.clone(),
            source: self.source.clone(),
            sync: self.sync,
            time_ms: self.time_ms,
            in_point_ms: self.in_point_ms,
            out_point_ms: self.out_point_ms,
            playing: self.playing,
            loop_video: self.loop_video,
            muted: self.muted,
            stopped: self.stopped,
        }
    }
}

/// Editor video preview synchronization state (`T-167`, `T-201`), mirroring `TimelineAudio`.
#[derive(Default)]
pub(crate) struct TimelineVideoPreview {
    preview: Option<u64>,
    pub(crate) channels: HashMap<String, ActivePreviewVideo>,
    pub(crate) retained_by_target: HashMap<Vec<Id>, ActivePreviewVideo>,
    pub(crate) stopped_resources: HashSet<Id>,
    pub(crate) history: Vec<VideoCommand>,
    event_wall_clock: Option<Instant>,
}

impl TimelineVideoPreview {
    pub(crate) fn follow_preview(&mut self, epoch: Option<u64>) {
        if self.preview != epoch {
            self.channels.clear();
            self.retained_by_target.clear();
            self.stopped_resources.clear();
            self.event_wall_clock = None;
            self.preview = epoch;
        }
    }

    pub(crate) fn pause_active(&mut self) {
        for v in self.channels.values_mut() {
            if v.sync == VideoSyncMode::Stream {
                v.playing = false;
                self.retained_by_target
                    .insert(v.target_path.clone(), v.clone());
            }
        }
        for retained in self.retained_by_target.values_mut() {
            if retained.sync == VideoSyncMode::Stream {
                retained.playing = false;
            }
        }
    }

    pub(crate) fn reset_event_wall_clock(&mut self) {
        self.event_wall_clock = Some(Instant::now());
    }

    pub(crate) fn advance_event_wall_clock(&mut self) {
        let now = Instant::now();
        let dt_ms = self
            .event_wall_clock
            .replace(now)
            .map(|previous| {
                now.duration_since(previous)
                    .as_millis()
                    .min(u128::from(u32::MAX)) as u32
            })
            .unwrap_or(0);
        self.advance_event_streams(dt_ms, &[]);
    }

    pub(crate) fn has_playing_events(&self) -> bool {
        self.channels
            .values()
            .any(|video| video.sync == VideoSyncMode::Event && video.playing && !video.stopped)
    }

    fn event_media_probes(&self) -> Vec<ResolvedVideoPlacement> {
        self.channels
            .values()
            .filter(|video| video.sync == VideoSyncMode::Event && !video.stopped)
            .map(ActivePreviewVideo::to_resolved_placement)
            .collect()
    }

    fn set_event_media_duration(&mut self, channel: &str, duration_ms: Result<u32, ()>) {
        let Some(video) = self.channels.get_mut(channel) else {
            return;
        };
        if video.sync != VideoSyncMode::Event || video.stopped {
            return;
        }

        match duration_ms {
            Ok(duration_ms) if duration_ms > 0 => {
                let resume_after_unavailable = video.media_unavailable;
                video.media_duration_ms = Some(duration_ms);
                video.media_unavailable = false;
                let elapsed = u64::from(video.time_ms.saturating_sub(video.in_point_ms));
                video.time_ms = clamp_or_loop_video_time_ms(
                    video.in_point_ms,
                    elapsed,
                    video.out_point_ms,
                    video.media_duration_ms,
                    video.loop_video,
                );
                let effective_end = match video.out_point_ms {
                    Some(out_point_ms) => Some(out_point_ms.min(duration_ms)),
                    None => Some(duration_ms),
                };
                let empty_window = effective_end.is_some_and(|end_ms| end_ms <= video.in_point_ms);
                if empty_window
                    || (!video.loop_video
                        && effective_end.is_some_and(|end_ms| video.time_ms >= end_ms))
                {
                    video.playing = false;
                } else if resume_after_unavailable {
                    video.playing = true;
                }
            }
            Ok(_) | Err(()) => {
                // A decoder error is retained by VideoTextures and surfaced as an actionable
                // Stage warning. Keep the authored Event channel for possible external-file
                // recovery, but do not request repaint every 8 ms while no frame can advance.
                video.media_duration_ms = None;
                video.media_unavailable = true;
                video.playing = false;
            }
        }
        self.retained_by_target
            .insert(video.target_path.clone(), video.clone());
    }

    pub(crate) fn advance_event_streams(&mut self, dt_ms: u32, commands: &[VideoCommand]) {
        if dt_ms == 0 {
            return;
        }
        let has_stop_all = commands
            .iter()
            .any(|cmd| matches!(cmd, VideoCommand::StopAll));
        if has_stop_all {
            return;
        }
        let stopped_in_batch: HashSet<&str> = commands
            .iter()
            .filter_map(|cmd| match cmd {
                VideoCommand::Stop { resource } => Some(resource.as_str()),
                _ => None,
            })
            .collect();
        for v in self.channels.values_mut() {
            if v.sync == VideoSyncMode::Event
                && v.playing
                && !v.stopped
                && !stopped_in_batch.contains(v.resource.as_str())
            {
                let elapsed = u64::from(
                    v.time_ms
                        .saturating_sub(v.in_point_ms)
                        .saturating_add(dt_ms),
                );
                let next_time_ms = clamp_or_loop_video_time_ms(
                    v.in_point_ms,
                    elapsed,
                    v.out_point_ms,
                    v.media_duration_ms,
                    v.loop_video,
                );
                v.time_ms = next_time_ms;
                let effective_end = match (v.out_point_ms, v.media_duration_ms) {
                    (Some(out_point_ms), Some(duration_ms)) => Some(out_point_ms.min(duration_ms)),
                    (Some(out_point_ms), None) => Some(out_point_ms),
                    (None, Some(duration_ms)) => Some(duration_ms),
                    (None, None) => None,
                };
                let empty_window = effective_end.is_some_and(|end_ms| end_ms <= v.in_point_ms);
                if empty_window
                    || (!v.loop_video && effective_end.is_some_and(|end_ms| next_time_ms >= end_ms))
                {
                    v.playing = false;
                }
                self.retained_by_target
                    .insert(v.target_path.clone(), v.clone());
            }
        }
    }

    pub(crate) fn apply(&mut self, commands: Vec<VideoCommand>) -> Vec<String> {
        let mut logs = Vec::new();
        let stopped_in_batch: HashSet<Id> = commands
            .iter()
            .filter_map(|cmd| match cmd {
                VideoCommand::Stop { resource } => Some(resource.clone()),
                _ => None,
            })
            .collect();

        for command in commands {
            self.history.push(command.clone());
            if self.history.len() > VIDEO_HISTORY {
                self.history.remove(0);
            }
            match command {
                VideoCommand::SeekAndPresent {
                    channel,
                    target_path,
                    layer_id,
                    resource,
                    source,
                    target_time_ms,
                    in_point_ms,
                    out_point_ms,
                    playing,
                    muted,
                    loop_video,
                } => {
                    let existing_stopped = self
                        .channels
                        .get(&channel)
                        .is_some_and(|v| v.stopped && v.resource == resource)
                        || (self.stopped_resources.contains(&resource)
                            && self
                                .retained_by_target
                                .get(&target_path)
                                .is_some_and(|v| v.channel == channel && v.stopped));
                    if stopped_in_batch.contains(&resource) || existing_stopped {
                        self.stopped_resources.insert(resource.clone());
                        if let Some(existing) = self.channels.get_mut(&channel) {
                            existing.playing = false;
                            existing.stopped = true;
                            self.retained_by_target
                                .insert(existing.target_path.clone(), existing.clone());
                        }
                        continue;
                    }
                    self.stopped_resources.remove(&resource);
                    let prev_cue = self
                        .channels
                        .get(&channel)
                        .and_then(|v| v.last_cue_point.clone());
                    let entry = ActivePreviewVideo {
                        channel: channel.clone(),
                        target_path: target_path.clone(),
                        layer_id,
                        resource,
                        source,
                        sync: VideoSyncMode::Stream,
                        time_ms: target_time_ms,
                        in_point_ms,
                        out_point_ms,
                        media_duration_ms: None,
                        media_unavailable: false,
                        playing,
                        muted,
                        loop_video,
                        stopped: false,
                        last_cue_point: prev_cue,
                    };
                    self.channels.insert(channel, entry.clone());
                    self.retained_by_target.insert(target_path, entry);
                }
                VideoCommand::StartEvent {
                    channel,
                    target_path,
                    layer_id,
                    resource,
                    source,
                    start_offset_ms,
                    out_point_ms,
                    loop_video,
                    muted,
                } => {
                    if stopped_in_batch.contains(&resource) {
                        self.stopped_resources.insert(resource);
                        continue;
                    }
                    self.stopped_resources.remove(&resource);
                    let entry = ActivePreviewVideo {
                        channel: channel.clone(),
                        target_path: target_path.clone(),
                        layer_id,
                        resource,
                        source,
                        sync: VideoSyncMode::Event,
                        time_ms: start_offset_ms,
                        in_point_ms: start_offset_ms,
                        out_point_ms,
                        media_duration_ms: None,
                        media_unavailable: false,
                        playing: true,
                        muted,
                        loop_video,
                        stopped: false,
                        last_cue_point: None,
                    };
                    self.channels.insert(channel, entry.clone());
                    self.retained_by_target.insert(target_path, entry);
                }
                VideoCommand::Stop { resource } => {
                    self.stopped_resources.insert(resource.clone());
                    for v in self.channels.values_mut() {
                        if v.resource == resource {
                            v.playing = false;
                            v.stopped = true;
                            self.retained_by_target
                                .insert(v.target_path.clone(), v.clone());
                        }
                    }
                    for retained in self.retained_by_target.values_mut() {
                        if retained.resource == resource {
                            retained.playing = false;
                            retained.stopped = true;
                        }
                    }
                    self.channels.retain(|_, v| v.resource != resource);
                }
                VideoCommand::StopChannel { channel } => {
                    if let Some(mut removed) = self.channels.remove(&channel) {
                        removed.playing = false;
                        self.retained_by_target
                            .insert(removed.target_path.clone(), removed);
                    }
                }
                VideoCommand::PauseStreams => {
                    for v in self.channels.values_mut() {
                        if v.sync == VideoSyncMode::Stream {
                            v.playing = false;
                            self.retained_by_target
                                .insert(v.target_path.clone(), v.clone());
                        }
                    }
                    for retained in self.retained_by_target.values_mut() {
                        if retained.sync == VideoSyncMode::Stream {
                            retained.playing = false;
                        }
                    }
                }
                VideoCommand::StopAll => {
                    self.channels.clear();
                    self.retained_by_target.clear();
                    self.stopped_resources.clear();
                }
                VideoCommand::CuePointTriggered {
                    layer_id,
                    resource,
                    cue_point,
                    frame,
                } => {
                    for v in self.channels.values_mut() {
                        if v.layer_id == layer_id && v.resource == resource {
                            v.last_cue_point = Some(cue_point.name.clone());
                            self.retained_by_target
                                .insert(v.target_path.clone(), v.clone());
                        }
                    }
                    logs.push(format!(
                        "[video] Cue point '{}' ({} ms, frame {})",
                        cue_point.name,
                        cue_point.time_ms,
                        frame + 1
                    ));
                }
            }
        }
        logs
    }

    pub(crate) fn status_for_path(&self, path: &[Id]) -> Option<&ActivePreviewVideo> {
        self.channels
            .values()
            .find(|v| v.target_path == path)
            .or_else(|| self.retained_by_target.get(path))
            .or_else(|| self.channels.values().next())
            .or_else(|| self.retained_by_target.values().next())
    }

    pub(crate) fn placement_for_target(&self, path: &[Id]) -> Option<ResolvedVideoPlacement> {
        self.channels
            .values()
            .find(|v| v.target_path.as_slice() == path)
            .or_else(|| self.retained_by_target.get(path))
            .map(ActivePreviewVideo::to_resolved_placement)
    }
}

pub(crate) struct AuthoringPreview {
    project: Project,
    symbol: Id,
    pub(crate) playback: AuthoredPlayback,
    pub(crate) swf_player: Option<neoflash_import::SwfBytecodePlayer>,
    /// Distinguishes playback sessions; a new session silences the previous one's sounds.
    epoch: u64,
}

/// Attach non-serialized geometry hints only for editor Stage consumers. Core/runtime/export
/// evaluation deliberately leaves them absent so those paths do not pay an unused full-path scan.
pub(crate) fn prepare_stage_path_fingerprints(scene: &mut AuthoredScene) {
    for draw in &mut scene.draws {
        if draw.item.path_fingerprint.is_some() {
            continue;
        }
        draw.item.path_fingerprint = match &draw.item.kind {
            ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
                Some(path_commands_fingerprint(commands))
            }
            _ => None,
        };
    }
}

fn next_preview_epoch() -> u64 {
    static EPOCH: AtomicU64 = AtomicU64::new(1);
    EPOCH.fetch_add(1, Ordering::Relaxed)
}

impl Editor {
    fn valid_authored_preview(&self) -> Option<&AuthoringPreview> {
        self.authored_preview.as_ref().filter(|preview| {
            let frame_matches = preview
                .swf_player
                .as_ref()
                .map_or(preview.playback.frame() == self.frame, |swf| {
                    swf.presented_frame() == self.frame
                });
            preview.symbol == self.symbol_id() && frame_matches && preview.project == self.project
        })
    }

    /// True when the active document was imported from a `.swf` movie or retains an embedded
    /// `swf-original-source` (`application/x-shockwave-flash`) resource (`T-204`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn has_swf_bytecode_source(&self) -> bool {
        self.import_source.as_deref().is_some_and(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("swf"))
        }) || self.project.resources.iter().any(|r| {
            r.id == "swf-original-source"
                || r.media_type == "application/x-shockwave-flash"
                || r.path.ends_with(".swf")
        })
    }

    pub(crate) fn load_swf_bytecode_player(&self) -> Option<neoflash_import::SwfBytecodePlayer> {
        if let Some(res) = self.project.resources.iter().find(|r| {
            !r.data_base64.is_empty()
                && (r.id == "swf-original-source"
                    || r.media_type == "application/x-shockwave-flash"
                    || r.path.ends_with(".swf"))
        }) && let Ok(bytes) = res.decoded_bytes()
            && let Ok(player) = neoflash_import::SwfBytecodePlayer::from_bytes(&bytes)
        {
            return Some(player);
        }
        self.import_source
            .as_deref()
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("swf"))
            })
            .and_then(|p| neoflash_import::SwfBytecodePlayer::from_path(p).ok())
    }

    pub(crate) fn reset_authored_preview(&mut self) {
        self.authored_preview = None;
        self.accumulator = 0.;
        self.clear_live_preview_pointer_state();
    }

    pub(crate) fn sync_authored_preview(&mut self) {
        // Also catches direct inspector edits, undo and document replacement.
        // Resource payloads share their immutable storage with this snapshot.
        if self.authored_preview.is_some() && self.valid_authored_preview().is_none() {
            self.reset_authored_preview();
        }
        if self.playing {
            self.clear_simple_button_state();
        }
        if self.playing && self.authored_preview.is_none() {
            let symbol = self.symbol_id().to_owned();
            let mut playback = AuthoredPlayback::with_live_scripts(
                &self.project,
                &symbol,
                self.frame,
                self.live_script_preview,
            );
            let mut swf_player = if self.live_script_preview && self.editing_scene() {
                self.load_swf_bytecode_player()
            } else {
                None
            };
            self.frame = playback.frame();
            let mut traces = playback.drain_traces();
            let preview_diagnostics = playback.drain_diagnostics();
            let mut unsupported_diagnostics = Vec::new();
            let mut halted = playback.is_root_halted_by_script();
            if let Some(swf) = &mut swf_player {
                let state = swf.seek_to_frame(self.frame);
                self.frame = state
                    .frame
                    .min(self.symbol().timeline.frame_count.saturating_sub(1));
                for t in state.traces {
                    if !traces.contains(&t) {
                        traces.push(t);
                    }
                }
                for diag in state.unsupported_diagnostics {
                    if !unsupported_diagnostics.contains(&diag) {
                        unsupported_diagnostics.push(diag);
                    }
                }
                if !state.playing {
                    halted = true;
                }
            }
            self.authored_preview = Some(AuthoringPreview {
                playback,
                swf_player,
                project: self.project.clone(),
                symbol,
                epoch: next_preview_epoch(),
            });
            for line in traces {
                self.output.push(format!("[trace] {line}"));
            }
            for diag in preview_diagnostics {
                let formatted = format!("[preview] {diag}");
                if !self.output.contains(&formatted) {
                    self.output.push(formatted);
                }
            }
            for diag in unsupported_diagnostics {
                let formatted = format!("[swf] {diag}");
                if !self.output.contains(&formatted) {
                    self.output.push(formatted);
                }
            }
            if halted {
                self.playing = false;
            }
        }
    }

    pub(crate) fn tick_authored_preview(&mut self) {
        if !self.playing {
            return;
        }
        self.sync_authored_preview();
        if !self.playing {
            return;
        }
        let mut traces = Vec::new();
        let mut preview_diagnostics = Vec::new();
        let mut unsupported_diagnostics = Vec::new();
        let mut halted = false;
        let mut tick_video_commands = Vec::new();
        if let Some(preview) = &mut self.authored_preview {
            preview
                .playback
                .set_live_scripts(&self.project, self.live_script_preview);
            preview.playback.tick(&self.project);
            self.frame = preview.playback.frame();
            traces = preview.playback.drain_traces();
            preview_diagnostics = preview.playback.drain_diagnostics();
            tick_video_commands = preview.playback.drain_video_commands();
            halted = preview.playback.is_root_halted_by_script();
            if self.live_script_preview
                && let Some(swf) = &mut preview.swf_player
            {
                let state = swf.step_frame();
                self.frame = state
                    .frame
                    .min(self.symbol().timeline.frame_count.saturating_sub(1));
                for t in state.traces {
                    if !traces.contains(&t) {
                        traces.push(t);
                    }
                }
                for diag in state.unsupported_diagnostics {
                    if !unsupported_diagnostics.contains(&diag) {
                        unsupported_diagnostics.push(diag);
                    }
                }
                if !state.playing {
                    halted = true;
                }
            }
        }
        let epoch = self.valid_authored_preview().map(|p| p.epoch);
        self.timeline_video.follow_preview(epoch);
        let dt_ms = ((1000.0 / f64::from(self.project.fps.max(1))).round() as u32).max(1);
        self.timeline_video
            .advance_event_streams(dt_ms, &tick_video_commands);
        self.timeline_video.reset_event_wall_clock();
        for log_line in self.timeline_video.apply(tick_video_commands) {
            self.output.push(log_line);
        }
        for line in traces {
            self.output.push(format!("[trace] {line}"));
        }
        for diag in preview_diagnostics {
            let formatted = format!("[preview] {diag}");
            if !self.output.contains(&formatted) {
                self.output.push(formatted);
            }
        }
        for diag in unsupported_diagnostics {
            let formatted = format!("[swf] {diag}");
            if !self.output.contains(&formatted) {
                self.output.push(formatted);
            }
        }
        if halted {
            self.playing = false;
            self.timeline_video.pause_active();
        }
    }

    /// Deterministically seek the active or embedded `SwfBytecodePlayer` from frame 0 to
    /// `target_frame` and project its display-list state onto Stage without mutating `project`.
    pub fn seek_swf_preview(
        &mut self,
        target_frame: u32,
    ) -> Option<neoflash_import::SwfFrameState> {
        let max_frame = self.symbol().timeline.frame_count.saturating_sub(1);
        let clamped_target = target_frame.min(max_frame);
        let symbol = self.symbol_id().to_owned();
        let mut swf = self
            .authored_preview
            .as_mut()
            .and_then(|p| p.swf_player.take())
            .or_else(|| self.load_swf_bytecode_player())?;
        let state = swf.seek_to_frame(clamped_target);
        self.frame = state.frame.min(max_frame);
        self.playing = false;
        let playback = AuthoredPlayback::with_live_scripts(
            &self.project,
            &symbol,
            self.frame,
            self.live_script_preview,
        );
        for line in &state.traces {
            let formatted = format!("[trace] {line}");
            if !self.output.contains(&formatted) {
                self.output.push(formatted);
            }
        }
        for diag in &state.unsupported_diagnostics {
            let formatted = format!("[swf] {diag}");
            if !self.output.contains(&formatted) {
                self.output.push(formatted);
            }
        }
        self.authored_preview = Some(AuthoringPreview {
            playback,
            swf_player: Some(swf),
            project: self.project.clone(),
            symbol,
            epoch: next_preview_epoch(),
        });
        Some(state)
    }

    /// Deterministically restart the SWF runtime preview at frame 0 without mutating `project`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn restart_swf_preview(&mut self) -> Option<neoflash_import::SwfFrameState> {
        self.seek_swf_preview(0)
    }

    /// Deterministically step the SWF runtime preview forward or backward by `delta` frames.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn step_swf_preview(&mut self, delta: i32) -> Option<neoflash_import::SwfFrameState> {
        let max_frame = self.symbol().timeline.frame_count.saturating_sub(1) as i64;
        let base = i64::from(self.frame);
        let next = (base + i64::from(delta)).clamp(0, max_frame) as u32;
        self.seek_swf_preview(next)
    }

    /// Execute a `.swf` movie's AVM1/AVM2 bytecode and timeline for `max_frames`, logging
    /// `trace()` and unsupported diagnostics, and projecting the resulting display-list state
    /// as an ephemeral Stage preview overlay without mutating `self.project`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn play_swf_bytes_in_editor(
        &mut self,
        bytes: &[u8],
        max_frames: usize,
    ) -> Result<neoflash_import::SwfPlaybackResult, String> {
        let mut swf_player =
            neoflash_import::SwfBytecodePlayer::from_bytes(bytes).map_err(|e| e.to_string())?;
        let result = swf_player.run_frames(max_frames.max(1));
        for line in &result.trace_output {
            self.output.push(format!("[trace] {line}"));
        }
        for diag in &result.unsupported_diagnostics {
            let formatted = format!("[swf] {diag}");
            if !self.output.contains(&formatted) {
                self.output.push(formatted);
            }
        }
        if let Some(last) = result.frames_executed.last() {
            self.frame = last
                .frame
                .min(self.symbol().timeline.frame_count.saturating_sub(1));
            self.playing = last.playing;
        }
        let symbol = self.symbol_id().to_owned();
        let playback = AuthoredPlayback::with_live_scripts(
            &self.project,
            &symbol,
            self.frame,
            self.live_script_preview,
        );
        self.authored_preview = Some(AuthoringPreview {
            playback,
            swf_player: Some(swf_player),
            project: self.project.clone(),
            symbol,
            epoch: next_preview_epoch(),
        });
        Ok(result)
    }

    /// Dispatch a click event on a placed element path during live authoring preview.
    pub(crate) fn click_authored_preview(&mut self, path: &[Id]) {
        let mut traces = Vec::new();
        let mut preview_diagnostics = Vec::new();
        let mut click_video_commands = Vec::new();
        let mut was_halted = false;
        let mut halted = false;
        let mut clicked = false;
        if let Some(preview) = &mut self.authored_preview {
            was_halted = preview.playback.is_root_halted_by_script();
            preview
                .playback
                .set_live_scripts(&self.project, self.live_script_preview);
            preview.playback.click(&self.project, path);
            self.frame = preview.playback.frame();
            traces = preview.playback.drain_traces();
            preview_diagnostics = preview.playback.drain_diagnostics();
            click_video_commands = preview.playback.drain_video_commands();
            halted = preview.playback.is_root_halted_by_script();
            clicked = true;
        }
        if clicked && !click_video_commands.is_empty() {
            let epoch = self.valid_authored_preview().map(|p| p.epoch);
            self.timeline_video.follow_preview(epoch);
            for log_line in self.timeline_video.apply(click_video_commands) {
                self.output.push(log_line);
            }
        }
        for line in traces {
            self.output.push(format!("[trace] {line}"));
        }
        for diag in preview_diagnostics {
            let formatted = format!("[preview] {diag}");
            if !self.output.contains(&formatted) {
                self.output.push(formatted);
            }
        }
        if halted {
            self.playing = false;
            self.timeline_video.pause_active();
        } else if clicked && was_halted {
            self.playing = true;
            self.accumulator = 0.;
            self.last_tick = Instant::now();
        }
    }

    /// The playing preview session, if any.
    pub(crate) fn preview_epoch(&self) -> Option<u64> {
        self.authored_preview
            .as_ref()
            .filter(|_| self.playing)
            .map(|preview| preview.epoch)
    }

    /// Timeline sound commands of the playing preview since the last call.
    pub(crate) fn drain_preview_sounds(&mut self) -> Vec<SoundCommand> {
        match &mut self.authored_preview {
            Some(preview) if self.playing => {
                let mut cmds = preview.playback.drain_sound_commands();
                if let Some(swf) = &mut preview.swf_player {
                    for sc in swf.drain_sound_commands(&self.project) {
                        if !cmds.contains(&sc) {
                            cmds.push(sc);
                        }
                    }
                }
                cmds
            }
            _ => vec![],
        }
    }

    /// Timeline video commands of the playing preview since the last call.
    pub(crate) fn drain_preview_videos(&mut self) -> Vec<VideoCommand> {
        match &mut self.authored_preview {
            Some(preview) if self.playing => preview.playback.drain_video_commands(),
            _ => vec![],
        }
    }

    /// Feed editor playback's video commands to the native video preview state.
    ///
    /// Retains active/paused channels across transport pause while `valid_authored_preview()`
    /// remains valid, and clears them when the preview session is reset by scrubbing or editing (`T-201`).
    pub(crate) fn pump_timeline_video(&mut self) {
        self.sync_authored_preview();
        let retained_epoch = self.valid_authored_preview().map(|preview| preview.epoch);
        let commands = self.drain_preview_videos();
        self.timeline_video.follow_preview(retained_epoch);
        for log_line in self.timeline_video.apply(commands) {
            self.output.push(log_line);
        }
        self.refresh_event_video_durations();
        if !self.playing {
            self.timeline_video.pause_active();
            self.timeline_video.advance_event_wall_clock();
        } else {
            // Event streams advance deterministically with authored timeline ticks
            // while transport runs. Reset the wall-clock baseline so pausing never
            // double-counts that interval.
            self.timeline_video.reset_event_wall_clock();
        }
    }

    fn refresh_event_video_durations(&mut self) {
        let probes = self.timeline_video.event_media_probes();
        if probes.is_empty() {
            return;
        }
        let base_dir = self
            .tracked_disk_path()
            .and_then(std::path::Path::parent)
            .map(std::path::Path::to_path_buf);
        self.video_textures.begin_frame(
            &self.project,
            base_dir.as_deref(),
            self.history.revision(),
        );
        for placement in probes {
            let duration = self
                .video_textures
                .duration_for_placement(&self.project, base_dir.as_deref(), &placement)
                .map_err(|_| ());
            self.timeline_video
                .set_event_media_duration(&placement.channel, duration);
        }
    }

    /// Resolve the active, retained-paused, or static-scrubbed `ResolvedVideoPlacement` for `item` on Stage (`T-201`).
    pub(crate) fn resolve_stage_video_placement(
        &self,
        item: &DrawItem,
    ) -> Option<ResolvedVideoPlacement> {
        if !matches!(item.kind, ElementKind::Video { .. }) {
            return None;
        }
        if self.valid_authored_preview().is_some() {
            if let Some(placement) = self
                .timeline_video
                .placement_for_target(&item.instance_path)
            {
                return Some(placement);
            }
            if self
                .timeline_video
                .stopped_resources
                .iter()
                .any(|_| !self.timeline_video.history.is_empty())
            {
                // If Stop fired before any frame was presented for this video resource in the
                // active preview session, check whether the static placement is stopped.
                let clip_frames = self.presented_clip_frames();
                let static_placement = resolve_draw_item_video_placement(
                    &self.project,
                    self.symbol_id(),
                    self.frame,
                    &clip_frames,
                    item,
                )?;
                if self
                    .timeline_video
                    .stopped_resources
                    .contains(&static_placement.resource)
                {
                    return None;
                }
                return Some(static_placement);
            }
        }
        let clip_frames = self.presented_clip_frames();
        resolve_draw_item_video_placement(
            &self.project,
            self.symbol_id(),
            self.frame,
            &clip_frames,
            item,
        )
    }

    pub(crate) fn current_authored_scene(&mut self, show_masking: bool) -> AuthoredScene {
        self.sync_authored_preview();
        let mut scene =
            self.authored_scene_from_preview(self.authored_preview.as_ref(), show_masking);
        prepare_stage_path_fingerprints(&mut scene);
        scene
    }

    /// Picking uses the same clocks and masks as painting, including a paused
    /// preview. A direct edit must not expose a stale presentation to callers.
    pub(crate) fn presented_authored_scene(&self, show_masking: bool) -> AuthoredScene {
        let mut scene =
            self.authored_scene_from_preview(self.valid_authored_preview(), show_masking);
        prepare_stage_path_fingerprints(&mut scene);
        scene
    }

    /// Frame currently presented for one top-level instance in the active
    /// authoring preview. A paused independent MovieClip can differ from the
    /// static frame derived from its parent's playhead.
    pub(crate) fn presented_instance_frame(&self, element: &Id) -> Option<u32> {
        self.valid_authored_preview()?
            .playback
            .clip_frame(std::slice::from_ref(element))
    }

    /// Frames of the whole paused preview tree, relative to the active symbol.
    /// An empty map means static authored evaluation is sufficient.
    pub(crate) fn presented_clip_frames(&self) -> HashMap<Vec<Id>, u32> {
        self.valid_authored_preview()
            .map(|preview| preview.playback.clip_frames())
            .unwrap_or_default()
    }

    /// When `Edit multiple frames` is active, merge placements that exist only on other keyframes
    /// inside `onion_range()` (nearest keyframe first) so they can be hit-tested, marquee-selected,
    /// and bounded without duplicating objects already present on `self.frame`.
    pub(crate) fn interactive_authored_scene(
        &self,
        mut base: AuthoredScene,
        show_masking: bool,
    ) -> AuthoredScene {
        prepare_stage_path_fingerprints(&mut base);
        if !self.timeline_edit.edit_multiple {
            return base;
        }
        let mut merged = base;
        let mut seen_top: HashSet<Id> = merged
            .draws
            .iter()
            .filter_map(|d| d.item.instance_path.first().cloned())
            .collect();
        let mut frames = self.edit_multiple_key_frames();
        frames.retain(|&f| f != self.frame);
        frames.sort_by_key(|&f| (f.abs_diff(self.frame), f));
        let symbol_id = self.symbol_id();
        let eval_project = self.project_for_stage_eval();
        for f in frames {
            let ghost = evaluate_scene(&eval_project, symbol_id, f, true, show_masking);
            let mut added_this_frame = HashSet::new();
            let prefix = |id: &str| format!("@emf-{f}:{id}");
            for mut draw in ghost.draws {
                let Some(top) = draw.item.instance_path.first() else {
                    continue;
                };
                if seen_top.contains(top) {
                    continue;
                }
                added_this_frame.insert(top.clone());
                draw.clip_ids = draw.clip_ids.iter().map(|id| prefix(id)).collect();
                draw.mask_group = draw.mask_group.as_deref().map(prefix);
                merged.draws.push(AuthoredDraw { ..draw });
            }
            if !added_this_frame.is_empty() {
                // Keep the transitive mask/group graph: an outer source may depend on a mask
                // that does not appear directly in any visible draw's clip list.
                for mut clip in ghost.clips {
                    clip.id = prefix(&clip.id);
                    for component in &mut clip.components {
                        component.clip_ids =
                            component.clip_ids.iter().map(|id| prefix(id)).collect();
                    }
                    for component in clip.alpha_components.iter_mut().flatten() {
                        component.clip_ids =
                            component.clip_ids.iter().map(|id| prefix(id)).collect();
                    }
                    merged.clips.push(clip);
                }
                for mut group in ghost.mask_groups {
                    group.id = prefix(&group.id);
                    group.parent_id = group.parent_id.as_deref().map(prefix);
                    group.clip_ids = group.clip_ids.iter().map(|id| prefix(id)).collect();
                    merged.mask_groups.push(group);
                }
            }
            seen_top.extend(added_this_frame);
        }
        prepare_stage_path_fingerprints(&mut merged);
        merged
    }

    pub(crate) fn presented_interactive_authored_scene(&self, show_masking: bool) -> AuthoredScene {
        let base = self.presented_authored_scene(show_masking);
        self.interactive_authored_scene(base, show_masking)
    }

    /// Active `SymbolKind::Button` frame (`0` = Up, `1` = Over, `2` = Down) for top-level button
    /// instances on the authoring stage when `Control ▸ Enable simple buttons` (`⌥⌘B`) is active.
    pub(crate) fn simple_button_frame(&self, path: &[Id]) -> Option<u32> {
        if !self.enable_simple_buttons || self.playing || path.len() != 1 {
            return None;
        }
        let id = &path[0];
        if let Some(pressed) = &self.pressed_simple_button {
            if pressed == id {
                return Some(if self.hovered_simple_button.as_ref() == Some(id) {
                    2
                } else {
                    1
                });
            }
            return Some(0);
        }
        if self.hovered_simple_button.as_ref() == Some(id) {
            Some(1)
        } else {
            Some(0)
        }
    }

    fn authored_scene_from_preview(
        &self,
        preview: Option<&AuthoringPreview>,
        show_masking: bool,
    ) -> AuthoredScene {
        if !self.playing && self.frame >= self.symbol().timeline.frame_count {
            return AuthoredScene::default();
        }
        let eval_project = self.project_for_stage_eval();
        let simple_buttons_active = self.enable_simple_buttons
            && !self.playing
            && (preview.is_none()
                || self.hovered_simple_button.is_some()
                || self.pressed_simple_button.is_some());
        if simple_buttons_active {
            return evaluate_scene_with_buttons(
                &eval_project,
                self.symbol_id(),
                self.frame,
                true,
                show_masking,
                &|path| self.simple_button_frame(path),
            );
        }
        match preview {
            Some(preview) => {
                if self.live_script_preview
                    && self.editing_scene()
                    && let Some(swf) = &preview.swf_player
                {
                    return swf.evaluate_stage_scene(
                        &eval_project,
                        self.symbol_id(),
                        self.frame,
                        show_masking,
                    );
                }
                preview.playback.scene(&eval_project, true, show_masking)
            }
            None => evaluate_scene(
                &eval_project,
                self.symbol_id(),
                self.frame,
                true,
                show_masking,
            ),
        }
    }
}

#[cfg(test)]
mod timeline_video_preview_tests {
    use super::*;
    use neoflash_core::ResourceUnit;
    use neoflash_export::encode_deterministic_mp4_rgba_frames;
    use std::time::Duration;

    fn start_event(out_point_ms: Option<u32>, loop_video: bool) -> VideoCommand {
        VideoCommand::StartEvent {
            channel: "event-channel".into(),
            target_path: vec!["video-instance".into()],
            layer_id: "video-layer".into(),
            resource: "video-resource".into(),
            source: None,
            start_offset_ms: 100,
            out_point_ms,
            loop_video,
            muted: false,
        }
    }

    #[test]
    fn event_wall_clock_keeps_full_elapsed_time_and_finite_stream_stops_at_out_point() {
        let mut preview = TimelineVideoPreview::default();
        preview.apply(vec![start_event(Some(1_000), false)]);
        preview.event_wall_clock = Some(Instant::now() - Duration::from_millis(900));

        preview.advance_event_wall_clock();

        let video = preview.channels.get("event-channel").unwrap();
        assert_eq!(video.time_ms, 1_000);
        assert!(!video.playing);
        assert!(!video.stopped, "natural completion is not an authored Stop");
        assert!(!preview.has_playing_events());
        assert_eq!(
            preview
                .retained_by_target
                .get(&vec!["video-instance".into()])
                .unwrap()
                .time_ms,
            1_000
        );
    }

    #[test]
    fn looping_event_remains_active_after_large_elapsed_step() {
        let mut preview = TimelineVideoPreview::default();
        preview.apply(vec![start_event(Some(400), true)]);

        preview.advance_event_streams(650, &[]);

        let video = preview.channels.get("event-channel").unwrap();
        assert_eq!(video.time_ms, 150);
        assert!(video.playing);
        assert!(preview.has_playing_events());
    }

    #[test]
    fn event_without_authored_out_point_uses_media_duration_and_unavailable_media_idles() {
        let mut preview = TimelineVideoPreview::default();
        preview.apply(vec![start_event(None, false)]);
        preview.set_event_media_duration("event-channel", Ok(400));

        preview.advance_event_streams(300, &[]);
        let video = preview.channels.get("event-channel").unwrap();
        assert_eq!(video.time_ms, 400);
        assert!(!video.playing);
        assert!(!preview.has_playing_events());

        let mut unavailable = TimelineVideoPreview::default();
        unavailable.apply(vec![start_event(None, false)]);
        unavailable.set_event_media_duration("event-channel", Err(()));
        let failed = unavailable.channels.get("event-channel").unwrap();
        assert_eq!(
            failed.time_ms, 100,
            "decoder errors do not invent an end time"
        );
        assert!(failed.media_unavailable);
        assert!(!failed.playing);
        assert!(!unavailable.has_playing_events());

        // An external file can become available later. The retained Event channel resumes from
        // its authored time rather than being falsely treated as naturally completed.
        unavailable.set_event_media_duration("event-channel", Ok(400));
        let recovered = unavailable.channels.get("event-channel").unwrap();
        assert!(!recovered.media_unavailable);
        assert!(recovered.playing);
        assert_eq!(recovered.time_ms, 100);
    }

    #[test]
    fn event_without_out_point_loops_over_media_duration() {
        let mut preview = TimelineVideoPreview::default();
        preview.apply(vec![start_event(None, true)]);
        preview.set_event_media_duration("event-channel", Ok(400));

        preview.advance_event_streams(650, &[]);

        let video = preview.channels.get("event-channel").unwrap();
        assert_eq!(video.time_ms, 150);
        assert!(video.playing);
        assert!(preview.has_playing_events());

        let mut empty = TimelineVideoPreview::default();
        empty.apply(vec![start_event(None, true)]);
        empty.set_event_media_duration("event-channel", Ok(100));
        assert!(!empty.channels["event-channel"].playing);
        assert!(!empty.has_playing_events());
    }

    #[test]
    fn editor_propagates_native_decoder_duration_to_event_without_out_point() {
        let rgba = vec![255_u8; 4 * 4 * 4];
        let encoded = encode_deterministic_mp4_rgba_frames(4, 4, 10, &[&rgba, &rgba]);
        let mut project = Project::blank();
        project.resources.push(
            ResourceUnit::from_bytes("video-resource", "clip.mp4", "video/mp4", &encoded).unwrap(),
        );
        let ctx = eframe::egui::Context::default();
        let mut editor = Editor::from_project(&ctx, project);
        editor.timeline_video.apply(vec![start_event(None, false)]);

        editor.refresh_event_video_durations();

        let video = &editor.timeline_video.channels["event-channel"];
        assert_eq!(video.media_duration_ms, Some(200));
        assert!(video.playing);
        editor.timeline_video.advance_event_streams(100, &[]);
        assert_eq!(editor.timeline_video.channels["event-channel"].time_ms, 200);
        assert!(!editor.timeline_video.has_playing_events());
    }
}
