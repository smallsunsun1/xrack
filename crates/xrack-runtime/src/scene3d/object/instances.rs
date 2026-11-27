//! Instance addresses, instancing LOD, and per-instance renderer inputs.

use nalgebra::Vector3;
use std::{num::NonZeroU32, sync::Arc};

use super::SceneObject;
use crate::scene3d::{Instance3D, mesh_node_index};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct InstancingLodRange([f64; 2]);

impl InstancingLodRange {
    pub(super) const ALL: Self = Self([0.0, f64::INFINITY]);

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
    pub fn imported_placement(&self) -> Option<(u32, Option<u32>)> {
        self.imported_placement
    }

    pub(crate) fn scene_row(&self) -> Option<u32> {
        self.source_row.map(|row| row.get() - 1)
    }

    pub(crate) fn with_source_row(mut self, row: u32) -> Self {
        self.source_row = NonZeroU32::new(row + 1);
        self
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
                && self.opacity == other.opacity
                && self.instance_data() == other.instance_data()
                && self.shader_instance_index() == other.shader_instance_index()
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
