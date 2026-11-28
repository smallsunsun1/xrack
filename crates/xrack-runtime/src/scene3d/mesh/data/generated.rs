//! Built-in meshes and preparation of generated triangle parts.

use std::sync::{Arc, OnceLock};

use super::{Mesh3D, MeshTangent, MeshVertex};
use crate::scene3d::mesh::{
    material::{
        MeshAlphaMode, MeshCullMode, MeshMaterial, MeshMaterialSlot, MeshPart, MeshPbr,
        MeshTopology,
    },
    primitives::{self, PrimitiveParts},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::scene3d) enum Primitive {
    Box,
    Sphere,
    Rectangle,
    Cylinder,
    Cone,
    Capsule,
}

impl Mesh3D {
    pub fn resolve(key: &str, imported: impl FnOnce(&str) -> Option<Self>) -> Option<Self> {
        Self::builtin(key).or_else(|| imported(key))
    }

    pub(crate) fn builtin(key: &str) -> Option<Self> {
        static RECTANGLE: OnceLock<Option<Mesh3D>> = OnceLock::new();
        static CYLINDER: OnceLock<Option<Mesh3D>> = OnceLock::new();
        static CONE: OnceLock<Option<Mesh3D>> = OnceLock::new();

        match key {
            "primitive:box" => Some(Self::Box),
            "primitive:sphere" => Some(Self::Sphere),
            "primitive:rectangle" => RECTANGLE
                .get_or_init(|| {
                    Self::prepare_primitive(Primitive::Rectangle, primitives::rectangle())
                })
                .clone(),
            "primitive:cylinder" => CYLINDER
                .get_or_init(|| {
                    Self::prepare_primitive(Primitive::Cylinder, primitives::cylinder())
                })
                .clone(),
            "primitive:cone" => CONE
                .get_or_init(|| Self::prepare_primitive(Primitive::Cone, primitives::cone()))
                .clone(),
            _ => None,
        }
    }

    fn prepare_primitive(kind: Primitive, parts: PrimitiveParts) -> Option<Self> {
        Self::prepare_parts(Some(kind), MeshTopology::Triangles, parts, Vec::new())
    }

    pub(crate) fn prepare_capsule(parts: PrimitiveParts) -> Option<Self> {
        Self::prepare_primitive(Primitive::Capsule, parts)
    }

    pub(crate) fn prepare_procedural(parts: PrimitiveParts) -> Option<Self> {
        Self::prepare_parts(None, MeshTopology::Triangles, parts, Vec::new())
    }

    pub(crate) fn prepare_authored_mesh(
        topology: MeshTopology,
        vertices: Vec<MeshVertex>,
        tangents: Vec<MeshTangent>,
    ) -> Option<Self> {
        Self::prepare_parts(None, topology, vec![("surface", vertices)], tangents)
    }

    fn prepare_parts(
        kind: Option<Primitive>,
        topology: MeshTopology,
        parts: PrimitiveParts,
        tangents: Vec<MeshTangent>,
    ) -> Option<Self> {
        let mut vertices = Vec::new();
        let mut ranges = Vec::new();
        let mut slots = Vec::new();
        let mut materials = Vec::new();
        for (index, (name, part)) in parts.into_iter().enumerate() {
            let start = vertices.len();
            vertices.extend(part);
            ranges.push(MeshPart {
                lighting: Default::default(),
                topology,
                vertices: start..vertices.len(),
                material: index,
                volume_inverse: None,
            });
            slots.push(MeshMaterialSlot {
                source_index: Some(index as u32),
                name: Some(name.into()),
            });
            materials.push(MeshMaterial {
                point_size: 1.0,
                line_width: 1.0,
                opacity: 1.0,
                opacity_texture: None,
                invert_opacity_map_value: false,
                shader: None,
                channels: Default::default(),
                vertex_colors: true,
                vertex_masks: Default::default(),
                cull_mode: MeshCullMode::None,
                depth_draw_mode: Default::default(),
                blend_mode: Default::default(),
                base_color: [1.0; 4],
                base_texture: None,
                base_item: None,
                alpha_mode: MeshAlphaMode::Opaque,
                pbr: Some(MeshPbr::principled()),
            });
        }
        let Self::Imported(mut mesh) = Self::imported(vertices, tangents, ranges, materials)
            .ok()?
            .with_material_slots(slots.into())?
        else {
            return None;
        };
        let geometry = Arc::get_mut(&mut Arc::get_mut(&mut mesh)?.geometry)?;
        geometry.primitive = kind;
        geometry.authored_tangents = !geometry.tangents.is_empty();
        Some(Self::Imported(mesh))
    }

    pub(in crate::scene3d) fn primitive(&self) -> Option<Primitive> {
        match self {
            Self::Box => Some(Primitive::Box),
            Self::Sphere => Some(Primitive::Sphere),
            Self::Imported(mesh) if mesh.model_blend.is_none() => mesh.primitive,
            Self::Imported(_) => None,
        }
    }

    /// Expanded vertices in model space, grouped by the topology of each part.
    pub fn vertices(&self) -> &[MeshVertex] {
        static BOX: OnceLock<Vec<MeshVertex>> = OnceLock::new();
        static SPHERE: OnceLock<Vec<MeshVertex>> = OnceLock::new();
        match self {
            Self::Box => BOX.get_or_init(primitives::box_vertices),
            Self::Sphere => SPHERE.get_or_init(primitives::sphere_vertices),
            Self::Imported(mesh) => &mesh.vertices,
        }
    }
}
