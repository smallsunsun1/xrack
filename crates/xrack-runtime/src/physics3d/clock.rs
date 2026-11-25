//! Fixed physics steps driven by elapsed time and optional render pose interpolation.

use std::time::Instant;

use rapier3d::prelude::Pose;

use super::{PhysicsWorld3D, Simulation, rotation_angles};
use crate::{Point3D, Vector3D};

const MAX_SUBSTEPS: i64 = 128;

pub(super) struct PhysicsClock {
    timestep: f64,
    committed_steps: u64,
    fraction: f64,
    alpha: f64,
    dropped_time: f64,
    time_scale: f64,
    running: bool,
    last_tick: Option<Instant>,
    changed: bool,
}

impl PhysicsClock {
    pub(super) fn new(timestep: f64) -> Self {
        Self {
            timestep,
            committed_steps: 0,
            fraction: 0.0,
            alpha: 1.0,
            dropped_time: 0.0,
            time_scale: 1.0,
            running: true,
            last_tick: None,
            changed: false,
        }
    }

    pub(super) fn clear(&mut self) {
        self.committed_steps = 0;
        self.fraction = 0.0;
        self.alpha = 1.0;
        self.dropped_time = 0.0;
        self.last_tick = None;
        self.changed = true;
    }

    pub(super) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    fn plan(&mut self, elapsed: f64, max_steps: u32) -> Option<u32> {
        if !self.running || self.time_scale == 0.0 {
            return Some(0);
        }
        let budget = self.fraction + elapsed * self.time_scale / self.timestep;
        if !budget.is_finite() {
            return None;
        }
        // Snap values one rounding error away from an exact step boundary.
        let nearest = budget.round();
        let tolerance = (8.0 * f64::EPSILON * budget.max(1.0)).min(1.0e-10);
        let budget = if nearest > 0.0 && (nearest - budget).abs() <= tolerance {
            nearest
        } else {
            budget
        };
        let whole = budget.floor();
        let steps = whole.min(f64::from(max_steps)) as u32;
        let fraction = budget - whole;
        let dropped = (whole - f64::from(steps)) * self.timestep;
        self.changed |= fraction != self.alpha || dropped > 0.0;
        self.fraction = fraction;
        self.alpha = fraction;
        self.dropped_time = (self.dropped_time + dropped).min(f64::MAX);
        Some(steps)
    }

    fn commit(&mut self, steps: u32) {
        self.committed_steps = self.committed_steps.saturating_add(u64::from(steps));
        self.changed = true;
    }
}

impl PhysicsWorld3D {
    /// Step once and expose the exact committed pose, including while the clock is paused.
    pub fn step(&self) -> bool {
        self.update(|simulation| {
            simulation.clock.last_tick = None;
            simulation.clock.fraction = 0.0;
            simulation.clock.alpha = 1.0;
            simulation.step_batch(1);
            simulation.snapshot_pose_history();
            Some(())
        })
        .is_some()
    }

    /// Advance by elapsed wall seconds, keeping a fractional step for the next call.
    /// Catch-up beyond the requested limit is counted as dropped simulation time.
    pub fn advance(&self, elapsed: f64, max_steps: i64) -> i64 {
        if !elapsed.is_finite() || elapsed < 0.0 {
            return 0;
        }
        self.advance_with(max_steps, |clock| {
            clock.last_tick = None;
            Some(elapsed)
        })
    }

    /// Advance using time elapsed since the previous call on this world.
    pub fn advance_realtime(&self, max_steps: i64) -> i64 {
        self.advance_realtime_at(max_steps, Instant::now())
    }

    fn advance_realtime_at(&self, max_steps: i64, now: Instant) -> i64 {
        self.advance_with(max_steps, |clock| {
            if !clock.running {
                clock.last_tick = None;
                return None;
            }
            clock
                .last_tick
                .replace(now)
                .map(|before| now.saturating_duration_since(before).as_secs_f64())
        })
    }

    fn advance_with(
        &self,
        max_steps: i64,
        elapsed: impl FnOnce(&mut PhysicsClock) -> Option<f64>,
    ) -> i64 {
        if !(1..=MAX_SUBSTEPS).contains(&max_steps) {
            return 0;
        }
        self.update_selectively(|simulation| {
            let Some(elapsed) = elapsed(&mut simulation.clock) else {
                return Some((0, false));
            };
            let steps = simulation.clock.plan(elapsed, max_steps as u32)?;
            if steps > 0 {
                simulation.step_batch(steps);
            }
            Some((i64::from(steps), steps > 0))
        })
        .unwrap_or_default()
    }

    pub fn set_running(&self, running: bool) -> bool {
        self.update_selectively(|simulation| {
            let clock = &mut simulation.clock;
            if clock.running != running {
                clock.running = running;
                clock.last_tick = None;
                clock.changed = true;
            }
            Some((true, false))
        })
        .is_some()
    }

    pub fn is_running(&self) -> bool {
        self.read_clock(|clock| clock.running)
    }

    pub fn set_time_scale(&self, scale: f64) -> bool {
        if !scale.is_finite() || scale < 0.0 {
            return false;
        }
        self.update_selectively(|simulation| {
            let clock = &mut simulation.clock;
            if clock.time_scale != scale {
                clock.time_scale = scale;
                clock.last_tick = None;
                clock.changed = true;
            }
            Some((true, false))
        })
        .is_some()
    }

    pub fn time_scale(&self) -> f64 {
        self.read_clock(|clock| clock.time_scale)
    }

    pub fn simulation_time(&self) -> f64 {
        self.read_clock(|clock| clock.committed_steps as f64 * clock.timestep)
    }

    pub fn dropped_time(&self) -> f64 {
        self.read_clock(|clock| clock.dropped_time)
    }

    pub fn interpolation_alpha(&self) -> f64 {
        self.read_clock(|clock| clock.alpha)
    }

    /// Retain the previous fixed-step pose for this body's interpolated getters.
    pub fn set_pose_interpolation(&self, body: i64, enabled: bool) -> bool {
        self.update_selectively(|simulation| {
            let handle = *simulation.bodies.get(&body)?;
            let pose = *simulation.world.bodies.get(handle)?.position();
            if enabled {
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    simulation.pose_history.entry(body)
                {
                    entry.insert(pose);
                    simulation.clock.changed = true;
                }
            } else if simulation.pose_history.remove(&body).is_some() {
                simulation.clock.changed = true;
            }
            Some((true, false))
        })
        .is_some()
    }

    pub fn interpolated_position(&self, body: i64) -> Point3D {
        self.interpolated_pose(body)
            .map(|pose| {
                let p = pose.translation;
                Point3D::new(p.x.into(), p.y.into(), p.z.into())
            })
            .unwrap_or_default()
    }

    pub fn interpolated_rotation(&self, body: i64) -> Vector3D {
        self.interpolated_pose(body)
            .map(|pose| rotation_angles(pose.rotation))
            .unwrap_or_default()
    }

    fn read_clock<T: Default>(&self, read: impl FnOnce(&PhysicsClock) -> T) -> T {
        let Some(state) = &self.0 else {
            return T::default();
        };
        state.clock_changed.track();
        state
            .state
            .try_borrow()
            .ok()
            .map(|simulation| read(&simulation.clock))
            .unwrap_or_default()
    }

    fn interpolated_pose(&self, body: i64) -> Option<Pose> {
        let state = self.0.as_ref()?;
        state.changed.track();
        state.clock_changed.track();
        let simulation = state.state.try_borrow().ok()?;
        simulation.render_pose(body)
    }
}

impl Simulation {
    pub(super) fn render_pose(&self, body: i64) -> Option<Pose> {
        let handle = *self.bodies.get(&body)?;
        let current = *self.world.bodies.get(handle)?.position();
        let alpha = self.clock.alpha as f32;
        let Some(previous) = self.pose_history.get(&body).filter(|_| alpha < 1.0) else {
            return Some(current);
        };
        Some(Pose::from_parts(
            previous.translation * (1.0 - alpha) + current.translation * alpha,
            previous.rotation.slerp(current.rotation, alpha),
        ))
    }

    fn step_batch(&mut self, steps: u32) {
        self.triggers.begin_batch();
        self.contact_events.begin_batch();
        self.clear_character_reports();
        for _ in 0..steps {
            self.snapshot_pose_history();
            self.prepare_character_step();
            self.align_distance_joints();
            self.apply_queued_loads();
            self.triggers
                .step(&mut self.world, &self.contacts, &self.static_friction);
            self.finish_kinematic_step();
            self.contact_events
                .sample(&self.world, &self.bodies, &self.contacts);
            self.contacts
                .collect_kinematic_contacts(&self.world, &self.bodies);
            self.use_stepped_query_bvh();
            self.restore_continuous_loads();
        }
        self.contacts.finish_batch();
        self.clock.commit(steps);
    }

    fn snapshot_pose_history(&mut self) {
        for (&id, pose) in &mut self.pose_history {
            if let Some(body) = self
                .bodies
                .get(&id)
                .and_then(|handle| self.world.bodies.get(*handle))
            {
                *pose = *body.position();
            }
        }
    }

    pub(super) fn snap_pose_history(&mut self, body: i64, pose: Pose) {
        if let Some(previous) = self.pose_history.get_mut(&body) {
            *previous = pose;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn moving_world(step: f64) -> (PhysicsWorld3D, i64) {
        let world = PhysicsWorld3D::new(Vector3D::default(), step);
        let body = world.add_sphere(Point3D::default(), 0.1, 1.0, 0.0, 0.0);
        assert!(body > 0);
        assert!(world.set_linear_velocity(body, Vector3D::new(2.0, 0.0, 0.0)));
        (world, body)
    }

    #[test]
    fn frames_between_steps_only_change_clock_and_interpolated_bindings() {
        let (world, body) = moving_world(0.1);
        let position = crate::Property::default();
        let interpolated = crate::Property::default();
        assert!(world.set_pose_interpolation(body, true));
        assert_eq!(world.advance(0.025, 4), 0);
        assert_eq!(world.advance(0.075, 4), 1);
        assert_eq!(world.simulation_time(), 0.1);
        assert!((position.get(|| world.position(body)).x - 0.2).abs() < 1e-5);
        assert_eq!(
            interpolated.get(|| world.interpolated_position(body)).x,
            0.0
        );
        assert_eq!(world.advance(0.05, 4), 0);
        assert!(!position.is_dirty());
        assert!(interpolated.is_dirty());
        assert!((interpolated.get(|| world.interpolated_position(body)).x - 0.1).abs() < 1e-5);
        let teleport = Point3D::new(3.0, 0.0, 0.0);
        assert!(world.reset_body(body, teleport, Vector3D::default()));
        assert_eq!(world.interpolated_position(body), teleport);
        assert!(world.step());
        assert_eq!(world.interpolation_alpha(), 1.0);
        assert_eq!(world.advance(0.025, 4), 0);
        assert_eq!(world.interpolated_position(body), teleport);
    }

    #[test]
    fn catch_up_is_capped_with_exact_remainder_and_explicit_pause() {
        let (world, body) = moving_world(0.1);
        assert_eq!(world.advance(0.35, 2), 2);
        assert!((world.dropped_time() - 0.1).abs() < 1e-12);
        assert!((world.interpolation_alpha() - 0.5).abs() < 1e-12);
        assert!((world.position(body).x - 0.4).abs() < 1e-5);

        assert!(world.set_running(false));
        assert!(!world.is_running());
        assert_eq!(world.advance(200.0, 2), 0);
        assert_eq!(world.simulation_time(), 0.2);
        assert!(world.step());
        assert!((world.simulation_time() - 0.3).abs() < 1e-12);
        assert_eq!(world.interpolation_alpha(), 1.0);

        assert!(world.set_running(true));
        assert!(world.set_time_scale(0.5));
        assert_eq!(world.advance(0.2, 2), 1);
        assert_eq!(world.time_scale(), 0.5);
        assert!(!world.set_time_scale(-1.0));
        assert_eq!(world.advance(f64::INFINITY, 2), 0);
        assert_eq!(world.advance(1.0, 0), 0);
        assert_eq!(world.advance(1.0, MAX_SUBSTEPS + 1), 0);
        assert_eq!(world.simulation_time(), 0.4);
        assert!(world.clear());
        assert_eq!(world.time_scale(), 0.5);
        assert_eq!(world.simulation_time(), 0.0);
        assert_eq!(world.dropped_time(), 0.0);
    }

    #[test]
    fn monotonic_driver_resets_its_baseline_after_pause() {
        let (world, _) = moving_world(0.1);
        let origin = Instant::now();
        assert_eq!(world.advance_realtime_at(4, origin), 0);
        assert_eq!(world.interpolation_alpha(), 1.0);
        assert_eq!(
            world.advance_realtime_at(4, origin + Duration::from_millis(120)),
            1
        );
        assert!(world.set_running(false));
        assert_eq!(
            world.advance_realtime_at(4, origin + Duration::from_secs(20)),
            0
        );
        assert!(world.set_running(true));
        assert_eq!(
            world.advance_realtime_at(4, origin + Duration::from_secs(21)),
            0
        );
        assert_eq!(
            world.advance_realtime_at(4, origin + Duration::from_millis(21080)),
            1
        );
        assert_eq!(world.simulation_time(), 0.2);
        assert_eq!(world.dropped_time(), 0.0);
    }

    #[test]
    fn a_batch_keeps_intermediate_triggers_and_applies_one_shot_load_once() {
        let (batched, body) = moving_world(0.05);
        let (manual, reference) = moving_world(0.05);
        let start = Point3D::new(-1.0, 0.0, 0.0);
        let zero = Vector3D::default();
        let sensor =
            batched.add_box_trigger(Point3D::default(), Vector3D::new(0.4, 2.0, 2.0), zero);
        for (world, handle) in [(&batched, body), (&manual, reference)] {
            assert!(world.reset_body(handle, start, zero));
            assert!(world.set_linear_velocity(handle, Vector3D::new(4.0, 0.0, 0.0)));
            assert!(world.apply_force(handle, Vector3D::new(1.0, 0.0, 0.0)));
        }
        assert_eq!(batched.advance(0.6, 12), 12);
        for _ in 0..12 {
            assert!(manual.step());
        }
        assert_eq!(batched.position(body), manual.position(reference));
        assert_eq!(batched.velocity(body), manual.velocity(reference));
        assert_eq!(batched.trigger_entered(sensor), [body]);
        assert_eq!(batched.trigger_exited(sensor), [body]);
        assert_eq!(batched.trigger_count(sensor), 0);
        assert_eq!(batched.advance(0.05, 12), 1);
        assert!(batched.trigger_entered(sensor).is_empty());
        assert!(batched.trigger_exited(sensor).is_empty());
    }

    #[test]
    fn a_batch_preserves_each_character_hit_in_substep_order() {
        let setup = || {
            let zero = Vector3D::default();
            let world = PhysicsWorld3D::new(zero, 0.05);
            let wall = world.add_box(
                Point3D::new(0.4, 0.0, 0.0),
                Vector3D::new(0.2, 2.0, 2.0),
                0.0,
                0.0,
                0.0,
            );
            let actor = world.add_character(Point3D::default(), 0.25, 0.6);
            assert!(wall > 0 && actor > 0);
            assert!(world.set_character_hit_reports(actor, true));
            assert!(world.set_character_movement(actor, Vector3D::new(2.0, 0.0, 0.0)));
            (world, actor)
        };
        let (manual, reference) = setup();
        let (batched, actor) = setup();
        let mut expected = Vec::new();
        for _ in 0..6 {
            assert!(manual.step());
            expected.extend(manual.character_hits(reference));
        }
        assert!(expected.len() > manual.character_hits(reference).len());
        assert_eq!(batched.advance(0.3, 6), 6);
        assert_eq!(batched.character_hits(actor), expected);
    }

    #[test]
    fn contact_snapshot_keeps_the_bounce_from_an_earlier_substep() {
        let zero = Vector3D::default();
        let world = PhysicsWorld3D::new(zero, 0.05);
        let wall = world.add_box(
            Point3D::default(),
            Vector3D::new(0.2, 2.0, 2.0),
            0.0,
            0.0,
            1.0,
        );
        let ball = world.add_sphere(Point3D::new(-1.0, 0.0, 0.0), 0.1, 1.0, 0.0, 1.0);
        assert!(world.set_contact_reports(wall, true, false));
        assert!(world.set_contact_reports(ball, false, true));
        assert!(world.set_linear_velocity(ball, Vector3D::new(4.0, 0.0, 0.0)));
        assert_eq!(world.advance(0.5, 10), 10);
        assert!(world.position(ball).x < -0.5);
        assert!(world.contacts(ball).iter().any(|hit| hit.body == wall));
        assert_eq!(world.advance(0.05, 10), 1);
        assert!(world.contacts(ball).is_empty());
    }
}
