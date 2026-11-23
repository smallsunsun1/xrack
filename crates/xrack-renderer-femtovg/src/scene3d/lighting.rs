use std::{collections::VecDeque, sync::Arc};
use xrack_runtime::{
    Node,
    scene3d::{MAX_SCENE_LIGHTS, SceneLight, SceneLightSource, SceneModel, viewport_lighting},
};

use super::{append_floats, buffer};

const MAX_CACHED_LIGHT_SETS: usize = 8;

#[derive(PartialEq)]
pub(super) struct LightSet {
    lights: Option<Arc<[SceneLight]>>,
    fallback: [SceneLight; 1],
    ambient: [f32; 3],
    shadow_bias: f32,
    shadow_slots: [u8; MAX_SCENE_LIGHTS],
}

impl LightSet {
    pub(super) fn has_ambient(&self) -> bool {
        self.ambient.iter().any(|&channel| channel > 0.0)
    }

    pub(super) fn from_node(node: &Node, model: Option<&SceneModel>, layers: u64) -> Self {
        let (lights, fallback, ambient) = viewport_lighting(node, model, layers);
        Self {
            lights,
            shadow_slots: [0; MAX_SCENE_LIGHTS],
            fallback: [fallback],
            shadow_bias: node.float("shadow-bias").unwrap_or(0.001).clamp(0.0, 0.05),
            ambient,
        }
    }

    pub(super) fn lights(&self) -> &[SceneLight] {
        let lights = self.lights.as_deref().unwrap_or(&self.fallback);
        &lights[..lights.len().min(MAX_SCENE_LIGHTS)]
    }

    fn encode(&self) -> Vec<u8> {
        let lights = self.lights();
        // Two vec4 header rows, then four vec4 rows per light.
        let mut bytes = Vec::with_capacity(32 + MAX_SCENE_LIGHTS * 64);
        let [r, g, b] = self.ambient;
        append_floats(&mut bytes, [r, g, b, 0.0]);
        append_floats(
            &mut bytes,
            [
                lights.len() as f32,
                if self.lights.is_some() { 1.0 } else { 0.0 },
                self.shadow_bias,
                0.0,
            ],
        );
        for (light, slot) in lights.iter().zip(self.shadow_slots) {
            let slot = f32::from(slot);
            let [r, g, b] = light.radiance;
            let rows = match light.source {
                SceneLightSource::Directional {
                    toward_light: [x, y, z],
                } => [[0.0; 4], [x, y, z, 0.0], [r, g, b, slot], [0.0; 4]],
                SceneLightSource::Point {
                    position: [x, y, z],
                    inverse_range_squared,
                } => [
                    [x, y, z, 1.0],
                    [0.0, 0.0, 0.0, inverse_range_squared],
                    [r, g, b, slot],
                    [0.0; 4],
                ],
                SceneLightSource::Spot {
                    position: [x, y, z],
                    inverse_range_squared,
                    toward_light: [dx, dy, dz],
                    cone_scale,
                    cone_offset,
                    ..
                } => [
                    [x, y, z, 2.0],
                    [dx, dy, dz, inverse_range_squared],
                    [r, g, b, slot],
                    [cone_scale, cone_offset, 0.0, 0.0],
                ],
            };
            append_floats(&mut bytes, rows.into_iter().flatten());
        }
        bytes.resize(32 + MAX_SCENE_LIGHTS * 64, 0);
        bytes
    }
}

/// Keep light uniforms resident while cameras, objects or different viewports move.
#[derive(Default)]
pub(super) struct LightBuffers {
    entries: VecDeque<(LightSet, wgpu::Buffer)>,
    pub uploads: u64,
}

impl LightBuffers {
    pub(super) fn get(
        &mut self,
        device: &wgpu::Device,
        mut state: LightSet,
        slots: [u8; MAX_SCENE_LIGHTS],
    ) -> wgpu::Buffer {
        state.shadow_slots = slots;
        let existing = self.entries.iter().position(|(cached, _)| *cached == state);
        if let Some(entry) = existing.and_then(|index| self.entries.remove(index)) {
            let buffer = entry.1.clone();
            self.entries.push_back(entry);
            return buffer;
        }
        self.uploads = self.uploads.saturating_add(1);
        let buffer = buffer(
            device,
            "XRack 3D lights",
            &state.encode(),
            wgpu::BufferUsages::UNIFORM,
        );
        if self.entries.len() == MAX_CACHED_LIGHT_SETS {
            self.entries.pop_front();
        }
        self.entries.push_back((state, buffer.clone()));
        buffer
    }
}
