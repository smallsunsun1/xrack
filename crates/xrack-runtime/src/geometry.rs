use crate::{Length, Property};
use std::{
    cell::{Cell, OnceCell},
    fmt,
    rc::Rc,
};

thread_local! {
    static NEXT_SCROLL_TARGET: Cell<u64> = const { Cell::new(0) };
}

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
struct ScrollInteraction {
    active: Property<bool>,
    pressed: Property<bool>,
    hovered: Property<bool>,
}

#[derive(Default)]
struct Geometry {
    values: [Property<Length>; 10],
    ready: Property<bool>,
    scroll_target_id: OnceCell<u64>,
    scroll_thumb: [Property<f64>; 2],
    scroll_interaction: OnceCell<ScrollInteraction>,
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
    // A queued request can outlive the target's scene node. A fresh handle
    // never inherits an old request even when its allocation reuses an address.
    #[expect(
        clippy::expect_used,
        reason = "scroll target IDs must stay unique even if the counter reaches its limit"
    )]
    pub fn scroll_target_id(&self) -> u64 {
        *self.0.scroll_target_id.get_or_init(|| {
            NEXT_SCROLL_TARGET.with(|next| {
                let id = next.get();
                next.set(id.checked_add(1).expect("scroll target ID exhausted"));
                id
            })
        })
    }

    pub fn matches_scroll_target(&self, id: u64) -> bool {
        self.0.scroll_target_id.get() == Some(&id)
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

    pub fn publish_scroll_thumb(&self, position: f64, size: f64) {
        self.0.scroll_thumb[0].set_if_changed(position);
        self.0.scroll_thumb[1].set_if_changed(size);
    }

    pub fn scroll_visual_position(&self) -> f64 {
        self.0.scroll_thumb[0].get(|| 0.0)
    }

    pub fn scroll_visual_size(&self) -> f64 {
        self.0.scroll_thumb[1].get(|| 0.0)
    }

    pub fn observes_scroll_interaction(&self) -> bool {
        self.0.scroll_interaction.get().is_some()
    }

    pub fn publish_scroll_interaction(&self, active: bool, pressed: bool, hovered: bool) {
        if let Some(state) = self.0.scroll_interaction.get() {
            state.active.set_if_changed(active);
            state.pressed.set_if_changed(pressed);
            state.hovered.set_if_changed(hovered);
        }
    }

    pub fn scroll_active(&self) -> bool {
        self.0
            .scroll_interaction
            .get_or_init(Default::default)
            .active
            .get(|| false)
    }

    pub fn scroll_pressed(&self) -> bool {
        self.0
            .scroll_interaction
            .get_or_init(Default::default)
            .pressed
            .get(|| false)
    }

    pub fn scroll_hovered(&self) -> bool {
        self.0
            .scroll_interaction
            .get_or_init(Default::default)
            .hovered
            .get(|| false)
    }

    pub fn clear(&self) {
        self.publish_scroll_interaction(false, false, false);
        self.publish_scroll_thumb(0.0, 0.0);
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
