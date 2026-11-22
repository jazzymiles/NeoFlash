//! Resolve SWF audio controls against the same authored resources as the editable importer.
use super::*;
use crate::swf::media::{SwfSoundAsset, parse_sound_stream_head, read_sound_info};
use neoflash_core::FrameSound;

#[derive(Clone, Debug)]
pub(super) enum TimelineSoundCue {
    Event(u16, FrameSound),
    Header,
    Block { sequence: u32, starts: u32 },
    Diagnostic(String),
}

#[derive(Clone, Debug)]
pub(super) enum ButtonSoundCue {
    None,
    Event(u16, FrameSound),
    Diagnostic(String),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ActiveClipStream {
    pub sprite_id: u16,
    pub sequence: u32,
    pub frame: u32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct LiveClipStreamState {
    pub instance_id: u64,
    pub sprite_id: u16,
    pub sequence: u32,
    pub starts: u32,
    pub frame: u32,
    pub looped: bool,
}

#[derive(Clone, Debug)]
pub(super) enum PendingAudio {
    Command(SoundCommand),
    Stream {
        sequence: u32,
        starts: u32,
        frame: u32,
    },
    ClipStreams(Vec<LiveClipStreamState>),
}

pub(super) fn define_sound_asset(data: &[u8]) -> Option<(u16, SwfSoundAsset)> {
    let mut r = SwfSlice::new(data);
    let id = r.u16()?;
    let rate = (r.u8()? >> 2) & 3;
    let duration_44 = u64::from(r.u32()?) << (3 - rate);
    Some((
        id,
        SwfSoundAsset {
            resource_id: format!("swf-sound-{id}"),
            duration_44,
        },
    ))
}

fn decode_event_sound(
    scope: &str,
    tag: &RawFrameTag,
    frame: u32,
    assets: &HashMap<u16, SwfSoundAsset>,
    classes: &HashMap<&str, u16>,
    output: &mut Vec<TimelineSoundCue>,
) {
    let mut reader = SwfSlice::new(&tag.payload);
    let at = if scope == "root" {
        format!("#tag={}:frame={frame}", tag.code)
    } else {
        format!("#{scope}:tag={}:frame={frame}", tag.code)
    };
    let id = if tag.code == 89 {
        let Some((name, _)) = reader.c_string(9) else {
            output.push(TimelineSoundCue::Diagnostic(format!(
                "{at}: incomplete StartSound2 class name"
            )));
            return;
        };
        let Some(id) = classes.get(name.as_str()) else {
            output.push(TimelineSoundCue::Diagnostic(format!(
                "{at}: StartSound2 class '{name}' has no embedded sound"
            )));
            return;
        };
        *id
    } else if let Some(id) = reader.u16() {
        id
    } else {
        output.push(TimelineSoundCue::Diagnostic(format!(
            "{at}: incomplete StartSound character ID"
        )));
        return;
    };
    // Inspection can still report the authored event when its resource is unavailable.
    let missing = SwfSoundAsset {
        resource_id: format!("swf-sound-{id}"),
        duration_44: 0,
    };
    let sound = read_sound_info(&mut reader, assets.get(&id).unwrap_or(&missing), |msg| {
        output.push(TimelineSoundCue::Diagnostic(format!(
            "{at}:sound={id}: {msg}"
        )));
    });
    output.push(match sound {
        Some(sound) => TimelineSoundCue::Event(id, sound),
        None => TimelineSoundCue::Diagnostic(format!("{at}:sound={id}: incomplete SOUNDINFO")),
    });
}

pub(super) fn decode_timeline_audio(
    scope: &str,
    frames: &mut [Vec<RawFrameTag>],
    assets: &HashMap<u16, SwfSoundAsset>,
    classes: &HashMap<&str, u16>,
) -> BTreeMap<u32, Vec<TimelineSoundCue>> {
    let mut cues = BTreeMap::new();
    let mut sequence = 0;
    let mut stream = None;
    for (index, tags) in frames.iter_mut().enumerate() {
        let frame = index as u32;
        tags.retain(|tag| {
            match tag.code {
                15 | 89 => {
                    decode_event_sound(
                        scope,
                        tag,
                        frame,
                        assets,
                        classes,
                        cues.entry(frame).or_insert_with(Vec::new),
                    );
                }
                18 | 45 => {
                    stream = parse_sound_stream_head(&tag.payload).map(|_| {
                        sequence += 1;
                        None
                    });
                    cues.entry(frame)
                        .or_insert_with(Vec::new)
                        .push(TimelineSoundCue::Header);
                }
                19 => {
                    if let Some(first) = stream.as_mut()
                        && !tag.payload.is_empty()
                    {
                        let starts = *first.get_or_insert(frame);
                        cues.entry(frame)
                            .or_insert_with(Vec::new)
                            .push(TimelineSoundCue::Block { sequence, starts });
                    }
                }
                _ => return true,
            }
            false
        });
    }
    cues
}

pub(super) fn decode_define_button_sound(
    payload: &[u8],
    assets: &HashMap<u16, SwfSoundAsset>,
) -> Option<(u16, [ButtonSoundCue; 4])> {
    let mut r = SwfSlice::new(payload);
    let button_id = r.u16()?;
    let mut slots = std::array::from_fn(|_| ButtonSoundCue::None);
    for (slot_idx, slot_out) in slots.iter_mut().enumerate() {
        let Some(sound_id) = r.u16() else {
            break;
        };
        if sound_id == 0 {
            continue;
        }
        let missing = SwfSoundAsset {
            resource_id: format!("swf-sound-{sound_id}"),
            duration_44: 0,
        };
        let mut diag = None;
        let parsed = read_sound_info(&mut r, assets.get(&sound_id).unwrap_or(&missing), |msg| {
            diag.get_or_insert_with(|| {
                format!("#tag=17:button={button_id}:slot={slot_idx}:sound={sound_id}: {msg}")
            });
        });
        match parsed {
            Some(fs) => {
                *slot_out = ButtonSoundCue::Event(sound_id, fs);
            }
            None => {
                *slot_out = ButtonSoundCue::Diagnostic(diag.unwrap_or_else(|| {
                    format!(
                        "#tag=17:button={button_id}:slot={slot_idx}:sound={sound_id}: incomplete SOUNDINFO"
                    )
                }));
                break;
            }
        }
    }
    Some((button_id, slots))
}

pub(super) fn button_sound_slot_for_cond_mask(btn_cond_mask: u16) -> Option<usize> {
    if (btn_cond_mask & 0x0001) != 0 {
        // IdleToOverUp (rollOver)
        Some(1)
    } else if (btn_cond_mask & 0x0002) != 0 {
        // OverUpToIdle (rollOut)
        Some(0)
    } else if (btn_cond_mask & 0x0004) != 0 {
        // OverUpToOverDown (press)
        Some(2)
    } else if (btn_cond_mask & 0x0008) != 0 {
        // OverDownToOverUp (release)
        Some(3)
    } else if (btn_cond_mask & (0x0020 | 0x0080)) != 0 {
        // OutDownToOverDown / IdleToOverDown (dragOver)
        Some(2)
    } else if (btn_cond_mask & (0x0010 | 0x0100 | 0x0040)) != 0 {
        // OverDownToOutDown / OverDownToIdle / OutDownToIdle (dragOut / releaseOutside)
        Some(0)
    } else {
        None
    }
}

fn collect_button_ids_preorder(display_list: &BTreeMap<u16, LiveSlot>, out: &mut Vec<u16>) {
    for slot in display_list.values() {
        out.push(slot.character_id);
        collect_button_ids_preorder(&slot.children, out);
    }
}

impl SwfBytecodePlayer {
    fn emit_event_sound(&mut self, frame: u32, id: u16, sound: FrameSound) {
        let event = SwfAudioEvent {
            frame,
            sound_id: id,
            sync: sound.sync.label().to_ascii_lowercase(),
            loops: if sound.loop_forever {
                0x7fff
            } else {
                sound.loops as u16
            },
        };
        let command = if sound.sync == SoundSync::Stop {
            SoundCommand::Stop {
                resource: sound.resource,
            }
        } else {
            SoundCommand::Play {
                envelope: sound.envelope_points(),
                resource: sound.resource,
                sync: sound.sync,
                loops: sound.loops,
                loop_forever: sound.loop_forever,
                in_point: sound.in_point,
                out_point: sound.out_point,
            }
        };
        self.frame_sound_commands
            .push(PendingAudio::Command(command));
        self.audio_events.push(event.clone());
        self.frame_audio_events.push(event);
    }

    pub(super) fn apply_root_audio(&mut self, frame: u32) {
        let Some(cues) = self.root_audio.get(&frame).cloned() else {
            return;
        };
        for cue in cues {
            match cue {
                TimelineSoundCue::Header => {
                    self.stream_sound_active = true;
                    self.frame_sound_commands.push(PendingAudio::Command(
                        SoundCommand::StreamStop {
                            channel: "swf-root-stream".to_owned(),
                        },
                    ));
                }
                TimelineSoundCue::Diagnostic(message) => {
                    self.record_unsupported_diagnostic(message);
                }
                TimelineSoundCue::Block { sequence, starts } => {
                    if !self.stream_sound_active {
                        continue;
                    }
                    self.frame_sound_commands.push(PendingAudio::Stream {
                        sequence,
                        starts,
                        frame,
                    });
                    let event = SwfAudioEvent {
                        frame,
                        sound_id: 0,
                        sync: "stream".to_owned(),
                        loops: 1,
                    };
                    self.audio_events.push(event.clone());
                    self.frame_audio_events.push(event);
                }
                TimelineSoundCue::Event(id, sound) => {
                    self.emit_event_sound(frame, id, sound);
                }
            }
        }
    }

    pub(super) fn flush_nested_event_audio(&mut self, root_frame: u32) {
        let mut events = Vec::new();
        let mut diagnostics = Vec::new();
        Self::drain_slot_event_audio_recursive(
            &mut self.display_list,
            &self.sprites,
            &mut events,
            &mut diagnostics,
        );
        for d in diagnostics {
            self.record_unsupported_diagnostic(d);
        }
        for (id, sound) in events {
            self.emit_event_sound(root_frame, id, sound);
        }
    }

    pub(super) fn collect_and_queue_nested_audio(&mut self, root_frame: u32) {
        let mut events = Vec::new();
        let mut diagnostics = Vec::new();
        let mut live_streams = Vec::new();
        Self::drain_slot_audio_and_streams_recursive(
            &mut self.display_list,
            &self.sprites,
            &mut events,
            &mut diagnostics,
            &mut live_streams,
        );
        for d in diagnostics {
            self.record_unsupported_diagnostic(d);
        }
        for (id, sound) in events {
            self.emit_event_sound(root_frame, id, sound);
        }
        for _ in &live_streams {
            let ev = SwfAudioEvent {
                frame: root_frame,
                sound_id: 0,
                sync: "stream".to_owned(),
                loops: 1,
            };
            self.audio_events.push(ev.clone());
            self.frame_audio_events.push(ev);
        }
        if !live_streams.is_empty() || !self.active_clip_streams.is_empty() {
            self.frame_sound_commands
                .push(PendingAudio::ClipStreams(live_streams));
        }
    }

    fn drain_slot_event_audio_recursive(
        display_list: &mut BTreeMap<u16, LiveSlot>,
        sprites: &HashMap<u16, TimelineDef>,
        events: &mut Vec<(u16, FrameSound)>,
        diagnostics: &mut Vec<String>,
    ) {
        for slot in display_list.values_mut() {
            let frames = std::mem::take(&mut slot.pending_audio_frames);
            if !frames.is_empty()
                && let Some(def) = sprites.get(&slot.character_id)
            {
                for f in frames {
                    if let Some(cues) = def.audio.get(&f) {
                        for cue in cues {
                            match cue {
                                TimelineSoundCue::Event(id, sound) => {
                                    events.push((*id, sound.clone()));
                                }
                                TimelineSoundCue::Diagnostic(msg) => {
                                    diagnostics.push(msg.clone());
                                }
                                TimelineSoundCue::Header | TimelineSoundCue::Block { .. } => {}
                            }
                        }
                    }
                }
            }
            Self::drain_slot_event_audio_recursive(
                &mut slot.children,
                sprites,
                events,
                diagnostics,
            );
        }
    }

    fn drain_slot_audio_and_streams_recursive(
        display_list: &mut BTreeMap<u16, LiveSlot>,
        sprites: &HashMap<u16, TimelineDef>,
        events: &mut Vec<(u16, FrameSound)>,
        diagnostics: &mut Vec<String>,
        live_streams: &mut Vec<LiveClipStreamState>,
    ) {
        for slot in display_list.values_mut() {
            let frames = std::mem::take(&mut slot.pending_audio_frames);
            let looped = std::mem::take(&mut slot.looped_this_step);
            let entered = std::mem::take(&mut slot.entered_this_step);
            if let Some(def) = sprites.get(&slot.character_id) {
                for f in frames {
                    if let Some(cues) = def.audio.get(&f) {
                        for cue in cues {
                            match cue {
                                TimelineSoundCue::Event(id, sound) => {
                                    events.push((*id, sound.clone()));
                                }
                                TimelineSoundCue::Diagnostic(msg) => {
                                    diagnostics.push(msg.clone());
                                }
                                TimelineSoundCue::Header | TimelineSoundCue::Block { .. } => {}
                            }
                        }
                    }
                }
                if slot.playing
                    && (slot.total_frames > 1 || entered)
                    && let Some(cues) = def.audio.get(&slot.current_frame)
                {
                    for cue in cues {
                        if let TimelineSoundCue::Block { sequence, starts } = *cue {
                            live_streams.push(LiveClipStreamState {
                                instance_id: slot.instance_id,
                                sprite_id: slot.character_id,
                                sequence,
                                starts,
                                frame: slot.current_frame,
                                looped,
                            });
                            break;
                        }
                    }
                }
            }
            Self::drain_slot_audio_and_streams_recursive(
                &mut slot.children,
                sprites,
                events,
                diagnostics,
                live_streams,
            );
        }
    }

    pub(super) fn trigger_button_sounds_for_pointer(&mut self, btn_cond_mask: u16) {
        let Some(slot_idx) = button_sound_slot_for_cond_mask(btn_cond_mask) else {
            return;
        };
        if self.button_sounds.is_empty() {
            return;
        }
        let mut char_ids = Vec::new();
        collect_button_ids_preorder(&self.display_list, &mut char_ids);
        let frame = self.presented_frame;
        for cid in char_ids {
            let Some(cue) = self.button_sounds.get(&cid).map(|s| s[slot_idx].clone()) else {
                continue;
            };
            match cue {
                ButtonSoundCue::None => {}
                ButtonSoundCue::Event(id, sound) => {
                    self.emit_event_sound(frame, id, sound);
                }
                ButtonSoundCue::Diagnostic(msg) => {
                    self.record_unsupported_diagnostic(msg);
                }
            }
        }
    }

    pub(super) fn stop_swf_sounds(&mut self) {
        self.stream_sound_active = false;
        self.stop_all_sounds_pending = true;
        self.frame_sound_commands
            .push(PendingAudio::Command(SoundCommand::StopAll));
    }

    /// Drain the latest frame’s sound commands against resources available in the imported project.
    pub fn drain_sound_commands(&mut self, project: &Project) -> Vec<SoundCommand> {
        let mut out = Vec::new();
        let fps = self.frame_rate.max(1.0 / 256.0);
        if std::mem::take(&mut self.stop_all_sounds_pending)
            && !self
                .frame_sound_commands
                .iter()
                .any(|c| matches!(c, PendingAudio::Command(SoundCommand::StopAll)))
        {
            out.push(SoundCommand::StopAll);
            self.last_stream_frame = None;
            self.active_clip_streams.clear();
        }
        let pending_list = std::mem::take(&mut self.frame_sound_commands);
        for pending in pending_list {
            match pending {
                PendingAudio::Command(SoundCommand::StopAll) => {
                    self.last_stream_frame = None;
                    self.active_clip_streams.clear();
                    out.push(SoundCommand::StopAll);
                }
                PendingAudio::Command(SoundCommand::StreamStop { channel })
                    if channel == "swf-root-stream" =>
                {
                    if self.last_stream_frame.take().is_some() {
                        out.push(SoundCommand::StreamStop { channel });
                    }
                }
                PendingAudio::Command(command) => {
                    if let SoundCommand::Play { resource, .. }
                    | SoundCommand::StreamStart { resource, .. }
                    | SoundCommand::Stop { resource } = &command
                        && !project.resources.iter().any(|r| r.id == *resource)
                    {
                        continue;
                    }
                    out.push(command);
                }
                PendingAudio::Stream {
                    sequence,
                    starts,
                    frame,
                } => {
                    let resource = format!("swf-stream-root-{sequence}");
                    if !project.resources.iter().any(|r| r.id == resource) {
                        continue;
                    }
                    let consecutive = self.last_stream_frame.is_some_and(|(prev_seq, prev)| {
                        prev_seq == sequence && prev.checked_add(1) == Some(frame)
                    });
                    self.last_stream_frame = Some((sequence, frame));
                    if consecutive {
                        continue;
                    }
                    out.push(SoundCommand::StreamStart {
                        channel: "swf-root-stream".to_owned(),
                        resource,
                        offset: f64::from(frame.saturating_sub(starts)) / fps,
                        loops: 1,
                        loop_forever: false,
                        envelope: Vec::new(),
                        in_point: None,
                        out_point: None,
                    });
                }
                PendingAudio::ClipStreams(live_streams) => {
                    let live_ids: BTreeSet<u64> =
                        live_streams.iter().map(|s| s.instance_id).collect();
                    let stopped_ids: Vec<u64> = self
                        .active_clip_streams
                        .keys()
                        .copied()
                        .filter(|id| !live_ids.contains(id))
                        .collect();
                    for id in stopped_ids {
                        self.active_clip_streams.remove(&id);
                        out.push(SoundCommand::StreamStop {
                            channel: format!("swf-clip-stream-{id}"),
                        });
                    }
                    for live in live_streams {
                        let resource =
                            format!("swf-stream-sym-{}-{}", live.sprite_id, live.sequence);
                        if !project.resources.iter().any(|r| r.id == resource) {
                            if self.active_clip_streams.remove(&live.instance_id).is_some() {
                                out.push(SoundCommand::StreamStop {
                                    channel: format!("swf-clip-stream-{}", live.instance_id),
                                });
                            }
                            continue;
                        }
                        let channel = format!("swf-clip-stream-{}", live.instance_id);
                        if let Some(prev) = self.active_clip_streams.get(&live.instance_id).copied()
                        {
                            let consecutive = prev.sprite_id == live.sprite_id
                                && prev.sequence == live.sequence
                                && prev.frame.checked_add(1) == Some(live.frame)
                                && !live.looped;
                            self.active_clip_streams.insert(
                                live.instance_id,
                                ActiveClipStream {
                                    sprite_id: live.sprite_id,
                                    sequence: live.sequence,
                                    frame: live.frame,
                                },
                            );
                            if consecutive {
                                continue;
                            }
                            out.push(SoundCommand::StreamStop {
                                channel: channel.clone(),
                            });
                        } else {
                            self.active_clip_streams.insert(
                                live.instance_id,
                                ActiveClipStream {
                                    sprite_id: live.sprite_id,
                                    sequence: live.sequence,
                                    frame: live.frame,
                                },
                            );
                        }
                        out.push(SoundCommand::StreamStart {
                            channel,
                            resource,
                            offset: f64::from(live.frame.saturating_sub(live.starts)) / fps,
                            loops: 1,
                            loop_forever: false,
                            envelope: Vec::new(),
                            in_point: None,
                            out_point: None,
                        });
                    }
                }
            }
        }
        self.frame_audio_events.clear();
        out
    }
}
