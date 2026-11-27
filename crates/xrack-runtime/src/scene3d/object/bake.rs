//! Lightmap surface sampling and occlusion.

use nalgebra::Vector3;

use super::SceneObject;
use crate::scene3d::{
    BakedSurface, MaterialChannel, MeshMaterial, MeshVertex, MeshWorkflow, Ray3D,
};

impl SceneObject {
    pub(in crate::scene3d) fn bake_occludes(&self, ray: Ray3D, max: f32) -> bool {
        self.surface_hit(ray, 0.0, max, true).is_some()
    }

    pub(in crate::scene3d) fn bake_hit(
        &self,
        ray: Ray3D,
        min: f32,
        max: f32,
    ) -> Option<(f32, BakedSurface)> {
        let hit = self.surface_hit(ray, min, max, true)?;
        let parts = self.mesh.parts();
        let part = parts.partition_point(|part| part.vertices.end <= hit.vertex);
        let material = parts
            .get(part)
            .and_then(|part| self.mesh.materials().get(part.material));
        self.bake_sample(hit.vertex, hit.weights, material, hit.back_face)
            .map(|sample| (hit.distance, sample))
    }

    /// Evaluate the same material UVs and world pose used by the opaque draw.
    #[doc(hidden)]
    pub fn bake_sample(
        &self,
        vertex: usize,
        weights: [f32; 3],
        material: Option<&MeshMaterial>,
        back_face: bool,
    ) -> Option<BakedSurface> {
        let triangle = *self.mesh.vertices().as_chunks::<3>().0.get(vertex / 3)?;
        let vertices = self.pick_pose().deform::<true>(vertex, triangle);
        let interpolate = |value: fn(&MeshVertex) -> [f32; 3]| {
            vertices
                .iter()
                .zip(weights)
                .fold(Vector3::zeros(), |sum, (vertex, weight)| {
                    sum + Vector3::from(value(vertex)) * weight
                })
        };
        let local = interpolate(|vertex| vertex.position);
        let position = self.transform.linear * local + self.transform.origin;
        let normal = (self.transform.inverse.transpose() * interpolate(|vertex| vertex.normal))
            .try_normalize(1e-12)?
            * if back_face { -1.0 } else { 1.0 };
        let interpolate_uv = |slot| {
            std::array::from_fn(|axis| {
                vertices
                    .iter()
                    .zip(weights)
                    .map(|(vertex, weight)| {
                        let uv = if slot == 0 { vertex.uv } else { vertex.uv1 };
                        uv[axis] * weight
                    })
                    .sum()
            })
        };
        let uv = interpolate_uv(0);
        let rgba = std::array::from_fn::<_, 4, _>(|channel| {
            vertices
                .iter()
                .zip(weights)
                .map(|(vertex, weight)| vertex.color[channel] * weight)
                .sum()
        });
        let [r, g, b, alpha] = rgba;
        let color = Vector3::new(r, g, b);
        let tint = Vector3::from(crate::scene3d::lighting::linear_color(self.color));
        let mut diffuse = color.component_mul(&tint);
        let mut emission = Vector3::zeros();
        if let Some(material) = material {
            if !material.depth_covers(uv, alpha, f32::from(self.color.0 as u8) / 255.0) {
                return None;
            }
            if !material.vertex_colors {
                diffuse = tint;
            }
            let [r, g, b, _] = material.base_color;
            diffuse.component_mul_assign(&Vector3::new(r, g, b));
            if let Some(texture) = &material.base_texture {
                diffuse.component_mul_assign(&Vector3::from(
                    texture
                        .channel_color(uv, material.channels.channel(MaterialChannel::BaseColor)),
                ));
            }
            if let Some(pbr) = &material.pbr {
                if pbr.workflow == MeshWorkflow::SpecularGlossiness {
                    let mut specular = [1.0; 3];
                    let mut weight = material.vertex_masks.specular(rgba);
                    if let Some(dielectric) = &pbr.dielectric {
                        specular = dielectric.color;
                        weight *= dielectric.specular;
                        if let Some(map) = &dielectric.color_texture {
                            let tint = map.channel_color(
                                interpolate_uv(map.uv_slot),
                                material.channels.channel(MaterialChannel::SpecularColor),
                            );
                            specular = std::array::from_fn(|index| specular[index] * tint[index]);
                        }
                        if let Some(map) = &dielectric.specular_texture {
                            weight *= map.scalar_at(
                                interpolate_uv(map.uv_slot),
                                material.channels.channel(MaterialChannel::Specular),
                            );
                        }
                    }
                    diffuse *=
                        1.0 - specular.into_iter().fold(0.0, f32::max).clamp(0.0, 1.0) * weight;
                } else {
                    let metallic = pbr.metallic
                        * material.vertex_masks.metalness(rgba)
                        * pbr.surface_texture.as_ref().map_or(1.0, |map| {
                            map.scalar_at(
                                interpolate_uv(map.uv_slot),
                                material.channels.channel(MaterialChannel::Metalness),
                            )
                        });
                    diffuse *= 1.0 - metallic.clamp(0.0, 1.0);
                }
                emission = Vector3::from(pbr.emissive);
                if let Some(map) = &pbr.emissive_texture {
                    emission.component_mul_assign(&Vector3::from(map.channel_color(
                        interpolate_uv(map.uv_slot),
                        material.channels.channel(MaterialChannel::Emissive),
                    )));
                }
            } else {
                emission = diffuse;
                diffuse = Vector3::zeros();
            }
        }
        Some(BakedSurface {
            position: position.into(),
            normal: normal.into(),
            diffuse: diffuse.map(|channel| channel.clamp(0.0, 1.0)).into(),
            emission: emission.into(),
            layers: self.layers,
            receives_shadow: self.receives_shadow,
            lighting_index: self.lighting_index,
        })
    }
}
