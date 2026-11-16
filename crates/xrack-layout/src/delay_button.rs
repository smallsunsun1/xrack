use crate::{ActivationKey, Point, Target, pointer_events::PRESS_AND_HOLD_SLOP};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ButtonHoldTiming {
    delay: Duration,
    interval: Option<Duration>,
}

impl ButtonHoldTiming {
    pub fn from_node(node: &xrack_runtime::Node) -> Option<Self> {
        if !matches!(node.kind, "Button" | "ToolButton" | "RoundButton") {
            return None;
        }
        if node.boolean("auto-repeat", false) {
            let interval = node.int("auto-repeat-interval").unwrap_or(100);
            return (interval > 0).then(|| Self {
                delay: Duration::from_millis(
                    node.int("auto-repeat-delay")
                        .unwrap_or(300)
                        .clamp(0, 60_000) as u64,
                ),
                interval: Some(Duration::from_millis(interval.min(60_000) as u64)),
            });
        }
        let delay = node.int("press-and-hold-interval").unwrap_or(800);
        (node.press_and_hold_handler && delay > 0).then(|| Self {
            delay: Duration::from_millis(delay.min(60_000) as u64),
            interval: None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonHoldEvent {
    Repeat,
    PressAndHold,
}

impl ButtonHoldEvent {
    pub fn handler(self) -> &'static str {
        match self {
            Self::Repeat => "clicked",
            Self::PressAndHold => "press-and-hold",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ButtonHold {
    pub target: Target,
    pub source: DelaySource,
    pub scope: Vec<usize>,
    pub timing: ButtonHoldTiming,
    pub deadline: Option<Instant>,
    origin: Option<Point>,
}

impl ButtonHold {
    pub fn new(
        target: Target,
        source: DelaySource,
        scope: Vec<usize>,
        timing: ButtonHoldTiming,
        origin: Option<Point>,
        now: Instant,
    ) -> Self {
        Self {
            target,
            source,
            scope,
            timing,
            deadline: Some(now + timing.delay),
            origin,
        }
    }

    pub fn moved_too_far(&self, point: Point) -> bool {
        self.timing.interval.is_none()
            && self.origin.is_some_and(|origin| {
                (point.x - origin.x).hypot(point.y - origin.y) > PRESS_AND_HOLD_SLOP
            })
    }

    pub fn sample(&mut self, now: Instant) -> Option<ButtonHoldEvent> {
        if now < self.deadline? {
            return None;
        }
        // Skip missed activations instead of delivering a burst after a stall.
        self.deadline = self.timing.interval.map(|interval| now + interval);
        Some(if self.deadline.is_some() {
            ButtonHoldEvent::Repeat
        } else {
            ButtonHoldEvent::PressAndHold
        })
    }
}
