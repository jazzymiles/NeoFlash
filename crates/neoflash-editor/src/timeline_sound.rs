//! Timeline (frame) sounds in the editor: attaching library audio to keyframes (Timeline
//! controls ▸ Sound, Properties ▸ Sound) and the native preview of editor playback.
//!
//! What starts, stops or seeks is decided by the shared `Player` (`SoundCommand`, the same path
//! as HTML5). This module only decodes each resource once (rodio / Symphonia: MP3, WAV) and mixes
//! voices to the default Core Audio output. Scrubbing is silent; Test Movie plays in the browser.
use crate::app::Editor;
use neoflash_core::*;
use rodio::Source as _;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Longest decoded preview of one resource: ten minutes of 48 kHz stereo.
const MAX_DECODED_SAMPLES: usize = 48_000 * 2 * 600;
/// Commands kept for diagnostics and tests.
const HISTORY: usize = 256;
/// Channel of Properties ▸ Sound ▸ Play.
const AUDITION: &str = "properties-play";

/// Decoded PCM of one resource, interleaved.
pub(crate) struct DecodedSound {
    samples: Arc<[f32]>,
    channels: u16,
    rate: u32,
}

impl DecodedSound {
    fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }
    pub(crate) fn duration(&self) -> f64 {
        self.frames() as f64 / f64::from(self.rate.max(1))
    }
    pub(crate) fn rate(&self) -> u32 {
        self.rate
    }
    pub(crate) fn channels(&self) -> u16 {
        self.channels
    }
}

fn decode(resource: &ResourceUnit) -> Result<DecodedSound, String> {
    let bytes = resource
        .decoded_bytes()
        .map_err(|error| error.to_string())?;
    let decoder = rodio::Decoder::new(std::io::Cursor::new(bytes))
        .map_err(|error| format!("{}: {error}", resource.path))?;
    let (channels, rate) = (decoder.channels(), decoder.sample_rate());
    let samples: Vec<f32> = decoder.take(MAX_DECODED_SAMPLES).collect();
    if samples.is_empty() || channels == 0 || rate == 0 {
        return Err(format!("{}: no decodable audio", resource.path));
    }
    Ok(DecodedSound {
        samples: samples.into(),
        channels,
        rate,
    })
}

/// One playing sound: stereo output with the command's loop count and volume envelope.
struct VoiceSource {
    sound: Arc<DecodedSound>,
    frame: usize,
    end: Option<usize>,
    envelope: Vec<[f64; 3]>,
    envelope_frames: f64,
    right: Option<f32>,
}

impl VoiceSource {
    fn new(
        sound: Arc<DecodedSound>,
        offset: f64,
        loops: u32,
        loop_forever: bool,
        envelope: Vec<[f64; 3]>,
    ) -> Option<Self> {
        let frames = sound.frames();
        let end = (!loop_forever).then(|| frames.saturating_mul(loops.max(1) as usize));
        let frame = (offset.max(0.) * f64::from(sound.rate)) as usize;
        if frames == 0 || end.is_some_and(|end| frame >= end) {
            return None;
        }
        let flat = envelope.iter().all(|p| p[1] == 1.0 && p[2] == 1.0);
        Some(Self {
            envelope_frames: end.unwrap_or(frames) as f64,
            envelope: if flat { vec![] } else { envelope },
            sound,
            frame,
            end,
            right: None,
        })
    }
}

impl Iterator for VoiceSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.right.take() {
            return Some(right);
        }
        if self.end.is_some_and(|end| self.frame >= end) {
            return None;
        }
        let channels = usize::from(self.sound.channels);
        let index = (self.frame % self.sound.frames()) * channels;
        let left = self.sound.samples[index];
        let right = if channels > 1 {
            self.sound.samples[index + 1]
        } else {
            left
        };
        let (gain_left, gain_right) = if self.envelope.is_empty() {
            (1.0, 1.0)
        } else {
            envelope_gains(&self.envelope, self.frame as f64 / self.envelope_frames)
        };
        self.frame += 1;
        self.right = Some(right * gain_right as f32);
        Some(left * gain_left as f32)
    }
}

impl rodio::Source for VoiceSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        self.sound.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

struct Voice {
    sink: rodio::Sink,
    resource: Id,
    /// Stream channel (or the Properties audition); `None` for Event/Start sounds.
    channel: Option<String>,
}

impl Drop for Voice {
    fn drop(&mut self) {
        self.sink.stop();
    }
}

/// Editor audio: the output device (opened on first use), decoded resources and voices.
pub(crate) struct TimelineAudio {
    /// Control ▸ Mute sounds.
    pub(crate) muted: bool,
    native: bool,
    output: Option<rodio::OutputStream>,
    failure: Option<String>,
    decoded: HashMap<Id, (usize, Result<Arc<DecodedSound>, String>)>,
    voices: Vec<Voice>,
    preview: Option<u64>,
    /// Recent commands of editor playback (diagnostics and tests).
    pub(crate) history: Vec<SoundCommand>,
}

impl Default for TimelineAudio {
    fn default() -> Self {
        Self {
            muted: false,
            // Tests and screenshot captures never open an audio device.
            native: !cfg!(test) && std::env::var_os("NEOFLASH_SCREENSHOT").is_none(),
            output: None,
            failure: None,
            decoded: HashMap::new(),
            voices: vec![],
            preview: None,
            history: vec![],
        }
    }
}

impl TimelineAudio {
    /// Decoded audio of `resource`, cached until its bytes change.
    pub(crate) fn decoded(
        &mut self,
        project: &Project,
        resource: &str,
    ) -> Result<Arc<DecodedSound>, String> {
        let unit = project
            .resources
            .iter()
            .find(|unit| unit.id == resource)
            .ok_or_else(|| format!("sound resource {resource} is missing"))?;
        let identity = Arc::as_ptr(&unit.data_base64).cast::<u8>() as usize;
        if let Some((cached, result)) = self.decoded.get(resource)
            && *cached == identity
        {
            return result.clone();
        }
        let result = decode(unit).map(Arc::new);
        self.decoded
            .insert(resource.to_owned(), (identity, result.clone()));
        result
    }

    fn mixer(&mut self) -> Option<&rodio::mixer::Mixer> {
        if !self.native || self.failure.is_some() {
            return None;
        }
        if self.output.is_none() {
            match rodio::OutputStreamBuilder::open_default_stream() {
                Ok(mut stream) => {
                    stream.log_on_drop(false);
                    self.output = Some(stream);
                }
                Err(error) => {
                    self.failure = Some(format!("No audio output: {error}"));
                    return None;
                }
            }
        }
        self.output.as_ref().map(|stream| stream.mixer())
    }

    fn start(
        &mut self,
        project: &Project,
        resource: Id,
        channel: Option<String>,
        offset: f64,
        (loops, loop_forever): (u32, bool),
        envelope: Vec<[f64; 3]>,
    ) {
        if self.muted || !self.native {
            return;
        }
        let sound = match self.decoded(project, &resource) {
            Ok(sound) => sound,
            Err(error) => {
                self.failure.get_or_insert(error);
                return;
            }
        };
        let Some(source) = VoiceSource::new(sound, offset, loops, loop_forever, envelope) else {
            return;
        };
        let Some(mixer) = self.mixer() else { return };
        let sink = rodio::Sink::connect_new(mixer);
        sink.append(source);
        self.voices.push(Voice {
            sink,
            resource,
            channel,
        });
    }

    /// Apply the playback's sound commands in order.
    pub(crate) fn apply(&mut self, project: &Project, commands: Vec<SoundCommand>) {
        self.voices.retain(|voice| !voice.sink.empty());
        for command in commands {
            self.history.push(command.clone());
            if self.history.len() > HISTORY {
                self.history.remove(0);
            }
            match command {
                SoundCommand::StopAll => self
                    .voices
                    .retain(|v| v.channel.as_deref() == Some(AUDITION)),
                SoundCommand::Stop { resource } => self
                    .voices
                    .retain(|v| v.resource != resource || v.channel.is_some()),
                SoundCommand::StreamStop { channel } => {
                    self.voices.retain(|v| v.channel.as_ref() != Some(&channel))
                }
                SoundCommand::Play {
                    resource,
                    sync,
                    loops,
                    loop_forever,
                    envelope,
                } => {
                    if sync == SoundSync::Start
                        && self.voices.iter().any(|v| v.resource == resource)
                    {
                        continue;
                    }
                    self.start(project, resource, None, 0., (loops, loop_forever), envelope);
                }
                SoundCommand::StreamStart {
                    channel,
                    resource,
                    offset,
                    loops,
                    loop_forever,
                    envelope,
                } => {
                    self.voices.retain(|v| v.channel.as_ref() != Some(&channel));
                    let channel = Some(channel);
                    self.start(
                        project,
                        resource,
                        channel,
                        offset,
                        (loops, loop_forever),
                        envelope,
                    );
                }
            }
        }
    }

    /// A new (or no) playback session silences the previous session's sounds.
    fn follow_preview(&mut self, epoch: Option<u64>) {
        if self.preview != epoch {
            self.voices
                .retain(|voice| voice.channel.as_deref() == Some(AUDITION));
            self.preview = epoch;
        }
    }

    /// Properties ▸ Sound ▸ Play / Edit envelope ▸ Play: hear the sound once with its envelope.
    pub(crate) fn audition(&mut self, project: &Project, sound: &FrameSound) {
        self.voices
            .retain(|v| v.channel.as_deref() != Some(AUDITION));
        let envelope = sound.envelope_points();
        self.start(
            project,
            sound.resource.clone(),
            Some(AUDITION.into()),
            0.,
            (1, false),
            envelope,
        );
    }

    pub(crate) fn stop_audition(&mut self) {
        self.voices
            .retain(|v| v.channel.as_deref() != Some(AUDITION));
    }

    pub(crate) fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if muted {
            self.voices.clear();
        }
    }
}

impl Editor {
    /// Audio resources of the document as `(id, path)`, in library order.
    pub(crate) fn audio_resources(&self) -> Vec<(Id, String)> {
        self.project
            .resources
            .iter()
            .filter(|unit| unit.is_audio())
            .map(|unit| (unit.id.clone(), unit.path.clone()))
            .collect()
    }

    /// The sound of the keyframe that starts the span under the playhead on the current layer.
    pub(crate) fn frame_sound(&self) -> Option<(u32, FrameSound)> {
        let layer = self.symbol().timeline.layers.get(self.layer)?;
        let key = layer.key_at(self.frame)?;
        Some((key.frame, key.sound.clone()?))
    }

    /// Attach `resource` to the current keyframe (default sync Event; an existing sound keeps its
    /// sync, repeat and effect), or remove the sound with `None`. Creates a blank keyframe when the
    /// layer has none at or before the playhead.
    fn attach_frame_sound(&mut self, resource: Option<Id>) {
        let (layer, frame) = (self.layer, self.frame);
        let has_key = self.symbol().timeline.layers[layer].key_at(frame).is_some();
        if !has_key && resource.is_some() {
            self.ensure_key(true);
        }
        if let Some(key) = self.symbol_mut().timeline.layers[layer].key_at_mut(frame) {
            key.sound = resource.map(|id| match key.sound.take() {
                Some(sound) => FrameSound {
                    resource: id,
                    ..sound
                },
                None => FrameSound::new(id),
            });
        }
    }

    /// Timeline controls ▸ Sound ▸ <library sound>, Properties ▸ Sound select. One Undo step.
    pub(crate) fn set_frame_sound(&mut self, resource: Option<Id>) {
        self.finish_edits();
        if !self.layer_editable() {
            self.message("Select an unlocked, visible layer first.");
            return;
        }
        let attached = resource.is_some();
        self.transact(|editor| editor.attach_frame_sound(resource));
        if attached {
            self.show_frame_sound();
        }
    }

    /// Select the sound's keyframe so Properties shows its Sound section.
    fn show_frame_sound(&mut self) {
        if let Some((start, _)) = self.frame_sound() {
            let layer = self.symbol().timeline.layers[self.layer].id.clone();
            self.select_frame_range(FrameRange {
                layers: vec![layer],
                start,
                end: start,
            });
            self.show_panel(crate::workspace::PanelId::Properties);
        }
    }

    /// Properties ▸ Sound: change sync, repeat or effect of the current keyframe's sound.
    pub(crate) fn edit_frame_sound(&mut self, edit: impl FnOnce(&mut FrameSound)) {
        self.finish_edits();
        if !self.layer_editable() || self.frame_sound().is_none() {
            return;
        }
        let (layer, frame) = (self.layer, self.frame);
        self.transact(|editor| {
            if let Some(sound) = editor.symbol_mut().timeline.layers[layer]
                .key_at_mut(frame)
                .and_then(|key| key.sound.as_mut())
            {
                edit(sound);
                sound.loops = sound.loops.clamp(1, FrameSound::MAX_LOOPS);
            }
        });
    }

    /// Timeline controls ▸ Sound ▸ Import sound…: add the file as a resource and attach it to
    /// the current keyframe in one Undo step.
    pub(crate) fn import_frame_sound(&mut self, path: &Path) -> Result<(), String> {
        self.finish_edits();
        if !self.layer_editable() {
            return Err("Select an unlocked, visible layer first.".into());
        }
        let used: usize = self
            .project
            .resources
            .iter()
            .map(ResourceUnit::byte_len)
            .sum();
        let unit =
            crate::resources::read_resource(path, MAX_RESOURCE_TOTAL_BYTES.saturating_sub(used))?;
        if !unit.is_audio() {
            return Err(format!(
                "{} is not a sound file. Import MP3 or WAV audio.",
                unit.path
            ));
        }
        let mut candidate = self.project.clone();
        candidate.format_version = candidate.format_version.max(3);
        candidate.resources.push(unit.clone());
        candidate.validate().map_err(|error| error.to_string())?;
        let id = unit.id.clone();
        self.transact(|editor| {
            editor.project.format_version = editor.project.format_version.max(3);
            editor.project.resources.push(unit);
            editor.attach_frame_sound(Some(id));
        });
        self.show_frame_sound();
        Ok(())
    }

    pub(crate) fn import_frame_sound_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Sound", &["mp3", "wav"])
            .pick_file()
        else {
            return;
        };
        match self.import_frame_sound(&path) {
            Ok(()) => self.message("Sound imported and attached to the keyframe."),
            Err(error) => self.message(format!("Sound import failed: {error}")),
        }
    }

    /// Feed editor playback's sound commands to the native audio preview (once per UI frame).
    pub(crate) fn pump_timeline_audio(&mut self) {
        let epoch = self.preview_epoch();
        let commands = self.drain_preview_sounds();
        self.timeline_audio.follow_preview(epoch);
        self.timeline_audio.apply(&self.project, commands);
        if let Some(failure) = self.timeline_audio.failure.take() {
            self.message(format!("Timeline sound preview: {failure}"));
            // Keep a missing device sticky so playback does not retry every frame.
            if failure.starts_with("No audio output") {
                self.timeline_audio.failure = Some(failure);
                self.timeline_audio.native = false;
            }
        }
    }

    /// Control ▸ Mute sounds (⌥⌘M).
    pub(crate) fn toggle_mute_sounds(&mut self) {
        let muted = !self.timeline_audio.muted;
        self.timeline_audio.set_muted(muted);
        self.message(if muted { "Sounds muted." } else { "Sounds on." });
    }
}
