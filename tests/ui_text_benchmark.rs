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
    .map(|_| ())
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
    .map(|_| ())
}

#[cfg(target_os = "macos")]
#[test]
fn missing_glyph_uses_the_first_registered_font() -> Result<(), Box<dyn std::error::Error>> {
    use xrack_renderer_femtovg::femtovg::{Color, Paint, TextContext};

    let context = TextContext::default();
    let font = fs::read("/System/Library/Fonts/Menlo.ttc")?;
    let first = context.add_font_mem(&font)?;
    let preferred = context.add_font_mem(&font)?;
    assert_ne!(first, preferred);
    let paint = Paint::color(Color::white()).with_font(&[preferred]);
    let missing = context.measure_text(0.0, 0.0, "\u{10ffff}", &paint)?;
    assert_eq!(missing.glyphs.len(), 1);
    assert_eq!(missing.glyphs[0].font_id, first);
    assert_eq!(missing.glyphs[0].glyph_id, 0);
    let present = context.measure_text(0.0, 0.0, "A", &paint)?;
    assert_eq!(present.glyphs[0].font_id, preferred);
    assert_ne!(present.glyphs[0].glyph_id, 0);
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn prepared_ideographs_preserve_missing_glyph_fallback() -> Result<(), Box<dyn std::error::Error>> {
    use std::num::NonZeroUsize;
    use xrack_renderer_femtovg::femtovg::{Color, Paint, TextContext};

    let data = fs::read("/System/Library/Fonts/Menlo.ttc")?;
    let context = TextContext::default();
    let first = context.add_font_mem(&data)?;
    let preferred = context.add_font_mem(&data)?;
    let paint = Paint::color(Color::white())
        .with_font(&[preferred])
        .with_font_size(17.25)
        .with_letter_spacing(0.125);
    let text: String = (0x4e00..0x5e00).filter_map(char::from_u32).collect();
    assert!(context.cached_ideograph_placement('一', &paint).is_none());
    let mut advances = vec![f32::NAN; 0x1000];
    let capacity = NonZeroUsize::new(8192).unwrap();
    context.resize_shaped_words_cache(capacity);
    context.prepare_ideographs(&text, &paint, &mut advances);
    let parallel = std::thread::available_parallelism().is_ok_and(|workers| workers.get() > 1);
    for word in ["一", "丁", "以"] {
        if parallel {
            let character = word.chars().next().unwrap();
            assert!(
                context
                    .cached_ideograph_placement(character, &paint)
                    .is_some()
            );
        }
        let measured = context.measure_text(0.0, 0.0, word, &paint)?;
        assert_eq!(measured.glyphs.len(), 1);
        let glyph = &measured.glyphs[0];
        assert_eq!(glyph.font_id, first);
        assert_eq!(glyph.glyph_id, 0);
        if parallel {
            assert_eq!(
                advances[(u32::from(glyph.c) - 0x4e00) as usize],
                measured.width()
            );
        }
        assert_eq!(
            context.cached_ideograph_placement(glyph.c, &paint),
            Some((first, 0, [glyph.advance_y, glyph.offset_y]))
        );
    }
    if parallel {
        context.add_font_mem(&data)?;
        assert!(context.cached_ideograph_placement('一', &paint).is_none());
        context.prepare_ideographs(&text, &paint, &mut advances);
        assert!(context.cached_ideograph_placement('一', &paint).is_some());
        context.resize_shaped_words_cache(NonZeroUsize::MIN);
        assert!(context.cached_ideograph_placement('一', &paint).is_none());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn width_probe_keeps_full_ink_metrics_available() -> Result<(), Box<dyn std::error::Error>> {
    use xrack_renderer_femtovg::femtovg::{Color, Paint, TextContext};

    let font = fs::read("/System/Library/Fonts/Menlo.ttc")?;
    let make_context = || -> Result<_, Box<dyn std::error::Error>> {
        let context = TextContext::default();
        let id = context.add_font_mem(&font)?;
        Ok((context, Paint::color(Color::white()).with_font(&[id])))
    };
    let text = "Ag17汉";
    let (cached, paint) = make_context()?;
    let (fresh, reference) = make_context()?;
    let width = cached.measure_text_width(text, &paint.clone().with_font_size(12.0))?;
    let expected_width = fresh
        .measure_text(0.0, 0.0, text, &reference.clone().with_font_size(12.0))?
        .width();
    assert_eq!(width, expected_width);
    let measured = cached.measure_text(3.5, 7.25, text, &paint.with_font_size(26.5))?;
    let expected = fresh.measure_text(3.5, 7.25, text, &reference.with_font_size(26.5))?;

    assert!(measured.height() > 0.0);
    assert_eq!(measured.height(), expected.height());
    assert_eq!(measured.width(), expected.width());
    assert_eq!(measured.glyphs.len(), expected.glyphs.len());
    for (a, b) in measured.glyphs.iter().zip(&expected.glyphs) {
        assert_eq!(a.glyph_id, b.glyph_id);
        assert_eq!(
            [a.x, a.y, a.width, a.height, a.bearing_y],
            [b.x, b.y, b.width, b.height, b.bearing_y],
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn wrapped_document_with_many_distinct_glyphs_keeps_batching()
-> Result<(), Box<dyn std::error::Error>> {
    // Exercise the system CJK fallback with enough different glyphs to
    // exceed the budget for retained positions while seeding the atlas.
    let mut text = String::new();
    for row in 0..700 {
        text.extend(
            (0..44).filter_map(|column| char::from_u32(0x4e00 + (row * 44 + column) % 0x4f00)),
        );
        text.push('\n');
    }
    let batches = benchmark_document(
        text,
        (0..73).map(|phase| (328 + 4 * phase as u32, Some(-125 * phase - 8))),
        "wrapped_document_distinct_glyphs",
        "wrapped-document-distinct-glyphs-benchmark.json",
    )?;
    assert_eq!(batches, 73, "expected batched text from the first frame");
    Ok(())
}

fn benchmark_document(
    text: String,
    placements: impl ExactSizeIterator<Item = (u32, Option<i32>)>,
    scenario: &str,
    filename: &str,
) -> Result<u64, Box<dyn std::error::Error>> {
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
        let (render_scene, layout) = cache
            .prepare_reusing(scene.clone(), viewport, &input, |source, previous| {
                renderer.prepare_scene_recycling(&Document, source, viewport, &input, previous)
            })
            .into_parts();
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
    let glyph_batches = renderer.display_batch_stats().glyph_batches;
    let report = serde_json::json!({
        "scenario": scenario,
        "mode": "offscreen_serialized_gpu",
        "adapter": info.name,
        "backend": format!("{:?}", info.backend),
        "viewport": [viewport.width, viewport.height],
        "scale": SCALE,
        "text_bytes": text_bytes,
        "early_frames": EARLY_FRAMES,
        "glyph_batches": glyph_batches,
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
    Ok(glyph_batches)
}
