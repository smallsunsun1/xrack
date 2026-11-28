//! A mesh placed in a scene with its transform, visibility, and instance state.

mod appearance;
mod bake;
mod collision;
mod construction;
mod instances;
mod picking;
mod pose;
mod raster;

use crate::Color;
use std::{num::NonZeroU32, sync::Arc};

pub use instances::InstanceOrder;
pub(crate) use instances::InstancingLodRange;

use super::{
    BakeRole, BakedLightmap, LightmapResolution, Mesh3D, SceneTransform,
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
