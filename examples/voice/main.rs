use clap::Parser;
use std::{
    error::Error,
    io,
    time::{Duration, Instant},
};
use xrack::voice::{CaptureStatus, SAMPLE_RATE, VoiceCapture, VoicePlayback, VoicePlaybackSender};

#[derive(Parser)]
#[command(about = "Check the desktop microphone and optionally monitor it through headphones")]
struct Args {
    /// FFmpeg input device name or index. Omit to use the platform default.
    #[arg(long)]
    device: Option<String>,
    /// Test duration after the first input frame.
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u64).range(1..=60))]
    seconds: u64,
    /// Play captured audio locally. Wear headphones to avoid acoustic feedback.
    #[arg(long)]
    monitor: bool,
}

#[derive(Default)]
struct CaptureReport {
    frames: u64,
    dropped: u64,
    next_sequence: u64,
    peak: f32,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let playback = args.monitor.then(VoicePlayback::open).transpose()?;
    let sender = playback.as_ref().map(VoicePlayback::sender);
    let capture = VoiceCapture::start(args.device.as_deref())?;
    eprintln!("Opening microphone...");
    let report = measure(&capture, sender.as_ref(), Duration::from_secs(args.seconds));
    capture.stop();
    let released = capture.wait_stopped(Duration::from_secs(1));
    let report = report?;
    if !released {
        return Err(io::Error::other("timed out releasing the microphone").into());
    }
    println!(
        "Captured {} frames at {} Hz mono. Dropped {} frames. Peak {:.1}%. Microphone released.",
        report.frames,
        SAMPLE_RATE,
        report.dropped,
        report.peak * 100.0
    );
    Ok(())
}

fn measure(
    capture: &VoiceCapture,
    output: Option<&VoicePlaybackSender>,
    duration: Duration,
) -> io::Result<CaptureReport> {
    let mut frame = capture
        .wait_frame(Duration::from_secs(10))
        .ok_or_else(|| capture_error(capture.status(), "opening microphone"))?;
    let deadline = Instant::now() + duration;
    let mut report = CaptureReport::default();
    loop {
        report.frames += 1;
        report.dropped += frame.sequence.saturating_sub(report.next_sequence);
        report.next_sequence = frame.sequence + 1;
        report.peak = frame
            .samples
            .iter()
            .fold(report.peak, |peak, sample| peak.max(sample.abs()));
        if let Some(output) = output
            && !output.push(&frame)
        {
            return Err(io::Error::other("voice playback closed"));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(report);
        }
        match capture.wait_frame(remaining.min(Duration::from_secs(1))) {
            Some(next) => frame = next,
            None if Instant::now() >= deadline && capture.status() == CaptureStatus::Capturing => {
                return Ok(report);
            }
            None => return Err(capture_error(capture.status(), "reading microphone")),
        }
    }
}

fn capture_error(status: CaptureStatus, stage: &str) -> io::Error {
    let reason = match status {
        CaptureStatus::Failed(reason) => reason,
        CaptureStatus::Stopped | CaptureStatus::Stopping => "microphone stopped".into(),
        CaptureStatus::Opening | CaptureStatus::Capturing => "timed out waiting for audio".into(),
    };
    io::Error::other(format!("{stage}: {reason}"))
}
