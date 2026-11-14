//! Video is presented against the audio device clock when a soundtrack exists.
use crate::{
    VideoDecoder, VideoFrame,
    decoder::{LiveWindow, MediaError, check, is_network},
    subtitles,
};
use crate::{ff, native::api};
use std::{
    collections::VecDeque,
    ffi::c_void,
    path::{Path, PathBuf},
    ptr,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use xrack_runtime::{
    MediaCommand, MediaFrame, MediaOptions, MediaSession, MediaState, MediaStatus,
};

const RATE: u32 = 48_000;
const CHANNELS: usize = 2;
const AUDIO_AHEAD: u64 = RATE as u64 / 3;
const VIDEO_AHEAD: f64 = 0.25;
const VIDEO_BYTES_AHEAD: usize = 64 * 1024 * 1024;

struct Device {
    handle: *mut ff::XrackAudioDevice,
    state: Arc<Shared>,
}

impl Device {
    fn last_callback_elapsed(&self) -> Duration {
        let millis = self.state.audio_callback.load(Ordering::Relaxed);
        self.state
            .started
            .elapsed()
            .saturating_sub(Duration::from_millis(millis))
    }

    fn open(state: Arc<Shared>) -> Result<Self, MediaError> {
        let handle = unsafe {
            ((api().xr_audio_open).unwrap())(
                Some(render_audio),
                Arc::as_ptr(&state).cast_mut().cast(),
            )
        };
        if handle.is_null() {
            return Err(MediaError("no audio output device is available".into()));
        }
        Ok(Self { handle, state })
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // Closing the device waits for its callback before releasing the shared state.
        unsafe { ((api().xr_audio_close).unwrap())(self.handle) }
    }
}

struct AudioBlock {
    start: u64,
    samples: Vec<f32>,
}

impl AudioBlock {
    fn end(&self) -> u64 {
        self.start + (self.samples.len() / CHANNELS) as u64
    }
}

#[derive(Default)]
struct Playback {
    state: MediaState,
    requested_play: bool,
    volume: f32,
    options: MediaOptions,
    stopped: bool,
    buffering: bool,
    completed: i64,
    subtitle_dirty: bool,
    audio_track_dirty: bool,
    reopen: bool,
    go_live: bool,
    live_edge: f64,
    subtitles: Vec<subtitles::Cue>,
    audio_fraction: f64,
    underruns: u64,
    dirty: bool,
    request: Option<f64>,
    epoch: u64,

    audio: bool,
    audio_eof: bool,
    audio_cursor: u64,
    audio_blocks: VecDeque<AudioBlock>,
    submitted: VecDeque<(u64, u64, Instant, f64)>,
    audio_tail_until: Option<Instant>,
    resume_at: Option<Instant>,
    video: VecDeque<(f64, MediaFrame)>,
    video_eof: bool,
    anchor: Option<(f64, Instant)>,
}

impl Playback {
    fn update_window(&mut self, window: LiveWindow) {
        self.dirty |= !self.state.seekable
            || self.state.seek_start != window.start
            || self.state.seek_end != window.end;
        self.state.seekable = true;
        self.state.seek_start = window.start;
        self.state.seek_end = window.end;
        self.live_edge = window.edge;
    }

    fn seek_window(&self) -> LiveWindow {
        LiveWindow {
            start: self.state.seek_start,
            end: self.state.seek_end,
            edge: self.live_edge,
        }
    }

    fn reset(&mut self, position: f64) {
        self.epoch = self.epoch.wrapping_add(1);
        self.request = Some(position);
        self.state.position = position;
        self.state.ready = false;
        self.state.status = MediaStatus::Seeking;
        self.state.buffer_progress = 0.0;
        self.buffering = false;
        self.audio_fraction = 0.0;
        self.state.playing = false;
        self.state.ended = false;
        self.audio_blocks.clear();
        self.audio_cursor = sample_index(position);
        self.submitted.clear();
        self.resume_at = self.audio_tail_until.take().or(self.resume_at);
        self.audio_eof = false;
        self.video.clear();
        self.video_eof = false;
        self.anchor = None;
    }

    fn position(&self, now: Instant) -> f64 {
        if let Some(&(start, end, at, rate)) =
            self.submitted.iter().rev().find(|span| span.2 <= now)
        {
            let played = sample_index(now.saturating_duration_since(at).as_secs_f64() * rate);
            return seconds(start.saturating_add(played).min(end));
        }
        self.anchor.map_or(self.state.position, |(position, at)| {
            position + now.saturating_duration_since(at).as_secs_f64() * self.options.playback_rate
        })
    }

    fn fill_audio(&mut self, output: &mut [f32], delay: u32, now: Instant) {
        if !self.state.playing || !self.requested_play || self.buffering {
            return;
        }
        if self.resume_at.is_some_and(|resume| now < resume) {
            return;
        }
        self.resume_at = None;
        let start = self.audio_cursor;
        let rate = self.options.playback_rate;
        let mut output_frames = 0;
        for stereo in output.chunks_exact_mut(CHANNELS) {
            while self
                .audio_blocks
                .front()
                .is_some_and(|block| block.end() <= self.audio_cursor)
            {
                self.audio_blocks.pop_front();
            }
            let Some(block) = self.audio_blocks.front() else {
                break;
            };
            if self.audio_cursor >= block.start {
                let offset = (self.audio_cursor - block.start) as usize * CHANNELS;
                for (to, from) in stereo
                    .iter_mut()
                    .zip(&block.samples[offset..offset + CHANNELS])
                {
                    *to = *from * self.volume;
                }
            }
            self.audio_fraction += rate;
            let steps = self.audio_fraction.floor();
            self.audio_fraction -= steps;
            self.audio_cursor = self.audio_cursor.saturating_add(steps as u64);
            output_frames += 1;
        }
        if !self.audio_eof && output_frames < output.len() / CHANNELS {
            self.buffering = true;
            self.underruns = self.underruns.wrapping_add(1);
        }
        if output_frames > 0 {
            let latency = Duration::from_secs_f64(seconds(u64::from(delay)));
            let predicted = now + latency;
            let at = self.audio_tail_until.unwrap_or(predicted).max(predicted);
            while self.submitted.len() > 1 && self.submitted[1].2 <= now {
                self.submitted.pop_front();
            }
            if self.submitted.len() == 64 {
                self.submitted.pop_front();
            }
            self.submitted
                .push_back((start, self.audio_cursor, at, rate));
            self.audio_tail_until =
                Some(at + Duration::from_secs_f64(seconds(output_frames as u64)));
        }
    }
}

struct Shared {
    state: Mutex<Playback>,
    wake: Condvar,
    cancelled: Arc<AtomicBool>,
    audio_callback: std::sync::atomic::AtomicU64,
    started: Instant,
}

unsafe extern "C" fn render_audio(user: *mut c_void, samples: *mut f32, frames: u32, delay: u32) {
    // The device guarantees a writable interleaved stereo buffer for this callback.
    let output = unsafe { std::slice::from_raw_parts_mut(samples, frames as usize * CHANNELS) };
    output.fill(0.0);
    let shared = unsafe { &*user.cast::<Shared>() };
    shared.audio_callback.store(
        shared.started.elapsed().as_millis() as u64,
        Ordering::Relaxed,
    );
    if let Ok(mut playback) = shared.state.try_lock() {
        playback.fill_audio(output, delay, Instant::now());
    }
}

fn seconds(samples: u64) -> f64 {
    samples as f64 / f64::from(RATE)
}

fn sample_index(seconds: f64) -> u64 {
    (seconds * f64::from(RATE)).round() as u64
}

struct Session(Arc<Shared>);

pub fn install() {
    xrack_runtime::register_media_backend(open);
}

fn open(path: &Path, autoplay: bool, volume: f64) -> Result<Box<dyn MediaSession>, String> {
    let shared = Arc::new(Shared {
        state: Mutex::new(Playback {
            requested_play: autoplay,
            state: MediaState {
                status: MediaStatus::Loading,
                ..MediaState::default()
            },
            volume: volume as f32,
            submitted: VecDeque::with_capacity(64),
            ..Playback::default()
        }),
        wake: Condvar::new(),
        cancelled: Arc::default(),
        audio_callback: Default::default(),
        started: Instant::now(),
    });
    let producer = shared.clone();
    let path = path.to_path_buf();
    std::thread::Builder::new()
        .name("xrack-media".into())
        .spawn(move || {
            if let Err(error) = decode(path, &producer) {
                let mut playback = producer.state.lock().unwrap();
                if producer.cancelled.load(Ordering::Relaxed) {
                    return;
                }
                playback.state.error = error.to_string();
                playback.state.status = MediaStatus::Failed;
                playback.dirty = true;
                playback.state.playing = false;
                playback.state.ready = false;
                playback.requested_play = false;
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(Box::new(Session(shared)))
}

impl Drop for Session {
    fn drop(&mut self) {
        self.0.cancelled.store(true, Ordering::Relaxed);
        let _state = self.0.state.lock().unwrap();
        self.0.wake.notify_one();
    }
}

impl MediaSession for Session {
    fn command(&mut self, command: MediaCommand) {
        let mut playback = self.0.state.lock().unwrap();
        match command {
            MediaCommand::Play => {
                let was_stopped = std::mem::take(&mut playback.stopped);
                if playback.state.live {
                    playback.go_live |= was_stopped || playback.state.ended;
                } else {
                    playback.reopen |= was_stopped && !playback.state.seekable;
                }
                if playback.state.ended
                    || playback.state.duration > 0.0
                        && playback.state.position >= playback.state.duration
                {
                    playback.completed = 0;
                    if playback.state.live {
                        playback.go_live = true;
                    } else if playback.state.seekable {
                        playback.reset(0.0);
                    } else {
                        playback.reopen = true;
                    }
                }
                playback.requested_play = true;
            }
            MediaCommand::Pause => {
                if playback.state.playing {
                    let position = playback.position(Instant::now());
                    if playback.state.seekable {
                        playback.reset(position);
                    } else {
                        playback.state.position = position;
                        playback.state.playing = false;
                        playback.anchor = None;
                    }
                }
                playback.requested_play = false;
            }
            MediaCommand::Stop => {
                playback.requested_play = false;
                playback.stopped = true;
                playback.go_live = false;
                playback.completed = 0;
                if playback.state.live {
                    let position = playback.position(Instant::now());
                    playback.reset(position);
                    playback.request = None;
                    playback.state.ready = true;
                } else if playback.state.seekable {
                    playback.reset(0.0);
                } else {
                    playback.state.playing = false;
                    playback.anchor = None;
                    playback.audio_blocks.clear();
                    playback.video.clear();
                }
            }
            MediaCommand::Seek(position) if position.is_finite() && playback.state.seekable => {
                playback.go_live = false;
                playback.stopped = false;
                let duration = playback.state.duration;
                let position = if playback.state.live {
                    playback.seek_window().clamp(position)
                } else if duration > 0.0 {
                    position.clamp(0.0, duration)
                } else {
                    position.max(0.0)
                };
                if playback.state.duration > 0.0 && position == playback.state.duration {
                    playback.requested_play = false;
                    playback.reset((position - 1.0 / f64::from(RATE)).max(0.0));
                    playback.state.position = position;
                } else {
                    playback.reset(position);
                }
            }
            MediaCommand::Seek(_) => {}
            MediaCommand::GoLive if playback.state.live => {
                playback.stopped = false;
                playback.requested_play = true;
                playback.go_live = true;
                playback.request = None;
            }
            MediaCommand::GoLive => {}
        }
        self.0.wake.notify_one();
    }

    fn set_volume(&mut self, volume: f64) {
        self.0.state.lock().unwrap().volume = volume as f32;
    }

    fn configure(&mut self, options: &MediaOptions) {
        let mut playback = self.0.state.lock().unwrap();
        let now = Instant::now();
        let changed_rate = playback.options.playback_rate != options.playback_rate;
        if changed_rate && playback.state.playing && playback.anchor.is_some() {
            let position = playback.position(now);
            playback.anchor = Some((position, now));
        }
        playback.subtitle_dirty |= playback.options.subtitle_file != options.subtitle_file
            || playback.options.subtitle_track != options.subtitle_track;
        playback.audio_track_dirty |= playback.options.audio_track != options.audio_track;
        playback.options = options.clone();
        self.0.wake.notify_one();
    }

    fn sample(&mut self, now: Instant) -> MediaState {
        let mut playback = self.0.state.lock().unwrap();
        if playback.state.ready {
            if playback.buffering {
                let available = playback
                    .audio_blocks
                    .back()
                    .map_or(0, |block| block.end().saturating_sub(playback.audio_cursor));
                let threshold = u64::from(RATE) / 20;
                playback.buffering = !playback.audio_eof && available < threshold;
                playback.state.buffer_progress = if playback.buffering {
                    available as f64 / threshold as f64
                } else {
                    1.0
                };
            }
            if playback.requested_play && !playback.state.playing {
                playback.state.playing = true;
                if !playback.audio {
                    playback.anchor = Some((playback.state.position, now));
                }
            }
            if playback.state.playing {
                if playback.audio
                    && playback.audio_eof
                    && playback.audio_blocks.is_empty()
                    && playback.audio_tail_until.is_none_or(|at| now >= at)
                    && playback.anchor.is_none()
                {
                    let position = playback.position(now);
                    playback.submitted.clear();
                    playback.anchor = Some((position, now));
                }
                let position = playback.position(now).max(playback.state.position);
                let duration = playback.state.duration;
                playback.state.position = if duration > 0.0 {
                    position.min(duration)
                } else if playback.state.live && playback.state.seekable {
                    position.min(playback.state.seek_end)
                } else {
                    position
                };
                let video_position = playback.state.position + 0.006;
                while playback
                    .video
                    .front()
                    .is_some_and(|(timestamp, _)| *timestamp <= video_position)
                {
                    playback.state.frame = Some(playback.video.pop_front().unwrap().1);
                }
                if playback.video_eof
                    && playback.video.is_empty()
                    && (!playback.audio
                        || playback.audio_eof
                            && playback.audio_blocks.is_empty()
                            && playback.audio_tail_until.is_none_or(|at| now >= at))
                    && (duration == 0.0 || position >= duration)
                {
                    playback.completed += 1;
                    if playback.state.seekable
                        && !playback.state.live
                        && (playback.options.loops == -1
                            || playback.completed < playback.options.loops.max(1))
                    {
                        playback.reset(0.0);
                    } else {
                        playback.state.ended = true;
                        playback.state.playing = false;
                        playback.requested_play = false;
                    }
                }
                self.0.wake.notify_one();
            } else if playback.state.frame.is_none()
                && playback
                    .video
                    .front()
                    .is_some_and(|(timestamp, _)| *timestamp <= playback.state.position)
            {
                playback.state.frame = Some(playback.video.pop_front().unwrap().1);
            }
        }
        playback.state.at_live_edge = playback.state.live
            && playback.state.seekable
            && playback.state.position + 0.5 >= playback.live_edge;
        if playback.state.ready {
            playback.state.status = if playback.state.ended {
                MediaStatus::Ended
            } else if playback.stopped {
                MediaStatus::Stopped
            } else if playback.state.playing && playback.buffering {
                MediaStatus::Buffering
            } else if playback.state.playing {
                MediaStatus::Playing
            } else {
                MediaStatus::Paused
            };
        }
        playback.dirty = false;
        playback.state.subtitle = subtitles::at(&playback.subtitles, playback.state.position);
        playback.state.clone()
    }

    fn active(&self) -> bool {
        let state = self.0.state.lock().unwrap();
        !self.0.cancelled.load(Ordering::Relaxed)
            && (state.dirty
                || state.state.error.is_empty()
                    && (!state.state.ready
                        || state.requested_play
                        || state.state.live
                        || state.audio_track_dirty
                        || state.subtitle_dirty
                        || state.reopen))
    }

    fn poll_interval(&self) -> Duration {
        let playback = self.0.state.lock().unwrap();
        if playback.state.live
            && playback.state.ready
            && !playback.requested_play
            && !playback.dirty
        {
            Duration::from_millis(250)
        } else {
            Duration::from_millis(15)
        }
    }
}

fn video_frame(frame: VideoFrame) -> Result<MediaFrame, MediaError> {
    MediaFrame::new(frame.size, frame.rgba)
        .ok_or_else(|| MediaError("invalid decoded video frame".into()))
}

fn live_window(
    video: Option<&VideoDecoder>,
    audio: Option<&AudioDecoder>,
    refresh: bool,
) -> Option<LiveWindow> {
    match (video, audio) {
        (Some(video), Some(audio)) => video
            .live_window(refresh)?
            .intersect(audio.decoder.live_window(refresh)?),
        (Some(video), None) => video.live_window(refresh),
        (None, Some(audio)) => audio.decoder.live_window(refresh),
        (None, None) => None,
    }
}

/// Pace the selected HLS input by encoded packet bytes. The second demuxer
/// reads the same multiplexed data for audio and must not be charged again.
fn pace_input(shared: &Shared, bytes: u64, epoch: u64) {
    let mut remaining = bytes as f64 * 8.0;
    let mut playback = shared.state.lock().unwrap();
    while remaining > 0.0
        && playback.epoch == epoch
        && !playback.go_live
        && !shared.cancelled.load(Ordering::Relaxed)
    {
        let bandwidth = playback.options.network_bandwidth as f64;
        if bandwidth == 0.0 || !playback.requested_play {
            break;
        }
        let start = Instant::now();
        let delay = Duration::from_secs_f64((remaining / bandwidth).min(0.1));
        (playback, _) = shared.wake.wait_timeout(playback, delay).unwrap();
        remaining -= start.elapsed().as_secs_f64() * bandwidth;
    }
}

/// Start at the lowest rendition and change one step at a time. Queue
/// underruns drive downshifts; sustained playback with headroom drives upshifts.
struct Adaptive {
    last_change: Instant,
    underruns: u64,
}

impl Adaptive {
    fn choose(
        &mut self,
        now: Instant,
        current: usize,
        count: usize,
        underruns: u64,
        ahead: f64,
    ) -> Option<usize> {
        let elapsed = now.saturating_duration_since(self.last_change);
        let next =
            if current > 0 && underruns != self.underruns && elapsed >= Duration::from_secs(1) {
                current - 1
            } else if current + 1 < count
                && elapsed >= Duration::from_secs(8)
                && ahead >= 0.08
                && underruns == self.underruns
            {
                current + 1
            } else {
                if elapsed >= Duration::from_secs(8) && underruns != self.underruns {
                    self.last_change = now;
                    self.underruns = underruns;
                }
                return None;
            };
        self.underruns = underruns;
        self.last_change = now;
        Some(next)
    }
}

fn decode(path: PathBuf, shared: &Arc<Shared>) -> Result<(), MediaError> {
    let cancel = &shared.cancelled;
    let mut video = VideoDecoder::open_video(&path, cancel.clone())?;
    let mut variant = video.as_ref().map_or(0, |video| video.variants().0);
    let mut adaptive = Adaptive {
        last_change: Instant::now(),
        underruns: 0,
    };
    let track = shared.state.lock().unwrap().options.audio_track;
    let selected = VideoDecoder::open_audio_track(&path, track, variant, cancel.clone());
    let mut audio = match selected {
        Ok(audio) => audio.map(AudioDecoder::new),
        Err(error) if video.is_some() => {
            shared.state.lock().unwrap().state.audio_error = error.to_string();
            None
        }
        Err(error) => return Err(error),
    };
    {
        let mut state = shared.state.lock().unwrap();
        if state.options.audio_track == track {
            state.audio_track_dirty = false;
        }
    }
    if video.is_none() && audio.is_none() {
        return Err(MediaError("no playable audio or video streams".into()));
    }
    let input = video
        .as_ref()
        .or(audio.as_ref().map(|audio| &audio.decoder))
        .unwrap();
    let live = input.is_live();
    let mut origin = input.origin();
    if live && let Some(window) = input.live_window(false) {
        origin += window.start;
    }
    if let Some(video) = &mut video {
        video.set_live_origin(origin);
    }
    if let Some(audio) = &mut audio {
        audio.decoder.set_live_origin(origin);
    }
    let mut device = None;
    {
        let mut state = shared.state.lock().unwrap();
        let input = video
            .as_ref()
            .or(audio.as_ref().map(|audio| &audio.decoder))
            .unwrap();
        state.state.duration = input.duration();
        state.state.live = live;
        if live {
            if let Some(window) = live_window(video.as_ref(), audio.as_ref(), false) {
                state.update_window(window);
                state.reset(window.clamp(window.edge));
            }
        } else {
            state.state.seekable =
                input.seekable() && audio.as_ref().is_none_or(|audio| audio.decoder.seekable());
            state.state.seek_end = input.duration();
        }
        state.state.has_video = video.is_some();
        state.state.has_audio = audio.is_some();
        state.state.audio_tracks = input.audio_tracks();
        state.state.subtitle_tracks = input.subtitle_tracks();
        state.state.title = input.title().into();
        state.state.bitrate = input.variants().2;
        state.video_eof = video.is_none();
        state.audio_eof = audio.is_none();
        state.audio = audio.is_some();
    }
    if audio.is_some() {
        match Device::open(shared.clone()) {
            Ok(output) => device = Some(output),
            Err(error) if video.is_some() => {
                let mut state = shared.state.lock().unwrap();
                state.state.audio_error = error.to_string();
                state.audio = false;
                state.audio_eof = true;
            }
            Err(error) => return Err(error),
        }
    }
    let mut retry_audio = Instant::now() + Duration::from_secs(2);
    let mut next_video = None;
    let mut last_video = -1.0 / 30.0;
    let mut have_preview = false;
    let network = is_network(&path);
    let mut retries = 0u8;
    let mut refresh_at = Instant::now();
    let mut rendition = None;
    let mut subtitle_decoder = None;
    let mut next_cue: Option<subtitles::Cue> = None;
    'decode: loop {
        let mut playback = shared.state.lock().unwrap();
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        if live && Instant::now() >= refresh_at {
            drop(playback);
            let window = live_window(video.as_ref(), audio.as_ref(), true);
            refresh_at = Instant::now() + Duration::from_millis(500);
            playback = shared.state.lock().unwrap();
            if let Some(window) = window {
                if !playback.state.seekable && !have_preview && playback.state.position == 0.0 {
                    origin += window.start;
                    if let Some(video) = &mut video {
                        video.set_live_origin(origin);
                    }
                    if let Some(audio) = &mut audio {
                        audio.decoder.set_live_origin(origin);
                    }
                    playback.reset((window.edge - window.start).max(0.0));
                }
                let window = live_window(video.as_ref(), audio.as_ref(), false).unwrap_or(window);
                playback.update_window(window);
                if playback.requested_play && playback.state.position < window.start {
                    playback.reset(window.start);
                }
            }
        }
        if playback.go_live && playback.state.seekable {
            let window = playback.seek_window();
            playback.go_live = false;
            playback.reset(window.clamp(window.edge));
        }
        if playback.state.playing
            && playback.state.seekable
            && let Some(input) = video
                .as_ref()
                .or(audio.as_ref().map(|audio| &audio.decoder))
        {
            let (_, count, _) = input.variants();
            let video_ahead = playback
                .video
                .back()
                .map_or(0.0, |(pts, _)| *pts - playback.state.position);
            let audio_ahead = playback.audio_blocks.back().map_or(0.0, |block| {
                seconds(block.end().saturating_sub(playback.audio_cursor))
            });
            let ahead = if video.is_some() {
                video_ahead
            } else {
                audio_ahead
            };
            if let Some(next) =
                adaptive.choose(Instant::now(), variant, count, playback.underruns, ahead)
            {
                rendition = Some(next);
                playback.reopen = true;
            }
        }
        if std::mem::take(&mut playback.reopen) {
            playback.subtitle_dirty |= playback.options.subtitle_track >= 0;
            let position = playback.state.position;
            let seekable = playback.state.seekable;
            let track = playback.options.audio_track;
            let target = rendition.take().unwrap_or(variant);
            playback.reset(if seekable { position } else { 0.0 });
            playback.state.status = MediaStatus::Buffering;
            drop(playback);
            let reopened = (|| {
                let video = VideoDecoder::open_video_variant(&path, target, cancel.clone())?;
                let audio = VideoDecoder::open_audio_track(&path, track, target, cancel.clone())?
                    .map(AudioDecoder::new);
                if video.is_none() && audio.is_none() {
                    return Err(MediaError(
                        "media streams disappeared during reconnect".into(),
                    ));
                }
                Ok((video, audio))
            })();
            match reopened {
                Ok((next_video, next_audio)) => {
                    video = next_video;
                    audio = next_audio;
                    variant = target;
                }
                Err(_) if target != variant && !cancel.load(Ordering::Relaxed) => {
                    // Keep decoding the selected rendition if the alternative
                    // playlist or its probe segments are temporarily unavailable.
                }
                Err(error) => return Err(error),
            }
            if let Some(video) = &mut video {
                video.set_live_origin(origin);
            }
            if let Some(audio) = &mut audio {
                audio.decoder.set_live_origin(origin);
            }
            let window = live_window(video.as_ref(), audio.as_ref(), false);
            playback = shared.state.lock().unwrap();
            if let Some(window) = window {
                playback.update_window(window);
            }
            if !seekable {
                playback.request = None;
            }
            playback.video_eof = video.is_none();
            playback.audio_eof = !playback.audio || audio.is_none();
            playback.state.bitrate = video
                .as_ref()
                .or(audio.as_ref().map(|audio| &audio.decoder))
                .map_or(0, |input| input.variants().2);
            have_preview = false;
            next_video = None;
            last_video = playback.state.position - 1.0 / 30.0;
        }
        if playback.stopped && (live || !playback.state.seekable) {
            playback.state.ready = true;
            playback.dirty = true;
            drop(
                shared
                    .wake
                    .wait_timeout(playback, Duration::from_millis(500))
                    .unwrap(),
            );
            continue;
        }
        macro_rules! receive {
            ($frame:expr) => {
                match $frame {
                    Ok(frame) => frame,
                    Err(_) if cancel.load(Ordering::Relaxed) => return Ok(()),
                    Err(_) if network && retries < 2 => {
                        retries += 1;
                        let mut playback = shared.state.lock().unwrap();
                        if variant > 0 {
                            rendition = Some(variant - 1);
                            adaptive.last_change = Instant::now();
                            adaptive.underruns = playback.underruns;
                        }
                        playback.reopen = true;
                        continue 'decode;
                    }
                    Err(error) => return Err(error),
                }
            };
        }
        if std::mem::take(&mut playback.subtitle_dirty) {
            let file = playback.options.subtitle_file.clone();
            let track = playback.options.subtitle_track;
            let position = playback.state.position;
            drop(playback);
            next_cue = None;
            subtitle_decoder = None;
            let (cues, error) = if let Some(file) = &file {
                match subtitles::load(file) {
                    Ok(cues) => (cues, String::new()),
                    Err(error) => (Vec::new(), error),
                }
            } else if track >= 0 {
                match VideoDecoder::open_subtitle_track(&path, track, variant, cancel.clone()) {
                    Ok(Some(mut decoder)) => {
                        decoder.set_live_origin(origin);
                        if position > 0.0 && decoder.seekable() {
                            let _ = decoder.seek((position - 5.0).max(0.0));
                        }
                        subtitle_decoder = Some(decoder);
                        (Vec::new(), String::new())
                    }
                    Ok(None) => (Vec::new(), "subtitle track is unavailable".into()),
                    Err(error) => (Vec::new(), error.to_string()),
                }
            } else {
                (Vec::new(), String::new())
            };
            playback = shared.state.lock().unwrap();
            if playback.options.subtitle_file == file && playback.options.subtitle_track == track {
                playback.subtitles = cues;
                playback.state.subtitle_error = error;
                playback.dirty = true;
            } else {
                subtitle_decoder = None;
            }
        }
        if std::mem::take(&mut playback.audio_track_dirty) && playback.state.seekable {
            let track = playback.options.audio_track;
            drop(playback);
            let selected = VideoDecoder::open_audio_track(&path, track, variant, cancel.clone());
            playback = shared.state.lock().unwrap();
            if playback.options.audio_track == track {
                match selected {
                    Ok(Some(mut selected)) => {
                        selected.set_live_origin(origin);
                        audio = Some(AudioDecoder::new(selected));
                        let position = playback.state.position;
                        playback.reset(position);
                        playback.audio = device.is_some();
                    }
                    Err(error) => {
                        playback.state.audio_error = error.to_string();
                        playback.dirty = true;
                    }
                    Ok(None) => {}
                }
            }
        }
        if playback.state.playing
            && device
                .as_ref()
                .is_some_and(|output| output.last_callback_elapsed() >= Duration::from_secs(2))
        {
            let position = playback.position(Instant::now());
            playback.state.audio_error = "audio output stopped; reconnecting".into();
            playback.state.position = position;
            playback.audio = false;
            playback.audio_eof = true;
            playback.buffering = false;
            playback.submitted.clear();
            playback.anchor = Some((position, Instant::now()));
            drop(playback);
            drop(device.take());
            continue;
        }
        if device.is_none()
            && audio.is_some()
            && playback.state.seekable
            && playback.requested_play
            && Instant::now() >= retry_audio
        {
            drop(playback);
            retry_audio = Instant::now() + Duration::from_secs(2);
            if let Ok(output) = Device::open(shared.clone()) {
                device = Some(output);
                let mut state = shared.state.lock().unwrap();
                let position = state.state.position;
                state.reset(position);
                state.audio = true;
                state.state.audio_error.clear();
            }
            continue;
        }
        let epoch = playback.epoch;
        if let Some(mut position) = playback.request.take() {
            if live {
                let window = live_window(video.as_ref(), audio.as_ref(), false);
                if let Some(window) = window {
                    position = window.clamp(position);
                    playback.state.position = position;
                    playback.audio_cursor = sample_index(position);
                }
            }
            drop(playback);
            if let Some(video) = &mut video {
                video.seek(position)?;
            }
            if let Some(audio) = &mut audio {
                audio.seek(position)?;
            }
            if let Some(decoder) = &mut subtitle_decoder
                && decoder.seekable()
            {
                let _ = decoder.seek((position - 5.0).max(0.0));
                next_cue = None;
                shared.state.lock().unwrap().subtitles.clear();
            }
            let mut state = shared.state.lock().unwrap();
            state.video_eof = video.is_none();
            state.audio_eof = !state.audio;
            drop(state);
            have_preview = false;
            next_video = None;
            last_video = position - 1.0 / 30.0;
            continue;
        }
        let target = playback.state.position;
        let fill_subtitle = subtitle_decoder.is_some()
            && next_cue
                .as_ref()
                .is_none_or(|cue| cue.start <= target + 0.5);
        let fill_video = video.is_some()
            && !playback.video_eof
            && (!have_preview
                || (playback.video.len() < 8
                    && playback
                        .video
                        .iter()
                        .map(|(_, frame)| frame.rgba().len())
                        .sum::<usize>()
                        < VIDEO_BYTES_AHEAD
                    && playback
                        .video
                        .back()
                        .is_none_or(|frame| frame.0 < target + VIDEO_AHEAD)));
        let fill_audio = playback.audio
            && audio.is_some()
            && !playback.audio_eof
            && playback
                .audio_blocks
                .back()
                .map_or(0, |block| block.end().saturating_sub(playback.audio_cursor))
                < AUDIO_AHEAD;
        if !fill_video && !fill_audio && !fill_subtitle {
            if live || playback.requested_play && audio.is_some() {
                drop(
                    shared
                        .wake
                        .wait_timeout(playback, Duration::from_millis(400))
                        .unwrap(),
                );
            } else {
                drop(shared.wake.wait(playback).unwrap());
            }
            continue;
        }
        drop(playback);

        if fill_subtitle {
            let cue = next_cue.take().map_or_else(
                || subtitle_decoder.as_mut().unwrap().next_cue(),
                |cue| Ok(Some(cue)),
            );
            let mut state = shared.state.lock().unwrap();
            if let Ok(Some(cue)) = cue {
                if cue.start > target + 0.5 {
                    next_cue = Some(cue);
                } else if state.epoch == epoch {
                    state.subtitles.retain(|cue| cue.end > target);
                    if cue.end > target {
                        state.subtitles.push(cue);
                        state
                            .subtitles
                            .sort_by(|left, right| left.start.total_cmp(&right.start));
                        state.dirty = true;
                    }
                }
            } else {
                if let Err(error) = cue {
                    state.state.subtitle_error = error.to_string();
                    state.dirty = true;
                }
                subtitle_decoder = None;
            }
        }

        if fill_audio {
            let block = receive!(audio.as_mut().unwrap().next());
            if network && video.is_none() {
                pace_input(
                    shared,
                    audio.as_mut().unwrap().decoder.take_packet_bytes(),
                    epoch,
                );
            }
            let mut playback = shared.state.lock().unwrap();
            if playback.epoch == epoch {
                if let Some(block) = block {
                    if block.end() > playback.audio_cursor {
                        playback.audio_blocks.push_back(block);
                    }
                } else {
                    playback.audio_eof = true;
                }
            }
        }
        if fill_video {
            let frame = if let Some(frame) = next_video.take() {
                Some(frame)
            } else {
                receive!(video.as_mut().unwrap().next_frame())
            };
            if network {
                pace_input(shared, video.as_mut().unwrap().take_packet_bytes(), epoch);
            }
            let mut playback = shared.state.lock().unwrap();
            if playback.epoch != epoch {
                continue;
            }
            if let Some(frame) = frame {
                let timestamp = frame
                    .timestamp_seconds
                    .unwrap_or(last_video + 1.0 / 30.0)
                    .max(0.0);
                last_video = timestamp;
                if live && !have_preview && timestamp > target {
                    drop(playback);
                    let image = video_frame(frame)?;
                    let mut playback = shared.state.lock().unwrap();
                    if playback.epoch == epoch {
                        playback.state.frame = Some(image);
                        have_preview = true;
                    }
                } else if !have_preview && timestamp <= target {
                    // Keep the latest frame at or before the seek target.
                    drop(playback);
                    let mut selected = frame;
                    loop {
                        let Some(candidate) = receive!(video.as_mut().unwrap().next_frame()) else {
                            break;
                        };
                        // Seek preroll does not feed the playback queues.
                        video.as_mut().unwrap().take_packet_bytes();
                        let time = candidate
                            .timestamp_seconds
                            .unwrap_or(last_video + 1.0 / 30.0);
                        if time > target {
                            next_video = Some(candidate);
                            break;
                        }
                        last_video = time;
                        selected = candidate;
                        let state = shared.state.lock().unwrap();
                        if cancel.load(Ordering::Relaxed) {
                            return Ok(());
                        }
                        if state.epoch != epoch {
                            break;
                        }
                    }
                    let image = video_frame(selected)?;
                    let mut playback = shared.state.lock().unwrap();
                    if playback.epoch == epoch {
                        playback.state.frame = Some(image);
                        have_preview = true;
                    }
                } else {
                    drop(playback);
                    let image = video_frame(frame)?;
                    let mut playback = shared.state.lock().unwrap();
                    if playback.epoch != epoch {
                        continue;
                    }
                    playback.video.push_back((timestamp, image));
                    have_preview = true;
                }
            } else {
                playback.video_eof = true;
            }
        }
        let mut playback = shared.state.lock().unwrap();
        if playback.epoch == epoch
            && (have_preview || playback.video_eof)
            && (!playback.audio || !playback.audio_blocks.is_empty() || playback.audio_eof)
        {
            playback.state.ready = true;
            if !playback.buffering {
                playback.state.buffer_progress = 1.0;
            }
            if playback.state.duration > 0.0 && playback.state.position >= playback.state.duration {
                playback.state.ended = true;
            }
        }
    }
}

struct AudioDecoder {
    decoder: VideoDecoder,
    resample: *mut ff::SwrContext,
    cursor: u64,
    initialized: bool,
}

impl AudioDecoder {
    fn new(decoder: VideoDecoder) -> Self {
        Self {
            decoder,
            resample: ptr::null_mut(),
            cursor: 0,
            initialized: false,
        }
    }

    fn seek(&mut self, position: f64) -> Result<(), MediaError> {
        self.decoder.seek(position)?;
        unsafe { ((api().swr_free).unwrap())(&mut self.resample) };
        self.initialized = false;
        self.cursor = sample_index(position);
        Ok(())
    }

    fn next(&mut self) -> Result<Option<AudioBlock>, MediaError> {
        loop {
            let has_frame = self.decoder.receive()?;
            if !has_frame && self.resample.is_null() {
                return Ok(None);
            }
            let frame = has_frame.then(|| self.decoder.frame());
            unsafe {
                if let Some(frame) = frame
                    && self.resample.is_null()
                {
                    let mut layout = std::mem::zeroed();
                    ((api().av_channel_layout_default).unwrap())(&mut layout, CHANNELS as i32);
                    check(
                        ((api().swr_alloc_set_opts2).unwrap())(
                            &mut self.resample,
                            &layout,
                            ff::AVSampleFormat_AV_SAMPLE_FMT_FLT,
                            RATE as i32,
                            &frame.ch_layout,
                            frame.format as ff::AVSampleFormat,
                            frame.sample_rate,
                            0,
                            ptr::null_mut(),
                        ),
                        "configure audio resampler",
                    )?;
                    check(
                        ((api().swr_init).unwrap())(self.resample),
                        "start audio resampler",
                    )?;
                }
                let count = frame.map_or(0, |frame| frame.nb_samples);
                let capacity = ((api().swr_get_out_samples).unwrap())(self.resample, count);
                check(capacity, "size audio buffer")?;
                if capacity > RATE as i32 * 2 {
                    return Err(MediaError("excessive audio frame size".into()));
                }
                let mut samples = vec![0.0f32; capacity as usize * CHANNELS];
                let mut output = samples.as_mut_ptr().cast();
                let input = frame
                    .map_or(ptr::null_mut(), |frame| frame.extended_data)
                    .cast();
                let written = ((api().swr_convert).unwrap())(
                    self.resample,
                    &mut output,
                    capacity,
                    input,
                    count,
                );
                check(written, "resample audio")?;
                if let Some(timestamp) = frame.and_then(|_| self.decoder.timestamp()) {
                    let index = sample_index(timestamp);
                    if !self.initialized || index.abs_diff(self.cursor) > RATE as u64 / 10 {
                        self.cursor = index;
                    }
                }
                self.initialized = true;
                if written == 0 {
                    if has_frame {
                        continue;
                    }
                    return Ok(None);
                }
                samples.truncate(written as usize * CHANNELS);
                let start = self.cursor;
                self.cursor += written as u64;
                return Ok(Some(AudioBlock { start, samples }));
            }
        }
    }
}

impl Drop for AudioDecoder {
    fn drop(&mut self) {
        unsafe { ((api().swr_free).unwrap())(&mut self.resample) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_path(suffix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("xrack-{}-{suffix}.mp4", std::process::id()));
        std::fs::write(&path, include_bytes!("../tests/sync.mp4")).unwrap();
        path
    }

    #[test]
    fn adaptive_hls_uses_headroom_and_underruns() {
        let now = Instant::now();
        let mut policy = Adaptive {
            last_change: now,
            underruns: 0,
        };
        assert_eq!(
            policy.choose(now + Duration::from_secs(1), 0, 3, 0, 0.2),
            None
        );
        assert_eq!(
            policy.choose(now + Duration::from_secs(8), 0, 3, 0, 0.2),
            Some(1)
        );
        assert_eq!(
            policy.choose(now + Duration::from_secs(10), 1, 3, 1, 0.0),
            Some(0)
        );
        assert_eq!(
            policy.choose(now + Duration::from_secs(16), 0, 3, 1, 0.2),
            None
        );
    }

    #[test]
    fn audio_device_timeline_drives_video_and_seek_discards_old_audio() {
        let now = Instant::now();
        let mut playback = Playback {
            state: MediaState {
                ready: true,
                playing: true,
                ..MediaState::default()
            },
            requested_play: true,
            volume: 0.25,
            audio: true,
            audio_blocks: VecDeque::from([AudioBlock {
                start: 480,
                samples: vec![1.0; 960],
            }]),
            ..Playback::default()
        };
        let mut output = vec![0.0; 1920];
        playback.fill_audio(&mut output, 480, now);
        assert!(output[..960].iter().all(|value| *value == 0.0));
        assert!(output[960..].iter().all(|value| *value == 0.25));
        assert_eq!(playback.position(now), 0.0);
        assert!((playback.position(now + Duration::from_millis(25)) - 0.015).abs() < 1e-5);
        playback.reset(1.5);
        assert!(playback.audio_blocks.is_empty());
        assert_eq!(playback.position(now), 1.5);
        playback.fill_audio(&mut output, 480, now);
        assert!(playback.submitted.is_empty());
    }

    #[test]
    fn decodes_synchronized_h264_and_aac_and_seeks_both_tracks() {
        let fixture = fixture_path("decode");
        let mut video = VideoDecoder::open(&fixture).unwrap();
        let mut audio = AudioDecoder::new(VideoDecoder::open_audio(&fixture).unwrap().unwrap());
        assert!((video.duration() - 3.0).abs() < 0.03);
        let first = video.next_frame().unwrap().unwrap();
        assert!(first.rgba[(45 * 160 + 80) * 4 + 1] > 180);
        let block = audio.next().unwrap().unwrap();
        assert!(block.start < 2048);
        assert!(block.samples.iter().any(|sample| sample.abs() > 0.02));
        video.seek(2.0).unwrap();
        audio.seek(2.0).unwrap();
        let mut frame = video.next_frame().unwrap().unwrap();
        while frame.timestamp_seconds.unwrap() < 2.0 {
            frame = video.next_frame().unwrap().unwrap();
        }
        assert!(frame.timestamp_seconds.unwrap() < 2.06);
        let mut block = audio.next().unwrap().unwrap();
        while block.end() < sample_index(2.0) {
            block = audio.next().unwrap().unwrap();
        }
        assert!(seconds(block.start) < 2.03);
        drop((video, audio));
        std::fs::remove_file(fixture).unwrap();
    }

    #[test]
    fn plays_audio_only_file_and_exposes_status() {
        let path = std::env::temp_dir().join(format!("xrack-audio-{}.wav", std::process::id()));
        let mut wav = b"RIFF".to_vec();
        let samples = 24_000_u32;
        let bytes = samples * 2;
        wav.extend((36 + bytes).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16_u32.to_le_bytes());
        wav.extend(1_u16.to_le_bytes());
        wav.extend(1_u16.to_le_bytes());
        wav.extend(RATE.to_le_bytes());
        wav.extend((RATE * 2).to_le_bytes());
        wav.extend(2_u16.to_le_bytes());
        wav.extend(16_u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(bytes.to_le_bytes());
        wav.resize(44 + bytes as usize, 0);
        std::fs::write(&path, wav).unwrap();
        let mut player = open(&path, true, 0.0).unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            let state = player.sample(Instant::now());
            assert!(state.error.is_empty(), "{state:?}");
            if state.position > 0.05 {
                assert!(
                    state.has_audio && !state.has_video && state.seekable && state.frame.is_none()
                );
                assert_eq!(state.audio_tracks, 1);
                assert_eq!(state.status, MediaStatus::Playing);
                break;
            }
            assert!(Instant::now() < until, "{state:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
        player.command(MediaCommand::Stop);
        assert_eq!(player.sample(Instant::now()).position, 0.0);
        drop(player);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn audio_backend_advances_the_session_and_preserves_pause_and_seek() {
        let fixture = fixture_path("device");
        let mut session = open(&fixture, true, 0.0).unwrap();
        let wait = |session: &mut dyn MediaSession, condition: fn(&MediaState) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(4);
            loop {
                let now = Instant::now();
                let state = session.sample(now);
                assert!(state.error.is_empty(), "{}", state.error);
                if condition(&state) {
                    return state;
                }
                assert!(now < deadline, "playback timeout: {state:?}");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let before = wait(session.as_mut(), |state| {
            state.playing && state.position > 0.07
        });
        assert!(before.frame.is_some());
        session.command(MediaCommand::Pause);
        let paused = wait(session.as_mut(), |state| state.ready && !state.playing);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(session.sample(Instant::now()).position, paused.position);
        session.command(MediaCommand::Seek(2.1));
        let sought = wait(session.as_mut(), |state| {
            state.ready && state.position == 2.1
        });
        assert!(!sought.playing);
        session.command(MediaCommand::Play);
        wait(session.as_mut(), |state| state.position > 2.2);
        let after_flash = wait(session.as_mut(), |state| state.position > 2.55);
        let green = |frame: Option<MediaFrame>| frame.unwrap().rgba()[(45 * 160 + 80) * 4 + 1];
        assert!(
            green(before.frame) > 180,
            "flash must be presented against the audio clock"
        );
        assert!(
            green(after_flash.frame) < 100,
            "the post-seek clock must present the dark frame"
        );
        drop(session);
        std::fs::remove_file(fixture).unwrap();
    }
}
