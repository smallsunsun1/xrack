use crate::{ControlInput, Point};
use std::time::{Duration, Instant};

const DRAG_THRESHOLD: f32 = 8.0;
const MIN_SPEED: f32 = 25.0;
const MAX_SPEED: f32 = 3200.0;
const DAMPING: f32 = 7.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollGestureKind {
    Touch,
    MousePan,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScrollGestureEnd {
    pub tap: bool,
    pub scrolled: bool,
    pub control: Option<ControlInput>,
}

#[derive(Clone, Debug)]
pub(crate) struct DragMotion {
    point: Point,
    updated: Instant,
    velocity: Point,
    pub(crate) dragging: bool,
}

impl DragMotion {
    pub(crate) fn new(point: Point, now: Instant) -> Self {
        Self {
            point,
            updated: now,
            velocity: Point::default(),
            dragging: false,
        }
    }

    pub(crate) fn update(&mut self, point: Point, now: Instant) -> Option<Point> {
        let mut delta = Point {
            x: self.point.x - point.x,
            y: self.point.y - point.y,
        };
        let distance = delta.x.hypot(delta.y);
        if !distance.is_finite() || distance == 0.0 {
            return None;
        }
        if !self.dragging {
            if distance < DRAG_THRESHOLD {
                return None;
            }
            let factor = (distance - DRAG_THRESHOLD) / distance;
            delta.x *= factor;
            delta.y *= factor;
        }
        let elapsed = now.saturating_duration_since(self.updated).as_secs_f32();
        if elapsed > 0.0 {
            let blend = if !self.dragging || elapsed > 0.08 {
                1.0
            } else {
                elapsed / (elapsed + 0.025)
            };
            let smooth = |old: f32, movement: f32| {
                let speed = (movement / elapsed).clamp(-MAX_SPEED, MAX_SPEED);
                old + (speed - old) * blend
            };
            self.velocity.x = smooth(self.velocity.x, delta.x);
            self.velocity.y = smooth(self.velocity.y, delta.y);
        }
        self.point = point;
        self.updated = now;
        self.dragging = true;
        Some(delta)
    }

    pub(crate) fn finish(self, now: Instant, axes: [bool; 2]) -> Option<FlingMotion> {
        if !self.dragging || now.saturating_duration_since(self.updated) > Duration::from_millis(85)
        {
            return None;
        }
        let velocity = Point {
            x: if axes[0] { self.velocity.x } else { 0.0 },
            y: if axes[1] { self.velocity.y } else { 0.0 },
        };
        let mut motion = FlingMotion {
            velocity,
            updated: now,
        };
        motion.finish_step([true, true]).then_some(motion)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FlingMotion {
    velocity: Point,
    updated: Instant,
}

impl FlingMotion {
    pub(crate) fn step(&mut self, now: Instant) -> Option<Point> {
        let elapsed = now.saturating_duration_since(self.updated).as_secs_f32();
        // A window may resume after being occluded or suspended.
        if elapsed > 0.25 {
            return None;
        }
        self.updated = now;
        let decay = (-DAMPING * elapsed).exp();
        let factor = (1.0 - decay) / DAMPING;
        let delta = Point {
            x: self.velocity.x * factor,
            y: self.velocity.y * factor,
        };
        self.velocity.x *= decay;
        self.velocity.y *= decay;
        Some(delta)
    }

    pub(crate) fn finish_step(&mut self, moved: [bool; 2]) -> bool {
        if !moved[0] || self.velocity.x.abs() < MIN_SPEED {
            self.velocity.x = 0.0;
        }
        if !moved[1] || self.velocity.y.abs() < MIN_SPEED {
            self.velocity.y = 0.0;
        }
        self.velocity != Point::default()
    }
}
