//! Rigid-body constraints authored in world coordinates.

use rapier3d::prelude::{
    GenericJoint, GenericJointBuilder, ImpulseJointHandle, JointAxesMask, JointAxis, Pose, Real,
    Rotation, Vector,
};

use super::{PhysicsWorld3D, Simulation, vector};
use crate::{Point3D, Vector3D};

pub(super) struct JointEntry {
    pub(super) handle: ImpulseJointHandle,
    kind: JointKind,
    name: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum JointKind {
    Fixed,
    Spherical,
    Revolute,
    Prismatic,
    Distance,
}

impl JointKind {
    pub(super) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "fixed" => Self::Fixed,
            "spherical" => Self::Spherical,
            "revolute" => Self::Revolute,
            "prismatic" => Self::Prismatic,
            "distance" => Self::Distance,
            _ => return None,
        })
    }

    pub(super) fn locked_axes(self) -> JointAxesMask {
        match self {
            Self::Fixed => JointAxesMask::LOCKED_FIXED_AXES,
            Self::Spherical => JointAxesMask::LOCKED_SPHERICAL_AXES,
            Self::Revolute => JointAxesMask::LOCKED_REVOLUTE_AXES,
            Self::Prismatic => JointAxesMask::LOCKED_PRISMATIC_AXES,
            Self::Distance => JointAxesMask::empty(),
        }
    }

    pub(super) fn limits(self, min: f64, max: f64) -> Option<(JointAxis, [Real; 2])> {
        if min > max {
            return None;
        }
        let (axis, min, max) = match self {
            Self::Revolute if min >= -180.0 && max <= 180.0 => {
                (JointAxis::AngX, min.to_radians(), max.to_radians())
            }
            Self::Prismatic => (JointAxis::LinX, min, max),
            Self::Distance if min >= 0.0 && max > 0.0 => (JointAxis::LinX, min, max),
            _ => return None,
        };
        let (min, max) = (min as Real, max as Real);
        let valid = min.is_finite() && max.is_finite() && (self != Self::Distance || max > 0.0);
        valid.then_some((axis, [min, max]))
    }
}

impl PhysicsWorld3D {
    /// Connect two bodies at a world-space anchor and preserve their initial relative pose.
    pub fn add_fixed_joint(&self, first: i64, second: i64, anchor: Point3D) -> i64 {
        self.add_anchored_joint([first, second], anchor, JointKind::Fixed)
    }

    /// Connect two bodies at a world-space anchor while allowing free rotation.
    pub fn add_spherical_joint(&self, first: i64, second: i64, anchor: Point3D) -> i64 {
        self.add_anchored_joint([first, second], anchor, JointKind::Spherical)
    }

    /// Connect two bodies around a world-space axis with relative angular limits in degrees.
    pub fn add_revolute_joint(
        &self,
        first: i64,
        second: i64,
        anchor: Point3D,
        axis: Vector3D,
        min: f64,
        max: f64,
    ) -> i64 {
        self.add_axis_joint(
            [first, second],
            anchor,
            axis,
            [min, max],
            JointKind::Revolute,
        )
    }

    /// Connect two bodies along a world-space axis with relative translation limits.
    pub fn add_prismatic_joint(
        &self,
        first: i64,
        second: i64,
        anchor: Point3D,
        axis: Vector3D,
        min: f64,
        max: f64,
    ) -> i64 {
        self.add_axis_joint(
            [first, second],
            anchor,
            axis,
            [min, max],
            JointKind::Prismatic,
        )
    }

    /// Constrain the distance between world-space anchors at the bodies' current poses.
    pub fn add_distance_joint(
        &self,
        first: i64,
        second: i64,
        first_anchor: Point3D,
        second_anchor: Point3D,
        min: f64,
        max: f64,
    ) -> i64 {
        let (Some(first_anchor), Some(second_anchor), Some((limit_axis, limits))) = (
            vector([first_anchor.x, first_anchor.y, first_anchor.z]),
            vector([second_anchor.x, second_anchor.y, second_anchor.z]),
            JointKind::Distance.limits(min, max),
        ) else {
            return 0;
        };
        self.insert_joint([first, second], JointKind::Distance, |[first, second]| {
            let mut joint = GenericJointBuilder::new(JointKind::Distance.locked_axes())
                .local_anchor1(first.inverse_transform_point(first_anchor))
                .local_anchor2(second.inverse_transform_point(second_anchor))
                .limits(limit_axis, limits)
                .build();
            joint.coupled_axes = distance_coupling(limits[0]);
            if joint.coupled_axes.is_empty() {
                let axis = (second_anchor - first_anchor)
                    .try_normalize()
                    .unwrap_or(Vector::X);
                set_distance_axis(&mut joint, first, second, axis);
            }
            joint
        })
    }

    pub fn set_joint_limits(&self, joint: i64, min: f64, max: f64) -> bool {
        let configure = || {
            let mut simulation = self.0.as_ref()?.state.try_borrow_mut().ok()?;
            let (handle, kind) = simulation
                .joints
                .get(&joint)
                .map(|entry| (entry.handle, entry.kind))?;
            let (axis, limits) = kind.limits(min, max)?;
            let constraint = simulation.world.impulse_joints.get_mut(handle, true)?;
            constraint.data.set_limits(axis, limits);
            if kind == JointKind::Distance {
                let coupled = distance_coupling(limits[0]);
                constraint.data.coupled_axes = coupled;
                if coupled.is_empty() {
                    simulation.bounded_distance_joints.insert(joint, handle);
                } else {
                    simulation.bounded_distance_joints.remove(&joint);
                }
            }
            Some(())
        };
        configure().is_some()
    }

    /// Drive a revolute or prismatic joint toward a velocity. A zero force disables its motor.
    pub fn set_joint_motor(&self, joint: i64, velocity: f64, max_force: f64) -> bool {
        let max_force = max_force as Real;
        if !(max_force.is_finite() && max_force >= 0.0) {
            return false;
        }
        let configure = || {
            let mut simulation = self.0.as_ref()?.state.try_borrow_mut().ok()?;
            let (handle, kind) = simulation
                .joints
                .get(&joint)
                .map(|entry| (entry.handle, entry.kind))?;
            let (axis, speed) = match kind {
                JointKind::Revolute => (JointAxis::AngX, velocity.to_radians() as Real),
                JointKind::Prismatic => (JointAxis::LinX, velocity as Real),
                _ => return None,
            };
            if !speed.is_finite() {
                return None;
            }
            let responsiveness = simulation.world.integration_parameters.dt.recip();
            let joint = simulation.world.impulse_joints.get_mut(handle, true)?;
            if max_force == 0.0 {
                joint.data.motor_axes.remove(axis.into());
            } else {
                joint.data.set_motor_velocity(axis, speed, responsiveness);
                joint.data.set_motor_max_force(axis, max_force);
            }
            Some(())
        };
        configure().is_some()
    }

    /// Read the relative angle in degrees, slider travel, or distance between anchors.
    pub fn joint_position(&self, joint: i64) -> f64 {
        let read = || {
            let state = self.0.as_ref()?;
            state.changed.track();
            let simulation = state.state.try_borrow().ok()?;
            let entry = simulation.joints.get(&joint)?;
            let joint = simulation.world.impulse_joints.get(entry.handle)?;
            let first = simulation.world.bodies.get(joint.body1())?.position();
            let second = simulation.world.bodies.get(joint.body2())?.position();
            let value = match entry.kind {
                JointKind::Revolute => joint
                    .data
                    .as_revolute()?
                    .angle(&first.rotation, &second.rotation)
                    .to_degrees(),
                JointKind::Prismatic | JointKind::Distance => {
                    let first = *first * joint.data.local_frame1;
                    let second = *second * joint.data.local_frame2;
                    let delta = second.translation - first.translation;
                    if entry.kind == JointKind::Prismatic {
                        delta.dot(first.rotation * Vector::X)
                    } else {
                        delta.length()
                    }
                }
                _ => 0.0,
            };
            Some(f64::from(value))
        };
        read().unwrap_or_default()
    }

    pub fn remove_joint(&self, joint: i64) -> bool {
        self.update(|simulation| simulation.remove_joint(joint))
            .is_some()
    }

    fn add_anchored_joint(&self, bodies: [i64; 2], anchor: Point3D, kind: JointKind) -> i64 {
        let Some(anchor) = vector([anchor.x, anchor.y, anchor.z]) else {
            return 0;
        };
        let frame = Pose::from_parts(anchor, Rotation::IDENTITY);
        self.insert_joint(bodies, kind, |[first, second]| {
            GenericJointBuilder::new(kind.locked_axes())
                .local_frame1(first.inv_mul(&frame))
                .local_frame2(second.inv_mul(&frame))
                .build()
        })
    }

    fn add_axis_joint(
        &self,
        bodies: [i64; 2],
        anchor: Point3D,
        axis: Vector3D,
        [min, max]: [f64; 2],
        kind: JointKind,
    ) -> i64 {
        let (Some(anchor), Some(axis), Some((limit_axis, limits))) = (
            vector([anchor.x, anchor.y, anchor.z]),
            vector([axis.x, axis.y, axis.z]).and_then(Vector::try_normalize),
            kind.limits(min, max),
        ) else {
            return 0;
        };
        let frame = Pose::from_parts(anchor, Rotation::from_rotation_arc(Vector::X, axis));
        self.insert_joint(bodies, kind, |[first, second]| {
            GenericJointBuilder::new(kind.locked_axes())
                .local_frame1(first.inv_mul(&frame))
                .local_frame2(second.inv_mul(&frame))
                .limits(limit_axis, limits)
                .build()
        })
    }

    fn insert_joint(
        &self,
        [first, second]: [i64; 2],
        kind: JointKind,
        make_joint: impl FnOnce([Pose; 2]) -> GenericJoint,
    ) -> i64 {
        if first == second {
            return 0;
        }
        self.update(|simulation| {
            if [first, second]
                .iter()
                .any(|body| simulation.triggers.states.contains_key(body))
            {
                return None;
            }
            let first = *simulation.bodies.get(&first)?;
            let second = *simulation.bodies.get(&second)?;
            let first_body = simulation.world.bodies.get(first)?;
            let second_body = simulation.world.bodies.get(second)?;
            if !(first_body.is_dynamic() || second_body.is_dynamic()) {
                return None;
            }
            let data = make_joint([*first_body.position(), *second_body.position()]);
            simulation.register_joint([first, second], kind, data, None)
        })
        .unwrap_or_default()
    }
}

impl Simulation {
    pub(super) fn register_joint(
        &mut self,
        [first, second]: [rapier3d::prelude::RigidBodyHandle; 2],
        kind: JointKind,
        mut data: GenericJoint,
        name: Option<String>,
    ) -> Option<i64> {
        let id = self.next_id;
        self.next_id = id.checked_add(1)?;
        data.user_data = id as u128;
        data.set_contacts_enabled(false);
        let align = kind == JointKind::Distance && data.coupled_axes.is_empty();
        let handle = self.world.insert_impulse_joint(first, second, data);
        self.joints.insert(id, JointEntry { handle, kind, name });
        if align {
            self.bounded_distance_joints.insert(id, handle);
        }
        Some(id)
    }

    pub(super) fn remove_joint(&mut self, joint: i64) -> Option<()> {
        let entry = self.joints.get(&joint)?;
        self.world.remove_impulse_joint(entry.handle)?;
        self.untrack_joint(joint);
        Some(())
    }

    pub(super) fn untrack_joint(&mut self, joint: i64) {
        if let Some(name) = self.joints.remove(&joint).and_then(|entry| entry.name) {
            self.authored_joints_changed |= self.authored_joints.remove(&name).is_some();
        }
        self.bounded_distance_joints.remove(&joint);
    }

    pub(super) fn align_distance_joints(&mut self) {
        for &handle in self.bounded_distance_joints.values() {
            let Some(joint) = self.world.impulse_joints.get(handle) else {
                continue;
            };
            let (Some(first), Some(second)) = (
                self.world.bodies.get(joint.body1()),
                self.world.bodies.get(joint.body2()),
            ) else {
                continue;
            };
            let (first, second) = (*first.position(), *second.position());
            let first_anchor = first.transform_point(joint.data.local_anchor1());
            let second_anchor = second.transform_point(joint.data.local_anchor2());
            let Some(axis) = (second_anchor - first_anchor).try_normalize() else {
                continue;
            };
            let current_axis = first.transform_vector(joint.data.local_axis1());
            if (current_axis - axis).length_squared() > 1e-10
                && let Some(joint) = self.world.impulse_joints.get_mut(handle, false)
            {
                set_distance_axis(&mut joint.data, first, second, axis);
            }
        }
    }
}

pub(super) fn distance_coupling(min: Real) -> JointAxesMask {
    // Rapier's coupled linear limit only enforces the upper bound. A lower bound
    // uses the solver's signed linear limit on the current radial axis.
    if min > 0.0 {
        JointAxesMask::empty()
    } else {
        JointAxesMask::LIN_AXES
    }
}

pub(super) fn set_distance_axis(joint: &mut GenericJoint, first: Pose, second: Pose, axis: Vector) {
    joint.set_local_axis1(first.inverse_transform_vector(axis));
    joint.set_local_axis2(second.inverse_transform_vector(axis));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simulate(world: &PhysicsWorld3D, steps: usize) {
        for _ in 0..steps {
            assert!(world.step());
        }
    }

    #[test]
    fn fixed_and_spherical_joints_share_an_anchor_but_have_different_motion() {
        let world = PhysicsWorld3D::new(Vector3D::new(0.0, -9.81, 0.0), 1.0 / 120.0);
        let origin = Point3D::default();
        let start = Point3D::new(1.0, 0.0, 0.0);
        let support = world.add_sphere(origin, 0.08, 0.0, 0.0, 0.0);
        let bob = world.add_sphere(start, 0.2, 1.0, 0.0, 0.0);
        let sensor =
            world.add_box_trigger(origin, Vector3D::new(1.0, 1.0, 1.0), Vector3D::default());
        assert_eq!(world.add_fixed_joint(support, support, origin), 0);
        assert_eq!(world.add_fixed_joint(sensor, bob, origin), 0);
        let fixed = world.add_fixed_joint(support, bob, origin);
        assert!(fixed > bob);
        simulate(&world, 60);
        let held = world.position(bob);
        assert!(
            (held.x - 1.0).abs() < 0.005 && held.y.abs() < 0.005,
            "{held:?}"
        );
        assert!(!world.set_joint_limits(fixed, 0.0, 1.0));
        assert!(world.remove_joint(fixed));
        assert!(!world.remove_joint(fixed));

        let ball = world.add_spherical_joint(support, bob, origin);
        assert!(ball > fixed);
        simulate(&world, 60);
        let moved = world.position(bob);
        assert!(moved.y < -0.5, "{moved:?}");
        assert!((moved.x.hypot(moved.y) - 1.0).abs() < 0.015, "{moved:?}");
        assert!(world.remove(support));
        assert!(!world.remove_joint(ball));
        let after_detach = world.position(bob);
        simulate(&world, 60);
        assert!(world.position(bob).y < after_detach.y - 0.6);
    }

    #[test]
    fn hinge_drive_and_updated_limits_preserve_anchor_and_restrict_angle() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 1.0 / 120.0);
        let origin = Point3D::default();
        let start = Point3D::new(1.0, 0.0, 0.0);
        let mount = world.add_sphere(origin, 0.08, 0.0, 0.0, 0.0);
        let door = world.add_box(start, Vector3D::new(2.0, 0.6, 0.15), 1.0, 0.0, 0.0);
        let up = Vector3D::new(0.0, 2.0, 0.0);
        assert_eq!(
            world.add_revolute_joint(mount, door, origin, Vector3D::default(), -10.0, 65.0),
            0
        );
        assert_eq!(
            world.add_revolute_joint(mount, door, origin, up, -181.0, 65.0),
            0
        );
        let hinge = world.add_revolute_joint(mount, door, origin, up, -10.0, 65.0);
        assert!(hinge > 0);
        assert!(world.joint_position(hinge).abs() < 0.001);
        assert!(world.set_joint_motor(hinge, 120.0, 50.0));
        simulate(&world, 240);
        let angle = world.joint_position(hinge);
        assert!((angle - 65.0).abs() < 1.5, "{angle}");
        let center = world.position(door);
        assert!((center.x.hypot(center.z) - 1.0).abs() < 0.01 && center.y.abs() < 0.01);

        assert!(world.set_joint_limits(hinge, -30.0, 25.0));
        assert!(!world.set_joint_limits(hinge, 24.0, -24.0));
        simulate(&world, 80);
        assert!((world.joint_position(hinge) - 25.0).abs() < 1.5);
        assert!(world.set_joint_motor(hinge, 0.0, 0.0));
        assert!(world.clear());
        assert!(!world.set_joint_motor(hinge, 120.0, 50.0));
    }

    #[test]
    fn slider_constrains_translation_to_an_oblique_axis() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 1.0 / 120.0);
        let mount = world.add_sphere(Point3D::default(), 0.08, 0.0, 0.0, 0.0);
        let slider = world.add_sphere(Point3D::new(2.0, 0.0, 0.0), 0.2, 1.0, 0.0, 0.0);
        let joint = world.add_prismatic_joint(
            mount,
            slider,
            Point3D::new(1.0, 0.0, 0.0),
            Vector3D::new(0.0, 1.0, 1.0),
            -0.6,
            1.2,
        );
        assert!(joint > 0);
        assert!(world.apply_impulse(slider, Vector3D::new(10.0, 4.0, 4.0)));
        simulate(&world, 80);
        let point = world.position(slider);
        assert!(
            (point.x - 2.0).abs() < 0.01 && (point.y - point.z).abs() < 0.01,
            "{point:?}"
        );
        assert!((world.joint_position(joint) - 1.2).abs() < 0.025);
        assert!(world.set_joint_motor(joint, -3.0, 50.0));
        simulate(&world, 160);
        assert!((world.joint_position(joint) + 0.6).abs() < 0.025);
    }

    #[test]
    fn distance_joint_enforces_both_ends_of_the_range() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 1.0 / 120.0);
        let origin = Point3D::default();
        let start = Point3D::new(1.5, 0.0, 0.0);
        let mount = world.add_sphere(origin, 0.08, 0.0, 0.0, 0.0);
        let bob = world.add_sphere(start, 0.2, 1.0, 0.0, 0.0);
        let joint = world.add_distance_joint(mount, bob, origin, start, 1.25, 2.0);
        assert!(joint > 0);
        assert!((world.joint_position(joint) - 1.5).abs() < 0.001);
        assert!(!world.set_joint_motor(joint, 1.0, 10.0));
        assert!(world.apply_impulse(bob, Vector3D::new(-8.0, 0.0, 0.0)));
        simulate(&world, 60);
        let short = world.joint_position(joint);
        assert!((short - 1.25).abs() < 0.025, "{short}");
        assert!(world.apply_impulse(bob, Vector3D::new(8.0, 0.0, 0.0)));
        simulate(&world, 60);
        let long = world.joint_position(joint);
        assert!((long - 2.0).abs() < 0.025, "{long}");
        assert!(world.apply_impulse(bob, Vector3D::new(0.0, 4.0, 2.0)));
        for _ in 0..100 {
            assert!(world.step());
            let distance = world.joint_position(joint);
            assert!((1.23..2.025).contains(&distance), "{distance}");
        }
        assert!(world.position(bob).y.abs() > 0.4);
    }
}
