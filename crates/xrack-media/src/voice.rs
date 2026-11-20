//! Desktop voice input through FFmpeg's device interface and bounded PCM output.

pub use crate::MediaError;
use crate::{
    VideoDecoder,
    native::AudioDevice,
    player::{AudioDecoder, CHANNELS, RATE},
};
use std::{
    collections::VecDeque,
    ffi::{CStr, CString, c_void},
    sync::{
        Arc, Condvar, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

pub const SAMPLE_RATE: u32 = RATE;
pub const SAMPLES_PER_FRAME: usize = SAMPLE_RATE as usize / 100;
const QUEUE_FRAMES: usize = 12;

/// Ten milliseconds of normalized mono PCM at 48 kHz.
#[derive(Clone, Debug)]
pub struct VoiceFrame {
    /// Gaps indicate frames dropped to keep capture latency bounded.
    pub sequence: u64,
    pub samples: [f32; SAMPLES_PER_FRAME],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureStatus {
    Opening,
    Capturing,
    Stopping,
    Stopped,
    Failed(String),
}

impl CaptureStatus {
    fn is_reading(&self) -> bool {
        matches!(self, Self::Opening | Self::Capturing)
    }

    fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped | Self::Failed(_))
    }
}

struct CaptureData {
    status: CaptureStatus,
    frames: VecDeque<VoiceFrame>,
}

struct CaptureQueue {
    data: Mutex<CaptureData>,
    changed: Condvar,
    ready: Notify,
}

impl CaptureQueue {
    fn notify_readers(&self) {
        self.changed.notify_all();
        self.ready.notify_waiters();
    }

    fn set_status(&self, status: CaptureStatus) {
        let mut data = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        data.status = match (&data.status, status) {
            (CaptureStatus::Stopping, CaptureStatus::Capturing) => return,
            (CaptureStatus::Stopping, CaptureStatus::Failed(_)) => CaptureStatus::Stopped,
            (_, status) => status,
        };
        drop(data);
        self.notify_readers();
    }

    fn push(&self, frame: VoiceFrame, cancelled: &AtomicBool) {
        let mut data = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if cancelled.load(Ordering::Relaxed) {
            return;
        }
        if data.frames.len() == QUEUE_FRAMES {
            data.frames.pop_front();
        }
        data.frames.push_back(frame);
        drop(data);
        self.notify_readers();
    }
}

/// A microphone owned by its capture thread. Dropping the handle requests shutdown.
pub struct VoiceCapture {
    queue: Arc<CaptureQueue>,
    cancelled: Arc<AtomicBool>,
}

impl VoiceCapture {
    /// Start opening a desktop microphone without blocking the calling thread.
    /// Pass a device name accepted by AVFoundation, DirectShow or ALSA, or `None`.
    pub fn start(device: Option<&str>) -> Result<Self, MediaError> {
        let (format, source) = microphone_source(device)?;
        Self::start_source(format, source)
    }

    fn opening() -> Self {
        Self {
            queue: Arc::new(CaptureQueue {
                data: Mutex::new(CaptureData {
                    status: CaptureStatus::Opening,
                    frames: VecDeque::with_capacity(QUEUE_FRAMES),
                }),
                changed: Condvar::new(),
                ready: Notify::new(),
            }),
            cancelled: Arc::default(),
        }
    }

    fn start_source(format: &'static CStr, source: Option<CString>) -> Result<Self, MediaError> {
        let capture = Self::opening();
        let queue = capture.queue.clone();
        let cancelled = capture.cancelled.clone();
        std::thread::Builder::new()
            .name("xrack-microphone".into())
            .spawn(move || {
                let result = capture_voice(format, source.as_deref(), &queue, &cancelled);
                let status = match result {
                    Err(error) if !cancelled.load(Ordering::Relaxed) => {
                        CaptureStatus::Failed(error.to_string())
                    }
                    _ => CaptureStatus::Stopped,
                };
                queue.set_status(status);
            })
            .map_err(|error| MediaError(format!("start voice capture thread: {error}")))?;
        Ok(capture)
    }

    pub fn status(&self) -> CaptureStatus {
        self.queue
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .status
            .clone()
    }

    /// Take the oldest available frame without waiting.
    pub fn try_frame(&self) -> Option<VoiceFrame> {
        self.wait_frame(Duration::ZERO)
    }

    /// Receive the oldest available frame without blocking the executor.
    ///
    /// After queued frames are drained, returns `Ok(None)` on shutdown or the
    /// capture error on failure. Calling [`Self::stop`] discards queued frames
    /// and wakes pending receivers immediately. Dropping a pending receive does
    /// not consume a frame. Each frame is delivered to at most one receiver.
    pub async fn recv(&self) -> Result<Option<VoiceFrame>, MediaError> {
        loop {
            // Register before inspecting the queue so a push or stop racing
            // with this inspection also wakes the future's first poll.
            let ready = self.queue.ready.notified();
            {
                let mut data = self
                    .queue
                    .data
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if self.cancelled.load(Ordering::Relaxed) {
                    return Ok(None);
                }
                if let Some(frame) = data.frames.pop_front() {
                    return Ok(Some(frame));
                }
                match &data.status {
                    CaptureStatus::Opening | CaptureStatus::Capturing => {}
                    CaptureStatus::Stopping | CaptureStatus::Stopped => return Ok(None),
                    CaptureStatus::Failed(reason) => return Err(MediaError(reason.clone())),
                }
            }
            ready.await;
        }
    }

    /// Wait on a worker thread for a frame, the timeout, or the end of capture.
    pub fn wait_frame(&self, timeout: Duration) -> Option<VoiceFrame> {
        let data = self
            .queue
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut data, _) = self
            .queue
            .changed
            .wait_timeout_while(data, timeout, |data| {
                data.frames.is_empty()
                    && data.status.is_reading()
                    && !self.cancelled.load(Ordering::Relaxed)
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.cancelled.load(Ordering::Relaxed) {
            return None;
        }
        data.frames.pop_front()
    }

    /// Request shutdown immediately. The worker releases the device before publishing `Stopped`.
    pub fn stop(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        let mut data = self
            .queue
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if data.status.is_reading() {
            data.status = CaptureStatus::Stopping;
        }
        data.frames.clear();
        drop(data);
        self.queue.notify_readers();
    }

    /// Wait on a worker thread for capture to release the device.
    /// Returns false if the timeout expires before capture stops or fails.
    pub fn wait_stopped(&self, timeout: Duration) -> bool {
        let data = self
            .queue
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (data, _) = self
            .queue
            .changed
            .wait_timeout_while(data, timeout, |data| !data.status.is_terminal())
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        data.status.is_terminal()
    }
}

impl Drop for VoiceCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

fn microphone_source(device: Option<&str>) -> Result<(&'static CStr, Option<CString>), MediaError> {
    if device.is_some_and(|name| name.is_empty() || name.contains('\0')) {
        return Err(MediaError(
            "voice device name must be nonempty and contain no NUL".into(),
        ));
    }
    if cfg!(any(target_os = "macos", target_os = "windows"))
        && device.is_some_and(|name| name.contains(':'))
    {
        return Err(MediaError(
            "voice device names may not contain the FFmpeg device separator ':'".into(),
        ));
    }
    let (format, source) = if cfg!(target_os = "macos") {
        (
            c"avfoundation",
            Some(format!("none:{}", device.unwrap_or("default"))),
        )
    } else if cfg!(target_os = "windows") {
        (c"dshow", device.map(|name| format!("audio={name}")))
    } else if cfg!(target_os = "linux") {
        (c"alsa", Some(device.unwrap_or("default").to_owned()))
    } else {
        return Err(MediaError(
            "voice capture is currently available on desktop platforms".into(),
        ));
    };
    let source = source
        .map(CString::new)
        .transpose()
        .map_err(|error| MediaError(error.to_string()))?;
    Ok((format, source))
}

fn capture_voice(
    format: &CStr,
    source: Option<&CStr>,
    queue: &CaptureQueue,
    cancelled: &Arc<AtomicBool>,
) -> Result<(), MediaError> {
    if cancelled.load(Ordering::Relaxed) {
        return Ok(());
    }
    let decoder = VideoDecoder::open_voice(format, source, cancelled.clone())?;
    let mut decoder = AudioDecoder::with_channels(decoder, 1);
    queue.set_status(CaptureStatus::Capturing);
    let mut samples = [0.0; SAMPLES_PER_FRAME];
    let mut filled = 0;
    let mut sequence = 0;
    while !cancelled.load(Ordering::Relaxed) {
        let Some(block) = decoder.next()? else { break };
        let mut remaining = block.samples.as_slice();
        while !remaining.is_empty() {
            let count = remaining.len().min(SAMPLES_PER_FRAME - filled);
            samples[filled..filled + count].copy_from_slice(&remaining[..count]);
            filled += count;
            remaining = &remaining[count..];
            if filled == SAMPLES_PER_FRAME {
                queue.push(VoiceFrame { sequence, samples }, cancelled);
                sequence += 1;
                filled = 0;
            }
        }
    }
    Ok(())
}

/// Plays voice frames on the desktop output device. Keep this handle alive while playing.
pub struct VoicePlayback {
    device: AudioDevice<Mutex<VecDeque<f32>>>,
}

/// A voice output queue that can be used by the application's transport worker.
#[derive(Clone)]
pub struct VoicePlaybackSender(Weak<Mutex<VecDeque<f32>>>);

impl VoicePlayback {
    /// Open the default output device on the thread that will own it.
    pub fn open() -> Result<Self, MediaError> {
        let queue = Arc::new(Mutex::new(VecDeque::with_capacity(
            QUEUE_FRAMES * SAMPLES_PER_FRAME,
        )));
        Ok(Self {
            device: AudioDevice::open(queue, render_voice)?,
        })
    }

    pub fn sender(&self) -> VoicePlaybackSender {
        VoicePlaybackSender(Arc::downgrade(&self.device.state))
    }
}

impl VoicePlaybackSender {
    /// Add one frame. Returns false when the playback device has closed.
    pub fn push(&self, frame: &VoiceFrame) -> bool {
        let Some(queue) = self.0.upgrade() else {
            return false;
        };
        let mut samples = queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let excess =
            (samples.len() + SAMPLES_PER_FRAME).saturating_sub(QUEUE_FRAMES * SAMPLES_PER_FRAME);
        samples.drain(..excess);
        samples.extend(frame.samples.iter().map(|sample| {
            if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        }));
        true
    }
}

unsafe extern "C" fn render_voice(user: *mut c_void, output: *mut f32, frames: u32, _delay: u32) {
    // AudioDevice keeps the queue alive and miniaudio supplies interleaved stereo.
    let queue = unsafe { &*user.cast::<Mutex<VecDeque<f32>>>() };
    let output = unsafe { std::slice::from_raw_parts_mut(output, frames as usize * CHANNELS) };
    let Ok(mut queue) = queue.try_lock() else {
        output.fill(0.0);
        return;
    };
    for stereo in output.as_chunks_mut::<CHANNELS>().0 {
        stereo.fill(queue.pop_front().unwrap_or(0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        io::Write,
        task::{Context, Poll, Wake, Waker},
    };

    struct ReceiveWake(AtomicBool);

    impl Wake for ReceiveWake {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    #[test]
    fn async_receive_wakes_on_frame_failure_and_explicit_stop() {
        let capture = VoiceCapture::opening();
        let notification = Arc::new(ReceiveWake(AtomicBool::new(false)));
        let waker = Waker::from(notification.clone());
        let mut cx = Context::from_waker(&waker);
        let mut discarded = Box::pin(capture.recv());
        assert!(discarded.as_mut().poll(&mut cx).is_pending());
        drop(discarded);

        let mut receive = Box::pin(capture.recv());
        assert!(receive.as_mut().poll(&mut cx).is_pending());
        capture.queue.push(
            VoiceFrame {
                sequence: 3,
                samples: [0.25; SAMPLES_PER_FRAME],
            },
            &capture.cancelled,
        );
        assert!(notification.0.swap(false, Ordering::Relaxed));
        assert!(matches!(
            receive.as_mut().poll(&mut cx),
            Poll::Ready(Ok(Some(VoiceFrame { sequence: 3, .. })))
        ));

        let mut receive = Box::pin(capture.recv());
        assert!(receive.as_mut().poll(&mut cx).is_pending());
        capture
            .queue
            .set_status(CaptureStatus::Failed("device disconnected".into()));
        assert!(notification.0.swap(false, Ordering::Relaxed));
        assert!(matches!(
            receive.as_mut().poll(&mut cx),
            Poll::Ready(Err(MediaError(reason))) if reason == "device disconnected"
        ));

        let capture = VoiceCapture::opening();
        let mut receive = Box::pin(capture.recv());
        assert!(receive.as_mut().poll(&mut cx).is_pending());
        capture.stop();
        assert!(notification.0.load(Ordering::Relaxed));
        assert!(matches!(
            receive.as_mut().poll(&mut cx),
            Poll::Ready(Ok(None))
        ));
        assert_eq!(capture.status(), CaptureStatus::Stopping);
    }

    #[test]
    fn ffmpeg_input_becomes_10ms_mono_frames_without_opening_a_microphone() {
        let mut source = tempfile::NamedTempFile::new().unwrap();
        let frames = 1_764_u32;
        let bytes = frames * 4;
        source.write_all(b"RIFF").unwrap();
        source.write_all(&(36 + bytes).to_le_bytes()).unwrap();
        source.write_all(b"WAVEfmt ").unwrap();
        for value in [
            16_u32.to_le_bytes(),
            [1, 0, 2, 0],
            44_100_u32.to_le_bytes(),
            176_400_u32.to_le_bytes(),
            [4, 0, 16, 0],
        ] {
            source.write_all(&value).unwrap();
        }
        source.write_all(b"data").unwrap();
        source.write_all(&bytes.to_le_bytes()).unwrap();
        for index in 0..frames {
            let signal = if index % 32 < 16 {
                8_000_i16
            } else {
                -8_000_i16
            };
            source.write_all(&signal.to_le_bytes()).unwrap();
            source.write_all(&(signal / 2).to_le_bytes()).unwrap();
        }
        source.flush().unwrap();
        let source = source.into_temp_path();
        let name = CString::new(source.to_str().unwrap()).unwrap();
        let capture = VoiceCapture::start_source(c"wav", Some(name)).unwrap();
        for sequence in 0..4 {
            let frame = capture
                .wait_frame(Duration::from_secs(3))
                .unwrap_or_else(|| panic!("capture frame {sequence}: {:?}", capture.status()));
            assert_eq!(frame.sequence, sequence);
            assert!(frame.samples.iter().any(|value| value.abs() > 0.05));
            assert!(
                frame
                    .samples
                    .iter()
                    .all(|value| value.is_finite() && value.abs() < 1.0)
            );
        }
        assert!(capture.wait_frame(Duration::from_secs(3)).is_none());
        assert!(capture.wait_stopped(Duration::from_secs(3)));
        assert_eq!(capture.status(), CaptureStatus::Stopped);
        capture.stop();
        assert!(capture.try_frame().is_none());
    }

    #[test]
    fn stop_clears_frames_and_prevents_a_late_open_from_reviving_capture() {
        let capture = VoiceCapture::opening();
        let frame = VoiceFrame {
            sequence: 0,
            samples: [0.5; SAMPLES_PER_FRAME],
        };
        capture.queue.push(frame.clone(), &capture.cancelled);
        capture.stop();
        capture.queue.set_status(CaptureStatus::Capturing);
        capture.queue.push(frame, &capture.cancelled);
        assert_eq!(capture.status(), CaptureStatus::Stopping);
        assert!(capture.try_frame().is_none());
        assert!(capture.queue.data.lock().unwrap().frames.is_empty());
        assert!(!capture.wait_stopped(Duration::ZERO));
        capture
            .queue
            .set_status(CaptureStatus::Failed("cancelled opening device".into()));
        assert!(capture.wait_stopped(Duration::ZERO));
        assert_eq!(capture.status(), CaptureStatus::Stopped);
    }

    #[test]
    fn voice_output_is_bounded_and_supplies_stereo_or_silence() {
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let sender = VoicePlaybackSender(Arc::downgrade(&queue));
        for sequence in 0..=QUEUE_FRAMES {
            let frame = VoiceFrame {
                sequence: sequence as u64,
                samples: [sequence as f32 / QUEUE_FRAMES as f32; SAMPLES_PER_FRAME],
            };
            assert!(sender.push(&frame));
        }
        assert_eq!(
            queue.lock().unwrap().len(),
            QUEUE_FRAMES * SAMPLES_PER_FRAME
        );
        let mut output = [0.0; 4];
        unsafe {
            render_voice(
                Arc::as_ptr(&queue).cast_mut().cast(),
                output.as_mut_ptr(),
                2,
                0,
            )
        };
        assert_eq!(output, [1.0 / QUEUE_FRAMES as f32; 4]);
        queue.lock().unwrap().clear();
        unsafe {
            render_voice(
                Arc::as_ptr(&queue).cast_mut().cast(),
                output.as_mut_ptr(),
                2,
                0,
            )
        };
        assert_eq!(output, [0.0; 4]);
        drop(queue);
        assert!(!sender.push(&VoiceFrame {
            sequence: 0,
            samples: [0.0; SAMPLES_PER_FRAME]
        }));
    }
}
