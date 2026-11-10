use crate::{Interaction, LayoutNode, Point};
use xrack_runtime::{Length, Node, RowKey, ScaleRotateHandle, ScaleRotateSample};

#[derive(Clone, Debug, PartialEq)]
pub struct ScaleRotateTarget {
    pub path: Vec<usize>,
    pub handle: ScaleRotateHandle,
    row: Option<RowKey>,
}

impl ScaleRotateTarget {
    pub(crate) fn new(scene: &Node, path: &[usize]) -> Option<Self> {
        let handle = scene
            .interactive_at_path(path)?
            .scale_rotate_handle
            .clone()?;
        Some(Self {
            path: path.to_vec(),
            handle,
            row: scene.row_key_at(path),
        })
    }

    pub fn is_live(&self, scene: &Node, layout: &LayoutNode, input: &Interaction) -> bool {
        gesture_target_live(scene, layout, input, &self.path, self.row)
            && scene
                .interactive_at_path(&self.path)
                .and_then(|node| node.scale_rotate_handle.as_ref())
                == Some(&self.handle)
    }

    pub fn sample(
        &self,
        layout: &LayoutNode,
        center: Point,
        scale: f64,
        rotation: f64,
    ) -> Option<ScaleRotateSample> {
        let item = layout.at_path(&self.path)?;
        let center = item.map_from_window(center)?;
        Some(ScaleRotateSample {
            scale,
            rotation,
            center_x: Length(f64::from(center.x - item.bounds.x)),
            center_y: Length(f64::from(center.y - item.bounds.y)),
        })
    }
}

pub(crate) fn gesture_target_live(
    scene: &Node,
    layout: &LayoutNode,
    input: &Interaction,
    path: &[usize],
    row: Option<RowKey>,
) -> bool {
    !input.menu_open()
        && !input.choice_open()
        && input.allows_popup(layout, path)
        && scene.row_key_at(path) == row
        && layout.at_path(path).is_some_and(|item| {
            let bounds = item.visible_window_bounds(item.bounds);
            item.visible && item.enabled && bounds.width > 0.0 && bounds.height > 0.0
        })
}

/// Track angle deltas across the ±180° boundary while measuring scale from the start.
pub struct TouchTransform {
    initial_span: f64,
    last_angle: f64,
    rotation: f64,
}

impl TouchTransform {
    pub fn new(points: [Point; 2]) -> Option<Self> {
        let (span, angle) = polar(points)?;
        Some(Self {
            initial_span: span,
            last_angle: angle,
            rotation: 0.0,
        })
    }

    pub fn update(&mut self, points: [Point; 2]) -> Option<(f64, f64)> {
        let (span, angle) = polar(points)?;
        let delta = (angle - self.last_angle + 180.0).rem_euclid(360.0) - 180.0;
        self.last_angle = angle;
        self.rotation += delta;
        Some((span / self.initial_span, self.rotation))
    }
}

pub fn touch_centroid([a, b]: [Point; 2]) -> Point {
    Point {
        x: a.x * 0.5 + b.x * 0.5,
        y: a.y * 0.5 + b.y * 0.5,
    }
}

fn polar([a, b]: [Point; 2]) -> Option<(f64, f64)> {
    let x = f64::from(b.x) - f64::from(a.x);
    let y = f64::from(b.y) - f64::from(a.y);
    let span = x.hypot(y);
    (span >= 1.0 && span.is_finite()).then(|| (span, y.atan2(x).to_degrees()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_rotation_accumulates_through_the_angle_boundary() {
        let pair = |angle: f64| {
            let angle = angle.to_radians();
            [
                Point::default(),
                Point {
                    x: (40.0 * angle.cos()) as f32,
                    y: (40.0 * angle.sin()) as f32,
                },
            ]
        };
        let mut transform = TouchTransform::new(pair(170.0)).unwrap();
        assert!(transform.update([Point::default(); 2]).is_none());
        let (scale, rotation) = transform.update(pair(-170.0)).unwrap();
        assert!((scale - 1.0).abs() < 1e-6);
        assert!((rotation - 20.0).abs() < 1e-5);
        for angle in [-90.0, 0.0, 90.0, 170.0] {
            transform.update(pair(angle));
        }
        let (_, rotation) = transform.update(pair(-170.0)).unwrap();
        assert!((rotation - 380.0).abs() < 1e-5);
    }
}
