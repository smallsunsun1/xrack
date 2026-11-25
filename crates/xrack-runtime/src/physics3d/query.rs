//! Spatial queries against the same collider geometry used by the solver.

use std::{cmp::Ordering, collections::BTreeMap};

use rapier3d::{
    parry::{
        bounding_volume::Aabb,
        partitioning::{Bvh, BvhBuildStrategy, TraversalAction},
        query::{RayCast, RayIntersection, ShapeCastOptions},
        shape::SharedShape,
    },
    prelude::{Collider, Pose, QueryFilter, QueryPipeline, Ray, Vector},
};

use super::{
    PhysicsCollider3D, PhysicsWorld3D, Simulation, contacts::as_vector, scene_pose, vector,
};
use crate::{Point3D, Vector3D};

/// One query contact on a rigid body. A body handle of zero denotes a miss.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PhysicsHit3D {
    pub body: i64,
    pub distance: f64,
    pub position: Point3D,
    pub normal: Vector3D,
    pub sensor: bool,
}

/// Select which colliders a physics query will consider.
#[derive(Clone, Copy, Debug)]
pub struct PhysicsQueryFilter3D {
    group_mask: i64,
    ignore_body: i64,
    include_sensors: bool,
}

impl PhysicsQueryFilter3D {
    pub const fn new(group_mask: i64, ignore_body: i64, include_sensors: bool) -> Self {
        Self {
            group_mask,
            ignore_body,
            include_sensors,
        }
    }
}

impl Default for PhysicsQueryFilter3D {
    fn default() -> Self {
        Self::new(u32::MAX as i64, 0, false)
    }
}

struct QueryVolume {
    shape: SharedShape,
    pose: Pose,
}

impl QueryVolume {
    fn new(center: Point3D, rotation: Vector3D, shapes: PhysicsCollider3D) -> Option<Self> {
        let (shape, local) = shapes.into_convex_shape()?;
        let pose = scene_pose(center, rotation)? * local;
        let bounds = shape.compute_aabb(&pose);
        (bounds.mins.is_finite() && bounds.maxs.is_finite()).then_some(Self { shape, pose })
    }
}

struct ShapeSweep {
    volume: QueryVolume,
    ray: RaySegment,
    local_bounds: Aabb,
}

impl ShapeSweep {
    fn new(volume: QueryVolume, direction: Vector3D, max_distance: f64) -> Option<Self> {
        let ray = RaySegment::from_vector(volume.pose.translation, direction, max_distance)?;
        let centered = Pose {
            translation: Vector::ZERO,
            ..volume.pose
        };
        let local_bounds = volume.shape.compute_aabb(&centered);
        let end = ray.ray.point_at(ray.max_distance);
        ((end + local_bounds.mins).is_finite() && (end + local_bounds.maxs).is_finite()).then_some(
            Self {
                volume,
                ray,
                local_bounds,
            },
        )
    }

    fn visit(&self, query: QueryPipeline<'_>, mut hit: impl FnMut((PhysicsHit3D, u32)) -> f32) {
        let mut limit = self.ray.max_distance;
        let shape = self.volume.shape.as_ref();
        query.bvh.traverse(|node| {
            // Sweep the volume's center against the Minkowski-expanded bounds.
            let bounds = node.aabb();
            let expanded = Aabb::new(
                bounds.mins - self.local_bounds.maxs,
                bounds.maxs - self.local_bounds.mins,
            );
            if !expanded.intersects_local_ray(&self.ray.ray, limit) {
                return TraversalAction::Prune;
            }
            if let Some(leaf) = node.leaf_data()
                && let Some((collider, handle)) = query.colliders.get_unknown_gen(leaf)
                && query.filter.test(query.bodies, handle, collider)
            {
                let pose = collider.position();
                let relative = pose.inv_mul(&self.volume.pose);
                let velocity = pose.rotation.inverse() * self.ray.ray.dir;
                let options = ShapeCastOptions {
                    compute_impact_geometry_on_penetration: true,
                    ..ShapeCastOptions::with_max_time_of_impact(limit)
                };
                if let Ok(Some(contact)) = query.dispatcher.cast_shapes(
                    &relative,
                    velocity,
                    collider.shape(),
                    shape,
                    options,
                ) && let Some(contact) = contact_hit(
                    collider,
                    contact.time_of_impact,
                    pose.transform_point(contact.witness1),
                    pose.rotation * contact.normal1,
                ) {
                    limit = limit.min(hit((contact, leaf)));
                }
            }
            TraversalAction::Continue
        });
    }
}

struct RaySegment {
    ray: Ray,
    max_distance: f32,
}

impl RaySegment {
    fn new(origin: Point3D, direction: Vector3D, max_distance: f64) -> Option<Self> {
        let origin = vector([origin.x, origin.y, origin.z])?;
        Self::from_vector(origin, direction, max_distance)
    }

    fn from_vector(origin: Vector, direction: Vector3D, max_distance: f64) -> Option<Self> {
        let scale = direction
            .x
            .abs()
            .max(direction.y.abs())
            .max(direction.z.abs());
        let direction = vector([
            direction.x / scale,
            direction.y / scale,
            direction.z / scale,
        ])?
        .try_normalize()?;
        let ray = Ray::new(origin, direction);
        let distance = max_distance as f32;
        (max_distance >= 0.0 && distance.is_finite() && ray.point_at(distance).is_finite())
            .then_some(Self {
                ray,
                max_distance: distance,
            })
    }

    fn hit(&self, collider: &Collider, intersection: RayIntersection) -> Option<PhysicsHit3D> {
        let distance = intersection.time_of_impact;
        let point = self.ray.point_at(distance);
        contact_hit(collider, distance, point, intersection.normal)
    }
}

impl PhysicsWorld3D {
    /// Find the nearest collider surface. Geometry is queried at its current pose.
    pub fn ray_cast(
        &self,
        origin: Point3D,
        direction: Vector3D,
        max_distance: f64,
        group_mask: i64,
        ignore_body: i64,
        include_sensors: bool,
    ) -> PhysicsHit3D {
        let Some(ray) = RaySegment::new(origin, direction, max_distance) else {
            return PhysicsHit3D::default();
        };
        let filter = PhysicsQueryFilter3D::new(group_mask, ignore_body, include_sensors);
        self.with_query(filter, |query| {
            let mut best = None::<(PhysicsHit3D, u32)>;
            let mut limit = ray.max_distance;
            query.bvh.traverse(|node| {
                if !node.aabb().intersects_local_ray(&ray.ray, limit) {
                    return TraversalAction::Prune;
                }
                if let Some(leaf) = node.leaf_data()
                    && let Some((collider, handle)) = query.colliders.get_unknown_gen(leaf)
                    && query.filter.test(query.bodies, handle, collider)
                    && let Some(contact) = collider.shape().cast_ray_and_get_normal(
                        collider.position(),
                        &ray.ray,
                        limit,
                        false,
                    )
                    && let Some(hit) = ray.hit(collider, contact)
                    && best
                        .as_ref()
                        .is_none_or(|best| hit_order(&(hit, leaf), best).is_lt())
                {
                    limit = contact.time_of_impact;
                    best = Some((hit, leaf));
                }
                TraversalAction::Continue
            });
            best.map(|(hit, _)| hit).unwrap_or_default()
        })
        .unwrap_or_default()
    }

    /// Return one nearest surface per intersected body, ordered by distance and body handle.
    pub fn ray_cast_all(
        &self,
        origin: Point3D,
        direction: Vector3D,
        max_distance: f64,
        group_mask: i64,
        ignore_body: i64,
        include_sensors: bool,
    ) -> Vec<PhysicsHit3D> {
        let Some(ray) = RaySegment::new(origin, direction, max_distance) else {
            return Vec::new();
        };
        let filter = PhysicsQueryFilter3D::new(group_mask, ignore_body, include_sensors);
        self.with_query(filter, |query| {
            let mut per_body = BTreeMap::new();
            for (handle, collider, contact) in query.intersect_ray(ray.ray, ray.max_distance, false)
            {
                if let Some(hit) = ray.hit(collider, contact) {
                    let candidate = (hit, handle.into_raw_parts().0);
                    keep_nearest_for_body(&mut per_body, candidate);
                }
            }
            sorted_hits(per_body)
        })
        .unwrap_or_default()
    }

    /// Move a convex or compound volume without rotation and return its nearest contact.
    pub fn shape_cast(
        &self,
        center: Point3D,
        rotation: Vector3D,
        shapes: PhysicsCollider3D,
        direction: Vector3D,
        max_distance: f64,
        filter: PhysicsQueryFilter3D,
    ) -> PhysicsHit3D {
        let Some(sweep) = QueryVolume::new(center, rotation, shapes)
            .and_then(|volume| ShapeSweep::new(volume, direction, max_distance))
        else {
            return PhysicsHit3D::default();
        };
        self.with_query(filter, |query| {
            let mut best = None;
            sweep.visit(query, |candidate| {
                if best
                    .as_ref()
                    .is_none_or(|best| hit_order(&candidate, best).is_lt())
                {
                    best = Some(candidate);
                }
                candidate.0.distance as f32
            });
            best.map(|(hit, _)| hit).unwrap_or_default()
        })
        .unwrap_or_default()
    }

    /// Return one contact per intersected body, in travel order.
    pub fn shape_cast_all(
        &self,
        center: Point3D,
        rotation: Vector3D,
        shapes: PhysicsCollider3D,
        direction: Vector3D,
        max_distance: f64,
        filter: PhysicsQueryFilter3D,
    ) -> Vec<PhysicsHit3D> {
        let Some(sweep) = QueryVolume::new(center, rotation, shapes)
            .and_then(|volume| ShapeSweep::new(volume, direction, max_distance))
        else {
            return Vec::new();
        };
        self.with_query(filter, |query| {
            let mut per_body = BTreeMap::new();
            sweep.visit(query, |candidate| {
                keep_nearest_for_body(&mut per_body, candidate);
                sweep.ray.max_distance
            });
            sorted_hits(per_body)
        })
        .unwrap_or_default()
    }

    /// Return each overlapping body once, in handle order.
    pub fn shape_overlap(
        &self,
        center: Point3D,
        rotation: Vector3D,
        shapes: PhysicsCollider3D,
        filter: PhysicsQueryFilter3D,
    ) -> Vec<i64> {
        let Some(volume) = QueryVolume::new(center, rotation, shapes) else {
            return Vec::new();
        };
        self.with_query(filter, |query| {
            let mut bodies: Vec<_> = query
                .intersect_shape(volume.pose, volume.shape.as_ref())
                .map(|(_, collider)| collider.user_data as i64)
                .collect();
            bodies.sort_unstable();
            bodies.dedup();
            bodies
        })
        .unwrap_or_default()
    }

    fn with_query<T>(
        &self,
        filter: PhysicsQueryFilter3D,
        query: impl FnOnce(QueryPipeline<'_>) -> T,
    ) -> Option<T> {
        let PhysicsQueryFilter3D {
            group_mask,
            ignore_body,
            include_sensors,
        } = filter;
        let mask = u32::try_from(group_mask).ok()?;
        if mask == 0 {
            return None;
        }
        let state = self.0.as_ref()?;
        state.query_changed.track();
        let mut simulation = state.state.try_borrow_mut().ok()?;
        simulation.prepare_query_bvh();
        let world = &simulation.world;
        let accepts = |_, collider: &Collider| {
            collider.is_enabled() && (collider.collision_groups().memberships.bits() & mask != 0)
        };
        let mut filter = QueryFilter {
            exclude_rigid_body: simulation.bodies.get(&ignore_body).copied(),
            predicate: Some(&accepts),
            ..QueryFilter::default()
        };
        if !include_sensors {
            filter = filter.exclude_sensors();
        }
        let mut pipeline = world.broad_phase.as_query_pipeline(
            world.narrow_phase.query_dispatcher(),
            &world.bodies,
            &world.colliders,
            filter,
        );
        if let Some(bvh) = &simulation.query_bvh {
            pipeline.bvh = bvh;
        }
        Some(query(pipeline))
    }
}

impl Simulation {
    pub(super) fn invalidate_query_bvh(&mut self) {
        self.query_changed = true;
        self.query_bvh_dirty = true;
        self.query_bvh = None;
    }

    pub(super) fn use_stepped_query_bvh(&mut self) {
        self.query_changed = true;
        self.query_bvh_dirty = false;
        self.query_bvh = None;
    }

    pub(super) fn prepare_query_bvh(&mut self) {
        if self.query_bvh_dirty && self.query_bvh.is_none() {
            let world = &mut self.world;
            world
                .bodies
                .propagate_modified_body_positions_to_colliders(&mut world.colliders);
            self.query_bvh = Some(Bvh::from_iter(
                BvhBuildStrategy::Binned,
                world
                    .colliders
                    .iter()
                    .filter(|(_, collider)| collider.is_enabled())
                    .map(|(handle, collider)| {
                        (handle.into_raw_parts().0 as usize, collider.compute_aabb())
                    }),
            ));
        }
    }
}

fn contact_hit(
    collider: &Collider,
    distance: f32,
    point: Vector,
    normal: Vector,
) -> Option<PhysicsHit3D> {
    (distance.is_finite() && point.is_finite() && normal.is_finite()).then(|| PhysicsHit3D {
        body: collider.user_data as i64,
        distance: f64::from(distance),
        position: Point3D::new(point.x.into(), point.y.into(), point.z.into()),
        normal: as_vector(normal),
        sensor: collider.is_sensor(),
    })
}

fn keep_nearest_for_body(
    per_body: &mut BTreeMap<i64, (PhysicsHit3D, u32)>,
    candidate: (PhysicsHit3D, u32),
) {
    let best = per_body.entry(candidate.0.body).or_insert(candidate);
    if hit_order(&candidate, best).is_lt() {
        *best = candidate;
    }
}

fn sorted_hits(per_body: BTreeMap<i64, (PhysicsHit3D, u32)>) -> Vec<PhysicsHit3D> {
    let mut hits: Vec<_> = per_body.into_values().collect();
    hits.sort_by(hit_order);
    hits.into_iter().map(|(hit, _)| hit).collect()
}

fn hit_order(left: &(PhysicsHit3D, u32), right: &(PhysicsHit3D, u32)) -> Ordering {
    left.0
        .distance
        .total_cmp(&right.0.distance)
        .then_with(|| left.0.body.cmp(&right.0.body))
        .then_with(|| left.1.cmp(&right.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: i64 = u32::MAX as i64;

    #[test]
    fn ray_queries_share_current_collider_geometry_and_filter_by_group_and_body() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.1);
        let origin = Point3D::default();
        let direction = Vector3D::new(2.0, 0.0, 0.0);
        let size = Vector3D::new(1.0, 2.0, 2.0);
        let sensor = world.add_box_trigger(Point3D::new(1.0, 0.0, 0.0), size, Vector3D::default());
        let first = world.add_box(Point3D::new(3.0, 0.0, 0.0), size, 1.0, 0.0, 0.0);
        let tied = world.add_box(Point3D::new(3.0, 0.0, 0.0), size, 0.0, 0.0, 0.0);
        let last = world.add_box(Point3D::new(6.0, 0.0, 0.0), size, 0.0, 0.0, 0.0);
        assert!(world.set_collision_filter(last, 3, ALL));

        let closest = world.ray_cast(origin, direction, 9.0, ALL, 0, false);
        assert_eq!(closest.body, first);
        assert_eq!(closest.distance, 2.5);
        assert_eq!(closest.position, Point3D::new(2.5, 0.0, 0.0));
        assert_eq!(closest.normal, Vector3D::new(-1.0, 0.0, 0.0));
        let hits = world.ray_cast_all(origin, direction, 9.0, ALL, 0, true);
        assert_eq!(
            hits.iter().map(|hit| hit.body).collect::<Vec<_>>(),
            [sensor, first, tied, last]
        );
        assert!(hits[0].sensor);
        assert_eq!(hits[1], closest);
        assert_eq!(
            world
                .ray_cast(origin, direction, 9.0, ALL, first, false)
                .body,
            tied
        );
        assert_eq!(
            world
                .ray_cast(origin, direction, 9.0, 1 << 3, 0, false)
                .body,
            last
        );
        assert_eq!(
            world.ray_cast(origin, direction, 2.0, ALL, 0, false).body,
            0
        );
        assert_eq!(
            world
                .ray_cast(origin, Vector3D::default(), 9.0, ALL, 0, false)
                .body,
            0
        );
        assert!(
            world
                .ray_cast_all(origin, direction, 9.0, 0, 0, true)
                .is_empty()
        );

        assert!(world.reset_body(first, Point3D::new(2.0, 0.0, 0.0), Vector3D::default()));
        assert_eq!(
            world
                .ray_cast(origin, direction, 9.0, ALL, 0, false)
                .distance,
            1.5
        );
        assert!(world.set_linear_velocity(first, Vector3D::new(-5.0, 0.0, 0.0)));
        assert!(world.step());
        let moved = world.ray_cast(origin, direction, 9.0, ALL, 0, false);
        assert_eq!(moved.body, first);
        assert!((moved.distance - (world.position(first).x - 0.5)).abs() < 1e-6);
        assert!(world.remove(first));
        assert_eq!(
            world.ray_cast(origin, direction, 9.0, ALL, 0, false).body,
            tied
        );
        assert!(world.clear());
        assert!(
            world
                .ray_cast_all(origin, direction, 9.0, ALL, 0, true)
                .is_empty()
        );
    }

    #[test]
    fn ray_binding_tracks_collider_changes_instead_of_queued_loads() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.1);
        let body = world.add_sphere(Point3D::new(3.0, 0.0, 0.0), 0.5, 1.0, 0.0, 0.0);
        let bound = crate::Property::default();
        let read = || {
            world.ray_cast(
                Point3D::default(),
                Vector3D::new(1.0, 0.0, 0.0),
                10.0,
                1,
                0,
                false,
            )
        };
        assert_eq!(bound.get(read).distance, 2.5);
        assert!(world.apply_force(body, Vector3D::new(10.0, 0.0, 0.0)));
        assert!(!bound.is_dirty());
        assert!(world.step());
        assert!(bound.is_dirty());
        assert!(bound.get(read).distance > 2.5);
        assert!(world.set_collision_filter(body, 1, 0));
        assert!(bound.is_dirty());
        assert_eq!(bound.get(read).body, 0);
    }

    #[test]
    fn convex_sweeps_hit_off_axis_geometry_at_the_surface() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.02);
        let origin = Point3D::default();
        let rotation = Vector3D::default();
        let direction = Vector3D::new(1.0, 0.0, 0.0);
        let sensor = world.add_box_trigger(
            Point3D::new(1.0, 0.0, 0.0),
            Vector3D::new(0.2, 0.3, 0.3),
            rotation,
        );
        let first = world.add_box(
            Point3D::new(2.0, 0.5, 0.0),
            Vector3D::new(0.6, 0.4, 0.6),
            0.0,
            0.0,
            0.0,
        );
        let second = world.add_box(
            Point3D::new(4.0, 0.4, 0.0),
            Vector3D::new(0.6, 0.4, 0.6),
            0.0,
            0.0,
            0.0,
        );
        let sphere = || {
            PhysicsCollider3D::prepare(&crate::VecModel::from(vec![0]), |_| {
                crate::PhysicsShape3D::new("sphere", origin, Vector3D::new(0.9, 0.9, 0.9), [0.0; 3])
            })
        };
        assert_eq!(
            world.ray_cast(origin, direction, 6.0, ALL, 0, false).body,
            0
        );
        let filtered = PhysicsQueryFilter3D::default();
        let hit = world.shape_cast(origin, rotation, sphere(), direction, 6.0, filtered);
        assert_eq!(hit.body, first);
        assert!(
            (hit.distance - (1.7 - (0.45_f64.powi(2) - 0.3_f64.powi(2)).sqrt())).abs() < 0.001,
            "{hit:?}"
        );
        assert!(
            (hit.position.x - 1.7).abs() < 0.001 && (hit.position.y - 0.3).abs() < 0.001,
            "{hit:?}"
        );
        assert!(hit.normal.x < -0.6 && hit.normal.y < -0.6, "{hit:?}");
        assert_eq!(
            world
                .shape_cast(origin, rotation, sphere(), direction, 1.0, filtered)
                .body,
            0
        );
        let all = world.shape_cast_all(
            origin,
            rotation,
            sphere(),
            direction,
            6.0,
            PhysicsQueryFilter3D::new(ALL, 0, true),
        );
        assert_eq!(
            all.iter().map(|hit| hit.body).collect::<Vec<_>>(),
            [sensor, first, second]
        );
        assert!(all[0].sensor);
        assert_eq!(all[1], hit);
        let penetrating = world.shape_cast(
            Point3D::new(2.0, 0.5, 0.0),
            rotation,
            sphere(),
            direction,
            6.0,
            filtered,
        );
        assert_eq!(penetrating.body, first);
        assert_eq!(penetrating.distance, 0.0);
    }

    #[test]
    fn compound_volume_overlap_uses_rotation_and_filters_per_body() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.016);
        let origin = Point3D::default();
        let rotation = Vector3D::default();
        let probe_parts = crate::VecModel::from(vec![
            Point3D::new(-1.0, 0.0, 0.0),
            Point3D::new(1.0, 0.0, 0.0),
        ]);
        let probe = || {
            PhysicsCollider3D::prepare(&probe_parts, |center| {
                crate::PhysicsShape3D::new("box", *center, Vector3D::new(0.4, 0.4, 0.4), [0.0; 3])
            })
        };
        let body = world.add_sphere(Point3D::new(0.0, 0.0, 1.0), 0.3, 0.0, 0.0, 0.0);
        let sensor = world.add_box_trigger(
            Point3D::new(0.0, 0.0, -1.0),
            Vector3D::new(0.4, 0.4, 0.4),
            rotation,
        );
        assert!(world.set_collision_filter(sensor, 2, 0));
        let yaw = Vector3D::new(90.0, 0.0, 0.0);
        let all = PhysicsQueryFilter3D::new(ALL, 0, true);
        assert!(
            world
                .shape_overlap(origin, rotation, probe(), all)
                .is_empty()
        );
        assert_eq!(
            world.shape_overlap(origin, yaw, probe(), all),
            [body, sensor]
        );
        assert_eq!(
            world.shape_overlap(origin, yaw, probe(), PhysicsQueryFilter3D::new(4, 0, true)),
            [sensor]
        );
        assert_eq!(
            world.shape_overlap(
                origin,
                yaw,
                probe(),
                PhysicsQueryFilter3D::new(ALL, sensor, true)
            ),
            [body]
        );
        let hits = world.shape_cast_all(
            Point3D::new(0.0, 0.0, -3.0),
            yaw,
            probe(),
            Vector3D::new(0.0, 0.0, 1.0),
            7.0,
            all,
        );
        assert_eq!(
            hits.iter().map(|hit| hit.body).collect::<Vec<_>>(),
            [sensor, body]
        );
        assert!(hits[0].distance < hits[1].distance);
    }

    #[test]
    fn convex_queries_contact_static_meshes_and_planes() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.016);
        let origin = Point3D::default();
        let size = Vector3D::new(1.0, 1.0, 1.0);
        let zero = Vector3D::default();
        let prepare = |kind, mesh| {
            PhysicsCollider3D::prepare(&crate::VecModel::from(vec![0]), |_| {
                crate::PhysicsShape3D::new_with_mesh(kind, origin, size, [0.0; 3], mesh, |_| None)
            })
        };
        let mesh = world.add_body(
            origin,
            zero,
            prepare("mesh", "primitive:box"),
            0.0,
            0.0,
            0.0,
        );
        let plane = world.add_body(
            Point3D::new(0.0, -2.0, 0.0),
            zero,
            prepare("plane", ""),
            0.0,
            0.0,
            0.0,
        );
        assert!(mesh > 0 && plane > 0);
        let start = Point3D::new(0.0, 2.0, 0.0);
        let down = Vector3D::new(0.0, -1.0, 0.0);
        let filter = PhysicsQueryFilter3D::default();
        let hits = world.shape_cast_all(start, zero, prepare("sphere", ""), down, 6.0, filter);
        assert_eq!(
            hits.iter().map(|hit| hit.body).collect::<Vec<_>>(),
            [mesh, plane]
        );
        assert!((hits[0].position.y - 0.5).abs() < 1e-4, "{hits:?}");
        assert!((hits[1].position.y + 2.0).abs() < 1e-4, "{hits:?}");
        assert!(hits.iter().all(|hit| hit.normal.y > 0.999));
        assert_eq!(
            world
                .shape_cast(
                    start,
                    zero,
                    prepare("mesh", "primitive:box"),
                    down,
                    6.0,
                    filter
                )
                .body,
            0,
        );
        assert_eq!(
            world.shape_overlap(
                Point3D::new(0.0, 0.7, 0.0),
                zero,
                prepare("sphere", ""),
                filter
            ),
            [mesh],
        );
    }

    #[test]
    fn a_compound_body_has_one_hit_and_an_inside_origin_hits_its_exit_surface() {
        use crate::PhysicsShape3D;
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.016);
        let origin = Point3D::default();
        let direction = Vector3D::new(1.0, 0.0, 0.0);
        let rotation = Vector3D::default();
        let size = Vector3D::new(1.0, 1.0, 1.0);
        let centers = crate::VecModel::from(vec![origin, Point3D::new(2.0, 0.0, 0.0)]);
        let shapes = crate::PhysicsCollider3D::prepare(&centers, |center| {
            PhysicsShape3D::new("box", *center, size, [0.0; 3])
        });
        let body = world.add_body(origin, rotation, shapes, 0.0, 0.0, 0.0);
        assert!(body > 0);
        let hits = world.ray_cast_all(origin, direction, 6.0, ALL, 0, false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].distance, 0.5);
        assert_eq!(hits[0].body, body);
        assert_eq!(
            world.ray_cast(origin, direction, 6.0, ALL, 0, false),
            hits[0]
        );
    }
}
