//! Shape sweeps and trigger contacts against posed geometry.

use nalgebra::Vector3;

use super::SceneObject;
use crate::scene3d::{
    MeshVertex,
    cast::{MeshContact, ShapeSweep, TriangleContact, cast_shape_triangle},
    mesh::data::Primitive,
    pick::BoundsSegment,
};

impl SceneObject {
    pub(crate) fn sweep_contact(&self, sweep: ShapeSweep, max: f64) -> Option<MeshContact> {
        if !self.collidable || self.trigger {
            return None;
        }
        self.shape_contact(sweep, max)
    }

    pub(crate) fn trigger_contact(&self, sweep: ShapeSweep, max: f64) -> Option<MeshContact> {
        if !self.trigger {
            return None;
        }
        self.shape_contact(sweep, max)
    }

    fn shape_contact(&self, sweep: ShapeSweep, max: f64) -> Option<MeshContact> {
        if sweep.span(max).entry(self.world_bounds()).is_none() {
            return None;
        }
        let linear = self.transform.linear.cast::<f64>();
        let origin = self.transform.origin.cast::<f64>();
        let inverse = linear.try_inverse();
        let local_center = inverse.map(|inverse| inverse * (sweep.origin - origin));
        let triangles = self.mesh.vertices().as_chunks::<3>().0;
        let inside_faces = |point: Vector3<f64>| {
            triangles.iter().all(|triangle| {
                let [a, b, c] =
                    triangle.map(|vertex| Vector3::from(vertex.position.map(f64::from)));
                let normal = (b - a).cross(&(c - a));
                normal == Vector3::zeros() || normal.dot(&(point - a)) < 0.0
            })
        };
        // Generated primitives are convex solids. Imported geometry contributes
        // its posed surfaces, which may describe open or disconnected meshes.
        let inside = local_center.is_some_and(|point| match self.mesh.primitive() {
            Some(Primitive::Box) => point.abs().max() < 0.5,
            Some(Primitive::Sphere) if point.norm_squared() < 1.0 => {
                triangles.iter().all(|triangle| {
                    let [a, b, c] =
                        triangle.map(|vertex| Vector3::from(vertex.position.map(f64::from)));
                    let normal = (b - a).cross(&(c - a));
                    let plane = normal.dot(&a);
                    plane == 0.0 || normal.dot(&(point - a)) * plane.signum() < 0.0
                })
            }
            Some(Primitive::Cylinder)
                if point.y.abs() < 0.5 && point.x * point.x + point.z * point.z < 0.25 =>
            {
                inside_faces(point)
            }
            Some(Primitive::Cone)
                if point.y > 0.0
                    && point.y < 1.0
                    && point.x * point.x + point.z * point.z < 0.25 * (1.0 - point.y).powi(2) =>
            {
                inside_faces(point)
            }
            Some(Primitive::Capsule) => {
                let (min, max) = self.mesh.bounds();
                (0..3).all(|axis| {
                    point[axis] > f64::from(min[axis]) && point[axis] < f64::from(max[axis])
                }) && inside_faces(point)
            }
            _ => false,
        });
        let pose = self.pick_pose();
        let mut best: Option<(usize, TriangleContact, [MeshVertex; 3], bool)> = None;
        let mut inspect = |vertex: usize, closest: f64| {
            let triangle = pose.deform::<false>(vertex, triangles[vertex / 3]);
            let world =
                triangle.map(|p| linear * Vector3::from(p.position.map(f64::from)) + origin);
            if let Some((contact, intersected)) = cast_shape_triangle(sweep, world, inside, closest)
                && best
                    .as_ref()
                    .is_none_or(|(previous_vertex, previous, _, previous_inside)| {
                        contact
                            .distance
                            .total_cmp(&previous.distance)
                            .then_with(|| previous_inside.cmp(&intersected))
                            .then(contact.initial_gap.total_cmp(&previous.initial_gap))
                            .then(vertex.cmp(previous_vertex))
                            .is_lt()
                    })
            {
                let distance = contact.distance;
                best = Some((vertex, contact, triangle, intersected));
                distance
            } else {
                closest
            }
        };
        let tree = (!inside && !pose.moves_positions())
            .then(|| self.mesh.pick_tree())
            .flatten();
        if let (Some(tree), Some(inverse), Some(local_center)) = (tree, inverse, local_center) {
            // Inverse-transforming a world sphere gives an ellipsoid. Its
            // coordinate radii conservatively expand every mesh-space BVH node.
            let span = BoundsSegment {
                origin: local_center,
                direction: inverse * sweep.direction,
                min: 0.0,
                max,
                expansion: Vector3::from_fn(|axis, _| {
                    inverse.row(axis).norm() * sweep.radius
                        + inverse[(axis, 1)].abs() * sweep.half_segment
                }),
            };
            tree.visit_expanded(span, |triangle, closest| {
                inspect(triangle.vertex as usize, closest)
            });
        } else {
            let mut closest = max;
            for vertex in (0..triangles.len() * 3).step_by(3) {
                closest = inspect(vertex, closest);
            }
        }
        let (_, contact, triangle, inside) = best?;
        let local_position = triangle
            .iter()
            .zip(contact.weights)
            .fold(Vector3::zeros(), |sum, (point, weight)| {
                sum + Vector3::from(point.position.map(f64::from)) * weight
            });
        let uv = std::array::from_fn(|axis| {
            triangle
                .iter()
                .zip(contact.weights)
                .map(|(point, weight)| f64::from(point.uv[axis]) * weight)
                .sum()
        });
        let local_normal = (linear.transpose() * contact.normal)
            .try_normalize(0.0)
            .unwrap_or(contact.normal);
        Some(MeshContact {
            contact,
            local_position,
            local_normal,
            uv,
            inside,
        })
    }
}
