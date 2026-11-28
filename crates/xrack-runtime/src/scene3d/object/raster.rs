//! Screen queries against posed point and line mesh parts.

use nalgebra::Vector3;

use crate::{
    Point2D,
    scene3d::{
        MeshMaterial, MeshPart, MeshTopology, MeshVertex, Ray3D, RayHit, SurfaceHit,
        screen_pick::{RasterPrimitive, RasterShape, RasterView, ScreenRect},
        visibility::Frustum,
    },
};

use super::{SceneObject, pose::PickPose};

impl SceneObject {
    fn raster_vertices<const NORMALS: bool>(
        &self,
        pose: &PickPose<'_>,
        topology: MeshTopology,
        vertex: usize,
    ) -> [MeshVertex; 2] {
        let vertices = self.mesh.vertices();
        match topology {
            MeshTopology::Points => {
                let [point] = pose.deform_vertices::<NORMALS, 1>(vertex, [vertices[vertex]]);
                [point; 2]
            }
            _ => {
                pose.deform_vertices::<NORMALS, 2>(vertex, [vertices[vertex], vertices[vertex + 1]])
            }
        }
    }

    fn visit_raster(
        &self,
        view: RasterView<'_>,
        region: &Frustum,
        pose: &PickPose<'_>,
        mut visit: impl FnMut(usize, MeshTopology, &MeshMaterial, RasterShape, [[f32; 3]; 2]) -> bool,
    ) -> bool {
        if self.mesh.raster_size() == 0.0 {
            return false;
        }
        let parts = self.mesh.parts();
        let mut test = |RasterPrimitive { vertex, part }: RasterPrimitive| {
            let MeshPart {
                topology, material, ..
            } = parts[part as usize];
            let material = &self.mesh.materials()[material];
            if !material.is_visible() {
                return false;
            }
            let vertex = vertex as usize;
            let world = self
                .raster_vertices::<false>(pose, topology, vertex)
                .map(|v| {
                    (self.transform.linear * Vector3::from(v.position) + self.transform.origin)
                        .into()
                });
            let shape = match topology {
                MeshTopology::Points => view.point(world[0], material.point_size),
                MeshTopology::Lines => view.line(world, material.line_width),
                MeshTopology::Triangles => None,
            };
            shape.is_some_and(|shape| visit(vertex, topology, material, shape, world))
        };
        if !pose.moves_positions()
            && let Some(tree) = self.mesh.raster_tree()
        {
            return tree.visit(region, self.transform_rows(), test);
        }
        parts
            .iter()
            .enumerate()
            .filter(|(_, part)| part.topology != MeshTopology::Triangles)
            .any(|(part, section)| {
                section
                    .vertices
                    .clone()
                    .step_by(section.topology.vertices_per_primitive())
                    .any(|vertex| {
                        test(RasterPrimitive {
                            vertex: vertex as u32,
                            part: part as u32,
                        })
                    })
            })
    }

    pub(in crate::scene3d) fn raster_hit(
        &self,
        view: RasterView<'_>,
        cursor: Point2D,
        region: &Frustum,
        ray: Ray3D,
        interval: [f32; 2],
    ) -> Option<SurfaceHit> {
        let mut best = None;
        let pose = self.pick_pose();
        let tint_alpha = self.tint_alpha();
        self.visit_raster(
            view,
            region,
            &pose,
            |vertex, topology, material, shape, world| {
                let Some(t) = view.parameter(shape, cursor) else {
                    return false;
                };
                let along = t as f32;
                let world = (Vector3::from(world[0]) * (1.0 - along)
                    + Vector3::from(world[1]) * along)
                    .into();
                let Some(distance) = view.distance(ray, world) else {
                    return false;
                };
                if !(interval[0]..=interval[1]).contains(&distance) {
                    return false;
                }
                if material.tests_fragment_coverage() {
                    let [a, b] = self.raster_material_vertices(&pose, topology, vertex);
                    let uv = [
                        std::array::from_fn(|i| a.uv[i] * (1.0 - along) + b.uv[i] * along),
                        std::array::from_fn(|i| a.uv1[i] * (1.0 - along) + b.uv1[i] * along),
                    ];
                    let alpha = a.color[3] * (1.0 - along) + b.color[3] * along;
                    if !material.covers(uv, alpha, tint_alpha) {
                        return false;
                    }
                }
                let hit = SurfaceHit {
                    topology,
                    distance,
                    vertex,
                    weights: [1.0 - along, along, 0.0],
                    back_face: false,
                    blended: material.is_blended(),
                };
                if hit.precedes_triangle(best) {
                    best = Some(hit);
                }
                false
            },
        );
        best
    }

    pub(in crate::scene3d) fn raster_intersects(
        &self,
        view: RasterView<'_>,
        region: &Frustum,
        rectangle: ScreenRect,
    ) -> bool {
        self.pickable
            && self.visit_raster(view, region, &self.pick_pose(), |_, _, _, shape, _| {
                shape.intersects(rectangle)
            })
    }

    fn raster_material_vertices(
        &self,
        pose: &PickPose<'_>,
        topology: MeshTopology,
        vertex: usize,
    ) -> [MeshVertex; 2] {
        let points = self.mesh.vertices();
        match topology {
            MeshTopology::Points => [pose.material_vertices(vertex, [points[vertex]])[0]; 2],
            _ => pose.material_vertices(vertex, [points[vertex], points[vertex + 1]]),
        }
    }

    pub(in crate::scene3d) fn raster_uv(&self, hit: SurfaceHit) -> [f32; 2] {
        let [a, b] = self.raster_material_vertices(&self.pick_pose(), hit.topology, hit.vertex);
        std::array::from_fn(|i| a.uv[i] * hit.weights[0] + b.uv[i] * hit.weights[1])
    }

    pub(super) fn describe_raster(&self, hit: SurfaceHit) -> RayHit<'_> {
        let [a, b] = self.raster_vertices::<true>(&self.pick_pose(), hit.topology, hit.vertex);
        let interpolate = |a: [f32; 3], b: [f32; 3]| {
            Vector3::from(a) * hit.weights[0] + Vector3::from(b) * hit.weights[1]
        };
        let local = interpolate(a.position, b.position);
        let normal = interpolate(a.normal, b.normal)
            .try_normalize(1e-12)
            .unwrap_or_else(|| Vector3::from(a.normal).normalize());
        RayHit {
            object_id: &self.id,
            row_index: self.source_row.map(|row| row.get() - 1),
            imported_placement: self.imported_placement,
            distance: hit.distance,
            position: (self.transform.origin + self.transform.linear * local).into(),
            normal: (self.transform.inverse.transpose() * normal)
                .normalize()
                .into(),
            local_position: local.into(),
            local_normal: normal.into(),
            uv: std::array::from_fn(|i| a.uv[i] * hit.weights[0] + b.uv[i] * hit.weights[1]),
        }
    }
}
