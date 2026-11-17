mod table_workload;

use std::{
    env, fs,
    path::PathBuf,
    time::{Duration, Instant},
};
use table_workload::{GRID, ROW_COUNT, VIEWPORT, aot, interpreted, scroll, update_visible};
use xrack_renderer_femtovg::Renderer;
use xrack_runtime::{Component, VecModel, update_profile};
use xrack_testing::UiDriver;

const SCALE: u32 = 2;
const WARMUP: usize = 30;
const FRAMES: usize = 240;

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn percentiles(mut samples: Vec<f64>) -> serde_json::Value {
    samples.sort_by(f64::total_cmp);
    let at = |percent: usize| samples[(samples.len() * percent).div_ceil(100) - 1];
    serde_json::json!({"p50": at(50), "p95": at(95), "p99": at(99)})
}

fn measure<C: Component>(
    renderer: &mut Renderer,
    target: &wgpu::Texture,
    component: C,
    model: VecModel<String>,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut ui = UiDriver::headless(component, VIEWPORT);
    let mut prepare = Vec::with_capacity(FRAMES);
    let mut submit = Vec::with_capacity(FRAMES);
    let mut gpu_wait = Vec::with_capacity(FRAMES);
    let mut total = Vec::with_capacity(FRAMES);
    let mut materialized = Vec::with_capacity(FRAMES);
    let mut evaluated = Vec::with_capacity(FRAMES);
    let mut layout_runs = Vec::with_capacity(FRAMES);

    for step in 0..WARMUP + FRAMES {
        let start = Instant::now();
        let counts = update_profile::measure(|| {
            scroll(&mut ui);
            update_visible(&mut ui, &model, step);
        })
        .ok_or("frame profiling scope is already in use")?;
        let prepared = Instant::now();
        renderer.draw_with_layout(ui.scene(), ui.layout(), target, SCALE as f32, ui.input())?;
        let submitted = Instant::now();
        renderer.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(10)),
        })?;
        let completed = Instant::now();
        if step >= WARMUP {
            prepare.push(milliseconds(prepared - start));
            submit.push(milliseconds(submitted - prepared));
            gpu_wait.push(milliseconds(completed - submitted));
            total.push(milliseconds(completed - start));
            materialized.push(ui.node(GRID).children.len() as f64);
            evaluated.push(counts.property_evaluations as f64);
            layout_runs.push(counts.layout_runs as f64);
        }
    }
    Ok(serde_json::json!({
        "prepare_ms": percentiles(prepare),
        "draw_and_submit_ms": percentiles(submit),
        "gpu_wait_ms": percentiles(gpu_wait),
        "total_ms": percentiles(total),
        "materialized_rows": percentiles(materialized),
        "property_evaluations": percentiles(evaluated),
        "layout_runs": percentiles(layout_runs),
    }))
}

#[test]
fn scrolling_table_with_model_updates() -> Result<(), Box<dyn std::error::Error>> {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let target = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("XRack table workload"),
        size: wgpu::Extent3d {
            width: VIEWPORT.width as u32 * SCALE,
            height: VIEWPORT.height as u32 * SCALE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let info = renderer.adapter.get_info();
    let (compiled, model) = aot();
    let compiled = measure(&mut renderer, &target, compiled, model)?;
    let (instance, model) = interpreted()?;
    let instance = measure(&mut renderer, &target, instance, model)?;
    let report = serde_json::json!({
        "schema_version": 1,
        "scenario": "scroll_and_update_table",
        "mode": "offscreen_serialized_gpu",
        "adapter": info.name,
        "backend": format!("{:?}", info.backend),
        "device_type": format!("{:?}", info.device_type),
        "os": env::consts::OS,
        "arch": env::consts::ARCH,
        "viewport": [VIEWPORT.width, VIEWPORT.height],
        "scale": SCALE,
        "rows": ROW_COUNT,
        "warmup_frames": WARMUP,
        "frames_per_backend": FRAMES,
        "aot": compiled,
        "interpreter": instance,
    });
    if let Some(directory) = env::var_os("TEST_UNDECLARED_OUTPUTS_DIR") {
        fs::write(
            PathBuf::from(directory).join("ui-frame-benchmark.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
    }
    println!("{report}");
    Ok(())
}
