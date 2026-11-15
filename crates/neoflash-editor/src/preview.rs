//! Ephemeral transport state. The serialized document stays authored and pure.
use crate::app::Editor;
use neoflash_core::{
    AuthoredDraw, AuthoredPlayback, AuthoredScene, DrawItem, ElementKind, Id, Project,
    ResolvedVideoPlacement, SoundCommand, VideoCommand, VideoSyncMode, clamp_or_loop_video_time_ms,
    evaluate_scene, evaluate_scene_with_buttons, evaluate_scene_with_symbol_overrides,
    path_commands_fingerprint, resolve_draw_item_video_placement, timeline_video::VideoSourceSpec,
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
            let playback_symbol = playback.symbol().to_owned();
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
            if playback_symbol != symbol
                && self.editing_scene()
                && self.project.is_scene(&playback_symbol)
            {
                let next_frame = self.frame;
                let was_playing = self.playing;
                self.switch_scene(&playback_symbol);
                self.frame = next_frame;
                self.playing = was_playing;
            }
            self.authored_preview = Some(AuthoringPreview {
                playback,
                swf_player,
                project: self.project.clone(),
                symbol: playback_symbol,
                epoch: next_preview_epoch(),
            });
            let preview_loc = self.current_preview_output_location();
            for line in traces {
                self.push_output_event(
                    crate::output_pipeline::OutputEvent::trace(format!("[trace] {line}"))
                        .with_location(preview_loc.clone()),
                );
            }
            for diag in preview_diagnostics {
                let formatted = format!("[preview] {diag}");
                self.push_output_unique(
                    crate::output_pipeline::OutputEvent::warning(
                        crate::output_pipeline::OutputSource::Runtime,
                        formatted,
                    )
                    .with_location(preview_loc.clone()),
                );
            }
            for diag in unsupported_diagnostics {
                let formatted = format!("[swf] {diag}");
                self.push_output_unique(
                    crate::output_pipeline::OutputEvent::warning(
                        crate::output_pipeline::OutputSource::Runtime,
                        formatted,
                    )
                    .with_location(preview_loc.clone()),
                );
            }
            if halted {
                self.playing = false;
            }
        }
    }

    fn current_preview_output_location(&self) -> crate::output_pipeline::OutputLocation {
        let symbol = self.symbol_id().to_owned();
        let sym_name = self.project.symbol(&symbol).map(|s| s.name.clone());
        let layer_idx = self.project.symbol(&symbol).and_then(|sym| {
            sym.timeline
                .layers
                .iter()
                .position(|l| l.name.eq_ignore_ascii_case("actions"))
                .or_else(|| (self.layer < sym.timeline.layers.len()).then_some(self.layer))
        });
        let mut loc = crate::output_pipeline::OutputLocation::for_frame(
            symbol, sym_name, layer_idx, self.frame,
        );
        loc.document_id = Some(self.clipboard.session.clone());
        loc
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
        let mut scene_switched_to: Option<(Id, u32)> = None;
        if let Some(preview) = &mut self.authored_preview {
            preview
                .playback
                .set_live_scripts(&self.project, self.live_script_preview);
            preview.playback.tick(&self.project);
            self.frame = preview.playback.frame();
            let next_sym = preview.playback.symbol().to_owned();
            if next_sym != preview.symbol {
                preview.symbol = next_sym.clone();
                scene_switched_to = Some((next_sym, self.frame));
            }
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
        if let Some((next_scene, next_frame)) = scene_switched_to
            && self.editing_scene()
            && self.project.is_scene(&next_scene)
        {
            let saved_preview = self.authored_preview.take();
            let was_playing = self.playing;
            self.switch_scene(&next_scene);
            self.frame = next_frame;
            self.playing = was_playing;
            self.authored_preview = saved_preview;
        }
        let epoch = self.valid_authored_preview().map(|p| p.epoch);
        self.timeline_video.follow_preview(epoch);
        let dt_ms = ((1000.0 / f64::from(self.project.fps.max(1))).round() as u32).max(1);
        self.timeline_video
            .advance_event_streams(dt_ms, &tick_video_commands);
        self.timeline_video.reset_event_wall_clock();
        let preview_loc = self.current_preview_output_location();
        for log_line in self.timeline_video.apply(tick_video_commands) {
            self.push_output_coalesced(
                crate::output_pipeline::OutputEvent::info(
                    crate::output_pipeline::OutputSource::Runtime,
                    log_line,
                )
                .with_location(preview_loc.clone()),
            );
        }
        for line in traces {
            self.push_output_event(
                crate::output_pipeline::OutputEvent::trace(format!("[trace] {line}"))
                    .with_location(preview_loc.clone()),
            );
        }
        for diag in preview_diagnostics {
            let formatted = format!("[preview] {diag}");
            self.push_output_unique(
                crate::output_pipeline::OutputEvent::warning(
                    crate::output_pipeline::OutputSource::Runtime,
                    formatted,
                )
                .with_location(preview_loc.clone()),
            );
        }
        for diag in unsupported_diagnostics {
            let formatted = format!("[swf] {diag}");
            self.push_output_unique(
                crate::output_pipeline::OutputEvent::warning(
                    crate::output_pipeline::OutputSource::Runtime,
                    formatted,
                )
                .with_location(preview_loc.clone()),
            );
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
        let seek_loc = self.current_preview_output_location();
        for line in &state.traces {
            let formatted = format!("[trace] {line}");
            self.push_output_unique(
                crate::output_pipeline::OutputEvent::trace(formatted)
                    .with_location(seek_loc.clone()),
            );
        }
        for diag in &state.unsupported_diagnostics {
            let formatted = format!("[swf] {diag}");
            self.push_output_unique(
                crate::output_pipeline::OutputEvent::warning(
                    crate::output_pipeline::OutputSource::Runtime,
                    formatted,
                )
                .with_location(seek_loc.clone()),
            );
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
        if let Some(last) = result.frames_executed.last() {
            self.frame = last
                .frame
                .min(self.symbol().timeline.frame_count.saturating_sub(1));
            self.playing = last.playing;
        }
        let swf_loc = self.current_preview_output_location();
        for line in &result.trace_output {
            self.push_output_event(
                crate::output_pipeline::OutputEvent::trace(format!("[trace] {line}"))
                    .with_location(swf_loc.clone()),
            );
        }
        for diag in &result.unsupported_diagnostics {
            let formatted = format!("[swf] {diag}");
            self.push_output_unique(
                crate::output_pipeline::OutputEvent::warning(
                    crate::output_pipeline::OutputSource::Runtime,
                    formatted,
                )
                .with_location(swf_loc.clone()),
            );
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
        let mut scene_switched_to: Option<(Id, u32)> = None;
        if let Some(preview) = &mut self.authored_preview {
            was_halted = preview.playback.is_root_halted_by_script();
            preview
                .playback
                .set_live_scripts(&self.project, self.live_script_preview);
            preview.playback.click(&self.project, path);
            self.frame = preview.playback.frame();
            let next_sym = preview.playback.symbol().to_owned();
            if next_sym != preview.symbol {
                preview.symbol = next_sym.clone();
                scene_switched_to = Some((next_sym, self.frame));
            }
            traces = preview.playback.drain_traces();
            preview_diagnostics = preview.playback.drain_diagnostics();
            click_video_commands = preview.playback.drain_video_commands();
            halted = preview.playback.is_root_halted_by_script();
            clicked = true;
        }
        if let Some((next_scene, next_frame)) = scene_switched_to
            && self.editing_scene()
            && self.project.is_scene(&next_scene)
        {
            let saved_preview = self.authored_preview.take();
            let was_playing = self.playing;
            self.switch_scene(&next_scene);
            self.frame = next_frame;
            self.playing = was_playing;
            self.authored_preview = saved_preview;
        }
        let click_loc = self.current_preview_output_location();
        if clicked && !click_video_commands.is_empty() {
            let epoch = self.valid_authored_preview().map(|p| p.epoch);
            self.timeline_video.follow_preview(epoch);
            for log_line in self.timeline_video.apply(click_video_commands) {
                self.push_output_coalesced(
                    crate::output_pipeline::OutputEvent::info(
                        crate::output_pipeline::OutputSource::Runtime,
                        log_line,
                    )
                    .with_location(click_loc.clone()),
                );
            }
        }
        for line in traces {
            self.push_output_event(
                crate::output_pipeline::OutputEvent::trace(format!("[trace] {line}"))
                    .with_location(click_loc.clone()),
            );
        }
        for diag in preview_diagnostics {
            let formatted = format!("[preview] {diag}");
            self.push_output_unique(
                crate::output_pipeline::OutputEvent::warning(
                    crate::output_pipeline::OutputSource::Runtime,
                    formatted,
                )
                .with_location(click_loc.clone()),
            );
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
        let video_loc = self.current_preview_output_location();
        for log_line in self.timeline_video.apply(commands) {
            self.push_output_coalesced(
                crate::output_pipeline::OutputEvent::info(
                    crate::output_pipeline::OutputSource::Runtime,
                    log_line,
                )
                .with_location(video_loc.clone()),
            );
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
        let solo_overrides = self.solo_symbol_overrides();
        for f in frames {
            let ghost = if solo_overrides.is_empty() {
                evaluate_scene(&self.project, symbol_id, f, true, show_masking)
            } else {
                evaluate_scene_with_symbol_overrides(
                    &self.project,
                    &solo_overrides,
                    symbol_id,
                    f,
                    true,
                    show_masking,
                )
            };
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
        let simple_buttons_active = self.enable_simple_buttons
            && !self.playing
            && (preview.is_none()
                || self.hovered_simple_button.is_some()
                || self.pressed_simple_button.is_some());
        if simple_buttons_active {
            let eval_project = self.project_for_stage_eval();
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
                let eval_project = self.project_for_stage_eval();
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
            None => {
                let solo_overrides = self.solo_symbol_overrides();
                if solo_overrides.is_empty() {
                    evaluate_scene(
                        &self.project,
                        self.symbol_id(),
                        self.frame,
                        true,
                        show_masking,
                    )
                } else {
                    evaluate_scene_with_symbol_overrides(
                        &self.project,
                        &solo_overrides,
                        self.symbol_id(),
                        self.frame,
                        true,
                        show_masking,
                    )
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Local Loopback Test Movie HTTP Server (`127.0.0.1:0`, zero stale cache)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Default)]
struct PreviewMountTable {
    default_root: Option<std::path::PathBuf>,
    mounts: HashMap<String, std::path::PathBuf>,
    by_root: HashMap<std::path::PathBuf, String>,
    next_id: u64,
}

/// Reusable loopback HTTP/1.1 static asset server for `Test Movie` (`⌘Return`) and `Test Scene`
/// (`⌘⌥Return`). Serves fresh disk bytes on every request with strict `Cache-Control: no-store,
/// no-cache, must-revalidate` headers, RFC 3986 UTF-8 percent-decoding, full WASM/ES-module/media
/// MIME types, HTTP `Range` support for `<audio>`/`<video>` seeking, and directory-jail checks.
#[allow(dead_code)]
pub(crate) struct PreviewHttpServer {
    addr: std::net::SocketAddr,
    table: std::sync::Arc<std::sync::RwLock<PreviewMountTable>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    accept_thread: Option<std::thread::JoinHandle<()>>,
}

#[allow(dead_code)]
impl PreviewHttpServer {
    /// Start a new loopback HTTP server bound to `127.0.0.1:0`.
    pub(crate) fn start() -> std::io::Result<Self> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let table = std::sync::Arc::new(std::sync::RwLock::new(PreviewMountTable::default()));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_table = std::sync::Arc::clone(&table);
        let thread_stop = std::sync::Arc::clone(&stop);
        let accept_thread = std::thread::Builder::new()
            .name("neoflash-preview-http".into())
            .spawn(move || run_preview_accept_loop(listener, thread_table, thread_stop))?;
        Ok(Self {
            addr,
            table,
            stop,
            accept_thread: Some(accept_thread),
        })
    }

    /// Start a loopback HTTP server whose root `/` immediately serves `root_dir`.
    pub(crate) fn start_for_dir(root_dir: &std::path::Path) -> std::io::Result<Self> {
        let canonical = std::fs::canonicalize(root_dir)?;
        if !canonical.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("preview root '{}' is not a directory", root_dir.display()),
            ));
        }
        let server = Self::start()?;
        if let Ok(mut guard) = server.table.write() {
            guard.default_root = Some(canonical);
        }
        Ok(server)
    }

    pub(crate) fn local_addr(&self) -> std::net::SocketAddr {
        self.addr
    }

    pub(crate) fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }

    pub(crate) fn is_running(&self) -> bool {
        !self.stop.load(Ordering::Acquire)
            && self
                .accept_thread
                .as_ref()
                .is_some_and(|handle| !handle.is_finished())
    }

    /// Register or reuse a mount for `index_file`'s parent directory and return its loopback URL.
    pub(crate) fn mount_index_file(&self, index_file: &std::path::Path) -> Result<String, String> {
        let canonical_file = std::fs::canonicalize(index_file).map_err(|e| {
            format!(
                "Preview file '{}' is unavailable: {e}",
                index_file.display()
            )
        })?;
        if !canonical_file.is_file() {
            return Err(format!(
                "Preview path '{}' is not a file.",
                index_file.display()
            ));
        }
        let parent = canonical_file
            .parent()
            .ok_or_else(|| "Preview file has no parent directory.".to_owned())?
            .to_path_buf();
        let file_name = canonical_file
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| "Preview file name is not valid UTF-8.".to_owned())?;

        let slug = {
            let mut guard = self
                .table
                .write()
                .map_err(|_| "Preview server lock poisoned.".to_owned())?;
            guard.default_root = Some(parent.clone());
            if let Some(existing) = guard.by_root.get(&parent) {
                existing.clone()
            } else {
                guard.next_id += 1;
                let slug = format!("m{}", guard.next_id);
                guard.mounts.insert(slug.clone(), parent.clone());
                guard.by_root.insert(parent, slug.clone());
                slug
            }
        };
        Ok(format!(
            "http://127.0.0.1:{}/{slug}/{}",
            self.addr.port(),
            percent_encode_url_segment(file_name)
        ))
    }

    /// Gracefully stop the accept loop and wait for its background thread to exit.
    pub(crate) fn shutdown(&mut self) {
        if !self.stop.swap(true, Ordering::AcqRel) {
            let _ = std::net::TcpStream::connect_timeout(
                &self.addr,
                std::time::Duration::from_millis(100),
            );
        }
        if let Some(handle) = self.accept_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for PreviewHttpServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

static SHARED_PREVIEW_SERVER: std::sync::Mutex<Option<PreviewHttpServer>> =
    std::sync::Mutex::new(None);

/// Ensure the process-wide `PreviewHttpServer` is running, mount `index_file`, and return its
/// `http://127.0.0.1:<port>/...` URL (reusing the same server and mount across repeated `⌘Return` runs).
pub(crate) fn serve_preview_index(index_file: &std::path::Path) -> Result<String, String> {
    let mut guard = SHARED_PREVIEW_SERVER
        .lock()
        .map_err(|_| "Preview HTTP server lock poisoned.".to_owned())?;
    let needs_restart = guard.as_ref().is_none_or(|srv| !srv.is_running());
    if needs_restart {
        if let Some(mut old) = guard.take() {
            old.shutdown();
        }
        let srv = PreviewHttpServer::start()
            .map_err(|e| format!("Cannot start local preview HTTP server: {e}"))?;
        *guard = Some(srv);
    }
    guard
        .as_ref()
        .expect("preview server initialized")
        .mount_index_file(index_file)
}

/// Serve `index_file` on the reusable loopback HTTP server (with `Cache-Control: no-store`) and
/// launch it in the user's default browser.
pub(crate) fn open_preview_in_browser(index_file: &std::path::Path) -> Result<String, String> {
    let url = serve_preview_index(index_file)?;
    crate::platform_open::open_url(&url)?;
    Ok(url)
}

pub(crate) fn shutdown_shared_preview_server() {
    if let Ok(mut guard) = SHARED_PREVIEW_SERVER.lock()
        && let Some(mut srv) = guard.take()
    {
        srv.shutdown();
    }
}

fn run_preview_accept_loop(
    listener: std::net::TcpListener,
    table: std::sync::Arc<std::sync::RwLock<PreviewMountTable>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    while !stop.load(Ordering::Acquire) {
        let Ok((stream, _peer)) = listener.accept() else {
            if stop.load(Ordering::Acquire) {
                break;
            }
            continue;
        };
        if stop.load(Ordering::Acquire) {
            break;
        }
        let req_table = std::sync::Arc::clone(&table);
        let _ = std::thread::Builder::new()
            .name("neoflash-preview-req".into())
            .spawn(move || {
                let _ = handle_preview_http_connection(stream, &req_table);
            });
    }
}

#[allow(dead_code)]
fn handle_preview_http_connection(
    mut stream: std::net::TcpStream,
    table: &std::sync::RwLock<PreviewMountTable>,
) -> std::io::Result<()> {
    use std::io::Read as _;

    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(10)));

    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 2048];
    let header_end = loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(());
            }
            Err(err) => return Err(err),
        };
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        if buf.len() > 16 * 1024 {
            return write_preview_error(&mut stream, 400, "Bad Request", true, &[]);
        }
    };

    let Ok(header_text) = std::str::from_utf8(&buf[..header_end]) else {
        return write_preview_error(&mut stream, 400, "Bad Request", true, &[]);
    };
    let mut lines = header_text.split("\r\n");
    let Some(request_line) = lines.next() else {
        return write_preview_error(&mut stream, 400, "Bad Request", true, &[]);
    };
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(raw_target), Some(_version)) =
        (parts.next(), parts.next(), parts.next())
    else {
        return write_preview_error(&mut stream, 400, "Bad Request", true, &[]);
    };

    let send_body = match method {
        "GET" => true,
        "HEAD" => false,
        _ => {
            return write_preview_error(
                &mut stream,
                405,
                "Method Not Allowed",
                true,
                &[("Allow", "GET, HEAD")],
            );
        }
    };

    let mut range_header: Option<&str> = None;
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, val)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("range")
        {
            range_header = Some(val.trim());
        }
    }

    // Strip #fragment and ?query (e.g. cache-busting `?t=...`), then decode RFC 3986 path segments.
    let without_fragment = raw_target.split('#').next().unwrap_or(raw_target);
    let raw_path = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    if !raw_path.starts_with('/') {
        return write_preview_error(&mut stream, 400, "Bad Request", send_body, &[]);
    }

    let mut decoded_segments: Vec<String> = Vec::new();
    for raw_seg in raw_path.split('/').skip(1) {
        if raw_seg.is_empty() {
            continue;
        }
        let Ok(seg) = percent_decode_url_segment(raw_seg) else {
            return write_preview_error(&mut stream, 400, "Bad Request", send_body, &[]);
        };
        if seg == "." {
            continue;
        }
        if seg == ".." || seg.contains(['/', '\\', '\0']) {
            return write_preview_error(&mut stream, 403, "Forbidden", send_body, &[]);
        }
        decoded_segments.push(seg);
    }

    let (root_dir, rel_segments): (std::path::PathBuf, &[String]) = {
        let Ok(guard) = table.read() else {
            return write_preview_error(&mut stream, 500, "Internal Server Error", send_body, &[]);
        };
        if let Some(first) = decoded_segments.first()
            && let Some(mounted_root) = guard.mounts.get(first)
        {
            (mounted_root.clone(), &decoded_segments[1..])
        } else if let Some(default_root) = &guard.default_root {
            (default_root.clone(), &decoded_segments[..])
        } else {
            return write_preview_error(&mut stream, 404, "Not Found", send_body, &[]);
        }
    };

    let mut candidate = root_dir.clone();
    if rel_segments.is_empty() {
        candidate.push("index.html");
    } else {
        for seg in rel_segments {
            candidate.push(seg);
        }
    }
    if candidate.is_dir() {
        candidate.push("index.html");
    }
    if !candidate.exists() {
        return write_preview_error(&mut stream, 404, "Not Found", send_body, &[]);
    }
    let Ok(canonical_target) = std::fs::canonicalize(&candidate) else {
        return write_preview_error(&mut stream, 404, "Not Found", send_body, &[]);
    };
    if !canonical_target.starts_with(&root_dir) {
        return write_preview_error(&mut stream, 403, "Forbidden", send_body, &[]);
    }
    if !canonical_target.is_file() {
        return write_preview_error(&mut stream, 404, "Not Found", send_body, &[]);
    }

    let bytes = match std::fs::read(&canonical_target) {
        Ok(b) => b,
        Err(_) => return write_preview_error(&mut stream, 404, "Not Found", send_body, &[]),
    };
    let mime = preview_mime_type(&canonical_target);
    let total_len = bytes.len();

    if let Some(raw_range) = range_header
        && let Some(spec) = raw_range.strip_prefix("bytes=")
        && !spec.contains(',')
    {
        match parse_http_byte_range(spec, total_len) {
            Some((start, end_inclusive)) => {
                let slice = &bytes[start..=end_inclusive];
                let content_range = format!("bytes {start}-{end_inclusive}/{total_len}");
                return write_preview_response(
                    &mut stream,
                    206,
                    "Partial Content",
                    mime,
                    slice,
                    send_body,
                    &[("Content-Range", content_range.as_str())],
                );
            }
            None => {
                let unsatisfied = format!("bytes */{total_len}");
                return write_preview_error(
                    &mut stream,
                    416,
                    "Range Not Satisfiable",
                    send_body,
                    &[("Content-Range", unsatisfied.as_str())],
                );
            }
        }
    }

    write_preview_response(&mut stream, 200, "OK", mime, &bytes, send_body, &[])
}

#[allow(dead_code)]
fn parse_http_byte_range(spec: &str, total_len: usize) -> Option<(usize, usize)> {
    if total_len == 0 {
        return None;
    }
    let (start_str, end_str) = spec.trim().split_once('-')?;
    let start_str = start_str.trim();
    let end_str = end_str.trim();
    if start_str.is_empty() {
        let suffix_len: usize = end_str.parse().ok()?;
        if suffix_len == 0 {
            return None;
        }
        let start = total_len.saturating_sub(suffix_len);
        Some((start, total_len - 1))
    } else {
        let start: usize = start_str.parse().ok()?;
        if start >= total_len {
            return None;
        }
        let end = if end_str.is_empty() {
            total_len - 1
        } else {
            let parsed_end: usize = end_str.parse().ok()?;
            if parsed_end < start {
                return None;
            }
            parsed_end.min(total_len - 1)
        };
        Some((start, end))
    }
}

#[allow(dead_code)]
fn write_preview_error(
    stream: &mut std::net::TcpStream,
    status: u16,
    reason: &str,
    send_body: bool,
    extra_headers: &[(&str, &str)],
) -> std::io::Result<()> {
    let body = format!("{status} {reason}\n");
    write_preview_response(
        stream,
        status,
        reason,
        "text/plain; charset=utf-8",
        body.as_bytes(),
        send_body,
        extra_headers,
    )
}

#[allow(dead_code)]
fn write_preview_response(
    stream: &mut std::net::TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
    send_body: bool,
    extra_headers: &[(&str, &str)],
) -> std::io::Result<()> {
    use std::io::Write as _;

    let mut header = format!(
        "HTTP/1.1 {status} {reason}\r\n\
Content-Type: {content_type}\r\n\
Content-Length: {}\r\n\
Cache-Control: no-store, no-cache, must-revalidate, max-age=0\r\n\
Pragma: no-cache\r\n\
Expires: 0\r\n\
Accept-Ranges: bytes\r\n\
X-Content-Type-Options: nosniff\r\n\
Connection: close\r\n",
        body.len()
    );
    for (k, v) in extra_headers {
        header.push_str(k);
        header.push_str(": ");
        header.push_str(v);
        header.push_str("\r\n");
    }
    header.push_str("\r\n");
    stream.write_all(header.as_bytes())?;
    if send_body && !body.is_empty() {
        stream.write_all(body)?;
    }
    stream.flush()
}

#[allow(dead_code)]
pub(crate) fn preview_mime_type(path: &std::path::Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs" | "cjs") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json" | "map" | "webmanifest") => "application/json; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("avif") => "image/avif",
        Some("bmp") => "image/bmp",
        Some("ico") => "image/x-icon",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg" | "oga") => "audio/ogg",
        Some("opus") => "audio/opus",
        Some("aac") => "audio/aac",
        Some("m4a") => "audio/mp4",
        Some("flac") => "audio/flac",
        Some("mp4" | "m4v") => "video/mp4",
        Some("webm") => "video/webm",
        Some("ogv") => "video/ogg",
        Some("mov") => "video/quicktime",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("xml") => "application/xml; charset=utf-8",
        Some("txt") => "text/plain; charset=utf-8",
        Some("swf") => "application/x-shockwave-flash",
        _ => "application/octet-stream",
    }
}

#[allow(dead_code)]
fn percent_encode_url_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for &b in segment.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[allow(dead_code)]
fn percent_decode_url_segment(raw: &str) -> Result<String, ()> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(());
            }
            let hi = (bytes[i + 1] as char).to_digit(16).ok_or(())?;
            let lo = (bytes[i + 2] as char).to_digit(16).ok_or(())?;
            out.push(((hi << 4) | lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| ())
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

    #[test]
    fn solo_symbol_overrides_evaluates_stage_without_cloning_project() {
        let mut project = Project::demo();
        let root_id = project.root.clone();
        let first_layer_id = project.symbol(&root_id).unwrap().timeline.layers[0]
            .id
            .clone();
        let mut extra_layer = project.symbol(&root_id).unwrap().timeline.layers[0].clone();
        extra_layer.id = "layer-extra-bg".into();
        extra_layer.name = "Extra BG".into();
        for key in &mut extra_layer.keyframes {
            for el in &mut key.elements {
                el.id = format!("{}-bg", el.id);
            }
        }
        project
            .symbol_mut(&root_id)
            .unwrap()
            .timeline
            .layers
            .push(extra_layer);

        let ctx = eframe::egui::Context::default();
        let mut editor = Editor::from_project(&ctx, project);
        assert!(editor.solo_symbol_overrides().is_empty());
        assert!(matches!(
            editor.project_for_stage_eval(),
            std::borrow::Cow::Borrowed(_)
        ));
        let full_draws = editor.presented_authored_scene(true).draws.len();

        editor.toggle_layer_solo(0, false);
        let overrides = editor.solo_symbol_overrides();
        assert_eq!(overrides.len(), 1);
        let overridden_sym = overrides.get(&root_id).unwrap();
        assert!(
            overridden_sym
                .timeline
                .layers
                .iter()
                .find(|l| l.id == first_layer_id)
                .unwrap()
                .visible
        );
        assert!(
            !overridden_sym
                .timeline
                .layers
                .iter()
                .find(|l| l.id == "layer-extra-bg")
                .unwrap()
                .visible
        );
        let solo_draws = editor.presented_authored_scene(true).draws.len();
        assert!(solo_draws < full_draws);
    }

    #[test]
    fn t320_preview_http_server_no_store_unicode_mime_range_jail_and_reuse() {
        use std::io::{Read as _, Write as _};

        fn raw_http_request(addr: std::net::SocketAddr, request: &str) -> (String, Vec<u8>) {
            let mut stream = std::net::TcpStream::connect(addr).expect("connect preview http");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream.write_all(request.as_bytes()).unwrap();
            stream.flush().unwrap();
            let mut raw = Vec::new();
            stream.read_to_end(&mut raw).unwrap();
            let split = raw
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .expect("HTTP CRLF header boundary");
            let headers = String::from_utf8_lossy(&raw[..split]).into_owned();
            let body = raw[split + 4..].to_vec();
            (headers, body)
        }

        let root =
            std::env::temp_dir().join(format!("neoflash-preview-http-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let sub = root.join("assets");
        std::fs::create_dir_all(&sub).unwrap();
        let outside_secret = std::env::temp_dir().join(format!(
            "neoflash-preview-outside-{}.txt",
            std::process::id()
        ));
        std::fs::write(&outside_secret, b"TOP_SECRET_OUTSIDE_JAIL").unwrap();

        let index_path = root.join("index.html");
        std::fs::write(&index_path, b"<!doctype html><title>v1</title>").unwrap();
        std::fs::write(root.join("project.json"), br#"{"version":1}"#).unwrap();
        std::fs::write(root.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
        std::fs::write(root.join("host.mjs"), b"export const ok = 1;\n").unwrap();
        std::fs::write(sub.join("Сцена 1 — Тест.mp3"), b"0123456789").unwrap();
        std::fs::write(sub.join("c++ clip.mp4"), b"MP4DATA").unwrap();

        // 1. Shared preview server mounts index.html and reuses the exact same server & mount across repeated calls.
        shutdown_shared_preview_server();
        let url_1 = serve_preview_index(&index_path).expect("mount index v1");
        let url_2 = serve_preview_index(&index_path).expect("reuse mount index v2");
        assert_eq!(
            url_1, url_2,
            "repeated Test Movie runs must reuse the active loopback server and mount URL"
        );

        let after_scheme = url_1.strip_prefix("http://").expect("http url");
        let (host_port, mount_and_file) = after_scheme.split_once('/').unwrap();
        let addr: std::net::SocketAddr = host_port.parse().unwrap();
        let (mount_slug, _) = mount_and_file.split_once('/').unwrap();

        // 2. Verify strict no-cache headers + fresh disk reads after overwriting files in place.
        let (hdr_v1, body_v1) = raw_http_request(
            addr,
            &format!("GET /{mount_slug}/project.json?cb=1 HTTP/1.1\r\nHost: {host_port}\r\n\r\n"),
        );
        assert!(hdr_v1.starts_with("HTTP/1.1 200 OK"));
        assert!(
            hdr_v1.contains("Cache-Control: no-store, no-cache, must-revalidate, max-age=0"),
            "missing Cache-Control no-store header: {hdr_v1}"
        );
        assert!(hdr_v1.contains("Pragma: no-cache"));
        assert!(hdr_v1.contains("Expires: 0"));
        assert!(hdr_v1.contains("Content-Type: application/json; charset=utf-8"));
        assert_eq!(body_v1, br#"{"version":1}"#);

        // Mutate `project.json` and `index.html` on disk (simulating user editing stage/script + pressing Cmd+Return again).
        std::fs::write(
            root.join("project.json"),
            br#"{"version":2,"updated":true}"#,
        )
        .unwrap();
        let (_, body_v2) = raw_http_request(
            addr,
            &format!("GET /{mount_slug}/project.json HTTP/1.1\r\nHost: {host_port}\r\n\r\n"),
        );
        assert_eq!(
            body_v2, br#"{"version":2,"updated":true}"#,
            "preview server must serve updated disk bytes immediately without stale caching"
        );

        // 3. MIME types for .wasm, .mjs, Unicode/percent-encoded .mp3, and literal '+' in .mp4.
        let (wasm_hdr, wasm_body) = raw_http_request(
            addr,
            &format!(
                "GET /{mount_slug}/neoflash_web_bg.wasm HTTP/1.1\r\nHost: {host_port}\r\n\r\n"
            ),
        );
        assert!(wasm_hdr.contains("Content-Type: application/wasm"));
        assert_eq!(wasm_body, b"\0asm\x01\0\0\0");

        let (mjs_hdr, _) = raw_http_request(
            addr,
            &format!("GET /{mount_slug}/host.mjs HTTP/1.1\r\nHost: {host_port}\r\n\r\n"),
        );
        assert!(mjs_hdr.contains("Content-Type: text/javascript; charset=utf-8"));

        let encoded_unicode = percent_encode_url_segment("Сцена 1 — Тест.mp3");
        let (mp3_hdr, mp3_body) = raw_http_request(
            addr,
            &format!(
                "GET /{mount_slug}/assets/{encoded_unicode} HTTP/1.1\r\nHost: {host_port}\r\n\r\n"
            ),
        );
        assert!(mp3_hdr.starts_with("HTTP/1.1 200 OK"));
        assert!(mp3_hdr.contains("Content-Type: audio/mpeg"));
        assert_eq!(mp3_body, b"0123456789");

        // Literal '+' in URL path must remain '+' (RFC 3986 path segment, NOT form query space!).
        let (mp4_hdr, mp4_body) = raw_http_request(
            addr,
            &format!(
                "GET /{mount_slug}/assets/c++%20clip.mp4 HTTP/1.1\r\nHost: {host_port}\r\n\r\n"
            ),
        );
        assert!(mp4_hdr.starts_with("HTTP/1.1 200 OK"));
        assert!(mp4_hdr.contains("Content-Type: video/mp4"));
        assert_eq!(mp4_body, b"MP4DATA");

        // 4. HTTP Range (206 Partial Content & 416 Range Not Satisfiable), HEAD, and 405 Method Not Allowed.
        let (range_hdr, range_body) = raw_http_request(
            addr,
            &format!(
                "GET /{mount_slug}/assets/{encoded_unicode} HTTP/1.1\r\nHost: {host_port}\r\nRange: bytes=2-5\r\n\r\n"
            ),
        );
        assert!(range_hdr.starts_with("HTTP/1.1 206 Partial Content"));
        assert!(range_hdr.contains("Content-Range: bytes 2-5/10"));
        assert_eq!(range_body, b"2345");

        let (bad_range_hdr, _) = raw_http_request(
            addr,
            &format!(
                "GET /{mount_slug}/assets/{encoded_unicode} HTTP/1.1\r\nHost: {host_port}\r\nRange: bytes=50-60\r\n\r\n"
            ),
        );
        assert!(bad_range_hdr.starts_with("HTTP/1.1 416 Range Not Satisfiable"));

        let (head_hdr, head_body) = raw_http_request(
            addr,
            &format!("HEAD /{mount_slug}/host.mjs HTTP/1.1\r\nHost: {host_port}\r\n\r\n"),
        );
        assert!(head_hdr.starts_with("HTTP/1.1 200 OK"));
        assert!(head_hdr.contains("Content-Length: 21"));
        assert!(head_body.is_empty(), "HEAD response must have empty body");

        let (post_hdr, _) = raw_http_request(
            addr,
            &format!("POST /{mount_slug}/index.html HTTP/1.1\r\nHost: {host_port}\r\n\r\n"),
        );
        assert!(post_hdr.starts_with("HTTP/1.1 405 Method Not Allowed"));

        // 5. Directory traversal attempts (`..`, `%2e%2e`, `\`) are strictly blocked!
        let outside_name = outside_secret.file_name().unwrap().to_str().unwrap();
        for attack in [
            format!("/{mount_slug}/../{outside_name}"),
            format!("/{mount_slug}/%2e%2e/{outside_name}"),
            format!("/{mount_slug}/..\\{outside_name}"),
        ] {
            let (jail_hdr, jail_body) = raw_http_request(
                addr,
                &format!("GET {attack} HTTP/1.1\r\nHost: {host_port}\r\n\r\n"),
            );
            assert!(
                jail_hdr.starts_with("HTTP/1.1 403") || jail_hdr.starts_with("HTTP/1.1 400"),
                "expected 403/400 for traversal {attack}, got: {jail_hdr}"
            );
            assert!(!String::from_utf8_lossy(&jail_body).contains("TOP_SECRET_OUTSIDE_JAIL"));
        }

        // 6. Multi-mount `Test Movie` (`m1`) vs `Test Scene` (`m2`) `default_root` switching:
        //    root-relative requests (`/data.json`) must always resolve against the latest mounted preview
        //    while explicit `/m1/...` and `/m2/...` mounts continue to coexist!
        std::fs::write(root.join("data.json"), br#"{"source":"movie"}"#).unwrap();
        let (_, root_movie_body) = raw_http_request(
            addr,
            &format!("GET /data.json HTTP/1.1\r\nHost: {host_port}\r\n\r\n"),
        );
        assert_eq!(root_movie_body, br#"{"source":"movie"}"#);

        let scene_root = std::env::temp_dir().join(format!(
            "neoflash-preview-http-scene-test-{}-{}",
            std::process::id(),
            neoflash_core::new_id("srvscene")
        ));
        std::fs::create_dir_all(&scene_root).unwrap();
        let scene_index = scene_root.join("index.html");
        std::fs::write(&scene_index, b"<!doctype html><title>scene</title>").unwrap();
        std::fs::write(scene_root.join("data.json"), br#"{"source":"scene"}"#).unwrap();

        let scene_url = serve_preview_index(&scene_index).expect("mount scene preview");
        let scene_after_scheme = scene_url.strip_prefix("http://").unwrap();
        let (scene_host_port, scene_mount_and_file) = scene_after_scheme.split_once('/').unwrap();
        assert_eq!(
            scene_host_port, host_port,
            "Test Scene reuses same HTTP server"
        );
        let (scene_slug, _) = scene_mount_and_file.split_once('/').unwrap();
        assert_ne!(
            scene_slug, mount_slug,
            "Test Scene gets its own distinct mount slug"
        );

        let (_, root_scene_body) = raw_http_request(
            addr,
            &format!("GET /data.json HTTP/1.1\r\nHost: {host_port}\r\n\r\n"),
        );
        assert_eq!(
            root_scene_body, br#"{"source":"scene"}"#,
            "after Test Scene mount, root-relative /data.json must resolve to Test Scene, not stale Test Movie"
        );
        let (_, explicit_m1_body) = raw_http_request(
            addr,
            &format!("GET /{mount_slug}/data.json HTTP/1.1\r\nHost: {host_port}\r\n\r\n"),
        );
        assert_eq!(explicit_m1_body, br#"{"source":"movie"}"#);
        let (_, explicit_m2_body) = raw_http_request(
            addr,
            &format!("GET /{scene_slug}/data.json HTTP/1.1\r\nHost: {host_port}\r\n\r\n"),
        );
        assert_eq!(explicit_m2_body, br#"{"source":"scene"}"#);

        // Re-mounting `Test Movie` switches `default_root` back to `movie` while keeping `m1` slug!
        let remount_movie_url = serve_preview_index(&index_path).expect("remount movie preview");
        assert_eq!(remount_movie_url, url_1);
        let (_, root_back_to_movie) = raw_http_request(
            addr,
            &format!("GET /data.json HTTP/1.1\r\nHost: {host_port}\r\n\r\n"),
        );
        assert_eq!(root_back_to_movie, br#"{"source":"movie"}"#);

        // 7. Graceful shutdown and clean restart on next `serve_preview_index`.
        shutdown_shared_preview_server();
        let url_restarted = serve_preview_index(&index_path).expect("restart preview server");
        assert!(url_restarted.starts_with("http://127.0.0.1:"));
        shutdown_shared_preview_server();

        let _ = std::fs::remove_file(&outside_secret);
        let _ = std::fs::remove_dir_all(&scene_root);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn t320_editor_authored_preview_advances_across_scenes_and_syncs_active_scene() {
        let mut project = Project::blank();
        project.format_version = 6;
        project.script_language = Some(neoflash_core::ScriptLanguage::ActionScript3);
        let s1_id = project.root.clone();
        let s2_id: Id = "scene-2".into();
        project.symbol_mut(&s1_id).unwrap().timeline.frame_count = 2;

        let mut s2 = project.symbol(&s1_id).unwrap().clone();
        s2.id = s2_id.clone();
        s2.name = "Scene 2".into();
        s2.timeline.frame_count = 2;
        s2.timeline.layers[0].id = "s2-layer".into();
        s2.timeline.scripts = vec![neoflash_core::FrameScript {
            frame: 0,
            script: neoflash_core::Script {
                language: neoflash_core::ScriptLanguage::ActionScript3,
                source: r#"trace("ENTERED_S2"); stop();"#.into(),
            },
        }];
        project.symbols.push(s2);
        project.scenes = vec![s1_id.clone(), s2_id.clone()];
        project.upgrade_format_for_features();

        let ctx = eframe::egui::Context::default();
        let mut editor = Editor::from_project(&ctx, project);
        editor.live_script_preview = true;
        editor.timeline_edit.play_all_scenes = true;
        editor.playing = true;
        editor.sync_authored_preview();
        assert_eq!(editor.symbol_id(), s1_id);
        assert_eq!(editor.frame, 0);

        let before = editor.frame;
        editor.tick_authored_preview();
        editor.after_playback_tick(before);
        assert_eq!(editor.symbol_id(), s1_id);
        assert_eq!(editor.frame, 1);

        let before = editor.frame;
        editor.tick_authored_preview();
        editor.after_playback_tick(before);
        editor.sync_authored_preview();
        assert_eq!(
            editor.symbol_id(),
            s2_id,
            "unstopped play_all_scenes playback must switch active editor scene to Scene 2"
        );
        assert_eq!(editor.frame, 0);
        assert!(
            !editor.playing,
            "stop() on Scene 2 frame 1 must halt editor playback"
        );
        assert!(
            editor
                .output
                .events()
                .iter()
                .any(|ev| ev.message.contains("ENTERED_S2")),
            "Scene 2 entry trace must be emitted to Output"
        );

        // Also verify scripted `nextScene()` inside Scene 1 switches the active editor scene immediately!
        editor.output.clear();
        editor.project.symbol_mut(&s1_id).unwrap().timeline.scripts =
            vec![neoflash_core::FrameScript {
                frame: 1,
                script: neoflash_core::Script {
                    language: neoflash_core::ScriptLanguage::ActionScript3,
                    source: r#"nextScene();"#.into(),
                },
            }];
        editor.switch_scene(&s1_id);
        editor.timeline_edit.play_all_scenes = false;
        editor.playing = true;
        editor.sync_authored_preview();
        editor.tick_authored_preview();
        assert_eq!(
            editor.symbol_id(),
            s2_id,
            "scripted nextScene() must switch active editor scene even when play_all_scenes is false"
        );
        assert_eq!(editor.frame, 0);
        assert!(!editor.playing);
        assert!(
            editor
                .output
                .events()
                .iter()
                .any(|ev| ev.message.contains("ENTERED_S2"))
        );
    }
}
