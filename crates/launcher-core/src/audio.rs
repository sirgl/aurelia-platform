//! Music and sound effects. One worker thread owns the output device: the game sends
//! it commands and never waits for it, so a missing device, file or decoder only
//! silences the part that failed (the reason is shown in Settings).
//!
//! Music: the menu theme before a match; in the game a playlist that follows the
//! player's era, with a crossfade between tracks and on an era change. Effects: short
//! synthesized clips (platform/assets/audio/sfx, made by platform/tools/audio/make_sfx.py), decoded once
//! and mixed over the music. Headless callers never construct `MusicPlayer`.
use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use client::shell::assets;
pub use client::shell::presentation::motion::Sound;
pub use client::shell::screens::MusicMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Track {
    MainMenu,
    First,
    Second,
}

impl Track {
    fn filename(self) -> &'static str {
        match self {
            Self::MainMenu => "main_menu.mp3",
            Self::First => "track1.mp3",
            Self::Second => "track2.mp3",
        }
    }
}

/// Game tracks of an era, in playing order. Two game tracks exist (platform/assets/audio/imported):
/// the brighter first track leads in the ancient and classical eras, the second from the
/// medieval era on.
fn era_tracks(era: u8) -> [Track; 2] {
    match era {
        0 | 1 => [Track::First, Track::Second],
        _ => [Track::Second, Track::First],
    }
}

#[derive(Default)]
struct Playlist {
    mode: Option<MusicMode>,
    era: u8,
    next: usize,
}

impl Playlist {
    /// True when the change asks for another track now.
    fn set_mode(&mut self, mode: MusicMode) -> bool {
        if self.mode == Some(mode) {
            return false;
        }
        self.mode = Some(mode);
        self.next = 0;
        true
    }

    /// True when the era's playlist leads with another track (crossfade to it).
    fn set_era(&mut self, era: u8) -> bool {
        if self.era == era {
            return false;
        }
        let lead_changes = era_tracks(self.era)[0] != era_tracks(era)[0];
        self.era = era;
        if lead_changes {
            self.next = 0;
        }
        lead_changes && self.mode == Some(MusicMode::Game)
    }

    fn next_track(&mut self) -> Track {
        match self.mode {
            Some(MusicMode::Game) => {
                let tracks = era_tracks(self.era);
                let track = tracks[self.next % tracks.len()];
                self.next = (self.next + 1) % tracks.len();
                track
            }
            _ => Track::MainMenu,
        }
    }
}

/// File and loudness of every effect.
fn effect_file(sound: Sound) -> (&'static str, f32) {
    match sound {
        Sound::Select => ("select.wav", 0.45),
        Sound::Move => ("move.wav", 0.4),
        Sound::Melee => ("melee.wav", 0.8),
        Sound::Archery => ("archery.wav", 0.75),
        Sound::Gunpowder => ("gunpowder.wav", 0.8),
        Sound::Artillery => ("artillery.wav", 0.85),
        Sound::Naval => ("naval.wav", 0.85),
        Sound::Air => ("air.wav", 0.8),
        Sound::Death => ("death.wav", 0.7),
        Sound::CityFounded => ("city_founded.wav", 0.75),
        Sound::Building => ("building.wav", 0.6),
        Sound::Wonder => ("wonder.wav", 0.85),
        Sound::Tech => ("tech.wav", 0.65),
        Sound::Decree => ("policy.wav", 0.65),
        Sound::Hymn => ("faith.wav", 0.65),
        Sound::Notification => ("notification.wav", 0.55),
    }
}

const ALL_EFFECTS: [Sound; 16] = [
    Sound::Select,
    Sound::Move,
    Sound::Melee,
    Sound::Archery,
    Sound::Gunpowder,
    Sound::Artillery,
    Sound::Naval,
    Sound::Air,
    Sound::Death,
    Sound::CityFounded,
    Sound::Building,
    Sound::Wonder,
    Sound::Tech,
    Sound::Decree,
    Sound::Hymn,
    Sound::Notification,
];

/// The same effect again within this time is dropped (ten units hit at once).
const EFFECT_REPEAT: Duration = Duration::from_millis(90);
const CROSSFADE: Duration = Duration::from_millis(2500);
const TICK: Duration = Duration::from_millis(40);

enum Command {
    Mode(MusicMode),
    Era(u8),
    Volume(f32),
    Muted(bool),
    EffectsVolume(f32),
    Play(Sound),
    Shutdown,
}

/// Commands cross a channel; the worker exclusively owns the native output device. Its
/// music queue holds file-backed decoders, never whole-track PCM.
pub struct MusicPlayer {
    commands: Sender<Command>,
    failures: Receiver<String>,
    worker: Option<JoinHandle<()>>,
    mode: Option<MusicMode>,
    era: u8,
    volume: f32,
    muted: bool,
    effects_volume: f32,
    effects_muted: bool,
    error: Option<String>,
}

impl MusicPlayer {
    pub fn new() -> Self {
        let (commands, input) = mpsc::channel();
        let (status, failures) = mpsc::channel();
        let worker = thread::Builder::new().name("game-audio".into()).spawn(move || {
            if let Err(error) = playback_worker(input, &status) {
                eprintln!("Audio unavailable: {error}");
                let _ = status.send(error);
            }
        });
        let (worker, error) = match worker {
            Ok(worker) => (Some(worker), None),
            Err(error) => (None, Some(error.to_string())),
        };
        Self {
            commands,
            failures,
            worker,
            mode: None,
            era: 0,
            volume: 0.35,
            muted: false,
            effects_volume: 0.7,
            effects_muted: false,
            error,
        }
    }

    /// Repeating a mode never restarts playback (city/research/policy overlays can retain Game).
    pub fn set_mode(&mut self, mode: MusicMode) {
        if self.mode != Some(mode) {
            self.mode = Some(mode);
            let _ = self.commands.send(Command::Mode(mode));
        }
    }

    /// The player's era (0 ancient .. 7 information): the game playlist follows it.
    pub fn set_era(&mut self, era: u8) {
        if self.era != era {
            self.era = era;
            let _ = self.commands.send(Command::Era(era));
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        let volume = clean_volume(volume);
        if self.volume != volume {
            self.volume = volume;
            let _ = self.commands.send(Command::Volume(volume));
        }
    }

    pub fn set_muted(&mut self, muted: bool) {
        if self.muted != muted {
            self.muted = muted;
            let _ = self.commands.send(Command::Muted(muted));
        }
    }

    pub fn set_effects_volume(&mut self, volume: f32) {
        let volume = clean_volume(volume);
        if self.effects_volume != volume {
            self.effects_volume = volume;
            let _ = self.commands.send(Command::EffectsVolume(volume));
        }
    }

    pub fn set_effects_muted(&mut self, muted: bool) {
        self.effects_muted = muted;
    }

    /// Fire and forget; never blocks the frame.
    pub fn play(&mut self, sound: Sound) {
        if !self.effects_muted {
            let _ = self.commands.send(Command::Play(sound));
        }
    }

    /// Optional status collection; playback itself does not depend on UI redraw or polling.
    pub fn update(&mut self) {
        while let Ok(error) = self.failures.try_recv() {
            self.error = Some(error);
        }
    }

    pub fn unavailable_reason(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

impl Default for MusicPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MusicPlayer {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn clean_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// A track playing in its own sink, fading in or out.
struct Voice {
    sink: Sink,
    /// 0..1 crossfade level.
    level: f32,
    fading_out: bool,
    started: Instant,
    length: Option<Duration>,
}

/// The worker's state: device, music voices, decoded effects.
struct Mixer {
    handle: OutputStreamHandle,
    playlist: Playlist,
    voices: Vec<Voice>,
    volume: f32,
    muted: bool,
    effects_volume: f32,
    clips: HashMap<Sound, Clip>,
    last_played: HashMap<Sound, Instant>,
    status: Sender<String>,
    /// A track failed to open or play: music stays off, effects go on.
    music_failed: bool,
}

fn playback_worker(input: Receiver<Command>, status: &Sender<String>) -> Result<(), String> {
    let (_stream, handle) = OutputStream::try_default().map_err(|error| error.to_string())?;
    let (clips, missing) = load_clips();
    if let Some(error) = missing {
        let _ = status.send(error);
    }
    let status = status.clone();
    let mut mixer = Mixer {
        handle,
        playlist: Playlist::default(),
        voices: Vec::new(),
        volume: 0.35,
        muted: false,
        effects_volume: 0.7,
        clips,
        last_played: HashMap::new(),
        status,
        music_failed: false,
    };
    loop {
        match input.recv_timeout(TICK) {
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => return Ok(()),
            Ok(command) => mixer.apply(command),
            Err(RecvTimeoutError::Timeout) => {}
        }
        mixer.tick();
    }
}

impl Mixer {
    fn apply(&mut self, command: Command) {
        match command {
            Command::Mode(mode) => {
                if self.playlist.set_mode(mode) {
                    self.start_next();
                }
            }
            Command::Era(era) => {
                if self.playlist.set_era(era) {
                    self.start_next();
                }
            }
            Command::Volume(value) => self.volume = value,
            Command::Muted(value) => self.muted = value,
            Command::EffectsVolume(value) => self.effects_volume = value,
            Command::Play(sound) => self.play_effect(sound),
            Command::Shutdown => {}
        }
    }

    /// Crossfade to the playlist's next track: the playing voices fade out.
    fn start_next(&mut self) {
        for voice in &mut self.voices {
            voice.fading_out = true;
        }
        if self.playlist.mode.is_none() || self.music_failed {
            return;
        }
        match self.open_voice() {
            Ok(voice) => self.voices.push(voice),
            Err(error) => {
                self.music_failed = true;
                let _ = self.status.send(error);
            }
        }
    }

    fn open_voice(&mut self) -> Result<Voice, String> {
        let decoder = open_track(self.playlist.next_track())?;
        let length = decoder.total_duration();
        let sink = Sink::try_new(&self.handle).map_err(|error| error.to_string())?;
        sink.set_volume(0.0);
        sink.append(decoder);
        Ok(Voice { sink, level: 0.0, fading_out: false, started: Instant::now(), length })
    }

    /// Fades, the next track before the current one ends, and the volume.
    fn tick(&mut self) {
        let step = TICK.as_secs_f32() / CROSSFADE.as_secs_f32();
        let master = if self.muted { 0.0 } else { self.volume };
        for voice in &mut self.voices {
            let target = if voice.fading_out { 0.0 } else { 1.0 };
            voice.level += (target - voice.level).clamp(-step, step);
            voice.sink.set_volume(voice.level * master);
        }
        self.voices.retain(|v| !(v.fading_out && v.level <= 0.0) && !v.sink.empty());
        let ending = self.voices.iter().filter(|v| !v.fading_out).all(Voice::near_end);
        if ending && self.playlist.mode.is_some() {
            self.start_next();
        }
    }

    fn play_effect(&mut self, sound: Sound) {
        let now = Instant::now();
        if let Some(last) = self.last_played.get(&sound) {
            if now.duration_since(*last) < EFFECT_REPEAT {
                return;
            }
        }
        let Some(clip) = self.clips.get(&sound) else {
            return;
        };
        self.last_played.insert(sound, now);
        let gain = effect_file(sound).1 * self.effects_volume;
        let source = SamplesBuffer::new(1, clip.rate, clip.samples.to_vec()).amplify(gain);
        // A failed effect is only a missing sound.
        let _ = self.handle.play_raw(source.convert_samples());
    }
}

impl Voice {
    /// Within a crossfade of its end (or already drained): the next track starts.
    fn near_end(&self) -> bool {
        if self.sink.empty() {
            return true;
        }
        match self.length {
            Some(length) => self.started.elapsed() + CROSSFADE >= length,
            None => false,
        }
    }
}

fn open_track(track: Track) -> Result<Decoder<BufReader<File>>, String> {
    let path = assets::path("audio/imported").join(track.filename());
    let file = File::open(path).map_err(|error| format!("{}: {error}", track.filename()))?;
    Decoder::new(BufReader::new(file)).map_err(|error| format!("{}: {error}", track.filename()))
}

/// A decoded effect: mono 16-bit samples.
struct Clip {
    rate: u32,
    samples: Arc<[i16]>,
}

/// Every effect that loads; the first failure as a message (the others stay silent).
fn load_clips() -> (HashMap<Sound, Clip>, Option<String>) {
    let mut clips = HashMap::new();
    let mut error = None;
    for sound in ALL_EFFECTS {
        let name = effect_file(sound).0;
        let path = assets::path("audio/sfx").join(name);
        let clip = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| read_wav(&bytes));
        match clip {
            Ok(clip) => {
                clips.insert(sound, clip);
            }
            Err(reason) => {
                error.get_or_insert(format!("{name}: {reason}"));
            }
        }
    }
    (clips, error)
}

/// Mono 16-bit PCM WAV (what make_sfx.py writes).
fn read_wav(bytes: &[u8]) -> Result<Clip, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let mut rate = None;
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().expect("4 bytes")) as usize;
        let body = bytes.get(at + 8..at + 8 + size).ok_or("truncated chunk")?;
        match id {
            b"fmt " => rate = Some(wav_format(body)?),
            b"data" => {
                let rate = rate.ok_or("data before format")?;
                let samples = body.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b)).collect();
                return Ok(Clip { rate, samples });
            }
            _ => {}
        }
        at += 8 + size + size % 2;
    }
    Err("no audio data".into())
}

/// Sample rate of a mono 16-bit PCM format chunk.
fn wav_format(body: &[u8]) -> Result<u32, String> {
    if body.len() < 16 {
        return Err("short format chunk".into());
    }
    let format = u16::from_le_bytes([body[0], body[1]]);
    let channels = u16::from_le_bytes([body[2], body[3]]);
    let rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
    let bits = u16::from_le_bytes([body[14], body[15]]);
    if format != 1 || channels != 1 || bits != 16 || rate == 0 {
        return Err(format!("unsupported WAV: format {format}, {channels} channels, {bits} bits"));
    }
    Ok(rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_repeats_and_game_alternates_without_restarting_same_mode() {
        let mut playlist = Playlist::default();
        assert!(playlist.set_mode(MusicMode::Menu));
        assert_eq!(playlist.next_track(), Track::MainMenu);
        assert_eq!(playlist.next_track(), Track::MainMenu);
        assert!(playlist.set_mode(MusicMode::Game));
        assert_eq!(playlist.next_track(), Track::First);
        assert!(!playlist.set_mode(MusicMode::Game));
        assert_eq!(playlist.next_track(), Track::Second);
        assert_eq!(playlist.next_track(), Track::First);
        assert!(playlist.set_mode(MusicMode::Menu));
        assert_eq!(playlist.next_track(), Track::MainMenu);
    }

    #[test]
    fn the_game_playlist_follows_the_era_and_crossfades_only_when_its_lead_changes() {
        let mut playlist = Playlist::default();
        playlist.set_mode(MusicMode::Game);
        assert!(!playlist.set_era(1), "classical keeps the ancient lead track");
        assert_eq!(playlist.next_track(), Track::First);
        assert!(playlist.set_era(2), "the medieval era leads with the other track");
        assert_eq!(playlist.next_track(), Track::Second);
        assert_eq!(playlist.next_track(), Track::First);
        assert!(!playlist.set_era(5));
        let mut menu = Playlist::default();
        menu.set_mode(MusicMode::Menu);
        assert!(!menu.set_era(4), "the menu theme ignores the era");
    }

    #[test]
    fn every_effect_decodes_without_a_device() {
        let (clips, error) = load_clips();
        assert_eq!(error, None);
        assert_eq!(clips.len(), ALL_EFFECTS.len());
        for (sound, clip) in &clips {
            let seconds = clip.samples.len() as f32 / clip.rate as f32;
            assert!((0.2..=2.5).contains(&seconds), "{sound:?}: {seconds} s");
            let peak = clip.samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
            assert!(peak > 3000 && peak < 32000, "{sound:?}: peak {peak}");
            assert!(clip.samples[0].unsigned_abs() < 200, "{sound:?} starts with a click");
        }
    }

    #[test]
    fn broken_effect_files_are_rejected_not_played() {
        assert!(read_wav(b"not audio").is_err());
        let mut stereo = std::fs::read(assets::path("audio/sfx/select.wav")).unwrap();
        stereo[22] = 2;
        assert!(read_wav(&stereo).is_err());
        assert!(read_wav(&stereo[..30]).is_err());
    }

    fn verify_track(track: Track) {
        // Decode on CPU only: never construct MusicPlayer, OutputStream or an audio device.
        let decoder = open_track(track).unwrap();
        assert!((1..=2).contains(&decoder.channels()));
        assert!(decoder.sample_rate() >= 22050);
        assert_eq!(decoder.take(4096).count(), 4096, "MP3 must yield decoded samples");
    }

    #[test]
    fn menu_mp3_decodes_without_device() {
        verify_track(Track::MainMenu);
    }

    #[test]
    fn first_game_mp3_decodes_without_device() {
        verify_track(Track::First);
    }

    #[test]
    fn second_game_mp3_decodes_without_device() {
        verify_track(Track::Second);
    }
}
