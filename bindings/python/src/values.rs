//! Python representations of public interpreter values.

use pyo3::{
    PyTypeInfo,
    exceptions::{PyIndexError, PyKeyError, PyTypeError, PyValueError},
    inspect::PyStaticExpr,
    prelude::*,
    type_hint_identifier, type_hint_subscript,
    types::{PyBool, PyFloat, PyInt, PyList, PyString},
};
use std::{collections::BTreeMap, path::PathBuf};
use xrack_interpreter::{HostValue, ValueType};
use xrack_runtime as rt;

pub(super) struct PythonValue(pub(super) HostValue);

impl PythonValue {
    pub(super) fn into_for(self, ty: &ValueType) -> PyResult<HostValue> {
        let value = match (ty, self.0) {
            (ValueType::Float, HostValue::Int(value)) => HostValue::Float(value as f64),
            (ValueType::Hit3DList, HostValue::StringList(value)) if value.is_empty() => {
                HostValue::Hit3DList(rt::VecModel::default())
            }
            (ValueType::PhysicsContact3DList, HostValue::StringList(value)) if value.is_empty() => {
                HostValue::PhysicsContact3DList(rt::VecModel::default())
            }
            (ValueType::PhysicsHit3DList, HostValue::StringList(value)) if value.is_empty() => {
                HostValue::PhysicsHit3DList(rt::VecModel::default())
            }
            (ValueType::IntList, HostValue::StringList(value)) if value.is_empty() => {
                HostValue::IntList(Vec::new())
            }
            (ValueType::DateList, HostValue::StringList(value)) if value.is_empty() => {
                HostValue::DateList(Vec::new())
            }
            (_, value) => value,
        };
        if ty.matches(&value) {
            Ok(value)
        } else {
            Err(PyTypeError::new_err(format!("expected XRack {ty:?}")))
        }
    }
}

impl<'py> FromPyObject<'_, 'py> for PythonValue {
    type Error = PyErr;
    const INPUT_TYPE: PyStaticExpr = VALUE_TYPE;

    fn extract(value: Borrowed<'_, 'py, PyAny>) -> PyResult<Self> {
        decode(&value).map(Self)
    }
}

impl<'py> IntoPyObject<'py> for PythonValue {
    type Target = PyAny;
    type Output = Bound<'py, PyAny>;
    type Error = PyErr;
    const OUTPUT_TYPE: PyStaticExpr = VALUE_TYPE;

    fn into_pyobject(self, py: Python<'py>) -> PyResult<Self::Output> {
        encode(py, self.0)
    }
}

macro_rules! owned_value {
    ($name:ident, $runtime:ty) => {
        #[pyclass(frozen, eq, from_py_object, module = "xrack._native")]
        #[derive(Clone, PartialEq)]
        struct $name($runtime);

        impl From<$name> for $runtime {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl From<$runtime> for $name {
            fn from(value: $runtime) -> Self {
                Self(value)
            }
        }
    };
}

macro_rules! struct_value {
    ($name:ident { $($field:ident: $ty:ty),+ $(,)? }) => {
        owned_value!($name, rt::$name);

        #[pymethods]
        impl $name {
            #[new]
            fn new($($field: $ty),+) -> Self {
                Self(rt::$name { $($field: $field.into()),+ })
            }

            $(
                #[getter]
                fn $field(&self) -> $ty {
                    self.0.$field.to_owned().into()
                }
            )+
        }
    };
}

struct_value!(Font {
    family: String,
    size: Length,
    weight: i64,
    italic: bool
});
struct_value!(Point2D {
    x: Length,
    y: Length
});
struct_value!(Point3D {
    x: f64,
    y: f64,
    z: f64
});
struct_value!(Vector3D {
    x: f64,
    y: f64,
    z: f64
});
owned_value!(Quaternion3D, rt::Quaternion3D);

#[pymethods]
impl Quaternion3D {
    #[new]
    #[pyo3(signature = (x=0.0, y=0.0, z=0.0, w=1.0))]
    fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Self(rt::Quaternion3D::new(x, y, z, w))
    }

    #[staticmethod]
    fn from_axis_angle(axis: &Vector3D, degrees: f64) -> Self {
        Self(rt::Quaternion3D::from_axis_angle(axis.0, degrees))
    }
    #[staticmethod]
    fn from_euler(angles: &Vector3D) -> Self {
        Self(rt::Quaternion3D::from_euler(angles.0))
    }
    #[staticmethod]
    #[pyo3(signature = (forward, up=None))]
    fn look_rotation(forward: &Vector3D, up: Option<&Vector3D>) -> Self {
        let up = up.map_or(rt::Vector3D::new(0.0, 1.0, 0.0), |value| value.0);
        Self(rt::Quaternion3D::look_rotation(forward.0, up))
    }
    #[staticmethod]
    #[pyo3(signature = (origin, target, up=None))]
    fn look_at(origin: &Point3D, target: &Point3D, up: Option<&Vector3D>) -> Self {
        let up = up.map_or(rt::Vector3D::new(0.0, 1.0, 0.0), |value| value.0);
        Self(rt::Quaternion3D::look_at(origin.0, target.0, up))
    }
    fn then(&self, next: &Self) -> Self {
        Self(self.0.then(next.0))
    }
    fn inverse(&self) -> Self {
        Self(self.0.inverse())
    }
    fn slerp(&self, next: &Self, progress: f64) -> Self {
        Self(self.0.slerp(next.0, progress))
    }
    fn rotate(&self, vector: &Vector3D) -> Vector3D {
        Vector3D(self.0.rotate(vector.0))
    }
    #[getter]
    fn x(&self) -> f64 {
        self.0.x()
    }
    #[getter]
    fn y(&self) -> f64 {
        self.0.y()
    }
    #[getter]
    fn z(&self) -> f64 {
        self.0.z()
    }
    #[getter]
    fn w(&self) -> f64 {
        self.0.w()
    }
}

owned_value!(RotationTrack3D, rt::RotationTrack3D);

#[pymethods]
impl RotationTrack3D {
    #[new]
    fn new() -> Self {
        Self(rt::RotationTrack3D::default())
    }

    #[staticmethod]
    fn from_keys(keys: Vec<(f64, PyRef<'_, Quaternion3D>)>) -> Self {
        Self(rt::RotationTrack3D::new(
            keys.into_iter().map(|(time, q)| (time, q.0)).collect(),
        ))
    }

    #[getter]
    fn valid(&self) -> bool {
        self.0.valid()
    }

    #[getter]
    fn count(&self) -> i64 {
        self.0.count()
    }

    #[getter]
    fn duration(&self) -> f64 {
        self.0.duration()
    }

    fn sample(&self, time: f64) -> Quaternion3D {
        Quaternion3D(self.0.sample(time))
    }
}

owned_value!(InstanceTable3D, rt::InstanceTable3D);

#[pymethods]
impl InstanceTable3D {
    #[new]
    fn new() -> Self {
        Self(rt::InstanceTable3D::default())
    }

    #[getter]
    fn loaded(&self) -> bool {
        self.0.is_loaded()
    }

    #[getter]
    fn count(&self) -> usize {
        self.0.len()
    }

    fn __len__(&self) -> usize {
        self.0.len()
    }

    fn instance(&self, index: i64) -> Instance3D {
        Instance3D(self.0.instance(index))
    }
}

owned_value!(Instance3D, rt::Instance3D);

#[pymethods]
impl Instance3D {
    #[new]
    fn new() -> Self {
        Self(rt::Instance3D::default())
    }
    #[getter]
    fn valid(&self) -> bool {
        self.0.valid
    }
    #[getter]
    fn object_id(&self) -> String {
        self.0.object_id.clone()
    }
    #[getter]
    fn row_index(&self) -> i64 {
        self.0.row_index
    }
    #[getter]
    fn mesh_node_index(&self) -> i64 {
        self.0.mesh_node_index
    }
    #[getter]
    fn instance_index(&self) -> i64 {
        self.0.instance_index
    }
    #[getter]
    fn color(&self) -> Color {
        self.0.color.into()
    }
    #[getter]
    fn origin(&self) -> Point3D {
        self.0.origin.into()
    }
    #[getter]
    fn axis_x(&self) -> Vector3D {
        self.0.axis_x.into()
    }
    #[getter]
    fn axis_y(&self) -> Vector3D {
        self.0.axis_y.into()
    }
    #[getter]
    fn axis_z(&self) -> Vector3D {
        self.0.axis_z.into()
    }
    #[getter]
    fn data_x(&self) -> f64 {
        self.0.data_x
    }
    #[getter]
    fn data_y(&self) -> f64 {
        self.0.data_y
    }
    #[getter]
    fn data_z(&self) -> f64 {
        self.0.data_z
    }
    #[getter]
    fn data_w(&self) -> f64 {
        self.0.data_w
    }
}

owned_value!(Hit3D, rt::Hit3D);

#[pymethods]
impl Hit3D {
    #[new]
    #[pyo3(signature = (object_id, distance, position, normal, local_position, local_normal, uv, *, row_index = -1, mesh_node_index = -1, instance_index = -1))]
    fn new(
        object_id: String,
        distance: f64,
        position: Point3D,
        normal: Vector3D,
        local_position: Point3D,
        local_normal: Vector3D,
        uv: (f64, f64),
        row_index: i64,
        mesh_node_index: i64,
        instance_index: i64,
    ) -> Self {
        Self(rt::Hit3D::from_data(rt::scene3d::Hit3DData {
            valid: true,
            entering: false,
            object_id,
            row_index,
            mesh_node_index,
            instance_index,
            distance,
            position: position.0,
            normal: normal.0,
            local_position: local_position.0,
            local_normal: local_normal.0,
            u: uv.0,
            v: uv.1,
        }))
    }

    #[staticmethod]
    fn miss() -> Self {
        Self(rt::Hit3D::default())
    }

    #[getter]
    fn valid(&self) -> bool {
        self.0.valid
    }
    #[getter]
    fn entering(&self) -> bool {
        self.0.entering
    }
    #[getter]
    fn object_id(&self) -> String {
        self.0.object_id.clone()
    }
    #[getter]
    fn row_index(&self) -> i64 {
        self.0.row_index
    }
    #[getter]
    fn mesh_node_index(&self) -> i64 {
        self.0.mesh_node_index
    }
    #[getter]
    fn instance_index(&self) -> i64 {
        self.0.instance_index
    }
    #[getter]
    fn distance(&self) -> f64 {
        self.0.distance
    }
    #[getter]
    fn position(&self) -> Point3D {
        self.0.position.into()
    }
    #[getter]
    fn normal(&self) -> Vector3D {
        self.0.normal.into()
    }
    #[getter]
    fn local_position(&self) -> Point3D {
        self.0.local_position.into()
    }
    #[getter]
    fn local_normal(&self) -> Vector3D {
        self.0.local_normal.into()
    }
    #[getter]
    fn u(&self) -> f64 {
        self.0.u
    }
    #[getter]
    fn v(&self) -> f64 {
        self.0.v
    }
}
owned_value!(SceneModel3D, std::sync::Arc<rt::scene3d::SceneModel>);

#[pymethods]
impl SceneModel3D {
    #[new]
    fn new() -> Self {
        Self(Default::default())
    }

    fn __len__(&self) -> usize {
        self.0.len()
    }

    fn instance(&self, row_index: i64, mesh_node_index: i64, instance_index: i64) -> Instance3D {
        self.0
            .instance([row_index, mesh_node_index, instance_index])
            .into()
    }

    fn instance_camera(
        &self,
        camera: &str,
        viewport: Point2D,
        row_index: i64,
        mesh_node_index: i64,
        instance_index: i64,
    ) -> Instance3D {
        self.0
            .instance_camera(
                camera,
                viewport.into(),
                [row_index, mesh_node_index, instance_index],
            )
            .into()
    }

    fn instance_orbit(
        &self,
        yaw: f64,
        pitch: f64,
        distance: f64,
        viewport: Point2D,
        row_index: i64,
        mesh_node_index: i64,
        instance_index: i64,
    ) -> Instance3D {
        self.0
            .instance_orbit(
                [yaw, pitch, distance],
                viewport.into(),
                [row_index, mesh_node_index, instance_index],
            )
            .into()
    }

    fn map_position(&self, from_node: &str, to_node: &str, point: Point3D) -> Option<Point3D> {
        self.0
            .map_position(from_node, to_node, point.into())
            .map(Into::into)
    }
}

#[pyclass(frozen, eq, unsendable, from_py_object, module = "xrack._native")]
#[derive(Clone, PartialEq)]
struct PhysicsWorld3D(rt::PhysicsWorld3D);

#[pymethods]
impl PhysicsWorld3D {
    #[new]
    fn new(gravity: Vector3D, timestep: f64) -> Self {
        Self(rt::PhysicsWorld3D::new(gravity.into(), timestep))
    }

    fn sync_bodies(&self, bodies: PyRef<'_, RecordList>) -> bool {
        self.0.sync_body_records(&bodies.0, |_| None)
    }

    fn body_by_id(&self, id: &str) -> i64 {
        self.0.body_by_id(id)
    }

    fn sync_scene(&self, bodies: PyRef<'_, RecordList>, joints: PyRef<'_, RecordList>) -> bool {
        self.0.sync_scene_records(&bodies.0, &joints.0, |_| None)
    }

    fn joint_by_id(&self, id: &str) -> i64 {
        self.0.joint_by_id(id)
    }

    fn add_body(
        &self,
        center: Point3D,
        rotation: Vector3D,
        shapes: PyRef<'_, RecordList>,
        mass: f64,
        friction: f64,
        restitution: f64,
    ) -> i64 {
        self.0.add_body(
            center.into(),
            rotation.into(),
            rt::PhysicsCollider3D::from_records(&shapes.0),
            mass,
            friction,
            restitution,
        )
    }

    fn add_kinematic_body(
        &self,
        center: Point3D,
        rotation: Vector3D,
        shapes: PyRef<'_, RecordList>,
        friction: f64,
        restitution: f64,
    ) -> i64 {
        self.0.add_kinematic_body(
            center.into(),
            rotation.into(),
            rt::PhysicsCollider3D::from_records(&shapes.0),
            friction,
            restitution,
        )
    }

    fn add_trigger(
        &self,
        center: Point3D,
        rotation: Vector3D,
        shapes: PyRef<'_, RecordList>,
    ) -> i64 {
        self.0.add_trigger(
            center.into(),
            rotation.into(),
            rt::PhysicsCollider3D::from_records(&shapes.0),
        )
    }

    fn add_box(
        &self,
        center: Point3D,
        size: Vector3D,
        mass: f64,
        friction: f64,
        restitution: f64,
    ) -> i64 {
        self.0
            .add_box(center.into(), size.into(), mass, friction, restitution)
    }

    fn add_sphere(
        &self,
        center: Point3D,
        radius: f64,
        mass: f64,
        friction: f64,
        restitution: f64,
    ) -> i64 {
        self.0
            .add_sphere(center.into(), radius, mass, friction, restitution)
    }

    fn add_kinematic_box(
        &self,
        center: Point3D,
        size: Vector3D,
        rotation: Vector3D,
        friction: f64,
        restitution: f64,
    ) -> i64 {
        self.0.add_kinematic_box(
            center.into(),
            size.into(),
            rotation.into(),
            friction,
            restitution,
        )
    }

    fn is_kinematic(&self, body: i64) -> bool {
        self.0.is_kinematic(body)
    }

    fn set_kinematic(&self, body: i64, enabled: bool) -> bool {
        self.0.set_kinematic(body, enabled)
    }

    fn set_kinematic_target_with_pivot(
        &self,
        body: i64,
        position: Point3D,
        rotation: Vector3D,
        pivot: Vector3D,
    ) -> bool {
        self.0
            .set_kinematic_target_with_pivot(body, position.into(), rotation.into(), pivot.into())
    }

    fn set_kinematic_target(&self, body: i64, center: Point3D, rotation: Vector3D) -> bool {
        self.0
            .set_kinematic_target(body, center.into(), rotation.into())
    }

    fn add_box_trigger(&self, center: Point3D, size: Vector3D, rotation: Vector3D) -> i64 {
        self.0
            .add_box_trigger(center.into(), size.into(), rotation.into())
    }

    fn trigger_count(&self, trigger: i64) -> i64 {
        self.0.trigger_count(trigger)
    }

    fn set_kinematic_contact_reports(
        &self,
        static_kinematic: bool,
        kinematic_kinematic: bool,
    ) -> bool {
        self.0
            .set_kinematic_contact_reports(static_kinematic, kinematic_kinematic)
    }

    fn report_static_kinematic_collisions(&self) -> bool {
        self.0.report_static_kinematic_collisions()
    }

    fn report_kinematic_kinematic_collisions(&self) -> bool {
        self.0.report_kinematic_kinematic_collisions()
    }

    fn set_contact_events(&self, body: i64, enabled: bool) -> bool {
        self.0.set_contact_events(body, enabled)
    }

    fn contact_bodies(&self, body: i64) -> Vec<i64> {
        self.0.contact_bodies(body)
    }

    fn contact_entered(&self, body: i64) -> Vec<i64> {
        self.0.contact_entered(body)
    }

    fn contact_exited(&self, body: i64) -> Vec<i64> {
        self.0.contact_exited(body)
    }

    fn set_contact_reports(&self, body: i64, send: bool, receive: bool) -> bool {
        self.0.set_contact_reports(body, send, receive)
    }

    fn contacts(&self, body: i64) -> Vec<PhysicsContact3D> {
        self.0
            .contacts(body)
            .into_iter()
            .map(PhysicsContact3D)
            .collect()
    }

    fn set_collision_filter(&self, body: i64, group: i64, ignore_groups: i64) -> bool {
        self.0.set_collision_filter(body, group, ignore_groups)
    }

    fn add_fixed_joint(&self, first: i64, second: i64, anchor: Point3D) -> i64 {
        self.0.add_fixed_joint(first, second, anchor.into())
    }

    fn add_spherical_joint(&self, first: i64, second: i64, anchor: Point3D) -> i64 {
        self.0.add_spherical_joint(first, second, anchor.into())
    }

    fn add_revolute_joint(
        &self,
        first: i64,
        second: i64,
        anchor: Point3D,
        axis: Vector3D,
        min: f64,
        max: f64,
    ) -> i64 {
        self.0
            .add_revolute_joint(first, second, anchor.into(), axis.into(), min, max)
    }

    fn add_prismatic_joint(
        &self,
        first: i64,
        second: i64,
        anchor: Point3D,
        axis: Vector3D,
        min: f64,
        max: f64,
    ) -> i64 {
        self.0
            .add_prismatic_joint(first, second, anchor.into(), axis.into(), min, max)
    }

    fn add_distance_joint(
        &self,
        first: i64,
        second: i64,
        first_anchor: Point3D,
        second_anchor: Point3D,
        min: f64,
        max: f64,
    ) -> i64 {
        self.0.add_distance_joint(
            first,
            second,
            first_anchor.into(),
            second_anchor.into(),
            min,
            max,
        )
    }

    fn set_joint_limits(&self, joint: i64, min: f64, max: f64) -> bool {
        self.0.set_joint_limits(joint, min, max)
    }

    fn set_joint_motor(&self, joint: i64, velocity: f64, max_force: f64) -> bool {
        self.0.set_joint_motor(joint, velocity, max_force)
    }

    fn joint_position(&self, joint: i64) -> f64 {
        self.0.joint_position(joint)
    }

    fn remove_joint(&self, joint: i64) -> bool {
        self.0.remove_joint(joint)
    }

    fn trigger_overlaps(&self, trigger: i64, body: i64) -> bool {
        self.0.trigger_overlaps(trigger, body)
    }

    fn trigger_entered(&self, trigger: i64) -> Vec<i64> {
        self.0.trigger_entered(trigger)
    }

    fn trigger_exited(&self, trigger: i64) -> Vec<i64> {
        self.0.trigger_exited(trigger)
    }

    fn step(&self) -> bool {
        self.0.step()
    }

    fn advance(&self, elapsed: f64, max_steps: i64) -> i64 {
        self.0.advance(elapsed, max_steps)
    }

    fn advance_realtime(&self, max_steps: i64) -> i64 {
        self.0.advance_realtime(max_steps)
    }

    fn set_running(&self, running: bool) -> bool {
        self.0.set_running(running)
    }

    fn set_time_scale(&self, scale: f64) -> bool {
        self.0.set_time_scale(scale)
    }

    fn is_running(&self) -> bool {
        self.0.is_running()
    }

    fn time_scale(&self) -> f64 {
        self.0.time_scale()
    }

    fn simulation_time(&self) -> f64 {
        self.0.simulation_time()
    }

    fn dropped_time(&self) -> f64 {
        self.0.dropped_time()
    }

    fn interpolation_alpha(&self) -> f64 {
        self.0.interpolation_alpha()
    }

    fn set_pose_interpolation(&self, body: i64, enabled: bool) -> bool {
        self.0.set_pose_interpolation(body, enabled)
    }

    fn interpolated_position(&self, body: i64) -> Point3D {
        self.0.interpolated_position(body).into()
    }

    fn interpolated_rotation(&self, body: i64) -> Vector3D {
        self.0.interpolated_rotation(body).into()
    }

    fn clear(&self) -> bool {
        self.0.clear()
    }

    fn position(&self, body: i64) -> Point3D {
        self.0.position(body).into()
    }

    fn rotation(&self, body: i64) -> Vector3D {
        self.0.rotation(body).into()
    }

    fn velocity(&self, body: i64) -> Vector3D {
        self.0.velocity(body).into()
    }

    fn apply_impulse(&self, body: i64, impulse: Vector3D) -> bool {
        self.0.apply_impulse(body, impulse.into())
    }

    fn apply_force(&self, body: i64, force: Vector3D) -> bool {
        self.0.apply_force(body, force.into())
    }

    fn apply_force_at(&self, body: i64, force: Vector3D, point: Point3D) -> bool {
        self.0.apply_force_at(body, force.into(), point.into())
    }

    fn apply_torque(&self, body: i64, torque: Vector3D) -> bool {
        self.0.apply_torque(body, torque.into())
    }

    fn set_continuous_load(&self, body: i64, force: Vector3D, torque: Vector3D) -> bool {
        self.0
            .set_continuous_load(body, force.into(), torque.into())
    }

    fn clear_forces(&self, body: i64) -> bool {
        self.0.clear_forces(body)
    }

    fn apply_impulse_at(&self, body: i64, impulse: Vector3D, point: Point3D) -> bool {
        self.0.apply_impulse_at(body, impulse.into(), point.into())
    }

    fn apply_torque_impulse(&self, body: i64, impulse: Vector3D) -> bool {
        self.0.apply_torque_impulse(body, impulse.into())
    }

    fn set_linear_velocity(&self, body: i64, velocity: Vector3D) -> bool {
        self.0.set_linear_velocity(body, velocity.into())
    }

    fn set_angular_velocity(&self, body: i64, velocity: Vector3D) -> bool {
        self.0.set_angular_velocity(body, velocity.into())
    }

    fn angular_velocity(&self, body: i64) -> Vector3D {
        self.0.angular_velocity(body).into()
    }

    fn reset_body(&self, body: i64, center: Point3D, rotation: Vector3D) -> bool {
        self.0.reset_body(body, center.into(), rotation.into())
    }

    fn set_gravity_scale(&self, body: i64, scale: f64) -> bool {
        self.0.set_gravity_scale(body, scale)
    }

    fn set_mass(&self, body: i64, mass: f64) -> bool {
        self.0.set_mass(body, mass)
    }

    fn set_density(&self, body: i64, density: f64) -> bool {
        self.0.set_density(body, density)
    }

    fn use_default_density(&self, body: i64) -> bool {
        self.0.use_default_density(body)
    }

    fn set_default_density(&self, density: f64) -> bool {
        self.0.set_default_density(density)
    }

    fn default_density(&self) -> f64 {
        self.0.default_density()
    }

    fn set_mass_properties(
        &self,
        body: i64,
        mass: f64,
        local_center: Point3D,
        principal_inertia: Vector3D,
        principal_rotation: Vector3D,
    ) -> bool {
        self.0.set_mass_properties(
            body,
            mass,
            local_center.into(),
            principal_inertia.into(),
            principal_rotation.into(),
        )
    }

    fn set_inertia_matrix(
        &self,
        body: i64,
        mass: f64,
        local_center: Point3D,
        column_x: Vector3D,
        column_y: Vector3D,
        column_z: Vector3D,
    ) -> bool {
        self.0.set_inertia_matrix(
            body,
            mass,
            local_center.into(),
            column_x.into(),
            column_y.into(),
            column_z.into(),
        )
    }

    fn mass(&self, body: i64) -> f64 {
        self.0.mass(body)
    }

    fn center_of_mass(&self, body: i64) -> Point3D {
        self.0.center_of_mass(body).into()
    }

    fn local_center_of_mass(&self, body: i64) -> Point3D {
        self.0.local_center_of_mass(body).into()
    }

    fn principal_inertia(&self, body: i64) -> Vector3D {
        self.0.principal_inertia(body).into()
    }

    fn inertia_column(&self, body: i64, column: i64) -> Vector3D {
        self.0.inertia_column(body, column).into()
    }

    fn set_material(
        &self,
        body: i64,
        static_friction: f64,
        dynamic_friction: f64,
        restitution: f64,
    ) -> bool {
        self.0
            .set_material(body, static_friction, dynamic_friction, restitution)
    }

    fn static_friction(&self, body: i64) -> f64 {
        self.0.static_friction(body)
    }

    fn dynamic_friction(&self, body: i64) -> f64 {
        self.0.dynamic_friction(body)
    }

    fn restitution(&self, body: i64) -> f64 {
        self.0.restitution(body)
    }

    fn set_damping(&self, body: i64, linear: f64, angular: f64) -> bool {
        self.0.set_damping(body, linear, angular)
    }

    fn set_ccd_substeps(&self, maximum: i64) -> bool {
        self.0.set_ccd_substeps(maximum)
    }

    fn ccd_substeps(&self) -> i64 {
        self.0.ccd_substeps()
    }

    fn set_body_ccd(&self, body: i64, enabled: bool) -> bool {
        self.0.set_body_ccd(body, enabled)
    }

    fn body_ccd(&self, body: i64) -> bool {
        self.0.body_ccd(body)
    }

    fn set_soft_ccd(&self, body: i64, distance: f64) -> bool {
        self.0.set_soft_ccd(body, distance)
    }

    fn soft_ccd(&self, body: i64) -> f64 {
        self.0.soft_ccd(body)
    }

    fn set_length_unit(&self, length: f64) -> bool {
        self.0.set_length_unit(length)
    }

    fn length_unit(&self) -> f64 {
        self.0.length_unit()
    }

    fn set_gravity(&self, gravity: Vector3D) -> bool {
        self.0.set_gravity(gravity.into())
    }

    fn gravity(&self) -> Vector3D {
        self.0.gravity().into()
    }

    fn use_character_world_gravity(&self, body: i64) -> bool {
        self.0.use_character_world_gravity(body)
    }

    fn character_gravity(&self, body: i64) -> Vector3D {
        self.0.character_gravity(body).into()
    }

    fn set_axis_locks(&self, body: i64, linear: i64, angular: i64) -> bool {
        self.0.set_axis_locks(body, linear, angular)
    }

    fn is_sleeping(&self, body: i64) -> bool {
        self.0.is_sleeping(body)
    }

    fn add_character(&self, center: Point3D, radius: f64, segment_height: f64) -> i64 {
        self.0.add_character(center.into(), radius, segment_height)
    }

    fn set_character_movement(&self, body: i64, velocity: Vector3D) -> bool {
        self.0.set_character_movement(body, velocity.into())
    }

    fn set_character_gravity(&self, body: i64, gravity: Vector3D) -> bool {
        self.0.set_character_gravity(body, gravity.into())
    }

    fn set_character_mid_air_control(&self, body: i64, enabled: bool) -> bool {
        self.0.set_character_mid_air_control(body, enabled)
    }

    fn set_character_options(
        &self,
        body: i64,
        step_height: f64,
        snap_distance: f64,
        max_slope: f64,
        push_mass: f64,
    ) -> bool {
        self.0
            .set_character_options(body, step_height, snap_distance, max_slope, push_mass)
    }

    fn jump_character(&self, body: i64, speed: f64) -> bool {
        self.0.jump_character(body, speed)
    }

    fn teleport_character(&self, body: i64, center: Point3D) -> bool {
        self.0.teleport_character(body, center.into())
    }

    fn character_grounded(&self, body: i64) -> bool {
        self.0.character_grounded(body)
    }

    fn character_hit_wall(&self, body: i64) -> bool {
        self.0.character_hit_wall(body)
    }

    fn character_hit_ceiling(&self, body: i64) -> bool {
        self.0.character_hit_ceiling(body)
    }

    fn character_collisions(&self, body: i64) -> i64 {
        self.0.character_collisions(body)
    }

    fn character_support(&self, body: i64) -> i64 {
        self.0.character_support(body)
    }

    fn set_character_hit_reports(&self, body: i64, enabled: bool) -> bool {
        self.0.set_character_hit_reports(body, enabled)
    }

    fn character_hits(&self, body: i64) -> Vec<PhysicsContact3D> {
        self.0
            .character_hits(body)
            .into_iter()
            .map(PhysicsContact3D)
            .collect()
    }

    #[pyo3(signature = (origin, direction, max_distance, group_mask=4294967295, ignore_body=0, include_sensors=false))]
    fn ray_cast(
        &self,
        origin: &Point3D,
        direction: &Vector3D,
        max_distance: f64,
        group_mask: i64,
        ignore_body: i64,
        include_sensors: bool,
    ) -> PhysicsHit3D {
        PhysicsHit3D(self.0.ray_cast(
            origin.0,
            direction.0,
            max_distance,
            group_mask,
            ignore_body,
            include_sensors,
        ))
    }

    #[pyo3(signature = (origin, direction, max_distance, group_mask=4294967295, ignore_body=0, include_sensors=false))]
    fn ray_cast_all(
        &self,
        origin: &Point3D,
        direction: &Vector3D,
        max_distance: f64,
        group_mask: i64,
        ignore_body: i64,
        include_sensors: bool,
    ) -> Vec<PhysicsHit3D> {
        self.0
            .ray_cast_all(
                origin.0,
                direction.0,
                max_distance,
                group_mask,
                ignore_body,
                include_sensors,
            )
            .into_iter()
            .map(PhysicsHit3D)
            .collect()
    }

    #[pyo3(signature = (center, rotation, shapes, direction, max_distance, group_mask=4294967295, ignore_body=0, include_sensors=false))]
    fn shape_cast(
        &self,
        center: Point3D,
        rotation: Vector3D,
        shapes: PyRef<'_, RecordList>,
        direction: Vector3D,
        max_distance: f64,
        group_mask: i64,
        ignore_body: i64,
        include_sensors: bool,
    ) -> PhysicsHit3D {
        PhysicsHit3D(self.0.shape_cast(
            center.into(),
            rotation.into(),
            rt::PhysicsCollider3D::from_records(&shapes.0),
            direction.into(),
            max_distance,
            rt::PhysicsQueryFilter3D::new(group_mask, ignore_body, include_sensors),
        ))
    }

    #[pyo3(signature = (center, rotation, shapes, direction, max_distance, group_mask=4294967295, ignore_body=0, include_sensors=false))]
    fn shape_cast_all(
        &self,
        center: Point3D,
        rotation: Vector3D,
        shapes: PyRef<'_, RecordList>,
        direction: Vector3D,
        max_distance: f64,
        group_mask: i64,
        ignore_body: i64,
        include_sensors: bool,
    ) -> Vec<PhysicsHit3D> {
        self.0
            .shape_cast_all(
                center.into(),
                rotation.into(),
                rt::PhysicsCollider3D::from_records(&shapes.0),
                direction.into(),
                max_distance,
                rt::PhysicsQueryFilter3D::new(group_mask, ignore_body, include_sensors),
            )
            .into_iter()
            .map(PhysicsHit3D)
            .collect()
    }

    #[pyo3(signature = (center, rotation, shapes, group_mask=4294967295, ignore_body=0, include_sensors=false))]
    fn shape_overlap(
        &self,
        center: Point3D,
        rotation: Vector3D,
        shapes: PyRef<'_, RecordList>,
        group_mask: i64,
        ignore_body: i64,
        include_sensors: bool,
    ) -> Vec<i64> {
        self.0.shape_overlap(
            center.into(),
            rotation.into(),
            rt::PhysicsCollider3D::from_records(&shapes.0),
            rt::PhysicsQueryFilter3D::new(group_mask, ignore_body, include_sensors),
        )
    }

    fn remove(&self, body: i64) -> bool {
        self.0.remove(body)
    }
}

#[pyclass(frozen, eq, unsendable, from_py_object, module = "xrack._native")]
#[derive(Clone, PartialEq)]
struct WalkResult3D(rt::WalkResult3D);

owned_value!(PhysicsContact3D, rt::PhysicsContact3D);

#[pymethods]
impl PhysicsContact3D {
    #[new]
    fn new() -> Self {
        Self(rt::PhysicsContact3D::default())
    }

    #[getter]
    fn body(&self) -> i64 {
        self.0.body
    }

    #[getter]
    fn position(&self) -> Point3D {
        self.0.position.into()
    }

    #[getter]
    fn normal(&self) -> Vector3D {
        self.0.normal.into()
    }

    #[getter]
    fn impulse(&self) -> Vector3D {
        self.0.impulse.into()
    }

    #[getter]
    fn normal_impulse(&self) -> f64 {
        self.0.normal_impulse
    }
}

owned_value!(PhysicsHit3D, rt::PhysicsHit3D);

#[pymethods]
impl PhysicsHit3D {
    #[new]
    fn new() -> Self {
        Self(rt::PhysicsHit3D::default())
    }

    #[getter]
    fn body(&self) -> i64 {
        self.0.body
    }

    #[getter]
    fn position(&self) -> Point3D {
        self.0.position.into()
    }

    #[getter]
    fn normal(&self) -> Vector3D {
        self.0.normal.into()
    }

    #[getter]
    fn distance(&self) -> f64 {
        self.0.distance
    }

    #[getter]
    fn sensor(&self) -> bool {
        self.0.sensor
    }
}

#[pymethods]
impl WalkResult3D {
    #[new]
    #[pyo3(signature = (position, crossings, *, grounded = false, hit_wall = false, hit_ceiling = false, contacts = None, support = None))]
    fn new(
        position: Point3D,
        crossings: Vec<Hit3D>,
        grounded: bool,
        hit_wall: bool,
        hit_ceiling: bool,
        contacts: Option<Vec<Hit3D>>,
        support: Option<Hit3D>,
    ) -> Self {
        Self(rt::WalkResult3D {
            support: support.map(Into::into).unwrap_or_default(),
            collision_contacts: contacts
                .filter(|hits| !hits.is_empty())
                .map(|hits| hits.into_iter().map(Into::into).collect::<Vec<_>>().into()),
            grounded,
            hit_wall,
            hit_ceiling,
            position: position.into(),
            crossings: crossings
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
        })
    }

    #[getter]
    fn position(&self) -> Point3D {
        self.0.position.into()
    }

    #[getter]
    fn crossings(&self) -> Vec<Hit3D> {
        self.0.crossings.snapshot().into_iter().map(Hit3D).collect()
    }

    #[getter]
    fn contacts(&self) -> Vec<Hit3D> {
        self.0
            .collision_contacts
            .as_ref()
            .map_or_else(Vec::new, |contacts| {
                contacts.snapshot().into_iter().map(Hit3D).collect()
            })
    }

    #[getter]
    fn support(&self) -> Hit3D {
        self.0.support.clone().into()
    }

    #[getter]
    fn grounded(&self) -> bool {
        self.0.grounded
    }

    #[getter]
    fn hit_wall(&self) -> bool {
        self.0.hit_wall
    }

    #[getter]
    fn hit_ceiling(&self) -> bool {
        self.0.hit_ceiling
    }
}

struct_value!(ProjectedPoint3D {
    point: Point2D,
    depth: f64,
    valid: bool,
    inside: bool
});
struct_value!(Vector2D {
    x: Length,
    y: Length
});
owned_value!(Transform2D, rt::Transform2D);

#[pymethods]
impl Transform2D {
    #[new]
    fn new() -> Self {
        Self(rt::Transform2D::IDENTITY)
    }

    #[staticmethod]
    fn translation(x: Length, y: Length) -> Self {
        Self(rt::Transform2D::translation(x.0, y.0))
    }

    #[staticmethod]
    fn rotation(degrees: f64) -> Self {
        Self(rt::Transform2D::rotation(degrees))
    }

    #[staticmethod]
    fn scale(x: f64, y: f64) -> Self {
        Self(rt::Transform2D::scale(x, y))
    }

    fn then(&self, next: Self) -> Self {
        Self(self.0.then(next.0))
    }

    fn around(&self, point: Point2D) -> Self {
        Self(self.0.around(point.0))
    }

    fn map_point(&self, point: Point2D) -> Point2D {
        Point2D(self.0.map_point(point.0))
    }

    fn map_vector(&self, vector: Vector2D) -> Vector2D {
        Vector2D(self.0.map_vector(vector.0))
    }
}

struct_value!(KeyModifiers {
    control: bool,
    shift: bool,
    alt: bool,
    meta: bool,
    primary: bool
});
struct_value!(KeyEvent {
    key: String,
    text: String,
    modifiers: KeyModifiers,
    repeat: bool
});
struct_value!(WheelEvent {
    x: Length,
    y: Length,
    delta_x: Length,
    delta_y: Length,
    lines_x: f64,
    lines_y: f64,
    is_line: bool,
    modifiers: KeyModifiers,
});
struct_value!(PointerEvent {
    kind: PointerEventKind,
    button: PointerButton,
    x: Length,
    y: Length,
    inside: bool,
    is_touch: bool,
    touch_finger_id: i64,
    modifiers: KeyModifiers,
});
struct_value!(TouchPoint {
    id: i64,
    x: Length,
    y: Length,
    start_x: Length,
    start_y: Length,
    inside: bool,
});

owned_value!(Length, rt::Length);

#[pymethods]
impl Length {
    #[new]
    fn new(px: f64) -> Self {
        Self(rt::Length(px))
    }

    #[getter]
    fn px(&self) -> f64 {
        self.0.0
    }
}

owned_value!(Percent, rt::Percent);

#[pymethods]
impl Percent {
    #[new]
    fn new(percent: f64) -> Self {
        Self(rt::Percent(percent / 100.0))
    }

    #[getter]
    fn percent(&self) -> f64 {
        self.0.0 * 100.0
    }
}

owned_value!(Color, rt::Color);

#[pymethods]
impl Color {
    #[new]
    #[pyo3(signature = (red, green, blue, alpha = 255))]
    fn new(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self(rt::Color(u32::from_be_bytes([red, green, blue, alpha])))
    }

    #[staticmethod]
    fn from_hex(value: &str) -> PyResult<Self> {
        rt::Color::parse_hex_literal(value)
            .map(Self)
            .ok_or_else(|| PyValueError::new_err("expected an XRack hex color"))
    }

    #[getter]
    fn rgba(&self) -> (u8, u8, u8, u8) {
        let [red, green, blue, alpha] = self.0.0.to_be_bytes();
        (red, green, blue, alpha)
    }

    fn __str__(&self) -> String {
        format!("#{:08x}", self.0.0)
    }
}

struct_value!(GradientStop {
    color: Color,
    offset: f64
});
owned_value!(Brush, rt::Brush);

#[pymethods]
impl Brush {
    #[new]
    fn new(color: Color) -> Self {
        Self(rt::Brush::Solid(color.0))
    }

    #[staticmethod]
    fn linear(angle: f64, stops: Vec<PyRef<'_, GradientStop>>) -> PyResult<Self> {
        if !angle.is_finite() {
            return Err(PyValueError::new_err("gradient angle must be finite"));
        }
        Ok(Self(rt::Brush::Linear {
            angle_degrees: angle,
            stops: gradient_stops(stops)?,
        }))
    }

    #[staticmethod]
    fn radial(stops: Vec<PyRef<'_, GradientStop>>) -> PyResult<Self> {
        Ok(Self(rt::Brush::Radial {
            stops: gradient_stops(stops)?,
        }))
    }

    #[getter]
    fn kind(&self) -> &'static str {
        match self.0 {
            rt::Brush::Solid(_) => "solid",
            rt::Brush::Linear { .. } => "linear",
            rt::Brush::Radial { .. } => "radial",
        }
    }

    #[getter]
    fn color(&self) -> Option<Color> {
        match self.0 {
            rt::Brush::Solid(color) => Some(Color(color)),
            _ => None,
        }
    }

    #[getter]
    fn angle(&self) -> Option<f64> {
        match self.0 {
            rt::Brush::Linear { angle_degrees, .. } => Some(angle_degrees),
            _ => None,
        }
    }

    #[getter]
    fn stops(&self) -> Vec<GradientStop> {
        match &self.0 {
            rt::Brush::Solid(_) => Vec::new(),
            rt::Brush::Linear { stops, .. } | rt::Brush::Radial { stops } => {
                stops.iter().copied().map(GradientStop).collect()
            }
        }
    }
}

fn gradient_stops(
    stops: Vec<PyRef<'_, GradientStop>>,
) -> PyResult<std::sync::Arc<[rt::GradientStop]>> {
    let stops: Vec<_> = stops.iter().map(|stop| stop.0).collect();
    if stops.len() < 2
        || stops.iter().any(|stop| !(0.0..=1.0).contains(&stop.offset))
        || stops.windows(2).any(|pair| pair[0].offset > pair[1].offset)
    {
        return Err(PyValueError::new_err(
            "gradient requires two or more ordered stops in [0, 1]",
        ));
    }
    Ok(stops.into())
}

owned_value!(Date, rt::Date);
owned_value!(Time, rt::Time);

#[pymethods]
impl Date {
    #[new]
    fn new(year: i64, month: i64, day: i64) -> PyResult<Self> {
        rt::Date::new(year, month, day)
            .map(Self)
            .ok_or_else(|| PyValueError::new_err("invalid date"))
    }

    #[getter]
    fn year(&self) -> i64 {
        self.0.year
    }
    #[getter]
    fn month(&self) -> i64 {
        self.0.month
    }
    #[getter]
    fn day(&self) -> i64 {
        self.0.day
    }
    fn __str__(&self) -> String {
        self.0.to_string()
    }
}

#[pymethods]
impl Time {
    #[new]
    #[pyo3(signature = (hour, minute, second = 0))]
    fn new(hour: i64, minute: i64, second: i64) -> PyResult<Self> {
        rt::Time::new(hour, minute, second)
            .map(Self)
            .ok_or_else(|| PyValueError::new_err("invalid time"))
    }

    #[getter]
    fn hour(&self) -> i64 {
        self.0.hour
    }
    #[getter]
    fn minute(&self) -> i64 {
        self.0.minute
    }
    #[getter]
    fn second(&self) -> i64 {
        self.0.second
    }
    fn __str__(&self) -> String {
        self.0.to_string()
    }
}

owned_value!(StyledText, rt::StyledText);

#[pymethods]
impl StyledText {
    #[new]
    fn new(text: String) -> Self {
        Self(rt::StyledText::from_plain_text(text))
    }

    #[staticmethod]
    fn from_markdown(source: &str) -> PyResult<Self> {
        rt::StyledText::from_markdown(source)
            .map(Self)
            .map_err(as_value_error)
    }

    #[getter]
    fn text(&self) -> &str {
        self.0.text()
    }

    #[getter]
    fn links(&self) -> Vec<(usize, usize, String)> {
        self.0
            .links()
            .iter()
            .map(|link| (link.start, link.end, link.href.to_string()))
            .collect()
    }
}

#[pyclass(frozen, eq, unsendable, skip_from_py_object, module = "xrack._native")]
#[derive(Clone, PartialEq)]
struct Image(rt::ImageAsset);

#[pymethods]
impl Image {
    #[new]
    fn new() -> Self {
        Self(rt::ImageAsset::default())
    }

    #[staticmethod]
    fn from_file(path: PathBuf) -> PyResult<Self> {
        let format = match path.extension().and_then(|ext| ext.to_str()) {
            Some(ext) if ext.eq_ignore_ascii_case("png") => rt::ImageFormat::Png,
            Some(ext) if ext.eq_ignore_ascii_case("svg") => rt::ImageFormat::Svg,
            _ => return Err(PyValueError::new_err("expected a PNG or SVG file")),
        };
        let bytes = std::fs::read(path)?;
        let size = xrack_assets::image_size(format, &bytes).map_err(as_value_error)?;
        Ok(Self(rt::ImageAsset::Loaded(rt::LoadedImage::new(
            format, bytes, size,
        ))))
    }

    #[getter]
    fn size(&self) -> (u32, u32) {
        let [width, height] = self.0.size();
        (width, height)
    }

    #[getter]
    fn frame_count(&self) -> usize {
        self.0.frame_count()
    }
}

fn as_value_error(error: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(error.to_string())
}

owned_value!(PointerButtons, rt::PointerButtons);

#[pymethods]
impl PointerButtons {
    #[new]
    fn new(buttons: Vec<PointerButton>) -> Self {
        Self(buttons.into_iter().map(rt::PointerButton::from).collect())
    }

    fn __contains__(&self, button: PointerButton) -> bool {
        self.0.contains(button.into())
    }
}

#[pyclass(frozen, eq, unsendable, skip_from_py_object, module = "xrack._native")]
#[derive(Clone, PartialEq)]
struct TouchPoints(rt::TouchPoints);

#[pymethods]
impl TouchPoints {
    #[new]
    fn new(points: Vec<PyRef<'_, TouchPoint>>) -> Self {
        Self(
            points
                .iter()
                .map(|point| point.0)
                .collect::<Vec<_>>()
                .into(),
        )
    }

    fn snapshot(&self) -> Vec<TouchPoint> {
        self.0.snapshot().into_iter().map(TouchPoint).collect()
    }
}

#[pyclass(frozen, eq, unsendable, skip_from_py_object, module = "xrack._native")]
#[derive(Clone, PartialEq)]
struct Record(rt::RecordValue);

#[pymethods]
impl Record {
    #[new]
    fn new(name: String, fields: BTreeMap<String, PythonValue>) -> Self {
        Self(rt::RecordValue::new(
            name,
            fields
                .into_iter()
                .map(|(name, value)| (name, value.0))
                .collect(),
        ))
    }

    #[getter]
    fn name(&self) -> &str {
        self.0.name()
    }

    fn __getitem__(&self, field: &str) -> PyResult<PythonValue> {
        self.0
            .field(field)
            .cloned()
            .map(PythonValue)
            .ok_or_else(|| PyKeyError::new_err(field.to_owned()))
    }

    fn fields(&self) -> BTreeMap<String, PythonValue> {
        self.0
            .fields()
            .map(|(name, value)| (name.to_owned(), PythonValue(value.clone())))
            .collect()
    }
}

#[pyclass(frozen, unsendable, skip_from_py_object, module = "xrack._native")]
#[derive(Clone)]
struct RecordList(rt::RecordList);

type ValidateRow = dyn Fn(&rt::RecordValue) -> PyResult<()>;

#[pyclass(frozen, unsendable, module = "xrack._native")]
pub(super) struct RecordModel {
    model: rt::RecordList,
    validate: Box<ValidateRow>,
}

impl RecordModel {
    pub(super) fn new(
        model: rt::RecordList,
        validate: impl Fn(&rt::RecordValue) -> PyResult<()> + 'static,
    ) -> Self {
        Self {
            model,
            validate: Box::new(validate),
        }
    }
}

#[pymethods]
impl RecordModel {
    fn __len__(&self) -> usize {
        self.model.model().len()
    }

    fn __getitem__(&self, index: i64) -> PyResult<Record> {
        let model = self.model.model();
        model
            .row_at(row_index(model, index)? as i64)
            .map(Record)
            .ok_or_else(row_error)
    }

    fn __setitem__(&self, index: i64, row: PyRef<'_, Record>) -> PyResult<()> {
        let model = self.model.model();
        let index = row_index(model, index)?;
        (self.validate)(&row.0)?;
        model.set_row(index, row.0.clone());
        Ok(())
    }

    fn __delitem__(&self, index: i64) -> PyResult<()> {
        let model = self.model.model();
        model.remove(row_index(model, index)?);
        Ok(())
    }

    fn append(&self, row: PyRef<'_, Record>) -> PyResult<()> {
        (self.validate)(&row.0)?;
        self.model.model().push(row.0.clone());
        Ok(())
    }
}

#[pymethods]
impl RecordList {
    #[new]
    fn new(name: String, rows: Vec<PyRef<'_, Record>>) -> PyResult<Self> {
        if rows.iter().any(|row| row.0.name() != name) {
            return Err(PyTypeError::new_err(format!(
                "all rows must be records of type {name}"
            )));
        }
        Ok(Self(rt::RecordList::new(
            name,
            rows.iter().map(|row| row.0.clone()).collect(),
        )))
    }

    #[getter]
    fn name(&self) -> &str {
        self.0.name()
    }

    fn __len__(&self) -> usize {
        self.0.model().len()
    }

    fn __getitem__(&self, index: i64) -> PyResult<Record> {
        let index = row_index(self.0.model(), index)?;
        self.0
            .model()
            .row_at(index as i64)
            .map(Record)
            .ok_or_else(row_error)
    }
}

#[pyclass(frozen, unsendable, skip_from_py_object, module = "xrack._native")]
#[derive(Clone)]
pub(super) struct StringModel(pub(super) rt::VecModel<String>);

#[pymethods]
impl StringModel {
    #[new]
    fn new(rows: Vec<String>) -> Self {
        Self(rows.into())
    }

    fn __len__(&self) -> usize {
        self.0.len()
    }

    fn __getitem__(&self, index: i64) -> PyResult<String> {
        let index = row_index(&self.0, index)?;
        self.0.row_at(index as i64).ok_or_else(row_error)
    }

    fn __setitem__(&self, index: i64, value: String) -> PyResult<()> {
        let index = row_index(&self.0, index)?;
        self.0.set_row(index, value);
        Ok(())
    }

    fn __delitem__(&self, index: i64) -> PyResult<()> {
        let index = row_index(&self.0, index)?;
        self.0.remove(index);
        Ok(())
    }

    fn append(&self, value: String) {
        self.0.push(value);
    }

    fn snapshot(&self) -> Vec<String> {
        self.0.snapshot()
    }
}

fn row_index<T: Clone + PartialEq + 'static>(
    model: &rt::VecModel<T>,
    index: i64,
) -> PyResult<usize> {
    let len = model.len() as i64;
    let index = if index < 0 { len + index } else { index };
    if (0..len).contains(&index) {
        Ok(index as usize)
    } else {
        Err(row_error())
    }
}

fn row_error() -> PyErr {
    PyIndexError::new_err("XRack model index out of range")
}

macro_rules! bridge_values {
    (
        structs { $($tag:ident => $wrapper:ident),+ $(,)? }
        enums { $($name:ident { $($(#[$meta:meta])* $variant:ident),+ $(,)? }),+ $(,)? }
    ) => {
        $(
            #[pyclass(eq, frozen, from_py_object, module = "xrack._native")]
            #[derive(Clone, Copy, PartialEq)]
            enum $name { $($(#[$meta])* $variant),+ }

            impl From<$name> for rt::$name {
                fn from(value: $name) -> Self {
                    match value { $($name::$variant => Self::$variant),+ }
                }
            }

            impl From<rt::$name> for $name {
                fn from(value: rt::$name) -> Self {
                    match value { $(rt::$name::$variant => Self::$variant),+ }
                }
            }
        )+

        const VALUE_TYPE: PyStaticExpr = type_hint_subscript!(
            type_hint_identifier!("typing", "Union"),
            PyBool::TYPE_HINT,
            PyInt::TYPE_HINT,
            PyFloat::TYPE_HINT,
            PyString::TYPE_HINT,
            type_hint_subscript!(PyList::TYPE_HINT, PyString::TYPE_HINT),
            type_hint_subscript!(PyList::TYPE_HINT, PyInt::TYPE_HINT),
            type_hint_subscript!(PyList::TYPE_HINT, Date::TYPE_HINT),
            type_hint_subscript!(PyList::TYPE_HINT, Hit3D::TYPE_HINT),
            type_hint_subscript!(PyList::TYPE_HINT, PhysicsContact3D::TYPE_HINT),
            type_hint_subscript!(PyList::TYPE_HINT, PhysicsHit3D::TYPE_HINT),
            $($wrapper::TYPE_HINT,)+
            $($name::TYPE_HINT),+
        );

        fn decode(value: &Bound<'_, PyAny>) -> PyResult<HostValue> {
            if value.is_instance_of::<PyBool>() {
                return Ok(HostValue::Bool(value.extract()?));
            }
            if value.is_instance_of::<PyInt>() {
                return Ok(HostValue::Int(value.extract()?));
            }
            if value.is_instance_of::<PyFloat>() {
                return Ok(HostValue::Float(value.extract()?));
            }
            if value.is_instance_of::<PyString>() {
                return Ok(HostValue::String(value.extract()?));
            }
            if value.is_instance_of::<PyList>() {
                let list = value.cast::<PyList>()?;
                if !list.is_empty() && list.iter().all(|item| {
                    item.is_instance_of::<PyInt>() && !item.is_instance_of::<PyBool>()
                }) {
                    return Ok(HostValue::IntList(value.extract()?));
                }
                if !list.is_empty() && list.iter().all(|item| item.is_instance_of::<Date>()) {
                    let dates = list.iter().map(|item| Ok(item.extract::<PyRef<'_, Date>>()?.0))
                        .collect::<PyResult<Vec<_>>>()?;
                    return Ok(HostValue::DateList(dates));
                }
                if !list.is_empty() && list.iter().all(|item| item.is_instance_of::<Hit3D>()) {
                    let hits = list.iter().map(|item| Ok(item.extract::<PyRef<'_, Hit3D>>()?.0.clone()))
                        .collect::<PyResult<Vec<_>>>()?;
                    return Ok(HostValue::Hit3DList(hits.into()));
                }
                if !list.is_empty() && list.iter().all(|item| item.is_instance_of::<PhysicsContact3D>()) {
                    let contacts = list.iter().map(|item| Ok(item.extract::<PyRef<'_, PhysicsContact3D>>()?.0))
                        .collect::<PyResult<Vec<_>>>()?;
                    return Ok(HostValue::PhysicsContact3DList(contacts.into()));
                }
                if !list.is_empty() && list.iter().all(|item| item.is_instance_of::<PhysicsHit3D>()) {
                    let contacts = list.iter().map(|item| Ok(item.extract::<PyRef<'_, PhysicsHit3D>>()?.0))
                        .collect::<PyResult<Vec<_>>>()?;
                    return Ok(HostValue::PhysicsHit3DList(contacts.into()));
                }
                return Ok(HostValue::StringList(value.extract()?));
            }
            $(
                if value.is_instance_of::<$wrapper>() {
                    return Ok(HostValue::$tag(value.extract::<PyRef<'_, $wrapper>>()?.0.clone()));
                }
            )+
            $(
                if value.is_instance_of::<$name>() {
                    return Ok(HostValue::$name(value.extract::<$name>()?.into()));
                }
            )+
            Err(PyTypeError::new_err("expected a Python XRack value"))
        }

        fn encode(py: Python<'_>, value: HostValue) -> PyResult<Bound<'_, PyAny>> {
            macro_rules! convert {
                ($value:expr) => {
                    Ok($value.into_pyobject(py)?.into_any())
                };
            }
            match value {
                HostValue::Bool(value) => Ok(value.into_pyobject(py)?.to_owned().into_any()),
                HostValue::Int(value) => convert!(value),
                HostValue::Float(value) => convert!(value),
                HostValue::String(value) => convert!(value),
                HostValue::StringList(value) => convert!(value),
                HostValue::IntList(value) => convert!(value),
                HostValue::IntModel(model) => convert!(model.into_snapshot()),
                HostValue::DateList(dates) => convert!(dates.into_iter().map(Date).collect::<Vec<_>>()),
                HostValue::DateModel(model) => convert!(model.into_snapshot().into_iter().map(Date).collect::<Vec<_>>()),
                HostValue::Hit3DList(model) => convert!(model.into_snapshot().into_iter().map(Hit3D).collect::<Vec<_>>()),
                HostValue::PhysicsContact3DList(model) => convert!(model.into_snapshot().into_iter().map(PhysicsContact3D).collect::<Vec<_>>()),
                HostValue::PhysicsHit3DList(model) => convert!(model.into_snapshot().into_iter().map(PhysicsHit3D).collect::<Vec<_>>()),
                HostValue::Image(value) => convert!(Image(value.into())),
                // The combo projection is renderer data and has no public ValueType.
                HostValue::ComboModel(_) => Err(PyTypeError::new_err("invalid public XRack combo projection")),
                HostValue::SceneLights(_) | HostValue::SceneEnvironment(_) | HostValue::SceneReflections(_) | HostValue::PhysicsDebugLines(_) | HostValue::SceneFogVolumes(_) | HostValue::SceneEffects(_) | HostValue::SceneCaptures(_) | HostValue::SceneParticleEmitters(_) | HostValue::SceneParticleAffectors(_) => Err(PyTypeError::new_err("invalid public XRack scene projection")),
                $(HostValue::$tag(value) => convert!($wrapper(value)),)+
                $(HostValue::$name(value) => convert!($name::from(value)),)+
            }
        }

        #[pymodule]
        mod _native {
            #[pymodule_export]
            use super::{GradientStop, RecordModel, $($wrapper,)+ $($name),+};
            #[pymodule_export]
            use crate::{PyComponent, PyProgram, call_on_ui, load_file, load_mlir_file, post};
        }
    };
}

bridge_values! {
    structs {
        Brush => Brush,
        Color => Color,
        Date => Date,
        Font => Font,
        ImageAsset => Image,
        KeyEvent => KeyEvent,
        KeyModifiers => KeyModifiers,
        Length => Length,
        Percent => Percent,
        Point2D => Point2D,
        Point3D => Point3D,
        Vector3D => Vector3D,
        Quaternion3D => Quaternion3D,
        RotationTrack3D => RotationTrack3D,
        Hit3D => Hit3D,
        Instance3D => Instance3D,
        InstanceTable3D => InstanceTable3D,
        ProjectedPoint3D => ProjectedPoint3D,
        WalkResult3D => WalkResult3D,
        PhysicsWorld3D => PhysicsWorld3D,
        SceneObjects => SceneModel3D,
        PhysicsContact3D => PhysicsContact3D,
        PhysicsHit3D => PhysicsHit3D,
        PointerButtons => PointerButtons,
        PointerEvent => PointerEvent,
        Record => Record,
        RecordList => RecordList,
        StringModel => StringModel,
        StyledText => StyledText,
        Time => Time,
        TouchPoint => TouchPoint,
        TouchPoints => TouchPoints,
        Transform2D => Transform2D,
        Vector2D => Vector2D,
        WheelEvent => WheelEvent,
    }
    enums {
        AccessibleLive { Off, Polite, Assertive },
        ButtonDisplay { IconOnly, TextOnly, TextBesideIcon, TextUnderIcon },
        CheckState { Unchecked, PartiallyChecked, Checked },
        ColorScheme { System, Light, Dark },
        SceneToneMap { #[pyo3(name = "None_")] None, Linear, Reinhard, Aces, HejlDawson, Filmic },
        CrossAxisAlignment { Auto, Stretch, Start, End, Center },
        CursorShape { Auto, Default, Pointer, Text, Crosshair, Move, Grab, Grabbing, NotAllowed, Wait, Help, ResizeHorizontal, ResizeVertical, ZoomIn, ZoomOut },
        DialInputMode { Circular, Horizontal, Vertical },
        DialogRole { Accept, Reject },
        DragAction { #[pyo3(name = "None_")] None, Copy, Move, Link },
        DrawerEdge { Left, Right, Top, Bottom },
        EventResult { Accept, Reject },
        FileDialogMode { OpenFile, OpenFiles, SaveFile, SelectFolder, SelectFolders },
        FlexboxLayoutDirection { Row, RowReverse, Column, ColumnReverse },
        FlexboxLayoutWrap { Wrap, NoWrap, WrapReverse },
        FlickBoundsBehavior { StopAtBounds, DragOverBounds, OvershootBounds, DragAndOvershootBounds },
        FlickBoundsMovement { FollowBoundsBehavior, StopAtBounds },
        FlickDirection { Both, Horizontal, Vertical },
        FocusReason { Programmatic, TabNavigation, PointerClick, PopupActivation, WindowActivation, Accessibility, LayoutChange, KeyboardNavigation },
        ImageFit { Fill, Contain, Cover, Preserve },
        ImageHorizontalAlignment { Left, Center, Right },
        ImageRendering { Smooth, Pixelated },
        ImageTileMode { Stretch, Repeat, Round },
        ImageVerticalAlignment { Top, Center, Bottom },
        InputType { Text, Password, Number, Decimal, Search, Email, Url, Tel },
        InputPurpose { Auto, Username, CurrentPassword, NewPassword, OneTimeCode },
        KeyboardCapitalization { Auto, #[pyo3(name = "None_")] None, Words, Sentences, Characters },
        ReturnKey { Auto, Next, Done },
        LayoutAlignment { Stretch, Center, Start, End, SpaceBetween, SpaceAround, SpaceEvenly },
        LayoutDirection { LeftToRight, RightToLeft },
        LogLevel { Trace, Debug, Info, Warn, Error },
        MessageButtons { Ok, OkCancel, YesNo, YesNoCancel },
        MessageLevel { Info, Warning, Error },
        MouseSelectionMode { Characters, Words, Caret },
        InputValidity { Invalid, Intermediate, Acceptable },
        Orientation { Horizontal, Vertical },
        PointerButton { Primary, Secondary, Middle },
        PointerEventKind { Down, Move, Up, Cancel },
        PopupPlacement { Below, Above, Left, Right, Offset },
        ScrollAlignment { Beginning, Center, End, Visible, Contain },
        ScrollBarPolicy { AsNeeded, AlwaysOn, AlwaysOff },
        ScrollSnapMode { #[pyo3(name = "None_")] None, Always, OnRelease },
        SelectionTarget { Rows, Cells, Columns },
        SelectionGesture { Auto, Drag, PressAndHold },
        StandardButtonKind { Ok, Open, Save, SaveAll, Cancel, Close, Apply, Reset, RestoreDefaults, Help, Yes, YesToAll, No, NoToAll, Abort, Retry, Ignore, Discard },
        SwipeSide { Closed, Left, Right },
        TextHorizontalAlignment { Start, End, Left, Center, Right },
        TextOverflow { Clip, Elide },
        TextVerticalAlignment { Top, Center, Bottom },
        TextWrap { WordWrap, NoWrap, CharWrap },
    }
}
