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

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerConfig {
    interval: Duration,
    repeat: bool,
    triggered_on_start: bool,
}

impl TimerConfig {
    pub fn new(milliseconds: i64, repeat: bool, triggered_on_start: bool) -> Option<Self> {
        (milliseconds > 0).then(|| Self {
            interval: Duration::from_millis(milliseconds.min(i64::from(u32::MAX)) as u64),
            repeat,
            triggered_on_start,
        })
    }
}

#[doc(hidden)]
pub struct TimerSlot {
    key: u64,
    revision: Property<u64>,
}

impl Default for TimerSlot {
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
        }
    }
}

impl TimerSlot {
    pub fn revision(&self) -> u64 {
        self.revision.get(|| 0)
    }

    pub fn restart(&self) {
        self.revision.set(
            self.revision()
                .checked_add(1)
                .expect("timer revision exhausted"),
        );
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

#[doc(hidden)]
pub struct TimerTick {
    pub config: TimerConfig,
    pub revision: u64,
    pub completed: bool,
}

#[doc(hidden)]
pub struct TimerDelivery {
    registration: TimerRegistration,
    completed: bool,
}

impl TimerDelivery {
    pub fn fire(self) -> bool {
        (self.registration.deliver)(TimerTick {
            config: self.registration.config,
            revision: self.registration.revision,
            completed: self.completed,
        })
    }
}

struct Entry {
    registration: TimerRegistration,
    deadline: Option<Instant>,
    initial_trigger: bool,
}

/// A clock-driven queue. The host supplies time and synchronizes active registrations.
#[doc(hidden)]
#[derive(Default)]
pub struct TimerQueue {
    entries: BTreeMap<u64, Entry>,
}

impl TimerQueue {
    pub fn sync<'a>(
        &mut self,
        now: Instant,
        registrations: impl IntoIterator<Item = &'a TimerRegistration>,
    ) {
        let mut previous = std::mem::take(&mut self.entries);
        for registration in registrations {
            let old = previous.remove(&registration.key);
            let entry =
                if let Some(old) = old.as_ref().filter(|old| old.registration == *registration) {
                    Entry {
                        registration: registration.clone(),
                        deadline: old.deadline,
                        initial_trigger: old.initial_trigger,
                    }
                } else {
                    let initial_trigger = registration.config.triggered_on_start
                        && old.is_none_or(|old| old.registration.revision != registration.revision);
                    let deadline = if initial_trigger {
                        Some(now)
                    } else {
                        now.checked_add(registration.config.interval)
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
        let config = entry.registration.config;
        let completed = !entry.initial_trigger && !config.repeat;
        entry.initial_trigger = false;
        entry.deadline = if completed {
            None
        } else {
            // Missed periods coalesce. The next deadline stays on the original cadence.
            let remainder =
                (now.duration_since(deadline).as_nanos() % config.interval.as_nanos()) as u64;
            now.checked_add(config.interval - Duration::from_nanos(remainder))
        };
        Some(TimerDelivery {
            registration: entry.registration.clone(),
            completed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

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
