//! Ephemeral transport state. The serialized document stays authored and pure.
use crate::app::Editor;
use neoflash_core::{
    AuthoredDraw, AuthoredPlayback, AuthoredScene, ElementKind, Id, Project, SoundCommand,
    VideoCommand, VideoSyncMode, evaluate_scene, evaluate_scene_with_buttons,
    path_commands_fingerprint,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

const VIDEO_HISTORY: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActivePreviewVideo {
    pub(crate) channel: String,
    pub(crate) target_path: Vec<Id>,
    pub(crate) layer_id: Id,
    pub(crate) resource: Id,
    pub(crate) sync: VideoSyncMode,
    pub(crate) time_ms: u32,
    pub(crate) playing: bool,
    pub(crate) muted: bool,
    pub(crate) loop_video: bool,
    pub(crate) last_cue_point: Option<String>,
}

/// Editor video preview synchronization state (`T-167`), mirroring `TimelineAudio`.
#[derive(Default)]
pub(crate) struct TimelineVideoPreview {
    preview: Option<u64>,
    pub(crate) channels: HashMap<String, ActivePreviewVideo>,
    pub(crate) history: Vec<VideoCommand>,
}

impl TimelineVideoPreview {
    pub(crate) fn follow_preview(&mut self, epoch: Option<u64>) {
        if self.preview != epoch {
            self.channels.clear();
            self.preview = epoch;
        }
    }

    pub(crate) fn apply(&mut self, commands: Vec<VideoCommand>) -> Vec<String> {
        let mut logs = Vec::new();
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
                    target_time_ms,
                    playing,
                    muted,
                    loop_video,
                } => {
                    let prev_cue = self
                        .channels
                        .get(&channel)
                        .and_then(|v| v.last_cue_point.clone());
                    self.channels.insert(
                        channel.clone(),
                        ActivePreviewVideo {
                            channel,
                            target_path,
                            layer_id,
                            resource,
                            sync: VideoSyncMode::Stream,
                            time_ms: target_time_ms,
                            playing,
                            muted,
                            loop_video,
                            last_cue_point: prev_cue,
                        },
                    );
                }
                VideoCommand::StartEvent {
                    channel,
                    target_path,
                    layer_id,
                    resource,
                    start_offset_ms,
                    loop_video,
                    muted,
                } => {
                    self.channels.insert(
                        channel.clone(),
                        ActivePreviewVideo {
                            channel,
                            target_path,
                            layer_id,
                            resource,
                            sync: VideoSyncMode::Event,
                            time_ms: start_offset_ms,
                            playing: true,
                            muted,
                            loop_video,
                            last_cue_point: None,
                        },
                    );
                }
                VideoCommand::Stop { resource } => {
                    self.channels.retain(|_, v| v.resource != resource);
                }
                VideoCommand::StopChannel { channel } => {
                    self.channels.remove(&channel);
                }
                VideoCommand::PauseStreams => {
                    for v in self.channels.values_mut() {
                        if v.sync == VideoSyncMode::Stream {
                            v.playing = false;
                        }
                    }
                }
                VideoCommand::StopAll => {
                    self.channels.clear();
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
            .or_else(|| self.channels.values().next())
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

impl Editor {
    fn valid_authored_preview(&self) -> Option<&AuthoringPreview> {
        self.authored_preview.as_ref().filter(|preview| {
            preview.symbol == self.symbol_id()
                && preview.playback.frame() == self.frame
                && preview.project == self.project
        })
    }

    pub(crate) fn reset_authored_preview(&mut self) {
        self.authored_preview = None;
        self.accumulator = 0.;
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
            static EPOCH: AtomicU64 = AtomicU64::new(1);
            let mut playback = AuthoredPlayback::with_live_scripts(
                &self.project,
                &symbol,
                self.frame,
                self.live_script_preview,
            );
            let mut swf_player = if self.live_script_preview {
                self.import_source
                    .as_deref()
                    .filter(|p| {
                        p.extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| e.eq_ignore_ascii_case("swf"))
                    })
                    .and_then(|p| neoflash_import::SwfBytecodePlayer::from_path(p).ok())
            } else {
                None
            };
            self.frame = playback.frame();
            let mut traces = playback.drain_traces();
            let mut halted = playback.is_root_halted_by_script();
            if let Some(swf) = &mut swf_player {
                let state = swf.step_frame();
                for t in state.traces {
                    if !traces.contains(&t) {
                        traces.push(t);
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
                epoch: EPOCH.fetch_add(1, Ordering::Relaxed),
            });
            for line in traces {
                self.output.push(format!("[trace] {line}"));
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
        let mut halted = false;
        if let Some(preview) = &mut self.authored_preview {
            preview.playback.set_live_scripts(self.live_script_preview);
            preview.playback.tick(&self.project);
            self.frame = preview.playback.frame();
            traces = preview.playback.drain_traces();
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
                if !state.playing {
                    halted = true;
                }
            }
        }
        for line in traces {
            self.output.push(format!("[trace] {line}"));
        }
        if halted {
            self.playing = false;
        }
    }

    /// Directly load and execute a `.swf` movie (AVM1 or AVM2 bytecode + display list + timeline)
    /// inside the editor for `max_frames`, logging all `trace()` output to the Output panel (`T-168`).
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
        if let Some(last) = result.frames_executed.last() {
            self.frame = last
                .frame
                .min(self.symbol().timeline.frame_count.saturating_sub(1));
            self.playing = last.playing;
        }
        Ok(result)
    }

    /// Dispatch a click event on a placed element path during live authoring preview.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn click_authored_preview(&mut self, path: &[Id]) {
        let mut traces = Vec::new();
        let mut halted = false;
        if let Some(preview) = &mut self.authored_preview {
            preview.playback.set_live_scripts(self.live_script_preview);
            preview.playback.click(&self.project, path);
            self.frame = preview.playback.frame();
            traces = preview.playback.drain_traces();
            halted = preview.playback.is_root_halted_by_script();
        }
        for line in traces {
            self.output.push(format!("[trace] {line}"));
        }
        if halted {
            self.playing = false;
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
            Some(preview) if self.playing => preview.playback.drain_sound_commands(),
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
    pub(crate) fn pump_timeline_video(&mut self) {
        let epoch = self.preview_epoch();
        let commands = self.drain_preview_videos();
        self.timeline_video.follow_preview(epoch);
        for log_line in self.timeline_video.apply(commands) {
            self.output.push(log_line);
        }
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
            Some(preview) => preview.playback.scene(&eval_project, true, show_masking),
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
