//! Host-independent media state shared by compiled and interpreted components.

use crate::{Node, Property, Value};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

/// One tightly packed, straight-alpha video image shared by scene snapshots.
#[derive(Clone)]
pub struct MediaFrame(Arc<([u32; 2], Vec<u8>)>);

impl MediaFrame {
    pub fn new(size: [u32; 2], rgba: Vec<u8>) -> Option<Self> {
        let [width, height] = size;
        let bytes = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        (width > 0 && height > 0 && bytes == rgba.len()).then(|| Self(Arc::new((size, rgba))))
    }
    pub fn size(&self) -> [u32; 2] {
        self.0.0
    }
    pub fn rgba(&self) -> &[u8] {
        &self.0.1
    }
}

impl PartialEq for MediaFrame {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for MediaFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaFrame")
            .field("size", &self.size())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MediaStatus {
    #[default]
    Empty,
    Loading,
    Seeking,
    Buffering,
    Paused,
    Playing,
    Stopped,
    Ended,
    Failed,
}
impl MediaStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Loading => "loading",
            Self::Seeking => "seeking",
            Self::Buffering => "buffering",
            Self::Paused => "paused",
            Self::Playing => "playing",
            Self::Stopped => "stopped",
            Self::Ended => "ended",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MediaOptions {
    pub playback_rate: f64,
    pub network_bandwidth: u64,
    pub loops: i64,
    pub audio_track: i64,
    pub subtitle_track: i64,
    pub subtitle_file: Option<PathBuf>,
}

impl Default for MediaOptions {
    fn default() -> Self {
        Self {
            playback_rate: 1.0,
            network_bandwidth: 0,
            loops: 1,
            audio_track: -1,
            subtitle_track: -1,
            subtitle_file: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaState {
    pub frame: Option<MediaFrame>,
    pub position: f64,
    pub duration: f64,
    pub playing: bool,
    pub ready: bool,
    pub ended: bool,
    pub error: String,
    pub status: MediaStatus,
    pub seekable: bool,
    pub live: bool,
    pub seek_start: f64,
    pub seek_end: f64,
    pub at_live_edge: bool,
    pub has_audio: bool,
    pub has_video: bool,
    pub audio_tracks: i64,
    pub subtitle_tracks: i64,
    pub title: String,
    pub subtitle: String,
    pub audio_error: String,
    pub subtitle_error: String,
    pub buffer_progress: f64,
    pub bitrate: i64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaCommand {
    Play,
    Pause,
    Stop,
    Seek(f64),
    GoLive,
}

pub trait MediaSession {
    fn command(&mut self, command: MediaCommand);
    fn set_volume(&mut self, volume: f64);
    fn configure(&mut self, _options: &MediaOptions) {}
    fn sample(&mut self, now: Instant) -> MediaState;
    fn active(&self) -> bool;

    fn poll_interval(&self) -> Duration {
        Duration::from_millis(15)
    }
}

type Factory = fn(&Path, bool, f64) -> Result<Box<dyn MediaSession>, String>;
thread_local! {
    static FACTORY: Cell<Option<Factory>> = const { Cell::new(None) };
}

pub fn register_media_backend(factory: Factory) {
    FACTORY.set(Some(factory));
}

/// Open a media session using the backend installed for this UI thread.
#[doc(hidden)]
pub fn open_media_session(
    path: &Path,
    autoplay: bool,
    volume: f64,
) -> Result<Box<dyn MediaSession>, String> {
    FACTORY
        .get()
        .ok_or_else(|| "Media playback requires a media-enabled host".to_owned())?(
        path, autoplay, volume,
    )
}

#[doc(hidden)]
#[derive(Default)]
pub struct MediaSlot {
    state: Property<MediaState>,
    commands: RefCell<Vec<MediaCommand>>,
}

impl MediaSlot {
    pub fn position(&self) -> f64 {
        self.state.get(MediaState::default).position
    }
    pub fn duration(&self) -> f64 {
        self.state.get(MediaState::default).duration
    }
    pub fn playing(&self) -> bool {
        self.state.get(MediaState::default).playing
    }
    pub fn ready(&self) -> bool {
        self.state.get(MediaState::default).ready
    }
    pub fn ended(&self) -> bool {
        self.state.get(MediaState::default).ended
    }
    pub fn error(&self) -> String {
        self.state.get(MediaState::default).error
    }

    pub fn status(&self) -> String {
        self.state.get(MediaState::default).status.as_str().into()
    }
    pub fn seekable(&self) -> bool {
        self.state.get(MediaState::default).seekable
    }
    pub fn live(&self) -> bool {
        self.state.get(MediaState::default).live
    }
    pub fn seek_start(&self) -> f64 {
        self.state.get(MediaState::default).seek_start
    }
    pub fn seek_end(&self) -> f64 {
        self.state.get(MediaState::default).seek_end
    }
    pub fn at_live_edge(&self) -> bool {
        self.state.get(MediaState::default).at_live_edge
    }
    pub fn has_audio(&self) -> bool {
        self.state.get(MediaState::default).has_audio
    }
    pub fn has_video(&self) -> bool {
        self.state.get(MediaState::default).has_video
    }
    pub fn audio_tracks(&self) -> i64 {
        self.state.get(MediaState::default).audio_tracks
    }
    pub fn subtitle_tracks(&self) -> i64 {
        self.state.get(MediaState::default).subtitle_tracks
    }
    pub fn media_title(&self) -> String {
        self.state.get(MediaState::default).title
    }
    pub fn subtitle(&self) -> String {
        self.state.get(MediaState::default).subtitle
    }
    pub fn audio_error(&self) -> String {
        self.state.get(MediaState::default).audio_error
    }

    pub fn subtitle_error(&self) -> String {
        self.state.get(MediaState::default).subtitle_error
    }
    pub fn buffer_progress(&self) -> f64 {
        self.state.get(MediaState::default).buffer_progress
    }

    pub fn bitrate(&self) -> i64 {
        self.state.get(MediaState::default).bitrate
    }

    pub fn read(&self, name: &str) -> Option<Value> {
        Some(match name {
            "position" => self.position().into(),
            "duration" => self.duration().into(),
            "playing" => self.playing().into(),
            "ready" => self.ready().into(),
            "ended" => self.ended().into(),
            "error" => self.error().into(),
            "status" => self.status().into(),
            "seekable" => self.seekable().into(),
            "live" => self.live().into(),
            "seek-start" => self.seek_start().into(),
            "seek-end" => self.seek_end().into(),
            "at-live-edge" => self.at_live_edge().into(),
            "has-audio" => self.has_audio().into(),
            "has-video" => self.has_video().into(),
            "audio-tracks" => self.audio_tracks().into(),
            "subtitle-tracks" => self.subtitle_tracks().into(),
            "media-title" => self.media_title().into(),
            "subtitle" => self.subtitle().into(),
            "audio-error" => self.audio_error().into(),
            "subtitle-error" => self.subtitle_error().into(),
            "buffer-progress" => self.buffer_progress().into(),
            "bitrate" => self.bitrate().into(),
            _ => return None,
        })
    }

    pub fn command(&self, command: MediaCommand) {
        self.commands.borrow_mut().push(command);
    }

    pub fn populate(self: &Rc<Self>, node: &mut Node, file: &str) {
        let state = self.state.get(MediaState::default);
        node.media_frame = state.frame;
        let source = node.text("source").unwrap_or_default();
        let path = if source.starts_with("https://") || source.starts_with("http://") {
            Some(PathBuf::from(source))
        } else {
            local_source(file, source)
        };
        let rate = node.number("playback-rate").unwrap_or(1.0);
        let volume = node.number("volume").unwrap_or(1.0);
        node.media = Some(MediaRegistration {
            slot: self.clone(),
            path,
            autoplay: node.boolean("autoplay", false),
            options: MediaOptions {
                playback_rate: if rate.is_finite() {
                    rate.clamp(0.25, 4.0)
                } else {
                    1.0
                },
                network_bandwidth: node
                    .int("network-bandwidth")
                    .unwrap_or(0)
                    .clamp(0, 1_000_000_000) as u64,
                loops: node.int("loops").unwrap_or(1).max(-1),
                audio_track: node.int("audio-track").unwrap_or(-1),
                subtitle_track: node.int("subtitle-track").unwrap_or(-1),
                subtitle_file: node
                    .text("subtitle-file")
                    .and_then(|source| local_source(file, source)),
            },
            volume: if node.boolean("muted", false) {
                0.0
            } else if volume.is_finite() {
                volume.clamp(0.0, 1.0)
            } else {
                0.0
            },
        });
    }
}

fn local_source(file: &str, source: &str) -> Option<PathBuf> {
    if source.is_empty() {
        return None;
    }
    let path = Path::new(source);
    Some(Path::new(file).parent().unwrap_or(Path::new("")).join(path))
}

#[derive(Clone)]
#[doc(hidden)]
pub struct MediaRegistration {
    slot: Rc<MediaSlot>,
    path: Option<PathBuf>,
    autoplay: bool,
    volume: f64,
    options: MediaOptions,
}

impl MediaRegistration {
    pub fn key(&self) -> usize {
        Rc::as_ptr(&self.slot) as usize
    }
}

impl PartialEq for MediaRegistration {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.slot, &other.slot)
            && self.path == other.path
            && self.autoplay == other.autoplay
            && self.volume == other.volume
            && self.options == other.options
    }
}

impl std::fmt::Debug for MediaRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaRegistration")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

struct Entry {
    registration: MediaRegistration,
    session: Option<Box<dyn MediaSession>>,
}

#[doc(hidden)]
#[derive(Default)]
pub struct MediaQueue {
    entries: BTreeMap<usize, Entry>,
    changed: bool,
}

impl MediaQueue {
    pub fn sync<'a>(
        &mut self,
        registrations: impl IntoIterator<Item = &'a MediaRegistration>,
    ) -> bool {
        let mut changed = false;
        let mut old = std::mem::take(&mut self.entries);
        for registration in registrations {
            let key = Rc::as_ptr(&registration.slot) as usize;
            let previous = old.remove(&key);
            let mut entry = match previous {
                Some(entry) if entry.registration.path == registration.path => entry,
                _ => {
                    let mut state = MediaState::default();
                    let session = registration.path.as_ref().and_then(|path| {
                        let result =
                            open_media_session(path, registration.autoplay, registration.volume);
                        match result {
                            Ok(mut session) => {
                                state.status = MediaStatus::Loading;
                                session.configure(&registration.options);
                                Some(session)
                            }
                            Err(error) => {
                                state.error = error;
                                state.status = MediaStatus::Failed;
                                None
                            }
                        }
                    });
                    changed |= publish(&registration.slot, state);
                    Entry {
                        registration: registration.clone(),
                        session,
                    }
                }
            };
            if entry.registration.volume != registration.volume
                && let Some(session) = &mut entry.session
            {
                session.set_volume(registration.volume);
            }
            if entry.registration.options != registration.options
                && let Some(session) = &mut entry.session
            {
                session.configure(&registration.options);
            }
            entry.registration = registration.clone();
            self.entries.insert(key, entry);
        }
        for (_, entry) in old {
            entry.registration.slot.commands.take();
            changed |= publish(&entry.registration.slot, MediaState::default());
        }
        self.changed |= changed;
        changed
    }

    pub fn advance(&mut self, now: Instant) -> bool {
        let mut changed = std::mem::take(&mut self.changed);
        for entry in self.entries.values_mut() {
            let commands = entry.registration.slot.commands.take();
            if let Some(session) = &mut entry.session {
                for command in commands {
                    session.command(command);
                }
                changed |= publish(&entry.registration.slot, session.sample(now));
            }
        }
        changed
    }

    pub fn deadline(&self, now: Instant) -> Option<Instant> {
        if self.changed {
            return Some(now);
        }
        self.entries
            .values()
            .filter_map(|entry| {
                if !entry.registration.slot.commands.borrow().is_empty() {
                    Some(now)
                } else {
                    entry
                        .session
                        .as_ref()
                        .filter(|session| session.active())
                        .map(|session| now + session.poll_interval())
                }
            })
            .min()
    }
}

fn publish(slot: &MediaSlot, state: MediaState) -> bool {
    if slot.state.get(MediaState::default) == state {
        return false;
    }
    slot.state.set(state);
    true
}
