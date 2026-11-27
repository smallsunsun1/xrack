//! A mesh placed in a scene with its transform, visibility, and instance state.

mod bake;
mod collision;
mod picking;
mod pose;

use crate::Color;
use nalgebra::Vector3;
use std::{num::NonZeroU32, sync::Arc};

use super::{
    BakeRole, BakedLightmap, Instance3D, LightmapResolution, MAIN_LAYER, Mesh3D, SceneTransform,
    bounds::Bounds3D,
    mesh::{data::Primitive, morph::MorphPose, skin::SkinPose},
    mesh_node_index,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct InstancingLodRange([f64; 2]);

impl InstancingLodRange {
    const ALL: Self = Self([0.0, f64::INFINITY]);

    pub(crate) fn from_limits([near, far]: [f64; 2]) -> Self {
        let near = if near.is_finite() { near.max(0.0) } else { 0.0 };
        let far = if far.is_finite() && far >= 0.0 {
            far
        } else {
            f64::INFINITY
        };
        Self([near * near, far * far])
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneObject {
    pub id: String,
    pub mesh: Mesh3D,
    pub color: Color,
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

/// Transparent ordering information for a row of an instancing table.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InstanceOrder {
    pub group: [u32; 2],
    pub ordinal: u32,
    pub group_origin: [f32; 3],
    pub origin: [f32; 3],
    pub depth_sorting: bool,
}

impl SceneObject {
    #[doc(hidden)]
    pub fn instance_order(&self) -> Option<InstanceOrder> {
        self.instance_order
    }

    /// The shared world-space envelope for fitting a table's shadow projections.
    #[doc(hidden)]
    pub fn instance_shadow_bounds(&self) -> Option<&Arc<[[f32; 3]; 2]>> {
        self.instance_shadow_bounds.as_ref()
    }
    pub(crate) fn instance_address(&self) -> Option<[i64; 3]> {
        let row = i64::from(self.scene_row()?);
        let Some((node, instance)) = self.imported_placement() else {
            return Some([row, -1, -1]);
        };
        Some([row, mesh_node_index(node), instance.map_or(-1, i64::from)])
    }

    #[doc(hidden)]
    pub fn lighting_index(&self) -> u16 {
        self.lighting_index
    }

    #[doc(hidden)]
    pub fn imported_placement(&self) -> Option<(u32, Option<u32>)> {
        self.imported_placement
    }

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

    /// Set the depth-sort offset. Positive values move the sort key farther away.
    pub fn with_depth_bias(mut self, bias: f64) -> Self {
        self.depth_bias = if bias.is_finite() { bias } else { 0.0 };
        self
    }

    pub fn depth_bias(&self) -> f64 {
        self.depth_bias
    }

    #[doc(hidden)]
    pub fn lod_coverage(&self) -> u8 {
        self.lod_coverage
    }

    /// Configure this surface's participation and displacement in temporal reprojection.
    pub fn with_motion_vectors(mut self, enabled: bool, scale: f64) -> Self {
        self.motion_vector_enabled = enabled;
        let scale = scale as f32;
        self.motion_vector_scale = if scale.is_finite() { scale } else { 1.0 };
        self
    }

    pub fn motion_vector_enabled(&self) -> bool {
        self.motion_vector_enabled
    }

    pub fn motion_vector_scale(&self) -> f32 {
        self.motion_vector_scale
    }

    /// Four per-instance channels shared by programmable vertex and fragment stages.
    pub fn instance_data(&self) -> [f32; 4] {
        self.instance_data.unwrap_or([0.0; 4])
    }

    /// Choose the camera distances, in world units, at which this instance is visible.
    pub fn with_instancing_lod_range(mut self, range: [f64; 2]) -> Self {
        self.instancing_lod = InstancingLodRange::from_limits(range);
        self
    }

    pub(crate) fn has_instancing_lod_range(&self) -> bool {
        self.instancing_lod != InstancingLodRange::ALL
    }

    #[doc(hidden)]
    pub fn visible_at_distance(&self, eye: [f32; 3]) -> bool {
        if !self.has_instancing_lod_range() {
            return true;
        }
        let distance =
            (self.transform.origin.cast::<f64>() - Vector3::from(eye).cast::<f64>()).norm_squared();
        let InstancingLodRange([near, far]) = self.instancing_lod;
        near <= distance && distance < far
    }

    #[doc(hidden)]
    pub fn with_shadows(mut self, casts: bool, receives: bool) -> Self {
        self.casts_shadow = casts;
        self.receives_shadow = receives;
        self
    }

    #[doc(hidden)]
    pub fn is_pickable(&self) -> bool {
        self.pickable
    }

    pub fn with_pickable(mut self, enabled: bool) -> Self {
        self.pickable = enabled;
        self
    }

    pub fn with_collidable(mut self, enabled: bool) -> Self {
        self.collidable = enabled;
        self
    }

    /// Source index for a material program, independent of GPU culling and draw order.
    pub fn shader_instance_index(&self) -> u32 {
        self.imported_placement
            .and_then(|(_, instance)| instance)
            .or_else(|| self.scene_row())
            .unwrap_or(0)
    }

    #[doc(hidden)]
    pub fn same_vertex_program_instance(&self, other: &Self) -> bool {
        self.mesh.shader_displacement() == 0.0
            || self.color == other.color
                && self.instance_data() == other.instance_data()
                && self.shader_instance_index() == other.shader_instance_index()
    }

    pub(crate) fn scene_row(&self) -> Option<u32> {
        self.source_row.map(|row| row.get() - 1)
    }

    #[doc(hidden)]
    pub fn with_trigger(mut self, enabled: bool) -> Self {
        self.trigger = enabled
            && matches!(
                self.mesh.primitive(),
                Some(
                    Primitive::Box
                        | Primitive::Sphere
                        | Primitive::Cylinder
                        | Primitive::Cone
                        | Primitive::Capsule
                )
            );
        self
    }

    pub(crate) fn is_trigger(&self) -> bool {
        self.trigger
    }

    pub(crate) fn relative_to(mut self, root: SceneTransform) -> Option<Self> {
        self.transform = self.transform.relative_to(root)?;
        Some(self)
    }

    pub(crate) fn with_parent_transform(mut self, parent: SceneTransform) -> Option<Self> {
        self.transform = parent.child(self.transform)?;
        Some(self)
    }

    pub(crate) fn with_source_row(mut self, row: u32) -> Self {
        self.source_row = NonZeroU32::new(row + 1);
        self
    }

    fn render_bounds(&self) -> ([f32; 3], [f32; 3]) {
        let (min, max) = self.bounds();
        let expansion = self.mesh.shader_displacement();
        (
            min.map(|point| point - expansion),
            max.map(|point| point + expansion),
        )
    }

    pub(crate) fn world_bounds(&self) -> Bounds3D {
        let (min, max) = self.render_bounds();
        Bounds3D { min, max }.transform(self.transform_rows())
    }

    /// Corners of the transformed mesh bounds.
    pub fn bounds_corners(&self) -> [[f32; 3]; 8] {
        let (min, max) = self.render_bounds();
        let transform = self.transform_rows();
        std::array::from_fn(|corner| {
            let [x, y, z] = std::array::from_fn(|axis| {
                if corner & (1 << axis) == 0 {
                    min[axis]
                } else {
                    max[axis]
                }
            });
            transform.map(|[a, b, c, d]| a * x + b * y + c * z + d)
        })
    }

    /// Rows of the model-to-world affine transform, including translation.
    pub fn transform_rows(&self) -> [[f32; 4]; 3] {
        self.transform.rows()
    }

    /// Whether this object's model-to-world transform reverses orientation.
    pub fn is_reflected(&self) -> bool {
        self.transform.reflected
    }
}

impl Instance3D {
    pub(crate) fn from_object(object: &SceneObject, address: [i64; 3]) -> Self {
        Self::from_placement(
            object.id.clone(),
            address,
            object.color,
            object.transform,
            object.instance_data(),
        )
    }
}
