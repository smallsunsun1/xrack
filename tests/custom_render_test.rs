use custom_render_ui::CustomRenderProbe;
use std::{
    cell::{Cell, OnceCell},
    rc::Rc,
};
use xrack_interpreter::Program;
use xrack_layout::{Interaction, Rect};
use xrack_renderer_femtovg::{Geometry, Renderer, Snapshot, register_geometry, register_gpu};
use xrack_runtime::{Color, Component};

fn color_pipelines(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> [wgpu::RenderPipeline; 2] {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("XRack custom node test shader"),
        source: wgpu::ShaderSource::Wgsl(
            r#"
            @vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
                let positions = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
                return vec4(positions[index], 0.0, 1.0);
            }
            @fragment fn warm() -> @location(0) vec4<f32> {
                return vec4(1.0, 0.0, 0.0, 1.0);
            }
            @fragment fn cool() -> @location(0) vec4<f32> {
                return vec4(1.0, 1.0, 0.0, 1.0);
            }
        "#
            .into(),
        ),
    });
    ["warm", "cool"].map(|entry| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(entry),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        })
    })
}

fn pixel(frame: &Snapshot, x: usize, y: usize) -> &[u8] {
    let offset = (y * frame.width as usize + x) * 4;
    &frame.rgba[offset..offset + 4]
}

#[test]
fn retained_visual_group_reuses_translated_image_and_text_and_repaints_edits()
-> Result<(), Box<dyn std::error::Error>> {
    use custom_render_ui::RetainedSceneProbe;
    use xrack_runtime::{Image, ImageFormat, Length, LoadedImage, Value};

    let gpu = pollster::block_on(xrack_renderer_femtovg::RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut retained = Renderer::with_device(gpu.clone())?;
    let mut direct = Renderer::with_device(gpu)?;
    retained.set_retained_geometry_enabled(true);
    let mut aot = RetainedSceneProbe::new();
    let preview = xrack_compiler::prepare_preview(
        include_str!("custom_render.xr"),
        "custom_render.xr",
        Default::default(),
    )?;
    let instance = Program::from_module(&preview.module)?.create("RetainedSceneProbe")?;
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="red"/></svg>"#;
    let picture = Image::Loaded(LoadedImage::new(ImageFormat::Svg, svg.to_vec(), [8, 8]));
    aot.set_picture(picture.clone());
    instance.set("picture", Value::Image(picture))?;
    let input = Interaction::default();
    for _ in 0..3 {
        retained.snapshot(&aot.render_shared(), 128, 64, 1.0, &input)?;
    }
    assert!(retained.opacity_layer_stats().rasterized >= 1);
    let before = retained.opacity_layer_stats();
    aot.set_offset(Length(16.0));
    instance.set("offset", Value::Length(Length(16.0)))?;
    let moved = retained.snapshot(&aot.render_shared(), 128, 64, 1.0, &input)?;
    let expected = direct.snapshot(&instance.render_shared(), 128, 64, 1.0, &input)?;
    assert_eq!(moved.rgba, expected.rgba);
    assert_eq!(retained.opacity_layer_stats().reused, before.reused + 1);
    aot.set_caption("Edited".into());
    instance.set("caption", Value::String("Edited".into()))?;
    let edited = retained.snapshot(&aot.render_shared(), 128, 64, 1.0, &input)?;
    let expected = direct.snapshot(&instance.render_shared(), 128, 64, 1.0, &input)?;
    assert_eq!(edited.rgba, expected.rgba);
    assert_ne!(edited.rgba, moved.rgba);
    Ok(())
}

#[test]
fn custom_pass_and_shader_effect_share_the_aot_and_interpreter_frame()
-> Result<(), Box<dyn std::error::Error>> {
    use xrack_renderer_femtovg::{register_gpu_pass, register_shader_effect};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let input = Interaction::default();
    let aot = CustomRenderProbe::new();
    let preview = xrack_compiler::prepare_preview(
        include_str!("custom_render.xr"),
        "custom_render.xr",
        Default::default(),
    )?;
    let dynamic = Program::from_module(&preview.module)?.create("CustomRenderProbe")?;
    let inline = Rc::new(Cell::new(0));
    let offscreen = Rc::new(Cell::new(0));
    let pass = register_gpu_pass("custom-render-test/gpu", {
        let inline = inline.clone();
        let offscreen = offscreen.clone();
        let pipelines = OnceCell::new();
        move |ctx| {
            let calls = if ctx.inline { &inline } else { &offscreen };
            calls.set(calls.get() + 1);
            let pipeline = &pipelines.get_or_init(|| color_pipelines(ctx.device, ctx.format))[0];
            ctx.pass.set_pipeline(pipeline);
            ctx.pass.draw(0..3, 0..1);
            Ok(())
        }
    })?;
    let draw = |renderer: &mut Renderer, component: &dyn Component| {
        renderer.snapshot(&component.render_shared(), 128, 96, 2.0, &input)
    };
    let compiled = draw(&mut renderer, &aot)?;
    let interpreted = draw(&mut renderer, &dynamic)?;
    assert_eq!(compiled.rgba, interpreted.rgba);
    assert_eq!(pixel(&compiled, 26, 26), [255, 0, 0, 255]);
    assert_eq!(pixel(&compiled, 40, 40), [0, 0, 255, 255]);
    assert_eq!(pixel(&compiled, 2, 2), [17, 34, 51, 255]);
    assert_eq!(inline.get(), 2);
    assert_eq!(offscreen.get(), 2);
    assert!(renderer.transient_target_reuses() >= 1);
    drop(pass);

    let _shader = register_shader_effect(
        "custom-render-test/gpu",
        r#"
        fn shade(uv: vec2<f32>, frame: vec4<f32>) -> vec4<f32> {
            return vec4(0.0, select(0.0, 1.0, frame.x > 0.0), 0.0, 1.0);
        }
    "#,
    )?;
    let compiled = draw(&mut renderer, &aot)?;
    let interpreted = draw(&mut renderer, &dynamic)?;
    assert_eq!(compiled.rgba, interpreted.rgba);
    assert_eq!(pixel(&compiled, 26, 26), [0, 255, 0, 255]);
    assert_eq!(pixel(&compiled, 40, 40), [0, 0, 255, 255]);
    Ok(())
}

#[test]
fn custom_geometry_and_gpu_follow_scene_clipping_and_paint_order()
-> Result<(), Box<dyn std::error::Error>> {
    let geometry_calls = Rc::new(Cell::new(0));
    let geometry = register_geometry("custom-render-test/geometry", {
        let calls = Rc::clone(&geometry_calls);
        move |context| {
            calls.set(calls.get() + 1);
            assert_eq!(context.bounds, Rect::new(0.0, 0.0, 20.0, 20.0));
            let color = if context.revision == 0 {
                Color(0x00ff00ff)
            } else {
                Color(0xffffffff)
            };
            (0..10)
                .map(|row| Geometry::Rectangle {
                    bounds: Rect::new(0.0, row as f32 * 2.0, 40.0, 2.0),
                    color,
                })
                .collect()
        }
    })?;
    assert!(register_geometry("custom-render-test/geometry", |_| Vec::new()).is_err());

    let gpu_calls = Rc::new(Cell::new(0));
    let gpu = register_gpu("custom-render-test/gpu", {
        let calls = Rc::clone(&gpu_calls);
        let pipelines = OnceCell::new();
        move |context| {
            calls.set(calls.get() + 1);
            assert_eq!(context.format, wgpu::TextureFormat::Rgba8Unorm);
            assert_eq!(context.size[0], (context.paint.bounds.width * 2.0) as u32);
            assert_eq!(context.size[1], (context.paint.bounds.height * 2.0) as u32);
            if context.paint.data == "fail" {
                return Err("rejected sample".into());
            }
            let pipelines =
                pipelines.get_or_init(|| color_pipelines(context.device, context.format));
            let pipeline = &pipelines[usize::from(context.paint.data != "warm")];
            let mut pass = context
                .encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("custom XRack GPU test"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: context.target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
            pass.set_pipeline(pipeline);
            pass.draw(0..3, 0..1);
            Ok(())
        }
    })?;

    let input = Interaction::default();
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let preview = xrack_compiler::prepare_preview(
        include_str!("custom_render.xr"),
        "custom_render.xr",
        Default::default(),
    )?;
    let interpreted = Program::from_module(&preview.module)?.create("CustomRenderProbe")?;
    let mut generated = CustomRenderProbe::new();

    for (revision, data, flat, g) in [
        (0, "warm", [0, 255, 0, 255], 0),
        (1, "cool", [255, 255, 255, 255], 255),
    ] {
        generated.set_paint_revision(revision);
        generated.set_paint_data(data.into());
        interpreted.set("paint-revision", revision.into())?;
        interpreted.set("paint-data", data.to_owned().into())?;
        let aot = renderer.snapshot(&generated.render_shared(), 128, 96, 2.0, &input)?;
        let scene = interpreted.try_render()?;
        let dynamic = renderer.snapshot(&scene, 128, 96, 2.0, &input)?;
        assert_eq!(aot.rgba, dynamic.rgba);
        assert_eq!(pixel(&aot, 18, 18), flat);
        assert_eq!(pixel(&aot, 32, 32), [255, g, 0, 255]);
        assert_eq!(pixel(&aot, 38, 38), [0, 0, 255, 255]);
        assert_eq!(pixel(&aot, 18, 54), [17, 34, 51, 255]);
        assert_eq!(pixel(&aot, 110, 23), [17, 34, 51, 255]);
        let blended = pixel(&aot, 96, 23);
        assert_eq!(blended[3], 255);
        assert!(blended[0].abs_diff(136) <= 2);
        assert!(blended[1].abs_diff(((34u16 + g as u16) / 2) as u8) <= 2);
        assert!(blended[2].abs_diff(25) <= 2);
    }

    assert_eq!(geometry_calls.get(), 4);
    assert_eq!(gpu_calls.get(), 8);
    assert_eq!(renderer.rectangle_batch_draws(), 4);

    generated.set_paint_data("fail".into());
    let result = renderer.snapshot(&generated.render_shared(), 128, 96, 2.0, &input);
    assert!(matches!(result, Err(error) if error.contains("rejected sample")));
    generated.set_paint_data("warm".into());
    let recovered = renderer.snapshot(&generated.render_shared(), 128, 96, 2.0, &input)?;
    assert_eq!(pixel(&recovered, 32, 32), [255, 0, 0, 255]);
    drop((geometry, gpu));
    let fallback = renderer.snapshot(&generated.render_shared(), 128, 96, 2.0, &input)?;
    assert_eq!(pixel(&fallback, 32, 32), [17, 34, 51, 255]);
    assert!(register_geometry("custom-render-test/geometry", |_| Vec::new()).is_ok());
    Ok(())
}

#[test]
fn display_list_batches_images_and_glyphs_with_pixel_parity()
-> Result<(), Box<dyn std::error::Error>> {
    use custom_render_ui::DisplayListProbe;
    use xrack_runtime::{Image, ImageFormat, LoadedFonts};

    let asset = Image::Embedded {
        format: ImageFormat::Png,
        bytes: include_bytes!("../examples/media/assets/sprite-atlas.png"),
        size: [8, 8],
    };
    let fonts = LoadedFonts::new(vec![
        include_bytes!("../examples/layout/fonts/demo.ttc").to_vec(),
    ]);
    let mut aot = DisplayListProbe::new();
    aot.set_picture(asset.clone());
    let preview = xrack_compiler::prepare_preview(
        include_str!("custom_render.xr"),
        "custom_render.xr",
        Default::default(),
    )?;
    let interpreter = Program::from_module(&preview.module)?.create("DisplayListProbe")?;
    interpreter.set("picture", asset.into())?;
    let mut aot = aot.render();
    let mut interpreted = interpreter.try_render()?.as_ref().clone();
    aot.set_loaded_fonts(Some(fonts.clone()));
    interpreted.set_loaded_fonts(Some(fonts));

    let gpu = pollster::block_on(xrack_renderer_femtovg::RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut optimized = Renderer::with_device(gpu.clone())?;
    let mut direct = Renderer::with_device(gpu)?;
    direct.set_display_batching_enabled(false);
    let input = Interaction::default();
    for scale in [1.0, 2.0] {
        for scene in [&aot, &interpreted] {
            let width = (112.0 * scale) as u32;
            let height = (80.0 * scale) as u32;
            let before = optimized.display_batch_stats();
            let expected = direct.snapshot(scene, width, height, scale, &input)?;
            let actual = optimized.snapshot(scene, width, height, scale, &input)?;
            let after = optimized.display_batch_stats();
            assert_eq!(
                after.image_batches - before.image_batches,
                1,
                "scale {scale}"
            );
            assert_eq!(after.images - before.images, 2, "scale {scale}");
            assert_eq!(
                after.glyph_batches - before.glyph_batches,
                1,
                "scale {scale}"
            );
            assert_eq!(after.text_runs - before.text_runs, 2, "scale {scale}");
            assert_eq!(actual.layout, expected.layout);
            for (i, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
                assert!(
                    actual.abs_diff(*expected) <= 1,
                    "scale {scale}, pixel {}, channel {}: {actual} differs from {expected}",
                    i / 4,
                    i % 4
                );
            }
        }
    }
    Ok(())
}

#[test]
fn compiled_plans_and_indexed_models_match_the_interpreter()
-> Result<(), Box<dyn std::error::Error>> {
    use custom_render_ui::{PerformanceProbe, PerformanceRow};
    use xrack_runtime::{RecordValue, Value, update_profile};

    let preview = xrack_compiler::prepare_preview(
        include_str!("custom_render.xr"),
        "custom_render.xr",
        Default::default(),
    )?;
    let optimized = preview.module.to_mlir();
    assert!(optimized.contains("xr.concat"), "{optimized}");
    assert!(
        optimized.contains("xr.cache_callback_default"),
        "{optimized}"
    );
    assert!(
        optimized.contains("xr.inline_callback_default"),
        "{optimized}"
    );
    let instance = Program::from_module(&preview.module)?.create("PerformanceProbe")?;
    let mut generated = PerformanceProbe::new();

    let row = |owner: &str, label: &str| PerformanceRow {
        owner: owner.into(),
        label: label.into(),
    };
    let host_row = |owner: &str, label: &str| {
        RecordValue::new(
            "PerformanceRow",
            [
                ("owner".into(), Value::String(owner.into())),
                ("label".into(), Value::String(label.into())),
            ]
            .into_iter()
            .collect(),
        )
    };
    generated.set_rows(vec![row("one", "A"), row("One", "B"), row("one-more", "C")]);
    let host = xrack_runtime::RecordList::new(
        "PerformanceRow",
        vec![
            host_row("one", "A"),
            host_row("One", "B"),
            host_row("one-more", "C"),
        ],
    );
    instance.set("rows", Value::RecordList(host))?;
    assert_eq!(generated.get_selected(), [row("one", "A")]);
    assert_eq!(generated.get_filtered().len(), 3);
    assert_eq!(generated.get_sorted()[0].label, "B");
    assert_eq!(instance.record_model("selected")?.model().len(), 1);
    assert_eq!(instance.record_model("filtered")?.model().len(), 3);
    let projected = generated.model_selected();
    assert_eq!(projected.row_key(0), generated.model_rows().row_key(0));

    let profile = update_profile::measure(|| {
        generated.model_rows().push(row("two", "D"));
        assert_eq!(generated.model_selected().len(), 1);
    })
    .ok_or("profiling already active")?;
    assert_eq!(profile.model_rows_scanned, 1);
    assert_eq!(profile.model_rows_cloned, 0);

    let before_color = generated.render();
    generated.set_ink(Color(0xffffffff));
    assert!(before_color.same_frame_inputs(&generated.render()));
    assert!(generated.get_has_one());
    assert_eq!(instance.get("has-one")?, Value::Bool(true));
    assert_eq!(generated.get_plus_result(), 11);
    assert_eq!(instance.get("plus-result")?, Value::Int(11));
    assert_eq!(generated.get_summary(), "[7]: one");
    assert_eq!(instance.get("summary")?, Value::String("[7]: one".into()));
    generated.invoke_advance();
    instance.invoke("advance", &[])?;
    assert_eq!(generated.get_summary(), "[9]: one");
    assert_eq!(instance.get("summary")?, Value::String("[9]: one".into()));
    generated.on_calculate(|| 27);
    instance.on("calculate", Rc::new(|_| Ok(Some(27i64.into()))))?;
    assert_eq!(generated.get_summary(), "[27]: one");
    assert_eq!(instance.get("summary")?, Value::String("[27]: one".into()));
    assert_eq!(generated.get_plus_result(), 12);
    assert_eq!(instance.get("plus-result")?, Value::Int(12));
    generated.on_add_amount(|value| value * 3);
    instance.on(
        "add-amount",
        Rc::new(|args| {
            let [Value::Int(value)] = args else {
                return Err("expected int argument".into());
            };
            Ok(Some(Value::Int(value * 3)))
        }),
    )?;
    assert_eq!(generated.get_plus_result(), 24);
    assert_eq!(instance.get("plus-result")?, Value::Int(24));

    Ok(())
}

#[test]
fn opaque_geometry_reuses_its_gpu_buffer_after_translation()
-> Result<(), Box<dyn std::error::Error>> {
    use custom_render_ui::RetainedGeometryProbe;
    use xrack_runtime::Length;

    let mut component = RetainedGeometryProbe::new();
    let gpu = pollster::block_on(xrack_renderer_femtovg::RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut retained = Renderer::with_device(gpu.clone())?;
    let mut direct = Renderer::with_device(gpu)?;
    retained.set_retained_geometry_enabled(true);
    let input = Interaction::default();
    for position in [10.0, 20.0, 22.0] {
        component.set_offset(Length(position));
        let scene = component.render();
        let expected = direct.snapshot(&scene, 160, 64, 1.0, &input)?;
        let actual = retained.snapshot(&scene, 160, 64, 1.0, &input)?;
        assert_eq!(actual.rgba, expected.rgba, "offset {position}");
    }
    let (reuses, bytes) = retained.retained_geometry_stats();
    assert_eq!(reuses, 2);
    assert_eq!(bytes, 12 * 6 * 24);

    let rows = xrack_runtime::VecModel::from(vec![10, 20]);
    let first = rows.row_key(0).ok_or("first row")?;
    let second = rows.row_key(1).ok_or("second row")?;
    for row in [first, second] {
        let mut scene = component.render();
        scene.row_key = Some(row);
        retained.snapshot(&scene, 160, 64, 1.0, &input)?;
    }
    // Briefly leave the viewport and then return with the same source key.
    for _ in 0..4 {
        retained.snapshot(&xrack_runtime::Node::hidden(), 160, 64, 1.0, &input)?;
    }
    let mut scene = component.render();
    scene.row_key = Some(first);
    retained.snapshot(&scene, 160, 64, 1.0, &input)?;
    assert_eq!(retained.retained_row_geometry_reuses(), 1);
    assert_eq!(retained.retained_geometry_stats().1, 3 * 12 * 6 * 24);
    Ok(())
}
