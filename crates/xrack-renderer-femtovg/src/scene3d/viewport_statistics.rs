use std::time::Instant;

use xrack_runtime::{
    ViewportGpuFrame, ViewportRenderStats, ViewportRenderWork, ViewportStatistics,
};

use super::{SceneDrawStats, SceneRenderer, effect_history::Owner};

const MAX_VIEWPORT_REPORTS: usize = 64;

#[derive(Default)]
pub(crate) struct ViewportCollector {
    enabled: bool,
    statistics: ViewportStatistics,
}

pub(crate) struct RecordedViewport {
    started: Instant,
    counters: SceneDrawStats,
    shadows: u64,
    mesh_bytes: u64,
}

impl ViewportCollector {
    pub fn enable(&mut self, enabled: bool, gpu_timing_supported: bool) {
        self.enabled = enabled;
        self.statistics.gpu_timing_supported = gpu_timing_supported;
        self.clear();
    }

    pub fn set_gpu(&mut self, frame: Option<ViewportGpuFrame>) {
        self.statistics.gpu = frame;
    }

    pub fn begin_frame(&mut self) {
        if self.enabled {
            self.statistics.frame = self.statistics.frame.wrapping_add(1);
            self.clear();
        }
    }

    pub fn clear(&mut self) {
        self.statistics.views.clear();
        self.statistics.truncated = false;
    }

    pub fn statistics(&self) -> Option<&ViewportStatistics> {
        self.enabled.then_some(&self.statistics)
    }
}

impl SceneRenderer {
    pub(crate) fn begin_viewport_recording(&mut self) -> Option<RecordedViewport> {
        if !self.viewport_collector.enabled {
            return None;
        }
        let statistics = &mut self.viewport_collector.statistics;
        if statistics.views.len() == MAX_VIEWPORT_REPORTS {
            statistics.truncated = true;
            return None;
        }
        Some(RecordedViewport {
            started: Instant::now(),
            counters: self.stats,
            shadows: self.shadow_passes(),
            mesh_bytes: self.mesh_bytes_uploaded(),
        })
    }

    pub(crate) fn finish_viewport_recording(
        &mut self,
        recording: Option<RecordedViewport>,
        owner: Owner<'_>,
        work: ViewportRenderWork,
        output_size: [u32; 2],
    ) {
        let Some(before) = recording else { return };
        let after = &self.stats;
        let linked = after
            .linked_transparency_composites
            .saturating_sub(before.counters.linked_transparency_composites);
        let blended = after
            .transparency_composites
            .saturating_sub(before.counters.transparency_composites)
            .saturating_sub(linked);
        let raster_size = (matches!(
            work,
            ViewportRenderWork::Inline | ViewportRenderWork::Offscreen
        ) && after.last_raster_size != [0, 0])
        .then_some(after.last_raster_size);
        let item = ViewportRenderStats {
            path: owner.path.to_vec(),
            id: owner.id.map(str::to_owned),
            work,
            output_size,
            raster_size,
            samples: raster_size.map(|_| after.last_sample_count),
            cpu_record_micros: u64::try_from(before.started.elapsed().as_micros())
                .unwrap_or(u64::MAX),
            mesh_draws: after
                .color_draws
                .saturating_sub(before.counters.color_draws),
            mesh_vertices: after
                .color_vertices
                .saturating_sub(before.counters.color_vertices),
            sprite_draws: after
                .particle_draws
                .saturating_sub(before.counters.particle_draws),
            sprites: after
                .particle_instances
                .saturating_sub(before.counters.particle_instances),
            shadow_passes: self.shadow_passes().saturating_sub(before.shadows),
            reflection_faces: after
                .reflection_capture_faces
                .saturating_sub(before.counters.reflection_capture_faces),
            capture_passes: after
                .scene_capture_passes
                .saturating_sub(before.counters.scene_capture_passes),
            depth_prepass_draws: after
                .material_depth_prepass_draws
                .saturating_sub(before.counters.material_depth_prepass_draws),
            mesh_upload_bytes: self.mesh_bytes_uploaded().saturating_sub(before.mesh_bytes),
            blended_resolves: blended,
            linked_resolves: linked,
            linked_working_bytes: (linked != 0).then_some(after.linked_transparency_bytes),
        };
        self.viewport_collector.statistics.views.push(item);
    }

    fn mesh_bytes_uploaded(&self) -> u64 {
        self.scene.as_ref().map_or(0, |scene| {
            let transfers = &scene.meshes.transfers;
            transfers.vertex_bytes + transfers.index_bytes + transfers.deformation_bytes
        })
    }
}
