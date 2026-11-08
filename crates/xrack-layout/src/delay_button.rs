use crate::{ActivationKey, Point, Target};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub(crate) enum DelaySource {
    Pointer,
    Key(ActivationKey),
    Touch(Point),
    Accessibility,
}

#[derive(Clone, Debug)]
pub(crate) struct DelayHold {
    pub target: Target,
    pub source: DelaySource,
    pub scope: Vec<usize>,
    pub duration: Duration,
    start: Instant,
    pub fraction: f32,
    ready: bool,
}

impl DelayHold {
    pub fn new(
        target: Target,
        source: DelaySource,
        scope: Vec<usize>,
        duration: Duration,
        start: Instant,
    ) -> Self {
        Self {
            target,
            source,
            scope,
            duration,
            start,
            fraction: 0.0,
            ready: false,
        }
    }

    pub fn deadline(&self) -> Instant {
        self.start + self.duration
    }

    pub fn sample(&mut self, now: Instant) -> bool {
        if now >= self.deadline() {
            self.fraction = 1.0;
            self.ready = true;
            true
        } else {
            self.fraction = (now.saturating_duration_since(self.start).as_secs_f64()
                / self.duration.as_secs_f64()) as f32;
            false
        }
    }

    pub fn pending(&self) -> bool {
        !self.ready
    }
}
