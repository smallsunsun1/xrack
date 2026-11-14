use crate::{Point, Rect};
use std::f64::consts::{PI, TAU};
use xrack_runtime::{DialInputMode, Node, Orientation};

/// Shared geometry for the saturation and brightness color field.
pub struct ColorPlaneGeometry {
    pub field: Rect,
}

impl ColorPlaneGeometry {
    pub fn new(bounds: Rect) -> Self {
        Self {
            field: bounds.inset(6.0),
        }
    }

    pub fn point(&self, saturation: f64, brightness: f64) -> Point {
        Point {
            x: self.field.x + self.field.width * saturation.clamp(0.0, 1.0) as f32,
            y: self.field.y + self.field.height * (1.0 - brightness.clamp(0.0, 1.0)) as f32,
        }
    }

    pub fn action_at(&self, point: Point) -> Option<xrack_runtime::RangeAction> {
        let area = self.field;
        if area.width <= 0.0 || area.height <= 0.0 || !point.x.is_finite() || !point.y.is_finite() {
            return None;
        }
        Some(xrack_runtime::RangeAction::SetPoint {
            x: f64::from(((point.x - area.x) / area.width).clamp(0.0, 1.0)),
            y: f64::from((1.0 - (point.y - area.y) / area.height).clamp(0.0, 1.0)),
        })
    }
}

/// Shared thumb travel for hit mapping and painting.
pub struct SliderGeometry {
    start: Point,
    end: Point,
    pub radius: f32,
    horizontal: f32,
    vertical: f32,
}

impl SliderGeometry {
    pub fn new(bounds: Rect, orientation: Orientation) -> Self {
        let radius = (bounds.width.min(bounds.height) * 0.5).clamp(0.0, 8.0);
        Self::with_travel(bounds, orientation, radius, radius)
    }

    pub fn for_node(bounds: Rect, node: &Node) -> Self {
        let orientation = node.orientation();
        let radius = (bounds.width.min(bounds.height) * 0.5).clamp(0.0, 8.0);
        let extent = |part: &Node, property, available: f32| {
            part.length(property)
                .unwrap_or(radius * 2.0)
                .clamp(0.0, available)
        };
        let (horizontal, vertical) =
            node.control_part("SliderHandle")
                .map_or((radius, radius), |part| {
                    (
                        extent(part, "width", bounds.width) * 0.5,
                        extent(part, "height", bounds.height) * 0.5,
                    )
                });
        Self::with_travel(bounds, orientation, horizontal, vertical)
    }

    fn with_travel(bounds: Rect, orientation: Orientation, horizontal: f32, vertical: f32) -> Self {
        let radius = (bounds.width.min(bounds.height) * 0.5).clamp(0.0, 8.0);
        let (start, end) = match orientation {
            Orientation::Horizontal => (
                Point {
                    x: bounds.x + horizontal,
                    y: bounds.y + bounds.height * 0.5,
                },
                Point {
                    x: bounds.x + bounds.width - horizontal,
                    y: bounds.y + bounds.height * 0.5,
                },
            ),
            Orientation::Vertical => (
                Point {
                    x: bounds.x + bounds.width * 0.5,
                    y: bounds.y + bounds.height - vertical,
                },
                Point {
                    x: bounds.x + bounds.width * 0.5,
                    y: bounds.y + vertical,
                },
            ),
        };
        Self {
            start,
            end,
            radius,
            horizontal,
            vertical,
        }
    }

    pub fn position(&self, fraction: f64) -> Point {
        let t = fraction.clamp(0.0, 1.0) as f32;
        Point {
            x: self.start.x + (self.end.x - self.start.x) * t,
            y: self.start.y + (self.end.y - self.start.y) * t,
        }
    }

    pub(crate) fn contains_thumb(&self, fraction: f64, point: Point) -> bool {
        let center = self.position(fraction);
        (point.x - center.x).abs() <= self.horizontal && (point.y - center.y).abs() <= self.vertical
    }

    pub fn fraction_at(&self, point: Point) -> Option<f64> {
        let dx = f64::from(self.end.x - self.start.x);
        let dy = f64::from(self.end.y - self.start.y);
        let length_squared = dx * dx + dy * dy;
        if length_squared <= 0.0 {
            return None;
        }
        let x = f64::from(point.x - self.start.x);
        let y = f64::from(point.y - self.start.y);
        let fraction = (x * dx + y * dy) / length_squared;
        fraction.is_finite().then_some(fraction)
    }
}

/// The dial arc shared by hit testing, dragging and painting.
pub struct DialGeometry {
    pub center: Point,
    pub face_radius: f32,
    pub track_radius: f32,
    pub thumb_radius: f32,
    start: f64,
    sweep: f64,
}

impl DialGeometry {
    pub fn new(bounds: Rect, node: &Node) -> Self {
        let default_sweep = if node.boolean("wrap", false) {
            360.0
        } else {
            280.0
        };
        let start = node.number("start-angle").unwrap_or(-default_sweep * 0.5);
        let sweep = node
            .number("end-angle")
            .map_or(default_sweep, |end| end - start);
        let sweep = if sweep > 0.0 {
            sweep.clamp(1.0, 360.0)
        } else {
            default_sweep
        };
        let face_radius = bounds.width.min(bounds.height).max(0.0) * 0.5;
        let thumb_radius = (face_radius * 0.18).min(6.0);
        Self {
            center: Point {
                x: bounds.x + bounds.width * 0.5,
                y: bounds.y + bounds.height * 0.5,
            },
            face_radius,
            track_radius: (face_radius - thumb_radius - 2.0).max(0.0),
            thumb_radius,
            start: start.rem_euclid(360.0).to_radians(),
            sweep: sweep.to_radians(),
        }
    }

    /// Clockwise radians measured from twelve o'clock.
    pub fn angle(&self, fraction: f64) -> f64 {
        self.start + self.sweep * fraction.clamp(0.0, 1.0)
    }

    pub fn position(&self, fraction: f64) -> Point {
        let (sin, cos) = self.angle(fraction).sin_cos();
        Point {
            x: self.center.x + self.track_radius * sin as f32,
            y: self.center.y - self.track_radius * cos as f32,
        }
    }

    pub(crate) fn contains(&self, point: Point) -> bool {
        (point.x - self.center.x).hypot(point.y - self.center.y) <= self.face_radius
    }

    pub(crate) fn contains_thumb(&self, fraction: f64, point: Point) -> bool {
        let thumb = self.position(fraction);
        (point.x - thumb.x).hypot(point.y - thumb.y) <= self.thumb_radius
    }

    fn pointer_angle(&self, point: Point) -> Option<f64> {
        let x = point.x - self.center.x;
        let y = point.y - self.center.y;
        // Angles near the center are unstable during a drag.
        (self.track_radius > 0.0 && x.hypot(y) > self.face_radius * 0.16)
            .then(|| f64::from(x).atan2(-f64::from(y)))
    }

    pub(crate) fn fraction_at(&self, point: Point) -> Option<f64> {
        let phase = (self.pointer_angle(point)? - self.start).rem_euclid(TAU);
        Some(if phase <= self.sweep {
            phase / self.sweep
        } else if phase - self.sweep <= TAU - phase {
            1.0
        } else {
            0.0
        })
    }

    pub(crate) fn drag_delta(
        &self,
        mode: DialInputMode,
        previous: Point,
        next: Point,
    ) -> Option<f64> {
        let diameter = f64::from(self.face_radius * 2.0);
        let delta = match mode {
            DialInputMode::Circular => {
                let angle = self.pointer_angle(next)? - self.pointer_angle(previous)?;
                ((angle + PI).rem_euclid(TAU) - PI) / self.sweep
            }
            DialInputMode::Horizontal => f64::from(next.x - previous.x) / diameter,
            DialInputMode::Vertical => f64::from(previous.y - next.y) / diameter,
        };
        (delta.is_finite() && delta != 0.0).then_some(delta)
    }
}
