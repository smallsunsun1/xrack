//! Construction and defaults for placed scene objects.

use super::{InstancingLodRange, SceneObject};
use crate::{
    Color,
    scene3d::{BakeRole, LightmapResolution, MAIN_LAYER, Mesh3D, SceneTransform},
};

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
