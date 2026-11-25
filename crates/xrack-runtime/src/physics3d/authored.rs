//! Reconcile authored body declarations with a running simulation.

use std::collections::{BTreeMap, BTreeSet};

use rapier3d::prelude::{
    ColliderBuilder, GenericJoint, GenericJointBuilder, Pose, RigidBodyBuilder, Vector,
};

use super::{
    PhysicsCollider3D, PhysicsShape3D, PhysicsWorld3D, Simulation, collider_material,
    joints::{JointKind, distance_coupling, set_distance_axis},
    scene_pose, sensor_collider,
};
use crate::scene_hierarchy::CollisionShapeSpec;
use crate::{Point3D, RecordList, RecordValue, Value, Vector3D, scene3d::MeshAsset};

#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsShapeSpec {
    pub shape: String,
    pub position: Point3D,
    pub size: Vector3D,
    pub angles: [f64; 3],
    pub mesh: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsBodySpec {
    pub id: String,
    pub kind: String,
    pub position: Point3D,
    pub rotation: Vector3D,
    pub shapes: Vec<PhysicsShapeSpec>,
    pub mass: f64,
    pub friction: f64,
    pub restitution: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsJointSpec {
    pub id: String,
    pub kind: String,
    pub first: String,
    pub second: String,
    pub first_anchor: Point3D,
    pub second_anchor: Point3D,
    pub first_rotation: Vector3D,
    pub second_rotation: Vector3D,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

pub(super) struct AuthoredJoint {
    handle: i64,
    endpoints: [i64; 2],
    spec: PhysicsJointSpec,
}

struct PreparedBody {
    spec: PhysicsBodySpec,
    meshes: BTreeMap<String, Option<MeshAsset>>,
    builder: RigidBodyBuilder,
    colliders: Vec<ColliderBuilder>,
}

struct PreparedJoint {
    spec: PhysicsJointSpec,
    endpoints: [i64; 2],
    kind: JointKind,
    data: GenericJoint,
}

#[derive(Default)]
struct JointChanges {
    remove: Vec<i64>,
    add: Vec<PreparedJoint>,
}

pub(super) struct AuthoredBody {
    pub(super) handle: i64,
    spec: PhysicsBodySpec,
    meshes: BTreeMap<String, Option<MeshAsset>>,
}

impl PhysicsBodySpec {
    fn from_record(row: &RecordValue) -> Option<Self> {
        let Value::String(id) = row.field("id")? else {
            return None;
        };
        let Value::String(kind) = row.field("body-kind")? else {
            return None;
        };
        let Value::Point3D(position) = row.field("position")? else {
            return None;
        };
        let Value::RecordList(shapes) = row.field("shapes")? else {
            return None;
        };
        let rotation = match row.field("rotation") {
            Some(Value::Vector3D(value)) => *value,
            None => Vector3D::default(),
            _ => return None,
        };
        let number = |name, default| match row.field(name) {
            Some(Value::Float(value)) => Some(*value),
            None => Some(default),
            _ => None,
        };
        let (_, shapes) = shapes.model().inspect_rows(0, usize::MAX, |row| {
            let shape = CollisionShapeSpec::from_record(row)?;
            Some(PhysicsShapeSpec {
                shape: shape.shape.to_owned(),
                position: shape.position,
                size: shape.size,
                angles: shape.angles,
                mesh: shape.mesh.to_owned(),
            })
        });
        Some(Self {
            id: id.clone(),
            kind: kind.clone(),
            position: *position,
            rotation,
            shapes: shapes.into_iter().collect::<Option<Vec<_>>>()?,
            mass: number("mass", 1.0)?,
            friction: number("friction", 0.5)?,
            restitution: number("restitution", 0.0)?,
        })
    }

    fn prepare(
        &self,
        imported: &impl Fn(&str) -> Option<MeshAsset>,
    ) -> Option<(RigidBodyBuilder, Vec<ColliderBuilder>)> {
        let pose = scene_pose(self.position, self.rotation)?;
        let parts = self
            .shapes
            .iter()
            .map(|shape| {
                PhysicsShape3D::new_with_mesh(
                    &shape.shape,
                    shape.position,
                    shape.size,
                    shape.angles,
                    &shape.mesh,
                    imported,
                )
            })
            .collect::<Option<Vec<_>>>()?;
        let shapes = PhysicsCollider3D::from_shapes(parts);
        let material = |collider| collider_material(collider, self.friction, self.restitution);
        let (body, colliders) = match self.kind.as_str() {
            "fixed" => (
                RigidBodyBuilder::fixed(),
                shapes.into_surface_builders(true, self.friction, self.restitution)?,
            ),
            "dynamic" => {
                let mass = self.mass as f32;
                if !(mass > 0.0 && mass.is_finite() && mass.recip().is_finite()) {
                    return None;
                }
                (
                    RigidBodyBuilder::dynamic().ccd_enabled(true),
                    vec![material(shapes.into_convex()?.mass(mass))?],
                )
            }
            "kinematic" => (
                RigidBodyBuilder::kinematic_position_based().ccd_enabled(true),
                shapes.into_surface_builders(false, self.friction, self.restitution)?,
            ),
            "trigger" => (
                RigidBodyBuilder::fixed(),
                vec![sensor_collider(shapes.into_convex()?)],
            ),
            _ => return None,
        };
        (!colliders.is_empty()).then_some((body.pose(pose), colliders))
    }
}

impl PhysicsJointSpec {
    fn from_record(row: &RecordValue) -> Option<Self> {
        let string = |name| match row.field(name)? {
            Value::String(value) => Some(value.clone()),
            _ => None,
        };
        let point = |name| match row.field(name)? {
            Value::Point3D(value) => Some(*value),
            _ => None,
        };
        let rotation = |name| match row.field(name) {
            Some(Value::Vector3D(value)) => Some(*value),
            None => Some(Vector3D::default()),
            _ => None,
        };
        let number = |name| match row.field(name) {
            Some(Value::Float(value)) => Some(Some(*value)),
            None => Some(None),
            _ => None,
        };
        Some(Self {
            id: string("id")?,
            kind: string("joint-kind")?,
            first: string("first")?,
            second: string("second")?,
            first_anchor: point("first-anchor")?,
            second_anchor: point("second-anchor")?,
            first_rotation: rotation("first-rotation")?,
            second_rotation: rotation("second-rotation")?,
            min: number("min")?,
            max: number("max")?,
        })
    }

    fn prepare(&self, kind: JointKind, [first, second]: [Pose; 2]) -> Option<GenericJoint> {
        let a = scene_pose(self.first_anchor, self.first_rotation)?;
        let b = scene_pose(self.second_anchor, self.second_rotation)?;
        let mut joint = GenericJointBuilder::new(kind.locked_axes())
            .local_frame1(a)
            .local_frame2(b);
        let mut lower = 0.0;
        match (kind, self.min, self.max) {
            (JointKind::Fixed | JointKind::Spherical, _, _)
            | (JointKind::Prismatic | JointKind::Revolute, None, None) => {}
            (JointKind::Distance, min, Some(max)) => {
                let (axis, limits) = kind.limits(min.unwrap_or(0.0), max)?;
                lower = limits[0];
                joint = joint.limits(axis, limits);
            }
            (JointKind::Prismatic | JointKind::Revolute, Some(min), Some(max)) => {
                let (axis, limits) = kind.limits(min, max)?;
                joint = joint.limits(axis, limits);
            }
            _ => return None,
        }
        let mut joint = joint.build();
        if kind == JointKind::Distance {
            joint.coupled_axes = distance_coupling(lower);
            if joint.coupled_axes.is_empty() {
                let offset = (second * b).translation - (first * a).translation;
                let axis = offset.try_normalize().unwrap_or(Vector::X);
                set_distance_axis(&mut joint, first, second, axis);
            }
        }
        Some(joint)
    }
}

impl Simulation {
    fn prepare_authored_joints(
        &self,
        specs: Vec<PhysicsJointSpec>,
        body_ids: &BTreeSet<String>,
        pending: &[PreparedBody],
    ) -> Option<JointChanges> {
        let pending: BTreeMap<_, _> = pending
            .iter()
            .enumerate()
            .map(|(index, body)| {
                let id = self.next_id.checked_add(i64::try_from(index).ok()?)?;
                Some((body.spec.id.as_str(), (id, &body.spec)))
            })
            .collect::<Option<_>>()?;
        let endpoint = |name: &str| {
            if let Some(&(id, spec)) = pending.get(name) {
                let pose = scene_pose(spec.position, spec.rotation)?;
                return Some((id, spec.kind.as_str(), pose));
            }
            if !body_ids.contains(name) {
                return None;
            }
            let body = self.authored_bodies.get(name)?;
            let handle = *self.bodies.get(&body.handle)?;
            let pose = *self.world.bodies.get(handle)?.position();
            Some((body.handle, body.spec.kind.as_str(), pose))
        };
        let mut retained = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut changes = JointChanges::default();
        for spec in specs {
            if spec.id.trim().is_empty() || !seen.insert(spec.id.clone()) {
                return None;
            }
            let (first, first_kind, first_pose) = endpoint(&spec.first)?;
            let (second, second_kind, second_pose) = endpoint(&spec.second)?;
            if first == second
                || [first_kind, second_kind].contains(&"trigger")
                || ![first_kind, second_kind].contains(&"dynamic")
            {
                return None;
            }
            let endpoints = [first, second];
            if let Some(current) = self.authored_joints.get(&spec.id)
                && current.endpoints == endpoints
                && current.spec == spec
            {
                retained.insert(current.handle);
                continue;
            }
            let kind = JointKind::parse(&spec.kind)?;
            let data = spec.prepare(kind, [first_pose, second_pose])?;
            changes.add.push(PreparedJoint {
                spec,
                endpoints,
                kind,
                data,
            });
        }
        for joint in self
            .authored_joints
            .values()
            .filter(|joint| !retained.contains(&joint.handle))
        {
            let entry = self.joints.get(&joint.handle)?;
            self.world.impulse_joints.get(entry.handle)?;
            changes.remove.push(joint.handle);
        }
        Some(changes)
    }
}

impl PhysicsWorld3D {
    /// Synchronize a world-owned set of authored bodies, retaining unchanged definitions.
    pub fn sync_bodies(
        &self,
        specs: Vec<PhysicsBodySpec>,
        imported: impl Fn(&str) -> Option<MeshAsset>,
    ) -> bool {
        self.sync_authored(specs, None, imported)
    }

    /// Reconcile named bodies and their local-frame constraints in a single update.
    pub fn sync_scene(
        &self,
        bodies: Vec<PhysicsBodySpec>,
        joints: Vec<PhysicsJointSpec>,
        imported: impl Fn(&str) -> Option<MeshAsset>,
    ) -> bool {
        self.sync_authored(bodies, Some(joints), imported)
    }

    fn sync_authored(
        &self,
        specs: Vec<PhysicsBodySpec>,
        joints: Option<Vec<PhysicsJointSpec>>,
        imported: impl Fn(&str) -> Option<MeshAsset>,
    ) -> bool {
        self.update_selectively(|simulation| {
            let mut seen = BTreeSet::new();
            let mut prepared = Vec::new();
            for spec in specs {
                if spec.id.trim().is_empty() || !seen.insert(spec.id.clone()) {
                    return None;
                }
                let meshes: BTreeMap<_, _> = spec
                    .shapes
                    .iter()
                    .filter(|shape| {
                        matches!(shape.shape.as_str(), "convex-mesh" | "mesh" | "heightfield")
                    })
                    .map(|shape| (shape.mesh.clone(), imported(&shape.mesh)))
                    .collect();
                if simulation
                    .authored_bodies
                    .get(&spec.id)
                    .is_some_and(|body| body.spec == spec && body.meshes == meshes)
                {
                    continue;
                }
                let (builder, colliders) =
                    spec.prepare(&|key| meshes.get(key).and_then(Clone::clone))?;
                prepared.push(PreparedBody {
                    spec,
                    meshes,
                    builder,
                    colliders,
                });
            }

            let joints = match joints {
                Some(joints) => simulation.prepare_authored_joints(joints, &seen, &prepared)?,
                None => JointChanges::default(),
            };
            // Check allocations and removals before the first edit.
            let additions = prepared.len().checked_add(joints.add.len())?;
            simulation
                .next_id
                .checked_add(i64::try_from(additions).ok()?)?;
            let mut removed = Vec::new();
            for (id, existing) in &simulation.authored_bodies {
                if !seen.contains(id) {
                    removed.push((id.clone(), existing.handle));
                }
            }
            for handle in
                removed
                    .iter()
                    .map(|(_, handle)| *handle)
                    .chain(prepared.iter().filter_map(|body| {
                        simulation
                            .authored_bodies
                            .get(&body.spec.id)
                            .map(|body| body.handle)
                    }))
            {
                let body = simulation.bodies.get(&handle)?;
                simulation.world.bodies.get(*body)?;
            }
            let bodies_changed = !prepared.is_empty() || !removed.is_empty();
            let joints_changed = !joints.add.is_empty() || !joints.remove.is_empty();
            for joint in joints.remove {
                simulation.remove_joint(joint)?;
            }
            for (id, handle) in removed {
                simulation.remove_physics_body(handle)?;
                simulation.authored_bodies.remove(&id);
            }
            for PreparedBody {
                spec,
                meshes,
                builder,
                colliders,
            } in prepared
            {
                if let Some(existing) = simulation.authored_bodies.remove(&spec.id) {
                    simulation.remove_physics_body(existing.handle)?;
                }
                let handle = simulation.insert_physics_body(builder, colliders, None)?;
                let id = spec.id.clone();
                simulation.authored_bodies.insert(
                    id,
                    AuthoredBody {
                        handle,
                        spec,
                        meshes,
                    },
                );
            }
            for joint in joints.add {
                let [first, second] = joint.endpoints;
                let endpoints = [
                    *simulation.bodies.get(&first)?,
                    *simulation.bodies.get(&second)?,
                ];
                let name = joint.spec.id.clone();
                let handle = simulation.register_joint(
                    endpoints,
                    joint.kind,
                    joint.data,
                    Some(name.clone()),
                )?;
                simulation.authored_joints.insert(
                    name,
                    AuthoredJoint {
                        handle,
                        endpoints: joint.endpoints,
                        spec: joint.spec,
                    },
                );
            }
            simulation.authored_changed |= bodies_changed;
            simulation.authored_joints_changed |= joints_changed;
            Some((true, bodies_changed || joints_changed))
        })
        .unwrap_or_default()
    }

    /// Resolve a declared body by its authoring ID. Zero denotes a missing body.
    pub fn body_by_id(&self, id: &str) -> i64 {
        let read = || {
            let state = self.0.as_ref()?;
            state.authored_changed.track();
            let simulation = state.state.try_borrow().ok()?;
            simulation.authored_bodies.get(id).map(|body| body.handle)
        };
        read().unwrap_or(0)
    }

    /// Resolve a declared joint by its authoring ID. Zero denotes a missing joint.
    pub fn joint_by_id(&self, id: &str) -> i64 {
        let read = || {
            let state = self.0.as_ref()?;
            state.authored_joints_changed.track();
            let simulation = state.state.try_borrow().ok()?;
            simulation.authored_joints.get(id).map(|joint| joint.handle)
        };
        read().unwrap_or_default()
    }

    pub(crate) fn scene_poses<'a>(
        &self,
        ids: impl Iterator<Item = &'a str>,
    ) -> Option<Vec<Option<Pose>>> {
        let state = self.0.as_ref()?;
        state.authored_changed.track();
        let simulation = state.state.try_borrow().ok()?;
        let poses: Vec<_> = ids
            .map(|id| {
                let body = simulation.authored_bodies.get(id)?;
                simulation.render_pose(body.handle)
            })
            .collect();
        if poses.iter().any(Option::is_some) {
            state.changed.track();
            state.clock_changed.track();
            Some(poses)
        } else {
            None
        }
    }

    #[doc(hidden)]
    pub fn sync_body_records(
        &self,
        records: &RecordList,
        imported: impl Fn(&str) -> Option<MeshAsset>,
    ) -> bool {
        let (_, specs) = records
            .model()
            .inspect_rows(0, usize::MAX, PhysicsBodySpec::from_record);
        specs
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .is_some_and(|specs| self.sync_bodies(specs, imported))
    }

    #[doc(hidden)]
    pub fn sync_scene_records(
        &self,
        bodies: &RecordList,
        joints: &RecordList,
        imported: impl Fn(&str) -> Option<MeshAsset>,
    ) -> bool {
        let (_, bodies) = bodies
            .model()
            .inspect_rows(0, usize::MAX, PhysicsBodySpec::from_record);
        let (_, joints) = joints
            .model()
            .inspect_rows(0, usize::MAX, PhysicsJointSpec::from_record);
        let (Some(bodies), Some(joints)) = (
            bodies.into_iter().collect::<Option<Vec<_>>>(),
            joints.into_iter().collect::<Option<Vec<_>>>(),
        ) else {
            return false;
        };
        self.sync_scene(bodies, joints, imported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(id: &str, kind: &str, x: f64) -> PhysicsBodySpec {
        PhysicsBodySpec {
            id: id.into(),
            kind: kind.into(),
            position: Point3D::new(x, 0.0, 0.0),
            rotation: Vector3D::default(),
            shapes: vec![PhysicsShapeSpec {
                shape: "box".into(),
                position: Point3D::default(),
                size: Vector3D::new(1.0, 1.0, 1.0),
                angles: [0.0; 3],
                mesh: String::new(),
            }],
            mass: 1.0,
            friction: 0.3,
            restitution: 0.0,
        }
    }

    #[test]
    fn authored_scene_replaces_only_affected_constraints_and_rejects_invalid_edits() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.01);
        let mut bodies = vec![
            body("mount", "fixed", 0.0),
            body("door", "dynamic", 1.0),
            body("track", "fixed", 6.0),
            body("slider", "dynamic", 8.0),
        ];
        let origin = Point3D::default();
        let base = PhysicsJointSpec {
            id: "hinge".into(),
            kind: "revolute".into(),
            first: "mount".into(),
            second: "door".into(),
            first_anchor: origin,
            second_anchor: Point3D::new(-1.0, 0.0, 0.0),
            first_rotation: Vector3D::new(0.0, 0.0, 90.0),
            second_rotation: Vector3D::new(0.0, 0.0, 90.0),
            min: None,
            max: None,
        };
        let rail = PhysicsJointSpec {
            id: "rail".into(),
            kind: "prismatic".into(),
            first: "track".into(),
            second: "slider".into(),
            second_anchor: Point3D::new(-2.0, 0.0, 0.0),
            first_rotation: Vector3D::default(),
            second_rotation: Vector3D::default(),
            ..base.clone()
        };
        let joints = vec![base.clone(), rail.clone()];
        assert!(world.sync_scene(bodies.clone(), joints.clone(), |_| None));
        let mount = world.body_by_id("mount");
        let door = world.body_by_id("door");
        let hinge = world.joint_by_id("hinge");
        let rail_id = world.joint_by_id("rail");
        assert!(world.set_joint_motor(hinge, 80.0, 100.0));
        assert!(world.set_joint_motor(rail_id, 2.0, 100.0));
        for _ in 0..12 {
            assert!(world.step());
        }
        assert!(world.joint_position(hinge) > 1.0);
        let pose = world.position(door);
        let rail_position = world.joint_position(rail_id);
        let reversed = bodies.iter().rev().cloned().collect();
        assert!(world.sync_scene(reversed, vec![rail, base.clone()], |_| None));
        assert_eq!(world.body_by_id("door"), door);
        assert_eq!(world.joint_by_id("hinge"), hinge);
        assert_eq!(world.joint_by_id("rail"), rail_id);
        assert_eq!(world.position(door), pose);

        bodies[0].position.x = -0.3;
        let mut invalid = joints.clone();
        invalid[1].second = "missing".into();
        assert!(!world.sync_scene(bodies.clone(), invalid, |_| None));
        assert_eq!(world.body_by_id("mount"), mount);
        assert_eq!(world.joint_by_id("hinge"), hinge);
        assert!(world.sync_scene(bodies, joints, |_| None));
        assert_ne!(world.body_by_id("mount"), mount);
        assert_eq!(world.body_by_id("door"), door);
        assert_eq!(world.position(door), pose);
        assert_ne!(world.joint_by_id("hinge"), hinge);
        assert!(!world.remove_joint(hinge));
        assert_eq!(world.joint_by_id("rail"), rail_id);
        for _ in 0..8 {
            assert!(world.step());
        }
        assert!(world.joint_position(rail_id) > rail_position + 0.05);
        assert!(world.remove(door));
        assert_eq!(world.joint_by_id("hinge"), 0);
        assert!(world.remove_joint(rail_id));
        assert_eq!(world.joint_by_id("rail"), 0);
    }

    #[test]
    fn reconciliation_preserves_unchanged_bodies_and_excludes_manual_bodies() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.1);
        let runner = body("runner", "dynamic", 0.0);
        let ramp = body("ramp", "fixed", 5.0);
        let elevator = body("lift", "kinematic", 10.0);
        let sensor = body("sensor", "trigger", 0.0);
        let manual = world.add_sphere(Point3D::new(-5.0, 0.0, 0.0), 0.5, 1.0, 0.0, 0.0);
        assert!(world.sync_bodies(
            vec![runner.clone(), ramp, elevator.clone(), sensor.clone()],
            |_| None,
        ));
        let runner_id = world.body_by_id("runner");
        let ramp_id = world.body_by_id("ramp");
        let lift_id = world.body_by_id("lift");
        let sensor_id = world.body_by_id("sensor");
        assert!(world.is_kinematic(lift_id));
        assert!(world.set_linear_velocity(runner_id, Vector3D::new(2.0, 0.0, 0.0)));
        assert!(world.step());
        assert!(world.trigger_overlaps(sensor_id, runner_id));
        let position = world.position(runner_id);
        let velocity = world.velocity(runner_id);
        assert!(world.sync_bodies(
            vec![sensor, elevator, body("ramp", "fixed", 8.0), runner],
            |_| None,
        ));
        assert_eq!(world.body_by_id("runner"), runner_id);
        assert_eq!(world.body_by_id("sensor"), sensor_id);
        assert_eq!(world.position(runner_id), position);
        assert_eq!(world.velocity(runner_id), velocity);
        assert!(world.trigger_overlaps(sensor_id, runner_id));
        assert_ne!(world.body_by_id("ramp"), ramp_id);
        assert_eq!(world.position(world.body_by_id("ramp")).x, 8.0);
        assert!(world.remove(lift_id));
        assert_eq!(world.body_by_id("lift"), 0);
        assert!(world.sync_bodies(Vec::new(), |_| None));
        assert_eq!(world.body_by_id("runner"), 0);
        assert_eq!(world.position(manual).x, -5.0);
        assert!(world.sync_bodies(vec![body("new", "fixed", 1.0)], |_| None));
        world.clear();
        assert_eq!(world.body_by_id("new"), 0);
    }

    #[test]
    fn a_replaced_mesh_asset_updates_only_the_dependent_body() {
        use crate::scene3d::Mesh3D;

        let world = PhysicsWorld3D::new(Vector3D::default(), 0.1);
        let mut mesh_body = body("mesh", "dynamic", 0.0);
        mesh_body.shapes[0].shape = "convex-mesh".into();
        mesh_body.shapes[0].mesh = "external".into();
        let fixed = body("fixed", "fixed", 4.0);
        let specs = vec![mesh_body, fixed];
        let box_mesh = |_: &str| Some(MeshAsset::Static(Mesh3D::Box));
        assert!(world.sync_bodies(specs.clone(), box_mesh));
        let mesh = world.body_by_id("mesh");
        let fixed = world.body_by_id("fixed");
        assert!(mesh > 0 && fixed > 0);
        assert!(world.sync_bodies(specs.clone(), box_mesh));
        assert_eq!(world.body_by_id("mesh"), mesh);
        assert!(!world.sync_bodies(specs.clone(), |_| None));
        assert_eq!(world.body_by_id("mesh"), mesh);
        assert!(world.sync_bodies(specs, |_| Some(MeshAsset::Static(Mesh3D::Sphere))));
        assert_ne!(world.body_by_id("mesh"), mesh);
        assert_eq!(world.body_by_id("fixed"), fixed);
    }

    #[test]
    fn rejected_batches_leave_declarations_and_identifiers_intact() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.1);
        let runner = body("runner", "dynamic", 1.0);
        let ramp = body("ramp", "fixed", 8.0);
        assert!(world.sync_bodies(vec![runner.clone(), ramp], |_| None));
        let runner_id = world.body_by_id("runner");
        let ramp_id = world.body_by_id("ramp");
        let replacement = body("runner", "dynamic", -2.0);
        let mut invalid = body("second", "dynamic", 0.0);
        invalid.shapes[0].size.x = 0.0;
        assert!(!world.sync_bodies(vec![replacement.clone(), invalid], |_| None));
        assert!(!world.sync_bodies(vec![replacement, runner.clone()], |_| None));
        assert_eq!(world.body_by_id("runner"), runner_id);
        assert_eq!(world.body_by_id("ramp"), ramp_id);
        assert_eq!(world.position(runner_id).x, 1.0);
        assert!(world.sync_bodies(vec![runner, body("another", "fixed", -8.0)], |_| None));
        assert_eq!(world.body_by_id("another"), ramp_id + 1);
        assert_eq!(world.body_by_id("ramp"), 0);
    }
}
