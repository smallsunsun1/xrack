use transform2d_ui::{TransformComboProbe, TransformProbe};
use xrack_interpreter::{Program, load_mlir};
use xrack_layout::{ComboGeometry, Interaction, Point, Rect, Size, item::ItemRef};
use xrack_renderer_femtovg::Renderer;
use xrack_runtime::{Component, Length, Point2D, Transform2D, Value, Vector2D};

const SOURCE: &str = include_str!("transform2d.xr");

#[test]
#[ignore = "manual AOT/interpreter transform-binding benchmark"]
fn benchmark_transform_binding() -> Result<(), Box<dyn std::error::Error>> {
    use std::{hint::black_box, time::Instant};

    const UPDATES: usize = 512;
    let mut aot = TransformProbe::new();
    let compiled = xrack_compiler::compile(SOURCE, "transform2d.xr")?;
    let interpreted = load_mlir(&compiled.mlir)?.create("TransformProbe")?;
    fn run(
        name: &str,
        mut update: impl FnMut(f64) -> Result<Length, Box<dyn std::error::Error>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut samples = Vec::new();
        for round in 0..36 {
            let start = Instant::now();
            for step in 0..UPDATES {
                let dx = black_box((step % 47) as f64 + 0.25);
                black_box(update(dx)?);
            }
            if round >= 6 {
                samples.push(start.elapsed().as_secs_f64() * 1e9 / UPDATES as f64);
            }
        }
        samples.sort_unstable_by(f64::total_cmp);
        println!(
            "{name} transform binding p50={:.1} ns p95={:.1} ns",
            samples[14], samples[28]
        );
        Ok(())
    }
    let compiled = move |dx| {
        aot.set_dx(Length(dx));
        Ok(aot.get_mapped_x())
    };
    let dynamic = move |dx| {
        interpreted.set("dx", Length(dx).into())?;
        let Value::Length(value) = interpreted.get("mapped-x")? else {
            return Err("mapped-x must be a length".into());
        };
        Ok(value)
    };
    if std::env::var_os("XRACK_BENCH_INTERPRETER_FIRST").is_some_and(|value| value == "1") {
        run("interpreter", dynamic)?;
        run("AOT", compiled)?;
    } else {
        run("AOT", compiled)?;
        run("interpreter", dynamic)?;
    }
    Ok(())
}

fn point(x: f64, y: f64) -> Point2D {
    Point2D::new(Length(x), Length(y))
}

#[test]
fn transformed_combo_uses_item_local_coordinates_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let compiled = xrack_compiler::compile(SOURCE, "transform2d.xr")?;
    let mut interpreted = load_mlir(&compiled.mlir)?.create("TransformComboProbe")?;
    let mut aot = TransformComboProbe::new();
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    for ui in [&mut aot as &mut dyn Component, &mut interpreted] {
        let mut input = Interaction::default();
        let scene = ui.render_shared();
        let layout = renderer.layout_with_input(&scene, Size::new(360.0, 360.0), &input);
        let at = |path: &[usize]| {
            Some(ItemRef {
                node: scene.at_path(path)?,
                layout: layout.at_path(path)?,
            })
        };
        let combo = at(&[0, 0]).ok_or("combo missing")?;
        let sibling = at(&[1]).ok_or("sibling missing")?;
        let collapsed = at(&[2]).ok_or("collapsed item missing")?;
        let local = Point { x: 5.0, y: 7.0 };
        let window = Point { x: 145.0, y: 52.0 };
        assert_eq!(combo.local_rect(), Rect::new(0.0, 0.0, 90.0, 26.0));
        assert_eq!(combo.local_to_window(local), window);
        assert_eq!(combo.window_to_local(window), Some(local));
        assert_eq!(
            combo.map_to_item(sibling, local),
            Some(Point { x: 60.5, y: -44.0 })
        );
        assert_eq!(combo.map_to_item(collapsed, local), None);

        let center = |rect: Rect| Point {
            x: rect.x + rect.width * 0.5,
            y: rect.y + rect.height * 0.5,
        };
        // Use the same layout-space geometry as the painter to locate the visible arrow.
        let geometry = ComboGeometry::for_control(
            combo.node,
            combo.layout.bounds,
            combo.layout.layout_direction,
        );
        let entry = combo.layout.map_to_window(center(geometry.entry));
        let arrow = combo.layout.map_to_window(center(geometry.trigger));
        for (press, release, opens) in [(entry, arrow, false), (arrow, arrow, true)] {
            input.pointer_move(&scene, &layout, press);
            input.pointer_press_in_scene(&scene, &layout);
            input.pointer_move(&scene, &layout, release);
            let target = input.pointer_release_target().ok_or("combo missed")?;
            assert_eq!(target.path, [0, 0]);
            assert_eq!(input.open_choice_at_pointer(&scene, &layout, target), opens);
        }
        let menu = input
            .choice_menu(&scene, &layout)
            .ok_or("choice menu missing")?;
        let anchor = combo.layout.window_bounds(combo.layout.bounds);
        assert_eq!(menu.bounds.y, anchor.y + anchor.height);
        assert_eq!(menu.rows.get(1).map(String::as_str), Some("Second"));
        input.pointer_move(&scene, &layout, center(menu.row_bounds(1)));
        assert!(input.choice_press(&scene, &layout));
        let choice = input
            .choice_release(&scene, &layout)
            .ok_or("choice not selected")?;
        assert!(ui.control_at(&scene, &choice.path, choice.action));
    }
    assert_eq!(aot.get_selected(), 1);
    assert_eq!(interpreted.get("selected")?, Value::Int(1));
    Ok(())
}

#[test]
fn optimized_ir_folds_geometry_and_keeps_dynamic_composition_order()
-> Result<(), Box<dyn std::error::Error>> {
    let compiled = xrack_compiler::compile(SOURCE, "transform2d.xr")?;
    let raw = xrack_compiler::mlir::lower(
        &xrack_compiler::parse(SOURCE).map_err(|_| "parse failed")?,
        "transform2d.xr",
    )
    .map_err(|_| "lowering failed")?;
    let calls = |text: &str, operation: &str| {
        let attr = format!("value = \"{operation}\"");
        text.lines()
            .filter(|line| line.contains("\"xr.geometry_function\"") && line.contains(&attr))
            .count()
    };
    assert_eq!(calls(&raw, "compose"), 4);
    assert_eq!(calls(&raw, "identity"), 1);
    assert_eq!(calls(&compiled.mlir, "compose"), 1);
    assert_eq!(calls(&compiled.mlir, "identity"), 0);
    assert_eq!(calls(&compiled.mlir, "rotation"), 0);
    assert_eq!(calls(&compiled.mlir, "around"), 0);
    assert_eq!(calls(&compiled.mlir, "translation"), 1);
    assert!(
        compiled
            .mlir
            .lines()
            .any(|line| line.contains("\"xr.literal\"") && line.contains("!xr.point2d"))
    );
    assert!(
        compiled
            .mlir
            .lines()
            .any(|line| line.contains("\"xr.literal\"") && line.contains("!xr.transform2d"))
    );
    let invalid = SOURCE.replace(
        "Transform2D.map-vector(root.visual, Vector2D.new(3px, 4px))",
        "Transform2D.map-vector(root.visual, Point2D.new(3px, 4px))",
    );
    assert!(xrack_compiler::compile(&invalid, "transform2d.xr").is_err());
    Ok(())
}

#[test]
fn typed_transforms_agree_in_aot_and_interpreter_including_paint_and_hit_testing()
-> Result<(), Box<dyn std::error::Error>> {
    let compiled = xrack_compiler::compile(SOURCE, "transform2d.xr")?;
    let optimized = load_mlir(&compiled.mlir)?;
    let mut interpreted = optimized.create("TransformProbe")?;
    let mut aot = TransformProbe::new();
    let raw = xrack_compiler::mlir::lower(
        &xrack_compiler::parse(SOURCE).map_err(|_| "parse failed")?,
        "transform2d.xr",
    )
    .map_err(|_| "lowering failed")?;
    let raw_module = xrack_compiler::mlir::Module::parse(&raw)?;
    let reference = Program::from_module(&raw_module)?.create("TransformProbe")?;
    for (property, expected) in [
        ("constant-point", point(3.0, 10.0)),
        ("pivot-point", point(6.0, 11.0)),
    ] {
        assert_eq!(interpreted.get(property)?, Value::Point2D(expected));
        assert_eq!(reference.get(property)?, Value::Point2D(expected));
    }
    assert_eq!(aot.get_constant_point(), point(3.0, 10.0));
    assert_eq!(aot.get_pivot_point(), point(6.0, 11.0));
    assert_eq!(aot.get_identity(), Transform2D::IDENTITY);
    assert_eq!(aot.get_empty_vector(), Vector2D::default());
    assert_eq!(
        interpreted.get("identity")?,
        Value::Transform2D(Transform2D::IDENTITY)
    );
    assert_eq!(
        interpreted.get("empty-vector")?,
        Value::Vector2D(Vector2D::default())
    );

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    for (dx, scale) in [(5.0, 1), (11.0, 2)] {
        aot.set_dx(Length(dx));
        interpreted.set("dx", Length(dx).into())?;
        let target = point(dx + 9.0, 6.0);
        assert_eq!(aot.get_mapped(), target);
        assert_eq!(interpreted.get("mapped")?, Value::Point2D(target));
        assert_eq!(aot.get_mapped_x(), target.x);
        assert_eq!(interpreted.get("mapped-x")?, Value::Length(target.x));
        assert_eq!(
            aot.get_displacement(),
            Vector2D::new(Length(3.0), Length(4.0))
        );
        assert_eq!(
            interpreted.get("displacement-y")?,
            Value::Length(Length(4.0))
        );

        let aot_scene = aot.render_shared();
        let dynamic_scene = interpreted.render_shared();
        let untouched = Interaction::default();
        let compiled =
            renderer.snapshot(&aot_scene, 90 * scale, 62 * scale, scale as f32, &untouched)?;
        let dynamic = renderer.snapshot(
            &dynamic_scene,
            90 * scale,
            62 * scale,
            scale as f32,
            &untouched,
        )?;
        assert_eq!(compiled.rgba, dynamic.rgba);
        let x = dx as f32 + 35.0;
        let item = compiled.layout.at_path(&[0]).ok_or("tile missing")?;
        assert_eq!(
            item.window_bounds(item.bounds),
            Rect::new(x, 23.0, 12.0, 20.0)
        );
        assert_eq!(
            compiled.pixel((x as u32 + 6) * scale, 32 * scale),
            Some([34, 204, 153, 255])
        );
        for (ui, scene, layout) in [
            (&mut aot as &mut dyn Component, &aot_scene, &compiled.layout),
            (
                &mut interpreted as &mut dyn Component,
                &dynamic_scene,
                &dynamic.layout,
            ),
        ] {
            let mut input = Interaction::default();
            input.pointer_move(
                scene,
                layout,
                Point {
                    x: x + 6.0,
                    y: 32.0,
                },
            );
            input.pointer_press_in_scene(scene, layout);
            let target = input
                .pointer_release_target()
                .ok_or("transformed button missed")?;
            assert_eq!(target.path, [0, 0]);
            assert!(ui.dispatch_at(scene, &target.path, "clicked"));
        }
    }
    assert_eq!(aot.get_clicks(), 2);
    assert_eq!(interpreted.get("clicks")?, Value::Int(2));
    Ok(())
}
