//! Capsule character movement against the physics world's actual colliders.

use rapier3d::{
    control::{
        CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController,
    },
    parry::{
        query::{ShapeCastHit, ShapeCastOptions},
        shape::Shape,
    },
    prelude::{
        Collider, ColliderBuilder, ColliderHandle, Pose, QueryFilter, QueryFilterFlags,
        QueryPipeline, QueryPipelineMut, Ray, RigidBodyBuilder, Vector,
    },
};

use super::{PhysicsContact3D, PhysicsWorld3D, Simulation, contacts::as_vector, vector};
use crate::{Point3D, Vector3D};

const SIDE: i64 = 1;
const UP: i64 = 2;
const DOWN: i64 = 4;

pub(super) struct Character {
    controller: KinematicCharacterController,
    pub(super) gravity: Vector,
    pub(super) follows_world_gravity: bool,
    movement: Vector,
    airborne_movement: Vector,
    gravity_velocity: Vector,
    mid_air_control: bool,
    skin: f32,
    min_step_width: f32,
    push_mass: f32,
    flags: i64,
    support: i64,
    hits: Vec<CharacterCollision>,
    report_hits: bool,
    reports: Vec<PhysicsContact3D>,
}

impl PhysicsWorld3D {
    /// Add an upright capsule. Segment height is the distance between the hemisphere centers.
    pub fn add_character(&self, center: Point3D, radius: f64, segment_height: f64) -> i64 {
        let Some(center) = vector([center.x, center.y, center.z]) else {
            return 0;
        };
        let (radius, segment) = (radius as f32, segment_height as f32);
        let height = segment + 2.0 * radius;
        if !(height.is_finite() && radius > 0.0 && segment >= 0.0) {
            return 0;
        }
        let shape = if segment == 0.0 {
            ColliderBuilder::ball(radius)
        } else {
            ColliderBuilder::capsule_y(segment * 0.5, radius)
        };
        let skin = (radius * 0.02).max(f32::EPSILON);
        let min_step_width = radius;
        self.insert_colliders(
            RigidBodyBuilder::kinematic_position_based().translation(center),
            [shape],
            Some(Character {
                controller: KinematicCharacterController {
                    offset: CharacterLength::Absolute(skin),
                    autostep: autostep(height * 0.25, min_step_width),
                    ..Default::default()
                },
                gravity: Vector::ZERO,
                follows_world_gravity: true,
                movement: Vector::ZERO,
                airborne_movement: Vector::ZERO,
                gravity_velocity: Vector::ZERO,
                mid_air_control: true,
                skin,
                min_step_width,
                push_mass: 1.0,
                flags: 0,
                support: 0,
                hits: Vec::new(),
                report_hits: false,
                reports: Vec::new(),
            }),
        )
    }

    /// Set the persistent desired velocity in world units per second.
    pub fn set_character_movement(&self, body: i64, velocity: Vector3D) -> bool {
        let Some(velocity) = vector([velocity.x, velocity.y, velocity.z]) else {
            return false;
        };
        self.update_character(body, |character| character.movement = velocity)
    }

    pub fn set_character_gravity(&self, body: i64, gravity: Vector3D) -> bool {
        let Some(gravity) = vector([gravity.x, gravity.y, gravity.z]) else {
            return false;
        };
        self.update_selectively(|simulation| {
            let character = simulation.characters.get_mut(&body)?;
            character.follows_world_gravity = false;
            character.update_gravity(gravity);
            simulation.settings_changed = true;
            Some((true, false))
        })
        .is_some()
    }

    pub fn set_character_mid_air_control(&self, body: i64, enabled: bool) -> bool {
        self.update_character(body, |character| character.mid_air_control = enabled)
    }

    /// Set stair and ground-snap distances, slope limit in degrees, and pushing mass.
    pub fn set_character_options(
        &self,
        body: i64,
        step_height: f64,
        snap_distance: f64,
        max_slope: f64,
        push_mass: f64,
    ) -> bool {
        if !(0.0..90.0).contains(&max_slope) {
            return false;
        }
        let [step_height, snap_distance, push_mass] =
            [step_height, snap_distance, push_mass].map(|value| value as f32);
        if ![step_height, snap_distance, push_mass]
            .into_iter()
            .all(|value| value.is_finite() && value >= 0.0)
        {
            return false;
        }
        let slope = max_slope.to_radians() as f32;
        self.update_character(body, |character| {
            character.controller.autostep = autostep(step_height, character.min_step_width);
            character.controller.snap_to_ground =
                (snap_distance > 0.0).then_some(CharacterLength::Absolute(snap_distance));
            character.controller.max_slope_climb_angle = slope;
            character.controller.min_slope_slide_angle = slope;
            character.push_mass = push_mass;
        })
    }

    pub fn jump_character(&self, body: i64, speed: f64) -> bool {
        let speed = speed as f32;
        if !(speed.is_finite() && speed > 0.0) {
            return false;
        }
        self.update(|simulation| {
            let character = simulation.characters.get_mut(&body)?;
            if character.flags & DOWN == 0 {
                return None;
            }
            character.gravity_velocity.y = speed;
            character.airborne_movement = character.movement;
            character.flags &= !DOWN;
            character.support = 0;
            Some(())
        })
        .is_some()
    }

    pub fn teleport_character(&self, body: i64, center: Point3D) -> bool {
        let Some(center) = vector([center.x, center.y, center.z]) else {
            return false;
        };
        self.update(|simulation| {
            let character = simulation.characters.get_mut(&body)?;
            let handle = *simulation.bodies.get(&body)?;
            simulation
                .world
                .bodies
                .get_mut(handle)?
                .set_position(Pose::from_translation(center), true);
            character.gravity_velocity = Vector::ZERO;
            character.airborne_movement = character.movement;
            character.flags = 0;
            character.support = 0;
            simulation.character_hits_changed |= !character.reports.is_empty();
            character.reports.clear();
            simulation.snap_pose_history(body, Pose::from_translation(center));
            simulation.invalidate_query_bvh();
            Some(())
        })
        .is_some()
    }

    pub fn character_grounded(&self, body: i64) -> bool {
        self.character_collisions(body) & DOWN != 0
    }

    pub fn character_hit_wall(&self, body: i64) -> bool {
        self.character_collisions(body) & SIDE != 0
    }

    pub fn character_hit_ceiling(&self, body: i64) -> bool {
        self.character_collisions(body) & UP != 0
    }

    /// The last step's side (1), above (2), and ground (4) flags.
    pub fn character_collisions(&self, body: i64) -> i64 {
        self.read_character(body, |character| character.flags)
    }

    /// The body supporting this character after its last move, or zero.
    pub fn character_support(&self, body: i64) -> i64 {
        self.read_character(body, |character| character.support)
    }

    /// Enable shape-hit snapshots for a character. Reporting is disabled initially.
    pub fn set_character_hit_reports(&self, body: i64, enabled: bool) -> bool {
        self.update(|simulation| {
            let character = simulation.characters.get_mut(&body)?;
            character.report_hits = enabled;
            if !enabled {
                simulation.character_hits_changed |= !character.reports.is_empty();
                character.reports.clear();
            }
            Some(())
        })
        .is_some()
    }

    /// Read the shape hits from the latest completed step or batch, in sweep order.
    pub fn character_hits(&self, body: i64) -> Vec<PhysicsContact3D> {
        let read = || {
            let state = self.0.as_ref()?;
            state.character_hits_changed.track();
            let simulation = state.state.try_borrow().ok()?;
            simulation
                .characters
                .get(&body)
                .map(|character| character.reports.clone())
        };
        read().unwrap_or_default()
    }

    fn update_character(&self, body: i64, edit: impl FnOnce(&mut Character)) -> bool {
        self.update(|simulation| {
            edit(simulation.characters.get_mut(&body)?);
            Some(())
        })
        .is_some()
    }

    fn read_character<T: Default>(&self, body: i64, read: impl FnOnce(&Character) -> T) -> T {
        let value = || {
            let state = self.0.as_ref()?;
            state.changed.track();
            state
                .state
                .try_borrow()
                .ok()?
                .characters
                .get(&body)
                .map(read)
        };
        value().unwrap_or_default()
    }
}

impl Simulation {
    pub(super) fn clear_character_reports(&mut self) {
        for character in self.characters.values_mut() {
            self.character_hits_changed |= !character.reports.is_empty();
            character.reports.clear();
        }
    }

    pub(super) fn prepare_character_step(&mut self) {
        if self.characters.is_empty() {
            return;
        }
        self.prepare_query_bvh();
        let world = &mut self.world;
        let dt = world.integration_parameters.dt;
        for (id, character) in &mut self.characters {
            let Some(&body) = self.bodies.get(id) else {
                continue;
            };
            let Some(rigid) = world.bodies.get(body) else {
                continue;
            };
            let Some(collider) = rigid
                .colliders()
                .first()
                .and_then(|handle| world.colliders.get(*handle))
            else {
                continue;
            };
            let pose = *rigid.position();
            let groups = collider.collision_groups();
            let shape = collider.shared_shape().clone();
            let filter = QueryFilter::default()
                .exclude_sensors()
                .exclude_rigid_body(body)
                .groups(groups);
            let dispatcher = world.narrow_phase.query_dispatcher();
            let mut query = if let Some(bvh) = &self.query_bvh {
                QueryPipelineMut {
                    dispatcher,
                    bvh,
                    bodies: &mut world.bodies,
                    colliders: &mut world.colliders,
                    filter,
                }
            } else {
                world.broad_phase.as_query_pipeline_mut(
                    dispatcher,
                    &mut world.bodies,
                    &mut world.colliders,
                    filter,
                )
            };
            let next = character.move_step(&mut query, &*shape, pose, dt);
            self.character_hits_changed |= !character.reports.is_empty();
            if let Some(rigid) = world.bodies.get_mut(body) {
                rigid.set_next_kinematic_position(next);
            }
        }
    }

    pub(super) fn forget_character_body(&mut self, body: i64) {
        self.character_hits_changed |= self
            .characters
            .remove(&body)
            .is_some_and(|character| !character.reports.is_empty());
        for character in self.characters.values_mut() {
            if character.support == body {
                character.support = 0;
                character.flags &= !DOWN;
            }
            let count = character.reports.len();
            character.reports.retain(|point| point.body != body);
            self.character_hits_changed |= character.reports.len() != count;
        }
    }

    pub(super) fn clear_characters(&mut self) {
        self.character_hits_changed |= self
            .characters
            .values()
            .any(|character| !character.reports.is_empty());
        self.characters.clear();
    }
}

impl Character {
    pub(super) fn update_gravity(&mut self, gravity: Vector) {
        self.gravity = gravity;
        if gravity == Vector::ZERO {
            self.gravity_velocity = Vector::ZERO;
        }
    }

    fn move_step(
        &mut self,
        queries: &mut QueryPipelineMut<'_>,
        shape: &dyn Shape,
        pose: Pose,
        dt: f32,
    ) -> Pose {
        let was_grounded = self.flags & DOWN != 0;
        self.hits.clear();
        self.flags = 0;
        self.support = 0;
        if was_grounded || self.mid_air_control || self.gravity == Vector::ZERO {
            self.airborne_movement = self.movement;
        }
        self.gravity_velocity += self.gravity * dt;
        let desired = (self.airborne_movement + self.gravity_velocity) * dt;
        if !desired.is_finite() {
            self.gravity_velocity = Vector::ZERO;
            return pose;
        }
        let mut stair_landing = None;
        let mut next = {
            let queries = queries.as_ref();
            // KCC uses its dt for last-step platform velocity. Carry is computed
            // below from the actual upcoming kinematic transform instead.
            let moved = self
                .controller
                .move_shape(0.0, &queries, shape, &pose, desired, |hit| {
                    self.hits.push(hit);
                });
            let mut next = pose.append_translation(moved.translation);
            if was_grounded
                && desired.y <= 0.0
                && let Some((stepped, landing)) =
                    self.try_stair(&queries, shape, pose, desired, moved.translation)
            {
                next = stepped;
                self.hits.clear();
                stair_landing = landing;
            }
            if desired.y <= 0.0
                && let Some(support) = self.find_support(&queries, shape, &next)
            {
                self.flags |= DOWN;
                self.support = support.user_data as i64;
                if let Some(handle) = support.parent()
                    && let Some(platform) = queries
                        .bodies
                        .get(handle)
                        .filter(|body| body.is_kinematic())
                {
                    let offset = platform
                        .position()
                        .inverse_transform_point(next.translation);
                    let carry = platform.next_position().transform_point(offset) - next.translation;
                    if carry.is_finite() && carry.length_squared() > 1e-10 {
                        let exclude_platform =
                            |_, collider: &Collider| collider.parent() != Some(handle);
                        let rider_filter = QueryFilter {
                            predicate: Some(&exclude_platform),
                            ..queries.filter
                        };
                        let rider_queries = QueryPipeline {
                            filter: rider_filter,
                            ..queries
                        };
                        let carry_controller = KinematicCharacterController {
                            autostep: None,
                            snap_to_ground: None,
                            ..self.controller
                        };
                        let adjusted = carry_controller.move_shape(
                            0.0,
                            &rider_queries,
                            shape,
                            &next,
                            carry,
                            |hit| {
                                self.hits.push(hit);
                            },
                        );
                        next = next.append_translation(adjusted.translation);
                        if carry.y < 0.0 && adjusted.translation.y - carry.y > self.skin {
                            self.flags &= !DOWN;
                            self.support = 0;
                        }
                    }
                }
            }
            next
        };
        let floor = self.controller.max_slope_climb_angle.cos();
        for hit in &self.hits {
            let normal = hit.hit.normal1;
            if normal.y < -0.5 {
                self.flags |= UP;
            } else if normal.y < floor {
                self.flags |= SIDE;
            }
            let into = self.gravity_velocity.dot(normal);
            if into < 0.0 {
                self.gravity_velocity -= normal * into;
            }
        }
        if self.flags & DOWN != 0 && self.gravity_velocity.y < 0.0 {
            self.gravity_velocity.y = 0.0;
        }
        let solve_impulses = self.push_mass > 0.0
            && self.hits.iter().any(|hit| {
                queries
                    .colliders
                    .get(hit.handle)
                    .and_then(|collider| collider.parent())
                    .and_then(|handle| queries.bodies.get(handle))
                    .is_some_and(|body| body.is_dynamic())
            });
        if self.report_hits {
            self.report_movement(queries, shape, dt, stair_landing, solve_impulses);
        } else if solve_impulses {
            self.controller.solve_character_collision_impulses(
                dt,
                queries,
                shape,
                self.push_mass,
                &self.hits,
            );
        }
        if !next.translation.is_finite() {
            next.translation = pose.translation;
        }
        next
    }

    fn report_movement(
        &mut self,
        queries: &mut QueryPipelineMut<'_>,
        shape: &dyn Shape,
        dt: f32,
        stair_landing: Option<(ColliderHandle, ShapeCastHit)>,
        solve_impulses: bool,
    ) {
        if let Some((handle, hit)) = stair_landing
            && let Some(collider) = queries.colliders.get(handle)
            && let Some(report) = shape_hit_report(collider.user_data as i64, hit, Vector::ZERO)
        {
            self.reports.push(report);
        }

        for hit in &self.hits {
            let target = queries
                .colliders
                .get(hit.handle)
                .map(|collider| (collider.user_data as i64, collider.parent()));
            let momentum = |queries: &QueryPipelineMut<'_>| {
                let body = queries.bodies.get(target?.1?)?;
                body.is_dynamic().then(|| (body.linvel(), body.mass()))
            };
            let before = if solve_impulses {
                momentum(queries)
            } else {
                None
            };
            if solve_impulses {
                // Keep Rapier's per-hit solve order. Measure the struck body's
                // momentum around just this solve, before the rigid-body step.
                self.controller.solve_character_collision_impulses(
                    dt,
                    queries,
                    shape,
                    self.push_mass,
                    std::iter::once(hit),
                );
            }
            let impulse = before
                .and_then(|(before, mass)| {
                    momentum(queries).map(|(after, _)| (before - after) * mass)
                })
                .unwrap_or(Vector::ZERO);
            if let Some((body, _)) = target
                && let Some(point) = shape_hit_report(body, hit.hit, impulse)
            {
                self.reports.push(point);
            }
        }
    }

    fn try_stair(
        &self,
        queries: &QueryPipeline<'_>,
        shape: &dyn Shape,
        pose: Pose,
        desired: Vector,
        moved: Vector,
    ) -> Option<(Pose, Option<(ColliderHandle, ShapeCastHit)>)> {
        let autostep = self.controller.autostep?;
        let CharacterLength::Absolute(height) = autostep.max_height else {
            return None;
        };
        let horizontal = Vector::new(desired.x, 0.0, desired.z);
        let direction = horizontal.try_normalize()?;
        let distance = horizontal.length();
        if distance - moved.dot(direction) <= self.skin {
            return None;
        }
        let floor = self.controller.max_slope_climb_angle.cos();
        if !self.hits.iter().any(|hit| {
            hit.hit.normal1.y < floor
                && queries
                    .colliders
                    .get(hit.handle)
                    .and_then(|collider| collider.parent())
                    .and_then(|handle| queries.bodies.get(handle))
                    .is_some_and(|body| !body.is_dynamic())
        }) {
            return None;
        }
        let climb_queries = QueryPipeline {
            filter: QueryFilter {
                flags: queries.filter.flags | QueryFilterFlags::EXCLUDE_DYNAMIC,
                ..queries.filter
            },
            ..*queries
        };
        let cast = |queries: &QueryPipeline<'_>, from: &Pose, dir, max, target| {
            queries.cast_shape(
                from,
                dir,
                shape,
                ShapeCastOptions {
                    max_time_of_impact: max,
                    target_distance: target,
                    stop_at_penetration: false,
                    compute_impact_geometry_on_penetration: true,
                },
            )
        };
        // Use the current footprint for clearance, and look far enough ahead
        // to test the walkable top beyond the capsule's rounded leading edge.
        let lift = height + self.skin;
        if cast(queries, &pose, Vector::Y, lift, self.skin * 0.25).is_some() {
            return None;
        }
        let lifted = pose.append_translation(Vector::Y * lift);
        if cast(queries, &lifted, direction, distance, self.skin).is_some() {
            return None;
        }
        let forward = lifted.append_translation(horizontal);
        let lookahead = cast(queries, &forward, direction, self.min_step_width, self.skin)
            .map_or(self.min_step_width, |(_, hit)| hit.time_of_impact);
        let probe = forward.append_translation(direction * lookahead);
        let (_, landing) = cast(&climb_queries, &probe, -Vector::Y, lift, self.skin)?;
        let rise = lift - landing.time_of_impact;
        if rise <= self.skin || landing.normal1.y < floor {
            return None;
        }
        let actual_landing = cast(
            queries,
            &forward,
            -Vector::Y,
            landing.time_of_impact,
            self.skin,
        );
        let lower = actual_landing.map_or(landing.time_of_impact, |(_, hit)| hit.time_of_impact);
        Some((
            forward.append_translation(-Vector::Y * lower),
            actual_landing,
        ))
    }

    fn find_support<'a>(
        &self,
        queries: &QueryPipeline<'a>,
        shape: &dyn Shape,
        pose: &Pose,
    ) -> Option<&'a Collider> {
        let floor = self.controller.max_slope_climb_angle.cos();
        let reach = self.skin * 2.0 + 0.01;
        if let Some((handle, hit)) = queries.cast_shape(
            pose,
            -Vector::Y,
            shape,
            ShapeCastOptions {
                max_time_of_impact: reach,
                target_distance: self.skin,
                stop_at_penetration: false,
                compute_impact_geometry_on_penetration: true,
            },
        ) && hit.normal1.y + 1e-4 >= floor
        {
            return queries.colliders.get(handle);
        }
        // A tangent wall can win a near-zero shape hit. A short ray at the
        // capsule's foot checks the floor with no side extent in that case.
        let foot = pose.transform_point(Vector::Y * shape.compute_local_aabb().mins.y);
        let ray = Ray::new(foot + Vector::Y * reach, -Vector::Y);
        let (handle, hit) = queries.cast_ray_and_get_normal(&ray, reach * 2.0, false)?;
        if hit.normal.y + 1e-4 < floor {
            return None;
        }
        queries.colliders.get(handle)
    }
}

fn autostep(height: f32, width: f32) -> Option<CharacterAutostep> {
    (height > 0.0).then_some(CharacterAutostep {
        max_height: CharacterLength::Absolute(height),
        min_width: CharacterLength::Absolute(width),
        include_dynamic_bodies: false,
    })
}

fn shape_hit_report(body: i64, hit: ShapeCastHit, impulse: Vector) -> Option<PhysicsContact3D> {
    let position = hit.witness1;
    let normal = hit.normal1;
    if body <= 0 || !position.is_finite() || !normal.is_finite() || !impulse.is_finite() {
        return None;
    }
    Some(PhysicsContact3D {
        body,
        position: Point3D::new(position.x.into(), position.y.into(), position.z.into()),
        normal: as_vector(normal),
        impulse: as_vector(impulse),
        normal_impulse: f64::from(impulse.dot(normal).max(0.0)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> PhysicsWorld3D {
        PhysicsWorld3D::new(Vector3D::new(0.0, -9.81, 0.0), 0.016)
    }

    fn ground(world: &PhysicsWorld3D) -> i64 {
        world.add_box(
            Point3D::new(0.0, -0.1, 0.0),
            Vector3D::new(20.0, 0.2, 12.0),
            0.0,
            0.0,
            0.0,
        )
    }

    #[test]
    fn shape_hits_are_opt_in_world_space_and_clear_after_removal() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.016);
        let wall = world.add_box(
            Point3D::new(0.25, 0.7, 0.0),
            Vector3D::new(0.2, 2.0, 2.0),
            0.0,
            0.0,
            0.0,
        );
        let player = world.add_character(Point3D::new(-1.0, 0.7, 0.0), 0.28, 0.84);
        assert!(world.set_character_movement(player, Vector3D::new(2.0, 0.0, 0.0)));
        for _ in 0..35 {
            assert!(world.step());
        }
        assert!(world.character_hit_wall(player));
        assert!(world.character_hits(player).is_empty());
        assert!(world.set_character_hit_reports(player, true));
        assert!(world.step());
        let hits = world.character_hits(player);
        assert!(!hits.is_empty());
        for hit in hits {
            assert_eq!(hit.body, wall);
            assert!((hit.position.x - 0.15).abs() < 0.005, "{hit:?}");
            assert!(hit.normal.x < -0.99);
            assert_eq!(hit.normal_impulse, 0.0);
        }
        assert!(world.remove(wall));
        assert!(world.character_hits(player).is_empty());
    }

    #[test]
    fn reports_measure_push_momentum_without_changing_the_trajectory() {
        let setup = |reporting| {
            let world = PhysicsWorld3D::new(Vector3D::default(), 0.016);
            let crate_body = world.add_box(
                Point3D::new(0.0, 0.7, 0.0),
                Vector3D::new(0.55, 1.15, 0.8),
                0.45,
                0.0,
                0.0,
            );
            let player = world.add_character(Point3D::new(-1.15, 0.7, 0.0), 0.28, 0.84);
            assert!(world.set_character_options(player, 0.0, 0.0, 45.0, 4.0));
            assert!(world.set_character_hit_reports(player, reporting));
            assert!(world.set_character_movement(player, Vector3D::new(2.0, 0.0, 0.0)));
            (world, player, crate_body)
        };
        let (recorded, player, target) = setup(true);
        let (quiet, quiet_player, quiet_target) = setup(false);
        let mut measured = false;
        for _ in 0..48 {
            let speed = recorded.velocity(target).x;
            assert!(recorded.step());
            assert!(quiet.step());
            assert_eq!(recorded.position(player), quiet.position(quiet_player));
            assert_eq!(recorded.velocity(target), quiet.velocity(quiet_target));
            assert!(quiet.character_hits(quiet_player).is_empty());
            let impulse = -recorded
                .character_hits(player)
                .iter()
                .filter(|hit| hit.body == target)
                .map(|hit| hit.impulse.x)
                .sum::<f64>();
            if impulse > 0.01 {
                let velocity_delta = recorded.velocity(target).x - speed;
                assert!((impulse - velocity_delta * 0.45).abs() < 1e-4);
                measured = true;
            }
        }
        assert!(measured);
        assert!(recorded.set_character_hit_reports(player, false));
        assert!(recorded.character_hits(player).is_empty());
    }

    #[test]
    fn capsule_walks_up_a_step_and_is_stopped_by_a_wall() {
        let world = world();
        let floor = ground(&world);
        let stair = world.add_box(
            Point3D::new(0.6, 0.11, 0.0),
            Vector3D::new(1.2, 0.22, 2.0),
            0.0,
            0.2,
            0.0,
        );
        world.add_box(
            Point3D::new(2.1, 1.0, 0.0),
            Vector3D::new(0.2, 2.0, 3.0),
            0.0,
            0.2,
            0.0,
        );
        let player = world.add_character(Point3D::new(-1.4, 0.706, 0.0), 0.28, 0.84);
        assert!(player > 0 && world.step());
        assert_eq!(world.character_support(player), floor);
        assert!(world.set_character_options(player, 0.32, 0.25, 45.0, 1.0));
        assert!(world.set_character_movement(player, Vector3D::new(1.6, 0.0, 0.0)));
        for _ in 0..80 {
            assert!(world.step());
        }
        let on_step = world.position(player);
        assert!(
            on_step.x > 0.28 && (on_step.y - 0.926).abs() < 0.025,
            "{on_step:?}"
        );
        assert_eq!(world.character_support(player), stair);
        for _ in 0..85 {
            assert!(world.step());
        }
        let stopped = world.position(player);
        assert!((stopped.x - 1.714).abs() < 0.05, "{stopped:?}");
        assert_eq!(world.character_support(player), floor);
        assert_ne!(world.character_collisions(player) & SIDE, 0);
    }

    #[test]
    fn platform_carry_uses_the_upcoming_transform_and_does_not_drift_after_stop() {
        let world = world();
        let zero = Vector3D::default();
        let platform = world.add_kinematic_box(
            Point3D::new(0.0, 0.15, 0.0),
            Vector3D::new(3.0, 0.3, 3.0),
            zero,
            0.5,
            0.0,
        );
        let player = world.add_character(Point3D::new(0.0, 1.006, 0.0), 0.28, 0.84);
        assert!(world.step());
        assert_eq!(world.character_support(player), platform);
        let initial = world.position(player);
        assert!(!world.set_kinematic_target(player, Point3D::default(), zero));
        assert!(world.set_kinematic_target(platform, Point3D::new(0.5, 0.32, -0.25), zero));
        assert!(world.step());
        let lifted = world.position(player);
        assert!((lifted.x - initial.x - 0.5).abs() < 0.01, "{lifted:?}");
        assert!((lifted.y - initial.y - 0.17).abs() < 0.01, "{lifted:?}");
        assert!((lifted.z - initial.z + 0.25).abs() < 0.01);
        assert_eq!(world.character_support(player), platform);
        assert!(world.step());
        let stopped = world.position(player);
        assert!((stopped.x - lifted.x).abs() < 1e-5);
        assert!((stopped.y - lifted.y).abs() < 0.01);
        assert!(world.set_kinematic_target(platform, Point3D::new(0.5, 0.13, -0.25), zero));
        assert!(world.step());
        assert!((world.position(player).y - stopped.y + 0.19).abs() < 0.01);
        assert!(world.remove(platform));
        assert!(!world.character_grounded(player));
        assert_eq!(world.character_support(player), 0);
    }

    #[test]
    fn jump_clears_support_and_mid_air_control_can_hold_the_departure_velocity() {
        let world = world();
        ground(&world);
        let player = world.add_character(Point3D::new(0.0, 0.706, 0.0), 0.28, 0.84);
        assert!(world.step());
        assert!(world.character_grounded(player));
        assert!(world.set_character_mid_air_control(player, false));
        assert!(world.set_character_movement(player, Vector3D::new(1.5, 0.0, 0.0)));
        assert!(world.jump_character(player, 3.2));
        assert!(!world.jump_character(player, 3.2));
        assert!(world.set_character_movement(player, Vector3D::new(-1.5, 0.0, 0.0)));
        for _ in 0..12 {
            assert!(world.step());
        }
        let airborne = world.position(player);
        assert!(airborne.x > 0.25 && airborne.y > 1.05, "{airborne:?}");
        assert!(!world.character_grounded(player));
        assert!(world.teleport_character(player, Point3D::new(3.0, 0.706, 0.0)));
        assert!(world.step());
        assert!(world.character_grounded(player));
    }

    #[test]
    fn jump_reports_the_ceiling_and_does_not_cross_it() {
        let world = world();
        ground(&world);
        world.add_box(
            Point3D::new(0.0, 1.65, 0.0),
            Vector3D::new(3.0, 0.2, 3.0),
            0.0,
            0.0,
            0.0,
        );
        let player = world.add_character(Point3D::new(0.0, 0.706, 0.0), 0.28, 0.84);
        assert!(world.step());
        assert!(world.jump_character(player, 3.2));
        let mut hit_ceiling = false;
        for _ in 0..12 {
            assert!(world.step());
            hit_ceiling |= world.character_hit_ceiling(player);
            assert!(world.position(player).y + 0.7 <= 1.552);
        }
        assert!(hit_ceiling);
    }

    #[test]
    fn character_pushes_dynamic_bodies_and_passes_through_sensors() {
        let world = world();
        ground(&world);
        let crate_body = world.add_box(
            Point3D::new(-0.05, 0.26, 0.0),
            Vector3D::new(0.5, 0.52, 0.6),
            0.45,
            0.0,
            0.0,
        );
        let trigger = world.add_box_trigger(
            Point3D::new(1.05, 0.9, 0.0),
            Vector3D::new(0.8, 1.4, 1.5),
            Vector3D::default(),
        );
        let player = world.add_character(Point3D::new(-1.1, 0.706, 0.0), 0.28, 0.84);
        assert!(world.set_character_options(player, 0.25, 0.18, 45.0, 4.0));
        assert!(world.set_character_movement(player, Vector3D::new(2.0, 0.0, 0.0)));
        let mut entered = false;
        for _ in 0..88 {
            assert!(world.step());
            entered |= world.trigger_entered(trigger).contains(&player);
        }
        assert!(
            world.position(crate_body).x > 0.6,
            "{:?}",
            world.position(crate_body)
        );
        assert!(
            world.position(player).x > 0.7 && entered,
            "{:?}",
            world.position(player)
        );
    }
}
