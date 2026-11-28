//! Frustum selection, closest-point queries, and ray hits.

use nalgebra::Vector3;

use super::SceneObject;
use crate::scene3d::{
    ClosestSurface, Mesh3D, MeshCullMode, MeshMaterial, MeshTopology, MeshVertex, Ray3D, RayHit,
    SceneCamera, SurfaceHit, bake_opaque, surface::closest_triangle_point, visibility::Frustum,
};

impl SceneObject {
    #[doc(hidden)]
    pub fn is_pickable(&self) -> bool {
        self.pickable
    }

    pub fn with_pickable(mut self, enabled: bool) -> Self {
        self.pickable = enabled;
        self
    }

    pub(crate) fn intersects_selection(
        &self,
        camera: &SceneCamera,
        region: &Frustum,
        planes: u8,
    ) -> bool {
        if !self.pickable
            || !self.mesh.has_triangle_surface()
            || self.color.0 as u8 == 0
                && matches!(&self.mesh, Mesh3D::Imported(mesh) if mesh.instance_blended)
        {
            return false;
        }
        let test_faces = matches!(&self.mesh, Mesh3D::Imported(mesh)
            if mesh.has_face_culling || mesh.materials.iter().any(|material| !material.is_visible()));
        if planes == 0 && !test_faces {
            return true;
        }
        let pose = self.pick_pose();
        let intersects = |vertex, material: Option<&MeshMaterial>, planes| {
            if material.is_some_and(|material| !material.is_visible()) {
                return false;
            }
            let vertices = pose.deform::<false>(vertex, *self.mesh.triangle_at(vertex));
            let points = vertices.map(|vertex| {
                self.transform.linear * Vector3::from(vertex.position) + self.transform.origin
            });
            let cull = material.map_or(MeshCullMode::None, |material| material.cull_mode);
            if cull != MeshCullMode::None {
                let [a, b, c] = points;
                let facing = (b - a).cross(&(c - a)).dot(&camera.toward_viewer(a));
                if facing == 0.0 || !cull.accepts((facing > 0.0) ^ self.is_reflected()) {
                    return false;
                }
            }
            region.triangle_intersects(points.map(Into::into), planes)
        };
        if !pose.moves_positions()
            && let Some(tree) = self.mesh.pick_tree()
        {
            tree.intersects_selection(
                region,
                self.transform_rows(),
                planes,
                test_faces,
                |triangle, planes| {
                    let material = self
                        .mesh
                        .parts()
                        .get(triangle.part as usize)
                        .map(|part| &self.mesh.materials()[part.material]);
                    intersects(triangle.vertex as usize, material, planes)
                },
            )
        } else {
            self.mesh.triangle_sections().any(|(range, material)| {
                range
                    .step_by(3)
                    .any(|vertex| intersects(vertex, material, planes))
            })
        }
    }

    pub(in crate::scene3d) fn closest_surface(
        &self,
        point: Vector3<f64>,
        limit: f64,
    ) -> Option<ClosestSurface> {
        let pose = self.pick_pose();
        let origin = self.transform.origin.cast::<f64>();
        let linear = self.transform.linear.cast::<f64>();
        let mut best: Option<ClosestSurface> = None;
        let mut inspect = |vertex: usize, limit: f64| {
            let triangle = pose.deform::<false>(vertex, *self.mesh.triangle_at(vertex));
            let [a, b, c] =
                triangle.map(|v| linear * Vector3::from(v.position).cast::<f64>() + origin);
            if (b - a).cross(&(c - a)).norm_squared() == 0.0 {
                return limit;
            }
            let (weights, distance_squared) = closest_triangle_point(point, [a, b, c]);
            if distance_squared <= limit
                && (distance_squared < limit || best.is_none_or(|old| vertex < old.vertex))
            {
                let [u, v, w] = weights;
                let position = (a * u + b * v + c * w).cast::<f32>();
                if position.iter().copied().all(f32::is_finite) {
                    best = Some(ClosestSurface {
                        vertex,
                        weights,
                        distance_squared,
                        position,
                    });
                    return distance_squared;
                }
            }
            limit
        };
        if !pose.moves_positions()
            && let Some(tree) = self.mesh.pick_tree()
        {
            tree.visit_nearest(point, self.transform_rows(), limit, inspect);
        } else {
            let mut limit = limit;
            for vertex in self
                .mesh
                .triangle_sections()
                .flat_map(|(range, _)| range.step_by(3))
            {
                limit = inspect(vertex, limit);
            }
        }
        best
    }

    pub(in crate::scene3d) fn hit(&self, ray: Ray3D, min: f32, max: f32) -> Option<SurfaceHit> {
        self.pickable
            .then(|| self.surface_hit(ray, min, max, false))
            .flatten()
    }

    pub(in crate::scene3d) fn surface_hit(
        &self,
        ray: Ray3D,
        min: f32,
        max: f32,
        bake: bool,
    ) -> Option<SurfaceHit> {
        // Keep the ray parameter in world units under non-uniform scaling.
        let local_origin = self.transform.inverse * (ray.origin - self.transform.origin);
        let world_step = self.transform.inverse * ray.direction;
        if !overlaps_bounds(local_origin, world_step, self.bounds(), min, max) {
            return None;
        }
        let ray_scale = world_step.norm();
        let local_direction = world_step / ray_scale;
        let pose = self.pick_pose();
        let tint_alpha = self.tint_alpha();
        let moves_positions = pose.moves_positions();
        let mut best = None;
        let mut test = |vertex: usize, material: Option<&MeshMaterial>, closest| {
            if bake && material.is_some_and(|material| !bake_opaque(material)) {
                return closest;
            }
            let base = self.mesh.triangle_at(vertex);
            let posed;
            let [a, b, c] = if moves_positions {
                posed = pose.deform::<false>(vertex, *base);
                posed.each_ref()
            } else {
                base.each_ref()
            };
            let Some((distance, u, v, front)) =
                intersect_triangle(local_origin, local_direction, [a, b, c])
            else {
                return closest;
            };
            let distance = distance / ray_scale;
            if !(min..=closest).contains(&distance) {
                return closest;
            }
            let w = 1.0 - u - v;
            if let Some(material) = material {
                if !material.is_visible() || !material.cull_mode.accepts(front) {
                    return closest;
                }
                if material.tests_fragment_coverage() {
                    let [a, b, c] = pose.material_vertices(vertex, *base);
                    let uv = [
                        std::array::from_fn(|axis| {
                            a.uv[axis] * w + b.uv[axis] * u + c.uv[axis] * v
                        }),
                        std::array::from_fn(|axis| {
                            a.uv1[axis] * w + b.uv1[axis] * u + c.uv1[axis] * v
                        }),
                    ];
                    let vertex_alpha = a.color[3] * w + b.color[3] * u + c.color[3] * v;
                    let covered = if bake {
                        material.depth_covers(uv, vertex_alpha, tint_alpha)
                    } else {
                        material.covers(uv, vertex_alpha, tint_alpha)
                    };
                    if !covered {
                        return closest;
                    }
                }
            }
            let hit = SurfaceHit {
                topology: MeshTopology::Triangles,
                distance,
                vertex,
                weights: [w, u, v],
                back_face: material.is_some() && !front,
                blended: material.is_some_and(|material| material.is_blended()),
            };
            if hit.precedes_triangle(best) {
                best = Some(hit);
                distance
            } else {
                closest
            }
        };
        if !moves_positions
            && let Mesh3D::Imported(mesh) = &self.mesh
            && let Some(tree) = self.mesh.pick_tree()
        {
            tree.visit(local_origin, world_step, min, max, |triangle, closest| {
                let part = &self.mesh.parts()[triangle.part as usize];
                test(
                    triangle.vertex as usize,
                    Some(&mesh.materials[part.material]),
                    closest,
                )
            });
        } else {
            let mut closest = max;
            for (range, material) in self.mesh.triangle_sections() {
                for vertex in range.step_by(3) {
                    closest = test(vertex, material, closest);
                }
            }
        }
        best
    }

    pub(in crate::scene3d) fn describe_hit(&self, ray: Ray3D, surface: SurfaceHit) -> RayHit<'_> {
        if surface.topology != MeshTopology::Triangles {
            return self.describe_raster(surface);
        }
        let position = ray.origin + surface.distance * ray.direction;
        self.describe_surface(position, surface)
    }

    pub(in crate::scene3d) fn describe_surface(
        &self,
        position: Vector3<f32>,
        surface: SurfaceHit,
    ) -> RayHit<'_> {
        let base = *self.mesh.triangle_at(surface.vertex);
        let [a, b, c] = self.pick_pose().deform::<true>(surface.vertex, base);
        let [w, u, v] = surface.weights;
        let local_position = Vector3::from(a.position) * w
            + Vector3::from(b.position) * u
            + Vector3::from(c.position) * v;
        let interpolated =
            Vector3::from(a.normal) * w + Vector3::from(b.normal) * u + Vector3::from(c.normal) * v;
        // Mesh normals can cancel at a seam. Use the face's oriented normal in
        // that case so a surface hit always has a usable unit direction.
        let local_normal = interpolated.try_normalize(1e-12).unwrap_or_else(|| {
            let a = Vector3::from(a.position);
            (Vector3::from(b.position) - a)
                .cross(&(Vector3::from(c.position) - a))
                .normalize()
        });
        let sign = if surface.back_face { -1.0 } else { 1.0 };
        let world_normal = (self.transform.inverse.transpose() * local_normal).normalize();
        RayHit {
            object_id: &self.id,
            row_index: self.source_row.map(|row| row.get() - 1),
            imported_placement: self.imported_placement,
            distance: surface.distance,
            position: position.into(),
            normal: (sign * world_normal).into(),
            local_position: local_position.into(),
            local_normal: (sign * local_normal).into(),
            uv: std::array::from_fn(|axis| a.uv[axis] * w + b.uv[axis] * u + c.uv[axis] * v),
        }
    }
}

fn overlaps_bounds(
    origin: Vector3<f32>,
    direction: Vector3<f32>,
    (min, max): ([f32; 3], [f32; 3]),
    mut near: f32,
    mut far: f32,
) -> bool {
    for axis in 0..3 {
        if direction[axis].abs() < 1e-10 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return false;
            }
            continue;
        }
        let a = (min[axis] - origin[axis]) / direction[axis];
        let b = (max[axis] - origin[axis]) / direction[axis];
        near = near.max(a.min(b));
        far = far.min(a.max(b));
        if near > far {
            return false;
        }
    }
    true
}

fn intersect_triangle(
    origin: Vector3<f32>,
    direction: Vector3<f32>,
    [a, b, c]: [&MeshVertex; 3],
) -> Option<(f32, f32, f32, bool)> {
    let a = Vector3::from(a.position);
    let ab = Vector3::from(b.position) - a;
    let ac = Vector3::from(c.position) - a;
    let p = direction.cross(&ac);
    let det = ab.dot(&p);
    if det.abs() < 1e-8 {
        return None;
    }
    let delta = origin - a;
    let u = delta.dot(&p) / det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = delta.cross(&ab);
    let v = direction.dot(&q) / det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    Some((ac.dot(&q) / det, u, v, det > 0.0))
}
