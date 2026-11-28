//! Material assignment, draw coverage, and cached capture variants.

use crate::Color;
use std::{
    collections::VecDeque,
    ops::Range,
    sync::{Arc, Mutex, OnceLock, Weak},
};

use super::{Mesh3D, MeshData, MeshGeometry, Primitive};
use crate::scene3d::{
    effect::CaptureRenderables,
    mesh::material::{
        MeshAlphaMode, MeshCullMode, MeshDepthDraw, MeshLighting, MeshMaterial, MeshMaterialSlot,
        MeshPart, MeshPbr, MeshShader, MeshTopology, SceneAppearance, SceneItemTexture,
        SceneMaterial,
    },
};

impl MeshData {
    #[doc(hidden)]
    pub fn instance_blend_primitive(&self) -> Option<Mesh3D> {
        if !matches!(&*self.materials,
            [material] if matches!(material.alpha_mode, MeshAlphaMode::InstanceBlend { builtin: true, .. })
        ) {
            return None;
        }
        match self.geometry.primitive? {
            Primitive::Box => Some(Mesh3D::Box),
            Primitive::Sphere => Some(Mesh3D::Sphere),
            _ => None,
        }
    }

    /// A bare built-in primitive keeps its ordinary surface shading when posed.
    #[doc(hidden)]
    pub fn model_blend_primitive(&self) -> Option<Mesh3D> {
        self.model_blend.as_ref()?;
        if !Arc::ptr_eq(&self.materials, model_blend_primitive_materials()) {
            return None;
        }
        match self.geometry.primitive? {
            Primitive::Box => Some(Mesh3D::Box),
            Primitive::Sphere => Some(Mesh3D::Sphere),
            _ => None,
        }
    }

    #[doc(hidden)]
    pub fn uses_base_items(&self) -> bool {
        self.uses_base_items
    }

    #[doc(hidden)]
    pub fn uses_item_textures(&self) -> bool {
        !self.item_sources.is_empty()
    }

    #[doc(hidden)]
    pub fn item_sources(&self) -> &[String] {
        &self.item_sources
    }

    #[doc(hidden)]
    pub fn capture_sources(&self) -> &[crate::scene3d::effect::CaptureRead] {
        &self.capture_sources
    }

    #[doc(hidden)]
    pub fn uses_capture_positions(&self) -> bool {
        self.capture_positions
    }

    #[doc(hidden)]
    pub fn wide_points(&self) -> Option<&Arc<[Range<usize>]>> {
        self.wide_points.as_ref()
    }

    #[doc(hidden)]
    pub fn wide_lines(&self) -> Option<&Arc<[Range<usize>]>> {
        self.wide_lines.as_ref()
    }

    pub(super) fn with_materials(
        geometry: Arc<MeshGeometry>,
        materials: Arc<[MeshMaterial]>,
        authored: Option<Arc<SceneAppearance>>,
    ) -> Self {
        let mut blended_parts = Vec::new();
        let mut transmission_blur = None::<f32>;
        let mut has_opaque_parts = geometry.parts.is_empty();
        let mut raster_size = 0.0_f32;
        let mut wide_points = Vec::new();
        let mut wide_lines = Vec::new();
        let mut has_auxiliary_depth = geometry.parts.is_empty();
        let mut has_opaque_depth_prepass = geometry.parts.is_empty();
        let mut has_face_culling = false;
        let mut depth_prepasses = [false; 2];
        let mut blended_depth = false;
        let mut has_depth_writing_transmission = false;
        let mut has_volume = false;
        for (index, part) in geometry.parts.iter().enumerate() {
            let material = &materials[part.material];
            if !material.is_visible() {
                continue;
            }
            raster_size = raster_size.max(match part.topology {
                MeshTopology::Triangles => 0.0,
                MeshTopology::Points => material.point_size,
                MeshTopology::Lines => material.line_width,
            });
            match part.topology {
                MeshTopology::Points if material.point_size > 1.0 => {
                    wide_points.push(part.vertices.clone())
                }
                MeshTopology::Lines if material.line_width > 1.0 => {
                    wide_lines.push(part.vertices.clone())
                }
                _ => {}
            }
            let blended = material.is_blended();
            let writes_depth = material.depth_coverage().is_some();
            has_face_culling |= material.cull_mode != MeshCullMode::None;
            has_auxiliary_depth |= writes_depth;
            blended_depth |= writes_depth && blended;
            let transmission = (part.lighting != MeshLighting::BaseAndEmission)
                .then(|| material.transmission_blur())
                .flatten();
            has_opaque_depth_prepass |= material
                .depth_draw_mode
                .admits_opaque_prepass(blended, transmission.is_some());
            if writes_depth && material.depth_draw_mode == MeshDepthDraw::OpaquePrepass {
                depth_prepasses[usize::from(transmission.is_some())] = true;
            }
            if blended {
                blended_parts.push((index, geometry.part_centers[index]));
            }
            if let Some(blur) = transmission {
                transmission_blur = Some(transmission_blur.map_or(blur, |before| before.max(blur)));
                has_depth_writing_transmission |= !blended;
                has_volume |= material.pbr.as_ref().and_then(MeshPbr::volume).is_some();
            } else if !blended {
                has_opaque_parts = true;
            }
        }
        Self {
            geometry,
            model_blend: None,
            material_slots: Arc::from([]),
            slot_selectors: Default::default(),
            uses_base_items: materials.iter().any(|material| {
                material.base_item.is_some()
                    || material
                        .pbr
                        .as_ref()
                        .and_then(|pbr| pbr.specular_reflection.as_deref())
                        .is_some_and(|reflection| reflection.item.is_some())
            }),
            item_sources: materials
                .iter()
                .flat_map(|material| {
                    material
                        .base_item
                        .as_deref()
                        .map(SceneItemTexture::source)
                        .into_iter()
                        .chain(
                            material
                                .pbr
                                .as_ref()
                                .and_then(|pbr| pbr.specular_reflection.as_deref())
                                .and_then(|reflection| reflection.item.as_deref())
                                .map(SceneItemTexture::source),
                        )
                        .chain(material.shader.iter().flat_map(MeshShader::item_sources))
                })
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            capture_positions: materials
                .iter()
                .filter_map(|material| material.shader.as_ref())
                .any(|shader| {
                    crate::scene3d::material_shader::MaterialHooks::from_source(&shader.source)
                        .capture_position
                        && shader
                            .capture_sources(crate::scene3d::material_shader::MAX_TEXTURES)
                            .any(|read| {
                                read.attachment == crate::scene3d::effect::CaptureAttachment::Depth
                            })
                }),
            capture_sources: materials
                .iter()
                .filter_map(|material| material.shader.as_ref())
                .flat_map(|shader| {
                    shader.capture_sources(crate::scene3d::material_shader::MAX_TEXTURES)
                })
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .cloned()
                .collect(),
            shader_displacement: materials
                .iter()
                .map(|material| material.vertex_parameters()[3])
                .fold(0.0, f32::max),
            instance_blended: materials
                .iter()
                .any(|material| matches!(material.alpha_mode, MeshAlphaMode::InstanceBlend { .. })),
            materials,
            source_materials: None,
            blended_parts,
            transmission_blur,
            has_opaque_parts,
            raster_size,
            wide_points: (!wide_points.is_empty()).then(|| wide_points.into()),
            wide_lines: (!wide_lines.is_empty()).then(|| wide_lines.into()),
            has_auxiliary_depth,
            has_opaque_depth_prepass,
            has_face_culling,
            depth_prepasses,
            blended_depth,
            has_depth_writing_transmission,
            has_volume,
            authored,
            derivative_normals: Vec::new(),
        }
    }
}

fn default_mesh_material() -> MeshMaterial {
    MeshMaterial {
        point_size: 1.0,
        line_width: 1.0,
        opacity: 1.0,
        opacity_texture: None,
        invert_opacity_map_value: false,
        shader: None,
        channels: Default::default(),
        vertex_colors: true,
        vertex_masks: Default::default(),
        cull_mode: MeshCullMode::Back,
        depth_draw_mode: Default::default(),
        blend_mode: Default::default(),
        base_color: [1.0; 4],
        base_texture: None,
        base_item: None,
        alpha_mode: MeshAlphaMode::Opaque,
        pbr: Some(MeshPbr::principled()),
    }
}

pub(super) fn model_blend_primitive_materials() -> &'static Arc<[MeshMaterial]> {
    static MATERIALS: OnceLock<Arc<[MeshMaterial]>> = OnceLock::new();
    MATERIALS.get_or_init(|| {
        Arc::from([MeshMaterial {
            cull_mode: MeshCullMode::None,
            pbr: None,
            ..default_mesh_material()
        }])
    })
}

#[derive(Debug)]
#[doc(hidden)]
pub struct CaptureMaterial {
    style: SceneMaterial,
    replacement: Arc<[MeshMaterial]>,
    meshes: Mutex<VecDeque<CaptureMesh>>,
}

#[derive(Debug)]
struct CaptureMesh {
    source: Weak<MeshData>,
    filter: CaptureRenderables,
    mesh: Option<Mesh3D>,
}

impl PartialEq for CaptureMaterial {
    fn eq(&self, other: &Self) -> bool {
        self.style == other.style
    }
}

impl CaptureMaterial {
    pub(crate) fn new(style: SceneMaterial) -> Arc<Self> {
        let replacement = Arc::from([style.prepare(&default_mesh_material())]);
        Arc::new(Self {
            style,
            replacement,
            meshes: Mutex::default(),
        })
    }

    pub fn material(&self) -> &MeshMaterial {
        &self.replacement[0]
    }

    pub fn project(&self, mesh: &Mesh3D, filter: CaptureRenderables) -> Option<Mesh3D> {
        let source = mesh.material_source()?;
        let mut meshes = self
            .meshes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = meshes.iter().position(|entry| {
            entry.source.as_ptr() == Arc::as_ptr(source) && entry.filter == filter
        }) && let Some(entry) = meshes.remove(index)
        {
            let mesh = entry.mesh.clone();
            meshes.push_front(entry);
            return mesh;
        }
        let mut geometry = source.geometry.as_ref().clone();
        let sections = geometry
            .parts
            .drain(..)
            .zip(geometry.part_centers.drain(..))
            .filter(|(part, _)| filter.includes(source.materials[part.material].is_blended()))
            .map(|(mut part, center)| {
                part.material = 0;
                (part, center)
            });
        let (parts, centers): (Vec<_>, Vec<_>) = sections.unzip();
        let mesh = (!parts.is_empty()).then(|| {
            geometry.parts = parts;
            geometry.part_centers = centers;
            geometry.pick_tree = OnceLock::new();
            geometry.raster_tree = OnceLock::new();
            let mut data =
                MeshData::with_materials(Arc::new(geometry), self.replacement.clone(), None);
            data.model_blend.clone_from(&source.model_blend);
            data.derivative_normals = vec![self.style.overrides_normal_texture()];
            Mesh3D::Imported(Arc::new(data))
        });
        meshes.retain(|entry| entry.source.strong_count() > 0);
        meshes.truncate(63);
        meshes.push_front(CaptureMesh {
            source: Arc::downgrade(source),
            filter,
            mesh: mesh.clone(),
        });
        mesh
    }
}

impl Mesh3D {
    pub(in crate::scene3d) fn material_source(&self) -> Option<&Arc<MeshData>> {
        let builtin = |mesh: &Mesh3D| {
            let vertices = mesh.vertices().to_vec();
            let parts = vec![MeshPart {
                lighting: Default::default(),
                topology: crate::scene3d::MeshTopology::Triangles,
                vertices: 0..vertices.len(),
                material: 0,
                volume_inverse: None,
            }];
            let materials = vec![default_mesh_material()];
            let Self::Imported(mut source) =
                Self::imported(vertices, Vec::new(), parts, materials).ok()?
            else {
                return None;
            };
            Arc::get_mut(&mut Arc::get_mut(&mut source)?.geometry)?.primitive = mesh.primitive();
            Some(source)
        };
        static BOX: OnceLock<Option<Arc<MeshData>>> = OnceLock::new();
        static SPHERE: OnceLock<Option<Arc<MeshData>>> = OnceLock::new();
        match self {
            Self::Box => BOX.get_or_init(|| builtin(self)).as_ref(),
            Self::Sphere => SPHERE.get_or_init(|| builtin(self)).as_ref(),
            Self::Imported(data) => Some(data),
        }
    }

    pub(crate) fn styled(self, style: Option<&Arc<SceneAppearance>>) -> Self {
        let Some(style) = style else {
            return self;
        };
        let Some(source) = self.material_source() else {
            return self;
        };
        let mut changed = false;
        let mut derivative_normals = Vec::with_capacity(source.materials.len());
        let materials = source
            .materials
            .iter()
            .enumerate()
            .map(|(index, material)| {
                let style = style
                    .styles(source.material_slots.get(index), index)
                    .cloned()
                    .reduce(|mut before, later| {
                        before.overlay(&later);
                        before
                    });
                let Some(resolved) = style else {
                    derivative_normals.push(false);
                    return material.clone();
                };
                changed = true;
                let prepared = resolved.prepare(material);
                let authored_basis = source.authored_tangents
                    && prepared
                        .pbr
                        .as_ref()
                        .and_then(MeshPbr::tangent_normal_map)
                        .is_some_and(|map| {
                            let texture = &map.texture;
                            texture.uv_slot == 0
                                && texture.transform.map_linear([1.0, 0.0]) == [1.0, 0.0]
                                && texture.transform.map_linear([0.0, 1.0]) == [0.0, 1.0]
                        });
                derivative_normals.push(resolved.overrides_normal_texture() && !authored_basis);
                prepared
            })
            .collect();
        if !changed {
            return self;
        }
        let mut result =
            MeshData::with_materials(source.geometry.clone(), materials, Some(style.clone()));
        result.source_materials = Some(source.materials.clone());
        result.model_blend.clone_from(&source.model_blend);
        result.derivative_normals = derivative_normals;
        result.material_slots = source.material_slots.clone();
        Self::Imported(Arc::new(result))
    }

    pub(in crate::scene3d) fn with_particle_tint(self, tint: Color) -> Option<Self> {
        if tint.0 == 0xffffffff {
            return Some(self);
        }
        let source = self.material_source()?;
        let [r, g, b] = crate::scene3d::lighting::linear_color(tint);
        let rgba = [r, g, b, f32::from(tint.0 as u8) / 255.0];
        let materials = source
            .materials
            .iter()
            .cloned()
            .map(|mut material| {
                for (value, scale) in material.base_color.iter_mut().zip(rgba) {
                    *value *= scale;
                }
                material
            })
            .collect();
        let mut result = MeshData::with_materials(source.geometry.clone(), materials, None);
        result
            .derivative_normals
            .clone_from(&source.derivative_normals);
        result.material_slots = source.material_slots.clone();
        Some(Self::Imported(Arc::new(result)))
    }

    pub(in crate::scene3d) fn with_particle_basis(
        mut self,
        binormals: Vec<super::MeshTangent>,
        source: &MeshData,
    ) -> Option<Self> {
        let Self::Imported(data) = &mut self else {
            return None;
        };
        let data = Arc::get_mut(data)?;
        data.derivative_normals
            .clone_from(&source.derivative_normals);
        if !binormals.is_empty() {
            let geometry = Arc::get_mut(&mut data.geometry)?;
            let streams = Arc::get_mut(&mut geometry.streams)?;
            if streams.tangents.len() != streams.vertices.len()
                || binormals.len() != streams.vertices.len()
            {
                return None;
            }
            streams.binormals = binormals;
            streams.invalidate_indices();
        }
        Some(self)
    }

    pub(crate) fn with_instance_blending(self) -> Self {
        let builtin = matches!(self, Self::Box | Self::Sphere);
        let Some(source) = self.material_source() else {
            return self;
        };
        if source.materials.iter().all(|material| {
            matches!(
                material.effective_alpha_mode(),
                MeshAlphaMode::Blend | MeshAlphaMode::InstanceBlend { .. }
            )
        }) {
            return self;
        }
        let materials = source
            .materials
            .iter()
            .cloned()
            .map(|mut material| {
                material.alpha_mode = match material.alpha_mode {
                    MeshAlphaMode::Default => MeshAlphaMode::InstanceBlend {
                        cutoff: None,
                        builtin: false,
                        source_alpha: true,
                    },
                    MeshAlphaMode::Opaque => MeshAlphaMode::InstanceBlend {
                        cutoff: None,
                        builtin,
                        source_alpha: false,
                    },
                    MeshAlphaMode::Mask(cutoff) => MeshAlphaMode::InstanceBlend {
                        cutoff: Some(cutoff),
                        builtin,
                        source_alpha: false,
                    },
                    mode => mode,
                };
                if builtin {
                    material.pbr = None;
                    material.cull_mode = MeshCullMode::None;
                }
                material
            })
            .collect();
        let mut mesh =
            MeshData::with_materials(source.geometry.clone(), materials, source.authored.clone());
        mesh.model_blend.clone_from(&source.model_blend);
        mesh.derivative_normals
            .clone_from(&source.derivative_normals);
        mesh.material_slots = source.material_slots.clone();
        mesh.source_materials = Some(
            source
                .source_materials
                .as_ref()
                .unwrap_or(&source.materials)
                .clone(),
        );
        Self::Imported(Arc::new(mesh))
    }

    #[doc(hidden)]
    pub fn material_slots(&self) -> &[MeshMaterialSlot] {
        match self {
            Self::Imported(data) => &data.material_slots,
            _ => &[],
        }
    }

    pub(in crate::scene3d::mesh) fn slot_selectors(&self) -> &[String] {
        match self {
            Self::Imported(data) => data.slot_selectors.get_or_init(|| {
                crate::scene3d::mesh::catalog::material_slots(std::slice::from_ref(self)).into()
            }),
            _ => &[],
        }
    }

    #[doc(hidden)]
    pub fn with_material_slots(mut self, slots: Arc<[MeshMaterialSlot]>) -> Option<Self> {
        if slots.is_empty() {
            return Some(self);
        }
        let Self::Imported(mesh) = &mut self else {
            return None;
        };
        let data = Arc::get_mut(mesh)?;
        if slots.len() != data.materials.len() {
            return None;
        }
        data.material_slots = slots;
        data.slot_selectors.take();
        Some(self)
    }

    #[doc(hidden)]
    pub fn uses_base_items(&self) -> bool {
        matches!(self, Self::Imported(mesh) if mesh.uses_base_items())
    }

    pub fn materials(&self) -> &[MeshMaterial] {
        match self {
            Self::Imported(mesh) => &mesh.materials,
            _ => &[],
        }
    }

    /// Authored normal maps use the exact transformed UV derivatives.
    #[doc(hidden)]
    pub fn uses_vertex_tangents(&self, material: usize) -> bool {
        matches!(self, Self::Imported(mesh) if !mesh.tangents.is_empty()
            && !mesh.derivative_normals.get(material).copied().unwrap_or(false))
    }

    pub fn transmission_blur(&self) -> Option<f32> {
        match self {
            Self::Imported(mesh) => mesh.transmission_blur,
            _ => None,
        }
    }

    pub fn has_volume(&self) -> bool {
        matches!(self, Self::Imported(mesh) if mesh.has_volume)
    }

    pub fn has_depth_writing_transmission(&self) -> bool {
        matches!(self, Self::Imported(mesh) if mesh.has_depth_writing_transmission)
    }

    #[doc(hidden)]
    pub fn shader_displacement(&self) -> f32 {
        match self {
            Self::Imported(mesh) => mesh.shader_displacement,
            _ => 0.0,
        }
    }

    /// Geometry and material state used by the scene's depth-only passes.
    #[doc(hidden)]
    pub fn same_shadow_mesh(&self, other: &Self) -> bool {
        self == other
            || match (self, other) {
                (Self::Imported(a), Self::Imported(b)) => {
                    Arc::ptr_eq(a.vertex_streams(), b.vertex_streams())
                        && a.model_blend == b.model_blend
                        && a.parts.len() == b.parts.len()
                        && a.parts.iter().zip(&b.parts).all(|(left, right)| {
                            let before = &a.materials[left.material];
                            let after = &b.materials[right.material];
                            let same_size = match left.topology {
                                MeshTopology::Triangles => true,
                                MeshTopology::Points => before.point_size == after.point_size,
                                MeshTopology::Lines => before.line_width == after.line_width,
                            };
                            left.vertices == right.vertices
                                && left.topology == right.topology
                                && same_size
                                && before.same_shadow_surface(after)
                        })
                }
                _ => false,
            }
    }

    #[doc(hidden)]
    pub fn has_auxiliary_depth(&self) -> bool {
        match self {
            Self::Imported(mesh) => mesh.has_auxiliary_depth,
            _ => true,
        }
    }

    #[doc(hidden)]
    pub fn has_opaque_depth_prepass(&self) -> bool {
        match self {
            Self::Imported(mesh) => mesh.has_opaque_depth_prepass,
            _ => true,
        }
    }

    #[doc(hidden)]
    pub fn depth_participation(&self) -> ([bool; 2], bool) {
        match self {
            Self::Imported(mesh) => (mesh.depth_prepasses, mesh.blended_depth),
            _ => ([false; 2], false),
        }
    }

    pub fn has_opaque_parts(&self) -> bool {
        match self {
            Self::Imported(mesh) => mesh.has_opaque_parts,
            _ => true,
        }
    }

    /// Model-space bounds centers of the blend parts, in source order.
    pub fn blended_parts(&self) -> &[(usize, [f32; 3])] {
        match self {
            Self::Imported(mesh) => &mesh.blended_parts,
            _ => &[],
        }
    }

    pub fn sections(&self) -> impl Iterator<Item = (Range<usize>, Option<&MeshMaterial>)> {
        self.sections_with_topology()
            .map(|(_, vertices, material)| (vertices, material))
    }

    /// Sections with surface area, used by CPU geometry queries and lightmapping.
    #[doc(hidden)]
    pub fn triangle_sections(&self) -> impl Iterator<Item = (Range<usize>, Option<&MeshMaterial>)> {
        self.sections_with_topology()
            .filter_map(|(topology, vertices, material)| {
                (topology == MeshTopology::Triangles).then_some((vertices, material))
            })
    }

    fn sections_with_topology(
        &self,
    ) -> impl Iterator<Item = (MeshTopology, Range<usize>, Option<&MeshMaterial>)> {
        let builtin = self
            .parts()
            .is_empty()
            .then(|| (MeshTopology::Triangles, 0..self.vertices().len(), None));
        builtin.into_iter().chain(self.parts().iter().map(|part| {
            (
                part.topology,
                part.vertices.clone(),
                Some(&self.materials()[part.material]),
            )
        }))
    }
}
