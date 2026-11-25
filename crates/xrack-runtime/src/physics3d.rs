//! A persistent, explicitly stepped 3D rigid-body simulation.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fmt,
    rc::Rc,
    sync::{Mutex, PoisonError},
};

use nalgebra::{Quaternion, UnitQuaternion};
use rapier3d::{
    parry::partitioning::Bvh,
    prelude::{
        ActiveCollisionTypes, ActiveEvents, ActiveHooks, ColliderBuilder, ColliderSet,
        CollisionEvent, Group, ImpulseJointHandle, InteractionGroups, InteractionTestMode,
        PhysicsWorld, Pose, RigidBody, RigidBodyBuilder, RigidBodyHandle, RigidBodySet, Rotation,
        Vector,
    },
};

use crate::{ChangeSignal, Point3D, Vector3D};

mod authored;
mod character;
mod clock;
mod contact_events;
mod contacts;
mod debug;
mod dynamics;
mod joints;
mod kinematic;
mod mass;
mod materials;
mod mesh_shapes;
mod query;
mod settings;
mod shapes;
use authored::{AuthoredBody, AuthoredJoint};
pub use authored::{PhysicsBodySpec, PhysicsJointSpec, PhysicsShapeSpec};
use character::Character;
use clock::PhysicsClock;
pub use contacts::PhysicsContact3D;
use contacts::{ContactReports, StepEvents};
use debug::PhysicsDebugCache;
pub use debug::{PhysicsDebugLines, PhysicsDebugVertex};
use dynamics::Load;
use joints::JointEntry;
use kinematic::KinematicMotion;
use materials::WorldHooks;
pub use query::{PhysicsHit3D, PhysicsQueryFilter3D};
pub use shapes::{PhysicsCollider3D, PhysicsShape3D};

/// A local, shared physics world. The default value has no simulation.
#[derive(Clone, Default)]
pub struct PhysicsWorld3D(Option<Rc<WorldState>>);

struct WorldState {
    state: RefCell<Simulation>,
    changed: ChangeSignal,
    clock_changed: ChangeSignal,
    settings_changed: ChangeSignal,
    authored_changed: ChangeSignal,
    authored_joints_changed: ChangeSignal,
    triggers_changed: ChangeSignal,
    contacts_changed: ChangeSignal,
    contact_events_changed: ChangeSignal,
    materials_changed: ChangeSignal,
    mass_changed: ChangeSignal,
    query_changed: ChangeSignal,
    character_hits_changed: ChangeSignal,
}

struct Simulation {
    world: PhysicsWorld,
    clock: PhysicsClock,
    pose_history: BTreeMap<i64, Pose>,
    debug: Option<Box<PhysicsDebugCache>>,
    bodies: BTreeMap<i64, RigidBodyHandle>,
    authored_bodies: BTreeMap<String, AuthoredBody>,
    authored_changed: bool,
    authored_joints: BTreeMap<String, AuthoredJoint>,
    authored_joints_changed: bool,
    joints: BTreeMap<i64, JointEntry>,
    bounded_distance_joints: BTreeMap<i64, ImpulseJointHandle>,
    triggers: Triggers,
    contacts: ContactReports,
    contact_events: contact_events::ContactEvents,
    static_friction: BTreeMap<i64, f32>,
    materials_changed: bool,
    default_density: f32,
    default_density_bodies: BTreeSet<i64>,
    mass_changed: bool,
    queued_loads: BTreeMap<i64, Load>,
    kinematic_motion: BTreeMap<i64, KinematicMotion>,
    characters: BTreeMap<i64, Character>,
    query_changed: bool,
    settings_changed: bool,
    query_bvh_dirty: bool,
    query_bvh: Option<Bvh>,
    character_hits_changed: bool,
    next_id: i64,
}

#[derive(Default)]
struct Triggers {
    states: BTreeMap<i64, TriggerState>,
    reported: Vec<i64>,
    removed_overlaps: Vec<(i64, i64)>,
    collector: SensorEvents,
    changed: bool,
}

#[derive(Default)]
struct TriggerState {
    occupants: BTreeMap<i64, usize>,
    entered: Vec<i64>,
    exited: Vec<i64>,
}

struct SensorEvent {
    trigger: i64,
    body: i64,
    entered: bool,
    defer_exit: bool,
}

#[derive(Default)]
struct SensorEvents(Mutex<Vec<SensorEvent>>);

impl fmt::Debug for PhysicsWorld3D {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PhysicsWorld3D")
            .field("initialized", &self.0.is_some())
            .finish()
    }
}

impl PartialEq for PhysicsWorld3D {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(left), Some(right)) => Rc::ptr_eq(left, right),
            (None, None) => true,
            _ => false,
        }
    }
}

impl PhysicsWorld3D {
    /// Create a world with gravity in world units per second squared and a step in seconds.
    pub fn new(gravity: Vector3D, timestep: f64) -> Self {
        let solver_timestep = timestep as f32;
        let Some(gravity) = vector([gravity.x, gravity.y, gravity.z]) else {
            return Self::default();
        };
        if !(solver_timestep.is_finite()
            && solver_timestep > 0.0
            && solver_timestep.recip().is_finite())
        {
            return Self::default();
        }
        let mut world = PhysicsWorld::new();
        world.gravity = gravity;
        world.integration_parameters.dt = solver_timestep;
        world.integration_parameters.min_ccd_dt = solver_timestep * 0.01;
        Self(Some(Rc::new(WorldState {
            state: RefCell::new(Simulation {
                world,
                clock: PhysicsClock::new(timestep),
                pose_history: BTreeMap::new(),
                debug: None,
                bodies: BTreeMap::new(),
                authored_bodies: BTreeMap::new(),
                authored_changed: false,
                authored_joints: BTreeMap::new(),
                authored_joints_changed: false,
                joints: BTreeMap::new(),
                bounded_distance_joints: BTreeMap::new(),
                triggers: Triggers::default(),
                contacts: ContactReports::default(),
                contact_events: contact_events::ContactEvents::default(),
                static_friction: BTreeMap::new(),
                materials_changed: false,
                default_density: 1.0,
                default_density_bodies: BTreeSet::new(),
                mass_changed: false,
                queued_loads: BTreeMap::new(),
                kinematic_motion: BTreeMap::new(),
                characters: BTreeMap::new(),
                query_changed: false,
                settings_changed: false,
                query_bvh_dirty: false,
                query_bvh: None,
                character_hits_changed: false,
                next_id: 1,
            }),
            changed: ChangeSignal::default(),
            clock_changed: ChangeSignal::default(),
            settings_changed: ChangeSignal::default(),
            authored_changed: ChangeSignal::default(),
            authored_joints_changed: ChangeSignal::default(),
            triggers_changed: ChangeSignal::default(),
            contacts_changed: ChangeSignal::default(),
            contact_events_changed: ChangeSignal::default(),
            materials_changed: ChangeSignal::default(),
            mass_changed: ChangeSignal::default(),
            query_changed: ChangeSignal::default(),
            character_hits_changed: ChangeSignal::default(),
        })))
    }

    /// Add an axis-aligned box with full extents. A mass of zero creates a fixed body.
    pub fn add_box(
        &self,
        center: Point3D,
        size: Vector3D,
        mass: f64,
        friction: f64,
        restitution: f64,
    ) -> i64 {
        let Some(collider) = box_collider(size) else {
            return 0;
        };
        self.add_mass_body(center, collider, mass, friction, restitution)
    }

    /// Add a sphere. A mass of zero creates a fixed body.
    pub fn add_sphere(
        &self,
        center: Point3D,
        radius: f64,
        mass: f64,
        friction: f64,
        restitution: f64,
    ) -> i64 {
        let radius = radius as f32;
        if !(radius.is_finite() && radius > 0.0) {
            return 0;
        }
        self.add_mass_body(
            center,
            ColliderBuilder::ball(radius),
            mass,
            friction,
            restitution,
        )
    }

    /// Add a box driven by position targets, with an initial pose in scene coordinates.
    pub fn add_kinematic_box(
        &self,
        center: Point3D,
        size: Vector3D,
        rotation: Vector3D,
        friction: f64,
        restitution: f64,
    ) -> i64 {
        let (Some(collider), Some(pose)) = (box_collider(size), scene_pose(center, rotation))
        else {
            return 0;
        };
        let Some(collider) = collider_material(collider, friction, restitution) else {
            return 0;
        };
        self.insert(
            RigidBodyBuilder::kinematic_position_based()
                .ccd_enabled(true)
                .pose(pose),
            collider,
        )
    }

    /// Add a stationary box sensor with full extents. It detects overlaps without contact forces.
    pub fn add_box_trigger(&self, center: Point3D, size: Vector3D, rotation: Vector3D) -> i64 {
        let (Some(collider), Some(pose)) = (box_collider(size), scene_pose(center, rotation))
        else {
            return 0;
        };
        self.insert_trigger(pose, collider)
    }

    fn insert_trigger(&self, pose: Pose, collider: ColliderBuilder) -> i64 {
        self.insert(
            RigidBodyBuilder::fixed().pose(pose),
            sensor_collider(collider),
        )
    }

    pub fn trigger_count(&self, trigger: i64) -> i64 {
        self.read_trigger(trigger, |trigger| {
            i64::try_from(trigger.occupants.len()).unwrap_or(i64::MAX)
        })
    }

    pub fn trigger_overlaps(&self, trigger: i64, body: i64) -> bool {
        self.read_trigger(trigger, |trigger| trigger.occupants.contains_key(&body))
    }

    /// Return the body handles that entered during the latest completed step or batch.
    pub fn trigger_entered(&self, trigger: i64) -> Vec<i64> {
        self.read_trigger(trigger, |trigger| trigger.entered.clone())
    }

    /// Return the body handles that exited during the latest completed step or batch.
    pub fn trigger_exited(&self, trigger: i64) -> Vec<i64> {
        self.read_trigger(trigger, |trigger| trigger.exited.clone())
    }

    /// Set the group's index and ignored mask. Queries see the change immediately;
    /// collision pairs are updated by the next simulation step.
    pub fn set_collision_filter(&self, body: i64, group: i64, ignore_groups: i64) -> bool {
        let (Ok(group), Ok(ignored)) = (u32::try_from(group), u32::try_from(ignore_groups)) else {
            return false;
        };
        let Some(group_bit) = 1_u32.checked_shl(group) else {
            return false;
        };
        let groups = InteractionGroups::new(
            Group::from_bits_retain(group_bit),
            Group::from_bits_retain(!ignored),
            InteractionTestMode::And,
        );
        self.update(|simulation| {
            let handle = *simulation.bodies.get(&body)?;
            let body = simulation.world.bodies.get(handle)?;
            for &handle in body.colliders() {
                if let Some(collider) = simulation.world.colliders.get_mut(handle)
                    && collider.collision_groups() != groups
                {
                    collider.set_collision_groups(groups);
                    simulation.query_changed = true;
                }
            }
            Some(())
        })
        .is_some()
    }

    fn add_mass_body(
        &self,
        center: Point3D,
        collider: ColliderBuilder,
        mass: f64,
        friction: f64,
        restitution: f64,
    ) -> i64 {
        let Some(center) = vector([center.x, center.y, center.z]) else {
            return 0;
        };
        self.insert_mass_body(
            Pose::from_parts(center, Rotation::IDENTITY),
            collider,
            mass,
            friction,
            restitution,
        )
    }

    fn insert_mass_body(
        &self,
        pose: Pose,
        collider: ColliderBuilder,
        mass: f64,
        friction: f64,
        restitution: f64,
    ) -> i64 {
        let mass = mass as f32;
        if !(mass.is_finite() && mass >= 0.0) {
            return 0;
        }
        let (body, collider) = if mass > 0.0 {
            (
                RigidBodyBuilder::dynamic().ccd_enabled(true),
                collider.mass(mass),
            )
        } else {
            (RigidBodyBuilder::fixed(), collider)
        };
        let Some(collider) = collider_material(collider, friction, restitution) else {
            return 0;
        };
        self.insert(body.pose(pose), collider)
    }

    fn insert(&self, body: RigidBodyBuilder, collider: ColliderBuilder) -> i64 {
        self.insert_colliders(body, std::iter::once(collider), None)
    }

    fn insert_colliders(
        &self,
        body: RigidBodyBuilder,
        colliders: impl IntoIterator<Item = ColliderBuilder>,
        character: Option<Character>,
    ) -> i64 {
        self.update(|simulation| simulation.insert_physics_body(body, colliders, character))
            .unwrap_or(0)
    }

    pub fn remove(&self, body: i64) -> bool {
        self.update(|simulation| {
            simulation.remove_physics_body(body)?;
            let before = simulation.authored_bodies.len();
            simulation
                .authored_bodies
                .retain(|_, entry| entry.handle != body);
            simulation.authored_changed |= simulation.authored_bodies.len() != before;
            Some(())
        })
        .is_some()
    }

    pub fn clear(&self) -> bool {
        self.update(|simulation| {
            let mut world = PhysicsWorld::new();
            world.gravity = simulation.world.gravity;
            world.integration_parameters = simulation.world.integration_parameters;
            simulation.world = world;
            simulation.clock.clear();
            simulation.pose_history.clear();
            simulation.materials_changed |= !simulation.bodies.is_empty();
            simulation.mass_changed |= !simulation.bodies.is_empty();
            simulation.settings_changed |= !simulation.bodies.is_empty();
            simulation.default_density_bodies.clear();
            simulation.bodies.clear();
            simulation.authored_changed |= !simulation.authored_bodies.is_empty();
            simulation.authored_bodies.clear();
            simulation.static_friction.clear();
            simulation.queued_loads.clear();
            simulation.kinematic_motion.clear();
            simulation.clear_characters();
            simulation.use_stepped_query_bvh();
            simulation.authored_joints_changed |= !simulation.authored_joints.is_empty();
            simulation.authored_joints.clear();
            simulation.joints.clear();
            simulation.bounded_distance_joints.clear();
            simulation.triggers = Triggers {
                changed: !simulation.triggers.states.is_empty(),
                ..Triggers::default()
            };
            simulation.contacts.clear();
            simulation.contact_events.clear();
            Some(())
        })
        .is_some()
    }

    pub fn position(&self, body: i64) -> Point3D {
        self.read(body, |body| {
            let value = body.translation();
            Point3D::new(value.x.into(), value.y.into(), value.z.into())
        })
        .unwrap_or_default()
    }

    pub fn velocity(&self, body: i64) -> Vector3D {
        self.body_velocity(body)
            .map(|velocity| contacts::as_vector(velocity.linvel))
            .unwrap_or_default()
    }

    /// Return the yaw, pitch and roll in degrees, in SceneView3D field order.
    pub fn rotation(&self, body: i64) -> Vector3D {
        self.read(body, |body| rotation_angles(*body.rotation()))
            .unwrap_or_default()
    }

    fn update<T>(&self, operation: impl FnOnce(&mut Simulation) -> Option<T>) -> Option<T> {
        self.update_selectively(|simulation| Some((operation(simulation)?, true)))
    }

    fn update_selectively<T>(
        &self,
        operation: impl FnOnce(&mut Simulation) -> Option<(T, bool)>,
    ) -> Option<T> {
        let state = self.0.as_ref()?;
        let (
            result,
            world_changed,
            clock_changed,
            settings_changed,
            authored_changed,
            authored_joints_changed,
            triggers_changed,
            contacts_changed,
            contact_events_changed,
            character_hits_changed,
            query_changed,
            materials_changed,
            mass_changed,
        ) = {
            let mut simulation = state.state.try_borrow_mut().ok()?;
            let (result, world_changed) = operation(&mut simulation)?;
            if world_changed && let Some(cache) = &mut simulation.debug {
                cache.invalidate();
            }
            (
                result,
                world_changed,
                simulation.clock.take_changed(),
                std::mem::take(&mut simulation.settings_changed),
                std::mem::take(&mut simulation.authored_changed),
                std::mem::take(&mut simulation.authored_joints_changed),
                std::mem::take(&mut simulation.triggers.changed),
                simulation.contacts.take_changed(),
                simulation.contact_events.take_changed(),
                std::mem::take(&mut simulation.character_hits_changed),
                std::mem::take(&mut simulation.query_changed),
                std::mem::take(&mut simulation.materials_changed),
                std::mem::take(&mut simulation.mass_changed),
            )
        };
        if triggers_changed {
            state.triggers_changed.notify();
        }
        if contacts_changed {
            state.contacts_changed.notify();
        }
        if contact_events_changed {
            state.contact_events_changed.notify();
        }
        if character_hits_changed {
            state.character_hits_changed.notify();
        }
        if query_changed {
            state.query_changed.notify();
        }
        if materials_changed {
            state.materials_changed.notify();
        }
        if mass_changed {
            state.mass_changed.notify();
        }
        if authored_changed {
            state.authored_changed.notify();
        }
        if authored_joints_changed {
            state.authored_joints_changed.notify();
        }
        if settings_changed {
            state.settings_changed.notify();
        }
        if clock_changed {
            state.clock_changed.notify();
        }
        if world_changed {
            state.changed.notify();
        }
        Some(result)
    }

    fn read<T>(&self, body: i64, select: impl FnOnce(&RigidBody) -> T) -> Option<T> {
        let state = self.0.as_ref()?;
        state.changed.track();
        let simulation = state.state.try_borrow().ok()?;
        let handle = *simulation.bodies.get(&body)?;
        simulation.world.bodies.get(handle).map(select)
    }

    fn read_trigger<T: Default>(&self, trigger: i64, select: impl FnOnce(&TriggerState) -> T) -> T {
        let Some(state) = self.0.as_ref() else {
            return T::default();
        };
        state.triggers_changed.track();
        state
            .state
            .try_borrow()
            .ok()
            .and_then(|simulation| simulation.triggers.states.get(&trigger).map(select))
            .unwrap_or_default()
    }
}

impl Simulation {
    fn insert_physics_body(
        &mut self,
        body: RigidBodyBuilder,
        colliders: impl IntoIterator<Item = ColliderBuilder>,
        character: Option<Character>,
    ) -> Option<i64> {
        let mut colliders = colliders.into_iter();
        let first = colliders.next()?;
        let trigger = first.is_sensor;
        let groups = InteractionGroups::new(Group::GROUP_1, Group::ALL, InteractionTestMode::And);
        let id = self.next_id;
        self.next_id = id.checked_add(1)?;
        let (handle, _) = self
            .world
            .insert(body, first.collision_groups(groups).user_data(id as u128));
        for collider in colliders {
            self.world.insert_collider(
                collider.collision_groups(groups).user_data(id as u128),
                Some(handle),
            );
        }
        self.bodies.insert(id, handle);
        self.materials_changed |= !trigger;
        self.mass_changed |= !trigger;
        self.settings_changed = true;
        self.invalidate_query_bvh();
        if let Some(mut character) = character {
            character.gravity = self.world.gravity;
            self.characters.insert(id, character);
        }
        if trigger {
            self.triggers.states.insert(id, TriggerState::default());
        }
        Some(id)
    }

    fn remove_physics_body(&mut self, body: i64) -> Option<()> {
        let handle = *self.bodies.get(&body)?;
        let joints: Vec<_> = self
            .world
            .impulse_joints_with(handle)
            .map(|(_, _, _, joint)| joint.data.user_data as i64)
            .collect();
        self.world.remove_body(handle)?;
        for joint in joints {
            self.untrack_joint(joint);
        }
        self.bodies.remove(&body);
        self.settings_changed = true;
        self.pose_history.remove(&body);
        self.static_friction.remove(&body);
        self.default_density_bodies.remove(&body);
        self.mass_changed = true;
        self.materials_changed = true;
        self.forget_character_body(body);
        self.invalidate_query_bvh();
        self.queued_loads.remove(&body);
        self.kinematic_motion.remove(&body);
        self.triggers.remove(body);
        self.contacts.remove(body);
        self.contact_events.remove(body);
        Some(())
    }
}

fn sensor_collider(collider: ColliderBuilder) -> ColliderBuilder {
    collider
        .sensor(true)
        .active_events(ActiveEvents::COLLISION_EVENTS)
        .active_collision_types(ActiveCollisionTypes::all())
        .active_hooks(ActiveHooks::FILTER_INTERSECTION_PAIR)
}

impl Triggers {
    fn begin_batch(&mut self) {
        self.changed |= !self.reported.is_empty();
        for trigger in self.reported.drain(..) {
            if let Some(state) = self.states.get_mut(&trigger) {
                state.entered.clear();
                state.exited.clear();
            }
        }
    }

    fn step(
        &mut self,
        world: &mut PhysicsWorld,
        contacts: &ContactReports,
        static_friction: &BTreeMap<i64, f32>,
    ) {
        if self.states.is_empty() && !contacts.enabled() && static_friction.is_empty() {
            world.step();
            return;
        }

        for (trigger, body) in self.removed_overlaps.drain(..) {
            if let Some(state) = self.states.get_mut(&trigger) {
                state.report(trigger, body, false, &mut self.reported);
            }
        }
        world.step_with_events(
            &WorldHooks { static_friction },
            &StepEvents {
                sensors: &self.collector,
                contacts,
            },
        );
        let events = self
            .collector
            .0
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        // A body's existing overlap survives crossing a seam between attached
        // colliders, even when Rapier delivers the exit first in the same step.
        if events.iter().any(|event| event.defer_exit) {
            events.sort_by_key(|event| event.defer_exit);
        }
        for SensorEvent {
            trigger,
            body,
            entered,
            ..
        } in events.drain(..)
        {
            if let Some(state) = self.states.get_mut(&trigger) {
                if state.update_overlap(body, entered) {
                    state.report(trigger, body, entered, &mut self.reported);
                }
            }
        }
        self.changed |= !self.reported.is_empty();
    }

    fn remove(&mut self, body: i64) {
        if self.states.remove(&body).is_some() {
            self.reported.retain(|trigger| *trigger != body);
            self.removed_overlaps
                .retain(|(trigger, _)| *trigger != body);
            self.changed = true;
            return;
        }
        for (&trigger, state) in &mut self.states {
            if state.occupants.remove(&body).is_some() {
                self.removed_overlaps.push((trigger, body));
                self.changed = true;
            }
        }
    }
}

impl TriggerState {
    fn update_overlap(&mut self, body: i64, entered: bool) -> bool {
        use std::collections::btree_map::Entry;
        match (self.occupants.entry(body), entered) {
            (Entry::Vacant(entry), true) => {
                entry.insert(1);
                true
            }
            (Entry::Occupied(mut entry), true) => {
                *entry.get_mut() += 1;
                false
            }
            (Entry::Occupied(mut entry), false) => {
                let count = entry.get_mut();
                *count -= 1;
                if *count == 0 {
                    entry.remove();
                    true
                } else {
                    false
                }
            }
            (Entry::Vacant(_), false) => false,
        }
    }

    fn report(&mut self, trigger: i64, body: i64, entered: bool, reported: &mut Vec<i64>) {
        if self.entered.is_empty() && self.exited.is_empty() {
            reported.push(trigger);
        }
        if entered {
            self.entered.push(body);
        } else {
            self.exited.push(body);
        }
    }
}

impl SensorEvents {
    fn handle_collision_event(
        &self,
        bodies: &RigidBodySet,
        colliders: &ColliderSet,
        event: CollisionEvent,
    ) {
        if !event.sensor() || event.removed() {
            return;
        }
        let (Some(first), Some(second)) = (
            colliders.get(event.collider1()),
            colliders.get(event.collider2()),
        ) else {
            return;
        };
        let (trigger, body) = match (first.is_sensor(), second.is_sensor()) {
            (true, false) => (first, second),
            (false, true) => (second, first),
            _ => return,
        };
        let entered = event.started();
        let defer_exit = !entered
            && body
                .parent()
                .and_then(|handle| bodies.get(handle))
                .is_some_and(|parent| parent.colliders().len() > 1);
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(SensorEvent {
                trigger: trigger.user_data as i64,
                body: body.user_data as i64,
                entered,
                defer_exit,
            });
    }
}

impl Simulation {
    fn body_mut(&mut self, body: i64) -> Option<&mut RigidBody> {
        self.world.bodies.get_mut(*self.bodies.get(&body)?)
    }
}

fn box_collider(size: Vector3D) -> Option<ColliderBuilder> {
    let half = vector([size.x * 0.5, size.y * 0.5, size.z * 0.5])?;
    (half.min_element() > 0.0).then(|| ColliderBuilder::cuboid(half.x, half.y, half.z))
}

fn collider_material(
    collider: ColliderBuilder,
    friction: f64,
    restitution: f64,
) -> Option<ColliderBuilder> {
    let (friction, restitution) = (friction as f32, restitution as f32);
    (friction.is_finite() && friction >= 0.0 && (0.0..=1.0).contains(&restitution))
        .then(|| collider.friction(friction).restitution(restitution))
}

fn rotation_angles(q: Rotation) -> Vector3D {
    let rotation = UnitQuaternion::from_quaternion(Quaternion::new(
        f64::from(q.w),
        f64::from(q.x),
        f64::from(q.y),
        f64::from(q.z),
    ));
    let (pitch, yaw, roll) = rotation.euler_angles();
    Vector3D::new(yaw.to_degrees(), pitch.to_degrees(), roll.to_degrees())
}

fn scene_pose(center: Point3D, rotation: Vector3D) -> Option<Pose> {
    let center = vector([center.x, center.y, center.z])?;
    let angles = [rotation.x, rotation.y, rotation.z];
    if !angles.into_iter().all(f64::is_finite) {
        return None;
    }
    let angles = angles.map(|angle| angle.rem_euclid(360.0) as f32);
    let rotation =
        UnitQuaternion::from_rotation_matrix(&crate::scene_hierarchy::euler_rotation(angles));
    let q = rotation.quaternion();
    Some(Pose::from_parts(
        center,
        Rotation::from_xyzw(q.i, q.j, q.k, q.w),
    ))
}

fn vector(components: [f64; 3]) -> Option<Vector> {
    let [x, y, z] = components.map(|value| value as f32);
    let result = Vector::new(x, y, z);
    result.is_finite().then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_keeps_rigid_body_contacts_between_fixed_steps() {
        let world = PhysicsWorld3D::new(Vector3D::new(0.0, -9.81, 0.0), 1.0 / 60.0);
        let ground = world.add_box(
            Point3D::new(0.0, -0.1, 0.0),
            Vector3D::new(20.0, 0.2, 20.0),
            0.0,
            0.4,
            0.0,
        );
        let ball = world.add_sphere(Point3D::new(0.0, 3.0, 0.0), 0.5, 1.0, 0.4, 0.0);
        assert_ne!(ground, 0);
        assert_ne!(ball, 0);
        let before = world.position(ball);
        for _ in 0..120 {
            assert!(world.step());
        }
        let resting = world.position(ball);
        assert!(
            resting.y < before.y && (resting.y - 0.5).abs() < 0.025,
            "{resting:?}"
        );
        assert!(world.velocity(ball).y.abs() < 0.05);
        assert!(!world.apply_impulse(ground, Vector3D::new(5.0, 0.0, 0.0)));
        assert!(world.remove(ball));
        assert_eq!(world.position(ball), Point3D::default());
        assert!(world.clear());
        let next = world.add_sphere(before, 0.5, 1.0, 0.4, 0.0);
        assert!(next > ball);
    }

    #[test]
    fn impulses_transfer_between_dynamic_bodies() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 1.0 / 120.0);
        let first = world.add_sphere(Point3D::new(-2.0, 0.0, 0.0), 0.5, 1.0, 0.0, 0.5);
        let second = world.add_sphere(Point3D::new(0.0, 0.0, 0.0), 0.5, 1.0, 0.0, 0.5);
        assert!(world.apply_impulse(first, Vector3D::new(6.0, 0.0, 0.0)));
        for _ in 0..60 {
            assert!(world.step());
        }
        assert!(world.position(second).x > 1.0);
        assert!(world.velocity(second).x > world.velocity(first).x);
    }

    #[test]
    fn kinematic_targets_apply_on_step_and_lift_a_dynamic_body() {
        let world = PhysicsWorld3D::new(Vector3D::new(0.0, -9.81, 0.0), 1.0 / 60.0);
        let platform = world.add_kinematic_box(
            Point3D::default(),
            Vector3D::new(5.0, 0.25, 5.0),
            Vector3D::new(15.0, 0.0, 0.0),
            0.6,
            0.0,
        );
        let ball = world.add_sphere(Point3D::new(0.0, 0.625, 0.0), 0.5, 1.0, 0.6, 0.0);
        assert_ne!(platform, 0);
        assert!((world.rotation(platform).x - 15.0).abs() < 1e-4);
        let target_rotation = Vector3D::new(18.0, 1.0, -3.0);
        assert!(!world.apply_impulse(platform, Vector3D::new(10.0, 10.0, 10.0)));
        assert!(!world.set_kinematic_target(ball, Point3D::default(), target_rotation));
        assert!(world.set_kinematic_target(platform, Point3D::new(0.0, 3.0, 0.0), target_rotation));
        assert!(world.set_kinematic_target(platform, Point3D::default(), target_rotation));
        assert!(!world.set_kinematic_target(
            platform,
            Point3D::new(f64::NAN, 0.0, 0.0),
            target_rotation
        ));
        assert_eq!(world.position(platform), Point3D::default());
        assert!((world.rotation(platform).x - 15.0).abs() < 1e-4);
        assert!(world.step());
        assert_eq!(world.position(platform), Point3D::default());
        assert!((world.rotation(platform).x - target_rotation.x).abs() < 1e-4);
        assert!((world.rotation(platform).y - target_rotation.y).abs() < 1e-4);
        assert!((world.rotation(platform).z - target_rotation.z).abs() < 1e-4);
        for step in 1..=60 {
            assert!(world.set_kinematic_target(
                platform,
                Point3D::new(0.0, f64::from(step) / 60.0, 0.0),
                target_rotation,
            ));
            assert!(world.step());
        }
        assert_eq!(world.position(platform), Point3D::new(0.0, 1.0, 0.0));
        let ball_position = world.position(ball);
        assert!(ball_position.y > 1.5, "{ball_position:?}");
        assert!(world.remove(platform));
        assert!(!world.set_kinematic_target(platform, Point3D::default(), Vector3D::default()));
    }

    #[test]
    fn trigger_reports_follow_overlap_transitions_and_body_removal() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.1);
        let trigger = world.add_box_trigger(
            Point3D::default(),
            Vector3D::new(2.0, 2.0, 2.0),
            Vector3D::default(),
        );
        let ball = world.add_sphere(Point3D::default(), 0.25, 1.0, 0.0, 0.0);
        let other_trigger = world.add_box_trigger(
            Point3D::default(),
            Vector3D::new(3.0, 3.0, 3.0),
            Vector3D::default(),
        );
        assert!(trigger > 0 && ball > 0 && other_trigger > 0);
        let occupancy = crate::Property::default();
        let count = || occupancy.get(|| world.trigger_count(trigger));
        assert_eq!(count(), 0);

        assert!(world.step());
        assert_eq!(world.trigger_entered(trigger), [ball]);
        assert_eq!(count(), 1);
        assert!(world.trigger_overlaps(trigger, ball));
        assert!(!world.trigger_overlaps(trigger, other_trigger));
        assert!(world.step());
        assert!(world.trigger_entered(trigger).is_empty());
        assert_eq!(count(), 1);
        assert!(world.step());
        assert!(!occupancy.is_dirty());

        assert!(world.remove(ball));
        assert_eq!(count(), 0);
        assert!(world.trigger_exited(trigger).is_empty());
        assert!(world.step());
        assert_eq!(world.trigger_exited(trigger), [ball]);
        assert_eq!(world.trigger_exited(other_trigger), [ball]);
        assert!(world.step());
        assert!(world.trigger_exited(trigger).is_empty());
        assert!(world.clear());
        assert_eq!(world.trigger_count(trigger), 0);
        assert!(world.trigger_entered(other_trigger).is_empty());
    }

    #[test]
    fn fast_body_crosses_a_trigger_without_losing_either_event_or_momentum() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.1);
        let trigger = world.add_box_trigger(
            Point3D::default(),
            Vector3D::new(0.5, 2.0, 2.0),
            Vector3D::default(),
        );
        let ball = world.add_sphere(Point3D::new(-4.0, 0.0, 0.0), 0.25, 1.0, 0.0, 0.0);
        assert!(trigger > 0 && ball > 0);
        assert!(world.apply_impulse(ball, Vector3D::new(60.0, 0.0, 0.0)));
        assert!(world.step());
        assert!(world.position(ball).x > 1.0);
        assert!((world.velocity(ball).x - 60.0).abs() < 1e-3);
        assert_eq!(world.trigger_entered(trigger), [ball]);
        assert_eq!(world.trigger_exited(trigger), [ball]);
        assert_eq!(world.trigger_count(trigger), 0);
    }

    #[test]
    fn refiltering_an_existing_overlap_reports_transitions_on_the_next_step() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.016);
        let trigger = world.add_box_trigger(
            Point3D::default(),
            Vector3D::new(2.0, 2.0, 2.0),
            Vector3D::default(),
        );
        let ball = world.add_sphere(Point3D::default(), 0.25, 1.0, 0.0, 0.0);
        assert!(world.set_collision_filter(ball, 31, 0));
        assert!(world.step());
        assert_eq!(world.trigger_entered(trigger), [ball]);
        assert!(world.trigger_overlaps(trigger, ball));

        assert!(world.set_collision_filter(trigger, 2, 1_i64 << 31));
        assert!(!world.set_collision_filter(trigger, 32, 0));
        assert!(!world.set_collision_filter(trigger, 2, -1));
        assert!(!world.set_collision_filter(trigger, 2, 1_i64 << 32));
        assert!(!world.set_collision_filter(0, 2, 0));
        assert!(world.trigger_overlaps(trigger, ball));
        assert!(world.step());
        assert_eq!(world.trigger_exited(trigger), [ball]);
        assert!(!world.trigger_overlaps(trigger, ball));

        assert!(world.set_collision_filter(trigger, 2, 0));
        assert!(world.set_collision_filter(ball, 31, 1 << 2));
        assert!(world.step());
        assert!(!world.trigger_overlaps(trigger, ball));
        assert!(world.trigger_entered(trigger).is_empty());

        assert!(world.set_collision_filter(ball, 31, 0));
        assert!(world.step());
        assert_eq!(world.trigger_entered(trigger), [ball]);
        assert!(world.trigger_overlaps(trigger, ball));
        assert!(world.step());
        assert!(world.trigger_entered(trigger).is_empty());
    }

    #[test]
    fn collision_filter_applies_to_solid_ccd_contacts() {
        let simulate = |ignore_groups| {
            let world = PhysicsWorld3D::new(Vector3D::default(), 0.1);
            let wall = world.add_box(
                Point3D::default(),
                Vector3D::new(0.3, 2.0, 2.0),
                0.0,
                0.0,
                0.0,
            );
            let ball = world.add_sphere(Point3D::new(-4.0, 0.0, 0.0), 0.25, 1.0, 0.0, 0.0);
            assert!(world.set_collision_filter(wall, 31, 0));
            assert!(world.set_collision_filter(ball, 0, ignore_groups));
            assert!(world.apply_impulse(ball, Vector3D::new(60.0, 0.0, 0.0)));
            assert!(world.step());
            world.position(ball).x
        };
        assert!(simulate(0) < 0.0);
        assert!(simulate(1_i64 << 31) > 1.0);
    }
}
