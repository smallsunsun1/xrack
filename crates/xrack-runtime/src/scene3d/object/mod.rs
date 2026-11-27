//! A mesh placed in a scene with its transform, visibility, and instance state.

mod appearance;
mod bake;
mod collision;
mod instances;
mod picking;
mod pose;

use crate::Color;
use std::{num::NonZeroU32, sync::Arc};

pub use instances::InstanceOrder;
pub(crate) use instances::InstancingLodRange;

use super::{
    BakeRole, BakedLightmap, LightmapResolution, MAIN_LAYER, Mesh3D, SceneTransform,
    mesh::{morph::MorphPose, skin::SkinPose},
};

#[derive(Clone, Debug, PartialEq)]
pub struct SceneObject {
    pub id: String,
    pub mesh: Mesh3D,
    pub color: Color,
    pub(crate) opacity: f32,
    /// World-unit offset used only by depth-sorted transparency.
    depth_bias: f64,
    pub(crate) lod_coverage: u8,
    motion_vector_enabled: bool,
    motion_vector_scale: f32,
    pub(crate) instancing_lod: InstancingLodRange,
    pub(crate) instance_data: Option<[f32; 4]>,
    pub casts_shadow: bool,
    pub receives_shadow: bool,
    pub receives_reflections: bool,
    pub casts_reflections: bool,
    pub layers: u64,
    pub lightmap: Option<BakedLightmap>,
    pub bake_role: BakeRole,
    pub lightmap_resolution: LightmapResolution,
    source_row: Option<NonZeroU32>,
    pub(crate) lighting_index: u16,
    pub(crate) imported_placement: Option<(u32, Option<u32>)>,
    pub(crate) instance_order: Option<InstanceOrder>,
    pub(crate) instance_shadow_bounds: Option<Arc<[[f32; 3]; 2]>>,
    pickable: bool,
    collidable: bool,
    trigger: bool,
    transform: SceneTransform,
    skin: Option<Arc<SkinPose>>,
    morph: Option<Arc<MorphPose>>,
    styled_blend_centers: Option<Arc<[(usize, [f32; 3])]>>,
}

impl SceneObject {
    /// Resolve a mesh key separately from the object's stable identity.
    /// Built-in scene keys and imported mesh keys share this entry point.
    pub fn from_mesh(
        id: String,
        mesh: Mesh3D,
        center: [f64; 3],
        scale: [f64; 3],
        angles: [f64; 3],
        color: Color,
    ) -> Option<Self> {
        Self::new(
            id,
            mesh,
            center.map(|v| v as f32),
            scale.map(|v| v as f32),
            angles.map(|v| v as f32),
            color,
        )
    }

    /// Construct a box at its center using world dimensions and yaw, pitch,
    /// and roll in degrees (applied in pitch, yaw, roll order).
    pub fn cuboid(
        id: impl Into<String>,
        center: [f32; 3],
        dimensions: [f32; 3],
        angles: [f32; 3],
        color: Color,
    ) -> Option<Self> {
        Self::new(id.into(), Mesh3D::Box, center, dimensions, angles, color)
    }

    /// Construct a sphere from its center and world-space radius.
    pub fn sphere(
        id: impl Into<String>,
        center: [f32; 3],
        radius: f32,
        color: Color,
    ) -> Option<Self> {
        Self::new(
            id.into(),
            Mesh3D::Sphere,
            center,
            [radius; 3],
            [0.0; 3],
            color,
        )
    }

    fn new(
        id: String,
        mesh: Mesh3D,
        center: [f32; 3],
        scale: [f32; 3],
        angles: [f32; 3],
        color: Color,
    ) -> Option<Self> {
        let (transform, _) = SceneTransform::local(center, scale, angles, [0.0; 3])?;
        Some(Self::from_transform(id, mesh, transform, color))
    }

    pub(crate) fn from_transform(
        id: String,
        mesh: Mesh3D,
        transform: SceneTransform,
        color: Color,
    ) -> Self {
        Self {
            id,
            mesh,
            color,
            opacity: 1.0,
            depth_bias: 0.0,
            lod_coverage: 0,
            motion_vector_enabled: true,
            motion_vector_scale: 1.0,
            instancing_lod: InstancingLodRange::ALL,
            instance_data: None,
            casts_shadow: true,
            receives_shadow: true,
            receives_reflections: true,
            casts_reflections: true,
            layers: MAIN_LAYER,
            lightmap: None,
            bake_role: BakeRole::Auto,
            lightmap_resolution: LightmapResolution::Inherit,
            source_row: None,
            lighting_index: 0,
            imported_placement: None,
            instance_order: None,
            instance_shadow_bounds: None,
            pickable: true,
            collidable: true,
            trigger: false,
            transform,
            skin: None,
            morph: None,
            styled_blend_centers: None,
        }
    }
}
