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
    include_fp: u64,
    symbol: Id,
    pub(crate) playback: AuthoredPlayback,
    pub(crate) swf_player: Option<neoflash_import::SwfBytecodePlayer>,
    /// Ephemeral synthetic stage project used when previewing an unreconstructible `.swf` via
    /// `SwfBytecodePlayer` fallback so `editor.project` and `editor.history` stay untouched.
    pub(crate) standalone_swf_stage: Option<Project>,
    /// Queued `SoundCommand`s drained from `swf_player` across ticks in this preview session.
    pub(crate) pending_swf_sounds: Vec<SoundCommand>,
    /// Distinguishes playback sessions; a new session silences the previous one's sounds.
    epoch: u64,
    /// True when root `stop()` halted editor transport; pressing `Play` (`Enter`) again restarts
    /// cleanly from Frame 1 instead of snapping immediately back to paused (`T-356`).
    halted_transport: bool,
}

fn ensure_standalone_swf_proxy_symbols(
    stage: &mut Project,
    entries: &[neoflash_import::SwfDisplayEntry],
) {
    for entry in entries {
        if entry.character_id != 0 {
            let sym_id = format!("swf-sym-{}", entry.character_id);
            let min_frames = entry.total_frames.max(1);
            if let Some(existing) = stage.symbol_mut(&sym_id) {
                existing.timeline.frame_count = existing.timeline.frame_count.max(min_frames);
            } else {
                stage.symbols.push(neoflash_core::Symbol {
                    id: sym_id.clone(),
                    name: format!("SWF Character {}", entry.character_id),
                    folder: String::new(),
                    kind: neoflash_core::SymbolKind::MovieClip,
                    class_name: None,
                    linkage_id: None,
                    timeline: neoflash_core::Timeline {
                        frame_count: min_frames,
                        layers: vec![neoflash_core::Layer {
                            id: format!("{sym_id}-layer"),
                            name: "Layer 1".into(),
                            visible: true,
                            locked: false,
                            keyframes: vec![neoflash_core::Keyframe {
                                frame: 0,
                                elements: vec![neoflash_core::Element {
                                    id: format!("{sym_id}-proxy"),
                                    name: "Proxy".into(),
                                    kind: neoflash_core::ElementKind::Rectangle {
                                        width: 40.0,
                                        height: 40.0,
                                        fill: [80, 150, 235, 255],
                                        stroke: [20, 40, 70, 255],
                                        stroke_width: 1.0,
                                    },
                                    ..neoflash_core::Element::default()
                                }],
                                ..neoflash_core::Keyframe::default()
                            }],
                            properties: neoflash_core::LayerProperties::default(),
                        }],
                        scripts: Vec::new(),
                        guides: Vec::new(),
                    },
                });
            }
        }
        if !entry.children.is_empty() {
            ensure_standalone_swf_proxy_symbols(stage, &entry.children);
        }
    }
}

fn standalone_stage_has_all_char_symbols(
    stage: &Project,
    entries: &[neoflash_import::SwfDisplayEntry],
) -> bool {
    entries.iter().all(|entry| {
        (entry.character_id == 0
            || stage
                .symbol(&format!("swf-sym-{}", entry.character_id))
                .is_some())
            && standalone_stage_has_all_char_symbols(stage, &entry.children)
    })
}

fn ensure_standalone_swf_audio_resources(
    stage: &mut Project,
    events: &[neoflash_import::SwfAudioEvent],
) {
    if events.is_empty() {
        return;
    }
    let Ok(wav) = neoflash_core::wrap_pcm_as_wav(&[0u8; 512], 22_050, 1, 16) else {
        return;
    };
    let mut added = false;
    for ev in events {
        if ev.sound_id > 0 {
            let res_id = format!("swf-sound-{}", ev.sound_id);
            if !stage.resources.iter().any(|r| r.id == res_id)
                && let Ok(unit) = neoflash_core::ResourceUnit::from_bytes(
                    res_id,
                    format!("audio/swf_sound_{}.wav", ev.sound_id),
                    "audio/wav",
                    &wav,
                )
            {
                stage.resources.push(unit);
                added = true;
            }
        }
        if ev.sync.eq_ignore_ascii_case("stream") {
            for seq in 1..=4 {
                let res_id = format!("swf-stream-root-{seq}");
                if !stage.resources.iter().any(|r| r.id == res_id)
                    && let Ok(unit) = neoflash_core::ResourceUnit::from_bytes(
                        res_id,
                        format!("audio/swf_stream_root_{seq}.wav"),
                        "audio/wav",
                        &wav,
                    )
                {
                    stage.resources.push(unit);
                    added = true;
                }
            }
        }
    }
    if added {
        stage.format_version = stage.format_version.max(3);
    }
}

fn source_mentions_enter_frame(source: &str) -> bool {
    source.contains("onEnterFrame")
        || source.contains("ENTER_FRAME")
        || source.contains("enterFrame")
}

/// True when `symbol_id` (or an active independent MovieClip / class source it uses) registers
/// continuous per-frame execution (`onEnterFrame`, `Event.ENTER_FRAME`, `ObjectEvent::EnterFrame`,
/// or a multi-frame/scripted independent child MovieClip) that keeps running while the root
/// playhead is stopped on a single frame (`T-356`).
pub(crate) fn symbol_has_continuous_preview_activity(project: &Project, symbol_id: &str) -> bool {
    fn walk<'a>(
        project: &'a Project,
        symbol_id: &'a str,
        is_root: bool,
        visited: &mut HashSet<&'a str>,
    ) -> bool {
        if !visited.insert(symbol_id) {
            return false;
        }
        let Some(sym) = project.symbol(symbol_id) else {
            return false;
        };
        if !is_root
            && (sym.timeline.frame_count > 1
                || sym
                    .timeline
                    .scripts
                    .iter()
                    .any(|fs| !fs.script.source.trim().is_empty()))
        {
            return true;
        }
        if sym
            .timeline
            .scripts
            .iter()
            .any(|fs| source_mentions_enter_frame(&fs.script.source))
        {
            return true;
        }
        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    if el.scripts.iter().any(|h| {
                        h.event == neoflash_core::ObjectEvent::EnterFrame
                            || source_mentions_enter_frame(&h.script.source)
                    }) {
                        return true;
                    }
                    if let ElementKind::Instance {
                        symbol_id: child_id,
                        playback: neoflash_core::InstancePlayback::Independent,
                    } = &el.kind
                        && walk(project, child_id, false, visited)
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    if (project.document_class.is_some() || project.symbols.iter().any(|s| s.class_name.is_some()))
        && project
            .sources
            .iter()
            .any(|u| source_mentions_enter_frame(&u.source))
    {
        return true;
    }
    if project.resources.iter().any(|r| {
        r.path.to_ascii_lowercase().ends_with(".as")
            && r.decoded_bytes()
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .is_some_and(|text| source_mentions_enter_frame(&text))
    }) {
        return true;
    }
    let mut visited = HashSet::new();
    walk(project, symbol_id, true, &mut visited)
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
        let include_fp = self.hydrated_project_and_fingerprint().1;
        self.authored_preview.as_ref().filter(|preview| {
            let frame_matches = preview
                .swf_player
                .as_ref()
                .map_or(preview.playback.frame() == self.frame, |swf| {
                    swf.presented_frame() == self.frame
                });
            preview.symbol == self.symbol_id()
                && frame_matches
                && preview.project == self.project
                && preview.include_fp == include_fp
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
        }) && let Ok(bytes) = res.decoded_bundle_bytes()
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

    fn effective_swf_max_frame(
        &self,
        swf: &neoflash_import::SwfBytecodePlayer,
        is_standalone: bool,
    ) -> u32 {
        if is_standalone {
            swf.total_frames().max(1).saturating_sub(1)
        } else {
            self.symbol()
                .timeline
                .frame_count
                .max(swf.total_frames())
                .max(1)
                .saturating_sub(1)
        }
    }

    pub(crate) fn has_standalone_swf_preview(&self) -> bool {
        self.authored_preview
            .as_ref()
            .is_some_and(|p| p.standalone_swf_stage.is_some() && p.swf_player.is_some())
    }

    pub(crate) fn reset_authored_preview(&mut self) {
        let had_swf_or_standalone = self
            .authored_preview
            .as_ref()
            .is_some_and(|p| p.swf_player.is_some() || p.standalone_swf_stage.is_some());
        let had_standalone = self
            .authored_preview
            .as_ref()
            .is_some_and(|p| p.standalone_swf_stage.is_some());
        self.authored_preview = None;
        self.accumulator = 0.;
        self.clear_live_preview_pointer_state();
        if had_swf_or_standalone {
            self.timeline_audio
                .apply(&self.project, vec![SoundCommand::StopAll]);
        }
        if had_standalone {
            self.frame = self
                .frame
                .min(self.symbol().timeline.frame_count.saturating_sub(1));
        }
    }

    pub(crate) fn sync_authored_preview(&mut self) {
        let include_fp = self.hydrated_project_and_fingerprint().1;
        // Preserve standalone SWF fallback sessions across timeline seeks and Play-after-halt
        // unless the active authored document or edited symbol changed.
        if let Some(has_standalone) = self.authored_preview.as_ref().map(|p| {
            p.standalone_swf_stage.is_some()
                && p.swf_player.is_some()
                && p.symbol == self.symbol_id()
                && p.project == self.project
                && p.include_fp == include_fp
        }) && has_standalone
        {
            let needs_restart = self.playing
                && self
                    .authored_preview
                    .as_ref()
                    .is_some_and(|p| p.halted_transport);
            let needs_seek = !self.playing
                && self
                    .authored_preview
                    .as_ref()
                    .and_then(|p| p.swf_player.as_ref())
                    .is_some_and(|swf| swf.presented_frame() != self.frame);
            if needs_restart {
                self.clear_simple_button_state();
                let mut traces = Vec::new();
                let mut unsupported = Vec::new();
                let mut stop_transport = false;
                if let Some(preview) = &mut self.authored_preview
                    && let Some(swf) = &mut preview.swf_player
                {
                    let state = swf.restart();
                    self.frame = state.frame;
                    traces = state.traces;
                    unsupported = state.unsupported_diagnostics;
                    stop_transport = !state.playing;
                    if let Some(stage) = &mut preview.standalone_swf_stage {
                        ensure_standalone_swf_proxy_symbols(stage, &state.display_list);
                        ensure_standalone_swf_audio_resources(stage, &state.audio_events);
                    }
                    let audio_proj = preview
                        .standalone_swf_stage
                        .as_ref()
                        .unwrap_or(&self.project);
                    let mut cmds = vec![SoundCommand::StopAll];
                    for sc in swf.drain_sound_commands(audio_proj) {
                        if sc != SoundCommand::StopAll {
                            cmds.push(sc);
                        }
                    }
                    preview.pending_swf_sounds = cmds;
                    preview.epoch = next_preview_epoch();
                    preview.halted_transport = stop_transport;
                }
                let loc = self.current_preview_output_location();
                for line in traces {
                    self.push_output_event(
                        crate::output_pipeline::OutputEvent::trace(format!("[trace] {line}"))
                            .with_location(loc.clone()),
                    );
                }
                for diag in unsupported {
                    self.push_output_unique(
                        crate::output_pipeline::OutputEvent::warning(
                            crate::output_pipeline::OutputSource::Runtime,
                            format!("[swf] {diag}"),
                        )
                        .with_location(loc.clone()),
                    );
                }
                if stop_transport {
                    self.playing = false;
                }
                return;
            }
            if needs_seek {
                let target = self.frame;
                let _ = self.seek_swf_preview(target);
                return;
            }
        }
        // Also catches direct inspector edits, undo and document replacement.
        // Resource payloads share their immutable storage with this snapshot.
        if self.authored_preview.is_some() && self.valid_authored_preview().is_none() {
            self.reset_authored_preview();
        }
        if self.playing
            && self
                .authored_preview
                .as_ref()
                .is_some_and(|preview| preview.halted_transport)
        {
            self.reset_authored_preview();
        }
        if self.playing {
            self.clear_simple_button_state();
        }
        if self.playing && self.authored_preview.is_none() {
            let symbol = self.symbol_id().to_owned();
            let (hydrated_cow, include_fp) = self.hydrated_project_and_fingerprint();
            let hydrated = hydrated_cow.into_owned();
            let mut playback = AuthoredPlayback::with_live_scripts(
                &hydrated,
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
            let mut pending_swf_sounds = Vec::new();
            let mut halted = playback.is_root_halted_by_script();
            if let Some(swf) = &mut swf_player {
                let state = swf.seek_to_frame(self.frame);
                let max_f = self.effective_swf_max_frame(swf, false);
                self.frame = state.frame.min(max_f);
                pending_swf_sounds = swf.drain_sound_commands(&self.project);
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
            let keep_live_heartbeat = swf_player.is_none()
                && self.live_script_preview
                && symbol_has_continuous_preview_activity(&hydrated, &playback_symbol);
            let stop_transport = halted && !keep_live_heartbeat;
            self.authored_preview = Some(AuthoringPreview {
                playback,
                swf_player,
                standalone_swf_stage: None,
                pending_swf_sounds,
                project: self.project.clone(),
                include_fp,
                symbol: playback_symbol,
                epoch: next_preview_epoch(),
                halted_transport: stop_transport,
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
            if stop_transport {
                let halted_sounds = self.drain_preview_sounds_unchecked();
                if !halted_sounds.is_empty() {
                    self.timeline_audio.apply(&self.project, halted_sounds);
                }
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
        let hydrated = self.project_with_hydrated_includes().into_owned();
        let symbol_frame_cap = self.symbol().timeline.frame_count;
        if let Some(preview) = &mut self.authored_preview {
            let is_standalone_swf =
                preview.standalone_swf_stage.is_some() && preview.swf_player.is_some();
            if !is_standalone_swf {
                preview
                    .playback
                    .set_live_scripts(&hydrated, self.live_script_preview);
                preview.playback.tick(&hydrated);
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
            }
            if self.live_script_preview
                && let Some(swf) = &mut preview.swf_player
            {
                let state = swf.step_frame();
                let max_f = if is_standalone_swf {
                    swf.total_frames().max(1).saturating_sub(1)
                } else {
                    symbol_frame_cap
                        .max(swf.total_frames())
                        .max(1)
                        .saturating_sub(1)
                };
                self.frame = state.frame.min(max_f);
                if let Some(stage) = &mut preview.standalone_swf_stage {
                    ensure_standalone_swf_proxy_symbols(stage, &state.display_list);
                    ensure_standalone_swf_audio_resources(stage, &state.audio_events);
                }
                let audio_proj = preview
                    .standalone_swf_stage
                    .as_ref()
                    .unwrap_or(&self.project);
                for sc in swf.drain_sound_commands(audio_proj) {
                    preview.pending_swf_sounds.push(sc);
                }
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
        let keep_live_heartbeat = self.authored_preview.as_ref().is_some_and(|p| {
            p.swf_player.is_none()
                && self.live_script_preview
                && symbol_has_continuous_preview_activity(&hydrated, &p.symbol)
        });
        let stop_transport = halted && !keep_live_heartbeat;
        if let Some(preview) = &mut self.authored_preview {
            preview.halted_transport = stop_transport;
        }
        if stop_transport {
            let halted_sounds = self.drain_preview_sounds_unchecked();
            if !halted_sounds.is_empty() {
                let audio_proj = self
                    .authored_preview
                    .as_ref()
                    .and_then(|p| p.standalone_swf_stage.clone());
                self.timeline_audio
                    .apply(audio_proj.as_ref().unwrap_or(&self.project), halted_sounds);
            }
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
        let symbol = self.symbol_id().to_owned();
        let initial_include_fp = self.hydrated_project_and_fingerprint().1;
        let mut standalone_swf_stage = self
            .authored_preview
            .as_mut()
            .filter(|p| {
                p.symbol == symbol
                    && p.project == self.project
                    && p.include_fp == initial_include_fp
            })
            .and_then(|p| p.standalone_swf_stage.take());
        let mut swf = self
            .authored_preview
            .as_mut()
            .and_then(|p| p.swf_player.take())
            .or_else(|| self.load_swf_bytecode_player())?;
        let max_frame = self.effective_swf_max_frame(&swf, standalone_swf_stage.is_some());
        let clamped_target = target_frame.min(max_frame);
        let state = swf.seek_to_frame(clamped_target);
        self.frame = state.frame.min(max_frame);
        self.playing = false;
        if let Some(stage) = &mut standalone_swf_stage {
            ensure_standalone_swf_proxy_symbols(stage, &state.display_list);
            ensure_standalone_swf_audio_resources(stage, &state.audio_events);
        }
        let playback_frame = self
            .frame
            .min(self.symbol().timeline.frame_count.saturating_sub(1));
        let (playback, include_fp) = {
            let (hydrated_cow, fp) = self.hydrated_project_and_fingerprint();
            let pb = AuthoredPlayback::with_live_scripts(
                &hydrated_cow,
                &symbol,
                playback_frame,
                self.live_script_preview && standalone_swf_stage.is_none(),
            );
            (pb, fp)
        };
        {
            let audio_proj = standalone_swf_stage.as_ref().unwrap_or(&self.project);
            let swf_cmds = swf.drain_sound_commands(audio_proj);
            let mut sync_cmds = Vec::with_capacity(swf_cmds.len() + 1);
            if swf_cmds.first() != Some(&SoundCommand::StopAll) {
                sync_cmds.push(SoundCommand::StopAll);
            }
            sync_cmds.extend(swf_cmds);
            self.timeline_audio.apply(audio_proj, sync_cmds);
        }
        let seek_loc = self.current_preview_output_location();
        for line in &state.traces {
            self.push_output_event(
                crate::output_pipeline::OutputEvent::trace(format!("[trace] {line}"))
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
            standalone_swf_stage,
            pending_swf_sounds: Vec::new(),
            project: self.project.clone(),
            include_fp,
            symbol,
            epoch: next_preview_epoch(),
            halted_transport: false,
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
        let symbol = self.symbol_id().to_owned();
        let include_fp = self.hydrated_project_and_fingerprint().1;
        let can_step_in_place = delta == 1
            && self.authored_preview.as_ref().is_some_and(|p| {
                p.symbol == symbol
                    && p.project == self.project
                    && p.include_fp == include_fp
                    && p.swf_player.as_ref().is_some_and(|swf| {
                        let max_f =
                            self.effective_swf_max_frame(swf, p.standalone_swf_stage.is_some());
                        self.frame < max_f
                            && swf.presented_frame() == self.frame
                            && swf.is_playing()
                            && swf.current_frame() == self.frame + 1
                    })
            });
        if can_step_in_place {
            let hydrated_owned = self.hydrated_project_and_fingerprint().0.into_owned();
            let mut state_opt = None;
            let mut step_sounds = Vec::new();
            let mut audio_stage_clone = None;
            let symbol_cap = self.symbol().timeline.frame_count.saturating_sub(1);
            if let Some(preview) = &mut self.authored_preview
                && let Some(swf) = &mut preview.swf_player
            {
                let is_standalone = preview.standalone_swf_stage.is_some();
                let max_f = if is_standalone {
                    swf.total_frames().max(1).saturating_sub(1)
                } else {
                    symbol_cap
                        .saturating_add(1)
                        .max(swf.total_frames())
                        .max(1)
                        .saturating_sub(1)
                };
                let state = swf.step_frame();
                self.frame = state.frame.min(max_f);
                self.playing = false;
                if let Some(stage) = &mut preview.standalone_swf_stage {
                    ensure_standalone_swf_proxy_symbols(stage, &state.display_list);
                    ensure_standalone_swf_audio_resources(stage, &state.audio_events);
                    audio_stage_clone = Some(stage.clone());
                }
                let audio_proj = preview
                    .standalone_swf_stage
                    .as_ref()
                    .unwrap_or(&self.project);
                step_sounds = swf.drain_sound_commands(audio_proj);
                preview.playback = AuthoredPlayback::with_live_scripts(
                    &hydrated_owned,
                    &symbol,
                    self.frame.min(symbol_cap),
                    self.live_script_preview && !is_standalone,
                );
                preview.halted_transport = false;
                state_opt = Some(state);
            }
            if let Some(state) = state_opt {
                if !step_sounds.is_empty() {
                    self.timeline_audio.apply(
                        audio_stage_clone.as_ref().unwrap_or(&self.project),
                        step_sounds,
                    );
                }
                let step_loc = self.current_preview_output_location();
                for line in &state.traces {
                    self.push_output_event(
                        crate::output_pipeline::OutputEvent::trace(format!("[trace] {line}"))
                            .with_location(step_loc.clone()),
                    );
                }
                for diag in &state.unsupported_diagnostics {
                    self.push_output_unique(
                        crate::output_pipeline::OutputEvent::warning(
                            crate::output_pipeline::OutputSource::Runtime,
                            format!("[swf] {diag}"),
                        )
                        .with_location(step_loc.clone()),
                    );
                }
                return Some(state);
            }
        }
        let max_frame = self
            .authored_preview
            .as_ref()
            .and_then(|p| {
                p.swf_player
                    .as_ref()
                    .map(|swf| self.effective_swf_max_frame(swf, p.standalone_swf_stage.is_some()))
            })
            .or_else(|| {
                self.load_swf_bytecode_player()
                    .map(|swf| self.effective_swf_max_frame(&swf, false))
            })?;
        let base = i64::from(self.frame);
        let next = (base + i64::from(delta)).clamp(0, i64::from(max_frame)) as u32;
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
            let max_f = self.effective_swf_max_frame(&swf_player, false);
            self.frame = last.frame.min(max_f);
            self.playing = last.playing;
        }
        let pending_swf_sounds = swf_player.drain_sound_commands(&self.project);
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
        let (hydrated_cow, include_fp) = self.hydrated_project_and_fingerprint();
        let playback_frame = self
            .frame
            .min(self.symbol().timeline.frame_count.saturating_sub(1));
        let playback = AuthoredPlayback::with_live_scripts(
            &hydrated_cow,
            &symbol,
            playback_frame,
            self.live_script_preview,
        );
        self.authored_preview = Some(AuthoringPreview {
            playback,
            swf_player: Some(swf_player),
            standalone_swf_stage: None,
            pending_swf_sounds,
            project: self.project.clone(),
            include_fp,
            symbol,
            epoch: next_preview_epoch(),
            halted_transport: !self.playing,
        });
        Ok(result)
    }

    /// Attach a standalone bounded `SwfBytecodePlayer` Stage preview session from raw `.swf`
    /// bytes when static editable timeline reconstruction fails (`T-471`).
    ///
    /// Leaves `self.project`, `self.saved_project`, `self.history`, `self.path`, and
    /// `self.import_source` completely untouched while enabling Stage projection, `seek`,
    /// `step`, `restart`, `trace()` routing, and audio synchronization.
    pub fn attach_standalone_swf_preview_bytes(
        &mut self,
        bytes: &[u8],
        source_path: Option<&std::path::Path>,
        reconstruction_reason: Option<&str>,
    ) -> Result<neoflash_import::SwfPlaybackResult, String> {
        let mut probe =
            neoflash_import::SwfBytecodePlayer::from_bytes(bytes).map_err(|e| e.to_string())?;
        let probe_limit = usize::try_from(probe.total_frames().max(1))
            .unwrap_or(60)
            .clamp(1, 60);
        let probe_result = probe.run_frames(probe_limit);

        let symbol = self.symbol_id().to_owned();
        let mut standalone_stage = Project::blank();
        let old_root = standalone_stage.root.clone();
        standalone_stage.root = symbol.clone();
        standalone_stage.width = self.project.width;
        standalone_stage.height = self.project.height;
        standalone_stage.fps = self.project.fps.max(1);
        standalone_stage.background = self.project.background;
        if let Some(root_sym) = standalone_stage.symbol_mut(&old_root) {
            root_sym.id = symbol.clone();
            root_sym.timeline.frame_count = probe_result.total_frames.max(1);
        }
        for frame_state in &probe_result.frames_executed {
            ensure_standalone_swf_proxy_symbols(&mut standalone_stage, &frame_state.display_list);
        }
        ensure_standalone_swf_audio_resources(&mut standalone_stage, &probe_result.audio_events);

        let mut swf_player =
            neoflash_import::SwfBytecodePlayer::from_bytes(bytes).map_err(|e| e.to_string())?;
        let state0 = swf_player.seek_to_frame(0);
        ensure_standalone_swf_proxy_symbols(&mut standalone_stage, &state0.display_list);
        ensure_standalone_swf_audio_resources(&mut standalone_stage, &state0.audio_events);

        self.frame = state0
            .frame
            .min(probe_result.total_frames.max(1).saturating_sub(1));
        self.playing = false;

        let mut sync_cmds = vec![SoundCommand::StopAll];
        for sc in swf_player.drain_sound_commands(&standalone_stage) {
            if sc != SoundCommand::StopAll {
                sync_cmds.push(sc);
            }
        }
        self.timeline_audio.apply(&standalone_stage, sync_cmds);

        let (hydrated_cow, include_fp) = self.hydrated_project_and_fingerprint();
        let playback_frame = self
            .frame
            .min(self.symbol().timeline.frame_count.saturating_sub(1));
        let playback =
            AuthoredPlayback::with_live_scripts(&hydrated_cow, &symbol, playback_frame, false);

        let loc = source_path.map_or_else(
            || self.current_preview_output_location(),
            |p| crate::output_pipeline::OutputLocation::for_file(p.to_path_buf()),
        );
        let reason_suffix = reconstruction_reason
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|r| format!(" ({r})"))
            .unwrap_or_default();
        let fallback_msg = format!(
            "[swf-fallback] Static timeline reconstruction failed{reason_suffix}; attached bounded SwfBytecodePlayer fallback preview ({:?} VM, SWF v{}, {} frame(s)) on Stage while leaving the active authored document unchanged.",
            probe_result.vm_kind, probe_result.swf_version, probe_result.total_frames
        );
        self.status = fallback_msg.clone();
        self.push_output_coalesced(
            crate::output_pipeline::OutputEvent::warning(
                crate::output_pipeline::OutputSource::Import,
                fallback_msg,
            )
            .with_location(loc.clone()),
        );
        let runtime_loc = self.current_preview_output_location();
        for line in &state0.traces {
            self.push_output_event(
                crate::output_pipeline::OutputEvent::trace(format!("[trace] {line}"))
                    .with_location(runtime_loc.clone()),
            );
        }
        for diag in &state0.unsupported_diagnostics {
            self.push_output_unique(
                crate::output_pipeline::OutputEvent::warning(
                    crate::output_pipeline::OutputSource::Runtime,
                    format!("[swf] {diag}"),
                )
                .with_location(runtime_loc.clone()),
            );
        }

        self.authored_preview = Some(AuthoringPreview {
            playback,
            swf_player: Some(swf_player),
            standalone_swf_stage: Some(standalone_stage),
            pending_swf_sounds: Vec::new(),
            project: self.project.clone(),
            include_fp,
            symbol,
            epoch: next_preview_epoch(),
            halted_transport: false,
        });
        Ok(probe_result)
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
        let hydrated = self.project_with_hydrated_includes().into_owned();
        if let Some(preview) = &mut self.authored_preview {
            was_halted = preview.playback.is_root_halted_by_script();
            preview
                .playback
                .set_live_scripts(&hydrated, self.live_script_preview);
            preview.playback.click(&hydrated, path);
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
        let keep_live_heartbeat = self.authored_preview.as_ref().is_some_and(|p| {
            p.swf_player.is_none()
                && self.live_script_preview
                && symbol_has_continuous_preview_activity(&hydrated, &p.symbol)
        });
        let stop_transport = halted && !keep_live_heartbeat;
        if let Some(preview) = &mut self.authored_preview {
            preview.halted_transport = stop_transport;
        }
        if stop_transport {
            self.playing = false;
            self.timeline_video.pause_active();
        } else if clicked && was_halted {
            if let Some(preview) = &mut self.authored_preview {
                preview.halted_transport = false;
            }
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
        if !self.playing {
            return vec![];
        }
        self.drain_preview_sounds_unchecked()
    }

    fn drain_preview_sounds_unchecked(&mut self) -> Vec<SoundCommand> {
        match &mut self.authored_preview {
            Some(preview) => {
                let mut cmds = preview.playback.drain_sound_commands();
                for sc in std::mem::take(&mut preview.pending_swf_sounds) {
                    if !cmds.contains(&sc) {
                        cmds.push(sc);
                    }
                }
                if let Some(swf) = &mut preview.swf_player {
                    let audio_proj = preview
                        .standalone_swf_stage
                        .as_ref()
                        .unwrap_or(&self.project);
                    for sc in swf.drain_sound_commands(audio_proj) {
                        if !cmds.contains(&sc) {
                            cmds.push(sc);
                        }
                    }
                }
                cmds
            }
            None => vec![],
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
        self.valid_authored_preview()
            .filter(|preview| preview.standalone_swf_stage.is_none())?
            .playback
            .clip_frame(std::slice::from_ref(element))
    }

    /// Frames of the whole paused preview tree, relative to the active symbol.
    /// An empty map means static authored evaluation is sufficient.
    pub(crate) fn presented_clip_frames(&self) -> HashMap<Vec<Id>, u32> {
        self.valid_authored_preview()
            .filter(|preview| preview.standalone_swf_stage.is_none())
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
        if self.has_standalone_swf_preview() {
            return AuthoredScene::default();
        }
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
        let preview_has_standalone_swf =
            preview.is_some_and(|p| p.standalone_swf_stage.is_some() && p.swf_player.is_some());
        if !self.playing
            && !preview_has_standalone_swf
            && self.frame >= self.symbol().timeline.frame_count
        {
            return AuthoredScene::default();
        }
        let simple_buttons_active = self.enable_simple_buttons
            && !self.playing
            && !preview_has_standalone_swf
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
                if self.live_script_preview
                    && self.editing_scene()
                    && let Some(swf) = &preview.swf_player
                {
                    if let Some(standalone_stage) = &preview.standalone_swf_stage {
                        let live_entries = swf.display_list();
                        if standalone_stage_has_all_char_symbols(standalone_stage, &live_entries) {
                            return swf.evaluate_stage_scene(
                                standalone_stage,
                                &standalone_stage.root,
                                self.frame,
                                show_masking,
                            );
                        }
                        let mut patched_stage = standalone_stage.clone();
                        ensure_standalone_swf_proxy_symbols(&mut patched_stage, &live_entries);
                        return swf.evaluate_stage_scene(
                            &patched_stage,
                            &patched_stage.root,
                            self.frame,
                            show_masking,
                        );
                    }
                    let eval_project = self.project_for_stage_eval();
                    return swf.evaluate_stage_scene(
                        &eval_project,
                        self.symbol_id(),
                        self.frame,
                        show_masking,
                    );
                }
                let eval_project = self.project_for_stage_eval();
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

/// Maximum HTTP `POST /__neoflash/output` JSON body size accepted by `PreviewHttpServer` (`T-356`).
pub(crate) const MAX_PREVIEW_OUTPUT_BODY_BYTES: usize = 256 * 1024;
/// Maximum events accepted in a single `/__neoflash/output` JSON batch.
const MAX_PREVIEW_OUTPUT_BATCH_EVENTS: usize = 1_024;
/// Bounded thread-safe queue capacity for browser preview output items awaiting UI drain.
const MAX_QUEUED_PREVIEW_OUTPUT_ITEMS: usize = 8_192;
/// Maximum tracked browser preview session IDs for sequence deduplication.
const MAX_TRACKED_PREVIEW_SESSIONS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PreviewOutputKind {
    Trace,
    Warning,
    Error,
    Restart,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub(crate) struct PreviewOutputItem {
    #[serde(default)]
    pub(crate) session: Option<String>,
    #[serde(default, alias = "runId")]
    pub(crate) run_id: Option<String>,
    #[serde(default)]
    pub(crate) seq: u64,
    pub(crate) kind: PreviewOutputKind,
    #[serde(default)]
    pub(crate) message: String,
    #[serde(default, alias = "sourcePath")]
    pub(crate) source_path: Option<String>,
    #[serde(default, alias = "sourceLine")]
    pub(crate) source_line: Option<String>,
    #[serde(default)]
    pub(crate) line: Option<usize>,
    #[serde(default)]
    pub(crate) column: Option<usize>,
}

#[derive(serde::Deserialize)]
struct PreviewOutputBatchPayload {
    #[serde(default)]
    session: Option<String>,
    #[serde(default, alias = "runId")]
    run_id: Option<String>,
    events: Vec<PreviewOutputItem>,
}

#[derive(Default)]
struct PreviewOutputSink {
    queue: std::sync::Mutex<std::collections::VecDeque<PreviewOutputItem>>,
    last_seq_by_session: std::sync::Mutex<HashMap<String, u64>>,
    active_run_id: std::sync::Mutex<Option<String>>,
    repaint_ctx: std::sync::Mutex<Option<eframe::egui::Context>>,
}

impl PreviewOutputSink {
    fn set_repaint_context(&self, ctx: eframe::egui::Context) {
        if let Ok(mut guard) = self.repaint_ctx.lock() {
            *guard = Some(ctx);
        }
    }

    fn clear(&self) {
        if let Ok(mut active) = self.active_run_id.lock() {
            *active = None;
        }
        if let Ok(mut q) = self.queue.lock() {
            q.clear();
        }
        if let Ok(mut seqs) = self.last_seq_by_session.lock() {
            seqs.clear();
        }
    }

    fn set_active_run_id(&self, run_id: Option<String>) {
        let normalized = run_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        if let Ok(mut active) = self.active_run_id.lock() {
            *active = normalized;
        }
        if let Ok(mut q) = self.queue.lock() {
            q.clear();
        }
        if let Ok(mut seqs) = self.last_seq_by_session.lock() {
            seqs.clear();
        }
    }

    fn active_run_id(&self) -> Option<String> {
        self.active_run_id.lock().ok().and_then(|g| g.clone())
    }

    fn purge_pending_on_clear(&self) {
        if let Ok(mut q) = self.queue.lock() {
            q.clear();
        }
    }

    fn drain(&self) -> Vec<PreviewOutputItem> {
        if let Ok(mut q) = self.queue.lock() {
            q.drain(..).collect()
        } else {
            Vec::new()
        }
    }

    fn push_batch(&self, mut payload: PreviewOutputBatchPayload) -> bool {
        let batch_run_id = payload
            .run_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                payload.events.iter().find_map(|ev| {
                    ev.run_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                })
            });

        if let Ok(expected_guard) = self.active_run_id.lock()
            && let Some(expected) = expected_guard.as_deref()
            && batch_run_id.as_deref() != Some(expected)
        {
            return false;
        }

        if payload.events.is_empty() {
            return true;
        }
        if payload.events.iter().all(|ev| ev.seq > 0) {
            payload.events.sort_by_key(|ev| ev.seq);
        }
        let batch_session = payload
            .session
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);

        let mut accepted = Vec::with_capacity(payload.events.len());
        if let Ok(mut seqs) = self.last_seq_by_session.lock() {
            for mut item in payload.events {
                if item.session.is_none() {
                    item.session = batch_session.clone();
                }
                if item.run_id.is_none() {
                    item.run_id = batch_run_id.clone();
                }
                if let Some(sess) = item.session.as_deref()
                    && item.seq > 0
                {
                    if let Some(&prev_seq) = seqs.get(sess)
                        && item.seq <= prev_seq
                    {
                        continue;
                    }
                    if !seqs.contains_key(sess) && seqs.len() >= MAX_TRACKED_PREVIEW_SESSIONS {
                        seqs.clear();
                    }
                    seqs.insert(sess.to_owned(), item.seq);
                }
                accepted.push(item);
            }
        } else {
            accepted = payload.events;
        }
        if accepted.is_empty() {
            return true;
        }

        if let Ok(mut q) = self.queue.lock() {
            for item in accepted {
                if q.len() >= MAX_QUEUED_PREVIEW_OUTPUT_ITEMS {
                    q.pop_front();
                }
                q.push_back(item);
            }
        }
        if let Ok(guard) = self.repaint_ctx.lock()
            && let Some(ctx) = guard.as_ref()
        {
            ctx.request_repaint();
        }
        true
    }
}

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
/// MIME types, HTTP `Range` support for `<audio>`/`<video>` seeking, directory-jail checks, and
/// live `POST /__neoflash/output` telemetry ingestion into NeoFlash's `Output` panel (`T-356`).
#[allow(dead_code)]
pub(crate) struct PreviewHttpServer {
    addr: std::net::SocketAddr,
    table: std::sync::Arc<std::sync::RwLock<PreviewMountTable>>,
    output_sink: std::sync::Arc<PreviewOutputSink>,
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
        let output_sink = std::sync::Arc::new(PreviewOutputSink::default());
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_table = std::sync::Arc::clone(&table);
        let thread_sink = std::sync::Arc::clone(&output_sink);
        let thread_stop = std::sync::Arc::clone(&stop);
        let accept_thread = std::thread::Builder::new()
            .name("neoflash-preview-http".into())
            .spawn(move || {
                run_preview_accept_loop(listener, thread_table, thread_sink, thread_stop)
            })?;
        Ok(Self {
            addr,
            table,
            output_sink,
            stop,
            accept_thread: Some(accept_thread),
        })
    }

    pub(crate) fn set_repaint_context(&self, ctx: eframe::egui::Context) {
        self.output_sink.set_repaint_context(ctx);
    }

    pub(crate) fn clear_output_queue(&self) {
        self.output_sink.clear();
    }

    pub(crate) fn set_active_run_id(&self, run_id: Option<String>) {
        self.output_sink.set_active_run_id(run_id);
    }

    pub(crate) fn active_run_id(&self) -> Option<String> {
        self.output_sink.active_run_id()
    }

    pub(crate) fn purge_output_on_clear(&self) {
        self.output_sink.purge_pending_on_clear();
    }

    pub(crate) fn drain_output_items(&self) -> Vec<PreviewOutputItem> {
        self.output_sink.drain()
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
static SHARED_PREVIEW_REPAINT_CTX: std::sync::Mutex<Option<eframe::egui::Context>> =
    std::sync::Mutex::new(None);
static SHARED_PREVIEW_DOC_SESSION: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static SHARED_PREVIEW_ACTIVE_RUN_ID: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static NEXT_PREVIEW_RUN_COUNTER: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

/// Register the editor's `egui::Context` so background `POST /__neoflash/output` requests wake
/// `Editor::update` immediately while the browser window owns OS focus (`T-356`).
pub(crate) fn register_shared_preview_repaint_context(ctx: &eframe::egui::Context) {
    if let Ok(mut saved) = SHARED_PREVIEW_REPAINT_CTX.lock() {
        *saved = Some(ctx.clone());
    }
    if let Ok(guard) = SHARED_PREVIEW_SERVER.lock()
        && let Some(srv) = guard.as_ref()
    {
        srv.set_repaint_context(ctx.clone());
    }
}

/// Clear stale queued preview output items, allocate a fresh monotonic `run_id`, and record the
/// active document session ID when `Test Movie` (`⌘Return`) or `Test Scene` (`⌘⌥Return`) starts
/// (`T-356`, `T-379`).
pub(crate) fn begin_shared_preview_output_session(document_session: Option<&str>) -> String {
    let run_id = format!(
        "nf-run-{}",
        NEXT_PREVIEW_RUN_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    if let Ok(mut doc_guard) = SHARED_PREVIEW_DOC_SESSION.lock() {
        *doc_guard = document_session.map(str::to_owned);
    }
    if let Ok(mut run_guard) = SHARED_PREVIEW_ACTIVE_RUN_ID.lock() {
        *run_guard = Some(run_id.clone());
    }
    if let Ok(guard) = SHARED_PREVIEW_SERVER.lock()
        && let Some(srv) = guard.as_ref()
    {
        srv.set_active_run_id(Some(run_id.clone()));
    }
    run_id
}

pub(crate) fn current_shared_preview_run_id() -> Option<String> {
    SHARED_PREVIEW_ACTIVE_RUN_ID
        .lock()
        .ok()
        .and_then(|g| g.clone())
}

/// Permanently drop any queued-but-not-yet-drained items from the shared preview server when the
/// user clicks `Clear output` (`🗑`), while keeping the active `run_id` and per-session sequence
/// cursor intact so cleared logs can never replay (`T-379`).
pub(crate) fn purge_shared_preview_output_on_clear(document_session: &str) {
    if let Ok(owner) = SHARED_PREVIEW_DOC_SESSION.lock()
        && owner
            .as_deref()
            .is_none_or(|owner| owner == document_session)
        && let Ok(guard) = SHARED_PREVIEW_SERVER.lock()
        && let Some(srv) = guard.as_ref()
    {
        srv.purge_output_on_clear();
    }
}

/// Drain any pending browser `trace()` / runtime error items from the shared preview HTTP server,
/// returning `(active_document_session_id, items)`.
pub(crate) fn drain_shared_preview_output() -> (Option<String>, Vec<PreviewOutputItem>) {
    let items = if let Ok(guard) = SHARED_PREVIEW_SERVER.lock()
        && let Some(srv) = guard.as_ref()
    {
        srv.drain_output_items()
    } else {
        Vec::new()
    };
    if items.is_empty() {
        return (None, items);
    }
    let doc_session = SHARED_PREVIEW_DOC_SESSION
        .lock()
        .ok()
        .and_then(|g| g.clone());
    (doc_session, items)
}

/// Ensure the process-wide `PreviewHttpServer` is running, mount `index_file`, and return its
/// `http://127.0.0.1:<port>/...` URL (reusing the same server and mount across repeated `⌘Return` runs).
pub(crate) fn serve_preview_index(index_file: &std::path::Path) -> Result<String, String> {
    let saved_ctx = SHARED_PREVIEW_REPAINT_CTX
        .lock()
        .ok()
        .and_then(|g| g.clone());
    let saved_run_id = current_shared_preview_run_id();
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
        if let Some(ctx) = saved_ctx {
            srv.set_repaint_context(ctx);
        }
        srv.set_active_run_id(saved_run_id);
        *guard = Some(srv);
    } else if let (Some(srv), Some(ctx)) = (guard.as_ref(), saved_ctx) {
        srv.set_repaint_context(ctx);
    }
    guard
        .as_ref()
        .expect("preview server initialized")
        .mount_index_file(index_file)
}

/// Serve `index_file` on the reusable loopback HTTP server (with `Cache-Control: no-store`) and
/// launch it in the user's default browser tagged with `?nf_run=<run_id>` so stale tabs from
/// earlier runs are cleanly isolated (`T-379`).
pub(crate) fn open_preview_in_browser(index_file: &std::path::Path) -> Result<String, String> {
    let base_url = serve_preview_index(index_file)?;
    let launch_url = if let Some(run_id) = current_shared_preview_run_id() {
        let sep = if base_url.contains('?') { '&' } else { '?' };
        format!(
            "{base_url}{sep}nf_run={}",
            percent_encode_url_segment(&run_id)
        )
    } else {
        base_url
    };
    crate::platform_open::open_url(&launch_url)?;
    Ok(launch_url)
}

pub(crate) fn shutdown_shared_preview_server() {
    if let Ok(mut run_guard) = SHARED_PREVIEW_ACTIVE_RUN_ID.lock() {
        *run_guard = None;
    }
    if let Ok(mut doc_guard) = SHARED_PREVIEW_DOC_SESSION.lock() {
        *doc_guard = None;
    }
    if let Ok(mut guard) = SHARED_PREVIEW_SERVER.lock()
        && let Some(mut srv) = guard.take()
    {
        srv.shutdown();
    }
}

fn run_preview_accept_loop(
    listener: std::net::TcpListener,
    table: std::sync::Arc<std::sync::RwLock<PreviewMountTable>>,
    output_sink: std::sync::Arc<PreviewOutputSink>,
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
        let req_sink = std::sync::Arc::clone(&output_sink);
        let _ = std::thread::Builder::new()
            .name("neoflash-preview-req".into())
            .spawn(move || {
                let _ = handle_preview_http_connection(stream, &req_table, &req_sink);
            });
    }
}

fn is_preview_output_endpoint(decoded_segments: &[String]) -> bool {
    decoded_segments == ["__neoflash", "output"]
        || (decoded_segments.len() == 3
            && decoded_segments[1] == "__neoflash"
            && decoded_segments[2] == "output")
}

fn handle_preview_output_post(
    stream: &mut std::net::TcpStream,
    already_read: &[u8],
    content_length: Option<Result<usize, ()>>,
    expect_continue: bool,
    output_sink: &PreviewOutputSink,
) -> std::io::Result<()> {
    use std::io::{Read as _, Write as _};

    let content_len = match content_length {
        None => return write_preview_error(stream, 411, "Length Required", true, &[]),
        Some(Err(())) | Some(Ok(0)) => {
            return write_preview_error(stream, 400, "Bad Request", true, &[]);
        }
        Some(Ok(len)) if len > MAX_PREVIEW_OUTPUT_BODY_BYTES => {
            return write_preview_error(stream, 413, "Payload Too Large", true, &[]);
        }
        Some(Ok(len)) => len,
    };

    if expect_continue && already_read.len() < content_len {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
        stream.flush()?;
    }

    let mut body = Vec::with_capacity(content_len);
    let initial = already_read.len().min(content_len);
    body.extend_from_slice(&already_read[..initial]);
    let mut scratch = [0_u8; 4096];
    while body.len() < content_len {
        let remaining = content_len - body.len();
        let to_read = remaining.min(scratch.len());
        match stream.read(&mut scratch[..to_read]) {
            Ok(0) => return write_preview_error(stream, 400, "Bad Request", true, &[]),
            Ok(n) => body.extend_from_slice(&scratch[..n]),
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return write_preview_error(stream, 408, "Request Timeout", true, &[]);
            }
            Err(err) => return Err(err),
        }
    }

    let Ok(payload) = serde_json::from_slice::<PreviewOutputBatchPayload>(&body) else {
        return write_preview_error(stream, 400, "Bad Request", true, &[]);
    };
    if payload.events.len() > MAX_PREVIEW_OUTPUT_BATCH_EVENTS {
        return write_preview_error(stream, 413, "Payload Too Large", true, &[]);
    }

    if !output_sink.push_batch(payload) {
        return write_preview_response(
            stream,
            200,
            "OK",
            "application/json; charset=utf-8",
            b"{\"stale\":true}\n",
            true,
            &[("X-NeoFlash-Session-Stale", "1")],
        );
    }
    write_preview_response(
        stream,
        204,
        "No Content",
        "text/plain; charset=utf-8",
        b"",
        false,
        &[],
    )
}

#[allow(dead_code)]
fn handle_preview_http_connection(
    mut stream: std::net::TcpStream,
    table: &std::sync::RwLock<PreviewMountTable>,
    output_sink: &PreviewOutputSink,
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

    let mut range_header: Option<&str> = None;
    let mut content_length: Option<Result<usize, ()>> = None;
    let mut expect_continue = false;
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, val)) = line.split_once(':') {
            let name = name.trim();
            let val = val.trim();
            if name.eq_ignore_ascii_case("range") {
                range_header = Some(val);
            } else if name.eq_ignore_ascii_case("content-length") {
                content_length = Some(val.parse::<usize>().map_err(|_| ()));
            } else if name.eq_ignore_ascii_case("expect")
                && val.eq_ignore_ascii_case("100-continue")
            {
                expect_continue = true;
            }
        }
    }

    let err_send_body = method != "HEAD";
    // Strip #fragment and ?query (e.g. cache-busting `?t=...`), then decode RFC 3986 path segments.
    let without_fragment = raw_target.split('#').next().unwrap_or(raw_target);
    let raw_path = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    if !raw_path.starts_with('/') {
        return write_preview_error(&mut stream, 400, "Bad Request", err_send_body, &[]);
    }

    let mut decoded_segments: Vec<String> = Vec::new();
    for raw_seg in raw_path.split('/').skip(1) {
        if raw_seg.is_empty() {
            continue;
        }
        let Ok(seg) = percent_decode_url_segment(raw_seg) else {
            return write_preview_error(&mut stream, 400, "Bad Request", err_send_body, &[]);
        };
        if seg == "." {
            continue;
        }
        if seg == ".." || seg.contains(['/', '\\', '\0']) {
            return write_preview_error(&mut stream, 403, "Forbidden", err_send_body, &[]);
        }
        decoded_segments.push(seg);
    }

    if is_preview_output_endpoint(&decoded_segments) {
        return match method {
            "OPTIONS" => write_preview_response(
                &mut stream,
                204,
                "No Content",
                "text/plain; charset=utf-8",
                b"",
                false,
                &[("Allow", "POST, OPTIONS")],
            ),
            "POST" => handle_preview_output_post(
                &mut stream,
                &buf[header_end..],
                content_length,
                expect_continue,
                output_sink,
            ),
            _ => write_preview_error(
                &mut stream,
                405,
                "Method Not Allowed",
                err_send_body,
                &[("Allow", "POST, OPTIONS")],
            ),
        };
    }

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

    let mut file = match std::fs::File::open(&canonical_target) {
        Ok(file) => file,
        Err(_) => return write_preview_error(&mut stream, 404, "Not Found", send_body, &[]),
    };
    let total_len = match file.metadata() {
        Ok(metadata) if metadata.is_file() => metadata.len(),
        _ => return write_preview_error(&mut stream, 404, "Not Found", send_body, &[]),
    };
    let mime = preview_mime_type(&canonical_target);

    if let Some(raw_range) = range_header
        && let Some((unit, spec)) = raw_range.split_once('=')
        && unit.trim().eq_ignore_ascii_case("bytes")
        && !spec.contains(',')
    {
        match parse_http_byte_range(spec, total_len) {
            Some((start, end_inclusive)) => {
                let content_range = format!("bytes {start}-{end_inclusive}/{total_len}");
                return write_preview_file_response(
                    &mut stream,
                    206,
                    "Partial Content",
                    mime,
                    &mut file,
                    start,
                    end_inclusive - start + 1,
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

    write_preview_file_response(
        &mut stream,
        200,
        "OK",
        mime,
        &mut file,
        0,
        total_len,
        send_body,
        &[],
    )
}

#[allow(dead_code)]
fn parse_http_byte_range(spec: &str, total_len: u64) -> Option<(u64, u64)> {
    if total_len == 0 {
        return None;
    }
    let (start_str, end_str) = spec.trim().split_once('-')?;
    let start_str = start_str.trim();
    let end_str = end_str.trim();
    if start_str.is_empty() {
        let suffix_len: u64 = end_str.parse().ok()?;
        if suffix_len == 0 {
            return None;
        }
        let start = total_len.saturating_sub(suffix_len);
        Some((start, total_len - 1))
    } else {
        let start: u64 = start_str.parse().ok()?;
        if start >= total_len {
            return None;
        }
        let end = if end_str.is_empty() {
            total_len - 1
        } else {
            let parsed_end: u64 = end_str.parse().ok()?;
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

    write_preview_response_header(
        stream,
        status,
        reason,
        content_type,
        body.len() as u64,
        extra_headers,
    )?;
    if send_body && !body.is_empty() {
        stream.write_all(body)?;
    }
    stream.flush()
}

#[allow(dead_code)]
// Keep the streamed-file response parallel to `write_preview_response`; grouping these protocol
// fields would obscure the only additional inputs (file offset and length).
#[allow(clippy::too_many_arguments)]
fn write_preview_file_response(
    stream: &mut std::net::TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    file: &mut std::fs::File,
    offset: u64,
    content_len: u64,
    send_body: bool,
    extra_headers: &[(&str, &str)],
) -> std::io::Result<()> {
    use std::io::{Read as _, Seek as _, Write as _};

    write_preview_response_header(
        stream,
        status,
        reason,
        content_type,
        content_len,
        extra_headers,
    )?;
    if send_body && content_len != 0 {
        file.seek(std::io::SeekFrom::Start(offset))?;
        std::io::copy(&mut file.take(content_len), stream)?;
    }
    stream.flush()
}

#[allow(dead_code)]
fn write_preview_response_header(
    stream: &mut std::net::TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    content_len: u64,
    extra_headers: &[(&str, &str)],
) -> std::io::Result<()> {
    use std::io::Write as _;

    let mut header = format!(
        "HTTP/1.1 {status} {reason}\r\n\
Content-Type: {content_type}\r\n\
Content-Length: {content_len}\r\n\
Cache-Control: no-store, no-cache, must-revalidate, max-age=0\r\n\
Pragma: no-cache\r\n\
Expires: 0\r\n\
Accept-Ranges: bytes\r\n\
X-Content-Type-Options: nosniff\r\n\
Connection: close\r\n",
    );
    for (k, v) in extra_headers {
        header.push_str(k);
        header.push_str(": ");
        header.push_str(v);
        header.push_str("\r\n");
    }
    header.push_str("\r\n");
    stream.write_all(header.as_bytes())
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

fn format_preview_output_event(
    project: &Project,
    document_id: &str,
    item: PreviewOutputItem,
) -> (
    crate::output_pipeline::OutputEvent,
    Option<String>,
    Option<String>,
) {
    use crate::output_pipeline::{
        OutputEvent, OutputSeverity, OutputSource, format_structured_script_diagnostic_message,
        humanize_script_error_reason_with_snippet, resolve_runtime_output_location_with_snippet,
        source_line_snippet_for_output_location,
    };

    match item.kind {
        PreviewOutputKind::Trace => {
            let loc = resolve_runtime_output_location_with_snippet(
                project,
                document_id,
                item.source_path.as_deref(),
                item.line,
                item.column,
                &item.message,
                item.source_line.as_deref(),
            );
            let text = if item.message.starts_with("[trace]") {
                item.message
            } else if item.message.is_empty() {
                "[trace]".to_owned()
            } else {
                format!("[trace] {}", item.message)
            };
            (
                OutputEvent::trace(text).with_optional_location(loc),
                None,
                None,
            )
        }
        PreviewOutputKind::Warning => {
            let loc = resolve_runtime_output_location_with_snippet(
                project,
                document_id,
                item.source_path.as_deref(),
                item.line,
                item.column,
                &item.message,
                item.source_line.as_deref(),
            );
            let text = format_structured_script_diagnostic_message(
                project,
                OutputSeverity::Warning,
                loc.as_ref(),
                item.source_path.as_deref(),
                item.line,
                &item.message,
                item.source_line.as_deref(),
            );
            (
                OutputEvent::warning(OutputSource::Runtime, text).with_optional_location(loc),
                None,
                None,
            )
        }
        PreviewOutputKind::Error => {
            let loc = resolve_runtime_output_location_with_snippet(
                project,
                document_id,
                item.source_path.as_deref(),
                item.line,
                item.column,
                &item.message,
                item.source_line.as_deref(),
            );
            let clean_msg = item
                .source_path
                .as_deref()
                .and_then(|src| item.message.strip_prefix(&format!("{src}: ")))
                .unwrap_or(&item.message);
            let resolved_snippet = item
                .source_line
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    loc.as_ref()
                        .and_then(|l| source_line_snippet_for_output_location(project, l))
                });
            let reason =
                humanize_script_error_reason_with_snippet(clean_msg, resolved_snippet.as_deref());
            let text = format_structured_script_diagnostic_message(
                project,
                OutputSeverity::Error,
                loc.as_ref(),
                item.source_path.as_deref(),
                item.line,
                &item.message,
                item.source_line.as_deref(),
            );
            let status = text.lines().next().unwrap_or(&text).to_owned();
            (
                OutputEvent::error(OutputSource::Runtime, text).with_optional_location(loc),
                Some(status),
                Some(reason),
            )
        }
        PreviewOutputKind::Restart => (
            OutputEvent::info(OutputSource::Runtime, "─── Test Movie restarted ───"),
            None,
            None,
        ),
    }
}

impl Editor {
    /// Prepare a fresh `Output` telemetry session before `Test Movie` (`⌘Return`) or
    /// `Test Scene` (`⌘⌥Return`) launches (`T-356`, `T-379`).
    pub(crate) fn begin_preview_output_session(&mut self, banner: impl Into<String>) {
        self.clear_runtime_diagnostics_in_actions();
        let doc_id =
            (!self.clipboard.session.is_empty()).then_some(self.clipboard.session.as_str());
        let _ = begin_shared_preview_output_session(doc_id);
        self.push_output_event(crate::output_pipeline::OutputEvent::info(
            crate::output_pipeline::OutputSource::Runtime,
            format!("─── {} ───", banner.into()),
        ));
    }

    /// Ingest `PreviewOutputItem`s received from `POST /__neoflash/output` into the owning
    /// document's `OutputPipeline` (`T-356`, `T-382`).
    pub(crate) fn ingest_preview_output_items(
        &mut self,
        target_document_session: Option<&str>,
        items: Vec<PreviewOutputItem>,
    ) {
        if items.is_empty() || !self.has_document() {
            return;
        }
        if let Some(target_doc) = target_document_session
            && !target_doc.is_empty()
            && self.clipboard.session != target_doc
        {
            let Some(slot) = self
                .sessions
                .iter_mut()
                .flatten()
                .find(|s| s.clipboard.session == target_doc)
            else {
                // A browser tab can outlive its source document.
                return;
            };
            let doc_id = slot.clipboard.session.clone();
            slot.output.set_document_id(doc_id.clone());
            for item in items {
                let (ev, _, _) = format_preview_output_event(&slot.project, &doc_id, item);
                slot.output.push_event(ev);
            }
            return;
        }

        self.sync_output_document_id();
        let doc_id = self.clipboard.session.clone();
        for item in items {
            let (ev, next_status, err_reason) =
                format_preview_output_event(&self.project, &doc_id, item);
            if let (Some(loc), Some(reason)) = (ev.location.as_ref(), err_reason.as_deref()) {
                self.record_runtime_diagnostic_for_location(loc, reason);
            }
            if let Some(status) = next_status {
                self.status = status;
            }
            self.output.push_event(ev);
        }
    }

    /// Register `ctx` with the loopback preview server and drain any queued browser `trace()` or
    /// runtime error events into `Output` (`T-356`).
    pub(crate) fn poll_preview_output(&mut self, ctx: &eframe::egui::Context) {
        register_shared_preview_repaint_context(ctx);
        let (doc_session, items) = drain_shared_preview_output();
        if !items.is_empty() {
            self.ingest_preview_output_items(doc_session.as_deref(), items);
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

        let (suffix_range_hdr, suffix_range_body) = raw_http_request(
            addr,
            &format!(
                "GET /{mount_slug}/assets/{encoded_unicode} HTTP/1.1\r\nHost: {host_port}\r\nRange: BYTES=-3\r\n\r\n"
            ),
        );
        assert!(suffix_range_hdr.starts_with("HTTP/1.1 206 Partial Content"));
        assert!(suffix_range_hdr.contains("Content-Range: bytes 7-9/10"));
        assert_eq!(suffix_range_body, b"789");
        assert_eq!(
            parse_http_byte_range("4294967296-4294967299", 4_294_967_301),
            Some((4_294_967_296, 4_294_967_299)),
            "large media ranges must not be limited by in-memory buffer indexes"
        );

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
            layer: None,
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
                layer: None,
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

    #[test]
    fn t356_preview_http_output_ingestion_trace_error_navigation_and_payload_limits() {
        use crate::output_pipeline::{OutputSeverity, OutputSource};
        use crate::panels::actions_sidebar::ScriptLocation;
        use std::io::{Read as _, Write as _};
        use std::sync::{Arc, atomic::AtomicUsize};

        fn raw_http_request(addr: std::net::SocketAddr, request: &[u8]) -> (String, Vec<u8>) {
            let mut stream = std::net::TcpStream::connect(addr).expect("connect preview http");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream.write_all(request).unwrap();
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

        let mut project = Project::blank();
        project.script_language = Some(neoflash_core::ScriptLanguage::ActionScript2);
        let root_id = project.root.clone();
        let frame_script_source = "stop();\ntrace(\"init\");\nvar ticks = 0;\nthis.onEnterFrame = function() {\n    boom();\n};\n";
        project.symbol_mut(&root_id).unwrap().timeline.scripts = vec![neoflash_core::FrameScript {
            layer: None,
            frame: 0,
            script: neoflash_core::Script {
                language: neoflash_core::ScriptLanguage::ActionScript2,
                source: frame_script_source.into(),
            },
        }];
        project.sources.push(neoflash_core::SourceUnit {
            id: "unit-game".into(),
            path: "src/Game.as".into(),
            language: neoflash_core::ScriptLanguage::ActionScript2,
            source: "class Game {\n    function step():Void {\n        throw new Error(\"game-boom\");\n    }\n}\n".into(),
        });

        let ctx = eframe::egui::Context::default();
        let repaint_calls = Arc::new(AtomicUsize::new(0));
        let repaint_counter = Arc::clone(&repaint_calls);
        ctx.set_request_repaint_callback(move |_info| {
            repaint_counter.fetch_add(1, Ordering::SeqCst);
        });

        let mut editor = Editor::from_project(&ctx, project);
        let mut server = PreviewHttpServer::start().expect("start preview server");
        server.set_repaint_context(ctx.clone());
        let addr = server.local_addr();
        let host = format!("127.0.0.1:{}", addr.port());

        // 1. OPTIONS preflight succeeds, GET on /__neoflash/output returns 405, missing Content-Length returns 411,
        //    malformed JSON returns 400, and oversized payload (>256 KiB) returns 413.
        let (opt_hdr, opt_body) = raw_http_request(
            addr,
            format!("OPTIONS /__neoflash/output HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes(),
        );
        assert!(opt_hdr.starts_with("HTTP/1.1 204 No Content"), "{opt_hdr}");
        assert!(opt_hdr.contains("Allow: POST, OPTIONS"), "{opt_hdr}");
        assert!(opt_body.is_empty());

        let (get_hdr, _) = raw_http_request(
            addr,
            format!("GET /__neoflash/output HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes(),
        );
        assert!(
            get_hdr.starts_with("HTTP/1.1 405 Method Not Allowed"),
            "{get_hdr}"
        );

        let (no_len_hdr, _) = raw_http_request(
            addr,
            format!("POST /__neoflash/output HTTP/1.1\r\nHost: {host}\r\n\r\n{{}}").as_bytes(),
        );
        assert!(
            no_len_hdr.starts_with("HTTP/1.1 411 Length Required"),
            "{no_len_hdr}"
        );

        let bad_json = br#"{"session":"s1","events":"not-an-array"}"#;
        let mut bad_req = format!(
            "POST /__neoflash/output HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            bad_json.len()
        )
        .into_bytes();
        bad_req.extend_from_slice(bad_json);
        let (bad_hdr, _) = raw_http_request(addr, &bad_req);
        assert!(bad_hdr.starts_with("HTTP/1.1 400 Bad Request"), "{bad_hdr}");

        let over_len = MAX_PREVIEW_OUTPUT_BODY_BYTES + 1;
        let (over_hdr, _) = raw_http_request(
            addr,
            format!(
                "POST /__neoflash/output HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {over_len}\r\n\r\n"
            )
            .as_bytes(),
        );
        assert!(
            over_hdr.starts_with("HTTP/1.1 413 Payload Too Large"),
            "{over_hdr}"
        );
        assert!(server.drain_output_items().is_empty());

        // 2. Stale queued items are cleared before starting a fresh session.
        let stale_body =
            br#"{"session":"old","events":[{"seq":1,"kind":"trace","message":"stale"}]}"#;
        let mut stale_req = format!(
            "POST /__neoflash/output HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\n\r\n",
            stale_body.len()
        )
        .into_bytes();
        stale_req.extend_from_slice(stale_body);
        let (stale_hdr, _) = raw_http_request(addr, &stale_req);
        assert!(stale_hdr.starts_with("HTTP/1.1 204 No Content"));
        server.clear_output_queue();
        assert!(server.drain_output_items().is_empty());

        // 3. Valid live batch: Frame-1 trace, onEnterFrame traces, rate-cap suppression notice,
        //    Restart separator, and mapped Frame + SourceUnit runtime errors.
        //    Run egui's two-pass settle cycle (`outstanding = 1 -> 0`) so `repaint_delay` is
        //    `Duration::MAX` (matching NeoFlash parked idle in the background while browser runs).
        for _ in 0..2 {
            let _ = ctx.run(Default::default(), |_| {});
        }
        let before_repaints = repaint_calls.load(Ordering::SeqCst);
        let root_sym_name = editor.project.symbol(&root_id).unwrap().name.clone();
        let frame_origin = format!("{root_sym_name} / frame 1");
        let batch_json = serde_json::json!({
            "session": "live-1",
            "events": [
                {
                    "seq": 1,
                    "kind": "trace",
                    "message": "init",
                    "sourcePath": frame_origin,
                    "line": 2,
                    "column": 1
                },
                {
                    "seq": 2,
                    "kind": "trace",
                    "message": "tick 1",
                    "sourcePath": frame_origin,
                    "line": 4,
                    "column": 5
                },
                {
                    "seq": 2,
                    "kind": "trace",
                    "message": "duplicate-seq-ignored"
                },
                {
                    "seq": 3,
                    "kind": "trace",
                    "message": "[trace] ... suppressed 42 lines ..."
                },
                {
                    "seq": 4,
                    "kind": "restart",
                    "message": "Player restarted"
                },
                {
                    "seq": 5,
                    "kind": "error",
                    "message": format!("{frame_origin}: ReferenceError: boom is not defined"),
                    "sourcePath": frame_origin,
                    "line": 5,
                    "column": 5
                },
                {
                    "seq": 6,
                    "kind": "error",
                    "message": "Error: game-boom",
                    "sourcePath": "src/Game.as",
                    "line": 3,
                    "column": 9
                }
            ]
        })
        .to_string();
        let mut live_req = format!(
            "POST /m1/__neoflash/output HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            batch_json.len()
        )
        .into_bytes();
        live_req.extend_from_slice(batch_json.as_bytes());
        let (live_hdr, _) = raw_http_request(addr, &live_req);
        assert!(
            live_hdr.starts_with("HTTP/1.1 204 No Content"),
            "{live_hdr}"
        );
        assert!(
            repaint_calls.load(Ordering::SeqCst) > before_repaints,
            "POST /__neoflash/output must wake egui::Context::request_repaint() immediately"
        );

        editor.output.clear();
        let drained = server.drain_output_items();
        assert_eq!(drained.len(), 6, "duplicate seq=2 must be deduplicated");
        editor.ingest_preview_output_items(Some(&editor.clipboard.session.clone()), drained);

        let events = editor.output.events();
        assert_eq!(events.len(), 6);
        assert_eq!(events[0].severity, OutputSeverity::Trace);
        assert_eq!(events[0].source, OutputSource::Trace);
        assert_eq!(events[0].message, "[trace] init");
        assert!(!events[0].message.starts_with("[trace] [trace]"));

        assert_eq!(events[1].message, "[trace] tick 1");
        assert_eq!(
            events[2].message, "[trace] ... suppressed 42 lines ...",
            "pre-prefixed suppression notice must NEVER double-prefix [trace] [trace]"
        );
        assert_eq!(events[3].severity, OutputSeverity::Info);
        assert!(events[3].message.contains("restarted"));

        assert_eq!(events[4].severity, OutputSeverity::Error);
        assert_eq!(events[4].source, OutputSource::Runtime);
        assert_eq!(
            events[4].message,
            format!(
                "[Error] {root_sym_name} · Layer 1 · Frame 1, Line 5: ReferenceError: boom is not defined\n  > boom();"
            )
        );
        let frame_err_loc = events[4]
            .location
            .clone()
            .expect("mapped frame error location");
        assert!(frame_err_loc.is_navigable());
        assert_eq!(frame_err_loc.line, Some(5));
        assert_eq!(frame_err_loc.column, Some(5));

        assert_eq!(events[5].severity, OutputSeverity::Error);
        assert_eq!(
            events[5].message,
            "[Error] src/Game.as, Line 3: Error: game-boom\n  > throw new Error(\"game-boom\");"
        );
        let source_err_loc = events[5]
            .location
            .clone()
            .expect("mapped SourceUnit location");
        assert!(source_err_loc.is_navigable());
        assert_eq!(source_err_loc.line, Some(3));
        assert_eq!(source_err_loc.column, Some(9));

        // 4. Clicking the frame-script error in Output navigates Actions directly to line 5, col 5.
        assert!(editor.navigate_to_output_location(&ctx, &frame_err_loc));
        assert!(matches!(
            editor.current_script_location(),
            Some(ScriptLocation::Frame { frame: 0, .. })
        ));
        let frame_editor_id = editor.actions_editor_id();
        let frame_cursor = eframe::egui::text_edit::TextEditState::load(&ctx, frame_editor_id)
            .and_then(|s| s.cursor.char_range())
            .expect("caret placed in frame Actions editor");
        let expected_frame_idx =
            neoflash_ui::components::code::char_index(frame_script_source, 5, 5);
        assert_eq!(frame_cursor.primary.index, expected_frame_idx);

        // 5. Clicking the SourceUnit error in Output navigates Actions directly to src/Game.as line 3, col 9.
        assert!(editor.navigate_to_output_location(&ctx, &source_err_loc));
        assert_eq!(
            editor.current_script_location(),
            Some(ScriptLocation::Source("unit-game".into()))
        );
        let src_editor_id = editor.actions_editor_id();
        let src_cursor = eframe::egui::text_edit::TextEditState::load(&ctx, src_editor_id)
            .and_then(|s| s.cursor.char_range())
            .expect("caret placed in SourceUnit Actions editor");
        let expected_src_idx =
            neoflash_ui::components::code::char_index(&editor.project.sources[0].source, 3, 9);
        assert_eq!(src_cursor.primary.index, expected_src_idx);

        server.shutdown();
    }

    #[test]
    fn t356_one_frame_game_in_editor_play_executes_frame1_and_enter_frame_traces() {
        // 1. 1-frame AS2 game with `stop(); trace("init"); this.onEnterFrame = function() { ... };`
        let mut as2_game = Project::blank();
        as2_game.script_language = Some(neoflash_core::ScriptLanguage::ActionScript2);
        let root_id = as2_game.root.clone();
        as2_game.symbol_mut(&root_id).unwrap().timeline.frame_count = 1;
        as2_game.symbol_mut(&root_id).unwrap().timeline.scripts = vec![neoflash_core::FrameScript {
            layer: None,
            frame: 0,
            script: neoflash_core::Script {
                language: neoflash_core::ScriptLanguage::ActionScript2,
                source: "stop();\nvar n = 0;\ntrace(\"init\");\nthis.onEnterFrame = function() {\n    n = n + 1;\n    trace(\"tick \" + n);\n};\n".into(),
            },
        }];

        let ctx = eframe::egui::Context::default();
        let mut editor = Editor::from_project(&ctx, as2_game);
        editor.workspace.timeline.loop_playback = false;
        editor.output.clear();

        editor.set_playing(true);
        assert!(
            editor.playing,
            "1-frame game with stop() + onEnterFrame must keep transport active for enter_frame ticks"
        );
        assert_eq!(
            editor
                .output
                .events()
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>(),
            vec!["[trace] init"]
        );

        for _ in 0..3 {
            editor.tick_authored_preview();
        }
        assert!(editor.playing);
        assert_eq!(editor.frame, 0);
        assert_eq!(
            editor
                .output
                .events()
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>(),
            vec![
                "[trace] init",
                "[trace] tick 1",
                "[trace] tick 2",
                "[trace] tick 3",
            ]
        );

        // 2. 1-frame AS3 game with `stop(); trace("as3-init"); addEventListener(Event.ENTER_FRAME, ...);`
        let mut as3_game = Project::blank();
        as3_game.script_language = Some(neoflash_core::ScriptLanguage::ActionScript3);
        let as3_root = as3_game.root.clone();
        as3_game.symbol_mut(&as3_root).unwrap().timeline.frame_count = 1;
        as3_game.symbol_mut(&as3_root).unwrap().timeline.scripts = vec![neoflash_core::FrameScript {
            layer: None,
            frame: 0,
            script: neoflash_core::Script {
                language: neoflash_core::ScriptLanguage::ActionScript3,
                source: "stop();\ntrace(\"as3-init\");\naddEventListener(Event.ENTER_FRAME, function(e:Event):void {\n    trace(\"as3-tick\");\n});\n".into(),
            },
        }];
        let mut as3_editor = Editor::from_project(&ctx, as3_game);
        as3_editor.output.clear();
        as3_editor.set_playing(true);
        as3_editor.tick_authored_preview();
        as3_editor.tick_authored_preview();
        assert_eq!(
            as3_editor
                .output
                .events()
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>(),
            vec!["[trace] as3-init", "[trace] as3-tick", "[trace] as3-tick"]
        );

        // 3. Pure 1-frame `stop(); trace("ready");` (without onEnterFrame): emits on Play #1, halts,
        //    and re-runs cleanly when the user presses Play (`Enter`) a second time.
        let mut one_shot = Project::blank();
        one_shot.script_language = Some(neoflash_core::ScriptLanguage::ActionScript2);
        let one_root = one_shot.root.clone();
        one_shot.symbol_mut(&one_root).unwrap().timeline.frame_count = 1;
        one_shot.symbol_mut(&one_root).unwrap().timeline.scripts =
            vec![neoflash_core::FrameScript {
                layer: None,
                frame: 0,
                script: neoflash_core::Script {
                    language: neoflash_core::ScriptLanguage::ActionScript2,
                    source: "stop();\ntrace(\"ready\");\n".into(),
                },
            }];
        let mut one_editor = Editor::from_project(&ctx, one_shot);
        one_editor.output.clear();
        one_editor.set_playing(true);
        assert!(!one_editor.playing);
        assert_eq!(
            one_editor
                .output
                .events()
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>(),
            vec!["[trace] ready"]
        );
        one_editor.set_playing(true);
        assert!(!one_editor.playing);
        assert_eq!(
            one_editor
                .output
                .events()
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>(),
            vec!["[trace] ready", "[trace] ready"],
            "pressing Play again after stop() on a 1-frame movie must re-execute Frame 1"
        );
    }
}
