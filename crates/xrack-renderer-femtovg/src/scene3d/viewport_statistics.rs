use std::time::Instant;

use xrack_runtime::{
    GeometryHandle, Node, RenderStats3D, ViewportGpuFrame, ViewportRenderStats, ViewportRenderWork,
    ViewportStatistics,
};

use super::{SceneDrawStats, SceneRenderer, effect_history::Owner};

const MAX_VIEWPORT_REPORTS: usize = 64;

#[derive(Default)]
pub(crate) struct ViewportCollector {
    enabled: bool,
    inspector_first_frame: u64,
    statistics: ViewportStatistics,
    pending: Vec<(GeometryHandle, RenderStats3D)>,
}

#[derive(Clone)]
pub(crate) struct RenderSampleTarget {
    pub handle: GeometryHandle,
    pub revision: i64,
}

pub(crate) struct RecordedViewport {
    started: Instant,
    counters: SceneDrawStats,
    shadows: u64,
    mesh_bytes: u64,
    inspector: bool,
    pub samples: Vec<RenderSampleTarget>,
}

impl ViewportCollector {
    pub fn enable(&mut self, enabled: bool, gpu_supported: bool) {
        self.statistics.gpu_timing_supported = gpu_supported;
        if self.enabled != enabled {
            self.enabled = enabled;
            self.inspector_first_frame = self.statistics.frame.saturating_add(1);
            self.statistics.gpu = None;
            self.clear();
        }
    }

    pub fn set_gpu(&mut self, frame: Option<ViewportGpuFrame>) {
        self.statistics.gpu = frame;
    }

    pub fn report_gpu(&mut self, frame: ViewportGpuFrame) -> bool {
        if self.enabled
            && frame.frame >= self.inspector_first_frame
            && self
                .statistics
                .gpu
                .as_ref()
                .is_none_or(|previous| previous.frame < frame.frame)
        {
            self.set_gpu(Some(frame));
            true
        } else {
            false
        }
    }

    pub fn begin_frame(&mut self, gpu_supported: bool) {
        self.statistics.frame = self.statistics.frame.wrapping_add(1);
        self.statistics.gpu_timing_supported = gpu_supported;
        if self.enabled {
            self.clear();
        }
    }

    pub fn gpu_frame(&self) -> Option<u64> {
        self.statistics
            .gpu_timing_supported
            .then_some(self.statistics.frame)
    }

    pub fn finish_samples(&mut self, submitted: bool) -> bool {
        let mut changed = false;
        for (handle, sample) in self.pending.drain(..) {
            if submitted {
                changed |= handle.publish_render_stats(sample);
            }
        }
        changed
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
    pub(crate) fn begin_viewport_recording(&mut self, node: &Node) -> Option<RecordedViewport> {
        let collector = &mut self.viewport_collector;
        let revision = node.int("render-stats-revision").unwrap_or(0);
        let mut samples = Vec::new();
        for handle in node
            .geometry_handle
            .iter()
            .chain(node.instance_geometry_handle.iter())
        {
            if handle.needs_render_stats(revision)
                && !collector.pending.iter().any(|(old, _)| old == handle)
                && !samples
                    .iter()
                    .any(|sample: &RenderSampleTarget| &sample.handle == handle)
            {
                if revision > 0 {
                    samples.push(RenderSampleTarget {
                        handle: handle.clone(),
                        revision,
                    });
                } else {
                    collector
                        .pending
                        .push((handle.clone(), RenderStats3D::default()));
                }
            }
        }
        let room = collector.statistics.views.len() < MAX_VIEWPORT_REPORTS;
        let inspector = collector.enabled && room;
        collector.statistics.truncated |= collector.enabled && !room;
        if !inspector && samples.is_empty() {
            return None;
        }
        Some(RecordedViewport {
            started: Instant::now(),
            counters: self.stats,
            shadows: self.shadow_passes(),
            mesh_bytes: self.mesh_bytes_uploaded(),
            inspector,
            samples,
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
            path: if before.inspector {
                owner.path.to_vec()
            } else {
                Vec::new()
            },
            id: owner.id.filter(|_| before.inspector).map(str::to_owned),
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
        let collector = &mut self.viewport_collector;
        for sample in before.samples {
            let stats = RenderStats3D::recorded(
                sample.revision,
                collector.statistics.frame,
                &item,
                collector.statistics.gpu_timing_supported,
            );
            collector.pending.push((sample.handle, stats));
        }
        if before.inspector {
            collector.statistics.views.push(item);
        }
    }

    fn mesh_bytes_uploaded(&self) -> u64 {
        self.scene.as_ref().map_or(0, |scene| {
            let transfers = &scene.meshes.transfers;
            transfers.vertex_bytes + transfers.index_bytes + transfers.deformation_bytes
        })
    }
}
