mod frame_benchmark_support;

use frame_benchmark_support::{SCALE, milliseconds, offscreen_target, percentiles};
use std::{
    env,
    fmt::Write as _,
    fs,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};
use xrack_layout::{FrameCache, Interaction, Size};
use xrack_renderer_femtovg::Renderer;
use xrack_runtime::{Color, Component, Length, Node, TextVerticalAlignment, TextWrap};

struct Document;

impl Component for Document {
    fn render(&self) -> Node {
        Node::hidden()
    }

    fn dispatch(&mut self, _: &str, _: &str) -> bool {
        false
    }
}

#[test]
fn wrapped_document_reflows_through_distinct_widths() -> Result<(), Box<dyn std::error::Error>> {
    const PARAGRAPHS: u32 = 289;

    let mut text = String::new();
    for paragraph in 0..PARAGRAPHS {
        text.push_str("混排文档 CJK 研发说明：");
        for word in 0..11 {
            let identifier = 17_693 * paragraph + 19_991 * word;
            write!(text, "release-{identifier:08x}-identifier ")?;
        }
        text.push_str("。\n");
    }
    benchmark_document(
        text,
        (0..73).map(|phase| (328 + 4 * phase, None)),
        "wrapped_document_distinct_widths",
        "wrapped-document-benchmark.json",
    )
}

#[test]
fn wrapped_document_scrolls_at_fixed_width() -> Result<(), Box<dyn std::error::Error>> {
    let mut text = String::new();
    for section in 1..=120 {
        writeln!(
            text,
            "第{section}节 这个示例持续更新文本颜色，验证长文档中的字形批次、排版边界和稳定的绘制顺序。"
        )?;
    }
    benchmark_document(
        text,
        (0..112).map(|phase| (476, Some(-8 * (phase % 73)))),
        "wrapped_document_fixed_width_scroll",
        "wrapped-document-scroll-benchmark.json",
    )
}

fn benchmark_document(
    text: String,
    placements: impl ExactSizeIterator<Item = (u32, Option<i32>)>,
    scenario: &str,
    filename: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    const EARLY_FRAMES: usize = 16;

    let text_bytes = text.len();
    let mut label = Node::new("Label", None);
    label.properties.insert("height", Length(20_000.0).into());
    label.properties.insert("font-size", Length(12.0).into());
    label
        .properties
        .insert("font-family", "Menlo".to_owned().into());
    label.properties.insert("wrap", TextWrap::WordWrap.into());
    label
        .properties
        .insert("vertical-alignment", TextVerticalAlignment::Top.into());
    label.properties.insert("color", Color(0x101010ff).into());
    label.properties.insert("text", text.into());
    let mut root = Node::new("Window", None);
    root.properties
        .insert("background", Color(0xffffffff).into());
    root.children.push(label);

    let viewport = Size::new(640.0, 480.0);
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let target = offscreen_target(&renderer, viewport, "XRack wrapped document workload");
    let input = Interaction::default();
    let mut scene = Rc::new(root.into_shared());
    let mut cache = FrameCache::default();
    let mut frames = Vec::with_capacity(placements.len());
    let measured = placements.len() - EARLY_FRAMES;
    let mut prepare = Vec::with_capacity(measured);
    let mut submit = Vec::with_capacity(measured);
    let mut wait = Vec::with_capacity(measured);
    let mut total = Vec::with_capacity(measured);
    let mut previous_width = None;

    for (phase, (width, y)) in placements.enumerate() {
        let started = Instant::now();
        let properties = &mut Rc::make_mut(&mut scene).children[0].properties;
        if previous_width != Some(width) {
            properties.insert("width", Length(width.into()).into());
            previous_width = Some(width);
        }
        if let Some(y) = y {
            properties.insert("y", Length(y.into()).into());
        }
        let (render_scene, layout) =
            cache.prepare_reusing(scene.clone(), viewport, &input, |source, previous| {
                renderer.prepare_scene_recycling(&Document, source, viewport, &input, previous)
            });
        let prepared = Instant::now();
        renderer.draw_with_layout(&render_scene, &layout, &target, SCALE as f32, &input)?;
        let submitted = Instant::now();
        renderer.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(10)),
        })?;
        let completed = Instant::now();
        let prepare_ms = milliseconds(prepared - started);
        let submit_ms = milliseconds(submitted - prepared);
        let wait_ms = milliseconds(completed - submitted);
        let total_ms = milliseconds(completed - started);
        frames.push(serde_json::json!({
            "width": width,
            "y": y,
            "prepare_ms": prepare_ms,
            "draw_and_submit_ms": submit_ms,
            "gpu_wait_ms": wait_ms,
            "total_ms": total_ms,
        }));
        if phase >= EARLY_FRAMES {
            prepare.push(prepare_ms);
            submit.push(submit_ms);
            wait.push(wait_ms);
            total.push(total_ms);
        }
    }

    let info = renderer.adapter.get_info();
    let report = serde_json::json!({
        "scenario": scenario,
        "mode": "offscreen_serialized_gpu",
        "adapter": info.name,
        "backend": format!("{:?}", info.backend),
        "viewport": [viewport.width, viewport.height],
        "scale": SCALE,
        "text_bytes": text_bytes,
        "early_frames": EARLY_FRAMES,
        "prepare_ms": percentiles(prepare),
        "draw_and_submit_ms": percentiles(submit),
        "gpu_wait_ms": percentiles(wait),
        "total_ms": percentiles(total),
        "frames": frames,
    });
    if let Some(directory) = env::var_os("TEST_UNDECLARED_OUTPUTS_DIR") {
        fs::write(
            PathBuf::from(directory).join(filename),
            serde_json::to_vec_pretty(&report)?,
        )?;
    }
    println!("{report}");
    Ok(())
}
