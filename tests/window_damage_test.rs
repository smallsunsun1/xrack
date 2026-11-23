mod gpu_readback;

use std::rc::Rc;
use window_damage_ui::{DamageDemo, MixedDamageDemo};
use xrack_layout::{Interaction, Point, Size, StackTransitions, TextAreaMotion};
use xrack_renderer_femtovg::{RenderDevice, Renderer, WindowDamage};
use xrack_runtime::{Component, Length, Node};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Harness {
    incremental: Renderer,
    reference: Renderer,
    scale: f32,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    sample: xrack_runtime::update_profile::UpdateSample,
}

impl Harness {
    fn compare(
        &mut self,
        scene: &Node,
        input: &Interaction,
        copy: bool,
    ) -> TestResult<(WindowDamage, Vec<u8>)> {
        let viewport = Size::new(
            self.width as f32 / self.scale,
            self.height as f32 / self.scale,
        );
        let layout = Rc::new(self.incremental.layout_with_input(scene, viewport, input));
        self.compare_prepared(scene, &layout, input, copy)
    }

    fn compare_prepared(
        &mut self,
        scene: &Node,
        layout: &Rc<xrack_layout::LayoutNode>,
        input: &Interaction,
        copy: bool,
    ) -> TestResult<(WindowDamage, Vec<u8>)> {
        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
        if copy {
            usage |= wgpu::TextureUsages::COPY_DST;
        }
        // A different target on each frame exercises swapchain rotation.
        let target = self
            .incremental
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("window damage test destination"),
                size: wgpu::Extent3d {
                    width: self.width,
                    height: self.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage,
                view_formats: &[],
            });
        let mut work = Ok(WindowDamage::Full);
        let sample = xrack_runtime::update_profile::measure(|| {
            work = self.incremental.draw_window_prepared(
                scene,
                layout,
                &target,
                self.scale,
                input,
                &StackTransitions::default(),
            );
        })
        .ok_or("window damage profiling was already active")?;
        self.sample = sample;
        let work = work?;
        let expected = self.reference.snapshot_prepared(
            scene,
            layout.as_ref().clone(),
            self.width,
            self.height,
            self.scale,
            input,
        )?;
        let mut pixels = gpu_readback::read_texture(&self.incremental, &target)?;
        if self.format == wgpu::TextureFormat::Bgra8Unorm {
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        if let Some(index) = pixels
            .chunks_exact(4)
            .zip(expected.rgba.chunks_exact(4))
            .position(|(pixel, reference)| pixel != reference)
        {
            let byte = index * 4;
            panic!(
                "{work:?} at scale {} differs at ({}, {}): {:?} != {:?}",
                self.scale,
                index % self.width as usize,
                index / self.width as usize,
                &pixels[byte..byte + 4],
                &expected.rgba[byte..byte + 4],
            );
        }
        Ok((work, pixels))
    }
}

#[track_caller]
fn assert_small(work: WindowDamage, screen: [u32; 2]) {
    let WindowDamage::Partial { width, height, .. } = work else {
        panic!("expected partial redraw, got {work:?}");
    };
    assert!(width * height < screen[0] * screen[1] / 4, "{work:?}");
}

#[test]
fn dirty_regions_match_full_frames_through_aot_and_interpreter() -> TestResult {
    let compiled = xrack_compiler::compile(include_str!("window_damage.xr"), "window_damage.xr")?;
    let dynamic = xrack_interpreter::load_mlir(&compiled.mlir)?.create("DamageDemo")?;
    let mut aot = DamageDemo::new();
    let device = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    for (scale, format) in [
        (1.0, wgpu::TextureFormat::Rgba8Unorm),
        (1.5, wgpu::TextureFormat::Rgba8Unorm),
        (2.0, wgpu::TextureFormat::Bgra8Unorm),
    ] {
        let width = (320.0 * scale) as u32;
        let height = (200.0 * scale) as u32;
        let mut harness = Harness {
            incremental: Renderer::with_device(device.clone())?,
            reference: Renderer::with_device(device.clone())?,
            scale,
            width,
            height,
            format,
            sample: Default::default(),
        };
        harness.incremental.set_window_damage_forced(true);
        for use_interpreter in [false, true] {
            aot.set_tile_x(Length(34.0));
            aot.set_show_tile(true);
            aot.set_show_custom(false);
            dynamic.set("tile-x", Length(34.0).into())?;
            dynamic.set("show-tile", true.into())?;
            dynamic.set("show-custom", false.into())?;
            let mut input = Interaction::default();
            let select = |aot: &mut DamageDemo| {
                if use_interpreter {
                    dynamic.render_shared()
                } else {
                    aot.render_shared()
                }
            };
            let scene = select(&mut aot);
            assert_eq!(harness.compare(&scene, &input, true)?.0, WindowDamage::Full);
            let full_draws = harness.sample.draw_primitives;
            let (work, original) = harness.compare(&scene, &input, false)?;
            assert_eq!(work, WindowDamage::Unchanged);
            assert_eq!(harness.sample.draw_primitives, 0);
            let layout =
                harness
                    .incremental
                    .layout_with_input(&scene, Size::new(320.0, 200.0), &input);
            input.pointer_move(&scene, &layout, Point { x: 28.0, y: 24.0 });
            let (hover_work, hover_pixels) = harness.compare(&scene, &input, true)?;
            assert_small(hover_work, [width, height]);
            assert_eq!(harness.sample.window_image_transfers, 1);
            assert_eq!(harness.sample.gpu_submissions, 1);

            assert!(harness.sample.draw_primitives < full_draws);
            if scale == 1.0 && !use_interpreter {
                println!(
                    "hover: {hover_work:?} of {width}x{height}, primitive dispatches {} -> {}",
                    full_draws, harness.sample.draw_primitives,
                );
            }
            assert_ne!(hover_pixels, original);
            input.pointer_move(&scene, &layout, Point { x: 294.0, y: 116.0 });
            assert_small(harness.compare(&scene, &input, false)?.0, [width, height]);

            aot.set_tile_x(Length(68.0));
            dynamic.set("tile-x", Length(68.0).into())?;
            let (work, moved) = harness.compare(&select(&mut aot), &input, false)?;
            assert_small(work, [width, height]);
            assert_ne!(moved, original);

            aot.set_show_tile(false);
            dynamic.set("show-tile", false.into())?;
            let scene = select(&mut aot);
            let (work, removed) = harness.compare(&scene, &input, true)?;
            assert_small(work, [width, height]);
            assert_ne!(removed, moved);

            let layout =
                harness
                    .incremental
                    .layout_with_input(&scene, Size::new(320.0, 200.0), &input);
            input.pointer_move(&scene, &layout, Point { x: 176.0, y: 40.0 });
            input.pointer_press_in_scene(&scene, &layout);
            let (work, focused) = harness.compare(&scene, &input, false)?;
            assert_small(work, [width, height]);
            assert_ne!(focused, removed);
            let layout = Rc::new(layout);
            input.sync_editor(&scene);
            let (_, before_caret) = harness.compare_prepared(&scene, &layout, &input, true)?;
            assert!(harness.incremental.text.move_editor_caret(
                &mut input,
                &scene,
                &layout,
                TextAreaMotion::Left,
                false,
            ));
            let (work, caret) = harness.compare_prepared(&scene, &layout, &input, false)?;
            assert_small(work, [width, height]);
            assert_ne!(caret, before_caret);
            assert_eq!(harness.sample.window_geometry_builds, 0);

            aot.set_show_custom(true);
            dynamic.set("show-custom", true.into())?;
            assert_eq!(
                harness.compare(&select(&mut aot), &input, true)?.0,
                WindowDamage::Full
            );
            harness.width += 2;
            assert_eq!(
                harness.compare(&scene, &input, false)?.0,
                WindowDamage::Full
            );
            harness.width = width;
            assert_eq!(harness.sample.window_image_transfers, 0);
        }
    }
    Ok(())
}

#[test]
fn window_damage_keeps_direct_frames_off_the_backing() -> TestResult {
    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut harness = Harness {
        incremental: Renderer::with_device(gpu.clone())?,
        reference: Renderer::with_device(gpu)?,
        scale: 1.0,
        width: 320,
        height: 200,
        format: wgpu::TextureFormat::Bgra8Unorm,
        sample: Default::default(),
    };
    let mut scene = DamageDemo::new().render();
    let mut input = Interaction::default();
    // Automatic mode starts on the direct path, even if the first frame is expensive.
    for _ in 0..2 {
        assert_eq!(
            harness.compare(&scene, &input, false)?.0,
            WindowDamage::Full
        );
        assert_eq!(harness.sample.window_image_transfers, 0);
    }

    harness.incremental.set_window_damage_forced(true);
    assert_eq!(
        harness.compare(&scene, &input, false)?.0,
        WindowDamage::Full
    );
    assert_eq!(harness.sample.window_image_transfers, 1);
    assert_eq!(
        harness.compare(&scene, &input, false)?.0,
        WindowDamage::Unchanged
    );

    // Full-damage frames discard the pixel baseline, even when re-use is forced.
    let mut previous = Vec::new();
    for color in [0x667799ff, 0x334455ff].into_iter().cycle().take(12) {
        scene
            .properties
            .insert("background", xrack_runtime::Color(color).into());
        let (work, pixels) = harness.compare(&scene, &input, false)?;
        assert_eq!(work, WindowDamage::Full);
        assert_ne!(pixels, previous);
        assert_eq!(harness.sample.window_image_transfers, 0);
        previous = pixels;
    }

    let layout = harness
        .incremental
        .layout_with_input(&scene, Size::new(320.0, 200.0), &input);
    input.pointer_move(&scene, &layout, Point { x: 28.0, y: 24.0 });
    // The first local update reseeds after full damage. Its entire image must
    // match the latest direct frame and may only be reused on later frames.
    assert_eq!(
        harness.compare(&scene, &input, false)?.0,
        WindowDamage::Full
    );
    assert_eq!(harness.sample.window_image_transfers, 1);
    input.pointer_move(&scene, &layout, Point { x: 290.0, y: 110.0 });
    assert_small(harness.compare(&scene, &input, false)?.0, [320, 200]);
    assert_eq!(harness.sample.window_image_transfers, 1);
    assert_eq!(harness.sample.gpu_submissions, 1);
    Ok(())
}

#[test]
fn prepared_geometry_reuse_keeps_disjoint_damage_out_of_idle_viewports() -> TestResult {
    let compiled = xrack_compiler::compile(include_str!("window_damage.xr"), "window_damage.xr")?;
    let interpreted = xrack_interpreter::load_mlir(&compiled.mlir)?.create("MixedDamageDemo")?;
    let aot = MixedDamageDemo::new();
    let device = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    for scale in [1.0, 1.5, 2.0] {
        for scene in [aot.render_shared(), interpreted.render_shared()] {
            let mut harness = Harness {
                incremental: Renderer::with_device(device.clone())?,
                reference: Renderer::with_device(device.clone())?,
                scale,
                width: (400.0 * scale) as u32,
                height: (240.0 * scale) as u32,
                format: wgpu::TextureFormat::Bgra8Unorm,
                sample: Default::default(),
            };
            harness.incremental.set_window_damage_forced(true);
            let mut input = Interaction::default();
            let viewport = Size::new(400.0, 240.0);
            let layout = Rc::new(
                harness
                    .incremental
                    .layout_with_input(&scene, viewport, &input),
            );
            assert_eq!(
                harness.compare_prepared(&scene, &layout, &input, false)?.0,
                WindowDamage::Full
            );
            assert_eq!(harness.sample.window_geometry_builds, 1);
            assert_eq!(harness.sample.window_image_transfers, 1);

            let left = Point { x: 40.0, y: 56.0 };
            let right = Point { x: 330.0, y: 56.0 };
            input.pointer_move(&scene, &layout, left);
            assert_small(
                harness.compare_prepared(&scene, &layout, &input, true)?.0,
                [harness.width, harness.height],
            );
            assert_eq!(harness.sample.window_geometry_builds, 0);
            input.pointer_move(&scene, &layout, right);
            let (work, _) = harness.compare_prepared(&scene, &layout, &input, false)?;
            let WindowDamage::PartialRegions(regions) = work else {
                panic!("expected two disjoint hover regions, got {work:?}");
            };
            assert_eq!(regions.rectangles().len(), 2);
            assert!(
                regions.rectangles()[0].x + regions.rectangles()[0].width < (132.0 * scale) as u32
            );
            assert!(regions.rectangles()[1].x >= (268.0 * scale) as u32);
            assert_eq!(harness.sample.window_geometry_builds, 0);
            assert_eq!(harness.sample.window_image_transfers, 1);
            assert_eq!(harness.sample.gpu_submissions, 1);

            // The pointer is outside the viewport, but the button overlaps it.
            input.pointer_move(&scene, &layout, Point { x: 114.0, y: 115.0 });
            assert_eq!(
                harness.compare_prepared(&scene, &layout, &input, true)?.0,
                WindowDamage::Full
            );
            assert_eq!(harness.sample.window_image_transfers, 0);

            input.pointer_move(&scene, &layout, Point { x: 160.0, y: 40.0 });
            assert!(input.scroll_y(&layout, 72.0) || input.scroll_y(&layout, -72.0));
            let scrolled = Rc::new(
                harness
                    .incremental
                    .layout_with_input(&scene, viewport, &input),
            );
            assert_eq!(
                harness
                    .compare_prepared(&scene, &scrolled, &input, false)?
                    .0,
                WindowDamage::Full
            );
            assert_eq!(harness.sample.window_image_transfers, 0);

            input.pointer_move(&scene, &scrolled, left);
            assert_eq!(
                harness.compare_prepared(&scene, &scrolled, &input, true)?.0,
                WindowDamage::Full
            );
            assert_eq!(harness.sample.window_image_transfers, 1);
            input.pointer_move(&scene, &scrolled, right);
            assert!(matches!(
                harness
                    .compare_prepared(&scene, &scrolled, &input, false)?
                    .0,
                WindowDamage::PartialRegions(_)
            ));
            assert_eq!(harness.sample.window_geometry_builds, 0);

            // A translucent group across both buttons joins the regions
            // and is replayed once even when both hover states change.
            let mut overlap_scene = scene.as_ref().clone();
            overlap_scene.children.truncate(2);
            let mut ribbon = Node::new("Rectangle", None);
            for (name, value) in [("x", 8.0), ("y", 41.0), ("width", 384.0), ("height", 38.0)] {
                ribbon.properties.insert(name, Length(value).into());
            }
            ribbon
                .properties
                .insert("background", xrack_runtime::Color(0x52a0e288).into());
            overlap_scene.children.insert(0, ribbon);
            harness.incremental.set_window_damage_forced(false);
            harness.incremental.set_window_damage_forced(true);
            let mut input = Interaction::default();
            let layout = Rc::new(harness.incremental.layout_with_input(
                &overlap_scene,
                viewport,
                &input,
            ));
            input.pointer_move(&overlap_scene, &layout, left);
            assert_eq!(
                harness
                    .compare_prepared(&overlap_scene, &layout, &input, false)?
                    .0,
                WindowDamage::Full
            );
            input.pointer_move(&overlap_scene, &layout, right);
            let work = harness
                .compare_prepared(&overlap_scene, &layout, &input, false)?
                .0;
            assert!(
                matches!(work, WindowDamage::Partial { width, .. } if width >= (384.0 * scale) as u32),
                "{work:?}"
            );
        }
    }
    Ok(())
}
