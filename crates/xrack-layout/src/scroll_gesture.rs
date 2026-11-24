use crate::{ControlInput, Point};
use std::time::{Duration, Instant};

pub(crate) const DRAG_THRESHOLD: f32 = 8.0;
const MIN_SPEED: f32 = 25.0;
const MAX_SPEED: f32 = 3200.0;
const DAMPING: f32 = 7.5;
pub(crate) const SCROLL_FRAME_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollGestureKind {
    Touch,
    MousePan,
    MouseTumbler,
    MouseControl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PanDevice {
    Touch,
    LeftMouse,
    MiddleMouse,
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
    latest_velocity: Point,
    samples: u8,
    pub(crate) dragging: bool,
}

impl DragMotion {
    pub(crate) fn new(point: Point, now: Instant) -> Self {
        Self {
            point,
            updated: now,
            velocity: Point::default(),
            latest_velocity: Point::default(),
            samples: 0,
            dragging: false,
        }
    }

    pub(crate) fn update(
        &mut self,
        point: Point,
        now: Instant,
        window_distance: Option<f32>,
    ) -> Option<Point> {
        let mut delta = self.delta_to(point);
        let distance = delta.x.hypot(delta.y);
        if !distance.is_finite() || distance == 0.0 {
            return None;
        }
        if !self.dragging {
            let distance = window_distance.unwrap_or(distance);
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
            self.latest_velocity = Point {
                x: (delta.x / elapsed).clamp(-MAX_SPEED, MAX_SPEED),
                y: (delta.y / elapsed).clamp(-MAX_SPEED, MAX_SPEED),
            };
            self.velocity.x += (self.latest_velocity.x - self.velocity.x) * blend;
            self.velocity.y += (self.latest_velocity.y - self.velocity.y) * blend;
            self.samples = self.samples.saturating_add(1);
        }
        self.point = point;
        self.updated = now;
        self.dragging = true;
        Some(delta)
    }

    pub(crate) fn position(&self) -> Point {
        self.point
    }

    pub(crate) fn delta_to(&self, point: Point) -> Point {
        Point {
            x: self.point.x - point.x,
            y: self.point.y - point.y,
        }
    }

    fn recent_velocity(&self, now: Instant) -> Option<Point> {
        (self.dragging && now.saturating_duration_since(self.updated) <= Duration::from_millis(85))
            .then_some(self.velocity)
    }

    pub(crate) fn measured_release_velocity(&self, now: Instant) -> Option<Point> {
        // Use at least two motion samples to distinguish a flick from one coalesced move.
        (self.samples >= 2).then_some(())?;
        self.recent_velocity(now)
    }

    pub(crate) fn measured_release_velocities(&self, now: Instant) -> Option<(Point, Point)> {
        Some((self.measured_release_velocity(now)?, self.latest_velocity))
    }

    pub(crate) fn finish(self, now: Instant, axes: [bool; 2]) -> Option<FlingMotion> {
        let measured = self.recent_velocity(now)?;
        let velocity = Point {
            x: if axes[0] { measured.x } else { 0.0 },
            y: if axes[1] { measured.y } else { 0.0 },
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
    pub(crate) fn deadline(&self) -> Instant {
        self.updated + SCROLL_FRAME_INTERVAL
    }

    pub(crate) fn step(&mut self, now: Instant) -> Option<Point> {
        if now < self.deadline() {
            return Some(Point::default());
        }
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
