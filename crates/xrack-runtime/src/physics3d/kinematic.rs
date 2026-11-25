//! Switch between simulation-owned motion and authored pose targets.

use rapier3d::{
    parry::shape::Shape,
    prelude::{Pose, RigidBody, RigidBodyType, RigidBodyVelocity, Vector},
};

use super::{
    PhysicsWorld3D, Simulation, dynamics::Load, mass::valid_properties, scene_pose, vector,
};
use crate::{Point3D, Vector3D};

/// The pose at the start of a queued step and the velocity of the previous committed step.
pub(super) struct KinematicMotion {
    start: Option<Pose>,
    velocity: RigidBodyVelocity<f32>,
}

impl Default for KinematicMotion {
    fn default() -> Self {
        Self {
            start: None,
            velocity: RigidBodyVelocity::zero(),
        }
    }
}

impl PhysicsWorld3D {
    pub fn is_kinematic(&self, body: i64) -> bool {
        self.read(body, RigidBody::is_kinematic).unwrap_or_default()
    }

    /// Change which system controls the same body's motion.
    ///
    /// Releasing a body retains the velocity of its last committed step.
    pub fn set_kinematic(&self, body: i64, enabled: bool) -> bool {
        self.update(|simulation| {
            if simulation.characters.contains_key(&body) {
                return None;
            }
            let handle = *simulation.bodies.get(&body)?;
            let rigid = simulation.world.bodies.get(handle)?;
            if rigid.is_fixed() {
                return None;
            }
            if rigid.is_kinematic() == enabled {
                return Some(());
            }
            if !enabled
                && (!simulation.supports_dynamic_shapes(rigid)
                    || !valid_properties(&rigid.mass_properties().local_mprops))
            {
                return None;
            }

            let rigid = simulation.world.bodies.get_mut(handle)?;
            let pose = *rigid.position();
            if enabled {
                Load::default().install(rigid, false);
                rigid.set_linvel(Vector::ZERO, false);
                rigid.set_angvel(Vector::ZERO, false);
                rigid.set_body_type(RigidBodyType::KinematicPositionBased, true);
                rigid.set_next_kinematic_position(pose);
                simulation
                    .kinematic_motion
                    .insert(body, KinematicMotion::default());
            } else {
                let velocity = simulation
                    .kinematic_motion
                    .remove(&body)
                    .map_or_else(|| *rigid.vels(), |motion| motion.velocity);
                rigid.set_next_kinematic_position(pose);
                rigid.set_body_type(RigidBodyType::Dynamic, true);
                rigid.set_vels(velocity, false);
            }
            simulation.queued_loads.remove(&body);
            Some(())
        })
        .is_some()
    }

    /// Queue a pose for the next step. Replacing it again before stepping keeps the last target.
    pub fn set_kinematic_target(&self, body: i64, center: Point3D, rotation: Vector3D) -> bool {
        scene_pose(center, rotation).is_some_and(|pose| self.queue_kinematic_pose(body, pose))
    }

    /// Queue a pose whose rotation acts about a body-local pivot.
    pub fn set_kinematic_target_with_pivot(
        &self,
        body: i64,
        position: Point3D,
        rotation: Vector3D,
        pivot: Vector3D,
    ) -> bool {
        let (Some(mut pose), Some(pivot)) = (
            scene_pose(position, rotation),
            vector([pivot.x, pivot.y, pivot.z]),
        ) else {
            return false;
        };
        pose.translation += pivot - pose.rotation * pivot;
        pose.translation.is_finite() && self.queue_kinematic_pose(body, pose)
    }

    pub(super) fn body_velocity(&self, body: i64) -> Option<RigidBodyVelocity<f32>> {
        let state = self.0.as_ref()?;
        state.changed.track();
        let simulation = state.state.try_borrow().ok()?;
        let handle = *simulation.bodies.get(&body)?;
        let rigid = simulation.world.bodies.get(handle)?;
        Some(
            simulation
                .kinematic_motion
                .get(&body)
                .map_or_else(|| *rigid.vels(), |motion| motion.velocity),
        )
    }

    fn queue_kinematic_pose(&self, body: i64, pose: Pose) -> bool {
        // Position, rotation and velocity bindings observe the committed step.
        let queue = || {
            let mut simulation = self.0.as_ref()?.state.try_borrow_mut().ok()?;
            if simulation.characters.contains_key(&body) {
                return None;
            }
            let rigid = simulation.body_mut(body)?;
            if !rigid.is_kinematic() {
                return None;
            }
            let start = *rigid.position();
            rigid.set_next_kinematic_position(pose);
            simulation
                .kinematic_motion
                .entry(body)
                .or_default()
                .start
                .get_or_insert(start);
            Some(())
        };
        queue().is_some()
    }
}

impl Simulation {
    pub(super) fn finish_kinematic_step(&mut self) {
        if self.kinematic_motion.is_empty() {
            return;
        }
        let inverse_dt = self.world.integration_parameters.dt.recip();
        for (body, motion) in &mut self.kinematic_motion {
            motion.velocity = RigidBodyVelocity::zero();
            let Some(start) = motion.start.take() else {
                continue;
            };
            let Some(rigid) = self
                .bodies
                .get(body)
                .and_then(|handle| self.world.bodies.get(*handle))
            else {
                continue;
            };
            // Measure velocity from the committed poses, including the movement of
            // an offset center of mass during rotation.
            let delta = *rigid.rotation() * start.rotation.inverse();
            let shortest = if delta.w < 0.0 { -delta } else { delta };
            let angvel = shortest.to_scaled_axis() * inverse_dt;
            let linvel = (rigid.center_of_mass()
                - start.transform_point(rigid.local_center_of_mass()))
                * inverse_dt;
            if angvel.is_finite() && linvel.is_finite() {
                motion.velocity = RigidBodyVelocity { linvel, angvel };
            }
        }
    }

    pub(super) fn supports_dynamic_shapes(&self, rigid: &RigidBody) -> bool {
        !rigid.colliders().is_empty()
            && rigid.colliders().iter().all(|handle| {
                self.world.colliders.get(*handle).is_some_and(|collider| {
                    !collider.is_sensor() && convex_or_compound(collider.shape())
                })
            })
    }
}

fn convex_or_compound(shape: &dyn Shape) -> bool {
    (shape.is_convex() && shape.as_halfspace().is_none())
        || shape.as_compound().is_some_and(|compound| {
            compound
                .shapes()
                .iter()
                .all(|(_, part)| convex_or_compound(part.as_ref()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PhysicsCollider3D, PhysicsShape3D, VecModel};

    #[test]
    fn take_and_release_preserves_identity_and_committed_pivot_motion() {
        let zero = Vector3D::default();
        let origin = Point3D::default();
        let world = PhysicsWorld3D::new(zero, 0.1);
        let trigger = world.add_box_trigger(origin, Vector3D::new(12.0, 12.0, 12.0), zero);
        let body = world.add_box(origin, Vector3D::new(1.0, 1.0, 1.0), 2.0, 0.5, 0.0);
        assert!(world.set_mass(body, 3.0));
        assert!(world.step());
        assert_eq!(world.trigger_entered(trigger), [body]);

        assert!(world.set_linear_velocity(body, Vector3D::new(1.0, 0.0, 0.0)));
        assert!(world.set_angular_velocity(body, Vector3D::new(0.0, 1.0, 0.0)));
        assert!(world.apply_force(body, Vector3D::new(30.0, 0.0, 0.0)));
        assert!(world.set_continuous_load(body, Vector3D::new(20.0, 0.0, 0.0), zero));
        assert!(world.set_kinematic(body, true));
        assert!(world.is_kinematic(body));
        assert_eq!(world.mass(body), 3.0);
        assert_eq!(world.position(body), origin);
        assert_eq!(world.velocity(body), zero);
        assert_eq!(world.angular_velocity(body), zero);
        assert!(!world.apply_impulse(body, Vector3D::new(1.0, 0.0, 0.0)));
        assert!(world.set_kinematic_target_with_pivot(
            body,
            origin,
            Vector3D::new(90.0, 0.0, 0.0),
            Vector3D::new(1.0, 0.0, 0.0),
        ));
        assert_eq!(world.position(body), origin);
        assert!(world.step());
        let current = world.position(body);
        assert!(
            (current.x - 1.0).abs() < 1e-5 && (current.z - 1.0).abs() < 1e-5,
            "{current:?}"
        );
        let speed = world.velocity(body);
        let angular = world.angular_velocity(body);
        assert!(speed.x > 9.9, "{speed:?}");
        assert!(
            (angular.y - std::f64::consts::FRAC_PI_2 / 0.1).abs() < 1e-5,
            "{angular:?}"
        );
        assert_eq!(world.trigger_count(trigger), 1);
        assert!(
            world.trigger_entered(trigger).is_empty() && world.trigger_exited(trigger).is_empty()
        );

        assert!(world.set_kinematic_target(body, Point3D::new(-4.0, 0.0, 0.0), zero));
        assert!(world.set_kinematic(body, false));
        assert!(!world.is_kinematic(body));
        assert_eq!(world.position(body), current);
        assert_eq!(world.velocity(body), speed);
        assert_eq!(world.angular_velocity(body), angular);
        assert_eq!(world.mass(body), 3.0);
        assert_eq!(world.dynamic_friction(body), 0.5);
        assert!(world.step());
        assert!((world.position(body).x - current.x - speed.x * 0.1).abs() < 0.005);
        assert!((world.velocity(body).x - speed.x).abs() < 1e-5);
        assert!(
            world.trigger_entered(trigger).is_empty() && world.trigger_exited(trigger).is_empty()
        );
    }

    #[test]
    fn convertible_kinematic_bodies_keep_density_membership_and_joint_handles() {
        let zero = Vector3D::default();
        let center = Point3D::default();
        let world = PhysicsWorld3D::new(zero, 0.02);
        let size = Vector3D::new(1.0, 1.0, 1.0);
        let platform = world.add_kinematic_box(center, size, zero, 0.4, 0.0);
        let fixed = world.add_box(Point3D::new(2.0, 0.0, 0.0), size, 0.0, 0.4, 0.0);
        assert!(world.use_default_density(platform));
        assert!(world.set_kinematic(platform, false));
        let joint = world.add_spherical_joint(platform, fixed, Point3D::new(1.0, 0.0, 0.0));
        assert!(joint > 0);
        assert!(world.set_kinematic(platform, true));
        assert!(world.set_default_density(2.0));
        assert_eq!(world.mass(platform), 2.0);
        assert!(world.step());
        assert!(world.set_kinematic(platform, false));
        assert_eq!(world.mass(platform), 2.0);
        assert!(world.remove_joint(joint));
        assert!(world.apply_impulse(platform, Vector3D::new(2.0, 0.0, 0.0)));
        assert_eq!(world.velocity(platform), Vector3D::new(1.0, 0.0, 0.0));

        let surface = PhysicsCollider3D::prepare(&VecModel::from(vec![0]), |_| {
            PhysicsShape3D::new_with_mesh("mesh", center, size, [0.0; 3], "primitive:box", |_| None)
        });
        let mesh = world.add_kinematic_body(center, zero, surface, 0.0, 0.0);
        assert!(mesh > 0);
        assert!(!world.set_kinematic(mesh, false));
        assert!(world.is_kinematic(mesh));
        assert!(!world.set_mass(mesh, 3.0));
        assert!(!world.set_kinematic(fixed, true));
        let character = world.add_character(center, 0.25, 0.5);
        assert!(character > 0 && !world.set_kinematic(character, false));
    }

    #[test]
    fn zero_principal_inertia_remains_infinite_across_release() {
        let zero = Vector3D::default();
        let center = Point3D::default();
        let world = PhysicsWorld3D::new(zero, 0.02);
        let body = world.add_sphere(center, 0.5, 1.0, 0.0, 0.0);
        assert!(world.set_kinematic(body, true));
        assert!(world.set_mass_properties(body, 1.0, center, Vector3D::new(0.0, 2.0, 3.0), zero));
        assert!(world.set_kinematic(body, false));
        assert_eq!(world.principal_inertia(body), Vector3D::new(0.0, 2.0, 3.0));
        assert!(world.apply_torque_impulse(body, Vector3D::new(2.0, 2.0, 0.0)));
        assert_eq!(world.angular_velocity(body), Vector3D::new(0.0, 1.0, 0.0));
    }
}
