//! Predecoded WAV effects sharing one miniaudio output per UI thread.
use crate::{
    VideoDecoder, native,
    player::{AudioDecoder, CHANNELS, RATE},
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    ffi::c_void,
    fs::File,
    io::Read,
    path::Path,
    rc::{Rc, Weak},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Instant,
};
use xrack_runtime::{MediaCommand, MediaSession, MediaState, MediaStatus};

const MAX_SAMPLES: usize = 10 * RATE as usize * CHANNELS;

type Voices = Mutex<BTreeMap<u64, Voice>>;

struct Voice {
    samples: Arc<[f32]>,
    cursor: usize,
    volume: f32,
}

impl Voice {
    fn mix(&mut self, output: &mut [f32]) {
        let remaining = &self.samples[self.cursor..];
        let count = remaining.len().min(output.len());
        for (dest, &sample) in output.iter_mut().zip(&remaining[..count]) {
            *dest += sample * self.volume;
        }
        self.cursor += count;
    }

    fn playing(&self) -> bool {
        self.cursor < self.samples.len()
    }
}

unsafe extern "C" fn render(user: *mut c_void, data: *mut f32, frames: u32, _delay: u32) {
    let output = unsafe { std::slice::from_raw_parts_mut(data, frames as usize * CHANNELS) };
    output.fill(0.0);
    let voices = unsafe { &*user.cast::<Voices>() };
    if let Ok(mut voices) = voices.try_lock() {
        for voice in voices.values_mut() {
            voice.mix(output);
        }
        for sample in output {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }
}

struct Mixer {
    voices: Arc<Voices>,
    next_id: Cell<u64>,
    ready: Arc<OnceLock<Result<(), String>>>,
    _lifetime: mpsc::Sender<()>,
}

thread_local! {
    static MIXER: RefCell<Weak<Mixer>> = const { RefCell::new(Weak::new()) };
}

impl Mixer {
    fn current() -> Result<Rc<Self>, String> {
        MIXER.with_borrow_mut(|cached| {
            if let Some(mixer) = cached.upgrade() {
                return Ok(mixer);
            }
            let (lifetime, shutdown) = mpsc::channel();
            let mixer = Rc::new(Self {
                voices: Arc::default(),
                next_id: Cell::new(0),
                ready: Arc::default(),
                _lifetime: lifetime,
            });
            let voices = mixer.voices.clone();
            let ready = mixer.ready.clone();
            std::thread::Builder::new()
                .name("xrack-sound-output".into())
                .spawn(move || {
                    if let Err(error) = native::load() {
                        let _ = ready.set(Err(error.to_owned()));
                        return;
                    }
                    // The device is closed on this thread before its callback state is released.
                    let api = native::api();
                    let handle = unsafe {
                        (api.xr_audio_open.unwrap())(
                            Some(render),
                            Arc::as_ptr(&voices).cast_mut().cast(),
                        )
                    };
                    if handle.is_null() {
                        let _ = ready.set(Err("no audio output device is available".into()));
                        return;
                    }
                    let _ = ready.set(Ok(()));
                    let _ = shutdown.recv();
                    unsafe { (api.xr_audio_close.unwrap())(handle) };
                })
                .map_err(|error| error.to_string())?;
            *cached = Rc::downgrade(&mixer);
            Ok(mixer)
        })
    }
}

type Samples = Result<Arc<[f32]>, String>;

struct Sound {
    mixer: Rc<Mixer>,
    id: u64,
    volume: f32,
    preload: Option<mpsc::Receiver<Samples>>,
    samples: Option<Samples>,
    play_pending: bool,
    cancelled: Arc<AtomicBool>,
    state: MediaState,
}

pub(super) fn open(path: &Path, volume: f64) -> Result<Box<dyn MediaSession>, String> {
    let mixer = Mixer::current()?;
    let id = mixer.next_id.get();
    mixer.next_id.set(id + 1);
    let (loaded, preload) = mpsc::sync_channel(1);
    let path = path.to_owned();
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = cancelled.clone();
    std::thread::Builder::new()
        .name("xrack-sound-decode".into())
        .spawn(move || {
            let _ = loaded.send(decode(&path, worker_cancelled));
        })
        .map_err(|error| error.to_string())?;
    Ok(Box::new(Sound {
        mixer,
        id,
        volume: volume as f32,
        preload: Some(preload),
        samples: None,
        play_pending: false,
        cancelled,
        state: MediaState {
            status: MediaStatus::Loading,
            ..MediaState::default()
        },
    }))
}

fn decode(path: &Path, cancelled: Arc<AtomicBool>) -> Samples {
    let metadata = path.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > 16 * 1024 * 1024 {
        return Err("SoundEffect requires a local WAV file of up to 16 MiB".into());
    }
    let mut header = [0; 12];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|error| error.to_string())?;
    if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
        return Err("SoundEffect requires a local WAV file".into());
    }
    let decoder = VideoDecoder::open_audio_track(path, 0, 0, cancelled.clone())
        .map_err(|error| error.to_string())?
        .ok_or("WAV has no audio stream")?;
    let mut decoder = AudioDecoder::new(decoder);
    let mut samples = Vec::new();
    while !cancelled.load(Ordering::Relaxed) {
        let Some(block) = decoder.next().map_err(|error| error.to_string())? else {
            return if samples.is_empty() {
                Err("WAV has no audio samples".into())
            } else {
                Ok(samples.into())
            };
        };
        if samples.len() + block.samples.len() > MAX_SAMPLES {
            return Err("SoundEffect supports WAV clips of up to 10 seconds".into());
        }
        samples.extend(block.samples);
    }
    Err("sound preload cancelled".into())
}

impl Drop for Sound {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.mixer.voices.lock().unwrap().remove(&self.id);
    }
}

impl MediaSession for Sound {
    fn command(&mut self, command: MediaCommand) {
        if command == MediaCommand::Play && self.state.status != MediaStatus::Failed {
            self.play_pending = true;
        }
    }

    fn set_volume(&mut self, volume: f64) {
        self.volume = volume as f32;
        if let Some(voice) = self.mixer.voices.lock().unwrap().get_mut(&self.id) {
            voice.volume = self.volume;
        }
    }

    fn sample(&mut self, _now: Instant) -> MediaState {
        if let Some(preload) = &self.preload {
            self.samples = match preload.try_recv() {
                Ok(samples) => Some(samples),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("sound preload failed".into())),
            };
            if self.samples.is_some() {
                self.preload = None;
            }
        }
        let error = self
            .samples
            .as_ref()
            .and_then(|result| result.as_ref().err())
            .or_else(|| {
                self.mixer
                    .ready
                    .get()
                    .and_then(|result| result.as_ref().err())
            });
        if let Some(error) = error {
            self.state.error.clone_from(error);
            self.state.status = MediaStatus::Failed;
            self.play_pending = false;
        } else if let Some(Ok(samples)) = &self.samples
            && matches!(self.mixer.ready.get(), Some(Ok(())))
        {
            self.state.ready = true;
            let mut voices = self.mixer.voices.lock().unwrap();
            if std::mem::take(&mut self.play_pending) {
                voices.insert(
                    self.id,
                    Voice {
                        samples: samples.clone(),
                        cursor: 0,
                        volume: self.volume,
                    },
                );
            }
            self.state.playing = voices.get(&self.id).is_some_and(Voice::playing);
            self.state.status = if self.state.playing {
                MediaStatus::Playing
            } else {
                MediaStatus::Stopped
            };
        }
        self.state.clone()
    }

    fn active(&self) -> bool {
        // Keep polling until sample() has published the device and playback state.
        self.state.status == MediaStatus::Loading || self.play_pending || self.state.playing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preloads_mono_pcm_and_float_wav_formats() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("effect.wav");
        for (format, sample) in [
            (1u16, &[160u8][..]),
            (1, &8192i16.to_le_bytes()),
            (1, &[0, 0, 32]),
            (1, &536_870_912i32.to_le_bytes()),
            (3, &0.25f32.to_le_bytes()),
            (3, &0.25f64.to_le_bytes()),
        ] {
            let frames = 480;
            let sample_size = sample.len() as u16;
            let data_size = frames * u32::from(sample_size);
            let mut wav = b"RIFF".to_vec();
            wav.extend((36 + data_size).to_le_bytes());
            wav.extend(b"WAVEfmt ");
            wav.extend(16u32.to_le_bytes());
            wav.extend(format.to_le_bytes());
            wav.extend(1u16.to_le_bytes()); // Mono.
            wav.extend(RATE.to_le_bytes());
            wav.extend((RATE * u32::from(sample_size)).to_le_bytes());
            wav.extend(sample_size.to_le_bytes());
            wav.extend((sample_size * 8).to_le_bytes());
            wav.extend(b"data");
            wav.extend(data_size.to_le_bytes());
            wav.extend(sample.repeat(frames as usize));
            std::fs::write(&path, wav).unwrap();

            let samples = decode(&path, Arc::default()).unwrap_or_else(|error| {
                panic!("WAV format {format}, {sample_size} bytes: {error}")
            });
            assert_eq!(samples.len(), frames as usize * CHANNELS);
            assert!(samples.iter().all(|sample| *sample > 0.1 && *sample < 0.3));
        }
    }

    #[test]
    fn voices_mix_with_independent_gain_and_replay_from_the_start() {
        let mut confirmation = Voice {
            samples: [0.5, 0.75, -0.5, 0.25].into(),
            cursor: 0,
            volume: 0.5,
        };
        let mut notification = Voice {
            samples: [-0.5, 0.5].into(),
            cursor: 0,
            volume: 0.25,
        };
        let mut output = [0.0; 2];
        confirmation.mix(&mut output);
        notification.mix(&mut output);
        assert_eq!(output, [0.125, 0.5]);
        assert!(confirmation.playing());
        assert!(!notification.playing());
        output.fill(0.0);
        confirmation.mix(&mut output);
        assert_eq!(output, [-0.25, 0.125]);
        assert!(!confirmation.playing());
        confirmation.cursor = 0;
        output.fill(0.0);
        confirmation.mix(&mut output);
        assert_eq!(output, [0.25, 0.375]);
    }
}
