//! Render settings resolved for a scene object.

use super::SceneObject;

impl SceneObject {
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

    /// The resolved product of this node's opacity and its ancestors'.
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    #[doc(hidden)]
    pub fn tint_alpha(&self) -> f32 {
        self.opacity * (f32::from(self.color.0 as u8) / 255.0)
    }

    #[doc(hidden)]
    pub fn with_shadows(mut self, casts: bool, receives: bool) -> Self {
        self.casts_shadow = casts;
        self.receives_shadow = receives;
        self
    }

    #[doc(hidden)]
    pub fn lighting_index(&self) -> u16 {
        self.lighting_index
    }
}
