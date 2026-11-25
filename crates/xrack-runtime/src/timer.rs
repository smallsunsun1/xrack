use crate::Property;
use std::{
    collections::BTreeMap,
    fmt,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerCommand {
    Start,
    Stop,
    Restart,
}

impl TimerCommand {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "start" => Self::Start,
            "stop" => Self::Stop,
            "restart" => Self::Restart,
            _ => return None,
        })
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerConfig(TimerSchedule);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimerSchedule {
    Interval {
        period: Duration,
        repeat: bool,
        triggered_on_start: bool,
    },
    Frame,
}

impl TimerConfig {
    pub fn new(milliseconds: i64, repeat: bool, triggered_on_start: bool) -> Option<Self> {
        (milliseconds > 0).then(|| {
            Self(TimerSchedule::Interval {
                period: Duration::from_millis(milliseconds.min(i64::from(u32::MAX)) as u64),
                repeat,
                triggered_on_start,
            })
        })
    }

    pub fn frame() -> Self {
        Self(TimerSchedule::Frame)
    }

    pub fn try_read<E>(
        running: impl FnOnce() -> Result<bool, E>,
        interval: impl FnOnce() -> Result<i64, E>,
        repeat: impl FnOnce() -> Result<bool, E>,
        immediate: impl FnOnce() -> Result<bool, E>,
    ) -> Result<Option<Self>, E> {
        if !running()? {
            return Ok(None);
        }
        Ok(Self::new(interval()?, repeat()?, immediate()?))
    }

    pub fn read(
        running: impl FnOnce() -> bool,
        interval: impl FnOnce() -> i64,
        repeat: impl FnOnce() -> bool,
        immediate: impl FnOnce() -> bool,
    ) -> Option<Self> {
        Self::try_read(
            || Ok::<_, std::convert::Infallible>(running()),
            || Ok(interval()),
            || Ok(repeat()),
            || Ok(immediate()),
        )
        .unwrap_or_else(|error| match error {})
    }
}

#[doc(hidden)]
pub struct TimerSlot {
    key: u64,
    revision: Property<u64>,
    frame_clock: bool,
}

impl Default for TimerSlot {
    #[expect(
        clippy::expect_used,
        reason = "timer IDs must stay unique even if the counter reaches its limit"
    )]
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let key = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |key| {
                key.checked_add(1)
            })
            .expect("timer identities exhausted");
        Self {
            key,
            revision: Property::default(),
            frame_clock: false,
        }
    }
}

impl TimerSlot {
    pub fn frame() -> Self {
        Self {
            frame_clock: true,
            ..Self::default()
        }
    }

    pub fn try_command<E>(
        &self,
        command: TimerCommand,
        running: impl FnOnce() -> Result<bool, E>,
        set_running: impl FnOnce(bool) -> Result<(), E>,
    ) -> Result<(), E> {
        let active = match command {
            TimerCommand::Start => running()?,
            TimerCommand::Restart => self.frame_clock && running()?,
            TimerCommand::Stop => return set_running(false),
        };
        if command == TimerCommand::Restart || !active {
            self.restart();
        }
        if active { Ok(()) } else { set_running(true) }
    }

    pub fn command(
        &self,
        command: TimerCommand,
        running: impl FnOnce() -> bool,
        set_running: impl FnOnce(bool),
    ) {
        self.try_command(
            command,
            || Ok::<_, std::convert::Infallible>(running()),
            |value| {
                set_running(value);
                Ok(())
            },
        )
        .unwrap_or_else(|error| match error {})
    }

    pub fn revision(&self) -> u64 {
        self.revision.get(|| 0)
    }

    #[expect(
        clippy::expect_used,
        reason = "timer revision IDs must stay unique even if the counter reaches its limit"
    )]
    pub fn restart(&self) {
        self.revision.set(
            self.revision()
                .checked_add(1)
                .expect("timer revision exhausted"),
        );
    }

    /// Check the scheduled revision and configuration across reactive reads.
    pub fn try_tick_is_current<E>(
        &self,
        tick: &TimerTick,
        config: impl FnOnce() -> Result<Option<TimerConfig>, E>,
    ) -> Result<bool, E> {
        crate::property::watch_reads(|reads| {
            Ok(self.revision() == tick.revision
                && config()? == Some(tick.config)
                && !reads.changed())
        })
    }

    pub fn tick_is_current(
        &self,
        tick: &TimerTick,
        config: impl FnOnce() -> Option<TimerConfig>,
    ) -> bool {
        self.try_tick_is_current(tick, || Ok::<_, std::convert::Infallible>(config()))
            .unwrap_or_else(|error| match error {})
    }

    pub fn try_deliver<E>(
        &self,
        tick: &TimerTick,
        config: impl FnOnce() -> Result<Option<TimerConfig>, E>,
        stop: impl FnOnce() -> Result<(), E>,
        triggered: impl FnOnce() -> Result<(), E>,
    ) -> Result<bool, E> {
        if !self.try_tick_is_current(tick, config)? {
            return Ok(false);
        }
        if tick.completed {
            stop()?;
        }
        triggered()?;
        Ok(true)
    }

    pub fn deliver(
        &self,
        tick: &TimerTick,
        config: impl FnOnce() -> Option<TimerConfig>,
        stop: impl FnOnce(),
        triggered: impl FnOnce(),
    ) -> bool {
        self.try_deliver(
            tick,
            || Ok::<_, std::convert::Infallible>(config()),
            || {
                stop();
                Ok(())
            },
            || {
                triggered();
                Ok(())
            },
        )
        .unwrap_or_else(|error| match error {})
    }

    pub fn register(
        &self,
        config: TimerConfig,
        deliver: impl Fn(TimerTick) -> bool + 'static,
    ) -> TimerRegistration {
        TimerRegistration {
            key: self.key,
            config,
            revision: self.revision(),
            deliver: Rc::new(deliver),
        }
    }
}

/// A nonvisual registration with a stable identity and a weak component callback.
#[doc(hidden)]
#[derive(Clone)]
pub struct TimerRegistration {
    key: u64,
    revision: u64,
    config: TimerConfig,
    deliver: Rc<dyn Fn(TimerTick) -> bool>,
}

impl fmt::Debug for TimerRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TimerRegistration")
            .field("key", &self.key)
            .field("revision", &self.revision)
            .field("config", &self.config)
            .finish()
    }
}

impl PartialEq for TimerRegistration {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.revision == other.revision && self.config == other.config
    }
}

/// One sample from the host's frame clock. Times are in seconds.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct FrameTick {
    pub frame_time: f64,
    pub elapsed_time: f64,
    pub current_frame: i64,
}

#[doc(hidden)]
pub struct TimerTick {
    pub config: TimerConfig,
    pub revision: u64,
    pub completed: bool,
    pub frame: Option<FrameTick>,
}

#[doc(hidden)]
pub struct TimerDelivery {
    registration: TimerRegistration,
    completed: bool,
    frame: Option<FrameTick>,
}

impl TimerDelivery {
    pub fn fire(self) -> bool {
        (self.registration.deliver)(TimerTick {
            config: self.registration.config,
            revision: self.registration.revision,
            completed: self.completed,
            frame: self.frame,
        })
    }
}

struct Entry {
    registration: TimerRegistration,
    deadline: Option<Instant>,
    initial_trigger: bool,
}

#[derive(Default)]
struct FrameClock {
    last: Option<Instant>,
    elapsed: Duration,
    index: i64,
}

impl FrameClock {
    fn sample(&mut self, now: Instant) -> Option<FrameTick> {
        if self.last.is_some_and(|last| now <= last) {
            return None;
        }
        let delta = self
            .last
            .replace(now)
            .map_or(Duration::ZERO, |last| now.duration_since(last));
        self.elapsed = self.elapsed.saturating_add(delta);
        self.index = self.index.saturating_add(1);
        Some(FrameTick {
            frame_time: delta.as_secs_f64(),
            elapsed_time: self.elapsed.as_secs_f64(),
            current_frame: self.index,
        })
    }
}

struct FrameEntry {
    registration: TimerRegistration,
    clock: FrameClock,
}

/// A clock-driven queue. The host supplies time and synchronizes active registrations.
#[doc(hidden)]
#[derive(Default)]
pub struct TimerQueue {
    entries: BTreeMap<u64, Entry>,
    frames: BTreeMap<u64, FrameEntry>,
}

impl TimerQueue {
    pub fn sync<'a>(
        &mut self,
        now: Instant,
        registrations: impl IntoIterator<Item = &'a TimerRegistration>,
    ) {
        let mut previous = std::mem::take(&mut self.entries);
        let mut previous_frames = std::mem::take(&mut self.frames);
        for registration in registrations {
            let TimerSchedule::Interval {
                period,
                triggered_on_start,
                ..
            } = registration.config.0
            else {
                let clock = previous_frames
                    .remove(&registration.key)
                    .filter(|old| old.registration == *registration)
                    .map_or_else(FrameClock::default, |old| old.clock);
                self.frames.insert(
                    registration.key,
                    FrameEntry {
                        registration: registration.clone(),
                        clock,
                    },
                );
                continue;
            };
            let old = previous.remove(&registration.key);
            let entry =
                if let Some(old) = old.as_ref().filter(|old| old.registration == *registration) {
                    Entry {
                        registration: registration.clone(),
                        deadline: old.deadline,
                        initial_trigger: old.initial_trigger,
                    }
                } else {
                    let initial_trigger = triggered_on_start
                        && old.is_none_or(|old| old.registration.revision != registration.revision);
                    let deadline = if initial_trigger {
                        Some(now)
                    } else {
                        now.checked_add(period)
                    };
                    Entry {
                        registration: registration.clone(),
                        deadline,
                        initial_trigger,
                    }
                };
            self.entries.insert(registration.key, entry);
        }
    }

    pub fn has_frame_animations(&self) -> bool {
        !self.frames.is_empty()
    }

    /// Capture the frame's eligible registrations before invoking handlers.
    pub fn frame_deliveries(&mut self, now: Instant) -> Vec<TimerDelivery> {
        self.frames
            .values_mut()
            .filter_map(|entry| {
                Some(TimerDelivery {
                    registration: entry.registration.clone(),
                    completed: false,
                    frame: Some(entry.clock.sample(now)?),
                })
            })
            .collect()
    }

    // A previous handler can stop, restart or remove another animation.
    pub fn frame_delivery_is_current(&self, delivery: &TimerDelivery) -> bool {
        self.frames
            .get(&delivery.registration.key)
            .is_some_and(|entry| entry.registration == delivery.registration)
    }

    /// Suspend frame timing while the host has no visible drawable.
    pub fn reset_frame_baselines(&mut self) {
        for entry in self.frames.values_mut() {
            entry.clock.last = None;
        }
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.entries
            .values()
            .filter_map(|entry| entry.deadline)
            .min()
    }

    pub fn next_due(&mut self, now: Instant) -> Option<TimerDelivery> {
        let entry = self
            .entries
            .values_mut()
            .filter(|entry| entry.deadline.is_some_and(|deadline| deadline <= now))
            .min_by_key(|entry| entry.deadline)?;
        let deadline = entry.deadline?;
        let TimerSchedule::Interval { period, repeat, .. } = entry.registration.config.0 else {
            return None;
        };
        let completed = !entry.initial_trigger && !repeat;
        entry.initial_trigger = false;
        entry.deadline = if completed {
            None
        } else {
            // Missed periods coalesce. The next deadline stays on the original cadence.
            let remainder = (now.duration_since(deadline).as_nanos() % period.as_nanos()) as u64;
            now.checked_add(period - Duration::from_nanos(remainder))
        };
        Some(TimerDelivery {
            registration: entry.registration.clone(),
            completed,
            frame: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn frame_delivery_uses_frame_time_and_keeps_interval_deadlines_separate() {
        use std::cell::Cell;

        let start = Instant::now();
        let interval_slot = TimerSlot::default();
        let frame_slot = TimerSlot::frame();
        let timed = interval_slot.register(TimerConfig::new(20, true, false).unwrap(), |_| true);
        let last = Rc::new(Cell::<Option<FrameTick>>::new(None));
        let register = || {
            let last = last.clone();
            frame_slot.register(TimerConfig::frame(), move |tick| {
                last.set(tick.frame);
                true
            })
        };
        let mut queue = TimerQueue::default();
        let original = register();
        queue.sync(start, [&timed, &original]);
        assert_eq!(queue.deadline(), Some(start + Duration::from_millis(20)));
        assert!(queue.next_due(start).is_none());
        let mut batch = queue.frame_deliveries(start);
        assert!(queue.frame_delivery_is_current(&batch[0]));
        assert!(batch.pop().unwrap().fire());
        assert!(queue.frame_deliveries(start).is_empty());
        let first = last.get().unwrap();
        assert_eq!(
            (first.frame_time, first.elapsed_time, first.current_frame),
            (0.0, 0.0, 1)
        );

        let later = start + Duration::from_millis(12);
        queue.sync(later, [&timed, &register()]);
        assert!(queue.frame_deliveries(later).pop().unwrap().fire());
        let second = last.get().unwrap();
        assert_eq!(
            (second.frame_time, second.elapsed_time, second.current_frame),
            (0.012, 0.012, 2)
        );
        queue.reset_frame_baselines();
        let restored = start + Duration::from_secs(3);
        assert!(queue.frame_deliveries(restored).pop().unwrap().fire());
        let restored_sample = last.get().unwrap();
        assert_eq!(
            (
                restored_sample.frame_time,
                restored_sample.elapsed_time,
                restored_sample.current_frame
            ),
            (0.0, 0.012, 3)
        );

        let due = queue
            .frame_deliveries(restored + Duration::from_millis(16))
            .pop()
            .unwrap();
        frame_slot.command(
            TimerCommand::Restart,
            || true,
            |_| panic!("restart replaced running binding"),
        );
        queue.sync(restored, [&timed, &register()]);
        assert!(!queue.frame_delivery_is_current(&due));
        assert!(queue.frame_deliveries(restored).pop().unwrap().fire());
        assert_eq!(last.get().unwrap().current_frame, 1);
        queue.sync(restored, [&timed]);
        assert!(!queue.has_frame_animations());
    }

    #[test]
    fn late_periods_coalesce_without_moving_the_original_cadence() {
        let slot = TimerSlot::default();
        let start = Instant::now();
        let at = |millis| start + Duration::from_millis(millis);
        let config = TimerConfig::new(100, true, false).unwrap();
        let registration = slot.register(config, |tick| !tick.completed);
        let mut queue = TimerQueue::default();
        queue.sync(start, [&registration]);
        assert!(queue.next_due(at(352)).unwrap().fire());
        assert!(queue.next_due(at(352)).is_none());
        assert_eq!(queue.deadline(), Some(at(400)));
        queue.sync(at(365), [&registration]);
        assert_eq!(queue.deadline(), Some(at(400)));
        let faster = slot.register(TimerConfig::new(30, true, false).unwrap(), |_| true);
        queue.sync(at(372), [&faster]);
        assert_eq!(queue.deadline(), Some(at(402)));
        queue.sync(at(373), []);
        assert!(queue.deadline().is_none());
        queue.sync(at(800), [&faster]);
        assert_eq!(queue.deadline(), Some(at(830)));
    }

    #[test]
    fn immediate_single_shot_delivers_start_then_completion() {
        let slot = TimerSlot::default();
        let start = Instant::now();
        let config = TimerConfig::new(80, false, true).unwrap();
        let completed = Rc::new(RefCell::new(Vec::new()));
        let phases = completed.clone();
        let registration = slot.register(config, move |tick| {
            phases.borrow_mut().push(tick.completed);
            true
        });
        let mut queue = TimerQueue::default();
        queue.sync(start, [&registration]);
        assert!(queue.next_due(start).unwrap().fire());
        assert!(
            queue
                .next_due(start + Duration::from_millis(80))
                .unwrap()
                .fire()
        );
        assert_eq!(&*completed.borrow(), &[false, true]);
        queue.sync(start + Duration::from_millis(81), [&registration]);
        assert!(queue.deadline().is_none());
    }
}
