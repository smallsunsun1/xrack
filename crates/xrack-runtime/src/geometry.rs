use crate::{Length, Property};
use std::{fmt, rc::Rc};

#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub enum GeometryMetric {
    LayoutX,
    LayoutY,
    LayoutWidth,
    LayoutHeight,
    WindowX,
    WindowY,
    WindowWidth,
    WindowHeight,
    AvailableWidth,
    AvailableHeight,
}

impl GeometryMetric {
    #[doc(hidden)]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "layout-x" => Self::LayoutX,
            "layout-y" => Self::LayoutY,
            "layout-width" => Self::LayoutWidth,
            "layout-height" => Self::LayoutHeight,
            "window-bounds-x" => Self::WindowX,
            "window-bounds-y" => Self::WindowY,
            "window-bounds-width" => Self::WindowWidth,
            "window-bounds-height" => Self::WindowHeight,
            "available-width" => Self::AvailableWidth,
            "available-height" => Self::AvailableHeight,
            _ => return None,
        })
    }
}

#[derive(Default)]
struct Geometry {
    values: [Property<Length>; 10],
    ready: Property<bool>,
}

/// Identity and independently tracked geometry of one mounted element.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct GeometryHandle(Rc<Geometry>);

impl PartialEq for GeometryHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for GeometryHandle {}

impl fmt::Debug for GeometryHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeometryHandle").finish_non_exhaustive()
    }
}

impl GeometryHandle {
    pub fn identity(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
    pub fn read(&self, metric: GeometryMetric) -> Length {
        self.0.values[metric as usize].get(Length::default)
    }

    pub fn ready(&self) -> bool {
        self.0.ready.get(|| false)
    }

    pub fn publish(&self, layout: [f32; 4], window: [f32; 4], ready: bool) {
        self.publish_with_content(layout, window, [0.0; 2], ready);
    }

    pub fn publish_with_content(
        &self,
        layout: [f32; 4],
        window: [f32; 4],
        available: [f32; 2],
        ready: bool,
    ) {
        for (property, value) in self
            .0
            .values
            .iter()
            .zip(layout.into_iter().chain(window).chain(available))
        {
            let value = if value.is_finite() {
                f64::from(value)
            } else {
                0.0
            };
            property.set_if_changed(Length(value));
        }
        self.0.ready.set_if_changed(ready);
    }

    pub fn clear(&self) {
        self.publish([0.0; 4], [0.0; 4], false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_geometry_field_invalidates_only_its_readers() {
        let geometry = GeometryHandle::default();
        geometry.publish([1.0, 2.0, 100.0, 40.0], [1.0, 2.0, 100.0, 40.0], true);
        let local = Property::default();
        let window = Property::default();
        let read_local = || local.get(|| geometry.read(GeometryMetric::LayoutWidth));
        let read_window = || window.get(|| geometry.read(GeometryMetric::WindowWidth));
        assert_eq!(
            (read_local(), read_window()),
            (Length(100.0), Length(100.0))
        );
        geometry.publish([1.0, 2.0, 100.0, 40.0], [5.0, 6.0, 40.0, 100.0], true);
        assert!(!local.is_dirty());
        assert!(window.is_dirty());
        assert_eq!(read_window(), Length(40.0));
        geometry.clear();
        assert!(!geometry.ready());
        assert_eq!(read_local(), Length(0.0));
    }
}
