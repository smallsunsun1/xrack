//! Tuning for the rigid-body solver and its character gravity defaults.

use rapier3d::prelude::{Collider, RigidBody};

use super::{PhysicsWorld3D, Simulation, contacts::as_vector, vector};
use crate::Vector3D;

const MAX_CCD_SUBSTEPS: i64 = 64;

impl PhysicsWorld3D {
    /// Set the native continuous-collision budget. Zero disables shape sweeps.
    pub fn set_ccd_substeps(&self, maximum: i64) -> bool {
        if !(0..=MAX_CCD_SUBSTEPS).contains(&maximum) {
            return false;
        }
        self.update_selectively(|simulation| {
            let settings = &mut simulation.world.integration_parameters;
            let maximum = maximum as usize;
            if settings.max_ccd_substeps != maximum {
                settings.max_ccd_substeps = maximum;
                simulation.settings_changed = true;
            }
            Some((true, false))
        })
        .is_some()
    }

    pub fn ccd_substeps(&self) -> i64 {
        self.read_settings(|simulation| {
            Some(simulation.world.integration_parameters.max_ccd_substeps as i64)
        })
    }

    /// Select full CCD for this body, including sweeps against dynamic targets.
    pub fn set_body_ccd(&self, body: i64, enabled: bool) -> bool {
        self.update_body_collision_settings(body, |rigid| {
            let changed = rigid.is_ccd_enabled() != enabled;
            rigid.enable_ccd(enabled);
            changed
        })
    }

    pub fn body_ccd(&self, body: i64) -> bool {
        self.read_body_collision_settings(body, RigidBody::is_ccd_enabled)
    }

    /// Set the speculative contact prediction distance in world units.
    pub fn set_soft_ccd(&self, body: i64, distance: f64) -> bool {
        let prediction = distance as f32;
        if !(distance >= 0.0 && prediction.is_finite()) {
            return false;
        }
        self.update_body_collision_settings(body, |rigid| {
            let changed = rigid.soft_ccd_prediction() != prediction;
            rigid.set_soft_ccd_prediction(prediction);
            changed
        })
    }

    pub fn soft_ccd(&self, body: i64) -> f64 {
        self.read_body_collision_settings(body, |body| f64::from(body.soft_ccd_prediction()))
    }

    /// Set the scale used by Rapier's geometric and velocity tolerances.
    pub fn set_length_unit(&self, length: f64) -> bool {
        let length = length as f32;
        if !(length > 0.0 && length.is_finite() && length.recip().is_finite()) {
            return false;
        }
        self.update_selectively(|simulation| {
            let world = &mut simulation.world;
            let settings = &mut world.integration_parameters;
            if settings.length_unit == length {
                return Some((true, false));
            }
            // Velocity limits are normalized by this length.
            if !(length * settings.normalized_max_linear_velocity).is_finite() {
                return None;
            }
            settings.length_unit = length;
            // Existing pairs need fresh prediction bounds under the new units.
            for (_, collider) in world.colliders.iter_mut() {
                mark_contact_bounds_dirty(collider);
            }
            world.wake_up_all(true);
            simulation.settings_changed = true;
            Some((true, true))
        })
        .is_some()
    }

    pub fn length_unit(&self) -> f64 {
        self.read_settings(|simulation| {
            Some(f64::from(
                simulation.world.integration_parameters.length_unit,
            ))
        })
    }

    pub fn set_gravity(&self, gravity: Vector3D) -> bool {
        let Some(gravity) = vector([gravity.x, gravity.y, gravity.z]) else {
            return false;
        };
        self.update_selectively(|simulation| {
            if simulation.world.gravity == gravity {
                return Some((true, false));
            }
            simulation.world.gravity = gravity;
            simulation.world.wake_up_all(true);
            for character in simulation
                .characters
                .values_mut()
                .filter(|character| character.follows_world_gravity)
            {
                character.update_gravity(gravity);
            }
            simulation.settings_changed = true;
            Some((true, true))
        })
        .is_some()
    }

    pub fn gravity(&self) -> Vector3D {
        self.read_settings(|simulation| Some(as_vector(simulation.world.gravity)))
    }

    /// Make a character follow the current world gravity and its future updates.
    pub fn use_character_world_gravity(&self, body: i64) -> bool {
        self.update_selectively(|simulation| {
            let gravity = simulation.world.gravity;
            let character = simulation.characters.get_mut(&body)?;
            if !character.follows_world_gravity {
                character.follows_world_gravity = true;
                character.update_gravity(gravity);
                simulation.settings_changed = true;
            }
            Some((true, false))
        })
        .is_some()
    }

    pub fn character_gravity(&self, body: i64) -> Vector3D {
        self.read_settings(|simulation| Some(as_vector(simulation.characters.get(&body)?.gravity)))
    }

    fn update_body_collision_settings(
        &self,
        body: i64,
        update: impl FnOnce(&mut RigidBody) -> bool,
    ) -> bool {
        self.update_selectively(|simulation| {
            let handle = simulation.mass_handle(body)?;
            let rigid = simulation.world.bodies.get_mut(handle)?;
            if update(rigid) {
                for handle in rigid.colliders() {
                    if let Some(collider) = simulation.world.colliders.get_mut(*handle) {
                        mark_contact_bounds_dirty(collider);
                    }
                }
                simulation.settings_changed = true;
            }
            Some((true, false))
        })
        .is_some()
    }

    fn read_body_collision_settings<T: Default>(
        &self,
        body: i64,
        read: impl FnOnce(&RigidBody) -> T,
    ) -> T {
        self.read_settings(|simulation| {
            let handle = simulation.mass_handle(body)?;
            simulation.world.bodies.get(handle).map(read)
        })
    }

    pub(super) fn read_settings<T: Default>(
        &self,
        read: impl FnOnce(&Simulation) -> Option<T>,
    ) -> T {
        let value = || {
            let state = self.0.as_ref()?;
            state.settings_changed.track();
            let simulation = state.state.try_borrow().ok()?;
            read(&simulation)
        };
        value().unwrap_or_default()
    }
}

fn mark_contact_bounds_dirty(collider: &mut Collider) {
    // Reapply the local pose to refresh Rapier's cached broad-phase and contact bounds.
    if let Some(&pose) = collider.position_wrt_parent() {
        collider.set_position_wrt_parent(pose);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Point3D;

    fn shoot(substeps: i64, full_ccd: bool, soft_ccd: f64, dynamic_target: bool) -> f64 {
        let zero = Vector3D::default();
        let world = PhysicsWorld3D::new(zero, 0.1);
        let target = world.add_box(
            Point3D::default(),
            Vector3D::new(0.08, 2.0, 2.0),
            if dynamic_target { 1.0 } else { 0.0 },
            0.0,
            0.0,
        );
        if dynamic_target {
            assert!(world.set_axis_locks(target, 7, 7));
            assert!(world.set_body_ccd(target, false));
        }
        let projectile = world.add_sphere(Point3D::new(-1.8, 0.0, 0.0), 0.12, 1.0, 0.0, 0.0);
        assert!(world.set_ccd_substeps(substeps));
        assert!(world.set_body_ccd(projectile, full_ccd));
        assert!(world.set_soft_ccd(projectile, soft_ccd));
        assert!(world.set_linear_velocity(projectile, Vector3D::new(40.0, 0.0, 0.0)));
        assert!(world.step());
        world.position(projectile).x
    }

    #[test]
    fn world_budget_and_body_mode_control_fast_sweeps() {
        assert!(shoot(0, true, 0.0, false) > 1.0);
        let fixed = shoot(4, false, 0.0, false);
        assert!(fixed < 0.0, "{fixed}");
        let without_dynamic = shoot(4, false, 0.0, true);
        let with_dynamic = shoot(4, true, 0.0, true);
        assert!(without_dynamic > 1.0, "{without_dynamic}");
        assert!(with_dynamic < 0.0, "{with_dynamic}");
    }

    #[test]
    fn speculative_prediction_uses_the_configured_horizon() {
        let closed = shoot(0, false, 0.0, false);
        let predicted = shoot(0, false, 4.0, false);
        assert!(closed > 1.0);
        assert!(predicted < 0.0, "{predicted}");
    }

    #[test]
    fn gravity_updates_wake_bodies_and_respect_character_overrides() {
        let zero = Vector3D::default();
        let world = PhysicsWorld3D::new(zero, 0.02);
        let follower = world.add_character(Point3D::new(-2.0, 3.0, 0.0), 0.2, 0.6);
        let independent = world.add_character(Point3D::new(2.0, 3.0, 0.0), 0.2, 0.6);
        let ball = world.add_sphere(Point3D::new(0.0, 3.0, 0.0), 0.2, 1.0, 0.0, 0.0);
        assert!(world.set_character_gravity(independent, zero));
        let bound = crate::Property::default();
        assert_eq!(bound.get(|| world.gravity()), zero);
        for _ in 0..220 {
            assert!(world.step());
        }
        assert!(world.is_sleeping(ball));
        assert!(!bound.is_dirty());
        let gravity = Vector3D::new(0.0, -10.0, 0.0);
        assert!(world.set_gravity(gravity));
        assert!(bound.is_dirty());
        assert!(!world.is_sleeping(ball));
        assert_eq!(world.character_gravity(follower), gravity);
        assert_eq!(world.character_gravity(independent), zero);
        for _ in 0..10 {
            assert!(world.step());
        }
        assert!(world.position(ball).y < 2.9);
        assert!(world.position(follower).y < 2.9);
        assert_eq!(world.position(independent).y, 3.0);
        assert!(world.use_character_world_gravity(independent));
        assert_eq!(world.character_gravity(independent), gravity);
        assert!(world.step());
        assert!(world.position(independent).y < 3.0);
    }

    #[test]
    fn unit_scale_and_collision_settings_survive_body_mode_changes_and_clear() {
        let zero = Vector3D::default();
        let world = PhysicsWorld3D::new(zero, 0.02);
        let body = world.add_sphere(Point3D::default(), 0.2, 1.0, 0.0, 0.0);
        assert_eq!(world.length_unit(), 1.0);
        assert!(world.set_length_unit(100.0));
        assert!(!world.set_length_unit(0.0));
        assert!(!world.set_length_unit(2e38));
        assert!(world.set_ccd_substeps(3));
        assert!(!world.set_ccd_substeps(MAX_CCD_SUBSTEPS + 1));
        assert!(world.set_kinematic(body, true));
        assert!(world.set_body_ccd(body, false));
        assert!(world.set_soft_ccd(body, 0.25));
        assert!(!world.set_soft_ccd(body, -1.0));
        assert!(world.set_kinematic(body, false));
        assert!(!world.body_ccd(body));
        assert_eq!(world.soft_ccd(body), 0.25);
        let state = world.0.as_ref().expect("simulation exists");
        assert!(
            (state
                .state
                .borrow()
                .world
                .integration_parameters
                .allowed_linear_error()
                - 0.5)
                .abs()
                < 1e-5
        );
        assert!(world.clear());
        assert_eq!(world.length_unit(), 100.0);
        assert_eq!(world.ccd_substeps(), 3);
    }
}
