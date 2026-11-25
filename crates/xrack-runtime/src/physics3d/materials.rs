//! Collider materials and the opt-in contact hook for separate friction coefficients.

use std::collections::BTreeMap;

use rapier3d::prelude::{
    ActiveHooks, CoefficientCombineRule, ContactModificationContext, PairFilterContext,
    PhysicsHooks, Vector,
};

use super::{PhysicsWorld3D, Simulation};

pub(super) struct WorldHooks<'a> {
    pub(super) static_friction: &'a BTreeMap<i64, f32>,
}

// Reacquire static friction at low sliding speeds, with hysteresis to avoid
// changing coefficients repeatedly as a contact comes to rest.
const STICK_SPEED: f32 = 0.01;
const SLIP_SPEED: f32 = 0.03;
const SLIDING: u32 = 1;

#[derive(Clone, Copy, Default, PartialEq)]
struct Material {
    static_friction: f32,
    dynamic_friction: f32,
    restitution: f32,
}

impl PhysicsWorld3D {
    /// Apply the same material to all colliders on a solid body.
    pub fn set_material(
        &self,
        body: i64,
        static_friction: f64,
        dynamic_friction: f64,
        restitution: f64,
    ) -> bool {
        let material = Material {
            static_friction: static_friction as f32,
            dynamic_friction: dynamic_friction as f32,
            restitution: restitution as f32,
        };
        if !(static_friction >= 0.0
            && dynamic_friction >= 0.0
            && material.static_friction.is_finite()
            && material.dynamic_friction.is_finite()
            && (0.0..=1.0).contains(&restitution))
        {
            return false;
        }
        self.update(|simulation| {
            if simulation.material(body)? != material {
                simulation.apply_material(body, material)?;
            }
            Some(())
        })
        .is_some()
    }

    pub fn static_friction(&self, body: i64) -> f64 {
        f64::from(self.read_material(body).static_friction)
    }

    pub fn dynamic_friction(&self, body: i64) -> f64 {
        f64::from(self.read_material(body).dynamic_friction)
    }

    pub fn restitution(&self, body: i64) -> f64 {
        f64::from(self.read_material(body).restitution)
    }

    fn read_material(&self, body: i64) -> Material {
        let Some(state) = &self.0 else {
            return Material::default();
        };
        state.materials_changed.track();
        state
            .state
            .try_borrow()
            .ok()
            .and_then(|simulation| simulation.material(body))
            .unwrap_or_default()
    }
}

impl Simulation {
    fn material(&self, body: i64) -> Option<Material> {
        let handle = *self.bodies.get(&body)?;
        let rigid = self.world.bodies.get(handle)?;
        let collider = self.world.colliders.get(*rigid.colliders().first()?)?;
        if collider.is_sensor() {
            return None;
        }
        Some(Material {
            static_friction: self
                .static_friction
                .get(&body)
                .copied()
                .unwrap_or_else(|| collider.friction()),
            dynamic_friction: collider.friction(),
            restitution: collider.restitution(),
        })
    }

    fn apply_material(&mut self, body: i64, material: Material) -> Option<()> {
        let handle = *self.bodies.get(&body)?;
        let world = &mut self.world;
        let rigid = world.bodies.get(handle)?;
        let separate_friction = material.static_friction != material.dynamic_friction;
        let mut touching = Vec::new();
        for &handle in rigid.colliders() {
            let Some(collider) = world.colliders.get_mut(handle) else {
                continue;
            };
            collider.set_friction(material.dynamic_friction);
            collider.set_restitution(material.restitution);
            let mut hooks = collider.active_hooks();
            hooks.set(ActiveHooks::MODIFY_SOLVER_CONTACTS, separate_friction);
            collider.set_active_hooks(hooks);

            // Rapier's material setters don't mark the contact cache dirty.
            // Reapply the collider's local pose to refresh only its pairs.
            if let Some(&local) = collider.position_wrt_parent() {
                collider.set_position_wrt_parent(local);
            }
            touching.extend(
                world
                    .narrow_phase
                    .contact_pairs_with(handle)
                    .filter(|pair| pair.has_any_active_contact())
                    .filter_map(|pair| {
                        let other = if pair.collider1 == handle {
                            pair.collider2
                        } else {
                            pair.collider1
                        };
                        world.colliders.get(other)?.parent()
                    }),
            );
        }
        if separate_friction {
            self.static_friction.insert(body, material.static_friction);
        } else {
            self.static_friction.remove(&body);
        }
        world.wake_up(handle, true);
        for handle in touching {
            world.wake_up(handle, true);
        }
        self.materials_changed = true;
        Some(())
    }
}

impl PhysicsHooks for WorldHooks<'_> {
    fn filter_intersection_pair(&self, context: &PairFilterContext<'_>) -> bool {
        let first = &context.colliders[context.collider1];
        let second = &context.colliders[context.collider2];
        !(first.is_sensor() && second.is_sensor())
    }

    fn modify_solver_contacts(&self, context: &mut ContactModificationContext<'_>) {
        let body1 = context
            .rigid_body1
            .and_then(|body| context.bodies.get(body));
        let body2 = context
            .rigid_body2
            .and_then(|body| context.bodies.get(body));
        let [first, second] =
            [context.collider1, context.collider2].map(|id| &context.colliders[id]);
        let Some(manifold) = context.rigid_mut() else {
            return;
        };
        let limit = if *manifold.user_data & SLIDING == 0 {
            SLIP_SPEED
        } else {
            STICK_SPEED
        };
        let sliding = manifold.solver_contacts.iter().any(|contact| {
            let velocity = body2
                .map_or(Vector::ZERO, |body| body.velocity_at_point(contact.anchor2))
                - body1.map_or(Vector::ZERO, |body| body.velocity_at_point(contact.anchor1));
            let normal = *manifold.normal;
            let tangent = velocity - normal * velocity.dot(normal);
            tangent.length_squared() > limit * limit
        });
        if sliding {
            *manifold.user_data |= SLIDING;
        } else {
            *manifold.user_data &= !SLIDING;
            let static_coefficient = |collider: &rapier3d::prelude::Collider| {
                self.static_friction
                    .get(&(collider.user_data as i64))
                    .copied()
                    .unwrap_or_else(|| collider.friction())
            };
            *manifold.friction = CoefficientCombineRule::combine(
                static_coefficient(first),
                static_coefficient(second),
                first.friction_combine_rule(),
                second.friction_combine_rule(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::Point3D;

    fn slider() -> (PhysicsWorld3D, i64, i64) {
        let world = PhysicsWorld3D::new(crate::Vector3D::new(0.0, -10.0, 0.0), 0.01);
        let floor = world.add_box(
            Point3D::new(0.0, -0.25, 0.0),
            crate::Vector3D::new(20.0, 0.5, 8.0),
            0.0,
            0.0,
            0.0,
        );
        let block = world.add_box(
            Point3D::new(0.0, 0.25, 0.0),
            crate::Vector3D::new(1.0, 0.5, 1.0),
            1.0,
            0.0,
            0.0,
        );
        assert!(floor > 0 && block > 0);
        assert!(world.set_axis_locks(block, 0, 7));
        (world, floor, block)
    }

    #[test]
    fn static_friction_holds_then_switches_to_sliding_friction() {
        let (world, floor, block) = slider();
        assert!(world.set_material(floor, 1.2, 0.12, 0.0));
        assert!(world.set_continuous_load(
            block,
            crate::Vector3D::new(3.0, 0.0, 0.0),
            crate::Vector3D::default()
        ));
        for _ in 0..40 {
            assert!(world.step());
        }
        assert!(
            world.position(block).x.abs() < 0.002,
            "{:?}",
            world.position(block)
        );
        assert!(world.apply_impulse(block, crate::Vector3D::new(0.3, 0.0, 0.0)));
        for _ in 0..30 {
            assert!(world.step());
        }
        assert!(world.velocity(block).x > 0.9, "{:?}", world.velocity(block));

        assert!(world.set_material(floor, 1.2, 1.2, 0.0));
        for _ in 0..45 {
            assert!(world.step());
        }
        assert!(
            world.velocity(block).x.abs() < 0.02,
            "{:?}",
            world.velocity(block)
        );
        let state = world.0.as_ref().expect("simulation exists");
        let simulation = state.state.borrow();
        assert!(simulation.static_friction.is_empty());
    }

    #[test]
    fn edits_wake_contacting_bodies_and_only_invalidate_material_reads_on_changes() {
        let (world, floor, block) = slider();
        let coefficient = crate::Property::default();
        assert_eq!(coefficient.get(|| world.static_friction(floor)), 0.0);
        for _ in 0..350 {
            assert!(world.step());
        }
        assert!(world.is_sleeping(block));
        assert!(!coefficient.is_dirty());
        assert!(!world.set_material(floor, f64::INFINITY, 0.0, 0.0));
        assert!(!world.set_material(floor, 0.0, 0.0, 1.5));
        assert!(!coefficient.is_dirty());
        assert!(world.set_material(floor, 0.75, 0.25, 0.5));
        assert!(coefficient.is_dirty());
        assert_eq!(coefficient.get(|| world.static_friction(floor)), 0.75);
        assert_eq!(world.dynamic_friction(floor), 0.25);
        assert_eq!(world.restitution(floor), 0.5);
        assert!(!world.is_sleeping(block));
        assert!(world.set_material(floor, 0.75, 0.25, 0.5));
        assert!(!coefficient.is_dirty());
        assert!(world.remove(floor));
        assert_eq!(coefficient.get(|| world.static_friction(floor)), 0.0);
        let state = world.0.as_ref().expect("simulation exists");
        assert!(state.state.borrow().static_friction.is_empty());
    }

    #[test]
    fn updated_restitution_is_used_for_shared_contacts() {
        let world = PhysicsWorld3D::new(crate::Vector3D::default(), 0.01);
        let wall = world.add_box(
            Point3D::default(),
            crate::Vector3D::new(0.5, 4.0, 4.0),
            0.0,
            0.0,
            0.0,
        );
        let start = Point3D::new(-1.0, 0.0, 0.0);
        let ball = world.add_sphere(start, 0.25, 1.0, 0.0, 0.0);
        let launch = || {
            assert!(world.reset_body(ball, start, crate::Vector3D::default()));
            assert!(world.set_linear_velocity(ball, crate::Vector3D::new(5.0, 0.0, 0.0)));
            for _ in 0..22 {
                assert!(world.step());
            }
        };
        launch();
        assert!(world.velocity(ball).x.abs() < 0.1);
        assert!(world.set_material(wall, 0.0, 0.0, 1.0));
        launch();
        assert!(world.velocity(ball).x < -2.3, "{:?}", world.velocity(ball));
        assert_eq!(world.restitution(wall), 1.0);
    }
}
