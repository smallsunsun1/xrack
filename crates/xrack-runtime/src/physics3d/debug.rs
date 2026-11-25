//! Shared reactive snapshots of solver collider outlines.

use std::{collections::VecDeque, sync::Arc};

use rapier3d::{
    pipeline::{
        DebugColor, DebugRenderBackend, DebugRenderMode, DebugRenderObject, DebugRenderPipeline,
    },
    prelude::{RigidBodySet, Vector},
};

use super::PhysicsWorld3D;
use crate::{Node, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsDebugVertex {
    pub position: [f32; 3],
    pub color: [u8; 4],
}

#[derive(Clone, Debug)]
pub struct PhysicsDebugLines(Arc<[PhysicsDebugVertex]>);

impl PhysicsDebugLines {
    pub fn vertices(&self) -> &Arc<[PhysicsDebugVertex]> {
        &self.0
    }
}

impl PartialEq for PhysicsDebugLines {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

struct Snapshot {
    body: i64,
    epoch: u64,
    lines: PhysicsDebugLines,
}

pub(super) struct PhysicsDebugCache {
    pipeline: DebugRenderPipeline,
    snapshots: VecDeque<Snapshot>,
    epoch: u64,
}

impl PhysicsDebugCache {
    fn new() -> Self {
        Self {
            pipeline: DebugRenderPipeline::new(
                Default::default(),
                DebugRenderMode::COLLIDER_SHAPES,
            ),
            snapshots: VecDeque::new(),
            epoch: 0,
        }
    }

    pub(super) fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }
}

impl PhysicsWorld3D {
    /// Return the current solver collider outlines. A body handle of zero selects the entire world.
    pub fn debug_lines(&self, body: i64) -> Option<PhysicsDebugLines> {
        let state = self.0.as_ref()?;
        state.changed.track();
        let mut simulation = state.state.try_borrow_mut().ok()?;
        if body < 0 || (body > 0 && !simulation.bodies.contains_key(&body)) {
            return None;
        }
        let simulation = &mut *simulation;
        let cache = simulation
            .debug
            .get_or_insert_with(|| Box::new(PhysicsDebugCache::new()));
        let previous = cache
            .snapshots
            .iter()
            .position(|item| item.body == body)
            .and_then(|index| cache.snapshots.remove(index));
        if let Some(previous) = &previous
            && previous.epoch == cache.epoch
        {
            let lines = previous.lines.clone();
            cache.snapshots.push_front(Snapshot {
                body,
                epoch: cache.epoch,
                lines: lines.clone(),
            });
            return Some(lines);
        }

        let world = &mut simulation.world;
        if simulation.query_bvh_dirty {
            world
                .bodies
                .propagate_modified_body_positions_to_colliders(&mut world.colliders);
        }
        let mut collector = Collector {
            bodies: &world.bodies,
            body,
            vertices: Vec::new(),
        };
        cache
            .pipeline
            .render_colliders(&mut collector, &world.bodies, &world.colliders);
        let lines = match previous {
            Some(item) if *item.lines.0 == collector.vertices => item.lines,
            _ => PhysicsDebugLines(collector.vertices.into()),
        };
        cache.snapshots.push_front(Snapshot {
            body,
            epoch: cache.epoch,
            lines: lines.clone(),
        });
        const CACHE_ENTRIES: usize = 4;
        cache.snapshots.truncate(CACHE_ENTRIES);
        Some(lines)
    }
}

impl Node {
    /// Project a view's debug setting while its reactive scene dependencies are being collected.
    #[doc(hidden)]
    pub fn prepare_physics_debug(&mut self) {
        let lines = if self.boolean("physics-debug", false) {
            let body = self.int("physics-debug-body").unwrap_or(0);
            match self.properties.get("physics-world") {
                Some(Value::PhysicsWorld3D(world)) => world.debug_lines(body),
                _ => None,
            }
        } else {
            None
        };
        if let Some(lines) = lines.filter(|lines| !lines.0.is_empty()) {
            self.properties
                .insert("__xr-physics-debug-lines", Value::PhysicsDebugLines(lines));
        } else if self.properties.contains_key("__xr-physics-debug-lines") {
            self.properties.remove("__xr-physics-debug-lines");
        }
    }
}

struct Collector<'a> {
    bodies: &'a RigidBodySet,
    body: i64,
    vertices: Vec<PhysicsDebugVertex>,
}

impl DebugRenderBackend for Collector<'_> {
    fn filter_object(&self, object: DebugRenderObject<'_>) -> bool {
        matches!(object, DebugRenderObject::Collider(_, collider)
            if collider.is_enabled() && (self.body == 0 || collider.user_data == self.body as u128))
    }

    fn draw_line(
        &mut self,
        object: DebugRenderObject<'_>,
        a: Vector,
        b: Vector,
        _color: DebugColor,
    ) {
        let DebugRenderObject::Collider(_, collider) = object else {
            return;
        };
        if !(a.is_finite() && b.is_finite()) {
            return;
        }
        let color = if collider.is_sensor() {
            [200, 148, 255, 242]
        } else {
            match collider.parent().and_then(|parent| self.bodies.get(parent)) {
                Some(body) if body.is_kinematic() => [88, 235, 214, 242],
                Some(body) if body.is_dynamic() => [255, 191, 104, 242],
                _ => [144, 192, 255, 222],
            }
        };
        self.vertices.extend([a, b].map(|point| PhysicsDebugVertex {
            position: point.to_array(),
            color,
        }));
    }
}

#[cfg(test)]
mod tests {
    use crate::{PhysicsWorld3D, Point3D, Vector3D};

    #[test]
    fn debug_snapshots_follow_committed_shapes_and_reuse_stationary_geometry() {
        let zero = Vector3D::default();
        let origin = Point3D::default();
        let world = PhysicsWorld3D::new(zero, 0.016);
        let size = Vector3D::new(2.0, 2.0, 2.0);
        let body = world.add_box(origin, size, 1.0, 0.0, 0.0);
        let sensor = world.add_box_trigger(Point3D::new(-4.0, 0.0, 0.0), size, zero);
        assert!(body > 0 && sensor > 0);
        let state = world.0.as_ref().expect("simulation exists");
        assert!(state.state.borrow().debug.is_none());

        let solid = world.debug_lines(body).expect("body outline");
        assert_eq!(solid.vertices().len(), 24);
        assert!(
            solid
                .vertices()
                .iter()
                .all(|vertex| vertex.color == [255, 191, 104, 242])
        );
        let all = world.debug_lines(0).expect("world outline");
        assert_eq!(all.vertices().len(), 48);
        assert_eq!(world.debug_lines(body).as_ref(), Some(&solid));
        assert!(world.set_damping(body, 0.1, 0.0));
        assert_eq!(world.debug_lines(body).as_ref(), Some(&solid));

        assert!(world.reset_body(body, Point3D::new(5.0, 0.0, 0.0), zero));
        let moved = world.debug_lines(body).expect("moved outline");
        assert_ne!(solid, moved);
        let min_x = moved
            .vertices()
            .iter()
            .map(|p| p.position[0])
            .fold(f32::INFINITY, f32::min);
        let max_x = moved
            .vertices()
            .iter()
            .map(|p| p.position[0])
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!((min_x, max_x), (4.0, 6.0));
        assert!(world.set_kinematic(body, true));
        let kinematic = world.debug_lines(body).expect("kinematic outline");
        assert!(
            kinematic
                .vertices()
                .iter()
                .all(|vertex| vertex.color == [88, 235, 214, 242])
        );
        assert!(world.remove(body));
        assert!(world.debug_lines(body).is_none());
        let sensor = world.debug_lines(sensor).expect("sensor outline");
        assert!(
            sensor
                .vertices()
                .iter()
                .all(|vertex| vertex.color == [200, 148, 255, 242])
        );
    }
}
