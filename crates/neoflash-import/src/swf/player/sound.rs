//! Resolve SWF audio controls against the same authored resources as the editable importer.
use super::*;
use crate::swf::media::{SwfSoundAsset, parse_sound_stream_head, read_sound_info};
use neoflash_core::FrameSound;

#[derive(Clone, Debug)]
pub(super) enum RootSoundCue {
    Event(u16, FrameSound),
    Header,
    Block { sequence: u32, starts: u32 },
    Diagnostic(String),
}

#[derive(Clone, Debug)]
pub(super) enum PendingAudio {
    Command(SoundCommand),
    Stream {
        sequence: u32,
        starts: u32,
        frame: u32,
    },
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
    tag: &RawFrameTag,
    frame: u32,
    assets: &HashMap<u16, SwfSoundAsset>,
    classes: &HashMap<&str, u16>,
    output: &mut Vec<RootSoundCue>,
) {
    let mut reader = SwfSlice::new(&tag.payload);
    let at = format!("#tag={}:frame={frame}", tag.code);
    let id = if tag.code == 89 {
        let Some((name, _)) = reader.c_string(9) else {
            output.push(RootSoundCue::Diagnostic(format!(
                "{at}: incomplete StartSound2 class name"
            )));
            return;
        };
        let Some(id) = classes.get(name.as_str()) else {
            output.push(RootSoundCue::Diagnostic(format!(
                "{at}: StartSound2 class '{name}' has no embedded sound"
            )));
            return;
        };
        *id
    } else if let Some(id) = reader.u16() {
        id
    } else {
        output.push(RootSoundCue::Diagnostic(format!(
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
        output.push(RootSoundCue::Diagnostic(format!("{at}:sound={id}: {msg}")));
    });
    output.push(match sound {
        Some(sound) => RootSoundCue::Event(id, sound),
        None => RootSoundCue::Diagnostic(format!("{at}:sound={id}: incomplete SOUNDINFO")),
    });
}

pub(super) fn decode_root_audio(
    frames: &mut [Vec<RawFrameTag>],
    assets: &HashMap<u16, SwfSoundAsset>,
    classes: &HashMap<&str, u16>,
) -> BTreeMap<u32, Vec<RootSoundCue>> {
    let mut cues = BTreeMap::new();
    let mut sequence = 0;
    let mut stream = None;
    for (index, tags) in frames.iter_mut().enumerate() {
        let frame = index as u32;
        tags.retain(|tag| {
            match tag.code {
                15 | 89 => {
                    decode_event_sound(
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
                        .push(RootSoundCue::Header);
                }
                19 => {
                    if let Some(first) = stream.as_mut()
                        && !tag.payload.is_empty()
                    {
                        let starts = *first.get_or_insert(frame);
                        cues.entry(frame)
                            .or_insert_with(Vec::new)
                            .push(RootSoundCue::Block { sequence, starts });
                    }
                }
                _ => return true,
            }
            false
        });
    }
    cues
}

impl SwfBytecodePlayer {
    pub(super) fn apply_root_audio(&mut self, frame: u32) {
        let Some(cues) = self.root_audio.get(&frame).cloned() else {
            return;
        };
        for cue in cues {
            let event = match cue {
                RootSoundCue::Header => {
                    self.stream_sound_active = true;
                    self.frame_sound_commands.push(PendingAudio::Command(
                        SoundCommand::StreamStop {
                            channel: "swf-root-stream".to_owned(),
                        },
                    ));
                    continue;
                }
                RootSoundCue::Diagnostic(message) => {
                    self.record_unsupported_diagnostic(message);
                    continue;
                }
                RootSoundCue::Block { sequence, starts } => {
                    if !self.stream_sound_active {
                        continue;
                    }
                    self.frame_sound_commands.push(PendingAudio::Stream {
                        sequence,
                        starts,
                        frame,
                    });
                    SwfAudioEvent {
                        frame,
                        sound_id: 0,
                        sync: "stream".to_owned(),
                        loops: 1,
                    }
                }
                RootSoundCue::Event(id, sound) => {
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
                    event
                }
            };
            self.audio_events.push(event.clone());
            self.frame_audio_events.push(event);
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
        if std::mem::take(&mut self.stop_all_sounds_pending)
            && !self
                .frame_sound_commands
                .iter()
                .any(|c| matches!(c, PendingAudio::Command(SoundCommand::StopAll)))
        {
            out.push(SoundCommand::StopAll);
            self.last_stream_frame = None;
        }
        for pending in self.frame_sound_commands.drain(..) {
            let command = match pending {
                PendingAudio::Command(SoundCommand::StopAll) => {
                    self.last_stream_frame = None;
                    SoundCommand::StopAll
                }
                PendingAudio::Command(command @ SoundCommand::StreamStop { .. }) => {
                    if self.last_stream_frame.take().is_none() {
                        continue;
                    }
                    command
                }
                PendingAudio::Command(command) => command,
                PendingAudio::Stream {
                    sequence,
                    starts,
                    frame,
                } => {
                    let consecutive = self.last_stream_frame.is_some_and(|(prev_seq, prev)| {
                        prev_seq == sequence && prev.checked_add(1) == Some(frame)
                    });
                    self.last_stream_frame = Some((sequence, frame));
                    if consecutive {
                        continue;
                    }
                    SoundCommand::StreamStart {
                        channel: "swf-root-stream".to_owned(),
                        resource: format!("swf-stream-root-{sequence}"),
                        offset: f64::from(frame - starts) / self.frame_rate.max(1.0 / 256.0),
                        loops: 1,
                        loop_forever: false,
                        envelope: Vec::new(),
                        in_point: None,
                        out_point: None,
                    }
                }
            };
            if let SoundCommand::Play { resource, .. }
            | SoundCommand::StreamStart { resource, .. }
            | SoundCommand::Stop { resource } = &command
                && !project.resources.iter().any(|r| r.id == *resource)
            {
                continue;
            }
            out.push(command);
        }
        self.frame_audio_events.clear();
        out
    }
}
