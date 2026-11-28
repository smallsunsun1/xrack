use crate::{
    Hit3D, Length, Point2D, Point3D, Property, RenderStats3D, Vector3D, ViewportGpuStats,
    scene3d::{SceneModel, SceneTextureSources, camera::SceneViewQuery},
};
use std::{
    cell::{Cell, OnceCell, RefCell},
    fmt,
    rc::Rc,
    sync::{Arc, Weak},
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
struct SceneViewObservation {
    query: Property<SceneViewQuery>,
    texture_sources: Option<Property<Option<Arc<SceneTextureSources>>>>,
}

#[derive(Default)]
struct ScenePreloadObservation {
    requested: RefCell<Option<Weak<SceneModel>>>,
    ready: Property<bool>,
    observed: Cell<bool>,
}

#[derive(Default)]
struct Geometry {
    values: [Property<Length>; 10],
    ready: Property<bool>,
    scene_view: Option<Box<SceneViewObservation>>,
    scene_texture_size: OnceCell<Box<[Property<u32>; 2]>>,
    scene_preload: OnceCell<ScenePreloadObservation>,
    render_stats: OnceCell<Box<Property<RenderStats3D>>>,
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

#[doc(hidden)]
pub struct SceneTouchRequest {
    pub view: GeometryHandle,
    pub source: String,
    pub position: Point2D,
    pub id: i64,
    pub pressed: bool,
}

impl GeometryHandle {
    pub fn request_scene_touch(&self, source: String, position: Point2D, id: i64, pressed: bool) {
        crate::focus::FocusHandle::enqueue(crate::QueuedFocusRequest::SceneTouch(
            SceneTouchRequest {
                view: self.clone(),
                source,
                position,
                id,
                pressed,
            },
        ));
    }

    pub fn for_scene_view(observe_texture_sources: bool) -> Self {
        Self(Rc::new(Geometry {
            scene_view: Some(Box::new(SceneViewObservation {
                texture_sources: observe_texture_sources.then(Property::default),
                ..Default::default()
            })),
            ..Geometry::default()
        }))
    }

    pub fn with_scene_texture_size(self) -> Self {
        self.0.scene_texture_size.get_or_init(Box::default);
        self
    }

    pub fn observes_scene_texture_size(&self) -> bool {
        self.0.scene_texture_size.get().is_some()
    }

    pub fn with_render_stats(self) -> Self {
        self.0.render_stats.get_or_init(Box::default);
        self
    }

    pub fn render_stats(&self) -> RenderStats3D {
        self.0
            .render_stats
            .get()
            .map_or_else(Default::default, |stats| stats.get(Default::default))
    }

    pub fn needs_render_stats(&self, revision: i64) -> bool {
        self.0.render_stats.get().is_some_and(|stats| {
            let previous = stats.cached_value().unwrap_or_default();
            if revision > 0 {
                !previous.valid || previous.revision != revision
            } else {
                previous.valid
            }
        })
    }

    pub fn publish_render_stats(&self, sample: RenderStats3D) -> bool {
        let Some(stats) = self.0.render_stats.get() else {
            return false;
        };
        if stats.cached_value().unwrap_or_default() == sample {
            return false;
        }
        stats.set_if_changed(sample);
        true
    }

    pub fn publish_render_stats_gpu(
        &self,
        revision: i64,
        frame: u64,
        gpu: &ViewportGpuStats,
        truncated: bool,
    ) -> bool {
        let current = self
            .0
            .render_stats
            .get()
            .and_then(|stats| stats.cached_value());
        let Some(current) = current.filter(|sample| sample.matches(revision, frame)) else {
            return false;
        };
        self.publish_render_stats(current.completed(gpu, truncated))
    }

    pub fn scene_texture_size(&self, axis: usize) -> i64 {
        i64::from(
            self.0
                .scene_texture_size
                .get()
                .and_then(|size| size.get(axis))
                .map_or(0, |side| side.get(|| 0)),
        )
    }

    pub fn publish_scene_texture_size(&self, size: [u32; 2]) -> bool {
        let Some(properties) = self.0.scene_texture_size.get() else {
            return false;
        };
        let mut changed = false;
        for (property, side) in properties.iter().zip(size) {
            if property.cached_value().unwrap_or(0) != side {
                property.set_if_changed(side);
                changed = true;
            }
        }
        changed
    }

    pub fn preload_ready(&self) -> bool {
        let observation = self.0.scene_preload.get_or_init(Default::default);
        observation.observed.set(true);
        observation.ready.get(|| false)
    }

    pub fn publish_preload_request(&self, model: Option<&Arc<SceneModel>>) {
        let observation = match model {
            Some(_) => self.0.scene_preload.get_or_init(Default::default),
            None => match self.0.scene_preload.get() {
                Some(observation) => observation,
                None => return,
            },
        };
        let mut requested = observation.requested.borrow_mut();
        let before = requested.as_ref().map(Weak::as_ptr);
        if before != model.map(Arc::as_ptr) {
            *requested = model.map(Arc::downgrade);
            observation.ready.set_if_changed(false);
        }
    }

    pub fn publish_preload_ready(&self, model: &Arc<SceneModel>, ready: bool) -> bool {
        self.publish_preload_request(Some(model));
        let Some(observation) = self.0.scene_preload.get() else {
            return false;
        };
        if observation.ready.cached_value().unwrap_or(false) == ready {
            return false;
        }
        observation.ready.set_if_changed(ready);
        observation.observed.get()
    }

    pub fn publish_scene_view(
        &self,
        node: &crate::Node,
        viewport: [f64; 2],
        texture_sources: impl FnOnce() -> Option<Arc<SceneTextureSources>>,
    ) {
        if let Some(view) = &self.0.scene_view {
            view.query
                .set_if_changed(SceneViewQuery::for_view(node, viewport));
            if let Some(sources) = &view.texture_sources {
                sources.set_if_changed(texture_sources());
            }
        }
    }

    pub fn scene_view(&self) -> SceneViewQuery {
        self.0
            .scene_view
            .as_ref()
            .map_or_else(Default::default, |view| view.query.get(Default::default))
    }

    pub fn scene_item_hit(&self, point: Point2D) -> Hit3D {
        self.query_item(|view, sources| view.hit_item(point, sources))
    }

    pub fn scene_item_ray_hit(&self, origin: Point3D, direction: Vector3D, max: f64) -> Hit3D {
        self.query_item(|view, sources| view.ray_hit_item(origin, direction, max, sources))
    }

    fn query_item(
        &self,
        query: impl FnOnce(SceneViewQuery, &SceneTextureSources) -> Hit3D,
    ) -> Hit3D {
        self.0
            .scene_view
            .as_ref()
            .and_then(|view| {
                let sources = view.texture_sources.as_ref()?.get(|| None)?;
                Some(query(view.query.get(Default::default), &sources))
            })
            .unwrap_or_default()
    }

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
        if let Some(view) = &self.0.scene_view {
            view.query.set_if_changed(Default::default());
            if let Some(sources) = &view.texture_sources {
                sources.set_if_changed(None);
            }
        }
        self.publish_preload_request(None);
        self.publish_scene_texture_size([0; 2]);
        self.publish_render_stats(RenderStats3D::default());
        self.publish_scroll_interaction(false, false, false);
        self.publish_scroll_thumb(0.0, 0.0);
        self.publish([0.0; 4], [0.0; 4], false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_stats_readback_is_scoped_to_the_committed_revision_and_frame() {
        let handle = GeometryHandle::default().with_render_stats();
        assert!(handle.needs_render_stats(1));
        assert!(!handle.render_stats().valid);
        assert!(
            handle.publish_render_stats(RenderStats3D::from_data(crate::RenderStats3DData {
                revision: 1,
                frame: 2,
                valid: true,
                ..Default::default()
            },))
        );
        let gpu = ViewportGpuStats {
            path: Vec::new(),
            id: None,
            gpu_nanos: 123,
            passes: Vec::new(),
        };
        assert!(!handle.publish_render_stats_gpu(1, 1, &gpu, false));
        assert!(!handle.publish_render_stats_gpu(2, 2, &gpu, false));
        assert!(handle.publish_render_stats_gpu(1, 2, &gpu, false));
        assert_eq!(handle.render_stats().gpu_nanos, 123);
        assert!(!handle.needs_render_stats(1));
        assert!(handle.needs_render_stats(2));
        handle.clear();
        assert!(!handle.render_stats().valid);
        assert!(!handle.publish_render_stats_gpu(1, 2, &gpu, false));
    }

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
