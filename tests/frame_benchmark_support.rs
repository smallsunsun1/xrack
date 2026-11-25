use std::time::Duration;
use xrack_layout::Size;
use xrack_renderer_femtovg::Renderer;

pub(super) const SCALE: u32 = 2;

pub(super) fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

pub(super) fn percentiles(mut samples: Vec<f64>) -> serde_json::Value {
    samples.sort_by(f64::total_cmp);
    let at = |percent: usize| samples[(samples.len() * percent).div_ceil(100) - 1];
    serde_json::json!({"p50": at(50), "p95": at(95), "p99": at(99)})
}

pub(super) fn offscreen_target(
    renderer: &Renderer,
    viewport: Size,
    label: &'static str,
) -> wgpu::Texture {
    renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: viewport.width as u32 * SCALE,
            height: viewport.height as u32 * SCALE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}
