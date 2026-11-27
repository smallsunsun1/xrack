//! Imported mesh validation and decoding of baked geometry data.

use nalgebra::Vector3;
use std::sync::{Arc, OnceLock};

use super::{
    Mesh3D, MeshData, MeshGeometry, MeshHeightField, MeshTangent, MeshVertex, MeshVertexStreams,
};
use crate::scene3d::mesh::{
    animation::MeshAsset,
    indices::MIN_INDEXED_VERTICES,
    material::{MeshMaterial, MeshPart, MeshPbr},
};

pub const MAX_IMPORTED_VERTICES: usize = 900_000;

/// Import-time preparation for a mesh file. Both options default to false.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MeshImportOptions {
    pub automatic_lod: bool,
    pub lightmap_uv: bool,
}

impl MeshImportOptions {
    pub fn from_operation(name: &str) -> Option<Self> {
        let [automatic_lod, lightmap_uv] = match name {
            "xr.mesh_url" => [false, false],
            "xr.mesh_lod_url" => [true, false],
            "xr.mesh_lightmap_url" => [false, true],
            "xr.mesh_lod_lightmap_url" => [true, true],
            _ => return None,
        };
        Some(Self {
            automatic_lod,
            lightmap_uv,
        })
    }
}

/// An immutable mesh supplied by a host at runtime. Clones share the prepared
/// geometry and textures. The default is empty.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshAsset3D {
    asset: Option<MeshAsset>,
    bounds: crate::Bounds3D,
}

impl MeshAsset3D {
    #[doc(hidden)]
    pub fn new(asset: MeshAsset) -> Self {
        let bounds = crate::scene3d::hierarchy::rest_mesh_bounds(&asset);
        Self {
            asset: Some(asset),
            bounds,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.asset.is_some()
    }

    /// The primary mesh in its initial pose, before placement in a scene.
    pub fn bounds(&self) -> crate::Bounds3D {
        self.bounds
    }

    pub fn node(&self, selector: &str) -> crate::MeshNode3D {
        self.asset
            .as_ref()
            .map_or_else(crate::MeshNode3D::default, |asset| asset.node(selector))
    }

    pub fn node_at(&self, source: i64) -> crate::MeshNode3D {
        self.asset
            .as_ref()
            .map_or_else(crate::MeshNode3D::default, |asset| asset.node_at(source))
    }

    pub fn node_indices(&self, kind: &str) -> crate::VecModel<i64> {
        self.asset
            .as_ref()
            .map_or_else(crate::VecModel::default, |asset| asset.node_indices(kind))
    }

    pub fn mesh_nodes(&self, kind: &str) -> Vec<crate::MeshNode3D> {
        self.asset
            .as_ref()
            .map_or_else(Vec::new, |asset| asset.mesh_nodes(kind))
    }

    pub fn material_slot_selectors(&self, variant: &str) -> crate::VecModel<String> {
        self.asset
            .as_ref()
            .map_or_else(crate::VecModel::default, |asset| {
                asset.material_slot_selectors(variant)
            })
    }

    pub fn material_variant_selectors(&self) -> crate::VecModel<String> {
        self.asset.as_ref().map_or_else(
            crate::VecModel::default,
            MeshAsset::material_variant_selectors,
        )
    }

    pub fn animation_clip_selectors(&self) -> crate::VecModel<String> {
        self.asset.as_ref().map_or_else(
            crate::VecModel::default,
            MeshAsset::animation_clip_selectors,
        )
    }

    pub fn animation_duration(&self, selector: &str) -> f64 {
        self.asset
            .as_ref()
            .map_or(0.0, |asset| asset.animation_duration(selector))
    }

    #[doc(hidden)]
    pub fn asset(&self) -> Option<&MeshAsset> {
        self.asset.as_ref()
    }
}

pub fn mesh_resource_key(path: &str, options: MeshImportOptions) -> String {
    let prefix = match (options.automatic_lod, options.lightmap_uv) {
        (false, false) => "asset",
        (true, false) => "asset-lod",
        (false, true) => "asset-lightmap",
        (true, true) => "asset-lod-lightmap",
    };
    format!("{prefix}:{path}")
}

impl Mesh3D {
    pub fn imported(
        vertices: Vec<MeshVertex>,
        tangents: Vec<MeshTangent>,
        parts: Vec<MeshPart>,
        materials: Vec<MeshMaterial>,
    ) -> Result<Self, &'static str> {
        Self::imported_shared(vertices, tangents, parts, materials.into())
    }

    #[doc(hidden)]
    pub fn imported_shared(
        vertices: Vec<MeshVertex>,
        tangents: Vec<MeshTangent>,
        parts: Vec<MeshPart>,
        materials: Arc<[MeshMaterial]>,
    ) -> Result<Self, &'static str> {
        if vertices.is_empty()
            || !vertices.len().is_multiple_of(3)
            || vertices.len() > MAX_IMPORTED_VERTICES
        {
            return Err("mesh must contain complete triangles within the vertex limit");
        }
        let mut end = 0;
        for part in &parts {
            if part.vertices.start != end
                || part.vertices.is_empty()
                || !part.vertices.len().is_multiple_of(3)
                || part.material >= materials.len()
            {
                return Err("mesh parts must cover contiguous triangles with valid materials");
            }
            end = part.vertices.end;
        }
        if end != vertices.len() {
            return Err("mesh parts do not cover the vertex buffer");
        }
        let needs_tangents = parts.iter().any(|part| {
            materials[part.material]
                .pbr
                .as_ref()
                .and_then(MeshPbr::tangent_normal_map)
                .is_some()
        });
        if (needs_tangents || !tangents.is_empty()) && tangents.len() != vertices.len() {
            return Err("mesh tangent data does not match the vertex buffer");
        }
        if materials.iter().any(|material| {
            !material
                .base_color
                .iter()
                .all(|channel| (0.0..=1.0).contains(channel))
                || material
                    .alpha_mode
                    .cutoff()
                    .is_some_and(|alpha| !alpha.is_finite() || alpha < 0.0)
                || material
                    .base_texture
                    .as_ref()
                    .is_some_and(|texture| texture.uv_slot != 0 || !texture.transform.is_valid())
                || material.pbr.as_ref().is_some_and(|pbr| !pbr.is_valid())
        }) {
            return Err("mesh contains an invalid material factor");
        }
        if parts
            .iter()
            .filter_map(|part| part.volume_inverse)
            .flat_map(|inverse| inverse.into_iter().flatten())
            .any(|value| !value.is_finite())
        {
            return Err("invalid mesh volume transform");
        }
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        let mut part_centers = Vec::with_capacity(parts.len());
        for part in &parts {
            let mut low = [f32::INFINITY; 3];
            let mut high = [f32::NEG_INFINITY; 3];
            for vertex in &vertices[part.vertices.clone()] {
                if !vertex
                    .position
                    .into_iter()
                    .chain(vertex.normal)
                    .chain(vertex.uv)
                    .chain(vertex.uv1)
                    .all(f32::is_finite)
                    || !vertex
                        .color
                        .iter()
                        .all(|channel| (0.0..=1.0).contains(channel))
                    || Vector3::from(vertex.normal).norm_squared() < 1e-8
                {
                    return Err("mesh contains an invalid position or normal");
                }
                for axis in 0..3 {
                    low[axis] = low[axis].min(vertex.position[axis]);
                    high[axis] = high[axis].max(vertex.position[axis]);
                }
            }
            for axis in 0..3 {
                min[axis] = min[axis].min(low[axis]);
                max[axis] = max[axis].max(high[axis]);
            }
            part_centers.push(std::array::from_fn(|axis| {
                f32::midpoint(low[axis], high[axis])
            }));
        }
        let geometry = Arc::new(MeshGeometry {
            streams: Arc::new(MeshVertexStreams {
                draw_indices: (vertices.len() >= MIN_INDEXED_VERTICES).then(Box::default),
                vertices,
                tangents,
                binormals: Vec::new(),
                lightmap_uv: Vec::new(),
            }),
            parts,
            part_centers,
            lightmap_min_size: 0,
            min,
            max,
            skin: None,
            morph: None,
            heightfield: None,
            node_catalog: None,
            pick_tree: OnceLock::new(),
            primitive: None,
            authored_tangents: false,
        });
        Ok(Self::Imported(Arc::new(MeshData::with_materials(
            geometry, materials, None,
        ))))
    }

    /// Authored or generated lightmap coordinates, independent of material UV slots.
    pub fn lightmap_uvs(&self) -> &[[f32; 2]] {
        match self {
            Self::Imported(mesh) => &mesh.lightmap_uv,
            _ => &[],
        }
    }

    pub fn lightmap_min_size(&self) -> u32 {
        match self {
            Self::Imported(mesh) => mesh.lightmap_min_size,
            _ => 0,
        }
    }

    #[doc(hidden)]
    pub fn with_lightmap_uv(self, uv: Vec<[f32; 2]>) -> Option<Self> {
        self.attach_lightmap_uv(uv, 0)
    }

    #[doc(hidden)]
    pub fn heightfield(&self) -> Option<&MeshHeightField> {
        let Self::Imported(data) = self else {
            return None;
        };
        data.heightfield.as_deref()
    }

    #[doc(hidden)]
    pub fn with_heightfield(self, dimensions: [usize; 2], samples: Vec<f32>) -> Option<Self> {
        let [width, height] = dimensions;
        if width < 2
            || height < 2
            || width.checked_mul(height)? != samples.len()
            || samples.iter().any(|height| !(0.0..=1.0).contains(height))
        {
            return None;
        }
        self.attach_heightfield(Arc::new(MeshHeightField {
            dimensions,
            samples: samples.into_boxed_slice(),
        }))
    }

    #[doc(hidden)]
    pub fn with_packed_heightfield(self, dimensions: [usize; 2], bytes: &[u8]) -> Option<Self> {
        let (words, remainder) = bytes.as_chunks::<4>();
        if !remainder.is_empty() {
            return None;
        }
        self.with_heightfield(
            dimensions,
            words.iter().copied().map(f32::from_le_bytes).collect(),
        )
    }

    fn attach_heightfield(mut self, heightfield: Arc<MeshHeightField>) -> Option<Self> {
        let Self::Imported(data) = &mut self else {
            return None;
        };
        Arc::make_mut(&mut Arc::make_mut(data).geometry).heightfield = Some(heightfield);
        Some(self)
    }

    #[doc(hidden)]
    pub fn with_generated_lightmap_uv(mut self, uv: Vec<[f32; 2]>, min_size: u32) -> Option<Self> {
        let Self::Imported(source) = &mut self else {
            return None;
        };
        if source.skin.is_some() || source.morph.is_some() {
            return None;
        }
        if Arc::get_mut(source).is_some() {
            return self.attach_lightmap_uv(uv, min_size);
        }
        let mut mesh = Self::imported_shared(
            source.vertices.clone(),
            source.tangents.clone(),
            source.parts.clone(),
            source.materials.clone(),
        )
        .ok()?
        .with_material_slots(source.material_slots.clone())?;
        if let Some(heightfield) = &source.heightfield {
            mesh = mesh.attach_heightfield(heightfield.clone())?;
        }
        mesh.attach_lightmap_uv(uv, min_size)
    }

    pub(super) fn attach_lightmap_uv(mut self, uv: Vec<[f32; 2]>, min_size: u32) -> Option<Self> {
        if uv.is_empty() {
            return Some(self);
        }
        let Self::Imported(mesh) = &mut self else {
            return None;
        };
        let data = Arc::get_mut(&mut Arc::get_mut(mesh)?.geometry)?;
        if uv.len() != data.vertices.len() || !uv.iter().flatten().all(|value| value.is_finite()) {
            return None;
        }
        let streams = Arc::get_mut(&mut data.streams)?;
        streams.lightmap_uv = uv;
        streams.invalidate_indices();
        data.lightmap_min_size = min_size;
        Some(self)
    }

    #[doc(hidden)]
    pub fn with_packed_lightmap_uv(self, bytes: &[u8], min_size: u32) -> Option<Self> {
        let (coordinates, rest) = bytes.as_chunks::<8>();
        if !rest.is_empty() || coordinates.len() > MAX_IMPORTED_VERTICES {
            return None;
        }
        self.attach_lightmap_uv(
            coordinates
                .iter()
                .map(|uv| {
                    let [x, y] = uv.as_chunks::<4>().0 else {
                        unreachable!()
                    };
                    [*x, *y].map(f32::from_le_bytes)
                })
                .collect(),
            min_size,
        )
    }

    /// Read the compact triangle data emitted by the AOT resource compiler.
    #[doc(hidden)]
    pub fn from_packed(
        bytes: &[u8],
        tangent_bytes: &[u8],
        parts: Vec<MeshPart>,
        materials: Vec<MeshMaterial>,
    ) -> Option<Self> {
        if !bytes.len().is_multiple_of(MeshVertex::STRIDE)
            || bytes.len() / MeshVertex::STRIDE > MAX_IMPORTED_VERTICES
            || !tangent_bytes.len().is_multiple_of(8)
            || tangent_bytes.len() / 8 > MAX_IMPORTED_VERTICES
        {
            return None;
        }
        let vertices = bytes
            .as_chunks::<{ MeshVertex::STRIDE }>()
            .0
            .iter()
            .map(|row| {
                let [x, y, z, nx, ny, nz, u, v, r, g, b, a, u1, v1] = row.as_chunks::<4>().0 else {
                    unreachable!()
                };
                MeshVertex {
                    position: [*x, *y, *z].map(f32::from_le_bytes),
                    normal: [*nx, *ny, *nz].map(f32::from_le_bytes),
                    uv: [*u, *v].map(f32::from_le_bytes),
                    uv1: [*u1, *v1].map(f32::from_le_bytes),
                    color: [*r, *g, *b, *a].map(f32::from_le_bytes),
                }
            })
            .collect();
        let tangents = tangent_bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|bytes| {
                let [x, y, z, w] = bytes.as_chunks::<2>().0 else {
                    unreachable!()
                };
                [*x, *y, *z, *w].map(i16::from_le_bytes)
            })
            .collect();
        Self::imported(vertices, tangents, parts, materials).ok()
    }
}
