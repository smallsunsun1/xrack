use crate::{Length, Property};
use std::{fmt, rc::Rc};

/// A cumulative scale and clockwise rotation in degrees, with a local centroid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaleRotateSample {
    pub scale: f64,
    pub rotation: f64,
    pub center_x: Length,
    pub center_y: Length,
}

impl Default for ScaleRotateSample {
    fn default() -> Self {
        Self {
            scale: 1.0,
            rotation: 0.0,
            center_x: Length(0.0),
            center_y: Length(0.0),
        }
    }
}

#[derive(Clone, Default)]
pub struct ScaleRotateHandle(Rc<Properties>);

#[derive(Default)]
struct Properties {
    active: Property<bool>,
    scale: Property<f64>,
    rotation: Property<f64>,
    center_x: Property<Length>,
    center_y: Property<Length>,
}

impl fmt::Debug for ScaleRotateHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScaleRotateHandle")
            .finish_non_exhaustive()
    }
}

impl PartialEq for ScaleRotateHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl ScaleRotateHandle {
    pub fn active(&self) -> bool {
        self.0.active.get(|| false)
    }
    pub fn scale(&self) -> f64 {
        self.0.scale.get(|| 1.0)
    }
    pub fn rotation(&self) -> f64 {
        self.0.rotation.get(f64::default)
    }
    pub fn center_x(&self) -> Length {
        self.0.center_x.get(Length::default)
    }
    pub fn center_y(&self) -> Length {
        self.0.center_y.get(Length::default)
    }

    #[doc(hidden)]
    pub fn publish(&self, sample: Option<ScaleRotateSample>) {
        self.0.active.set_if_changed(sample.is_some());
        let sample = sample.unwrap_or_default();
        self.0.scale.set_if_changed(sample.scale);
        self.0.rotation.set_if_changed(sample.rotation);
        self.0.center_x.set_if_changed(sample.center_x);
        self.0.center_y.set_if_changed(sample.center_y);
    }
}
