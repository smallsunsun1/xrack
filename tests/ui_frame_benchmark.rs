mod frame_benchmark_support;
mod table_workload;

use frame_benchmark_support::{SCALE, milliseconds, offscreen_target, percentiles};
use std::{
    env, fs,
    path::PathBuf,
    time::{Duration, Instant},
};
use table_workload::{GRID, ROW_COUNT, VIEWPORT, aot, interpreted, scroll, update_visible};
use xrack_renderer_femtovg::{RenderDevice, Renderer};
use xrack_runtime::{Component, VecModel, update_profile};
use xrack_testing::UiDriver;

const WARMUP: usize = 30;
const FRAMES: usize = 240;

fn measure<C: Component>(
    renderer: &mut Renderer,
    component: C,
    model: VecModel<String>,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let target = offscreen_target(renderer, VIEWPORT, "XRack table workload");
    let mut ui = UiDriver::headless(component, VIEWPORT);
    let mut prepare = Vec::with_capacity(FRAMES);
    let mut submit = Vec::with_capacity(FRAMES);
    let mut gpu_wait = Vec::with_capacity(FRAMES);
    let mut total = Vec::with_capacity(FRAMES);
    let mut materialized = Vec::with_capacity(FRAMES);
    let mut evaluated = Vec::with_capacity(FRAMES);
    let mut projection_scans = Vec::with_capacity(FRAMES);
    let mut row_clones = Vec::with_capacity(FRAMES);
    let mut layout_runs = Vec::with_capacity(FRAMES);
    let mut rendering = Vec::with_capacity(FRAMES);

    for step in 0..WARMUP + FRAMES {
        let start = Instant::now();
        let counts = update_profile::measure(|| {
            scroll(&mut ui);
            update_visible(&mut ui, &model, step);
        })
        .ok_or("frame profiling scope is already in use")?;
        let prepared = Instant::now();
        let mut draw_result = Ok(());
        let frame = update_profile::measure(|| {
            draw_result = renderer.draw_with_layout(
                ui.scene(),
                ui.layout(),
                &target,
                SCALE as f32,
                ui.input(),
            );
        })
        .ok_or("render profiling scope is already in use")?;
        draw_result?;
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
            projection_scans.push(counts.model_rows_scanned as f64);
            row_clones.push(counts.model_rows_cloned as f64);
            layout_runs.push(counts.layout_runs as f64);
            rendering.push(frame);
        }
    }
    let metric = |read: fn(&update_profile::UpdateSample) -> usize| {
        percentiles(rendering.iter().map(|sample| read(sample) as f64).collect())
    };
    Ok(serde_json::json!({
        "prepare_ms": percentiles(prepare),
        "draw_and_submit_ms": percentiles(submit),
        "gpu_wait_ms": percentiles(gpu_wait),
        "total_ms": percentiles(total),
        "materialized_rows": percentiles(materialized),
        "property_evaluations": percentiles(evaluated),
        "model_rows_scanned": percentiles(projection_scans),
        "model_rows_cloned": percentiles(row_clones),
        "layout_runs": percentiles(layout_runs),
        "draw_primitive_dispatches": metric(|frame| frame.draw_primitives),
        "quad_batches": metric(|frame| frame.quad_batches),
        "glyph_batches": metric(|frame| frame.glyph_batches),
        "batched_vertex_payload_bytes": metric(|frame| frame.vertex_bytes),
        "renderer_queue_submissions": metric(|frame| frame.gpu_submissions),
        "image_upload_payload_bytes": metric(|frame| frame.texture_upload_bytes),
        "retained_geometry_upload_bytes": metric(|frame| frame.geometry_upload_bytes),
        "retained_geometry_reuses": metric(|frame| frame.geometry_reuses),
    }))
}

#[test]
fn scrolling_table_with_model_updates() -> Result<(), Box<dyn std::error::Error>> {
    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    // Use the same GPU while giving each backend its own canvas and text caches.
    let mut compiled_renderer = Renderer::with_device(gpu.clone())?;
    let mut interpreter_renderer = Renderer::with_device(gpu)?;
    let info = compiled_renderer.adapter.get_info();
    let (compiled, compiled_model) = aot();
    let (instance, interpreter_model) = interpreted()?;
    let run_aot = || measure(&mut compiled_renderer, compiled, compiled_model);
    let run_interpreter = || measure(&mut interpreter_renderer, instance, interpreter_model);
    let interpreter_first =
        env::var_os("XRACK_BENCH_INTERPRETER_FIRST").is_some_and(|value| value == "1");
    let (compiled, instance, order) = if interpreter_first {
        let instance = run_interpreter()?;
        (run_aot()?, instance, ["interpreter", "aot"])
    } else {
        (run_aot()?, run_interpreter()?, ["aot", "interpreter"])
    };
    let report = serde_json::json!({
        "schema_version": 2,
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
        "backend_order": order,
        "renderer_per_backend": true,
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

#[test]
fn searching_text_keys_while_updating_one_row() -> Result<(), Box<dyn std::error::Error>> {
    let source = VecModel::from(
        (0..10_000)
            .map(|index| format!("owner-{:04}.", index % 500))
            .collect::<Vec<_>>(),
    );
    let _ = source.lookup_text("owner", "owner-0000.", |row| Some(row.as_str()));
    let mut scan_ms = Vec::with_capacity(FRAMES);
    let mut lookup_ms = Vec::with_capacity(FRAMES);
    for index in 0..FRAMES {
        assert!(source.set_row(index, format!("owner-new-{index}.")));
        let key = format!("owner-{index:04}.");
        let start = Instant::now();
        let scan = source.filter_text(&key, |row| Some(row.as_str()));
        scan_ms.push(milliseconds(start.elapsed()));
        let start = Instant::now();
        let lookup = source.lookup_text("owner", &key, |row| Some(row.as_str()));
        lookup_ms.push(milliseconds(start.elapsed()));
        assert_eq!(lookup.snapshot(), scan.snapshot());
    }
    let report = serde_json::json!({
        "scenario": "indexed_model_lookup",
        "rows": source.len(),
        "scan_ms": percentiles(scan_ms),
        "indexed_lookup_ms": percentiles(lookup_ms),
    });
    println!("{report}");
    if let Some(directory) = env::var_os("TEST_UNDECLARED_OUTPUTS_DIR") {
        fs::write(
            PathBuf::from(directory).join("model-lookup-benchmark.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
    }
    Ok(())
}
