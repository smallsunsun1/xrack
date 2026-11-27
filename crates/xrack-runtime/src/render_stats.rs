use std::{
    ops::Deref,
    sync::{Arc, LazyLock},
};

use crate::{ViewportGpuStats, ViewportRenderStats};

/// A snapshot of the work recorded for one viewport.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderStats3D(Option<Arc<RenderStats3DData>>);

#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderStats3DData {
    pub valid: bool,
    pub revision: i64,
    pub frame: i64,
    pub work: String,
    pub output_width: i64,
    pub output_height: i64,
    pub raster_width: i64,
    pub raster_height: i64,
    pub samples: i64,
    pub cpu_micros: i64,
    pub mesh_draws: i64,
    pub mesh_vertices: i64,
    pub sprite_draws: i64,
    pub sprites: i64,
    pub shadow_passes: i64,
    pub reflection_faces: i64,
    pub capture_passes: i64,
    pub depth_prepass_draws: i64,
    pub mesh_upload_bytes: i64,
    pub gpu_supported: bool,
    pub gpu_valid: bool,
    pub gpu_nanos: i64,
    pub gpu_passes: i64,
    pub gpu_truncated: bool,
}

impl Deref for RenderStats3D {
    type Target = RenderStats3DData;

    fn deref(&self) -> &Self::Target {
        static EMPTY: LazyLock<RenderStats3DData> = LazyLock::new(RenderStats3DData::default);
        self.0.as_deref().unwrap_or(&EMPTY)
    }
}

fn count(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

impl RenderStats3D {
    #[doc(hidden)]
    pub fn from_data(data: RenderStats3DData) -> Self {
        if data.valid {
            Self(Some(Arc::new(data)))
        } else {
            Self::default()
        }
    }

    #[doc(hidden)]
    pub fn recorded(revision: i64, frame: u64, stats: &ViewportRenderStats, gpu: bool) -> Self {
        let [output_width, output_height] = stats.output_size.map(i64::from);
        let [raster_width, raster_height] = stats.raster_size.unwrap_or_default().map(i64::from);
        Self::from_data(RenderStats3DData {
            valid: true,
            revision,
            frame: count(frame),
            work: stats.work.name().into(),
            output_width,
            output_height,
            raster_width,
            raster_height,
            samples: stats.samples.map_or(0, i64::from),
            cpu_micros: count(stats.cpu_record_micros),
            mesh_draws: count(stats.mesh_draws),
            mesh_vertices: count(stats.mesh_vertices),
            sprite_draws: count(stats.sprite_draws),
            sprites: count(stats.sprites),
            shadow_passes: count(stats.shadow_passes),
            reflection_faces: count(stats.reflection_faces),
            capture_passes: count(stats.capture_passes),
            depth_prepass_draws: count(stats.depth_prepass_draws),
            mesh_upload_bytes: count(stats.mesh_upload_bytes),
            gpu_supported: gpu,
            ..Default::default()
        })
    }

    pub(crate) fn completed(mut self, gpu: &ViewportGpuStats, truncated: bool) -> Self {
        if let Some(data) = &mut self.0 {
            let data = Arc::make_mut(data);
            data.gpu_valid = true;
            data.gpu_truncated = truncated;
            data.gpu_nanos = count(gpu.gpu_nanos);
            data.gpu_passes = gpu.passes.iter().map(|pass| i64::from(pass.count)).sum();
        }
        self
    }

    pub(crate) fn matches(&self, revision: i64, frame: u64) -> bool {
        self.valid && self.revision == revision && self.frame == count(frame)
    }
}
