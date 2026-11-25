//! Dynamic body controls shared by the interpreter and AOT runtime.

use rapier3d::prelude::{LockedAxes, RigidBody, Vector};

use super::{PhysicsWorld3D, Simulation, scene_pose, vector};
use crate::{Point3D, Vector3D};

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct Load {
    force: Vector,
    torque: Vector,
}

impl Load {
    fn continuous(body: &RigidBody) -> Self {
        Self {
            force: body.user_force(),
            torque: body.user_torque(),
        }
    }

    fn combined(self, other: Self) -> Option<Self> {
        let force = self.force + other.force;
        let torque = self.torque + other.torque;
        (force.is_finite() && torque.is_finite()).then_some(Self { force, torque })
    }

    pub(super) fn install(self, body: &mut RigidBody, wake: bool) {
        body.reset_forces(false);
        body.reset_torques(false);
        body.add_force(self.force, false);
        body.add_torque(self.torque, false);
        if wake {
            body.wake_up(true);
        }
    }
}

impl PhysicsWorld3D {
    /// Apply a force at the center of mass for the next simulation step.
    pub fn apply_force(&self, body: i64, force: Vector3D) -> bool {
        let Some(force) = to_vector(force) else {
            return false;
        };
        self.queue_load(body, |_| Load {
            force,
            torque: Vector::ZERO,
        })
    }

    /// Apply a force at a world-space point for the next simulation step.
    pub fn apply_force_at(&self, body: i64, force: Vector3D, point: Point3D) -> bool {
        let (Some(force), Some(point)) = (to_vector(force), vector([point.x, point.y, point.z]))
        else {
            return false;
        };
        self.queue_load(body, |body| Load {
            force,
            torque: (point - body.center_of_mass()).cross(force),
        })
    }

    pub fn apply_torque(&self, body: i64, torque: Vector3D) -> bool {
        let Some(torque) = to_vector(torque) else {
            return false;
        };
        self.queue_load(body, |_| Load {
            force: Vector::ZERO,
            torque,
        })
    }

    /// Replace the world-space force and torque used at every subsequent step.
    pub fn set_continuous_load(&self, body: i64, force: Vector3D, torque: Vector3D) -> bool {
        let (Some(force), Some(torque)) = (to_vector(force), to_vector(torque)) else {
            return false;
        };
        self.update(|simulation| {
            let load = Load { force, torque };
            load.combined(
                simulation
                    .queued_loads
                    .get(&body)
                    .copied()
                    .unwrap_or_default(),
            )?;
            let body = simulation.dynamic_body_mut(body)?;
            if load != Load::continuous(body) {
                load.install(body, true);
            }
            Some(())
        })
        .is_some()
    }

    /// Clear both the queued and continuous forces and torques.
    pub fn clear_forces(&self, body: i64) -> bool {
        self.update(|simulation| {
            let rigid = simulation.dynamic_body_mut(body)?;
            if Load::continuous(rigid) != Load::default() {
                Load::default().install(rigid, true);
            }
            simulation.queued_loads.remove(&body);
            Some(())
        })
        .is_some()
    }

    pub fn apply_impulse(&self, body: i64, impulse: Vector3D) -> bool {
        let Some(impulse) = to_vector(impulse) else {
            return false;
        };
        self.update_dynamic(body, |body| body.apply_impulse(impulse, true))
    }

    /// Apply an impulse at a world-space point, changing linear and angular momentum.
    pub fn apply_impulse_at(&self, body: i64, impulse: Vector3D, point: Point3D) -> bool {
        let (Some(impulse), Some(point)) =
            (to_vector(impulse), vector([point.x, point.y, point.z]))
        else {
            return false;
        };
        self.update(|simulation| {
            let body = simulation.dynamic_body_mut(body)?;
            if !(point - body.center_of_mass()).cross(impulse).is_finite() {
                return None;
            }
            body.apply_impulse_at_point(impulse, point, true);
            Some(())
        })
        .is_some()
    }

    pub fn apply_torque_impulse(&self, body: i64, impulse: Vector3D) -> bool {
        let Some(impulse) = to_vector(impulse) else {
            return false;
        };
        self.update_dynamic(body, |body| body.apply_torque_impulse(impulse, true))
    }

    pub fn set_linear_velocity(&self, body: i64, velocity: Vector3D) -> bool {
        let Some(velocity) = to_vector(velocity) else {
            return false;
        };
        self.update_dynamic(body, |body| body.set_linvel(velocity, true))
    }

    /// Set the world XYZ angular velocity in radians per second.
    pub fn set_angular_velocity(&self, body: i64, velocity: Vector3D) -> bool {
        let Some(velocity) = to_vector(velocity) else {
            return false;
        };
        self.update_dynamic(body, |body| body.set_angvel(velocity, true))
    }

    pub fn angular_velocity(&self, body: i64) -> Vector3D {
        self.body_velocity(body)
            .map(|velocity| super::contacts::as_vector(velocity.angvel))
            .unwrap_or_default()
    }

    /// Move a dynamic body directly to a pose and reset its velocities and loads.
    pub fn reset_body(&self, body: i64, center: Point3D, rotation: Vector3D) -> bool {
        let Some(pose) = scene_pose(center, rotation) else {
            return false;
        };
        self.update(|simulation| {
            let rigid = simulation.dynamic_body_mut(body)?;
            rigid.set_position(pose, false);
            rigid.set_linvel(Vector::ZERO, false);
            rigid.set_angvel(Vector::ZERO, false);
            Load::default().install(rigid, true);
            simulation.queued_loads.remove(&body);
            simulation.snap_pose_history(body, pose);
            simulation.invalidate_query_bvh();
            Some(())
        })
        .is_some()
    }

    pub fn set_gravity_scale(&self, body: i64, scale: f64) -> bool {
        let scale = scale as f32;
        scale.is_finite() && self.update_dynamic(body, |body| body.set_gravity_scale(scale, true))
    }

    pub fn set_damping(&self, body: i64, linear: f64, angular: f64) -> bool {
        let (linear, angular) = (linear as f32, angular as f32);
        if !(linear.is_finite() && angular.is_finite() && linear >= 0.0 && angular >= 0.0) {
            return false;
        }
        self.update_dynamic(body, |body| {
            body.set_linear_damping(linear);
            body.set_angular_damping(angular);
        })
    }

    /// Lock translation and rotation about world axes. Each mask uses X=1, Y=2, Z=4.
    pub fn set_axis_locks(&self, body: i64, linear: i64, angular: i64) -> bool {
        if !(0..=7).contains(&linear) || !(0..=7).contains(&angular) {
            return false;
        }
        let mut locks = LockedAxes::empty();
        for (index, axis) in [
            LockedAxes::TRANSLATION_LOCKED_X,
            LockedAxes::TRANSLATION_LOCKED_Y,
            LockedAxes::TRANSLATION_LOCKED_Z,
            LockedAxes::ROTATION_LOCKED_X,
            LockedAxes::ROTATION_LOCKED_Y,
            LockedAxes::ROTATION_LOCKED_Z,
        ]
        .into_iter()
        .enumerate()
        {
            locks.set(axis, (linear | (angular << 3)) & (1 << index) != 0);
        }
        self.update_dynamic(body, |body| body.set_locked_axes(locks, true))
    }

    pub fn is_sleeping(&self, body: i64) -> bool {
        self.read(body, |body| body.is_dynamic() && body.is_sleeping())
            .unwrap_or_default()
    }

    fn update_dynamic(&self, body: i64, update: impl FnOnce(&mut RigidBody)) -> bool {
        self.update(|simulation| {
            update(simulation.dynamic_body_mut(body)?);
            Some(())
        })
        .is_some()
    }

    fn queue_load(&self, id: i64, make_load: impl FnOnce(&RigidBody) -> Load) -> bool {
        let queue = || {
            let mut simulation = self.0.as_ref()?.state.try_borrow_mut().ok()?;
            let body = simulation.world.bodies.get(*simulation.bodies.get(&id)?)?;
            if !body.is_dynamic() {
                return None;
            }
            let load = make_load(body);
            let queued = simulation
                .queued_loads
                .get(&id)
                .copied()
                .unwrap_or_default()
                .combined(load)?;
            queued.combined(Load::continuous(body))?;
            if queued == Load::default() {
                simulation.queued_loads.remove(&id);
            } else {
                simulation.queued_loads.insert(id, queued);
            }
            Some(())
        };
        queue().is_some()
    }
}

impl Simulation {
    fn dynamic_body_mut(&mut self, id: i64) -> Option<&mut RigidBody> {
        self.body_mut(id).filter(|body| body.is_dynamic())
    }

    pub(super) fn apply_queued_loads(&mut self) {
        for (id, queued) in &mut self.queued_loads {
            if let Some(body) = self
                .bodies
                .get(id)
                .and_then(|handle| self.world.bodies.get_mut(*handle))
            {
                let continuous = Load::continuous(body);
                body.add_force(queued.force, false);
                body.add_torque(queued.torque, false);
                body.wake_up(true);
                // Reuse the entry to store what must be restored after solving.
                *queued = continuous;
            }
        }
    }

    pub(super) fn restore_continuous_loads(&mut self) {
        for (id, continuous) in &self.queued_loads {
            if let Some(body) = self
                .bodies
                .get(id)
                .and_then(|handle| self.world.bodies.get_mut(*handle))
            {
                continuous.install(body, false);
            }
        }
        self.queued_loads.clear();
    }
}

fn to_vector(value: Vector3D) -> Option<Vector> {
    vector([value.x, value.y, value.z])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_loads_add_to_continuous_loads_and_expire_after_one_step() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.05);
        let zero = Vector3D::default();
        let body = world.add_sphere(Point3D::default(), 0.4, 2.0, 0.0, 0.0);
        assert!(world.set_continuous_load(body, Vector3D::new(4.0, 0.0, 0.0), zero));
        assert!(world.apply_force(body, Vector3D::new(6.0, 0.0, 0.0)));
        assert!(world.apply_force(body, Vector3D::new(2.0, 0.0, 0.0)));
        assert!(world.apply_torque(body, Vector3D::new(0.0, 0.0, 1.0)));
        assert_eq!(world.velocity(body), zero);
        assert_eq!(world.angular_velocity(body), zero);
        assert!(world.step());
        assert!((world.velocity(body).x - 0.3).abs() < 1e-5);
        let angular = world.angular_velocity(body).z;
        assert!(angular > 0.2);
        assert!(world.step());
        assert!((world.velocity(body).x - 0.4).abs() < 1e-5);
        assert!((world.angular_velocity(body).z - angular).abs() < 1e-5);
        assert!(world.apply_force(body, Vector3D::new(6.0, 0.0, 0.0)));
        assert!(world.clear_forces(body));
        assert!(world.step());
        assert!((world.velocity(body).x - 0.4).abs() < 1e-5);
    }

    #[test]
    fn off_center_loads_spin_the_body_and_reset_clears_all_motion() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.05);
        let zero = Vector3D::default();
        let center = Point3D::default();
        let body = world.add_box(center, Vector3D::new(1.0, 1.0, 1.0), 2.0, 0.0, 0.0);
        let fixed = world.add_sphere(Point3D::new(4.0, 0.0, 0.0), 1.0, 0.0, 0.0, 0.0);
        let push = Vector3D::new(4.0, 0.0, 0.0);
        assert!(!world.apply_force(fixed, push));
        assert!(!world.reset_body(fixed, center, zero));
        assert!(!world.apply_torque(body, Vector3D::new(f64::NAN, 0.0, 0.0)));
        assert!(world.apply_impulse_at(body, push, Point3D::new(0.0, 0.5, 0.0)));
        assert!((world.velocity(body).x - 2.0).abs() < 1e-6);
        assert!(world.angular_velocity(body).z < -1.0);
        assert!(world.apply_torque_impulse(body, Vector3D::new(0.0, 0.0, 1.0)));
        let reset_to = Point3D::new(1.0, 2.0, 3.0);
        assert!(world.set_continuous_load(body, push, push));
        assert!(world.apply_force_at(body, push, Point3D::new(0.0, 0.5, 0.0)));
        assert!(world.reset_body(body, reset_to, Vector3D::new(0.0, 0.0, 30.0)));
        assert_eq!(world.position(body), reset_to);
        assert_eq!(world.velocity(body), zero);
        assert_eq!(world.angular_velocity(body), zero);
        assert!(world.step());
        assert_eq!(world.position(body), reset_to);
        assert!((world.rotation(body).z - 30.0).abs() < 1e-4);
        assert!(world.apply_force_at(body, push, Point3D::new(1.0, 2.5, 3.0)));
        assert!(world.step());
        assert!(world.angular_velocity(body).z < -0.1);
    }

    #[test]
    fn gravity_damping_and_axis_locks_are_per_body() {
        let world = PhysicsWorld3D::new(Vector3D::new(0.0, -10.0, 0.0), 0.05);
        let origin = Point3D::default();
        let zero = Vector3D::default();
        let body = world.add_sphere(origin, 0.5, 1.0, 0.0, 0.0);
        assert!(!world.set_axis_locks(body, 8, 0));
        assert!(!world.set_damping(body, -1.0, 0.0));
        assert!(world.set_gravity_scale(body, -0.5));
        assert!(world.step());
        assert!((world.velocity(body).y - 0.25).abs() < 1e-5);
        assert!(world.reset_body(body, origin, zero));
        assert!(world.set_gravity_scale(body, 0.0));
        assert!(world.set_damping(body, 10.0, 10.0));
        assert!(world.set_axis_locks(body, 2, 5));
        assert!(world.apply_impulse(body, Vector3D::new(5.0, 5.0, 0.0)));
        assert!(world.apply_torque_impulse(body, Vector3D::new(1.0, 1.0, 1.0)));
        assert!(world.step());
        let velocity = world.velocity(body);
        let angular = world.angular_velocity(body);
        assert!(
            velocity.x > 0.0 && velocity.x < 5.0 && velocity.y.abs() < 1e-6,
            "{velocity:?}"
        );
        assert!(
            angular.y > 0.0 && angular.y < 10.0 && angular.x.abs() < 1e-6 && angular.z.abs() < 1e-6,
            "{angular:?}"
        );
        assert!(world.set_axis_locks(body, 0, 0));
        assert!(world.reset_body(body, origin, zero));
        assert!(world.set_damping(body, 0.0, 0.0));
        assert!(world.set_linear_velocity(body, Vector3D::new(2.0, 0.0, 0.0)));
        assert!(world.set_angular_velocity(body, Vector3D::new(0.0, 0.0, std::f64::consts::PI)));
        assert!(world.step());
        assert!((world.position(body).x - 0.1).abs() < 1e-6);
        assert!((world.rotation(body).z - 9.0).abs() < 0.1);
    }

    #[test]
    fn queued_force_wakes_a_sleeping_body_when_stepped() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.016);
        let body = world.add_sphere(Point3D::default(), 0.5, 1.0, 0.0, 0.0);
        for _ in 0..240 {
            assert!(world.step());
        }
        assert!(world.is_sleeping(body));
        assert!(world.apply_force(body, Vector3D::new(8.0, 0.0, 0.0)));
        assert!(world.is_sleeping(body));
        assert!(world.step());
        assert!(!world.is_sleeping(body));
        assert!(world.velocity(body).x > 0.1);
    }
}
