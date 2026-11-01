//! Timeline (frame) sounds: the `Player` turns keyframe sounds into host commands when
//! frames are entered. Hosts (Web Audio, the native editor) only schedule decoded audio;
//! which sound starts, stops or seeks, and at which offset, is decided here.
//!
//! Flash semantics: Event plays on every entry and overlaps; Start plays unless an instance
//! of the same sound is still playing (a host fact); Stop stops every instance of the sound;
//! Stream is locked to its span on the layer — it starts at the playhead's offset into the
//! span, restarts after a jump, and stops when the span ends or that timeline stops.
use crate::evaluate::Node;
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// One instruction for the audio host, in the order the timeline produced them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SoundCommand {
    /// Event or Start sync. For Start the host skips the command while an instance of
    /// `resource` is still playing.
    Play {
        resource: Id,
        sync: SoundSync,
        /// Total number of plays; ignored when `loop_forever`.
        loops: u32,
        loop_forever: bool,
        /// `(position 0..=1 of the played length, left, right)` volume points.
        envelope: Vec<[f64; 3]>,
    },
    /// Stop every playing instance of `resource` (Stop sync).
    Stop { resource: Id },
    /// Start a timeline-locked stream on `channel` at `offset` seconds into the sound.
    StreamStart {
        channel: String,
        resource: Id,
        offset: f64,
        loops: u32,
        loop_forever: bool,
        envelope: Vec<[f64; 3]>,
    },
    /// Stop the stream on `channel`.
    StreamStop { channel: String },
    /// Stop every sound (Restart, `stopAllSounds()`, `SoundMixer.stopAll()`).
    StopAll,
}

#[derive(Default)]
pub(crate) struct SoundState {
    commands: Vec<SoundCommand>,
    /// Active stream channel -> last timeline frame it was synchronised with.
    streams: HashMap<String, u32>,
    started: bool,
}

/// Frame sounds are ignored on guide and folder layers, which are never published.
fn sound_layers(symbol: &Symbol) -> impl Iterator<Item = &Layer> {
    symbol
        .timeline
        .layers
        .iter()
        .filter(|layer| !matches!(layer.properties.role, LayerRole::Guide | LayerRole::Folder))
}

fn envelope(sound: &FrameSound) -> Vec<[f64; 3]> {
    sound
        .effect
        .envelope()
        .iter()
        .map(|&(position, left, right)| [position, left, right])
        .collect()
}

/// Left and right gain of a `(position, left, right)` envelope at `position` (0..=1 of the
/// played length), linear between points and held before the first and after the last.
pub fn envelope_gains(points: &[[f64; 3]], position: f64) -> (f64, f64) {
    let Some(first) = points.first() else {
        return (1.0, 1.0);
    };
    let position = position.clamp(0.0, 1.0);
    let mut previous = first;
    for point in points {
        if point[0] >= position {
            if point[0] <= previous[0] {
                return (point[1], point[2]);
            }
            let t = (position - previous[0]) / (point[0] - previous[0]);
            return (
                previous[1] + (point[1] - previous[1]) * t,
                previous[2] + (point[2] - previous[2]) * t,
            );
        }
        previous = point;
    }
    (previous[1], previous[2])
}

/// Audio resources attached to any keyframe, for hosts that decode ahead of playback.
pub fn timeline_sound_resources(project: &Project) -> Vec<Id> {
    let mut ids: Vec<Id> = project
        .symbols
        .iter()
        .flat_map(|symbol| &symbol.timeline.layers)
        .flat_map(|layer| &layer.keyframes)
        .filter_map(|key| key.sound.as_ref())
        .filter(|sound| sound.sync != SoundSync::Stop)
        .map(|sound| sound.resource.clone())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Exclusive end frame of the span that starts at `key` on `layer`.
pub fn sound_span_end(layer: &Layer, key: &Keyframe, frame_count: u32) -> u32 {
    let next = layer
        .keyframes
        .iter()
        .filter(|k| k.frame > key.frame)
        .map(|k| k.frame)
        .min()
        .unwrap_or(frame_count);
    next.min(layer.properties.end_frame.unwrap_or(frame_count))
        .min(frame_count)
}

impl SoundState {
    fn push(&mut self, command: SoundCommand) {
        self.started = true;
        self.commands.push(command);
    }

    /// Event/Start/Stop sounds of the keyframes that begin exactly at `frame`.
    pub(crate) fn frame_entered(&mut self, symbol: &Symbol, frame: u32) {
        for layer in sound_layers(symbol) {
            let Some(key) = layer.key_at(frame).filter(|key| key.frame == frame) else {
                continue;
            };
            let Some(sound) = &key.sound else { continue };
            match sound.sync {
                SoundSync::Event | SoundSync::Start => self.push(SoundCommand::Play {
                    resource: sound.resource.clone(),
                    sync: sound.sync,
                    loops: sound.loops.max(1),
                    loop_forever: sound.loop_forever,
                    envelope: envelope(sound),
                }),
                SoundSync::Stop => self.push(SoundCommand::Stop {
                    resource: sound.resource.clone(),
                }),
                SoundSync::Stream => {}
            }
        }
    }

    /// Reconcile stream channels with the placed timelines. `running(path)` says whether the
    /// clock driving that timeline advances.
    pub(crate) fn sync_streams<'a>(
        &mut self,
        project: &Project,
        nodes: impl Iterator<Item = &'a Node>,
        running: impl Fn(&[Id]) -> bool,
    ) {
        let fps = f64::from(project.fps.max(1));
        let mut alive = HashSet::new();
        for node in nodes {
            let Some(symbol) = node.symbol_id.as_ref().and_then(|id| project.symbol(id)) else {
                continue;
            };
            if !running(&node.path) {
                continue;
            }
            for layer in sound_layers(symbol) {
                let Some(key) = layer.key_at(node.frame) else {
                    continue;
                };
                let Some(sound) = key.sound.as_ref().filter(|s| s.sync == SoundSync::Stream) else {
                    continue;
                };
                if node.frame >= sound_span_end(layer, key, symbol.timeline.frame_count) {
                    continue;
                }
                let channel =
                    serde_json::to_string(&(&node.path, &layer.id, key.frame)).unwrap_or_default();
                let continuous = self
                    .streams
                    .get(&channel)
                    .is_some_and(|&last| node.frame == last || node.frame == last + 1);
                if !continuous {
                    if self.streams.contains_key(&channel) {
                        self.push(SoundCommand::StreamStop {
                            channel: channel.clone(),
                        });
                    }
                    self.push(SoundCommand::StreamStart {
                        channel: channel.clone(),
                        resource: sound.resource.clone(),
                        offset: f64::from(node.frame - key.frame) / fps,
                        loops: sound.loops.max(1),
                        loop_forever: sound.loop_forever,
                        envelope: envelope(sound),
                    });
                }
                self.streams.insert(channel.clone(), node.frame);
                alive.insert(channel);
            }
        }
        let mut ended: Vec<_> = self
            .streams
            .keys()
            .filter(|channel| !alive.contains(*channel))
            .cloned()
            .collect();
        ended.sort();
        for channel in ended {
            self.streams.remove(&channel);
            self.push(SoundCommand::StreamStop { channel });
        }
    }

    /// Stop the streams only (a seek restarts them at the new offset).
    pub(crate) fn stop_streams(&mut self) {
        let mut channels: Vec<_> = self.streams.drain().map(|(channel, _)| channel).collect();
        channels.sort();
        for channel in channels {
            self.push(SoundCommand::StreamStop { channel });
        }
    }

    /// Stop everything. Streams resume at their offset on the next synchronised frame.
    pub(crate) fn stop_all(&mut self) {
        self.streams.clear();
        self.push(SoundCommand::StopAll);
    }

    /// Restart: forget pending commands; tell the host to silence what an earlier session started.
    pub(crate) fn reset(&mut self) {
        let started = self.started;
        *self = Self::default();
        if started {
            self.push(SoundCommand::StopAll);
        }
    }

    pub(crate) fn drain(&mut self) -> Vec<SoundCommand> {
        std::mem::take(&mut self.commands)
    }
}

impl SoundCommand {
    /// Namespace a stream channel of a runtime-created (linked) clip's own timeline.
    pub(crate) fn scoped(self, scope: &str) -> Self {
        match self {
            Self::StreamStart {
                channel,
                resource,
                offset,
                loops,
                loop_forever,
                envelope,
            } => Self::StreamStart {
                channel: format!("{scope}:{channel}"),
                resource,
                offset,
                loops,
                loop_forever,
                envelope,
            },
            Self::StreamStop { channel } => Self::StreamStop {
                channel: format!("{scope}:{channel}"),
            },
            other => other,
        }
    }
}
