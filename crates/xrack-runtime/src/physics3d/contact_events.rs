//! Observe solid contacts independently from per-point impulse reports.

use std::collections::BTreeMap;

use rapier3d::{
    parry::bounding_volume::BoundingVolume,
    prelude::{PhysicsWorld, QueryFilter, RigidBodyHandle},
};

use super::{PhysicsWorld3D, contacts::ContactReports};

#[derive(Default)]
pub(super) struct ContactEvents {
    receivers: BTreeMap<i64, BodyContacts>,
    changed: bool,
}

#[derive(Default)]
struct BodyContacts {
    touching: Vec<i64>,
    scratch: Vec<i64>,
    entered: Vec<i64>,
    exited: Vec<i64>,
    removed: Vec<i64>,
}

impl PhysicsWorld3D {
    /// Enable body-level contact transitions. Impulse reports have separate settings.
    pub fn set_contact_events(&self, body: i64, enabled: bool) -> bool {
        self.update_selectively(|simulation| {
            if !simulation.bodies.contains_key(&body)
                || simulation.triggers.states.contains_key(&body)
            {
                return None;
            }
            let events = &mut simulation.contact_events;
            if enabled {
                events.receivers.entry(body).or_default();
            } else {
                events.changed |= events.receivers.remove(&body).is_some();
            }
            Some((true, false))
        })
        .is_some()
    }

    /// Read the other solid bodies touching this body after the last completed substep.
    pub fn contact_bodies(&self, body: i64) -> Vec<i64> {
        self.read_contact_events(body, |state| &state.touching)
    }

    /// Read solid bodies that began touching during the latest nonempty step batch.
    pub fn contact_entered(&self, body: i64) -> Vec<i64> {
        self.read_contact_events(body, |state| &state.entered)
    }

    /// Read solid bodies that stopped touching during the latest nonempty step batch.
    pub fn contact_exited(&self, body: i64) -> Vec<i64> {
        self.read_contact_events(body, |state| &state.exited)
    }

    fn read_contact_events(
        &self,
        body: i64,
        select: impl FnOnce(&BodyContacts) -> &[i64],
    ) -> Vec<i64> {
        let read = || {
            let world = self.0.as_ref()?;
            world.contact_events_changed.track();
            let simulation = world.state.try_borrow().ok()?;
            let events = simulation.contact_events.receivers.get(&body)?;
            Some(select(events).to_vec())
        };
        read().unwrap_or_default()
    }
}

impl ContactEvents {
    pub(super) fn begin_batch(&mut self) {
        for state in self.receivers.values_mut() {
            self.changed |=
                !state.entered.is_empty() || !state.exited.is_empty() || !state.removed.is_empty();
            state.entered.clear();
            state.exited.clear();
            state.exited.append(&mut state.removed);
        }
    }

    pub(super) fn sample(
        &mut self,
        world: &PhysicsWorld,
        bodies: &BTreeMap<i64, RigidBodyHandle>,
        reports: &ContactReports,
    ) {
        if self.receivers.is_empty() {
            return;
        }
        let (static_kinematic, kinematic_kinematic) = reports.kinematic_modes();
        let dispatcher = world.narrow_phase.query_dispatcher();
        let geometry = (static_kinematic || kinematic_kinematic).then(|| {
            world.broad_phase.as_query_pipeline(
                dispatcher,
                &world.bodies,
                &world.colliders,
                QueryFilter::default().exclude_sensors(),
            )
        });
        for (&id, state) in &mut self.receivers {
            let Some((&rigid_handle, rigid)) = bodies
                .get(&id)
                .and_then(|handle| world.bodies.get(*handle).map(|rigid| (handle, rigid)))
            else {
                continue;
            };
            state.scratch.clear();
            for &handle in rigid.colliders() {
                let Some(collider) = world
                    .colliders
                    .get(handle)
                    .filter(|collider| collider.is_enabled())
                else {
                    continue;
                };
                for pair in world
                    .contact_pairs_with(handle)
                    .filter(|pair| pair.has_any_active_contact())
                {
                    let other = if pair.collider1 == handle {
                        pair.collider2
                    } else {
                        pair.collider1
                    };
                    let Some(other) = world
                        .colliders
                        .get(other)
                        .filter(|other| other.is_enabled() && !other.is_sensor())
                    else {
                        continue;
                    };
                    if rigid.is_dynamic()
                        || other
                            .parent()
                            .and_then(|parent| world.bodies.get(parent))
                            .is_some_and(|body| body.is_dynamic())
                    {
                        state.scratch.push(other.user_data as i64);
                    }
                }
                let Some(query) = geometry.as_ref().filter(|_| {
                    (static_kinematic && (rigid.is_fixed() || rigid.is_kinematic()))
                        || (kinematic_kinematic && rigid.is_kinematic())
                }) else {
                    continue;
                };
                let bounds = collider.compute_aabb();
                for leaf in query.bvh.leaves(|node| node.aabb().intersects(&bounds)) {
                    let Some((other, _)) = query.colliders.get_unknown_gen(leaf) else {
                        continue;
                    };
                    let other_id = other.user_data as i64;
                    if other_id == id
                        || !other.is_enabled()
                        || other.is_sensor()
                        || !collider.collision_groups().test(other.collision_groups())
                    {
                        continue;
                    }
                    let Some(other_handle) = other.parent() else {
                        continue;
                    };
                    let Some(other_body) = world.bodies.get(other_handle) else {
                        continue;
                    };
                    let allowed = if rigid.is_kinematic() {
                        (static_kinematic && other_body.is_fixed())
                            || (kinematic_kinematic && other_body.is_kinematic())
                    } else {
                        static_kinematic && other_body.is_kinematic()
                    };
                    if !allowed
                        || world
                            .impulse_joints
                            .joints_between(rigid_handle, other_handle)
                            .any(|(_, joint)| !joint.data.contacts_enabled())
                    {
                        continue;
                    }
                    let relative = collider.position().inv_mul(other.position());
                    if dispatcher
                        .intersection_test(&relative, collider.shape(), other.shape())
                        .is_ok_and(|result| result.intersecting)
                    {
                        state.scratch.push(other_id);
                    }
                }
            }
            state.scratch.sort_unstable();
            state.scratch.dedup();
            if state.touching != state.scratch {
                state.entered.extend(
                    state
                        .scratch
                        .iter()
                        .copied()
                        .filter(|body| state.touching.binary_search(body).is_err()),
                );
                state.exited.extend(
                    state
                        .touching
                        .iter()
                        .copied()
                        .filter(|body| state.scratch.binary_search(body).is_err()),
                );
                std::mem::swap(&mut state.touching, &mut state.scratch);
                self.changed = true;
            }
        }
    }

    pub(super) fn remove(&mut self, body: i64) {
        self.changed |= self.receivers.remove(&body).is_some();
        for state in self.receivers.values_mut() {
            if let Ok(index) = state.touching.binary_search(&body) {
                state.touching.remove(index);
                state.removed.push(body);
                self.changed = true;
            }
        }
    }

    pub(super) fn clear(&mut self) {
        self.changed |= !self.receivers.is_empty();
        self.receivers.clear();
    }

    pub(super) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Point3D, Vector3D};

    #[test]
    fn tracks_sleeping_contacts_without_force_reports_and_delivers_a_removal_once() {
        let zero = Vector3D::default();
        let world = PhysicsWorld3D::new(Vector3D::new(0.0, -9.81, 0.0), 0.02);
        let floor = world.add_box(
            Point3D::new(0.0, -0.25, 0.0),
            Vector3D::new(8.0, 0.5, 8.0),
            0.0,
            0.5,
            0.0,
        );
        let ball = world.add_sphere(Point3D::new(0.0, 0.9, 0.0), 0.25, 1.0, 0.5, 0.0);
        let sensor = world.add_box_trigger(Point3D::default(), Vector3D::new(3.0, 3.0, 3.0), zero);
        assert!(!world.set_contact_events(sensor, true));
        for _ in 0..240 {
            assert!(world.step());
        }
        assert!(world.is_sleeping(ball));
        assert!(world.set_contact_events(ball, true));
        assert!(world.step());
        assert_eq!(world.contact_bodies(ball), [floor]);
        assert_eq!(world.contact_entered(ball), [floor]);
        assert!(world.contacts(ball).is_empty());
        assert_eq!(world.advance(0.001, 8), 0);
        assert_eq!(world.contact_entered(ball), [floor]);
        assert!(world.step());
        assert_eq!(world.contact_bodies(ball), [floor]);
        assert!(world.contact_entered(ball).is_empty());
        assert!(world.remove(floor));
        assert!(world.contact_bodies(ball).is_empty());
        assert!(world.contact_exited(ball).is_empty());
        assert!(world.step());
        assert_eq!(world.contact_exited(ball), [floor]);
        assert!(world.step());
        assert!(world.contact_exited(ball).is_empty());
    }

    #[test]
    fn preserves_both_edges_of_one_bounce_in_a_step_batch() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.016);
        let wall = world.add_box(
            Point3D::default(),
            Vector3D::new(0.3, 2.0, 2.0),
            0.0,
            0.0,
            1.0,
        );
        let ball = world.add_sphere(Point3D::new(-1.0, 0.0, 0.0), 0.25, 1.0, 0.0, 1.0);
        assert!(world.set_contact_events(ball, true));
        assert!(world.set_linear_velocity(ball, Vector3D::new(6.0, 0.0, 0.0)));
        assert_eq!(world.advance(0.64, 40), 40);
        assert!(world.contact_bodies(ball).is_empty());
        assert_eq!(world.contact_entered(ball), [wall]);
        assert_eq!(world.contact_exited(ball), [wall]);
        assert!(world.set_contact_events(ball, false));
        assert!(world.contact_entered(ball).is_empty());
        assert!(world.contact_exited(ball).is_empty());
    }

    #[test]
    fn kinematic_geometry_respects_world_modes_and_groups_for_either_endpoint() {
        let zero = Vector3D::default();
        let world = PhysicsWorld3D::new(zero, 0.02);
        let size = Vector3D::new(1.0, 1.0, 1.0);
        let post = world.add_box(Point3D::default(), size, 0.0, 0.0, 0.0);
        let actor = world.add_kinematic_box(Point3D::new(-2.0, 0.0, 0.0), size, zero, 0.0, 0.0);
        assert!(world.set_contact_events(actor, true));
        assert!(world.set_contact_events(post, true));
        assert!(world.set_kinematic_target(actor, Point3D::new(-0.6, 0.0, 0.0), zero));
        assert!(world.step());
        assert!(world.contact_bodies(actor).is_empty());
        assert!(world.set_kinematic_contact_reports(true, false));
        assert!(world.step());
        assert_eq!(world.contact_entered(actor), [post]);
        assert_eq!(world.contact_entered(post), [actor]);
        assert!(world.set_collision_filter(post, 0, 1));
        assert!(world.step());
        assert_eq!(world.contact_exited(actor), [post]);
        assert_eq!(world.contact_exited(post), [actor]);
    }
}
