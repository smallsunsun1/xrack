use std::{
    fmt::Write as _,
    hash::{DefaultHasher, Hash, Hasher},
    rc::Rc,
    time::{Duration, Instant},
};
use xrack_layout::{FrameCache, Interaction, LayoutNode, Size, StackTransitions};
use xrack_renderer_femtovg::Renderer;
use xrack_runtime::{
    Color, Component, Length, MarkdownStream, Node, Orientation, Property, ScrollAction,
    ScrollBarPolicy, ScrollRequest, StyledText, ViewportHandle,
};

const WIDTH: u32 = 1120;
const HEIGHT: u32 = 760;
const ROW_HEIGHT: f64 = 54.0;
struct Bench;
impl Component for Bench {
    fn render(&self) -> Node {
        Node::hidden()
    }
    fn dispatch(&mut self, _: &str, _: &str) -> bool {
        false
    }
}

fn styled(text: StyledText, y: f64, history: bool) -> Node {
    let mut node = Node::new("StyledText", None);
    for (name, length) in [
        ("x", 200.0),
        ("y", y),
        ("width", 720.0),
        ("font-size", 16.0),
    ] {
        node.properties.insert(name, Length(length).into());
    }
    if history {
        node.properties.insert("height", Length(ROW_HEIGHT).into());
    }
    node.properties
        .insert("font-family", "Helvetica Neue".to_owned().into());
    node.properties.insert("color", Color(0x18233dff).into());
    node.properties.insert("text", text.into());
    node
}

#[test]
fn transcript_layout_benchmark() -> Result<(), Box<dyn std::error::Error>> {
    let rows = std::env::var("XRACK_PEER_ROWS").map_or(Ok(4096_usize), |s| s.parse())?;
    let paragraphs = std::env::var("XRACK_PEER_PARAGRAPHS").map_or(Ok(100_usize), |s| s.parse())?;
    let mut source = String::new();
    for index in 0..paragraphs {
        writeln!(
            source,
            "**构建 {index:04}**  ·  编译布局和绘制  ·  `tests/gpu_test.rs:5`  ·  [日志](https://example.invalid/log/{index})\n"
        )?;
    }
    let load_document = std::env::var_os("XRACK_PEER_LOAD_DOCUMENT").is_some();
    let mut markdown = MarkdownStream::default();
    let mut viewport = Node::new("ScrollView", Some("transcript"));
    viewport
        .properties
        .insert("width", Length(f64::from(WIDTH)).into());
    viewport
        .properties
        .insert("height", Length(f64::from(HEIGHT)).into());
    for name in ["horizontal-scroll-bar-policy", "vertical-scroll-bar-policy"] {
        viewport
            .properties
            .insert(name, ScrollBarPolicy::AlwaysOff.into());
    }
    for index in 0..rows {
        let line = format!(
            "**第 {index:04} 条回复**  ·  渲染验证  ·  `cargo test`  ·  [项目](https://example.invalid/review/{index})"
        );
        viewport.children.push(
            styled(
                StyledText::from_markdown(&line)?,
                index as f64 * ROW_HEIGHT,
                true,
            )
            .into_shared(),
        );
    }
    let live_y = rows as f64 * ROW_HEIGHT;
    // Measure loading new document data separately from appending to a
    // document that was already parsed before the first render.
    let initial_text = if load_document {
        StyledText::default()
    } else {
        markdown.update(&source)?
    };
    viewport.children.push(styled(initial_text, live_y, false));
    let offset = Rc::new(Property::<f64>::default());
    let handle = ViewportHandle::default();
    let position = offset.clone();
    let write = offset.clone();
    handle.bind_offset(
        Orientation::Vertical,
        move || Length(-position.get(|| 0.0)),
        move |position| write.set(-position.0),
    );
    viewport.viewport_handle = Some(handle.clone());

    let mut scene = Node::new("Window", None);
    scene
        .properties
        .insert("background", Color(0xffffffff).into());
    scene.children.push(viewport);
    let mut scene = Rc::new(scene.into_shared());
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let texture = |renderer: &Renderer| {
        renderer.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("XRack transcript comparison"),
            size: wgpu::Extent3d {
                width: WIDTH * 2,
                height: HEIGHT * 2,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    };
    let targets = std::array::from_fn::<_, 2, _>(|_| texture(&renderer));
    let size = Size::new(WIDTH as f32, HEIGHT as f32);
    let input = Interaction::default();
    let transitions = StackTransitions::default();
    let mut frames = FrameCache::default();
    let mut last_pixels = None;
    // These workloads fit the scroll cache's slot budget and expose enough
    // offscreen rows to verify that window preparation deferred their text.
    let expect_deferral = matches!(rows, 2048 | 4096);
    let live_start = load_document || std::env::var_os("XRACK_PEER_LIVE_START").is_some();
    let trace_frames = std::env::var_os("XRACK_PEER_TRACE_FRAMES").is_some();
    let prepare_pipelines = std::env::var_os("XRACK_PEER_LAZY_PIPELINES").is_none();
    let mut reference = if std::env::var_os("XRACK_PEER_VERIFY_LAYOUT").is_some() {
        let mut reference = pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))?;
        reference.set_styled_text_incremental_enabled(false);
        reference.set_styled_text_culling_enabled(false);
        let target = texture(&reference);
        Some((reference, target))
    } else {
        None
    };
    for streaming in [false, true] {
        if live_start && !streaming {
            continue;
        }
        if !streaming && rows == 0 {
            continue;
        }
        let mut times: [Vec<Duration>; 5] = std::array::from_fn(|_| Vec::new());
        for index in 0..70 {
            let start = Instant::now();
            if prepare_pipelines && index == 0 {
                renderer.prepare_window_text_pipelines(&scene, targets[0].format());
            }
            if streaming {
                writeln!(
                    source,
                    "**输出 {index:04}**  ·  测试当前工作区  ·  `bazel test`  ·  [详情](https://example.invalid/output/{index})\n"
                )?;
                Rc::make_mut(&mut scene).children[0].children[rows]
                    .properties
                    .insert("text", markdown.update(&source)?.into());
                if index == 0 {
                    offset.set(f64::from(f32::MAX));
                }
            } else {
                offset.set(
                    (rows.saturating_sub(48)) as f64 * ROW_HEIGHT + (index % 24) as f64 * 17.0,
                );
            }
            let updated = Instant::now();
            let (prepared, layout) =
                frames.prepare_reusing(scene.clone(), size, &input, |base, previous| {
                    renderer.prepare_scene_recycling(&Bench, base, size, &input, previous)
                });
            let laid_out = Instant::now();
            let target = &targets[index % 2];
            renderer.draw_window_prepared(&prepared, &layout, target, 2.0, &input, &transitions)?;
            let submitted = Instant::now();
            renderer.device.poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(10)),
            })?;
            let complete = Instant::now();
            if trace_frames {
                println!(
                    "frame {streaming} {index} prepare={:.3} submit={:.3} total={:.3} update={:.3} wait={:.3}",
                    (laid_out - updated).as_secs_f64() * 1000.,
                    (submitted - laid_out).as_secs_f64() * 1000.,
                    (complete - start).as_secs_f64() * 1000.,
                    (updated - start).as_secs_f64() * 1000.,
                    (complete - submitted).as_secs_f64() * 1000.,
                );
            }
            if index >= 10 {
                for (samples, elapsed) in times.iter_mut().zip([
                    updated - start,
                    laid_out - updated,
                    submitted - laid_out,
                    complete - submitted,
                    complete - start,
                ]) {
                    samples.push(elapsed);
                }
            }
            if matches!(index, 0 | 1 | 14 | 39 | 69)
                && let Some((reference, reference_target)) = &mut reference
            {
                let pixels = gpu_readback::read_texture(&renderer, target)?;
                let mut fresh = reference.layout_with_input(&scene, size, &input);
                reference.draw_with_layout(&scene, &fresh, reference_target, 2.0, &input)?;
                if index == 0 && expect_deferral {
                    verify_deferred_text_jump(
                        &renderer,
                        &scene,
                        &layout,
                        &fresh,
                        &handle,
                        &offset,
                        rows / 2,
                    );
                }
                let mut deferred = 0;
                for (actual, expected) in layout.children[0]
                    .children
                    .iter()
                    .zip(&mut fresh.children[0].children)
                {
                    if actual.styled_text.is_none() && expected.styled_text.is_some() {
                        let visible = expected.bounds.intersect(expected.clip);
                        assert!(visible.width <= 0.0 || visible.height <= 0.0);
                        assert!(actual.text_link_regions().next().is_none());
                        expected.styled_text = None;
                        deferred += 1;
                    }
                }
                assert!(
                    *layout == fresh,
                    "cached layout differs at {streaming}/{index}"
                );
                if expect_deferral {
                    assert!(
                        deferred > rows / 2,
                        "expected deferred text, found {deferred} items"
                    );
                }
                println!("deferred text rows {streaming}/{index}={deferred}");
                let direct = gpu_readback::read_texture(reference, reference_target)?;
                let max_delta = pixels
                    .iter()
                    .zip(&direct)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap();
                assert!(
                    max_delta <= 1,
                    "cached paint differs at {streaming}/{index}: {max_delta}"
                );
                println!("verified layout and paint {streaming}/{index} max_delta={max_delta}");
                if trace_frames {
                    let mut hash = DefaultHasher::new();
                    pixels.hash(&mut hash);
                    println!("pixel hash {streaming}/{index}={:016x}", hash.finish());
                }
            }
            if index == 69 {
                let scroll = layout.children[0].scroll_on(Orientation::Vertical).unwrap();
                if streaming {
                    assert_eq!(scroll.offset, scroll.max_offset);
                }
                assert!(scroll.max_offset > 100.);
                println!(
                    "scroll offset={} extent={}",
                    scroll.offset, scroll.max_offset
                );
                let pixels = gpu_readback::read_texture(&renderer, target)?;
                let ink = pixels
                    .chunks_exact(4)
                    .filter(|pixel| *pixel != [255; 4])
                    .count();
                assert!(
                    ink > 1000 && ink < (WIDTH * HEIGHT) as usize * 3,
                    "ink {ink}"
                );
                if let Some(before) = last_pixels.take() {
                    assert_ne!(before, pixels);
                }
                last_pixels = Some(pixels);
            }
        }
        for (stage, samples) in ["update", "prepare", "submit", "wait", "total"]
            .into_iter()
            .zip(&mut times)
        {
            samples.sort_unstable();
            println!(
                "XRack {rows} rows streaming={streaming} {stage}: p50={:.3}ms p95={:.3}ms",
                samples[29].as_secs_f64() * 1000.0,
                samples[56].as_secs_f64() * 1000.0
            );
        }
    }
    if reference.is_some() && expect_deferral {
        // The eager entry point may reuse the same viewport templates after
        // a window frame, but must return text geometry for every row.
        let (_, eager) = renderer.prepare_scene(&Bench, scene, size, &input);
        assert!(
            eager.children[0]
                .children
                .iter()
                .all(|item| item.styled_text.is_some())
        );
    }
    Ok(())
}

fn verify_deferred_text_jump(
    renderer: &Renderer,
    scene: &Node,
    layout: &LayoutNode,
    eager: &LayoutNode,
    handle: &ViewportHandle,
    offset: &Property<f64>,
    row: usize,
) {
    assert!(layout.children[0].children[row].styled_text.is_none());
    let request = || ScrollRequest {
        target: handle.clone(),
        action: ScrollAction::ToStyledText {
            paragraph: row,
            offset: 7,
        },
    };
    let initial = offset.get(|| 0.0);
    assert!(Interaction::default().apply_scroll_requests(scene, eager, [request()]));
    let expected = offset.get(|| 0.0);
    offset.set(initial);
    let mut resolved = 0;
    assert!(Interaction::default().apply_scroll_requests_with_text(
        scene,
        layout,
        [request()],
        |node, item| {
            resolved += 1;
            renderer.text.measure_styled_text(node, item)
        }
    ));
    assert_eq!(resolved, 1);
    assert_eq!(offset.get(|| 0.0), expected);
    offset.set(initial);
}
