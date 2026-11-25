//! Opt-in snapshots of solved and geometric rigid-body contacts.

use std::{
    collections::BTreeMap,
    sync::{Mutex, PoisonError},
};

use rapier3d::{
    parry::bounding_volume::BoundingVolume,
    prelude::{
        ActiveEvents, ColliderSet, CollisionEvent, ContactManifold, ContactPair, EventHandler,
        PhysicsWorld, QueryFilter, Real, RigidBodyHandle, RigidBodySet, SoftBodySet,
        SoftBodyTearEvent, Vector,
    },
};

use super::{PhysicsWorld3D, SensorEvents};
use crate::{Point3D, Vector3D};

/// One contact. The normal and any impulse point toward the receiving body.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PhysicsContact3D {
    /// The other body's handle, or zero for an empty value.
    pub body: i64,
    pub position: Point3D,
    pub normal: Vector3D,
    /// The world-space impulse in the report's normal convention.
    pub impulse: Vector3D,
    pub normal_impulse: f64,
}

#[derive(Clone, Copy, Default)]
struct Reports {
    send: bool,
    receive: bool,
}

#[derive(Default)]
pub(super) struct ContactReports {
    settings: BTreeMap<i64, Reports>,
    current: BTreeMap<i64, Vec<PhysicsContact3D>>,
    pending: Mutex<Vec<(i64, PhysicsContact3D)>>,
    static_kinematic: bool,
    kinematic_kinematic: bool,
    geometric_manifolds: Vec<ContactManifold>,
    changed: bool,
}

pub(super) struct StepEvents<'a> {
    pub(super) sensors: &'a SensorEvents,
    pub(super) contacts: &'a ContactReports,
}

impl PhysicsWorld3D {
    /// Configure reports for a solid body. Both directions are disabled initially.
    pub fn set_contact_reports(&self, body: i64, send: bool, receive: bool) -> bool {
        let configure = || {
            let mut simulation = self.0.as_ref()?.state.try_borrow_mut().ok()?;
            let simulation = &mut *simulation;
            if simulation.triggers.states.contains_key(&body) {
                return None;
            }
            let handle = *simulation.bodies.get(&body)?;
            let rigid = simulation.world.bodies.get(handle)?;
            for &handle in rigid.colliders() {
                if let Some(collider) = simulation.world.colliders.get_mut(handle) {
                    let mut events = collider.active_events();
                    events.set(ActiveEvents::CONTACT_FORCE_EVENTS, send);
                    collider.set_active_events(events);
                    if send {
                        collider.set_contact_force_event_threshold(0.0);
                    }
                }
            }
            if send || receive {
                simulation
                    .contacts
                    .settings
                    .insert(body, Reports { send, receive });
            } else {
                simulation.contacts.settings.remove(&body);
            }
            Some(())
        };
        configure().is_some()
    }

    /// Select contact reporting between bodies with authored or fixed motion.
    pub fn set_kinematic_contact_reports(
        &self,
        static_kinematic: bool,
        kinematic_kinematic: bool,
    ) -> bool {
        self.update_selectively(|simulation| {
            let reports = &mut simulation.contacts;
            if reports.static_kinematic != static_kinematic
                || reports.kinematic_kinematic != kinematic_kinematic
            {
                reports.static_kinematic = static_kinematic;
                reports.kinematic_kinematic = kinematic_kinematic;
                simulation.settings_changed = true;
            }
            Some((true, false))
        })
        .is_some()
    }

    pub fn report_static_kinematic_collisions(&self) -> bool {
        self.read_settings(|simulation| Some(simulation.contacts.static_kinematic))
    }

    pub fn report_kinematic_kinematic_collisions(&self) -> bool {
        self.read_settings(|simulation| Some(simulation.contacts.kinematic_kinematic))
    }

    /// Read contact points received during the latest completed step or batch.
    pub fn contacts(&self, body: i64) -> Vec<PhysicsContact3D> {
        let read = || {
            let state = self.0.as_ref()?;
            state.contacts_changed.track();
            let simulation = state.state.try_borrow().ok()?;
            simulation.contacts.current.get(&body).cloned()
        };
        read().unwrap_or_default()
    }
}

impl ContactReports {
    pub(super) fn kinematic_modes(&self) -> (bool, bool) {
        (self.static_kinematic, self.kinematic_kinematic)
    }

    pub(super) fn enabled(&self) -> bool {
        !self.settings.is_empty()
    }

    pub(super) fn finish_batch(&mut self) {
        let pending = self
            .pending
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        self.changed |=
            self.current.values().any(|points| !points.is_empty()) || !pending.is_empty();
        self.current.retain(|receiver, points| {
            points.clear();
            self.settings
                .get(receiver)
                .is_some_and(|setting| setting.receive)
        });
        for (receiver, point) in pending.drain(..) {
            self.current.entry(receiver).or_default().push(point);
        }
    }

    pub(super) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    pub(super) fn remove(&mut self, body: i64) {
        self.settings.remove(&body);
        self.changed |= self
            .current
            .remove(&body)
            .is_some_and(|points| !points.is_empty());
        for points in self.current.values_mut() {
            let count = points.len();
            points.retain(|point| point.body != body);
            self.changed |= count != points.len();
        }
    }

    pub(super) fn clear(&mut self) {
        self.settings.clear();
        self.changed |= self.current.values().any(|points| !points.is_empty());
        self.current.clear();
    }

    pub(super) fn collect_kinematic_contacts(
        &mut self,
        world: &PhysicsWorld,
        ids: &BTreeMap<i64, RigidBodyHandle>,
    ) {
        if !(self.static_kinematic || self.kinematic_kinematic) || self.settings.is_empty() {
            return;
        }
        let dispatcher = world.narrow_phase.query_dispatcher();
        let query = world.broad_phase.as_query_pipeline(
            dispatcher,
            &world.bodies,
            &world.colliders,
            QueryFilter::default().exclude_sensors(),
        );
        let pending = self
            .pending
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        let manifolds = &mut self.geometric_manifolds;
        // Walk participating kinematic bodies. Static/kinematic pairs have one
        // such body, and kinematic pairs are visited from the lower handle.
        for (&id, &participant) in &self.settings {
            let Some(&handle) = ids.get(&id) else {
                continue;
            };
            let Some(body) = world.bodies.get(handle).filter(|body| body.is_kinematic()) else {
                continue;
            };
            for &collider_handle in body.colliders() {
                let Some(first) = world
                    .colliders
                    .get(collider_handle)
                    .filter(|co| co.is_enabled())
                else {
                    continue;
                };
                let bounds = first.compute_aabb();
                for leaf in query.bvh.leaves(|node| node.aabb().intersects(&bounds)) {
                    let Some((second, _)) = query.colliders.get_unknown_gen(leaf) else {
                        continue;
                    };
                    if !second.is_enabled() || second.is_sensor() {
                        continue;
                    }
                    let other_id = second.user_data as i64;
                    let Some(other) = self.settings.get(&other_id).copied() else {
                        continue;
                    };
                    let receive = recipients(participant, other);
                    if !receive[0] && !receive[1] {
                        continue;
                    }
                    let Some(other_handle) = second.parent() else {
                        continue;
                    };
                    let Some(other_body) = world.bodies.get(other_handle) else {
                        continue;
                    };
                    let qualifying = match (other_body.is_fixed(), other_body.is_kinematic()) {
                        (true, _) => self.static_kinematic,
                        (_, true) => self.kinematic_kinematic && id < other_id,
                        _ => false,
                    };
                    if !qualifying
                        || !first.collision_groups().test(second.collision_groups())
                        || world
                            .impulse_joints
                            .joints_between(handle, other_handle)
                            .any(|(_, joint)| !joint.data.contacts_enabled())
                    {
                        continue;
                    }

                    let relative = first.position().inv_mul(second.position());
                    manifolds.clear();
                    if dispatcher
                        .contact_manifolds(
                            &relative,
                            first.shape(),
                            second.shape(),
                            0.0,
                            manifolds,
                            &mut None,
                        )
                        .is_err()
                    {
                        continue;
                    }
                    for manifold in manifolds.iter() {
                        let left_pose = manifold
                            .subshape_pos1()
                            .map_or(*first.position(), |local| *first.position() * *local);
                        let right_pose = manifold
                            .subshape_pos2()
                            .map_or(*second.position(), |local| *second.position() * *local);
                        let normal = left_pose.rotation * manifold.local_n1;
                        if !normal.is_finite() {
                            continue;
                        }
                        for point in manifold.points.iter().filter(|point| point.dist <= 0.0) {
                            let a = left_pose.transform_point(point.local_p1);
                            let b = right_pose.transform_point(point.local_p2);
                            append_contact(
                                pending,
                                [id, other_id],
                                receive,
                                a * 0.5 + b * 0.5,
                                normal,
                                0.0,
                            );
                        }
                    }
                }
            }
        }
        manifolds.clear();
    }

    fn collect(&self, bodies: &RigidBodySet, colliders: &ColliderSet, pair: &ContactPair) {
        let (Some(first), Some(second)) =
            (colliders.get(pair.collider1), colliders.get(pair.collider2))
        else {
            return;
        };
        let (first, second) = (first.user_data as i64, second.user_data as i64);
        let left = self.settings.get(&first).copied().unwrap_or_default();
        let right = self.settings.get(&second).copied().unwrap_or_default();
        let receive = recipients(left, right);
        if !receive[0] && !receive[1] {
            return;
        }
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        for manifold in pair.solver_manifolds() {
            for contact in &manifold.data.solver_contacts {
                let Some(point) = manifold.points.get(contact.contact_indices()[0] as usize) else {
                    continue;
                };
                let impulse = point.data.impulse;
                let normal = manifold.data.normal;
                if !(impulse.is_finite() && impulse > 0.0 && normal.is_finite()) {
                    continue;
                }
                let (a, b) = manifold.data.solver_contact_world_points(contact, bodies);
                append_contact(
                    &mut pending,
                    [first, second],
                    receive,
                    a * 0.5 + b * 0.5,
                    normal,
                    impulse,
                );
            }
        }
    }
}

fn recipients(first: Reports, second: Reports) -> [bool; 2] {
    [first.receive && second.send, second.receive && first.send]
}

fn append_contact(
    pending: &mut Vec<(i64, PhysicsContact3D)>,
    bodies: [i64; 2],
    receive: [bool; 2],
    position: Vector,
    normal: Vector,
    impulse: Real,
) {
    if !position.is_finite() {
        return;
    }
    let report = |other, normal: Vector| PhysicsContact3D {
        body: other,
        position: Point3D::new(position.x.into(), position.y.into(), position.z.into()),
        normal: as_vector(normal),
        impulse: as_vector(normal * impulse),
        normal_impulse: impulse.into(),
    };
    if receive[0] {
        pending.push((bodies[0], report(bodies[1], -normal)));
    }
    if receive[1] {
        pending.push((bodies[1], report(bodies[0], normal)));
    }
}

impl EventHandler for StepEvents<'_> {
    fn handle_collision_event(
        &self,
        bodies: &RigidBodySet,
        colliders: &ColliderSet,
        event: CollisionEvent,
        _pair: Option<&ContactPair>,
    ) {
        self.sensors
            .handle_collision_event(bodies, colliders, event);
    }

    fn handle_contact_force_event(
        &self,
        _dt: Real,
        bodies: &RigidBodySet,
        colliders: &ColliderSet,
        pair: &ContactPair,
        _force: Real,
    ) {
        self.contacts.collect(bodies, colliders, pair);
    }

    fn handle_soft_body_tear_event(&self, _bodies: &SoftBodySet, _event: &SoftBodyTearEvent) {}
}

pub(super) fn as_vector(value: Vector) -> Vector3D {
    Vector3D::new(value.x.into(), value.y.into(), value.z.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_kinematic_contacts_use_independent_world_modes_and_directional_reports() {
        let zero = Vector3D::default();
        let origin = Point3D::default();
        let size = Vector3D::new(1.0, 1.0, 1.0);
        let world = PhysicsWorld3D::new(zero, 0.02);
        let actor = world.add_kinematic_box(Point3D::new(-2.0, 0.0, 0.0), size, zero, 0.0, 0.0);
        let fixed = world.add_box(origin, size, 0.0, 0.0, 0.0);
        let other = world.add_kinematic_box(Point3D::new(-1.25, 0.0, 0.0), size, zero, 0.0, 0.0);
        assert!(world.set_contact_reports(actor, true, true));
        assert!(world.set_contact_reports(fixed, true, false));
        assert!(world.set_contact_reports(other, true, true));
        let target = Point3D::new(-0.625, 0.0, 0.0);
        assert!(world.set_kinematic_target(actor, target, zero));
        assert!(world.step());
        assert_eq!(world.position(actor), target);
        assert!(world.contacts(actor).is_empty());
        assert!(!world.report_static_kinematic_collisions());
        assert!(!world.report_kinematic_kinematic_collisions());

        assert!(world.set_kinematic_contact_reports(true, false));
        assert!(world.step());
        let fixed_contact = world.contacts(actor);
        assert!(!fixed_contact.is_empty());
        assert!(fixed_contact.iter().all(|point| {
            point.body == fixed
                && point.normal.x < -0.99
                && point.impulse == zero
                && point.normal_impulse == 0.0
        }));
        assert!(world.contacts(fixed).is_empty());
        assert!(world.contacts(other).is_empty());
        assert_eq!(world.position(actor), target);

        assert!(world.set_kinematic_contact_reports(false, true));
        assert!(world.step());
        let actor_points = world.contacts(actor);
        let other_points = world.contacts(other);
        assert!(!actor_points.is_empty());
        assert_eq!(actor_points.len(), other_points.len());
        for (a, b) in actor_points.iter().zip(&other_points) {
            assert_eq!(a.body, other);
            assert_eq!(b.body, actor);
            assert_eq!(a.position, b.position);
            assert!(a.normal.x > 0.99 && b.normal.x < -0.99);
            assert_eq!(a.normal_impulse, 0.0);
            assert_eq!(b.normal_impulse, 0.0);
        }
        assert!(world.set_kinematic_contact_reports(false, false));
        assert!(world.step());
        assert!(world.contacts(actor).is_empty() && world.contacts(other).is_empty());
    }

    #[test]
    fn non_dynamic_reports_preserve_contacts_from_every_substep() {
        let zero = Vector3D::default();
        let size = Vector3D::new(1.0, 1.0, 1.0);
        let world = PhysicsWorld3D::new(zero, 0.1);
        let wall = world.add_box(Point3D::default(), size, 0.0, 0.0, 0.0);
        let actor = world.add_kinematic_box(Point3D::new(-2.0, 0.0, 0.0), size, zero, 0.0, 0.0);
        assert!(world.set_kinematic_contact_reports(true, false));
        assert!(world.set_contact_reports(wall, true, false));
        assert!(world.set_contact_reports(actor, false, true));
        assert!(world.set_kinematic_target(actor, Point3D::new(-0.7, 0.0, 0.0), zero));
        assert!(world.step());
        let once = world.contacts(actor);
        assert!(!once.is_empty());
        assert_eq!(world.advance(0.2, 2), 2);
        let batched = world.contacts(actor);
        assert_eq!(batched.len(), once.len() * 2);
        assert!(batched.iter().all(|point| point.body == wall));

        assert!(world.set_kinematic_target(actor, Point3D::new(2.0, 0.0, 0.0), zero));
        assert!(world.step());
        assert!(world.contacts(actor).is_empty());
        assert!(world.clear());
        assert!(world.report_static_kinematic_collisions());
    }

    #[test]
    fn kinematic_report_candidates_respect_groups_joints_and_compound_gaps() {
        use crate::{PhysicsCollider3D, PhysicsShape3D, VecModel};
        let zero = Vector3D::default();
        let center = Point3D::default();
        let world = PhysicsWorld3D::new(zero, 0.02);
        let parts = VecModel::from(vec![-1.0_f64, 1.0]);
        let shapes = PhysicsCollider3D::prepare(&parts, |x| {
            PhysicsShape3D::new(
                "box",
                Point3D::new(*x, 0.0, 0.0),
                Vector3D::new(0.5, 1.0, 1.0),
                [0.0; 3],
            )
        });
        let mover = world.add_kinematic_body(center, zero, shapes, 0.0, 0.0);
        let wall = world.add_box(center, Vector3D::new(0.25, 1.0, 1.0), 0.0, 0.0, 0.0);
        assert!(world.set_kinematic_contact_reports(true, false));
        assert!(world.set_contact_reports(wall, true, false));
        assert!(world.set_contact_reports(mover, false, true));
        assert!(world.step());
        assert!(world.contacts(mover).is_empty());
        assert!(world.set_kinematic_target(mover, Point3D::new(-1.1, 0.0, 0.0), zero));
        assert!(world.step());
        let hit = world.contacts(mover);
        assert!(!hit.is_empty());
        assert!(hit.iter().all(|point| point.body == wall));

        assert!(world.set_collision_filter(wall, 1, 1));
        assert!(world.step());
        assert!(world.contacts(mover).is_empty());
        assert!(world.set_collision_filter(wall, 1, 0));
        assert!(world.set_kinematic(mover, false));
        let joint = world.add_fixed_joint(mover, wall, center);
        assert!(joint > 0);
        assert!(world.set_kinematic(mover, true));
        assert!(world.step());
        assert!(world.contacts(mover).is_empty());
        assert!(world.remove_joint(joint));
        assert!(world.step());
        assert!(!world.contacts(mover).is_empty());
    }

    #[test]
    fn reports_are_opt_in_and_the_two_receivers_get_opposite_impulses() {
        let world = PhysicsWorld3D::new(Vector3D::new(0.0, -9.81, 0.0), 0.016);
        let floor = world.add_box(
            Point3D::new(0.0, -0.1, 0.0),
            Vector3D::new(6.0, 0.2, 6.0),
            0.0,
            0.0,
            0.0,
        );
        let sphere = world.add_sphere(Point3D::new(0.0, 1.0, 0.0), 0.4, 1.0, 0.0, 0.0);
        assert!(!world.set_contact_reports(0, true, true));
        assert!(world.set_contact_reports(sphere, false, true));
        for _ in 0..30 {
            assert!(world.step());
            assert!(world.contacts(sphere).is_empty());
        }
        assert!(world.set_contact_reports(floor, true, true));
        assert!(world.set_contact_reports(sphere, true, true));
        assert!(world.step());
        let sphere_hits = world.contacts(sphere);
        let floor_hits = world.contacts(floor);
        assert!(!sphere_hits.is_empty());
        assert_eq!(sphere_hits.len(), floor_hits.len());
        for (received, opposite) in sphere_hits.iter().zip(&floor_hits) {
            assert_eq!(received.body, floor);
            assert_eq!(opposite.body, sphere);
            assert!(received.position.y.abs() < 0.02, "{received:?}");
            assert!(received.normal.y > 0.99);
            assert!(received.normal_impulse > 0.0);
            assert_eq!(received.position, opposite.position);
            assert_eq!(received.impulse.y, -opposite.impulse.y);
            assert_eq!(received.normal_impulse, opposite.normal_impulse);
        }
        assert!(world.set_contact_reports(floor, false, true));
        assert_eq!(world.contacts(sphere), sphere_hits);
        assert!(world.step());
        assert!(world.contacts(sphere).is_empty());
        assert!(!world.contacts(floor).is_empty());
        assert!(world.remove(sphere));
        assert!(world.contacts(floor).is_empty());
        assert!(world.clear());
        assert!(!world.set_contact_reports(floor, true, true));
    }
}
