//! Collision shapes prepared once when a rigid body is created.

use rapier3d::prelude::{ColliderBuilder, Pose, RigidBodyBuilder, SharedShape, Vector};

use super::mesh_shapes::{MeshShapeKind, prepare_mesh_shape};
use super::{PhysicsWorld3D, collider_material, scene_pose, vector};
use crate::{
    Point3D, RecordList, VecModel, Vector3D, scene_hierarchy::CollisionShapeSpec,
    scene3d::MeshAsset,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum PartKind {
    Convex,
    Surface,
    Plane,
}

/// One collision shape in a rigid body's local coordinate system.
#[derive(Clone)]
pub struct PhysicsShape3D {
    shape: SharedShape,
    local: Pose,
    kind: PartKind,
}

/// Local colliders prepared for insertion. Convex-only lists share one collider.
#[derive(Default)]
pub struct PhysicsCollider3D(Vec<PhysicsShape3D>);

impl PhysicsShape3D {
    pub fn new(shape: &str, position: Point3D, size: Vector3D, angles: [f64; 3]) -> Option<Self> {
        Self::new_with_mesh(shape, position, size, angles, "", |_| None)
    }

    #[doc(hidden)]
    pub fn new_with_mesh(
        shape: &str,
        position: Point3D,
        size: Vector3D,
        angles: [f64; 3],
        mesh: &str,
        imported: impl FnOnce(&str) -> Option<MeshAsset>,
    ) -> Option<Self> {
        let [yaw, pitch, roll] = angles;
        let mut local = scene_pose(position, Vector3D::new(yaw, pitch, roll))?;
        if shape == "plane" {
            return Some(Self {
                shape: SharedShape::halfspace(Vector::Y),
                local,
                kind: PartKind::Plane,
            });
        }
        let mesh_kind = match shape {
            "convex-mesh" => Some(MeshShapeKind::Convex),
            "mesh" => Some(MeshShapeKind::Triangle),
            "heightfield" => Some(MeshShapeKind::HeightField),
            _ => None,
        };
        if let Some(mesh_kind) = mesh_kind {
            let scale = vector([size.x, size.y, size.z])?;
            if scale.min_element() <= 0.0 {
                return None;
            }
            let asset = MeshAsset::resolve(mesh, imported)?;
            let shape = prepare_mesh_shape(asset, scale, mesh_kind)?;
            let kind = if mesh_kind == MeshShapeKind::Convex {
                PartKind::Convex
            } else {
                PartKind::Surface
            };
            return Some(Self { shape, local, kind });
        }
        let size = vector([size.x * 0.5, size.y * 0.5, size.z * 0.5])?;
        if size.min_element() <= 0.0 {
            return None;
        }
        let shape = match shape {
            "box" => SharedShape::cuboid(size.x, size.y, size.z),
            "sphere" if size.x == size.y && size.x == size.z => SharedShape::ball(size.x),
            "cylinder" if size.x == size.z => SharedShape::cylinder(size.y, size.x),
            "cone" if size.x == size.z => {
                // The authored cone's origin is at the center of its base.
                local = local.prepend_translation(Vector::Y * size.y);
                SharedShape::cone(size.y, size.x)
            }
            "capsule" if size.x == size.z && size.y >= size.x => {
                if size.y == size.x {
                    SharedShape::ball(size.x)
                } else {
                    SharedShape::capsule_y(size.y - size.x, size.x)
                }
            }
            _ => return None,
        };
        Some(Self {
            shape,
            local,
            kind: PartKind::Convex,
        })
    }
}

impl PhysicsCollider3D {
    pub fn from_records(shapes: &RecordList) -> Self {
        Self::from_records_with_meshes(shapes, |_| None)
    }

    #[doc(hidden)]
    pub fn from_records_with_meshes(
        shapes: &RecordList,
        imported: impl Fn(&str) -> Option<MeshAsset>,
    ) -> Self {
        Self::prepare(shapes.model(), |row| {
            let spec = CollisionShapeSpec::from_record(row)?;
            PhysicsShape3D::new_with_mesh(
                spec.shape,
                spec.position,
                spec.size,
                spec.angles,
                spec.mesh,
                &imported,
            )
        })
    }

    pub fn prepare<T: 'static>(
        shapes: &VecModel<T>,
        project: impl FnMut(&T) -> Option<PhysicsShape3D>,
    ) -> Self {
        let (_, projected) = shapes.inspect_rows(0, usize::MAX, project);
        let Some(parts) = projected.into_iter().collect::<Option<Vec<_>>>() else {
            return Self::default();
        };
        Self::from_shapes(parts)
    }

    pub(super) fn from_shapes(parts: Vec<PhysicsShape3D>) -> Self {
        if parts.len() <= 1 {
            return Self(parts);
        }
        if parts.iter().any(|part| part.kind == PartKind::Plane) {
            return Self::default();
        }
        // Rapier only allows convex children in a compound. Surface colliders
        // retain their own acceleration structures on the same rigid body.
        if parts.iter().any(|part| part.kind == PartKind::Surface) {
            return Self(parts);
        }
        let parts = parts
            .into_iter()
            .map(|part| (part.local, part.shape))
            .collect();
        Self(vec![PhysicsShape3D {
            shape: SharedShape::compound(parts),
            local: Pose::identity(),
            kind: PartKind::Convex,
        }])
    }

    pub(super) fn into_convex_shape(self) -> Option<(SharedShape, Pose)> {
        let [part]: [PhysicsShape3D; 1] = self.0.try_into().ok()?;
        (part.kind == PartKind::Convex).then_some((part.shape, part.local))
    }

    pub(super) fn into_convex(self) -> Option<ColliderBuilder> {
        let (shape, local) = self.into_convex_shape()?;
        Some(ColliderBuilder::new(shape).position(local))
    }

    pub(super) fn into_surface_builders(
        self,
        allow_plane: bool,
        friction: f64,
        restitution: f64,
    ) -> Option<Vec<ColliderBuilder>> {
        self.0
            .into_iter()
            .map(|part| {
                if part.kind == PartKind::Plane && !allow_plane {
                    return None;
                }
                collider_material(
                    ColliderBuilder::new(part.shape).position(part.local),
                    friction,
                    restitution,
                )
            })
            .collect()
    }
}

impl PhysicsWorld3D {
    /// Add a fixed or dynamic body constructed from a list of local shapes.
    pub fn add_body(
        &self,
        center: Point3D,
        rotation: Vector3D,
        shapes: PhysicsCollider3D,
        mass: f64,
        friction: f64,
        restitution: f64,
    ) -> i64 {
        let Some(pose) = scene_pose(center, rotation) else {
            return 0;
        };
        if mass == 0.0 {
            let Some(colliders) = shapes.into_surface_builders(true, friction, restitution) else {
                return 0;
            };
            return self.insert_colliders(RigidBodyBuilder::fixed().pose(pose), colliders, None);
        }
        let Some(collider) = shapes.into_convex() else {
            return 0;
        };
        self.insert_mass_body(pose, collider, mass, friction, restitution)
    }

    /// Add a kinematic body constructed from a list of local shapes.
    pub fn add_kinematic_body(
        &self,
        center: Point3D,
        rotation: Vector3D,
        shapes: PhysicsCollider3D,
        friction: f64,
        restitution: f64,
    ) -> i64 {
        let (Some(pose), Some(colliders)) = (
            scene_pose(center, rotation),
            shapes.into_surface_builders(false, friction, restitution),
        ) else {
            return 0;
        };
        self.insert_colliders(
            RigidBodyBuilder::kinematic_position_based()
                .ccd_enabled(true)
                .pose(pose),
            colliders,
            None,
        )
    }

    /// Add a fixed, nonblocking trigger constructed from a list of local shapes.
    pub fn add_trigger(
        &self,
        center: Point3D,
        rotation: Vector3D,
        shapes: PhysicsCollider3D,
    ) -> i64 {
        let (Some(pose), Some(collider)) = (scene_pose(center, rotation), shapes.into_convex())
        else {
            return 0;
        };
        self.insert_trigger(pose, collider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(shapes: Vec<PhysicsShape3D>) -> PhysicsCollider3D {
        let model = VecModel::from(shapes.into_iter().map(Some).collect::<Vec<_>>());
        PhysicsCollider3D::prepare(&model, Clone::clone)
    }

    fn shape(kind: &str, position: Point3D, size: Vector3D) -> PhysicsShape3D {
        PhysicsShape3D::new(kind, position, size, [0.0; 3]).expect("valid test collider")
    }

    #[test]
    fn rotated_compound_keeps_open_space_between_parts() {
        let launch = |z| {
            let world = PhysicsWorld3D::new(Vector3D::default(), 0.1);
            let wall = world.add_body(
                Point3D::default(),
                Vector3D::new(90.0, 0.0, 0.0),
                parts(vec![
                    shape(
                        "box",
                        Point3D::new(-1.0, 0.0, 0.0),
                        Vector3D::new(0.8, 0.8, 0.8),
                    ),
                    shape(
                        "box",
                        Point3D::new(1.0, 0.0, 0.0),
                        Vector3D::new(0.8, 0.8, 0.8),
                    ),
                ]),
                0.0,
                0.0,
                0.0,
            );
            let ball = world.add_sphere(Point3D::new(-3.0, 0.0, z), 0.2, 1.0, 0.0, 0.0);
            assert!(wall > 0 && ball > 0);
            assert!(world.apply_impulse(ball, Vector3D::new(55.0, 0.0, 0.0)));
            assert!(world.step());
            world.position(ball).x
        };
        assert!(launch(0.0) > 2.0);
        assert!(launch(1.0) < 0.0);
    }

    #[test]
    fn compound_trigger_emits_one_transition_for_a_body_overlapping_two_parts() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.016);
        let trigger = world.add_trigger(
            Point3D::default(),
            Vector3D::default(),
            parts(vec![
                shape(
                    "sphere",
                    Point3D::new(-0.25, 0.0, 0.0),
                    Vector3D::new(1.0, 1.0, 1.0),
                ),
                shape(
                    "sphere",
                    Point3D::new(0.25, 0.0, 0.0),
                    Vector3D::new(1.0, 1.0, 1.0),
                ),
            ]),
        );
        let body = world.add_sphere(Point3D::default(), 0.15, 1.0, 0.0, 0.0);
        assert!(trigger > 0 && body > 0);
        assert!(world.step());
        assert_eq!(world.trigger_count(trigger), 1);
        assert_eq!(world.trigger_entered(trigger), [body]);
        assert!(world.apply_impulse(body, Vector3D::new(4.0, 0.0, 0.0)));
        let mut exited = Vec::new();
        for _ in 0..25 {
            assert!(world.step());
            exited.extend(world.trigger_exited(trigger));
        }
        assert_eq!(exited, [body]);
        assert_eq!(world.trigger_count(trigger), 0);
    }

    #[test]
    fn capsule_rests_on_a_fixed_plane_and_invalid_planes_leave_the_world_unchanged() {
        let world = PhysicsWorld3D::new(Vector3D::new(0.0, -9.81, 0.0), 1.0 / 60.0);
        let origin = Point3D::default();
        let rotation = Vector3D::default();
        let plane = || parts(vec![shape("plane", origin, Vector3D::default())]);
        assert_eq!(world.add_body(origin, rotation, plane(), 1.0, 0.3, 0.0), 0);
        assert_eq!(
            world.add_kinematic_body(origin, rotation, plane(), 0.3, 0.0),
            0
        );
        assert_eq!(world.add_trigger(origin, rotation, plane()), 0);
        assert_eq!(world.add_body(origin, rotation, plane(), 0.0, 0.3, 0.0), 1);
        let capsule = world.add_body(
            Point3D::new(0.0, 2.2, 0.0),
            rotation,
            parts(vec![shape("capsule", origin, Vector3D::new(0.6, 1.6, 0.6))]),
            2.0,
            0.3,
            0.0,
        );
        assert!(capsule > 0);
        for _ in 0..120 {
            assert!(world.step());
        }
        assert!((world.position(capsule).y - 0.8).abs() < 0.03);
    }

    #[test]
    fn mixed_mesh_body_occupies_a_trigger_until_its_last_part_leaves() {
        let world = PhysicsWorld3D::new(Vector3D::default(), 0.016);
        let origin = Point3D::default();
        let angles = Vector3D::default();
        let shapes = || {
            parts(vec![
                PhysicsShape3D::new_with_mesh(
                    "mesh",
                    origin,
                    Vector3D::new(1.0, 1.0, 1.0),
                    [0.0; 3],
                    "primitive:box",
                    |_| None,
                )
                .unwrap(),
                shape(
                    "box",
                    Point3D::new(0.75, 0.0, 0.0),
                    Vector3D::new(0.9, 0.9, 0.9),
                ),
            ])
        };
        assert_eq!(world.add_body(origin, angles, shapes(), 1.0, 0.0, 0.0), 0);
        assert_eq!(world.add_trigger(origin, angles, shapes()), 0);
        let body = world.add_kinematic_body(origin, angles, shapes(), 0.0, 0.0);
        let trigger = world.add_box_trigger(origin, Vector3D::new(0.9, 1.6, 1.6), angles);
        assert!(body > 0 && trigger > 0);
        assert!(world.step());
        assert_eq!(world.trigger_entered(trigger), [body]);
        assert_eq!(world.trigger_count(trigger), 1);
        assert!(world.set_kinematic_target(body, Point3D::new(0.5, 0.0, 0.0), angles));
        assert!(world.step());
        assert!(world.step());
        assert!(world.trigger_exited(trigger).is_empty());
        assert_eq!(world.trigger_count(trigger), 1);
        assert!(world.set_kinematic_target(body, Point3D::new(2.0, 0.0, 0.0), angles));
        assert!(world.step());
        let mut exited = world.trigger_exited(trigger);
        assert!(world.step());
        exited.extend(world.trigger_exited(trigger));
        assert_eq!(exited, [body]);
        assert_eq!(world.trigger_count(trigger), 0);
    }
}
