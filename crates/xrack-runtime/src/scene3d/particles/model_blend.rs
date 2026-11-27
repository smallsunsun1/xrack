//! Analytic triangle particle motion shared by scene queries and GPU rendering.
use std::sync::Arc;

use nalgebra::{Matrix3, Quaternion, Unit, UnitQuaternion, Vector3};

use crate::{
    Point3D, Quaternion3D, RecordValue, Value, Vector3D,
    scene3d::{MeshVertex, hierarchy::SceneTransform},
};

#[doc(hidden)]
pub struct ModelBlendDefinition<'a> {
    pub mode: &'a str,
    pub order: &'a str,
    pub end_node: &'a str,
    pub activation_node: &'a str,
    pub activation_speed: f64,
    pub time: f64,
    pub duration: f64,
    pub stagger: f64,
    pub arrival: f64,
    pub origin: Point3D,
    pub offset: Vector3D,
    pub orientation: Quaternion3D,
    pub scale: Vector3D,
    pub velocity: Vector3D,
    pub spread: Vector3D,
    pub gravity: Vector3D,
    pub spin: Vector3D,
    pub axis: Vector3D,
    pub seed: i64,
}

impl Default for ModelBlendDefinition<'_> {
    fn default() -> Self {
        Self {
            mode: "",
            order: "sequential",
            end_node: "",
            activation_node: "",
            activation_speed: 1.0,
            time: 0.0,
            duration: 1.0,
            stagger: 0.0,
            arrival: 0.35,
            origin: Point3D::default(),
            offset: Vector3D::default(),
            orientation: Quaternion3D::default(),
            scale: Vector3D::new(1.0, 1.0, 1.0),
            velocity: Vector3D::default(),
            spread: Vector3D::default(),
            gravity: Vector3D::default(),
            spin: Vector3D::default(),
            axis: Vector3D::new(0.0, 0.0, 1.0),
            seed: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
enum Mode {
    Explode = 1,
    Construct,
    Transfer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
enum Order {
    Sequential,
    Random,
    Activation,
    Plane,
}

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct ModelBlend {
    motion: BlendMotion,
    anchors: Option<Box<BlendAnchors>>,
}

#[derive(Clone, Debug, PartialEq)]
struct BlendAnchors {
    end_name: Arc<str>,
    activation_name: Arc<str>,
    speed: f32,
    end: Option<SceneTransform>,
    activation: Option<SceneTransform>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BlendMotion {
    mode: Mode,
    order: Order,
    clock: [f32; 4],
    origin: [f32; 3],
    offset: [f32; 3],
    target: [f32; 4],
    scale: [f32; 3],
    velocity: [f32; 3],
    spread: [f32; 3],
    gravity: [f32; 3],
    spin: [f32; 4],
    axis: [f32; 3],
    seed: u32,
}

impl ModelBlendDefinition<'_> {
    pub fn prepare(self) -> Option<ModelBlend> {
        let mode = match self.mode {
            "explode" => Mode::Explode,
            "construct" => Mode::Construct,
            "transfer" => Mode::Transfer,
            _ => return None,
        };
        let order = match self.order {
            "" | "sequential" => Order::Sequential,
            "random" => Order::Random,
            "activation" => Order::Activation,
            _ => return None,
        };
        let clock = [self.time, self.duration, self.stagger, self.arrival];
        if !clock.iter().all(|arg| arg.is_finite()) || self.duration <= 0.0 {
            return None;
        }
        let [time, duration, stagger, arrival] = clock.map(|arg| arg.clamp(-1e4, 1e4) as f32);
        let duration = duration.max(1e-4);
        let spin = Vector3::from(checked_vector(self.spin)?);
        let speed = spin.norm();
        let [x, y, z] = spin.try_normalize(1e-8).unwrap_or_else(Vector3::z).into();
        let q = UnitQuaternion::from_rotation_matrix(&self.orientation.rotation_f32());
        let motion = BlendMotion {
            mode,
            order,
            clock: [
                time,
                duration,
                stagger.max(0.0),
                arrival.clamp(1e-4, duration),
            ],
            origin: checked_vector(Vector3D::new(self.origin.x, self.origin.y, self.origin.z))?,
            offset: checked_vector(self.offset)?,
            target: q.coords.into(),
            scale: checked_vector(self.scale)?.map(|value| value.clamp(1e-4, 1e4)),
            velocity: checked_vector(self.velocity)?,
            spread: checked_vector(self.spread)?.map(f32::abs),
            gravity: checked_vector(self.gravity)?,
            spin: [x, y, z, speed.to_radians()],
            axis: Vector3::from(checked_vector(self.axis)?)
                .try_normalize(1e-8)
                .unwrap_or_else(Vector3::z)
                .into(),
            seed: self.seed as u32,
        };
        let anchors = (!self.end_node.is_empty() || !self.activation_node.is_empty()).then(|| {
            Box::new(BlendAnchors {
                end_name: self.end_node.into(),
                activation_name: self.activation_node.into(),
                speed: if self.activation_speed.is_finite() {
                    self.activation_speed.clamp(0.0, 1e5) as f32
                } else {
                    0.0
                },
                end: None,
                activation: None,
            })
        });
        Some(ModelBlend { motion, anchors })
    }
}

fn checked_vector(value: Vector3D) -> Option<[f32; 3]> {
    let axes = [value.x, value.y, value.z];
    axes.iter()
        .all(|axis| (-1e5..=1e5).contains(axis))
        .then(|| axes.map(|axis| axis as f32))
}

#[doc(hidden)]
pub fn record_model_blend(row: &RecordValue) -> Option<ModelBlend> {
    let Value::String(mode) = row.field("model-blend-mode")? else {
        return None;
    };
    let base = ModelBlendDefinition::default();
    let scalar = |name, fallback| match row.field(name) {
        Some(Value::Float(value)) => *value,
        _ => fallback,
    };
    let vector = |name, fallback| match row.field(name) {
        Some(Value::Vector3D(value)) => *value,
        _ => fallback,
    };
    let string = |name| match row.field(name) {
        Some(Value::String(value)) => value.as_str(),
        _ => "",
    };
    ModelBlendDefinition {
        mode,
        end_node: string("model-blend-end-node"),
        activation_node: string("model-blend-activation-node"),
        activation_speed: scalar("model-blend-activation-speed", base.activation_speed),
        order: match row.field("model-blend-order") {
            Some(Value::String(order)) => order,
            _ => base.order,
        },
        time: scalar("model-blend-time", base.time),
        duration: scalar("model-blend-duration", base.duration),
        stagger: scalar("model-blend-stagger", base.stagger),
        arrival: scalar("model-blend-arrival", base.arrival),
        origin: match row.field("model-blend-origin") {
            Some(Value::Point3D(value)) => *value,
            _ => base.origin,
        },
        offset: vector("model-blend-offset", base.offset),
        orientation: match row.field("model-blend-orientation") {
            Some(Value::Quaternion3D(value)) => *value,
            _ => base.orientation,
        },
        scale: vector("model-blend-scale", base.scale),
        velocity: vector("model-blend-velocity", base.velocity),
        spread: vector("model-blend-spread", base.spread),
        gravity: vector("model-blend-gravity", base.gravity),
        spin: vector("model-blend-spin", base.spin),
        axis: vector("model-blend-axis", base.axis),
        seed: match row.field("model-blend-seed") {
            Some(Value::Int(seed)) => *seed,
            _ => base.seed,
        },
    }
    .prepare()
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelBlendPose {
    model: BlendMotion,
    triangles: u32,
    plane: [f32; 2],
    last_release: f32,
    affine: Option<SceneTransform>,
}

impl ModelBlend {
    pub(crate) fn uses_scene_nodes(&self) -> bool {
        self.anchors.is_some()
    }

    pub(crate) fn bind_scene_nodes(&mut self, resolve: impl Fn(&str) -> Option<SceneTransform>) {
        if let Some(anchors) = &mut self.anchors {
            anchors.end = resolve(&anchors.end_name);
            anchors.activation = resolve(&anchors.activation_name);
        }
    }

    pub(crate) fn pose(
        &self,
        bounds: ([f32; 3], [f32; 3]),
        triangles: u32,
        source: SceneTransform,
        row: SceneTransform,
    ) -> Option<ModelBlendPose> {
        let mut model = self.motion;
        let [time, duration, stagger, _] = model.clock;
        if triangles == 0 || model.mode != Mode::Construct && time <= 0.0 {
            return None;
        }
        let limits = |axis: [f32; 3]| {
            let (min, max) = bounds;
            std::array::from_fn::<_, 2, _>(|side| {
                (0..3)
                    .map(|index| {
                        let sign = usize::from(axis[index] < 0.0);
                        [min, max][side ^ sign][index] * axis[index]
                    })
                    .sum::<f32>()
            })
        };
        let mut last_release = stagger;
        let mut plane = [0.0, 0.0];
        if model.order == Order::Activation {
            let activation = self.anchors.as_ref().and_then(|anchors| {
                Some((source.relative_to(anchors.activation?)?, anchors.speed))
            });
            if let Some((frame, speed)) = activation {
                let [x, y, z, bias] = frame.rows()[2];
                model.order = Order::Plane;
                model.axis = [x, y, z];
                let nearest = limits(model.axis)[0] + bias;
                let inverse = if speed > 0.0 {
                    speed.max(1e-6).recip()
                } else {
                    0.0
                };
                plane = [bias, inverse];
                last_release = if inverse > 0.0 {
                    (-nearest).max(0.0) * inverse
                } else if nearest > 0.0 {
                    0.0
                } else {
                    f32::INFINITY
                };
            } else {
                let [start, end] = limits(model.axis);
                plane = [start, (end - start).max(1e-6).recip()];
            }
        }
        let affine = self.anchors.as_ref().and_then(|anchors| {
            let end = anchors.end?;
            if model.mode == Mode::Explode {
                return None;
            }
            // Keep the imported submesh's transform relative to the authored row.
            end.child(source.relative_to(row)?)?.relative_to(source)
        });
        let last_active = if last_release.is_finite() {
            duration + last_release
        } else {
            duration
        };
        model.clock[0] = time.clamp(-1.0, last_active);
        Some(ModelBlendPose {
            model,
            triangles,
            plane,
            last_release,
            affine,
        })
    }
}

impl ModelBlendPose {
    /// A fully assembled mesh uses the ordinary indexed mesh path, even when
    /// the first displayed time is already beyond the effect.
    pub(crate) fn settled_transform(self) -> Option<SceneTransform> {
        let [time, duration, _, _] = self.model.clock;
        if self.model.mode == Mode::Explode || time < duration + self.last_release {
            return None;
        }
        if let Some(affine) = self.affine {
            return Some(affine);
        }
        let [x, y, z, w] = self.model.target;
        let rotation =
            UnitQuaternion::new_unchecked(Quaternion::new(w, x, y, z)).to_rotation_matrix();
        let matrix = rotation.matrix();
        SceneTransform::from_instance_rows(std::array::from_fn(|row| {
            [
                matrix[(row, 0)] * self.model.scale[0],
                matrix[(row, 1)] * self.model.scale[1],
                matrix[(row, 2)] * self.model.scale[2],
                self.model.offset[row],
            ]
        }))
    }

    pub fn has_affine_destination(self) -> bool {
        self.affine.is_some()
    }

    pub fn triangles(self) -> u32 {
        self.triangles
    }

    pub fn uniforms(self) -> [[u32; 4]; 15] {
        let model = self.model;
        let offset = self
            .affine
            .map_or(model.offset, |affine| affine.origin.into());
        let linear = self
            .affine
            .map_or_else(Matrix3::identity, |affine| affine.linear);
        let words =
            |vector: [f32; 3], extra| [vector[0], vector[1], vector[2], extra].map(f32::to_bits);
        [
            model.clock.map(f32::to_bits),
            [
                model.mode as u32,
                model.order as u32,
                model.seed,
                self.triangles,
            ],
            words(model.origin, 0.0),
            words(offset, 0.0),
            words(model.velocity, 0.0),
            words(model.spread, 0.0),
            words(model.gravity, 0.0),
            model.spin.map(f32::to_bits),
            model.target.map(f32::to_bits),
            words(model.scale, 0.0),
            words(model.axis, self.plane[0]),
            [self.plane[1].to_bits(), 0, 0, 0],
            words(linear.column(0).into_owned().into(), 0.0),
            words(linear.column(1).into_owned().into(), 0.0),
            words(linear.column(2).into_owned().into(), 0.0),
        ]
    }

    pub(crate) fn bounds(self, (min, max): ([f32; 3], [f32; 3])) -> ([f32; 3], [f32; 3]) {
        let model = self.model;
        let radius = Vector3::from(std::array::from_fn(|index| {
            min[index].abs().max(max[index].abs())
        }))
        .norm();
        let scale = self.affine.map_or_else(
            || model.scale.into_iter().fold(1.0, f32::max),
            |affine| affine.linear.norm().max(1.0),
        );
        let offset: [f32; 3] = self
            .affine
            .map_or(model.offset, |affine| affine.origin.into());
        let [time, duration, _, _] = model.clock;
        let age = time.clamp(0.0, duration);
        let extent = std::array::from_fn(|index| {
            radius
                .max(model.origin[index].abs())
                .max(offset[index].abs() + radius * scale)
                + (model.velocity[index].abs() + model.spread[index]) * age
                + model.gravity[index].abs() * (0.5 * age * age)
                + 2.0 * radius * scale
        });
        (extent.map(|value| -value), extent)
    }

    pub(crate) fn deform<const NORMALS: bool>(
        self,
        index: u32,
        triangle: [MeshVertex; 3],
    ) -> [MeshVertex; 3] {
        self.deform_with_basis::<NORMALS, false>(index, triangle).0
    }

    pub(crate) fn deform_with_basis<const NORMALS: bool, const FRAME: bool>(
        self,
        index: u32,
        mut triangle: [MeshVertex; 3],
    ) -> ([MeshVertex; 3], Option<(Matrix3<f32>, bool)>) {
        let model = self.model;
        let [time, duration, stagger, arrival] = model.clock;
        let [a, b, c] = triangle.map(|vertex| Vector3::from(vertex.position));
        let center = (a + b + c) / 3.0;
        let seed = model.seed ^ index.wrapping_mul(0x9e37_79b9);
        let projected = center.dot(&Vector3::from(model.axis));
        let release = match model.order {
            Order::Sequential => {
                stagger * index as f32 / self.triangles.saturating_sub(1).max(1) as f32
            }
            Order::Random => stagger * random(seed ^ 0xa24b_aed5),
            Order::Activation => {
                stagger * ((projected - self.plane[0]) * self.plane[1]).clamp(0.0, 1.0)
            }
            Order::Plane => {
                let distance = projected + self.plane[0];
                if distance <= 0.0 && self.plane[1] == 0.0 {
                    f32::INFINITY
                } else {
                    (-distance).max(0.0) * self.plane[1]
                }
            }
        };
        let elapsed = time - release;
        if elapsed <= 0.0 {
            if model.mode == Mode::Construct {
                for vertex in &mut triangle {
                    vertex.position = model.origin;
                }
            }
            return (triangle, None);
        }
        let age = elapsed.min(duration);
        let variation = Vector3::new(
            random(seed),
            random(seed ^ 0xc13f_a9a9),
            random(seed ^ 0x91e1_0da5),
        )
        .map(|sample| 2.0 * sample - 1.0);
        let velocity =
            Vector3::from(model.velocity) + variation.component_mul(&Vector3::from(model.spread));
        let origin: Vector3<f32> = if model.mode == Mode::Construct {
            model.origin.into()
        } else {
            center
        };
        let free: Vector3<f32> =
            origin + (velocity + Vector3::from(model.gravity) * (0.5 * age)) * age;
        let spin = UnitQuaternion::from_axis_angle(
            &Unit::new_unchecked(Vector3::new(model.spin[0], model.spin[1], model.spin[2])),
            (age * model.spin[3]).rem_euclid(std::f32::consts::TAU),
        );
        let blend = if model.mode == Mode::Explode {
            0.0
        } else {
            let fraction = ((age - (duration - arrival)) / arrival).clamp(0.0, 1.0);
            fraction * fraction * (3.0 - 2.0 * fraction)
        };
        if let Some(affine) = self.affine.filter(|_| blend > 0.0) {
            let landed = elapsed >= duration;
            let (origin, pivot, basis) = if landed {
                (Vector3::zeros(), affine.origin, affine.linear)
            } else {
                (
                    center,
                    free.lerp(&(affine.origin + affine.linear * center), blend),
                    spin.to_rotation_matrix().matrix() * (1.0 - blend) + affine.linear * blend,
                )
            };
            let cofactor = cofactor(basis);
            let reflected = basis.column(0).dot(&cofactor.column(0)) < 0.0;
            let sign = if reflected { -1.0 } else { 1.0 };
            if reflected {
                triangle.swap(1, 2);
            }
            for vertex in &mut triangle {
                vertex.position =
                    (pivot + basis * (Vector3::from(vertex.position) - origin)).into();
                if NORMALS {
                    let normal = Vector3::from(vertex.normal);
                    let transformed = cofactor * normal;
                    vertex.normal = if transformed.norm_squared() > 1e-16 {
                        (sign * transformed).into()
                    } else {
                        (spin * normal).into()
                    };
                }
            }
            return (triangle, FRAME.then_some((basis, reflected)));
        }
        let [x, y, z, w] = model.target;
        let target = UnitQuaternion::new_unchecked(Quaternion::new(w, x, y, z));
        let scale = Vector3::from(model.scale);
        if model.mode != Mode::Explode && elapsed >= duration {
            for vertex in &mut triangle {
                vertex.position = (Vector3::from(model.offset)
                    + target * Vector3::from(vertex.position).component_mul(&scale))
                .into();
                if NORMALS {
                    vertex.normal =
                        (target * Vector3::from(vertex.normal).component_div(&scale)).into();
                }
            }
            return (
                triangle,
                FRAME.then(|| {
                    (
                        target.to_rotation_matrix().matrix() * Matrix3::from_diagonal(&scale),
                        false,
                    )
                }),
            );
        }
        let landing: Vector3<f32> =
            target * center.component_mul(&scale) + Vector3::from(model.offset);
        let rotation = if spin.coords.dot(&target.coords) < 0.0 {
            spin.nlerp(&UnitQuaternion::new_unchecked(-target.into_inner()), blend)
        } else {
            spin.nlerp(&target, blend)
        };
        let size = Vector3::repeat(1.0).lerp(&scale, blend);
        let pivot = free.lerp(&landing, blend);
        for vertex in &mut triangle {
            let local = (Vector3::from(vertex.position) - center).component_mul(&size);
            vertex.position = (pivot + rotation * local).into();
            if NORMALS {
                vertex.normal =
                    (rotation * Vector3::from(vertex.normal).component_div(&size)).into();
            }
        }
        (
            triangle,
            FRAME.then(|| {
                (
                    rotation.to_rotation_matrix().matrix() * Matrix3::from_diagonal(&size),
                    false,
                )
            }),
        )
    }
}

fn cofactor(matrix: Matrix3<f32>) -> Matrix3<f32> {
    let [x, y, z] = std::array::from_fn(|index| matrix.column(index));
    Matrix3::from_columns(&[y.cross(&z), z.cross(&x), x.cross(&y)])
}

fn random(input: u32) -> f32 {
    let bits = (input ^ (input >> 16)).wrapping_mul(0x7feb_352d);
    let bits = (bits ^ (bits >> 15)).wrapping_mul(0x846c_a68b);
    ((bits ^ (bits >> 16)) >> 8) as f32 / 16_777_216.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baked_triangle_basis_follows_model_blend_spin() {
        let triangle = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]].map(|position| MeshVertex {
            position,
            normal: [0.0, 0.0, 1.0],
            uv: [0.0; 2],
            uv1: [0.0; 2],
            color: [1.0; 4],
        });
        let model = ModelBlendDefinition {
            mode: "explode",
            time: 1.0,
            duration: 2.0,
            spin: Vector3D::new(0.0, 0.0, 90.0),
            ..Default::default()
        }
        .prepare()
        .unwrap();
        let blend = ModelBlendPose {
            model: model.motion,
            triangles: 1,
            plane: [0.0; 2],
            last_release: 0.0,
            affine: None,
        };
        let (posed, basis) = blend.deform_with_basis::<true, true>(0, triangle);
        assert_eq!(posed, blend.deform::<true>(0, triangle));
        let (basis, reflected) = basis.unwrap();
        assert!(!reflected);
        let edge = Vector3::from(posed[1].position) - Vector3::from(posed[0].position);
        assert!((basis * Vector3::x() - edge).norm() < 1e-6);
        assert!((edge - Vector3::y()).norm() < 1e-6);
    }
}
