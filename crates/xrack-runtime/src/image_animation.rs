use crate::{Image, ImageAsset, ImageFormat, LoadedAnimation, Node, Property};
use std::{
    collections::BTreeMap,
    fmt,
    rc::Rc,
    time::{Duration, Instant},
};

/// A composited frame and its exclusive end on the source's timeline.
#[derive(Debug, PartialEq, Eq)]
pub struct ImageAnimationFrame {
    pub end_ms: u32,
    pub png: &'static [u8],
}

/// Immutable, compiler-prepared frames shared by every use of an image.
#[derive(Debug)]
pub struct ImageAnimation {
    size: [u32; 2],
    frames: &'static [ImageAnimationFrame],
    plays: u32,
}

impl ImageAnimation {
    #[doc(hidden)]
    pub const fn new(size: [u32; 2], frames: &'static [ImageAnimationFrame], plays: u32) -> Self {
        Self {
            size,
            frames,
            plays,
        }
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn frame(&self, index: usize) -> Image {
        self.frames
            .get(index)
            .or_else(|| self.frames.first())
            .map_or(Image::Empty, |frame| Image::Embedded {
                format: ImageFormat::Png,
                bytes: frame.png,
                size: self.size,
            })
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub(crate) fn matches_loaded(&self, other: &LoadedAnimation) -> bool {
        self.size == other.size
            && self.plays == other.plays
            && self
                .frames
                .iter()
                .map(|frame| (frame.end_ms, frame.png))
                .eq(other
                    .frames
                    .iter()
                    .map(|(end, frame)| (*end, frame.bytes())))
    }
}

impl PartialEq for ImageAnimation {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
            || (self.size == other.size && self.plays == other.plays && self.frames == other.frames)
    }
}

impl Eq for ImageAnimation {}

enum Timeline<'a> {
    Embedded(&'a ImageAnimation),
    Loaded(&'a LoadedAnimation),
}

impl<'a> Timeline<'a> {
    fn for_image(image: &'a ImageAsset) -> Option<Self> {
        match image {
            ImageAsset::Embedded(Image::Animated(animation)) => Some(Self::Embedded(animation)),
            ImageAsset::Animated(animation) => Some(Self::Loaded(animation)),
            _ => None,
        }
    }

    fn plays(&self) -> u32 {
        match self {
            Self::Embedded(animation) => animation.plays,
            Self::Loaded(animation) => animation.plays,
        }
    }

    fn end(&self, frame: usize) -> f64 {
        f64::from(match self {
            Self::Embedded(animation) => animation.frames[frame].end_ms,
            Self::Loaded(animation) => animation.frames[frame].0,
        })
    }

    fn frame_at(&self, position: f64) -> usize {
        match self {
            Self::Embedded(animation) => animation
                .frames
                .partition_point(|frame| f64::from(frame.end_ms) <= position),
            Self::Loaded(animation) => animation
                .frames
                .partition_point(|(end, _)| f64::from(*end) <= position),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Sample {
    source: ImageAsset,
    frame: usize,
}

#[derive(Clone, Debug, PartialEq)]
struct Playback {
    source: ImageAsset,
    playing: bool,
    paused: bool,
    speed: f64,
    loops: i64,
}

impl Playback {
    fn from_node(node: &Node) -> Self {
        let speed = node.number("speed").unwrap_or(1.0);
        Self {
            source: node.image_asset("source").unwrap_or_default(),
            playing: node.boolean("playing", true),
            paused: node.boolean("paused", false),
            speed: if speed.is_finite() {
                speed.clamp(0.0, 16.0)
            } else {
                0.0
            },
            loops: node.int("loops").unwrap_or(0),
        }
    }

    fn active(&self) -> bool {
        self.playing && !self.paused && self.speed > 0.0
    }

    fn plays(&self, animation: &Timeline<'_>) -> u32 {
        match self.loops {
            i64::MIN..=-1 => 0,
            0 => animation.plays(),
            count => count.min(i64::from(u32::MAX)) as u32,
        }
    }
}

#[doc(hidden)]
#[derive(Default)]
pub struct ImagePlaybackSlot(Rc<Property<Sample>>);

impl ImagePlaybackSlot {
    pub fn current_frame(&self) -> i64 {
        self.0.get(Sample::default).frame as i64
    }

    pub fn frame_count(&self) -> i64 {
        self.0.get(Sample::default).source.frame_count() as i64
    }

    pub fn populate(&self, node: &mut Node, finished: impl Fn() -> bool + 'static) {
        let config = Playback::from_node(node);
        let sample = self.0.get(Sample::default);
        let frame = if config.playing && sample.source == config.source {
            sample.frame
        } else {
            0
        };
        node.properties
            .insert("current-frame", (frame as i64).into());
        node.image_playback = Some(Rc::new(ImagePlaybackRegistration {
            sample: self.0.clone(),
            config,
            finished: Rc::new(finished),
        }));
    }
}

#[doc(hidden)]
pub struct ImagePlaybackRegistration {
    sample: Rc<Property<Sample>>,
    config: Playback,
    finished: Rc<dyn Fn() -> bool>,
}

impl ImagePlaybackRegistration {
    fn key(&self) -> usize {
        Rc::as_ptr(&self.sample) as usize
    }

    pub fn finish(&self) -> bool {
        (self.finished)()
    }
}

impl PartialEq for ImagePlaybackRegistration {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.sample, &other.sample) && self.config == other.config
    }
}

impl fmt::Debug for ImagePlaybackRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImagePlaybackRegistration")
            .field("key", &self.key())
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

struct Entry {
    registration: Rc<ImagePlaybackRegistration>,
    anchor: Instant,
    presented: bool,
    position_ms: f64,
    next: Option<Instant>,
    ended: bool,
    pending_finished: bool,
}

impl Entry {
    fn integrate(&mut self, now: Instant) {
        let config = &self.registration.config;
        if self.next.is_some() {
            let elapsed_ms =
                now.saturating_duration_since(self.anchor).as_nanos() as f64 / 1_000_000.0;
            self.position_ms += elapsed_ms * config.speed;
        }
        self.anchor = now;
    }

    fn sample(&mut self) -> bool {
        let config = &self.registration.config;
        let mut frame = 0;
        let mut ended = false;
        self.next = None;
        if config.playing
            && let Some(animation) = Timeline::for_image(&config.source)
            && config.source.frame_count() >= 2
        {
            let count = config.source.frame_count();
            let duration = animation.end(count - 1);
            if duration > 0.0 {
                let plays = config.plays(&animation);
                ended = plays != 0 && self.position_ms >= duration * f64::from(plays);
                if ended {
                    frame = count - 1;
                } else {
                    let within = self.position_ms % duration;
                    frame = animation.frame_at(within);
                    if self.presented && config.active() {
                        let remaining_ms = animation.end(frame) - within;
                        self.next = Duration::try_from_secs_f64(
                            (remaining_ms / config.speed / 1000.0).max(0.001),
                        )
                        .ok()
                        .and_then(|delay| self.anchor.checked_add(delay));
                    }
                }
            }
        }
        self.pending_finished |= ended && !self.ended;
        self.ended = ended;
        let sample = Sample {
            source: config.source.clone(),
            frame,
        };
        if self.registration.sample.get(Sample::default) == sample {
            return false;
        }
        self.registration.sample.set(sample);
        true
    }
}

/// Clock and deadlines for visible playback registrations. The host owns time.
#[doc(hidden)]
#[derive(Default)]
pub struct ImagePlaybackQueue {
    entries: BTreeMap<usize, Entry>,
}

impl ImagePlaybackQueue {
    pub fn sync<'a>(
        &mut self,
        now: Instant,
        registrations: impl IntoIterator<Item = (&'a Rc<ImagePlaybackRegistration>, bool)>,
    ) -> bool {
        let mut changed = false;
        let mut previous = std::mem::take(&mut self.entries);
        for (registration, presented) in registrations {
            let key = registration.key();
            if self.entries.contains_key(&key) {
                continue;
            }
            let mut entry = if let Some(mut entry) = previous.remove(&key) {
                let old = &entry.registration.config;
                let next = &registration.config;
                if old == next && entry.presented == presented {
                    entry.registration = registration.clone();
                    self.entries.insert(key, entry);
                    continue;
                }
                let reset = old.source != next.source || !next.playing || !old.playing;
                entry.integrate(now);
                if reset {
                    entry.position_ms = 0.0;
                    entry.ended = false;
                    entry.pending_finished = false;
                }
                entry.registration = registration.clone();
                entry.presented = presented;
                entry
            } else {
                Entry {
                    registration: registration.clone(),
                    anchor: now,
                    presented,
                    position_ms: 0.0,
                    next: None,
                    ended: false,
                    pending_finished: false,
                }
            };
            changed |= entry.sample();
            self.entries.insert(key, entry);
        }
        for entry in previous.into_values() {
            let sample = &entry.registration.sample;
            let empty = Sample::default();
            if sample.get(Sample::default) != empty {
                sample.set(empty);
                changed = true;
            }
        }
        changed
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.entries
            .values()
            .filter_map(|entry| {
                if entry.pending_finished {
                    Some(entry.anchor)
                } else {
                    entry.next
                }
            })
            .min()
    }

    /// Coalesce missed ticks and publish only the frame visible at `now`.
    pub fn advance(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for entry in self
            .entries
            .values_mut()
            .filter(|entry| entry.next.is_some_and(|next| next <= now))
        {
            entry.integrate(now);
            changed |= entry.sample();
        }
        changed
    }

    pub fn next_finished(&mut self) -> Option<Rc<ImagePlaybackRegistration>> {
        let entry = self
            .entries
            .values_mut()
            .find(|entry| entry.pending_finished)?;
        entry.pending_finished = false;
        Some(entry.registration.clone())
    }
}
